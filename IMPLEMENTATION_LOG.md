# FieldIDE 구현 로그

이 문서는 `PLAN_FIELD_IDE.md`의 10주 일정을 구현 단위로 추적한다. 상태 표기는 `대기`, `진행 중`, `완료`, `차단`을 사용한다.

## 주차별 구현 단위

| 주차 | 구현 단위 | 핵심 완료 기준 | 상태 |
|---:|---|---|---|
| 1주 | 기반과 단일 패널 편집 | workspace, reducer, TerminalGuard, 80×24, Rope open/atomic save/cursor | 완료 |
| 2주 | 편집 기능과 경로 안전 | undo/redo, 검색, 여러 문서, sandbox, 선택적 강조 | 완료 |
| 3주 | 프로세스와 ROS graph | ProcessRunner, fake ros2, node/topic parser | 완료 |
| 4주 | 로그와 명시적 매핑 | bounded log, 위치 추출, node mapping TOML | 대기 |
| 5주 | 선택 빌드 | package discovery, colcon build, 실패 위치 | 대기 |
| 6주 | 안전 제어와 lifecycle | Observe/Armed/Control, audit, run/launch | 대기 |
| 7주 | 복구 판정과 MVP 통합 | node/topic 검증, rollback 안내, 복구 e2e | 대기 |
| 8주 | 모바일 UX와 선택 기능 | palette/help, compact render, 조건부 LSP/session | 대기 |
| 9주 | 실환경·보안·성능 | aarch64, Pi/Jazzy, soak, threat fixes | 대기 |
| 10주 | 사용자 검증과 릴리스 | 문서, 바이너리/checksum, 사용자 시험, v0.1 | 대기 |

## 1주차 — 기반과 단일 패널 편집

실행일: 2026-10-02

### 구현

- Rust workspace와 네 crate(`core`, `editor`, `adapters`, `tui`) 경계를 생성했다.
- side effect를 `Effect`로 반환하는 순수 Command/Event reducer와 안전 모드의 최소 전이를 구현했다.
- 진입 실패와 panic unwind에도 raw mode/alternate screen을 복원하는 `TerminalGuard`를 구현했다.
- 80×24 단일 패널과 79×23 이하 경고 화면, 비대화형 `--demo` 출력을 구현했다.
- Rope 기반 UTF-8 버퍼에 grapheme 단위 커서 이동, 파일 열기와 같은 디렉터리 임시 파일을 이용한 원자적 저장을 구현했다.
- 한글, 결합 문자, family emoji, CRLF round-trip 테스트를 추가했다.
- Ubuntu/Windows에서 fmt, clippy, test, demo를 실행하는 CI 정의를 추가했다.

### 검증

계획된 명령:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p fieldide-tui -- --demo
```

초기 환경에는 Rust 도구체인이 없어 rustup을 설치했다. 처음 고정한 Rust 1.81은 현재 의존성의 Edition 2024 메타데이터를 읽지 못했으므로, 계획서의 “개발 시작 시점 stable” 원칙에 맞춰 stable 채널과 최소 지원 버전 1.85로 조정했다.

최종 검증 결과:

- `cargo fmt --all -- --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 10개 단위 테스트 및 전체 doc-test 통과
- `cargo run -p fieldide-tui -- --demo`: 80×24 화면 출력 및 정상 종료

초기 화면 테스트는 전각 문자의 후행 셀을 공백으로 보존하는 Ratatui `TestBackend` 특성을 고려하지 않아 2건이 실패했다. 셀 폭을 정규화해 검증하도록 수정했고 전체 재실행에서 통과했다.

### 2주차로 이관

- selection, undo/redo, literal search와 여러 파일 전환
- canonicalize 기반 workspace traversal/symlink 차단
- Tree-sitter 강조(시간 초과 시 plain text 유지)

## 2주차 — 편집 기능과 경로 안전

