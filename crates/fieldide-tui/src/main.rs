use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use fieldide_core::AppState;
use fieldide_editor::TextBuffer;
use fieldide_tui::terminal::TerminalGuard;
use fieldide_tui::ui;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;
use std::io::{self, stdout};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "fieldide", about = "Mobile-first ROS 2 recovery console")]
struct Args {
    /// Render the deterministic 80×24 demo and exit.
    #[arg(long)]
    demo: bool,
    /// UTF-8 file to preview.
    path: Option<PathBuf>,
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let content = if let Some(path) = args.path.as_deref() {
        TextBuffer::open(path)?.text()
    } else {
        "FieldIDE 1주차 데모\n한글과 emoji 🚀를 안전하게 표시합니다.\n\nObserve 모드 · 80×24 단일 패널".to_owned()
    };

    if args.demo {
        return render_demo(&content);
    }
    run_interactive(&content)
}

fn render_demo(content: &str) -> io::Result<()> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| ui::render(frame, &AppState::default(), content))?;
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let line = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        println!("{line}");
    }
    Ok(())
}

fn run_interactive(content: &str) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|frame| ui::render(frame, &AppState::default(), content))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    guard.restore()?;
    terminal.show_cursor()?;
    Ok(())
}
