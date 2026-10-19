# Feature Specification: Task 002 — Encrypted Local Vault

**Feature Branch**: `main` (owner-directed for the initial project phase)
**Created**: 2026-10-19
**Status**: Approved by the owner on 2026-10-19 (all decisions D1–D17 as recommended).
Task 002B implemented; Task 002C pending. Plan: [plan.md](plan.md) · Tasks: [tasks.md](tasks.md)
**Input**: Design the encrypted local vault (storage, cryptography, key custody, lifecycle,
integrity, backup, platform security, future broker, AI-agent threats) before implementation.

**Authoritative design**: [security model](../../docs/security.md) ·
[architecture](../../docs/architecture.md#vault-architecture-task-002) ·
[ADR 0001](../../docs/adr/0001-key-custody-and-storage.md). This spec states requirements and
tests; it does not repeat the design rationale.

## Scope

**In scope for Task 002 (implementation in 002B/002C after approval)**

- Vault initialization, status, passphrase change, recovery-key unlock, and vault-key rotation.
- Encrypted storage of secrets organized by project and environment.
- CLI secret commands: set, get (reveal), list (names only), delete.
- Encrypted backup, verify, and restore.
- Truthful vault status in `maxonkey status` and the TUI Dashboard.

**Out of scope** (separate specifications): local broker, MCP server and approvals, TUI unlock
and secret browsing, clipboard, audit log, OS keychain or hardware-backed unlock, encrypted
sharing, sync, plaintext export, multi-user access.

**Previously approved requirements preserved**: offline single-owner operation; data separate
from the executable; no secret material in logs/errors/tests/artifacts; owner approval for every
future MCP plaintext read, creation, update, and deletion; Millennium Blue UI, keyboard hints,
textual state labels, no motion; no absent capability labelled as working.

## User Scenarios & Testing

### User Story 1 — Create a vault (Priority: P1)

The owner runs `maxonkey vault init`, enters a passphrase twice on a hidden prompt, optionally
records a recovery key, and gets an encrypted vault in the OS-standard data directory.

**Independent Test**: initialize into a temporary `--vault` path through a pseudo-terminal and
inspect the resulting file.

**Acceptance Scenarios**:

1. **Given** no vault, **When** `vault init` completes, **Then** exactly one `vault.sqlite3` exists
   with an authenticated header, a passphrase key slot, and (unless declined) a recovery slot.
2. **Given** an existing vault, **When** `vault init` runs, **Then** it fails and the existing file
   is byte-for-byte unchanged.
3. **Given** a passphrase shorter than 12 characters or two entries that differ, **Then** init
   fails and no file is created.
4. **Given** init is interrupted at any step, **Then** either no vault exists or a complete vault exists.
5. **Given** the recovery key is shown, **Then** it is shown once, on the terminal only, and never
   written to any file.

### User Story 2 — Store and retrieve secrets (Priority: P1)

The owner stores `STRIPE_KEY` for project `shop`, environment `prod`, and later retrieves it.

**Independent Test**: set, list, get, update, and delete with canary values; scan all files and outputs.

**Acceptance Scenarios**:

1. `secret set shop prod STRIPE_KEY` prompts for the passphrase, then for the value on a hidden
   prompt (or reads stdin only with `--stdin`), and stores it.
2. `secret list [project [environment]]` prints names only, never values.
3. `secret get shop prod STRIPE_KEY` prints only the value to stdout, followed by a newline only
   when stdout is a terminal.
4. `secret delete` asks for confirmation on the terminal (skipped with `--yes`) and removes the record.
5. Setting an existing name updates it; the stored ciphertext changes even if the value is identical.

### User Story 3 — Change passphrase and recover (Priority: P1)

**Acceptance Scenarios**:

1. `vault change-passphrase` requires the current passphrase, sets a new one, and does not
   re-encrypt records; the old passphrase no longer opens this vault.
2. `vault recover` unlocks with the recovery key and requires a new passphrase.
3. With no recovery slot, `vault recover` states that no recovery key exists and exits non-zero.
4. `vault rotate-key` re-encrypts all records under a new vault key in one transaction.

### User Story 4 — Back up, verify, restore (Priority: P2)

**Acceptance Scenarios**:

1. `vault backup <path>` writes a consistent encrypted snapshot without overwriting an existing file.
2. `vault verify [path]` authenticates every header field, slot, manifest, and record, and reports
   pass/fail with the count of damaged records (no names, no values).
3. `vault restore <path>` refuses a backup that fails verification; on success, keeps the previous
   vault as a timestamped file and atomically replaces the active vault.

### User Story 5 — Honest status (Priority: P2)

1. `maxonkey status` and the TUI Dashboard show `Vault: NOT INITIALIZED` or
   `Vault: INITIALIZED (LOCKED)` plus the vault path; they never show `UNLOCKED` in Task 002
   because no process keeps the vault open between commands.
2. Status reads only the plaintext header and permission state; it never prompts for the passphrase.
3. Status warns when vault file or directory permissions are broader than owner-only.

### Edge Cases

- stdin/stdout redirected: passphrase still read from the controlling terminal; with no terminal,
  commands that need the passphrase fail with guidance (no stdin fallback).
- Vault header with KDF parameters above the ceilings: rejected before memory is allocated.
- Unknown format or schema version: refused, never downgraded.
- Concurrent CLI writers: serialized by SQLite; neither corrupts the manifest.
- Disk full or I/O error mid-write: transaction rolled back, prior generation intact.
- Vault on a network or cloud-synced folder via `--vault`: allowed with a warning.

## Requirements

### Functional Requirements

- **FR-001**: Provide `vault init|status|change-passphrase|recover|rotate-key|backup|verify|restore`
  and `secret set|get|list|delete` as CLI subcommands, plus a global `--vault <path>` option.
- **FR-002**: Default vault path is `ProjectDirs::from("", "", "MaxonKey").data_local_dir()/vault.sqlite3`
  (see [architecture](../../docs/architecture.md#data-locations-and-permissions)).
- **FR-003**: Secrets are addressed by (project, environment, name); names are case-sensitive
  UTF-8, 1–128 characters, no control characters. Values are 1 byte – 64 KiB.
- **FR-004**: Each command that needs vault content unlocks, performs its work, and locks before exit.
- **FR-005**: Exit codes: `0` success, `1` general error, `2` usage error, `3` authentication failed
  (wrong passphrase or damaged key slot), `4` integrity failure, `5` vault not initialized.
- **FR-006**: Update `info.rs` capability status truthfully as each capability ships.

### Security Requirements

**Cryptography and keys**

- **SR-001**: Use only the algorithms and crates in [ADR 0001](../../docs/adr/0001-key-custody-and-storage.md):
  XChaCha20-Poly1305, Argon2id v1.3, OS RNG, SHA-256. No other primitive and no custom construction.
- **SR-002**: Vault key (VK): 32 bytes from the OS RNG, generated once per vault (and on rotation).
- **SR-003**: Argon2id defaults m=262144 KiB, t=3, p=1, 16-byte random salt, 32-byte output.
  Creation rejects m<65536 KiB or t<3. Opening rejects m>2097152 KiB, t>10, p>16.
- **SR-004**: Every encryption uses a fresh 24-byte random nonce. Nonces are never derived,
  counted, or reused.
- **SR-005**: AAD binds each ciphertext to purpose, vault id, record id, and field; key-slot AAD
  additionally binds format version and all KDF fields.
- **SR-006**: Passphrases are normalized to Unicode NFC, must contain at least 12 characters,
  and are otherwise used byte-for-byte (no trimming, no case folding).
- **SR-007**: Recovery key: 32 random bytes, displayed once as grouped RFC 4648 Base32 (no
  padding); used directly as a key-slot KEK. A mistyped key fails slot authentication (exit 3).

**Storage and integrity**

- **SR-010**: User-supplied metadata (project, environment, name, notes, timestamps) is stored only
  inside encrypted blobs; no plaintext index or plaintext search column exists.
- **SR-011**: Plaintext is padded to a 64-byte multiple before encryption.
- **SR-012**: Every write updates records and the encrypted manifest in one `BEGIN IMMEDIATE`
  transaction and increments the generation counter.
- **SR-013**: Unlock verifies header, key slot, manifest, and every record digest before any write is
  permitted. Any failure prevents writes.
- **SR-014**: SQLite runs with `journal_mode=DELETE`, `synchronous=FULL`, `secure_delete=ON`,
  `trusted_schema=OFF`; extension loading stays disabled; `quick_check` runs on every open.
- **SR-015**: Init and restore use no-clobber atomic placement; no command overwrites a vault or
  backup file without moving the previous one aside.

**Input, output, and memory**

- **SR-020**: No passphrase, recovery key, or secret value is accepted as a command-line argument or
  environment variable. The parser defines no such option.
- **SR-021**: Passphrases and recovery keys are read only from the controlling terminal
  (`/dev/tty`, `CONIN$`) with echo disabled.
- **SR-022**: Secret values are read from a hidden terminal prompt or, with explicit `--stdin`,
  from standard input.
- **SR-023**: Secret values reach stdout only through `secret get`. No other output, log, or error
  contains values, passphrases, keys, or secret names.
- **SR-024**: Keys, passphrases, recovery keys, decrypted values, and the metadata index are held in
  zeroizing types and wiped on drop, including on error paths. Strings returned by input libraries
  are moved into zeroizing wrappers immediately.
- **SR-025**: Secret-bearing types print `[REDACTED]` for `Debug` and do not implement `Display`.
- **SR-026**: On Linux the process sets `PR_SET_DUMPABLE=0` before reading any passphrase.

**Files and platforms**

- **SR-030**: Linux: data directory created `0700`, vault and backups created `0600` at creation time.
- **SR-031**: Windows: data stored under `%LOCALAPPDATA%` (not roaming); status warns if the vault
  ACL grants Everyone, Users, or Authenticated Users.
- **SR-032**: No vault data is stored beside the executable or in the repository.

**Supply chain**

- **SR-040**: New crypto/storage dependencies are exact-pinned at the versions in
  [architecture](../../docs/architecture.md#dependencies-exact-versions-checked-2026-10-19)
  unless a reviewed reason changes them; CI adds `cargo deny check advisories licenses`.

## Security Test Requirements

All tests use generated canary values (for example `MK-CANARY-<32 random hex>`) and test-only
passphrases. Library-level tests use reduced Argon2id parameters through a test-only constructor
that the CLI cannot reach; at least one end-to-end test uses the real defaults.

| ID | Test | Pass condition |
| --- | --- | --- |
| ST-01 | Known-answer tests: Argon2id RFC 9106 §5.3 vector and the AEAD_XChaCha20_Poly1305 vector from draft-irtf-cfrg-xchacha-03 against the configured crates; frozen regression vectors for MaxonKey's KEK derivation, AAD, and blob layout | Outputs match exactly; any format change breaks the test |
| ST-02 | Canary scan: after set/update/delete/backup, search vault, journal, backups, stdout/stderr of every non-`get` command for canary value, secret name, project, environment, and passphrase (raw, UTF-16LE, Base64, hex) | 0 matches |
| ST-03 | Tamper sweep: for each byte of every header field, key slot, blob, and manifest, flip one bit and unlock/read | 100 % fail with exit code 3 or 4; 0 plaintext returned |
| ST-04 | Row swap: swap `meta_blob`/`value_blob` between records, and between two vaults | Detected (exit 4) |
| ST-05 | Record rollback and deletion: replace a record with its earlier stored version; delete a row; insert a copied row | Detected (exit 4) |
| ST-06 | Wrong passphrase / wrong recovery key | Exit 3, one generic message, no partial data, file unchanged |
| ST-07 | KDF ceilings: header with m=4 GiB, t=100, p=255 | Rejected before allocation, exit 4 |
| ST-08 | Nonce uniqueness: 10 000 encryptions of the same plaintext | 10 000 distinct nonces and ciphertexts |
| ST-09 | Crash consistency: injected failure between each statement of a write transaction, and process kill during write (Linux) | Vault opens at previous or new generation, never mixed; verify passes |
| ST-10 | Init safety: existing vault, interrupted init, mismatched passphrase, short passphrase | Existing file unchanged; no partial vault |
| ST-11 | Parser hardening: `--passphrase`, `--value`, `--key`, and `MAXONKEY_PASSPHRASE` env | Rejected or ignored; never used |
| ST-12 | No TTY: passphrase commands with stdin/stdout redirected and no controlling terminal | Fail with guidance, never read passphrase from stdin |
| ST-13 | Permissions (Linux): new directory/vault/backup modes | `0700` / `0600` / `0600` regardless of umask `000` |
| ST-14 | Redaction: `format!("{:?}")` of every secret-bearing type; forced panic while unlocked | Only `[REDACTED]`; no canary in panic output |
| ST-15 | Zeroization: wrapper types call `zeroize` on drop (unit test over a `Zeroize` probe) | Buffers zeroed |
| ST-16 | Backup/restore: round trip; restore of tampered backup; restore keeps previous vault | Values identical; tampered refused; previous file present |
| ST-17 | Passphrase change: old passphrase fails, new succeeds, recovery slot still works, record ciphertext unchanged | All hold |
| ST-18 | Key rotation: all record ciphertexts change, values identical, old VK cannot open records | All hold |
| ST-19 | Unicode: NFC and NFD forms of the same passphrase unlock the same vault | Both succeed |
| ST-20 | Format/version: unknown format byte, future schema version | Refused, file unchanged |

## Success Criteria

- **SC-001**: ST-01 to ST-20 pass in CI on Windows and Linux (ST-09 kill variant and ST-13 Linux only).
- **SC-002**: With default Argon2id parameters, unlock completes in ≤ 2.0 s on the GitHub-hosted
  Windows and Linux runners (median of 5); measured value recorded in the implementation notes.
- **SC-003**: With 1 000 secrets, `secret list` completes in ≤ 3.0 s and `secret set` in ≤ 3.5 s on
  the same runners, including key derivation.
- **SC-004**: Vault file contains 0 occurrences of any plaintext user metadata or value (ST-02).
- **SC-005**: Every single-bit modification of authenticated data is detected (ST-03, 100 %).
- **SC-006**: `cargo fmt`, `check`, `clippy -D warnings`, `test`, release build, and
  `cargo deny check advisories licenses` pass with `--locked`.
- **SC-007**: README, security, and architecture docs describe only implemented behaviour as available.

## Assumptions

- Owner-only use on a computer the owner controls; agents may run as the same OS user.
- A personal vault holds at most a few thousand secrets.
- GitHub-hosted runners are the performance reference until owner hardware is measured.

## Implementation phasing (after approval)

| Task | Content |
| --- | --- |
| 002B | `crypto`, `secret`, `paths`, `prompt`, `store` modules; `vault init/status/change-passphrase/recover`; ST-01, 06–08, 10–15, 17, 19, 20 |
| 002C | Records, manifest, `secret` commands, `rotate-key`, `backup/verify/restore`; ST-02–05, 09, 16, 18 |
| 003 | Separate spec: local broker, MCP server, owner approval UI |

## Owner approval checkpoint

Approved by the owner on 2026-10-19: every row as recommended.

| # | Decision | Recommended | Alternatives considered | Security implications | Complexity | Owner decision |
| --- | --- | --- | --- | --- | --- | --- |
| D1 | Storage encryption layer | SQLite (bundled) + per-record XChaCha20-Poly1305, all user metadata encrypted | SQLCipher whole-file; single encrypted file | Hides values and names; leaks record count, padded sizes, file times. SQLCipher hides more structure but adds a C crypto stack | Medium | Approved |
| D2 | AEAD | XChaCha20-Poly1305, `chacha20poly1305 =0.11.0` | AES-256-GCM; AES-GCM-SIV; ring/aws-lc-rs; libsodium | Random 192-bit nonces remove nonce-reuse risk; constant-time without AES-NI; audited crate | Low | Approved |
| D3 | Argon2id parameters | m=256 MiB, t=3, p=1; floor 64 MiB/t=3; open ceilings 2 GiB/t=10/p=16 | OWASP minimum 19 MiB/t=2; RFC 9106 2 GiB/t=1 | ≈0.75 s per guess on a 4-vCPU runner; ceilings stop header-based DoS | Low | Approved |
| D4 | Key hierarchy | Random VK wrapped per key slot | Encrypt data directly with passphrase-derived key | Fast passphrase change; enables recovery slot and future hardware slots | Low | Approved |
| D5 | Key custody | Passphrase only; VK never in OS keychain | Keychain/DPAPI auto-unlock; Windows Hello/TPM/FIDO2 slot | Keychain unlock would let any same-user agent open the vault; hardware slot deferred | Low | Approved |
| D6 | Recovery | Optional recovery key offered at init (default: create) | No recovery; mandatory recovery key | Second offline secret that fully opens the vault; must be stored safely | Low | Approved |
| D7 | Rollback/tamper detection | Encrypted manifest (generation + SHA-256 per record) | Per-record AEAD only; external counter in OS keychain | Detects deletion, per-record rollback, swaps; whole-file rollback stays undetectable | Medium | Approved |
| D8 | Unlock model for Task 002 | Passphrase per CLI command, no background process; TUI status only | Session cache/daemon now | No unlocked state for agents to piggyback on; slower repeated use | Low | Approved |
| D9 | Passphrase input policy | Terminal only; min 12 chars; NFC normalization; no stdin/env/argv | `--passphrase-stdin` for scripting; no normalization | Agents cannot supply it without knowing it; scripting not supported | Low | Approved |
| D10 | Secret value I/O | Hidden prompt or explicit `--stdin`; reveal only via `secret get` to stdout; no clipboard | Clipboard with auto-clear; `run` env injection | Values printed to stdout are outside MaxonKey's control once shown | Low | Approved |
| D11 | SQLite durability mode | `journal_mode=DELETE`, `synchronous=FULL`, `secure_delete=ON` | WAL mode | Fewer stale ciphertext copies; slower writes irrelevant at personal scale | Low | Approved |
| D12 | Data location and permissions | OS-standard paths; Linux 0700/0600 at creation; Windows profile ACL + warning | Explicit Windows DACL via `windows-sys`; beside executable | Same-user processes are not blocked by any of these | Low | Approved |
| D13 | Backup policy | Encrypted snapshots; automatic only before migration/restore (keep 3); no plaintext export | Periodic automatic backups; plaintext export | Old backups open with the passphrase valid at backup time | Low | Approved |
| D14 | Memory protection | Zeroize + redaction + Linux non-dumpable; no memory locking in Task 002 | `mlock`/`VirtualLock` | Pages may reach swap/hibernation before wiping | Low (Medium with locking) | Approved |
| D15 | Future broker shape (direction only) | Owner-only socket/named pipe; broker-owned approval console; single-use 60 s approvals; MCP holds no keys | Per-session grants; OS-native consent dialogs; no broker | Guards against misbehaving agents using MaxonKey; not against code attacking the OS session | High (Task 003) | Direction approved |
| D16 | Agent isolation guidance | Document separate OS account/container/VM for agents as the real boundary | Rely on MaxonKey approvals alone | Sets honest expectations | Low | Approved |
| D17 | Supply-chain check | Add `cargo deny check advisories licenses` to CI in 002B | `cargo audit` only; none | Catches known-vulnerable or mis-licensed dependencies | Low | Approved |