실행일: 2026-10-05

### 구현

- `TextBuffer`에 grapheme 단위 전후 삭제, Home/End, Shift 선택, 전체 선택을 구현했다.
- 선택 교체를 포함한 undo/redo transaction과 저장본 기준 dirty 복원을 구현했다.
- 파일 끝에서 처음으로 한 번 순환하는 literal 검색을 구현했다.
- `DocumentSet`으로 세 문서 이상을 열고 Tab/Shift+Tab으로 전환하도록 했다.
- canonicalize된 workspace root를 기준으로 `..` traversal과 Unix symlink 탈출을 차단했다.
- TUI가 실제 `TextBuffer`와 `AppState`를 보유하도록 연결하고 입력·저장·검색·문서 전환을 구현했다.
- `Ctrl+Q` 또는 터미널 호환용 `Ctrl+C`로 종료하며, 미저장 문서가 있으면 두 번 눌러야 종료되도록 했다.
- Unicode 셀 너비에 맞춘 cursor 표시, selection 강조와 세로 scroll을 추가했다.
- Tree-sitter는 계획의 축소안대로 넣지 않고 plain text 편집을 유지했다.

### 검증

- `cargo fmt --all -- --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 17개 단위 테스트 및 전체 doc-test 통과
- `cargo run -p fieldide-tui -- --demo`: 80×24 화면 출력 및 정상 종료
- 100회 Unicode 편집 undo/redo 왕복, 1MiB literal 검색, 3문서 전환, traversal 차단을 자동 시험했다.

### 3주차로 이관

- 비동기 `ProcessRunner`, stdout/stderr 동시 수집, timeout/cancel
- fake `ros2` executable과 node/topic fixture parser
- ROS graph 결과를 Dashboard에 연결

## 3주차 — 프로세스와 ROS graph

실행일: 2026-10-06

### 구현

- Tokio child process 기반 `ProcessRunner`와 executable+argv 실행 규칙을 구현했다.
- stdout/stderr를 동시에 끝까지 소비하고 각 스트림을 256KiB로 제한하며 초과 여부를 기록한다.
- 명령별 timeout, 실행 전·실행 중 cancellation, child kill과 wait를 구현했다.
- `ros2 node list`, `topic list -t`, `topic info --verbose` 어댑터와 CRLF 대응 파서를 구현했다.
- 알 수 없는 출력은 panic이나 추측 대신 warning으로 보존하고 누락된 count는 0과 warning으로 표시한다.
- Jazzy node/topic fixture metadata와 독립 실행 가능한 `fieldide-fake-ros2`를 추가했다.
- F2 Dashboard에서 node, topic type, publisher/subscriber count를 표시한다.
- `r`/F5 refresh는 별도 작업 스레드에서 실행해 UI 루프를 막지 않으며 Esc로 취소한다.
- Esc/F1은 편집기로 돌아가며, 갱신 중 Esc는 취소도 요청한다. `--ros2-bin`으로 실제 또는 fake 실행 파일을 선택한다.

### 검증

- `cargo fmt --all -- --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 32개 단위·통합 테스트 및 전체 doc-test 통과
- fake `ros2` node/topic 종단 스냅샷: 통과
- stdout/stderr 각 1MiB 폭주를 4KiB로 제한하면서 deadlock 없이 종료: 통과
- 50ms timeout, 실행 중 취소, 실행 전 취소: 통과
- CRLF 및 알 수 없는 node 출력 warning 변환: 통과

OneDrive가 기존 `target`의 Windows `.exe/.pdb`를 잠근 실행에서는 linker 재기록이 실패해, 동일 소스를 OS 임시 target 디렉터리에서 전체 재검증했다.

### 4주차로 이관

- 크기가 제한된 로그 tail과 ANSI 제거
- `path:line:column` 위치 추출 및 파일 열기
- `fieldide.toml` node→package/executable/launch 명시적 매핑
