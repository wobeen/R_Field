use crate::process::{CancellationToken, ProcessError, ProcessRunner, ProcessSpec};
use fieldide_core::{RosNode, RosSnapshot, RosTopic};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug)]
pub enum RosError {
    Process(ProcessError),
    CommandFailed { args: Vec<String>, stderr: String },
    InvalidUtf8(String),
    OutputTruncated(String),
}

impl fmt::Display for RosError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(error) => error.fmt(formatter),
            Self::CommandFailed { args, stderr } => {
                write!(formatter, "ros2 {} 실패: {stderr}", args.join(" "))
            }
            Self::InvalidUtf8(command) => {
                write!(formatter, "ros2 {command} 출력이 UTF-8이 아닙니다")
            }
            Self::OutputTruncated(command) => {
                write!(formatter, "ros2 {command} 출력이 제한을 초과했습니다")
            }
        }
    }
}

impl std::error::Error for RosError {}

impl From<ProcessError> for RosError {
    fn from(error: ProcessError) -> Self {
        Self::Process(error)
    }
}

pub struct RosCli<R> {
    runner: R,
    program: PathBuf,
    timeout: Duration,
}

impl<R> RosCli<R>
where
    R: ProcessRunner,
{
    #[must_use]
    pub fn new(runner: R, program: impl Into<PathBuf>) -> Self {
        Self {
            runner,
            program: program.into(),
            timeout: Duration::from_secs(2),
        }
    }

    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn inspect(&self, cancellation: CancellationToken) -> Result<RosSnapshot, RosError> {
        let mut warnings = Vec::new();
        let node_text = self.run(&["node", "list"], cancellation.clone()).await?;
        let (nodes, node_warnings) = parse_node_list(&node_text);
        warnings.extend(node_warnings);

        let topic_text = self
            .run(&["topic", "list", "-t"], cancellation.clone())
            .await?;
        let (summaries, topic_warnings) = parse_topic_list(&topic_text);
        warnings.extend(topic_warnings);
        let mut topics = Vec::with_capacity(summaries.len());
        for (name, types) in summaries {
            let info = self
                .run(
                    &["topic", "info", "--verbose", name.as_str()],
                    cancellation.clone(),
                )
                .await?;
            let (publishers, subscribers, info_warnings) = parse_topic_info(&name, &info);
            warnings.extend(info_warnings);
            topics.push(RosTopic {
                name,
                types,
                publishers,
                subscribers,
            });
        }
        Ok(RosSnapshot {
            nodes,
            topics,
            warnings,
        })
    }

    async fn run(
        &self,
        args: &[&str],
        cancellation: CancellationToken,
    ) -> Result<String, RosError> {
        let output = self
            .runner
            .run(
                ProcessSpec {
                    args: args.iter().map(Into::into).collect(),
                    timeout: self.timeout,
                    ..ProcessSpec::new(&self.program)
                },
                cancellation,
            )
            .await?;
        if !output.success() {
            return Err(RosError::CommandFailed {
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        if output.stdout_truncated || output.stderr_truncated {
            return Err(RosError::OutputTruncated(args.join(" ")));
        }
        String::from_utf8(output.stdout).map_err(|_| RosError::InvalidUtf8(args.join(" ")))
    }
}

#[must_use]
pub fn parse_node_list(input: &str) -> (Vec<RosNode>, Vec<String>) {
    let mut nodes = Vec::new();
    let mut warnings = Vec::new();
    for line in input.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line.starts_with('/') && !line.chars().any(char::is_whitespace) {
            nodes.push(RosNode {
                name: line.to_owned(),
            });
        } else {
            warnings.push(format!("알 수 없는 node list 행: {line}"));
        }
    }
    nodes.sort_by(|left, right| left.name.cmp(&right.name));
    nodes.dedup_by(|left, right| left.name == right.name);
    (nodes, warnings)
}

#[must_use]
pub fn parse_topic_list(input: &str) -> (Vec<(String, Vec<String>)>, Vec<String>) {
    let mut topics = Vec::new();
    let mut warnings = Vec::new();
    for line in input.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((name, rest)) = line.split_once(char::is_whitespace) else {
            if line.starts_with('/') {
                topics.push((line.to_owned(), Vec::new()));
            } else {
                warnings.push(format!("알 수 없는 topic list 행: {line}"));
            }
            continue;
        };
        if !name.starts_with('/') {
            warnings.push(format!("알 수 없는 topic list 행: {line}"));
            continue;
        }
        let types: Vec<String> = rest
            .trim()
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if types.is_empty() {
            warnings.push(format!("topic type을 해석하지 못함: {line}"));
        }
        topics.push((name.to_owned(), types));
    }
    topics.sort_by(|left, right| left.0.cmp(&right.0));
    topics.dedup_by(|left, right| left.0 == right.0);
    (topics, warnings)
}

#[must_use]
pub fn parse_topic_info(topic: &str, input: &str) -> (u32, u32, Vec<String>) {
    let mut publishers = None;
    let mut subscribers = None;
    let mut warnings = Vec::new();
    for line in input.lines().map(str::trim) {
        let normalized = line.to_ascii_lowercase();
        if let Some(value) = normalized.strip_prefix("publisher count:") {
            publishers = value.trim().parse().ok();
            if publishers.is_none() {
                warnings.push(format!("{topic} publisher count를 해석하지 못함: {line}"));
            }
        } else if let Some(value) = normalized
            .strip_prefix("subscription count:")
            .or_else(|| normalized.strip_prefix("subscriber count:"))
        {
            subscribers = value.trim().parse().ok();
            if subscribers.is_none() {
                warnings.push(format!(
                    "{topic} subscription count를 해석하지 못함: {line}"
                ));
            }
        }
    }
    if publishers.is_none() {
        warnings.push(format!("{topic} publisher count가 없습니다"));
    }
    if subscribers.is_none() {
        warnings.push(format!("{topic} subscription count가 없습니다"));
    }
    (publishers.unwrap_or(0), subscribers.unwrap_or(0), warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers_accept_crlf_and_preserve_unknown_as_warning() {
        let (nodes, warnings) = parse_node_list("/camera\r\nunknown output\r\n/motor\r\n");
        assert_eq!(nodes.len(), 2);
        assert_eq!(warnings.len(), 1);

        let (topics, warnings) = parse_topic_list(
            "/cmd_vel [geometry_msgs/msg/Twist]\r\n/status [std_msgs/msg/String]\r\n",
        );
        assert_eq!(topics.len(), 2);
        assert!(warnings.is_empty());

        let (publishers, subscribers, warnings) = parse_topic_info(
            "/cmd_vel",
            "Type: geometry_msgs/msg/Twist\r\nPublisher count: 1\r\nSubscription count: 2\r\n",
        );
        assert_eq!((publishers, subscribers), (1, 2));
        assert!(warnings.is_empty());
    }

    #[test]
    fn missing_counts_become_zero_with_warnings() {
        let (publishers, subscribers, warnings) = parse_topic_info("/unknown", "new format");
        assert_eq!((publishers, subscribers), (0, 0));
        assert_eq!(warnings.len(), 2);
    }
}
