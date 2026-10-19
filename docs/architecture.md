# Initial architecture

One synchronous Rust binary, `maxonkey`. No workspace, background worker, server,
configuration store, database, or cryptographic layer yet.

- `main.rs`: clap parsing, dispatch, and failure exit code.
- `cli.rs`: optional subcommand; absent command launches the TUI.
- `info.rs`: capability status and About text shared by CLI and TUI.
- `tui.rs`: three-screen state, key handling, event-driven rendering, terminal cleanup.

The TUI uses Crossterm events on one thread and Ratatui's Crossterm backend.
It redraws on events without timers or animations. It rejects redirected terminal IO
before setup; a cleanup guard handles setup/loop errors, and Ratatui installs panic
restoration. q, Esc, and Ctrl+C are normal exits. Terminal termination by force-kill
cannot guarantee cleanup.

CLI executable tests verify output and errors; unit tests verify parser, navigation,
and rendered layouts. A Linux PTY check validates real exit and mode restoration.
CI targets Windows and Linux. All dependencies are locked; direct versions are exact.

SQLite is provisional for later storage. MCP and vault components are absent until
approved specifications define their security boundaries. Future application data
must remain separate from the binary (see README); no data path is used today.
