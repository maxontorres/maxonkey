# ADR 0001 — Key custody and vault storage

- **Status**: Accepted — approved by the owner on 2026-10-19; implemented in Task 002B (key slots, manifest) and 002C (records)
- **Date**: 2026-10-19
- **Scope**: How MaxonKey derives, wraps, holds, and stores keys and encrypted records.
- **Related**: [security model](../security.md), [architecture](../architecture.md),
  [Spec 002](../../specs/002-encrypted-vault/spec.md)

This record decides how keys are derived, wrapped, held, and stored.

## Context

MaxonKey is a single-owner, offline secrets manager. Windows comes first, then Linux,
then macOS. The owner also runs AI coding agents with shell access **under the same
operating-system account**. Two consequences follow:

1. Anything the owner's account can read without a human present, an agent can read too.
   Same-user file permissions, the OS keychain, and Windows DPAPI do not separate the
   owner from an agent running as the owner.
2. The only secret an agent does not automatically have is something only the human
   knows or physically holds: the master passphrase (or a recovery key kept offline).

The design must use established algorithms and maintained libraries only. MaxonKey
must not invent cryptography.

## Decision

### 1. Key hierarchy (envelope encryption)

```
master passphrase ──Argon2id(salt, params)──► KEK (32 bytes, never stored)
recovery key (optional, 32 random bytes) ───► used directly as a second KEK
                                                   │ XChaCha20-Poly1305 key wrap
                                                   ▼
                                      Vault Key, VK (32 random bytes)
                                                   │ XChaCha20-Poly1305
                                                   ▼
                     encrypted record metadata, record values, and the manifest
```

- **VK** is generated once from the OS random generator (`getrandom`). It encrypts all
  vault content. It never leaves process memory unencrypted and is never written to disk,
  the OS keychain, logs, or the terminal.
- **KEK** (key-encryption key) is derived from the passphrase with Argon2id and used only
  to wrap (encrypt) VK. Changing the passphrase re-wraps VK; it does not re-encrypt data.
- An optional **recovery key** wraps the same VK in a second *key slot*. It is shown once at
  initialization for the owner to store offline. It is the only way to recover a vault if
  the passphrase is forgotten.

### 2. Algorithms and libraries

| Purpose | Choice | Crate (exact candidate) |
| --- | --- | --- |
| Authenticated encryption | XChaCha20-Poly1305, random 24-byte nonce per encryption | `chacha20poly1305 =0.11.0` (RustCrypto, NCC-audited) |
| Passphrase key derivation | Argon2id v1.3, 16-byte random salt, 32-byte output | `argon2 =0.6.0` (RustCrypto) |
| Randomness | OS CSPRNG | `getrandom =0.4.3` |
| Manifest record digests | SHA-256 | `sha2 =0.11.1` (RustCrypto) |
| Key/secret memory wiping | `Zeroize`, `Zeroizing<T>`, `ZeroizeOnDrop` | `zeroize =1.9.1` |
| Storage | SQLite 3.53.2 bundled, application-layer encryption | `rusqlite =0.40.2` with `bundled` |

Argon2id defaults: **m = 256 MiB, t = 3, p = 1** (measured ≈0.75 s on a 4-vCPU Linux
cloud runner). Minimum accepted when creating a vault: m = 64 MiB, t = 3 (RFC 9106 second
recommended option). Maximum accepted when *opening* a vault: m ≤ 2 GiB, t ≤ 10, p ≤ 16,
so a tampered header cannot cause a denial of service. The library itself allows up to
`u32::MAX`, so MaxonKey enforces these limits.

### 3. Storage layout

SQLite stores **only ciphertext and non-sensitive structure**:

- `vault_header` (one row): format version and random 16-byte vault id.
- `key_slots`: per slot (passphrase slot, optional recovery slot) the KDF identifier,
  parameters, salt, and wrapped VK. Plaintext but authenticated: every header and slot
  field is bound into the AEAD *associated data* (AAD) of the slot it protects. Keeping KDF
  settings per slot means a passphrase change never invalidates the recovery slot.
- `records`: random 16-byte record id, `meta_blob`, `value_blob`. All user-supplied text —
  project, environment, secret name, notes, timestamps — lives inside `meta_blob`.
  Secret values live in `value_blob`.
