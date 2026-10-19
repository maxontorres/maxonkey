# Implementation Plan: Task 001

**Branch**: `001-project-initialization` | **Date**: 2026-10-19 | **Spec**: [spec.md](spec.md)

## Summary
Build a small synchronous Rust binary. clap dispatches informational commands;
no subcommand starts an event-driven Ratatui application using Crossterm.

## Technical Context
- Rust stable, edition 2024; installed stable is 1.99.1. No separate MSRV promise yet.
- Direct dependencies: clap 4.6.7 (derive), Ratatui 0.30.2, Crossterm 0.29.0.
- Storage: none. No network, configuration, or secret data in the application.
- Tests: standard Rust unit/integration tests; Ratatui TestBackend for layouts/events;
  Linux pseudo-terminal validation for real cleanup where available.
- Platforms: Windows first; Windows and Linux CI, macOS unverified.
- Constraints: no animations, no fabricated data, no future service/storage abstractions.

## Constitution Check
Before and after design: approved scope only; local/offline; owner-approval policy documented;
version-matched API review; acceptance tests; visible navigation and simple modules. No exceptions.

## Project Structure
- `src/main.rs`: CLI dispatch and error exit.
- `src/cli.rs`: clap parser; `src/info.rs`: shared truthful status and about text.
- `src/tui.rs`: screen state, keyboard handling, render, terminal lifecycle.
- `tests/cli.rs`: executable output/error contracts.
- `tests/tui_pty.py`: optional Linux terminal integration check, no third-party Python packages.
- `.github/workflows/ci.yml`: locked checks on Ubuntu and Windows.
- Requested project documentation in root and `docs/`; short feature artifacts here.

## Decisions and API Evidence
See [research.md](research.md). Keep a synchronous event reader; redraw only on events.
Use Ratatui `try_init`/`try_restore` with a cleanup guard created before initialization,
plus its panic hook. Reject non-terminals before changing modes.
Only local screen state exists; no persisted model. The CLI contract and acceptance scenarios
are in spec.md. README is the setup guide; no duplicate quickstart or future data model needed.

## Validation
Run fmt, `cargo check --locked`, Clippy, tests, and release build with lockfile.
Test real q/Esc/Ctrl+C exit and termios restoration on a pseudo-terminal where possible.
Review implementation against every FR/SC after checks; record actual results in tasks.md.
