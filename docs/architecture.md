# Architecture

This document describes the implemented architecture. Task 002B (vault lifecycle) and the
secret records of Task 002C (T101–T102) are implemented; parts marked **(002C)** or
**(future)** are approved designs not yet built.

Related: [security model](security.md) · [ADR 0001 — key custody and storage](adr/0001-key-custody-and-storage.md) ·
[Spec 002 — encrypted vault](../specs/002-encrypted-vault/spec.md)

## Application overview

One synchronous Rust binary, `maxonkey`. No workspace, background worker, server, or
configuration store.

- `main.rs`: clap parsing, Linux process hardening, dispatch, exit codes.
- `cli.rs`: subcommands and the global `--vault <PATH>` option; no command launches the TUI.
- `commands.rs`: `vault init|status|change-passphrase|recover` and `secret set|get|list|delete` handlers.
- `info.rs`: capability status, vault status line, and About text shared by CLI and TUI.
- `tui.rs`: three-screen state, key handling, event-driven rendering, terminal cleanup.
  The Dashboard shows whether a vault exists; the TUI never unlocks it.
- Vault modules: see [modules](#modules).

The TUI uses Crossterm events on one thread and Ratatui's Crossterm backend.
It redraws on events without timers or animations. It rejects redirected terminal IO
before setup; a cleanup guard handles setup/loop errors, and Ratatui installs panic
restoration. q, Esc, and Ctrl+C are normal exits. Terminal termination by force-kill
cannot guarantee cleanup.

Unit tests live beside each module. CLI executable tests verify output, errors, and exit
codes. Linux PTY scripts validate real TUI exit and the full vault flow with real
Argon2id costs. CI targets Windows and Linux and runs cargo-deny. All dependencies are
locked; direct versions are exact.

## Vault architecture (Task 002)

### Design principles

- Keep one binary and plain modules. No workspace, traits, or service layers until a second
  implementation actually exists.
- Standard constructions from maintained RustCrypto crates. No custom cryptography.
- The vault file holds only ciphertext and non-sensitive structure.
- Decrypt as little as possible, for as short a time as possible.
- Task 002 has **no background process**. The broker arrives with MCP (see
  [future broker](#future-local-broker-cli-tui-and-mcp)).

### Modules

| Module | Responsibility |
| --- | --- |
| `crypto.rs` | Thin wrappers over `argon2`, `chacha20poly1305`, `getrandom`, and `sha2`: derive KEK, seal/open blobs with AAD, padding, AAD layouts, SHA-256 record digests. Only module that touches crypto crates. |
| `secret.rs` | Redacting, zeroizing wrapper types for keys, passphrases (NFC-normalized), and decrypted bytes. |
| `vault.rs` | Vault lifecycle (init, status, unlock, lock on drop, change passphrase, recovery key), manifest and record verification, record metadata layout, in-memory index, secret set/get/list/delete, error-to-exit-code mapping. |
| `store.rs` | SQLite connection setup, schema, version checks, record and manifest transactions. Never sees plaintext. Backup/restore in 002C. |
| `paths.rs` | OS-standard data directory, owner-only creation, permission warnings (Unix modes, Windows ACL). |
| `prompt.rs` | Hidden input and confirmations from the controlling terminal, output written directly to it, and `--stdin` value input. |

### Key hierarchy

```
passphrase ──Argon2id──► KEK₁ ─┐
recovery key (optional) ► KEK₂ ─┼─ XChaCha20-Poly1305 wrap ─► VK (random 256-bit)
                               ┘                              │
                                     XChaCha20-Poly1305 per blob, AAD-bound
                                                              ▼
                                         meta_blob · value_blob · manifest
```

See [ADR 0001](adr/0001-key-custody-and-storage.md) for parameters and rationale.

### Vault file format (SQLite)

File: `vault.sqlite3` in the [data directory](#data-locations-and-permissions).

| Table | Columns (all plaintext unless noted) | Notes |
| --- | --- | --- |
| `vault_header` | `format_version`, `vault_id` (16 B random) | One row. Authenticated through key-slot and manifest AAD. |
| `key_slots` | `slot_id` (`1` passphrase, `2` recovery), `kdf_id`, `kdf_m`, `kdf_t`, `kdf_p`, `salt` (16 B), `wrapped_vk` (blob) | KDF settings live per slot, so changing the passphrase never disturbs the recovery slot. The recovery slot uses `kdf_id = 0` and zero costs. All fields are bound into the slot's AAD. |
| `records` | `record_id` (16 B random, primary key), `meta_blob` (encrypted), `value_blob` (encrypted, nullable) | Project, environment, name, notes, timestamps are all inside `meta_blob`. Projects and environments are fields, not separate records. |
| `manifest` | `blob` (encrypted) | Version, generation counter, record count, then `(record_id, SHA-256(meta_blob ‖ value_blob))` per record, ids ascending. Created empty at init. |

All tables are `STRICT`. `PRAGMA application_id = 0x4D4B5631` ("MKV1") identifies vault
files and `PRAGMA user_version = 1` is the schema version; unknown values are refused.

Blob layout: `format byte (1) ‖ nonce (24 B) ‖ ciphertext ‖ tag (16 B)`.
AAD layouts (fixed length, little-endian integers, no ambiguity):

- Key slot: `"MaxonKey/v1/" ‖ 0x01 ‖ format_version (4) ‖ vault_id ‖ slot_id (1) ‖ kdf_id (1) ‖ m (4) ‖ t (4) ‖ p (4) ‖ salt` (63 B).
- Manifest: `"MaxonKey/v1/" ‖ 0x02 ‖ vault_id ‖ 16 zero bytes ‖ 0x00` (46 B).
- Records: `"MaxonKey/v1/" ‖ 0x03 ‖ vault_id ‖ record_id ‖ field (1 B)` (46 B); field `0x01` is
  `meta_blob`, `0x02` is `value_blob`.

Plaintext inside a blob is a 4-byte length prefix, the data, and zero padding to a
multiple of 64 bytes; malformed padding is rejected. Manifest and record contents use
small fixed binary layouts with explicit lengths. Frozen test vectors pin all layouts.

- Manifest plaintext: `version (1) ‖ generation (8) ‖ count (4) ‖ count × (record_id (16) ‖ SHA-256 (32))`.
- `meta_blob` plaintext: `version (1) ‖ created (8) ‖ updated (8) ‖ project, environment, name
  (each 2-byte length ‖ UTF-8) ‖ notes (4-byte length ‖ UTF-8)`; times are Unix seconds, notes
  are empty until a command sets them.
- `value_blob` plaintext: the value bytes (1 B – 64 KiB).

**What SQLite still reveals** (accepted, see [ADR 0001](adr/0001-key-custody-and-storage.md#consequences)):
number of records, padded blob sizes, KDF parameters, schema version, and file timestamps.

### SQLite configuration

- `rusqlite =0.40.2` with `bundled` (SQLite 3.53.2 compiled in; identical on Windows and Linux,
  no system SQLite needed).
- `journal_mode=DELETE`, `synchronous=FULL`: simplest crash-safe mode for a single writer;
  no long-lived `-wal` file beside the vault.
- `secure_delete=ON`: deleted ciphertext is overwritten inside the file.
- `foreign_keys=ON`, `trusted_schema=OFF`; extension loading never enabled.
- Every write is one `BEGIN IMMEDIATE` transaction that updates the affected records **and** the
  manifest together, with a `busy_timeout` for concurrent CLI invocations.
- `PRAGMA quick_check` on every open; `PRAGMA integrity_check` in `maxonkey vault verify`.

### Lifecycle flows

**Initialize** — refuse if the path exists → read passphrase twice from the terminal →
check minimum length → optionally generate the recovery key, write it directly to the
terminal (never stdout), wait for Enter → create the data directory (owner-only) →
generate `vault_id`, salt, VK → derive KEK → wrap VK into slot 1 (and slot 2 with the
recovery key) → seal an empty manifest → write everything into a private temporary file in
the same directory (`synchronous=FULL`) → rename into place **without overwriting**
(`tempfile` `persist_noclobber`) → `fsync` the directory. Interrupting before the rename
leaves no vault; an existing vault is never replaced.

**Unlock** — open file → `quick_check` → read header → reject KDF parameters outside limits →
derive KEK (Argon2id working memory wiped afterwards) → unwrap VK (authentication failure =
"wrong passphrase or damaged vault"; the two cannot be told apart, by design) → wipe KEK →
decrypt and verify the manifest → check that the stored records are exactly those listed, with
matching digests → decrypt every `meta_blob` into the in-memory index. Any damaged, missing, or
extra record refuses the unlock (exit 4) and reports only counts.

**Read value** — locate record in index → decrypt `value_blob` → hand to the output path →
wipe immediately after.

**Write** — encrypt both blobs with fresh nonces → in one `BEGIN IMMEDIATE` transaction, confirm
the manifest is the one verified at unlock (otherwise another process wrote; refuse) → update the
record and the manifest (generation + 1) → commit. On any error, roll back; the vault stays at
the previous generation.

**Lock** — wipe VK, the index, and any decrypted values. In Task 002 this happens at the end
of every CLI command.

**Change passphrase** — unlock with the current passphrase (or recovery key) → new salt and
parameters → re-wrap VK into slot 1 in one transaction. Data is not re-encrypted.

**Rotate vault key (002C)** (explicit, rare) — generate new VK, re-encrypt every record and the
manifest, re-wrap all slots, all in one transaction.

### Integrity, atomic writes, corruption

| Situation | Behaviour |
| --- | --- |
| Crash or power loss during a write | SQLite rollback journal restores the last committed state. |
| `quick_check` fails, wrong `application_id`, not a database | Refuse to open (exit 4) without modifying the file. **(002C)** `vault verify` and restore guidance. Never auto-repair or drop data. |
| Key slot fails to authenticate | Single generic message: wrong passphrase or damaged vault. |
| Manifest or record fails to authenticate, or digest mismatch | Refuse to unlock (exit 4) and report the number of damaged and missing records (no names). **(002C)** restore guidance with `vault verify`/`restore`. |
| Unknown future format/schema version | Refuse to open; never downgrade. |
| Schema migration | Automatic encrypted backup first, then migrate in one transaction. |

### Backup and recovery (002C, except the recovery key)

- **Backup** (`maxonkey vault backup <path>`): consistent snapshot using SQLite
  `VACUUM INTO` (or rusqlite's online backup API) into a new owner-only file. The backup is
  the same encrypted format; it opens with the passphrase (or recovery key) valid at backup time.
  Plaintext export is not provided.
- **Verify** (`maxonkey vault verify [path]`): integrity check plus full authentication of
  header, slots, manifest, and every record.
- **Restore** (`maxonkey vault restore <path>`): verify the backup unlocks and is intact →
  move the current vault aside to a timestamped file → place the backup with an atomic rename.
- **Automatic backups**: before schema migrations and restores only; keep the newest three in
  `backups/` inside the data directory. No automatic backup on passphrase change, so an
  old passphrase is not silently preserved.
- **Recovery key**: optional second key slot; see [ADR 0001](adr/0001-key-custody-and-storage.md).

### Data locations and permissions

Resolved with `directories =6.0.0` `ProjectDirs::from("", "", "MaxonKey").data_local_dir()`.

| Platform | Vault path |
| --- | --- |
| Windows | `%LOCALAPPDATA%\MaxonKey\data\vault.sqlite3` |
| Linux | `$XDG_DATA_HOME/maxonkey/vault.sqlite3` (fallback `~/.local/share/maxonkey/`) |
| macOS (future) | `~/Library/Application Support/MaxonKey/vault.sqlite3` |

A `--vault <path>` option allows tests and portable use. Linux creates the directory `0700`
and files `0600` at creation time (not via `chmod` afterwards) and warns if they are broader
on open. Windows relies on the per-user `%LOCALAPPDATA%` ACL and warns if the file grants
access to broad groups (Everyone, Users, Authenticated Users). Application data is never
stored beside the executable.

### Memory protection

- `zeroize =1.9.1`: VK, KEK, passphrase buffers, recovery key, decrypted values, and the
  metadata index implement `ZeroizeOnDrop` or sit in `Zeroizing<…>`.
- Wrapper types never derive `Debug`/`Display` with contents; they print `[REDACTED]`.
- Avoid intermediate `String` copies; decrypt into pre-sized zeroizing buffers.
- Linux: `prctl(PR_SET_DUMPABLE, 0)` at startup (no core dumps; blocks same-user ptrace).
- Argon2id working memory (256 MiB) is allocated by MaxonKey and wiped after each derivation,
  because the library frees it without wiping.
- Memory locking (`mlock`/`VirtualLock`) is not used (decision D14); zeroization does not
  stop the OS from paging memory out before it is wiped.

### Future local broker (CLI, TUI, and MCP)

Not part of Task 002; recorded so Task 002 does not block it.

```
 owner terminal          AI agent host
 ┌────────┐ ┌─────┐      ┌──────────────┐
 │  CLI   │ │ TUI │      │ maxonkey mcp │  (stdio MCP server, holds no keys)
 └───┬────┘ └──┬──┘      └──────┬───────┘
     └─────────┼────────────────┘  owner-only local socket / named pipe
               ▼
      ┌─────────────────────┐   approval prompt owned by the broker,
      │   maxonkey broker   │◄─ shown in the owner's broker console
      │  VK in memory only  │
      └─────────┬───────────┘
                ▼
          vault.sqlite3
```

- **Transport**: Unix domain socket in `$XDG_RUNTIME_DIR/maxonkey/` (`0700`) on Linux; named pipe
  with an owner-SID-only DACL, `PIPE_REJECT_REMOTE_CLIENTS`, and first-instance creation on
  Windows. Candidate crate: `interprocess =2.4.6` (supports Windows security descriptors and
  Unix peer credentials). Single-machine only; no TCP.
- **Identity**: peer process id/executable are recorded for display and audit, but are *not*
  a security boundary: every same-user process looks the same to the OS.
- **Unlock**: only in the broker's own console. Clients never transmit the passphrase.
- **Approval**: every plaintext read, create, update, and delete from MCP shows the request
  (operation, project/environment/name, requesting process) in the broker console. Approval is
  single-use, bound to a random request id, expires after 60 seconds, and defaults to deny.
- **Auto-lock**: idle timeout, explicit lock, broker exit. Session-lock/sleep hooks per platform later.
- **MCP server**: a separate process that translates MCP tool calls into broker requests and
  holds no keys. It never offers a passphrase or unlock tool.
- Residual risk (documented in [security](security.md#ai-agents-running-as-the-owner)): a
  same-user agent can drive any interface the owner can, so approvals guard against
  misbehaving agents using MaxonKey's interfaces, not against code that attacks the OS session.

### Dependencies (exact versions checked 2026-10-19)

Versions were resolved from crates.io and read from the matching published source.
All except `interprocess` (future) are in `Cargo.toml` with exact pins (`sha2` since 002C, with
default features off).
Platform-only: `libc =0.2.191` (Linux `prctl`) and `windows-sys =0.61.2` (Windows ACL
check), both already present in the lockfile through other dependencies.

| Crate | Version | Use | Reference |
| --- | --- | --- | --- |
| `chacha20poly1305` | 0.11.0 | XChaCha20-Poly1305 AEAD | https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/ |
| `argon2` | 0.6.0 | Argon2id KEK derivation (`hash_password_into`) | https://docs.rs/argon2/0.6.0/argon2/ |
| `getrandom` | 0.4.3 | OS random bytes (`fill`) | https://docs.rs/getrandom/0.4.3/getrandom/ |
| `sha2` | 0.11.1 | Manifest digests | https://docs.rs/sha2/0.11.1/sha2/ |
| `zeroize` | 1.9.1 | Memory wiping (`derive` feature) | https://docs.rs/zeroize/1.9.1/zeroize/ |
| `rusqlite` | 0.40.2 | SQLite, `bundled` feature (SQLite 3.53.2) | https://docs.rs/rusqlite/0.40.2/rusqlite/ |
| `directories` | 6.0.0 | OS-standard data paths | https://docs.rs/directories/6.0.0/directories/ |
| `tempfile` | 3.27.0 | Atomic no-clobber create (`persist_noclobber`) | https://docs.rs/tempfile/3.27.0/tempfile/ |
| `rpassword` | 7.5.4 | Hidden terminal input (`/dev/tty` / `CONIN$`) | https://docs.rs/rpassword/7.5.4/rpassword/ |
| `unicode-normalization` | 0.1.26 | NFC passphrase normalization | https://docs.rs/unicode-normalization/0.1.26/unicode_normalization/ |
| `data-encoding` | 2.11.1 | Base32 display of the recovery key | https://docs.rs/data-encoding/2.11.1/data_encoding/ |
| `interprocess` | 2.4.6 | Future broker IPC only | https://docs.rs/interprocess/2.4.6/interprocess/ |

Not selected: `secrecy` (last release 2024; a small local wrapper over `zeroize` is enough),
`keyring 4.2.0` (OS keychain custody rejected in ADR 0001), SQLCipher features, `aes-gcm`.
