# MaxonKey Constitution

## Core Principles

### I. Local-first scope
The application MUST work offline and MUST implement only the approved feature scope.
Secret handling, encryption, and persistent storage MUST follow an owner-approved
specification and decision record (Spec 002, ADR 0001). MCP requests MUST NOT be handled
before their specification is approved.

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
Storage is bundled SQLite with application-layer encryption (ADR 0001).

## Development Workflow
Specify → plan → tasks → implement → verify against acceptance criteria.
Run rustfmt, cargo check, Clippy, tests, and a locked release build before completion.

## Governance
Changes to these principles require a documented rationale and review in a dedicated branch.
Version governance changes semantically. AGENTS.md supplies shared working instructions.

**Version**: 1.1.0 | **Ratified**: 2026-10-19 | **Last Amended**: 2026-10-19

Amendment 1.1.0 rationale: the owner approved the Task 002 security architecture, so the
Task 001 prohibition on encryption and persistent vaults is replaced by a requirement to
follow the approved specification. Committed on `main` under the owner's instruction for
the initial project phase.
