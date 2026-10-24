# Tasks: Task 002 — Encrypted Local Vault

**Spec**: [spec.md](spec.md) · **Plan**: [plan.md](plan.md)

## 002B — Vault lifecycle (complete)

- [x] T001 Add exact-pinned dependencies; read matching-version sources (`Cargo.toml`, `Cargo.lock`)
- [x] T002 `secret.rs`: zeroizing, redacting `Key`, `SecretBytes`, NFC `Passphrase`
- [x] T003 `crypto.rs`: Argon2id KEK with wiped memory, XChaCha20-Poly1305 seal/open, padding, AAD layouts, parameter limits
- [x] T004 `paths.rs`: default path, owner-only directory creation, Unix mode and Windows ACL warnings
- [x] T005 `store.rs`: strict schema, `application_id`/`user_version`, `quick_check`, pragmas, slot replacement transaction
- [x] T006 `vault.rs`: atomic no-clobber init, status, unlock with verification, change passphrase, recovery key format, exit codes
- [x] T007 `prompt.rs` and `commands.rs`: terminal-only input, recovery key written to the terminal
- [x] T008 CLI `vault` subcommands, `--vault`, truthful `status`, About text, TUI Dashboard vault line
- [x] T009 Linux `PR_SET_DUMPABLE=0` at startup
- [x] T010 `deny.toml`; CI runs `tests/vault_pty.py` and checksum-pinned cargo-deny
- [x] T011 Documentation: README, architecture, security, ADR, product, AGENTS, constitution

### Security test traceability (002B)

| Spec test | Where | Status |
| --- | --- | --- |
| ST-01 | `crypto::tests::{argon2id_rfc9106_vector, xchacha20poly1305_draft_vector, frozen_maxonkey_vectors}`; frozen KEK cross-checked with libargon2 (argon2-cffi) | Pass |
| ST-02 (credentials only) | `vault::tests::vault_file_contains_no_credentials`; `vault_pty.py` disk scan | Pass; secret values/names in 002C |
| ST-03 (header, slots, manifest) | `vault::tests::tampering_with_header_slots_or_manifest_is_detected` (every byte, plus integer fields) | Pass; records in 002C |
| ST-04 (slots, manifest) | `vault::tests::blobs_from_another_vault_are_rejected` | Pass; records in 002C |
| ST-06 | `vault::tests::wrong_credentials_fail_generically_and_leave_file_unchanged`; `vault_pty.py` | Pass |
| ST-07 | `vault::tests::excessive_kdf_parameters_are_refused`; `crypto::tests::kdf_parameter_limits` | Pass |
| ST-08 | `crypto::tests::nonces_are_unique` | Pass |
| ST-10 | `vault::tests::init_never_overwrites`; `tests/cli.rs`; `vault_pty.py` | Pass |
| ST-11 | `cli::tests::credential_options_do_not_exist`; `tests/cli.rs`; `vault_pty.py` (`MAXONKEY_PASSPHRASE`) | Pass |
| ST-12 | `vault_pty.py` (new session without a controlling terminal) | Pass (Linux) |
| ST-13 | `paths::tests::private_dir_is_owner_only_and_broad_modes_warn`; `vault_pty.py` under umask 000 | Pass (Linux) |
| ST-14 | `secret::tests::debug_output_is_redacted`; `vault::tests::errors_never_contain_credentials` | Partial: forced-panic check in 002C |
| ST-15 | `secret::tests::key_zeroize_clears_memory` (drop wiping via `ZeroizeOnDrop`) | Pass |
| ST-17 | `vault::tests::change_passphrase_rewraps_only_the_passphrase_slot`; `vault_pty.py` | Pass |
| ST-19 | `vault::tests::nfc_and_nfd_passphrases_open_the_same_vault`; `secret::tests::passphrase_is_nfc_normalized` | Pass |
| ST-20 | `vault::tests::unknown_versions_and_foreign_files_are_refused` | Pass |
| SC-002 | `vault_pty.py` median unlock 0.64 s on the Linux dev container | Linux measured; Windows runner not measured (no Windows terminal test) |

## 002C — Secrets, integrity manifest, backups (in progress)

- [x] T101 Add `sha2 =0.11.1`; record blob layouts and AAD (purpose 3); manifest entries with digests
- [x] T102 In-memory metadata index; `secret set|get|list|delete` (FR-003, SR-020–SR-023)
- [ ] T103 Transactional writes updating records and manifest (SR-012); crash-injection tests (ST-09)
- [ ] T104 `vault rotate-key` (ST-18)
- [ ] T105 `vault backup|verify|restore`, automatic backups before migration/restore (ST-16)
- [ ] T106 Remaining ST-02, ST-03, ST-04, ST-05, ST-14 coverage for records; SC-003 timing
- [ ] T107 Documentation and capability status updates
