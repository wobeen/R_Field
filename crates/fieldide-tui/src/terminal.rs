use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use std::io::{self, stdout};

pub trait TerminalOps {
    fn enter(&mut self) -> io::Result<()>;
    fn restore(&mut self) -> io::Result<()>;
}

#[derive(Debug, Default)]
pub struct CrosstermOps;

impl TerminalOps for CrosstermOps {
    fn enter(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        if let Err(error) = execute!(stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(())
    }

    fn restore(&mut self) -> io::Result<()> {
        let screen_result = execute!(stdout(), LeaveAlternateScreen);
        let raw_result = disable_raw_mode();
        screen_result.and(raw_result)
    }
}

/// Restores raw mode and the alternate screen even while unwinding a panic.
pub struct TerminalGuard<T: TerminalOps = CrosstermOps> {
    ops: T,
    active: bool,
}

impl TerminalGuard<CrosstermOps> {
    pub fn enter() -> io::Result<Self> {
        Self::with_ops(CrosstermOps)
    }
}

impl<T: TerminalOps> TerminalGuard<T> {
    pub fn with_ops(mut ops: T) -> io::Result<Self> {
        ops.enter()?;
        Ok(Self { ops, active: true })
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            self.ops.restore()
        } else {
            Ok(())
        }
    }
}

impl<T: TerminalOps> Drop for TerminalGuard<T> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct FakeOps(Arc<Mutex<Vec<&'static str>>>);

    impl TerminalOps for FakeOps {
        fn enter(&mut self) -> io::Result<()> {
            self.0.lock().expect("events lock").push("enter");
            Ok(())
        }

        fn restore(&mut self) -> io::Result<()> {
            self.0.lock().expect("events lock").push("restore");
            Ok(())
        }
    }

    #[test]
    fn drop_restores_terminal_during_panic() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let panic_events = Arc::clone(&events);
        let result = std::panic::catch_unwind(move || {
            let _guard = TerminalGuard::with_ops(FakeOps(panic_events)).expect("enter");
            panic!("forced panic");
        });

        assert!(result.is_err());
        assert_eq!(*events.lock().expect("events lock"), ["enter", "restore"]);
    }

    #[test]
    fn explicit_restore_is_idempotent() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut guard = TerminalGuard::with_ops(FakeOps(Arc::clone(&events))).expect("enter");
        guard.restore().expect("restore");
        guard.restore().expect("second restore");
        drop(guard);
        assert_eq!(*events.lock().expect("events lock"), ["enter", "restore"]);
    }
}
