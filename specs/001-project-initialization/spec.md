# Feature Specification: Task 001 — Project Initialization

**Feature Branch**: `001-project-initialization`
**Created**: 2026-10-19
**Status**: Accepted for implementation
**Input**: Establish MaxonKey 0.1.0 with CLI, accessible TUI, Spec Kit, tests, and CI.

## User Scenarios & Testing

### User Story 1 — Inspect the prototype (Priority: P1)
A developer can discover commands, identify the version and vendor, and see honest capability status.
**Independent Test**: Execute each supported CLI command without a terminal.
**Acceptance Scenarios**:
1. Given a built binary, `help` and `--help` succeed and list commands.
2. `version` and `--version` print `maxonkey 0.1.0`; `about` identifies Maxon Torres Software.
3. `status` reports vault, encryption, storage, secret CRUD, and MCP as unimplemented.
4. Unknown commands fail with a useful error and do not launch the TUI.

### User Story 2 — Navigate the console (Priority: P1)
A user launches the full-screen TUI, visits Dashboard, Help, and About, then exits cleanly.
**Independent Test**: Render with a test backend, inject keys, and verify real terminal exit separately.
**Acceptance Scenarios**:
1. No command in an interactive terminal opens Dashboard.
2. Tab/Right/Down and Shift+Tab/Left/Up cycle screens; 1/2/3 select directly; ? opens Help.
3. q, Esc, and Ctrl+C exit, restoring the terminal on normal exit and handled errors.
4. Resizing preserves navigation; tiny terminals show an explicit resize notice and exit hints.
5. Selection uses labels, a `>` marker, and a distinct border. No animation is present.
6. Dashboard accurately labels unavailable features and contains no fictional audit records.

### Edge Cases
- Redirected stdin/stdout: no-command invocation fails with CLI guidance before terminal setup.
- Release/repeat key events do not cause duplicate navigation.
- Event/draw errors propagate after terminal restoration; panic cleanup uses Ratatui's hook.
- Zero-sized or very small rendering areas do not panic.

## Requirements

### Functional Requirements
- **FR-001**: Ship one Rust binary named `maxonkey`, version 0.1.0.
- **FR-002**: Support all seven requested CLI command/flag forms plus default TUI launch.
- **FR-003**: Use the exact Millennium Blue palette, readable text, explicit state symbols/labels,
  visible keyboard hints, responsive layouts, and no motion by default.
- **FR-004**: Implement Dashboard, Help, About, keyboard navigation, and graceful exit.
- **FR-005**: Do not implement cryptography, storage, secret operations, or MCP.
- **FR-006**: Include shared AI rules, minimal requested docs, Spec Kit artifacts, stable toolchain,
  committed lockfile, automated CLI/TUI tests, and Windows/Linux CI.

## Success Criteria
- **SC-001**: All CLI scenarios pass automated tests.
- **SC-002**: Navigation, exit events, small layouts, and truthful screen content pass automated tests.
- **SC-003**: Local fmt, check, Clippy, tests, and release build succeed; CI runs the requested checks
  on Windows and Linux. Distinguish configured CI from observed remote results.
- **SC-004**: No application data files are created and no unavailable capability is presented as working.

## Assumptions
- Interactive TUI requires a terminal; Windows Terminal with true color is recommended.
- No animation means reduced-motion needs are satisfied in this prototype; future animations must be opt-in.
- Installation and security decisions are documented, not implemented in Task 001.
