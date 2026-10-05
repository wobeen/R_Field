use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use fieldide_adapters::process::{CancellationToken, TokioProcessRunner};
use fieldide_adapters::ros::RosCli;
use fieldide_core::{
    reduce_command, reduce_event, AppEvent, AppState, Command, Effect, Panel, RosNode, RosSnapshot,
    RosTopic,
};
use fieldide_editor::{DocumentSet, Workspace};
use fieldide_tui::terminal::TerminalGuard;
use fieldide_tui::ui;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;
use std::io::{self, stdout};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "fieldide", about = "Mobile-first ROS 2 recovery console")]
struct Args {
    /// Render the deterministic 80×24 demo and exit.
    #[arg(long)]
    demo: bool,
    /// Root allowed for file access. Defaults to the current directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// ros2 executable or the fieldide fake adapter.
    #[arg(long, default_value = "ros2")]
    ros2_bin: PathBuf,
    /// UTF-8 files to edit. Use Tab and Shift+Tab to switch between them.
    paths: Vec<PathBuf>,
}

struct EditorApp {
    state: AppState,
    documents: DocumentSet,
    search: Option<String>,
    transient_status: Option<String>,
    quit_armed: bool,
    ros2_program: PathBuf,
    background_sender: Sender<AppEvent>,
    background_receiver: Receiver<AppEvent>,
    ros_cancellation: Option<CancellationToken>,
}

impl EditorApp {
    fn new(documents: DocumentSet, ros2_program: PathBuf) -> Self {
        let (background_sender, background_receiver) = mpsc::channel();
        let mut app = Self {
            state: AppState::default(),
            documents,
            search: None,
            transient_status: None,
            quit_armed: false,
            ros2_program,
            background_sender,
            background_receiver,
            ros_cancellation: None,
        };
        app.state.panel = Panel::Editor;
        app.sync_state();
        app
    }

    fn sync_state(&mut self) {
        if let Some(document) = self.documents.active() {
            self.state.active_file = document.path.clone();
            self.state.dirty = document.buffer.is_dirty();
        }
    }

    fn status(&self) -> Option<String> {
        self.search.as_ref().map_or_else(
            || self.transient_status.clone(),
            |query| Some(format!("검색: {query}█  · Enter 다음 찾기 · Esc 취소")),
        )
    }

