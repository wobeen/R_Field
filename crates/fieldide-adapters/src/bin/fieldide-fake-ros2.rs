use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    if let Ok(delay) = env::var("FIELDIDE_FAKE_DELAY_MS") {
        if let Ok(milliseconds) = delay.parse() {
            std::thread::sleep(Duration::from_millis(milliseconds));
        }
    }
    if env::var("FIELDIDE_FAKE_FLOOD").as_deref() == Ok("1") {
        let block = vec![b'x'; 1024 * 1024];
        let _ = io::stdout().write_all(&block);
        let _ = io::stderr().write_all(&block);
        return ExitCode::SUCCESS;
    }

    let args: Vec<String> = env::args().skip(1).collect();
    let fixture_root = env::var_os("FIELDIDE_FIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ros/jazzy")
        });
    let fixture = match args.as_slice() {
        [node, list] if node == "node" && list == "list" => fixture_root.join("node_list.txt"),
        [topic, list, typed] if topic == "topic" && list == "list" && typed == "-t" => {
            fixture_root.join("topic_list.txt")
        }
        [topic, info, verbose, name]
            if topic == "topic" && info == "info" && verbose == "--verbose" =>
        {
            fixture_root.join(format!("topic_info_{}.txt", fixture_name(name)))
        }
        _ => {
            eprintln!("unsupported fake ros2 arguments: {}", args.join(" "));
            return ExitCode::from(2);
        }
    };
    match fs::read(&fixture) {
        Ok(bytes) => {
            if io::stdout().write_all(&bytes).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("fixture read failed ({}): {error}", fixture.display());
            ExitCode::FAILURE
        }
    }
}

fn fixture_name(topic: &str) -> String {
    topic
        .trim_start_matches('/')
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}
