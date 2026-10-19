# Shared AI development rules

MaxonKey is a personal local-first secrets manager by Maxon Torres Software.
Read this file, the constitution in `.specify/memory/constitution.md`, and the active
feature's spec, plan, and tasks before changing code. Inspect git status first;
preserve unrelated work and use a dedicated branch. Never push directly to main.

## Development
- Use Spec-Driven Development: short spec, implementation plan, acceptance criteria,
  tasks, implementation, automated verification, then convergence against the spec.
- Official Spec Kit Codex skills are checked in at `.agents/skills/`; restart local
  Codex after setup to discover `$speckit-*`. Feature artifacts live in `specs/`.
- Keep documentation proportionate. Do not generate future-feature designs or
  introduce service/repository abstractions before they are needed.
- Task 001 is CLI/TUI foundation only. Do not add encryption, actual vault/SQLite
  storage, secret CRUD, or MCP implementations.

## Dependency/API policy
- Read `Cargo.toml`, `Cargo.lock`, and `cargo tree` before using dependency APIs.
- Consult official documentation for the exact resolved version (docs.rs versioned
  URLs or matching upstream release source). Never infer APIs from old tutorials.
- Use the stable toolchain in `rust-toolchain.toml`; commit `Cargo.lock` for this binary.
- Do not upgrade dependencies without a written reason and compatibility/security
  review. Direct versions are exact pins; make deliberate, reviewed updates.
- Use `--locked` for verification. Never silently regenerate the lockfile to fix CI.

## Verification
Write meaningful tests for CLI parsing/output, keyboard behavior, terminal lifecycle,
and layout boundaries. Run:

```sh
cargo fmt --all -- --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

On Linux, also run `python3 tests/tui_pty.py target/release/maxonkey` for real
terminal exit/restoration coverage. Windows/Linux CI is configured; distinguish
local results from observed runner results. Never claim a check passed unless it ran.

## Security and UI
- Never put real secrets in code, fixtures, logs, screenshots, or agent artifacts.
- Future MCP plaintext reads, creation, updates, and deletion require explicit owner
  approval. No autonomous secret-management path or implicit approval is permitted.
- Report unavailable capabilities truthfully. No fictional audit entries or implied
  vault protection. Do not label an absent vault as locked or unlocked.
- Preserve the Millennium Blue palette in `docs/ui-design.md`, visible keyboard
  hints, textual/symbolic state labels, and small-terminal safety.
- No motion in this prototype. Future animation must honor reduced-motion preferences
  and remain optional. Keep application data separate from the executable.

## Delivery
Summarize changes, actual verification, limitations, unresolved security decisions,
and the next recommended task. Commit scoped changes and prepare a PR when supported.
