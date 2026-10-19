# Tasks: Task 001

## Phase 1: Setup
- [x] T001 Inspect empty connected repository, create dedicated branch, install official Codex Spec Kit assets in `.specify/` and `.agents/skills/`.
- [x] T002 Establish constitution and accepted scope in `.specify/memory/constitution.md` and `specs/001-project-initialization/spec.md`.
- [x] T003 Create `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, and commit resolved `Cargo.lock`.

## Phase 2: Foundation
- [x] T004 Record shared instructions and requested product, security, UI, and architecture documentation in `AGENTS.md`, `CLAUDE.md`, `README.md`, and `docs/`.

## Phase 3: US1 — CLI (P1)
- [x] T005 [US1] Write CLI parsing and executable output/error tests in `src/cli.rs` and `tests/cli.rs`.
- [x] T006 [US1] Implement parser, informational content, and dispatch in `src/cli.rs`, `src/info.rs`, and `src/main.rs`.

## Phase 4: US2 — TUI (P1)
- [x] T007 [US2] Write navigation, exit, truthful content, and responsive layout tests in `src/tui.rs`.
- [x] T008 [US2] Implement screen state, rendering, and terminal lifecycle in `src/tui.rs`.
- [x] T009 [US2] Add real terminal exit/restoration coverage in `tests/tui_pty.py`.

## Phase 5: Verification
- [x] T010 Configure Ubuntu/Windows required checks in `.github/workflows/ci.yml`.
- [x] T011 Run fmt, check, Clippy, tests, release build, and available PTY checks; record evidence in this file.
- [x] T012 Review against spec/plan, commit and push feature branch; prepare PR if a base branch exists.

## Dependencies and delivery
Execute phases in order. US2 uses US1 informational text. Tests precede corresponding behavior.
No parallel agent work is needed. MVP is US1; delivery includes both US1 and US2.

## Local verification evidence — 2026-10-19
- Rust stable 1.99.1, x86_64-unknown-linux-gnu.
- Initial empty-binary run: six CLI acceptance tests failed as expected before implementation.
- Final `cargo fmt --all -- --check`: passed.
- Final `cargo check --locked`: passed.
- Final `cargo clippy --locked --all-targets -- -D warnings`: passed.
- Final `cargo test --locked`: passed, 8 unit tests + 6 executable integration tests.
- Final `cargo build --release --locked`: passed.
- `python3 tests/tui_pty.py target/release/maxonkey`: passed for q, Esc, and Ctrl+C;
  verifies mode/cursor/alternate-screen restoration, navigation, and resize.
- Spec Kit prerequisite resolution: passed for spec, plan, and tasks.
- Review: all six FRs, four SCs, ten acceptance scenarios, and five constitution
  principles checked; no remaining implementation gaps in Task 001.
- Remote Windows and Ubuntu jobs passed all Rust checks and release builds for implementation
  commit `4764bb9`; Ubuntu additionally passed the PTY test. macOS remains unverified.
  Evidence: https://github.com/maxontorres/maxonkey/actions/runs/39264078625
- Feature branch published. PR cannot be opened because the repository had no main/base
  commit and the only published branch is this feature branch (GitHub selected it as default).
  No main branch was created or pushed. PR title/body prepared separately in the workspace.
- Final follow-up commit changes only this completion record; implementation is unchanged.
