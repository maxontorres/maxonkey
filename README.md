# MaxonKey 0.1.0

A personal, local-first secrets manager by **Maxon Torres Software**.
Windows first; Linux and macOS support will follow.

**Foundation prototype only:** the CLI and TUI work. Encryption, vault storage,
secret CRUD, project/environment management, MCP, and sharing are not implemented.
Do not use this version to manage secrets. It reads and stores no secrets.

## Develop and run

Install [Rust with rustup](https://rustup.rs/) and a native build toolchain
(on Windows: Visual Studio Build Tools with Desktop development with C++).
Clone the repository, then:

```sh
cargo build --locked
cargo run --locked -- --help
cargo run --locked -- status
cargo run --locked
```

The repository selects stable Rust with rustfmt and Clippy. Cargo.lock pins dependencies.
The TUI requires interactive stdin and stdout. A true-color terminal such as Windows
Terminal is recommended; use at least 64 columns × 22 rows for the full layout.
Smaller terminals display resize guidance; exit keys remain active.

Commands: `maxonkey` opens the TUI; `help`/`--help`, `version`/`--version`, `about`,
and `status` provide CLI information. In the TUI: Tab/arrows navigate, Shift+Tab
moves back, 1/2/3 select Dashboard/Help/About, ? opens Help, q/Esc/Ctrl+C exit.
There are no animations.

```sh
cargo fmt --all -- --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
# Optional Linux terminal integration check:
python3 tests/tui_pty.py target/release/maxonkey
```

GitHub Actions runs the Rust checks on Windows and Linux, plus the PTY check on Linux.
macOS is not yet verified. See [AGENTS.md](AGENTS.md) for AI development rules and
[Task 001](specs/001-project-initialization/spec.md) for acceptance criteria.

Spec Kit 1.2.1 is configured for Codex using `.agents/skills/` and `.specify/`.
A fresh local Codex session discovers `$speckit-*`. Optional CLI setup:
`uv tool install specify-cli==1.2.1`. See the feature [research](specs/001-project-initialization/research.md)
for the official integration references and initialization command.

## Planned installation

Distribute precompiled executables: `maxonkey.exe` for Windows and `maxonkey` for
Linux, with architecture-specific downloads and published integrity checks.
Windows users will extract to a tools directory and optionally add it to PATH;
Linux users will place the executable in `~/.local/bin` and mark it executable.
Task 001 builds locally to `target/release/`; no installers, release workflow,
or downloadable release assets are provided yet.

Future application data will live outside the executable directory: provisionally
`%LOCALAPPDATA%\MaxonKey` on Windows and `$XDG_DATA_HOME/maxonkey` (fallback
`~/.local/share/maxonkey`) on Linux. Final paths and permissions await security design.
This version creates no application data directory.

Product scope: [product](docs/product.md). Open decisions: [security](docs/security.md).
Implementation structure: [architecture](docs/architecture.md). Theme: [UI design](docs/ui-design.md).
