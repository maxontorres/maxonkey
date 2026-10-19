# MaxonKey Constitution

## Core Principles

### I. Local-first scope
The application MUST work offline and MUST implement only the approved feature scope.
Task 001 MUST NOT handle secrets, encryption, persistent vaults, or MCP requests.

### II. Owner authority
Future MCP plaintext reads, creation, updates, and deletion MUST require explicit owner approval.
No AI agent may bypass that boundary.

### III. Evidence before completion
Features MUST have a short specification, plan, acceptance criteria, and automated tests.
Only checks that actually ran successfully may be reported as passed.

### IV. Current dependency APIs
Developers MUST inspect resolved versions and matching official documentation before using APIs.
Commit Cargo.lock, use stable Rust, and justify dependency changes.

### V. Accessible simplicity
Keyboard access and textual state labels MUST accompany visual styling.
Prefer a small binary with direct modules over speculative abstractions.

## Technical Constraints
Rust; Windows first, Linux and macOS later. clap CLI, Ratatui/Crossterm TUI.
SQLite is provisional, not an implemented storage dependency.

## Development Workflow
Specify → plan → tasks → implement → verify against acceptance criteria.
Run rustfmt, cargo check, Clippy, tests, and a locked release build before completion.

## Governance
Changes to these principles require a documented rationale and review in a dedicated branch.
Version governance changes semantically. AGENTS.md supplies shared working instructions.

**Version**: 1.0.0 | **Ratified**: 2026-10-19 | **Last Amended**: 2026-10-19