    fn title(&self) -> String {
        let active = self.documents.active();
        let name = active
            .and_then(|document| document.path.as_deref())
            .and_then(|path| path.file_name())
            .map_or_else(
                || "[새 문서]".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
        format!(
            "Editor · {name} · {}/{}",
            self.active_number(),
            self.documents.len()
        )
    }

    fn active_number(&self) -> usize {
        if self.documents.is_empty() {
            0
        } else {
            self.documents.active_index() + 1
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        if !is_key_press(key) {
            return Ok(false);
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let quit = matches!(key.code, KeyCode::Char('\u{3}' | '\u{11}'))
            || (control && matches!(key.code, KeyCode::Char('c' | 'C' | 'q' | 'Q')));

        // Handle quit before search mode so the emergency exit always works.
        if quit {
            if self.documents.has_dirty_documents() && !self.quit_armed {
                self.quit_armed = true;
                self.transient_status = Some(
                    "저장하지 않은 문서가 있습니다 · Ctrl+Q 또는 Ctrl+C를 한 번 더 누르세요"
                        .to_owned(),
                );
                return Ok(false);
            }
            if let Some(cancellation) = &self.ros_cancellation {
                cancellation.cancel();
            }
            return Ok(true);
        }
        if self.search.is_some() {
            return Ok(self.handle_search_key(key));
        }
        self.transient_status = None;
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        self.quit_armed = false;
        if key.code == KeyCode::F(1) {
            self.state.panel = Panel::Editor;
            return Ok(false);
        }
        if key.code == KeyCode::F(2) {
            self.state.panel = Panel::Dashboard;
            return Ok(false);
        }
        if key.code == KeyCode::F(5)
            || (self.state.panel == Panel::Dashboard && !control && key.code == KeyCode::Char('r'))
        {
            self.start_ros_refresh();
            return Ok(false);
        }
        if self.state.panel == Panel::Dashboard {
            if key.code == KeyCode::Esc {
                if let Some(cancellation) = &self.ros_cancellation {
                    cancellation.cancel();
                    self.transient_status = Some("ROS graph 갱신 취소를 요청했습니다".to_owned());
                }
                self.state.panel = Panel::Editor;
            }
            return Ok(false);
        }
        if control && key.code == KeyCode::Char('s') {
            self.save_active()?;
            return Ok(false);
        }
        if control && key.code == KeyCode::Char('f') {
            self.search = Some(String::new());
            return Ok(false);
        }

        if !control && key.code == KeyCode::Tab {
            if shift {
                self.documents.previous();
            } else {
                self.documents.next();
            }
            self.sync_state();
            return Ok(false);
        }

        let Some(buffer) = self
            .documents
            .active_mut()
            .map(|document| &mut document.buffer)
        else {
            return Ok(false);
        };
        match (control, key.code) {
            (true, KeyCode::Char('z')) => {
                if !buffer.undo() {
                    self.transient_status = Some("되돌릴 변경이 없습니다".to_owned());
                }
            }
            (true, KeyCode::Char('y')) => {
                if !buffer.redo() {
                    self.transient_status = Some("다시 실행할 변경이 없습니다".to_owned());
                }
            }
            (true, KeyCode::Char('a')) => buffer.select_all(),
            (false, KeyCode::Left) => buffer.move_left_with_selection(shift),
            (false, KeyCode::Right) => buffer.move_right_with_selection(shift),
            (false, KeyCode::Up) => buffer.move_up_with_selection(shift),
            (false, KeyCode::Down) => buffer.move_down_with_selection(shift),
            (false, KeyCode::Home) => buffer.move_home_with_selection(shift),
            (false, KeyCode::End) => buffer.move_end_with_selection(shift),
            (false, KeyCode::Backspace) => buffer.backspace(),
            (false, KeyCode::Delete) => buffer.delete_forward(),
            (false, KeyCode::Enter) => buffer.insert("\n"),
            (false, KeyCode::Char(character)) => buffer.insert(&character.to_string()),
            _ => {}
        }
        self.sync_state();
        Ok(false)
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.search = None;
                self.transient_status = Some("검색을 취소했습니다".to_owned());
            }
            KeyCode::Backspace => {
                self.search.as_mut().expect("search mode").pop();
            }
            KeyCode::Enter => {
                let query = self.search.as_deref().unwrap_or_default().to_owned();
                if query.is_empty() {
                    self.transient_status = Some("검색어를 입력하세요".to_owned());
                } else if self
                    .documents
                    .active_mut()
                    .is_some_and(|document| document.buffer.find_next(&query).is_some())
                {
                    self.transient_status = Some(format!("찾음: {query}"));
                } else {
                    self.transient_status = Some(format!("찾을 수 없음: {query}"));
                }
                self.search = None;
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search.as_mut().expect("search mode").push(character);
            }
            _ => {}
        }
        false
    }

    fn save_active(&mut self) -> io::Result<()> {
        let Some(document) = self.documents.active_mut() else {
            return Ok(());
        };
        let Some(path) = document.path.clone() else {
            self.transient_status = Some("새 문서는 아직 저장 경로가 없습니다".to_owned());
            return Ok(());
        };
        match document.buffer.save_atomic(&path) {
            Ok(()) => {
                reduce_event(&mut self.state, AppEvent::FileSaved);
                self.transient_status = Some(format!("저장했습니다: {}", path.display()));
            }
            Err(error) => {
                self.transient_status = Some(format!("저장 실패: {error}"));
            }
        }
        self.sync_state();
        Ok(())
    }

    fn start_ros_refresh(&mut self) {
        let effects = reduce_command(&mut self.state, Command::RefreshRos);
        if !effects.contains(&Effect::RefreshRos) {
            return;
        }
        reduce_event(&mut self.state, AppEvent::RosRefreshStarted);
        let cancellation = CancellationToken::new();
        self.ros_cancellation = Some(cancellation.clone());
        let sender = self.background_sender.clone();
        let program = self.ros2_program.clone();
        thread::spawn(move || {
            let event = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    let inspector = RosCli::new(TokioProcessRunner, program);
                    match runtime.block_on(inspector.inspect(cancellation)) {
                        Ok(snapshot) => AppEvent::RosSnapshotLoaded(snapshot),
                        Err(error) => AppEvent::Failed(format!("ROS graph 갱신 실패: {error}")),
                    }
                }
                Err(error) => AppEvent::Failed(format!("비동기 실행기 생성 실패: {error}")),
            };
            let _ = sender.send(event);
        });
    }

    fn poll_background(&mut self) {
        while let Ok(event) = self.background_receiver.try_recv() {
            reduce_event(&mut self.state, event);
            self.ros_cancellation = None;
            self.transient_status = None;
        }
    }
}

