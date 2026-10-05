use fieldide_adapters::process::{
    CancellationToken, ProcessError, ProcessRunner, ProcessSpec, TokioProcessRunner,
};
use fieldide_adapters::ros::RosCli;
use std::path::PathBuf;
use std::time::Duration;

fn fake_ros2() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fieldide-fake-ros2"))
}

#[tokio::test]
async fn fake_ros2_drives_node_and_topic_snapshot() {
    let inspector = RosCli::new(TokioProcessRunner, fake_ros2());
    let snapshot = inspector
        .inspect(CancellationToken::new())
        .await
        .expect("fixture inspection");
    assert_eq!(snapshot.nodes.len(), 3);
    assert_eq!(snapshot.topics.len(), 3);
    assert_eq!(snapshot.warnings.len(), 1);
    let motor = snapshot
        .topics
        .iter()
        .find(|topic| topic.name == "/motor_state")
        .expect("motor topic");
    assert_eq!((motor.publishers, motor.subscribers), (1, 0));
}

#[tokio::test]
async fn runner_drains_both_streams_and_bounds_output() {
    let output = TokioProcessRunner
        .run(
            ProcessSpec {
                max_output_bytes: 4096,
                ..ProcessSpec::new(fake_ros2()).env("FIELDIDE_FAKE_FLOOD", "1")
            },
            CancellationToken::new(),
        )
        .await
        .expect("flood process");
    assert_eq!(output.stdout.len(), 4096);
    assert_eq!(output.stderr.len(), 4096);
    assert!(output.stdout_truncated);
    assert!(output.stderr_truncated);
}

#[tokio::test]
async fn runner_times_out_slow_child() {
    let result = TokioProcessRunner
        .run(
            ProcessSpec {
                timeout: Duration::from_millis(50),
                ..ProcessSpec::new(fake_ros2())
                    .args(["node", "list"])
                    .env("FIELDIDE_FAKE_DELAY_MS", "1000")
            },
            CancellationToken::new(),
        )
        .await;
    assert!(matches!(result, Err(ProcessError::TimedOut(_))));
}

#[tokio::test]
async fn runner_cancels_slow_child() {
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        trigger.cancel();
    });
    let result = TokioProcessRunner
        .run(
            ProcessSpec::new(fake_ros2())
                .args(["node", "list"])
                .env("FIELDIDE_FAKE_DELAY_MS", "1000"),
            cancellation,
        )
        .await;
    assert!(matches!(result, Err(ProcessError::Cancelled)));
}

#[tokio::test]
async fn runner_honors_cancellation_requested_before_spawn() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let result = TokioProcessRunner
        .run(
            ProcessSpec::new(fake_ros2())
                .args(["node", "list"])
                .env("FIELDIDE_FAKE_DELAY_MS", "1000"),
            cancellation,
        )
        .await;
    assert!(matches!(result, Err(ProcessError::Cancelled)));
}
