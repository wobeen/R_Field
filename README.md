# FieldIDE

FieldIDE는 작은 SSH 터미널에서 ROS 2 장애를 진단하고 복구하는 모바일 우선 TUI입니다.

현재 1주차 골격은 순수 reducer, UTF-8/Rope 편집 버퍼, 원자적 저장, panic에도 복원되는 터미널 guard와 80×24 단일 패널 데모를 포함합니다.

```bash
cargo run -p fieldide-tui -- --demo
cargo test --workspace
```

대화형 화면은 `cargo run -p fieldide-tui`로 열고 `q`로 종료합니다. Rust 1.85 이상이 필요하며 저장소의 `rust-toolchain.toml`은 현재 stable을 사용합니다.
