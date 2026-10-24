# Implementation Plan: Task 002 — Encrypted Local Vault

**Spec**: [spec.md](spec.md) · **Design**: [architecture](../../docs/architecture.md#vault-architecture-task-002),
[ADR 0001](../../docs/adr/0001-key-custody-and-storage.md), [security model](../../docs/security.md)
**Status**: 002B implemented; 002C in progress (records and `secret` commands done).

## Technical context

- Rust stable (1.97 locally), edition 2024, one binary. No workspace or library crate.
- New direct dependencies, exact-pinned (versions and docs in
  [architecture](../../docs/architecture.md#dependencies-exact-versions-checked-2026-10-19)):
  `argon2` and `chacha20poly1305` with `default-features = false` and `alloc` + `zeroize`;
  `getrandom`, `zeroize` (`derive`), `rusqlite` (`bundled`), `directories`, `tempfile`,
  `rpassword`, `unicode-normalization`, `data-encoding`; `libc` on Linux and `windows-sys`
  (`Win32_Foundation`, `Win32_Security`, `Win32_Security_Authorization`,
  `Win32_System_SystemServices`) on Windows. `sha2` (default features off) was added in 002C.
- APIs were read from the published sources of those exact versions before use.

## Key implementation choices

| Topic | Choice | Reason |
| --- | --- | --- |
| Crypto boundary | Only `crypto.rs` imports crypto crates | One place to review |
| Encryption calls | `AeadInOut::encrypt_in_place`/`decrypt_in_place` on `Zeroizing<Vec<u8>>` with capacity reserved for the tag | `Aead::decrypt` returns a plain `Vec` that would not be wiped; reserving avoids reallocation copies |
| Argon2id memory | `hash_password_into_with_memory` with a MaxonKey-owned block buffer, wiped afterwards | The crate frees its own buffer without wiping |
| KDF settings | Stored per key slot, bound into that slot's AAD | Passphrase change must not invalidate the recovery slot |
| Format identity | `PRAGMA application_id` + `user_version` | Reject foreign files and future versions before reading tables |
| Atomic init | `tempfile` file in the target directory → `into_temp_path()` → SQLite writes → `persist_noclobber` → directory `fsync` | Never overwrite; nothing left on failure |
| Passphrase input | `rpassword` (`/dev/tty`, `CONIN$`), result moved into `Zeroizing` and NFC-normalized | Terminal only; agents and scripts cannot supply it |
| Recovery key display | Written to `/dev/tty` / `CONOUT$`, never stdout | Cannot be captured by redirection |
| Test cost | `#[cfg(test)]` Argon2id parameters (64 KiB, t=1); CLI always uses defaults | Fast unit tests without a CLI path to weaker parameters |
| Platform hardening | Linux `prctl(PR_SET_DUMPABLE, 0)` at startup; Windows ACL warning via `GetNamedSecurityInfoW` | D12, D14 |

## Phases

1. **002B — vault lifecycle** (done): modules `secret`, `crypto`, `paths`, `prompt`, `store`,
   `vault`, `commands`; commands `vault init|status|change-passphrase|recover`; truthful
   status/TUI; tests; `deny.toml` and CI.
2. **002C — secrets and backups**: `records` CRUD with metadata/value blobs, manifest digests
   (`sha2`), in-memory index, `secret set|get|list|delete`, `vault rotate-key|backup|verify|restore`,
   remaining security tests.

## Verification

`cargo fmt --check`, `cargo check`, `cargo clippy -D warnings`, `cargo test`, release build
(all `--locked`); Linux `tests/tui_pty.py` and `tests/vault_pty.py`; `cargo deny --locked check
advisories licenses`; Windows CI runs the Rust checks and unit tests.
