use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl ProcessSpec {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            timeout: Duration::from_secs(2),
            max_output_bytes: 256 * 1024,
        }
    }

    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl ProcessOutput {
    #[must_use]
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

#[derive(Debug)]
pub enum ProcessError {
    Io(io::Error),
    TimedOut(Duration),
    Cancelled,
    PipeTask(String),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::TimedOut(duration) => {
                write!(formatter, "프로세스가 {duration:?} 후 시간 초과했습니다")
            }
            Self::Cancelled => write!(formatter, "프로세스가 취소되었습니다"),
            Self::PipeTask(message) => write!(formatter, "출력 수집 작업 실패: {message}"),
        }
    }
}

impl std::error::Error for ProcessError {}

impl From<io::Error> for ProcessError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct CancellationToken {
    sender: watch::Sender<bool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }

    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.sender.subscribe()
    }
}

pub trait ProcessRunner: Send + Sync {
    fn run<'a>(
        &'a self,
        spec: ProcessSpec,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<ProcessOutput, ProcessError>> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TokioProcessRunner;

impl ProcessRunner for TokioProcessRunner {
    fn run<'a>(
        &'a self,
        spec: ProcessSpec,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<ProcessOutput, ProcessError>> + Send + 'a>> {
        Box::pin(async move { run_process(spec, cancellation).await })
    }
}

async fn run_process(
    spec: ProcessSpec,
    cancellation: CancellationToken,
) -> Result<ProcessOutput, ProcessError> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .envs(spec.env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProcessError::PipeTask("stdout pipe가 없습니다".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProcessError::PipeTask("stderr pipe가 없습니다".to_owned()))?;
    let stdout_task = tokio::spawn(read_limited(stdout, spec.max_output_bytes));
    let stderr_task = tokio::spawn(read_limited(stderr, spec.max_output_bytes));
    let cancelled = cancellation.subscribe();

    let wait_result = tokio::select! {
        result = child.wait() => result.map_err(ProcessError::Io),
        () = tokio::time::sleep(spec.timeout) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(ProcessError::TimedOut(spec.timeout))
        }
        () = wait_for_cancellation(cancelled) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(ProcessError::Cancelled)
        }
    };

    let stdout = join_pipe(stdout_task).await?;
    let stderr = join_pipe(stderr_task).await?;
    let status = wait_result?;
    Ok(ProcessOutput {
        exit_code: status.code(),
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

async fn wait_for_cancellation(mut receiver: watch::Receiver<bool>) {
    if *receiver.borrow() {
        return;
    }
    while receiver.changed().await.is_ok() {
        if *receiver.borrow() {
            return;
        }
    }
    std::future::pending::<()>().await;
}

struct LimitedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_limited(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> io::Result<LimitedOutput> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut chunk = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        let retained = remaining.min(count);
        bytes.extend_from_slice(&chunk[..retained]);
        truncated |= retained < count;
    }
    Ok(LimitedOutput { bytes, truncated })
}

async fn join_pipe(
    task: tokio::task::JoinHandle<io::Result<LimitedOutput>>,
) -> Result<LimitedOutput, ProcessError> {
    task.await
        .map_err(|error| ProcessError::PipeTask(error.to_string()))?
        .map_err(ProcessError::Io)
}
