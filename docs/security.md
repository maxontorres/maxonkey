# Security requirements and open decisions

Task 001 does not protect or manage secrets. It has no vault, cryptography, database,
MCP server, clipboard access, telemetry, networking, or secret entry fields.
Informational status must never imply that a vault is secured, locked, or unlocked.

Confirmed requirements for later work:
- Local-first offline vault with encryption, single owner, and separate application data.
- Explicit owner approval before every MCP plaintext read, creation, update, or deletion.
- No secret material in logs, error messages, tests, or development artifacts.
- Future secure sharing requires a separately reviewed specification.

Unresolved decisions (must be addressed before sensitive implementation): threat model
and attacker boundaries; audited cryptographic library and authenticated encryption;
password/key derivation and parameters; key custody and OS keychain integration;
unlock timeout and memory lifetime/zeroization; metadata encryption and SQLite/WAL
leakage; file permissions, backups, recovery, and deletion guarantees; clipboard policy;
MCP transport/authentication, trusted owner approval UI, approval scope/expiry/replay
prevention, and denial behavior. Cross-platform data locations remain provisional.

Recommended next task: approve a threat model and vault/security specification, including
MCP owner-approval boundaries, before writing secret-handling code.