- `manifest` (one row): one encrypted blob listing the vault generation counter and every
  record id with the SHA-256 digest of its stored blobs.

Each blob = `format byte ‖ 24-byte nonce ‖ ciphertext ‖ 16-byte tag`. The AAD is a fixed-length
concatenation of a domain label, vault id, record id, and field tag, so a ciphertext copied to
another row, field, or vault fails authentication. Plaintexts are padded to a 64-byte multiple
to reduce length leakage.

Metadata blobs are decrypted at unlock into an in-memory index (searching works without a
plaintext index on disk). Value blobs are decrypted only when a value is explicitly requested.

### 4. Custody while unlocked

- **Task 002 (no broker)**: every CLI command that needs the vault prompts for the passphrase
  on the controlling terminal, does its work, wipes keys, and exits. Nothing stays unlocked
  between commands. The TUI only shows vault status; TUI unlock comes later with an idle auto-lock.
- **Future broker (MCP task)**: one local broker process holds VK; CLI, TUI, and MCP become
  clients over a user-only local socket / named pipe. See
  [architecture](../architecture.md#future-local-broker-cli-tui-and-mcp).

## Alternatives considered

| Topic | Alternative | Why not chosen (now) |
| --- | --- | --- |
| Encryption layer | **SQLCipher** (whole-file page encryption, `bundled-sqlcipher`) | Hides all metadata including row counts, but adds a C crypto provider (OpenSSL on Windows), uses PBKDF2 unless given a raw key, gives no record-level rollback detection, and keeps every page plaintext in the SQLite page cache while open. Reasonable fallback if the owner rates structure leakage above build simplicity. |
| Encryption layer | **One encrypted file** (whole vault serialized and sealed, KeePass-style) | Simplest consistency model, but every unlock decrypts every secret value into memory and every write rewrites the file. Per-record encryption keeps values encrypted until requested. |
| AEAD | **AES-256-GCM** (`aes-gcm =0.11.1`) | Equally standard; 96-bit random nonces have a lower safe-message bound and constant-time speed depends on AES-NI. No advantage for a local vault. |
| AEAD | **AES-256-GCM-SIV** | Nonce-misuse resistant, but random 192-bit nonces already remove the reuse risk; less mature crate usage. |
| AEAD/KDF provider | **ring / aws-lc-rs / libsodium** | Mature, but C/assembly build dependencies (CMake/NASM on Windows) for no security gain here. |
| KDF | **scrypt / PBKDF2** | Argon2id is the current recommendation (RFC 9106, OWASP) for memory-hard password hashing. |
| Custody | **VK or KEK in the OS keychain** (Credential Manager/DPAPI, Secret Service, macOS Keychain) for password-less unlock | Any process running as the owner — including an AI agent — can read these items without the human. This removes the only barrier MaxonKey has against same-user agents. Rejected for Task 002. |
| Custody | **Hardware-bound unlock** (Windows Hello/TPM, FIDO2 `hmac-secret`, Secure Enclave) | Strongest owner-presence signal; platform-specific and larger effort. Candidate future key slot, not in Task 002. |
| Recovery | **No recovery at all** | Simpler, but a forgotten passphrase means permanent loss. Offered as an opt-out instead. |
| Integrity | **No manifest** (per-record AEAD only) | Forgery and row-swapping are still caught, but deleting a record or replaying an older record version is not. The manifest is one extra encrypted row. |

## Consequences

- Positive: standard constructions only; pure-Rust crypto with no C crypto toolchain;
  passphrase change is fast; values stay encrypted in memory until requested; tampering,
  row swapping, record deletion, and per-record rollback are detected.
- Negative: SQLite reveals the number of records, approximate sizes, and file modification
  time. Whole-file rollback to an older genuine copy cannot be detected without trusted
  external state. Forgetting both the passphrase and the recovery key loses the vault.
- Old copies and backups remain openable with the passphrase (or recovery key) that was valid
  when they were made. Rotating the passphrase does not revoke old copies; rotating VK
  (full re-encryption) is a separate, explicit operation.
- Any change to algorithms, key hierarchy, or custody requires a new ADR.
