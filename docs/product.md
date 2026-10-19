# Confirmed product requirements

MaxonKey 0.1.0 is a personal local-first secrets manager developed by Maxon Torres Software.
Rust; Windows first, Linux and macOS later. Single-user, offline operation.

Interfaces: clap CLI, Ratatui/Crossterm TUI, and a future MCP server for AI coding agents.
The product will provide a local encrypted vault, project/environment organization,
secret creation/retrieval/update/deletion, and eventually secure developer sharing.
SQLite is provisionally selected; it is not installed or used in Task 001.

MCP plaintext reads and secret creation, updates, and deletion require owner approval.

Task 001 delivers only CLI information commands and Dashboard, Help, and About screens
with keyboard navigation and graceful exit. All sensitive capabilities and organization/
sharing features remain unavailable. No secret input or persistence exists.
