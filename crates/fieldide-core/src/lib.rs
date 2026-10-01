//! Pure application state and reducer for FieldIDE.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyMode {
    Observe,
    Armed,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Dashboard,
    Editor,
    Logs,
    Build,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub mode: SafetyMode,
    pub panel: Panel,
    pub active_file: Option<PathBuf>,
    pub dirty: bool,
    pub notice: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: SafetyMode::Observe,
            panel: Panel::Dashboard,
            active_file: None,
            dirty: false,
            notice: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ShowPanel(Panel),
    Open(PathBuf),
    Edit,
    Save,
    ArmControl,
    ConfirmControl,
    ReturnToObserve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    LoadFile(PathBuf),
    SaveFile(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    FileLoaded(PathBuf),
    FileSaved,
    Failed(String),
}

/// Applies a user command and returns side effects for the outer runtime.
pub fn reduce_command(state: &mut AppState, command: Command) -> Vec<Effect> {
    state.notice = None;
    match command {
        Command::ShowPanel(panel) => state.panel = panel,
        Command::Open(path) => return vec![Effect::LoadFile(path)],
        Command::Edit => {
            if state.active_file.is_some() {
                state.dirty = true;
            } else {
                state.notice = Some("열린 파일이 없습니다".to_owned());
            }
        }
        Command::Save => {
            if let Some(path) = &state.active_file {
                return vec![Effect::SaveFile(path.clone())];
            }
            state.notice = Some("저장할 파일이 없습니다".to_owned());
        }
        Command::ArmControl if state.mode == SafetyMode::Observe => {
            state.mode = SafetyMode::Armed;
        }
        Command::ConfirmControl if state.mode == SafetyMode::Armed => {
            state.mode = SafetyMode::Control;
        }
        Command::ReturnToObserve => state.mode = SafetyMode::Observe,
        Command::ArmControl | Command::ConfirmControl => {
            state.notice = Some("허용되지 않은 안전 상태 전이입니다".to_owned());
        }
    }
    Vec::new()
}

/// Folds an asynchronous result back into application state.
pub fn reduce_event(state: &mut AppState, event: AppEvent) {
    match event {
        AppEvent::FileLoaded(path) => {
            state.active_file = Some(path);
            state.panel = Panel::Editor;
            state.dirty = false;
            state.notice = None;
        }
        AppEvent::FileSaved => {
            state.dirty = false;
            state.notice = Some("저장했습니다".to_owned());
        }
        AppEvent::Failed(message) => state.notice = Some(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_is_an_effect_then_file_loaded_updates_state() {
        let path = PathBuf::from("src/main.rs");
        let mut state = AppState::default();

        assert_eq!(
            reduce_command(&mut state, Command::Open(path.clone())),
            vec![Effect::LoadFile(path.clone())]
        );
        assert_eq!(state.panel, Panel::Dashboard);

        reduce_event(&mut state, AppEvent::FileLoaded(path.clone()));
        assert_eq!(state.active_file, Some(path));
        assert_eq!(state.panel, Panel::Editor);
    }

    #[test]
    fn control_requires_arming() {
        let mut state = AppState::default();

        reduce_command(&mut state, Command::ConfirmControl);
        assert_eq!(state.mode, SafetyMode::Observe);
        assert!(state.notice.is_some());

        reduce_command(&mut state, Command::ArmControl);
        reduce_command(&mut state, Command::ConfirmControl);
        assert_eq!(state.mode, SafetyMode::Control);

        reduce_command(&mut state, Command::ReturnToObserve);
        assert_eq!(state.mode, SafetyMode::Observe);
    }

    #[test]
    fn editing_and_saving_follow_events() {
        let mut state = AppState::default();
        let path = PathBuf::from("한글.md");
        reduce_event(&mut state, AppEvent::FileLoaded(path.clone()));

        reduce_command(&mut state, Command::Edit);
        assert!(state.dirty);
        assert_eq!(
            reduce_command(&mut state, Command::Save),
            vec![Effect::SaveFile(path)]
        );

        reduce_event(&mut state, AppEvent::FileSaved);
        assert!(!state.dirty);
    }
}
