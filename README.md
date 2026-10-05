# FieldIDE
# make new line in terminal
FieldIDE는 작은 SSH 터미널에서 ROS 2 장애를 진단하고 복구하는 모바일 우선 TUI입니다.

현재 3주차 PoC는 UTF-8/Rope 편집기와 workspace sandbox에 더해 비동기 프로세스 실행기, timeout·취소·출력 제한, ROS 2 node/topic 파서, fixture 기반 fake `ros2`와 Dashboard를 포함합니다.
```bash
cargo run -p fieldide-tui -- --demo
cargo run -p fieldide-tui -- README.md PLAN_FIELD_IDE.md Cargo.toml
cargo test --workspace
```

ROS 2 환경에서는 `F2`로 Dashboard를 열고 `r` 또는 `F5`로 graph를 갱신합니다. `Esc` 또는 `F1`로 편집기에 돌아가며, 갱신 중 `Esc`는 실행도 취소합니다. 기본 실행 파일은 `ros2`이며 다른 경로는 `--ros2-bin <경로>`로 지정합니다.

ROS 설치 없이 fixture Dashboard를 시험하려면 먼저 fake 실행 파일을 빌드합니다.

```powershell
cargo build -p fieldide-adapters --bin fieldide-fake-ros2
cargo run -p fieldide-tui -- --ros2-bin target/debug/fieldide-fake-ros2.exe README.md
```

Linux에서는 마지막 경로의 `.exe`를 제외합니다.

대화형 화면은 `cargo run -p fieldide-tui -- <파일...>`로 엽니다. 파일 접근은 현재 디렉터리 내부로 제한되며 `--workspace <경로>`로 root를 명시할 수 있습니다.

- 일반 입력 및 Enter, Backspace, Delete: 편집
- 방향키/Home/End, Shift 조합: 이동 및 선택
- `Ctrl+S`: 원자적 저장
- `Ctrl+Z` / `Ctrl+Y`: undo / redo
- `Ctrl+F`: literal 검색
- `Tab` / `Shift+Tab`: 문서 전환
- `Ctrl+Q` 또는 `Ctrl+C`: 종료(미저장 문서가 있으면 한 번 더 확인)

Rust 1.85 이상이 필요하며 저장소의 `rust-toolchain.toml`은 현재 stable을 사용합니다. Tree-sitter 강조는 2주차 축소안에 따라 plain text로 유지하고 이후 단계로 이관했습니다.