fn is_key_press(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    if args.demo {
        return render_demo();
    }

    let root = args.workspace.unwrap_or(std::env::current_dir()?);
    let workspace = Workspace::new(&root).map_err(io::Error::other)?;
    let mut documents = if args.paths.is_empty() {
        DocumentSet::with_untitled(
            "FieldIDE 2주차 편집기\n\n파일을 지정해 열거나 여기에서 입력할 수 있습니다.\n",
        )
    } else {
        DocumentSet::default()
    };
    for path in args.paths {
        documents
            .open(&workspace, &path)
            .map_err(io::Error::other)?;
    }
    run_interactive(EditorApp::new(documents, args.ros2_bin))
}

fn render_demo() -> io::Result<()> {
    let state = AppState {
        panel: Panel::Dashboard,
        ros: RosSnapshot {
            nodes: vec![
                RosNode {
                    name: "/motor_controller".to_owned(),
                },
                RosNode {
                    name: "/robot_state_publisher".to_owned(),
                },
            ],
            topics: vec![RosTopic {
                name: "/cmd_vel".to_owned(),
                types: vec!["geometry_msgs/msg/Twist".to_owned()],
                publishers: 1,
                subscribers: 1,
            }],
            warnings: Vec::new(),
        },
        notice: Some("FieldIDE 3주차 fixture Dashboard".to_owned()),
        ..AppState::default()
    };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| ui::render_dashboard(frame, &state, None))?;
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let line = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        println!("{line}");
    }
    Ok(())
}

fn run_interactive(mut app: EditorApp) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    loop {
        app.poll_background();
        let title = app.title();
        let status = app.status();
        if app.state.panel == Panel::Dashboard {
            terminal.draw(|frame| ui::render_dashboard(frame, &app.state, status.as_deref()))?;
        } else {
            let document = app.documents.active().expect("at least one document");
            terminal.draw(|frame| {
                ui::render_editor(
                    frame,
                    &app.state,
                    &document.buffer,
                    &title,
                    status.as_deref(),
                );
            })?;
        }
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_key(key)? {
                    break;
                }
            }
        }
    }

    guard.restore()?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> EditorApp {
        EditorApp::new(DocumentSet::with_untitled(""), PathBuf::from("ros2"))
    }

    fn ctrl_q() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_q_exits_clean_editor() {
        assert!(app().handle_key(ctrl_q()).expect("key handling"));
    }

    #[test]
    fn ctrl_uppercase_q_exits() {
        let key = KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::CONTROL);
        assert!(app().handle_key(key).expect("key handling"));
    }

    #[test]
    fn ctrl_q_control_character_exits() {
        let key = KeyEvent::new(KeyCode::Char('\u{11}'), KeyModifiers::NONE);
        assert!(app().handle_key(key).expect("key handling"));
    }

    #[test]
    fn ctrl_c_exits_when_terminal_reserves_ctrl_q() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app().handle_key(key).expect("key handling"));

        let raw_key = KeyEvent::new(KeyCode::Char('\u{3}'), KeyModifiers::NONE);
        assert!(app().handle_key(raw_key).expect("raw key handling"));
    }

    #[test]
    fn ctrl_q_exits_even_while_search_is_active() {
        let mut app = app();
        app.search = Some("query".to_owned());
        assert!(app.handle_key(ctrl_q()).expect("key handling"));
    }

    #[test]
    fn dirty_document_requires_second_ctrl_q() {
        let mut app = app();
        app.documents
            .active_mut()
            .expect("document")
            .buffer
            .insert("changed");
        assert!(!app.handle_key(ctrl_q()).expect("first quit"));
        let repeat = KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        );
        assert!(app.handle_key(repeat).expect("confirmed quit"));
    }

    #[test]
    fn escape_returns_from_dashboard_to_editor() {
        let mut app = app();
        app.state.panel = Panel::Dashboard;
        let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.handle_key(escape).expect("escape handling"));
        assert_eq!(app.state.panel, Panel::Editor);
    }
}
