# Security model

**Status**: Approved by the owner on 2026-10-19. Task 002B implements the key hierarchy,
passphrase and recovery-key slots, vault file integrity checks, terminal-only credential
input, memory wiping, and Linux process hardening. Task 002C so far adds encrypted secret
records with a verified integrity manifest and the `secret` commands; key rotation and
backups follow, and the broker and MCP come in a later task. Statements marked "planned"
or "future" do not protect anything yet.

Related: [architecture](architecture.md) · [ADR 0001 — key custody and storage](adr/0001-key-custody-and-storage.md) ·
[Spec 002 — encrypted vault](../specs/002-encrypted-vault/spec.md)

## Contents

- [Current guarantees](#current-guarantees)
- [Confirmed requirements](#confirmed-requirements)
- [Security goals and non-goals](#security-goals-and-non-goals)
- [Assets and trust boundaries](#assets-and-trust-boundaries)
- [Threat model](#threat-model)
- [What MaxonKey cannot protect against](#what-maxonkey-cannot-protect-against)
- [AI agents running as the owner](#ai-agents-running-as-the-owner)
- [Cryptography summary](#cryptography-summary)
- [Secret handling rules](#secret-handling-rules)
- [Platform notes](#platform-notes)
- [Decisions still open](#decisions-still-open)

## Current guarantees

MaxonKey can create a vault, change its passphrase, and recover it with the recovery key.
It stores secrets by project, environment, and name with `maxonkey secret set|get|list|delete`;
every command asks for the passphrase, verifies every record against the encrypted manifest,
and locks again before it exits. Values reach standard output only through `secret get`.
Vault-key rotation, backup, verify, and restore are not implemented yet. It has no MCP server, clipboard access, telemetry, or
networking. Status output shows `NOT INITIALIZED` or `INITIALIZED (LOCKED)` and never
claims the vault is unlocked, because no process keeps it open between commands.

## Confirmed requirements

- Local-first offline vault with encryption, single owner, application data separate from
  the executable.
- Explicit owner approval before every MCP plaintext read, creation, update, or deletion.
- No secret material in logs, error messages, tests, or development artifacts.
- Future secure sharing requires a separately reviewed specification.

## Security goals and non-goals

**Goals** (planned, Task 002 onward):

1. A copied vault file or backup reveals no secret values and no user-written names
   (projects, environments, secret names, notes) to someone without the passphrase or
   recovery key.
2. Guessing the passphrase offline is expensive (memory-hard Argon2id).
3. Any modification of the vault file is detected; MaxonKey never returns forged or swapped data.
4. Plaintext secrets exist only in process memory, only while needed, and are wiped after use.
5. Secrets do not leak through command-line arguments, environment variables, shell history,
   logs, error messages, or crash output produced by MaxonKey.
6. AI agents cannot read or change secrets through MaxonKey's own interfaces without the
   owner's explicit, per-request approval.

**Non-goals**: multi-user accounts, servers, cloud sync, protecting against a compromised
operating system or administrator, protecting secrets after the owner has revealed them to
another program, and guaranteed physical erasure on SSDs.

## Assets and trust boundaries

| Asset | Where it lives |
| --- | --- |
| Secret values | Encrypted in `records.value_blob`; plaintext only in memory on demand |
| Secret metadata (names, projects, environments, notes, timestamps) | Encrypted in `records.meta_blob`; plaintext index in memory while unlocked |
| Master passphrase | Only in the owner's head and briefly in memory during unlock |
| Vault Key (VK) | Wrapped in the header; plaintext only in memory while unlocked |
| Recovery key (optional) | Owner's offline storage (paper, separate password manager) |

Trust boundaries:

- **Trusted**: the owner at the keyboard; the OS kernel and administrator; the MaxonKey binary
  as built from this repository.
- **Not trusted**: anyone with a copy of the vault file or backups; other OS users; network
  peers; AI agents and their tool calls; MCP clients; any program the owner did not
  deliberately give a secret to.
- **Same-user processes** (including AI agents): not trusted, but **not isolated by the OS**
  from MaxonKey. See [AI agents running as the owner](#ai-agents-running-as-the-owner).

## Threat model

"Protected" means MaxonKey's design addresses it. "Partial" means it raises cost or limits
damage but does not stop a determined attacker. "No" means outside MaxonKey's control.

| # | Threat | Protected? | Mitigation | Remaining risk |
| --- | --- | --- | --- | --- |
| T1 | **Stolen vault database or backup** (lost laptop, copied file, cloud folder) | Yes, for content | All values and user metadata encrypted with XChaCha20-Poly1305 under a random 256-bit VK; VK wrapped by Argon2id-derived KEK | Record count, approximate sizes, KDF parameters, file times are visible. Weak passphrase → T2. |
| T2 | **Offline passphrase guessing** | Partial | Argon2id m=256 MiB, t=3; 16-byte salt; minimum passphrase length 12 characters; guidance to use a 4+ word passphrase | A short or reused passphrase can still be guessed. No rate limit is possible offline. |
| T3 | **Compromised or prompt-injected AI coding agent** using MaxonKey's interfaces (CLI, MCP) | Yes, while locked; Partial while unlocked | Passphrase entered only on a terminal prompt, never via arguments/env/MCP; in the future broker, every plaintext read and every mutation needs a fresh owner approval in a broker-owned prompt; approvals single-use, request-bound, expiring | An agent that actively attacks the OS session (see T3b) is not stopped. |
| T3b | **Agent with arbitrary code execution under the owner's account** attacking MaxonKey itself (reading process memory, injecting keystrokes, replacing the binary, keylogging) | No (hardening only) | Linux: non-dumpable process blocks same-user ptrace and `/proc/<pid>/mem`; locked by default; short unlock windows; documentation recommends running agents under a separate OS account, container, or VM | Same-user code can replace `maxonkey`, fake its prompt, or (on Windows) read its memory. This is an OS-level boundary MaxonKey cannot create. |
| T4 | **Accidental secret disclosure** (screen sharing, pasting, logs, crash dumps) | Partial | Values hidden unless explicitly revealed; secret types redact `Debug`/`Display`; errors never contain plaintext; no clipboard in Task 002; Linux core dumps disabled | Once revealed on screen or to stdout, MaxonKey cannot recall it. |
| T5 | **Unauthorized MCP requests** (future) | Yes, by design | Broker-side approval for reads/mutations; MCP never receives the passphrase or VK; default deny on timeout; request binding and replay prevention; MCP disabled unless owner enables it | Owner approving a malicious request ("approval fatigue"). |
| T6 | **Terminal history and logging leaks** | Yes, for MaxonKey's own inputs/outputs | Secrets and passphrases never accepted as arguments or environment variables; hidden prompts; stdin only with an explicit flag; no logging of values or names | Values the owner pipes in from other tools, or prints to a logged terminal, can be recorded by those tools. |
| T7 | **Database tampering** (bit flips, edited rows, swapped ciphertexts, deleted rows) | Yes, detected | AEAD with AAD binding vault id, record id, field; encrypted manifest of record digests; header bound into key-slot AAD; `PRAGMA quick_check` on open | Detection only: a damaged vault must be restored from backup. |
| T8 | **Rollback** to an older genuine vault file | Partial | Per-record rollback detected by the manifest; owner sees generation counter and last-modified time after unlock | Replacing the *whole* file with an older genuine copy is undetectable without trusted external state. |
| T9 | **Untrusted computer** (shared, borrowed, or infected machine) | No | Documentation: never unlock on a machine you do not control | Keyloggers capture the passphrase; memory can be read. |
| T10 | **Forgotten master passphrase** | Partial | Optional recovery key slot created at initialization; clear warnings | Without the passphrase and recovery key, the vault is unrecoverable by design. There is no backdoor. |
| T11 | **Other local OS users** | Yes | Data directory and files are owner-only (Linux `0700`/`0600`; Windows per-user `%LOCALAPPDATA%` ACL); future broker endpoint owner-only | Administrators/root can read everything. |
| T12 | **Swap, hibernation, and memory remnants** | Partial | Zeroize keys and plaintext after use; keep values encrypted until requested; short unlock lifetime | Pages may reach swap or hibernation files before wiping. Memory locking is a recorded open decision. |
| T13 | **Fake MaxonKey prompt (phishing)** by a malicious tool or agent | Partial | Documentation: MaxonKey never asks for the passphrase through an AI chat, MCP, or a web page | A convincing fake terminal prompt cannot be technically prevented. |
| T14 | **Malicious dependency or build** | Partial | Exact version pins, committed `Cargo.lock`, `--locked` builds, RustCrypto crates, CI checks; dependency review before upgrades | Supply-chain compromise of a pinned crate is possible; `cargo audit`/`cargo deny` should run in CI (see spec). |

## What MaxonKey cannot protect against

Stated plainly, so nobody relies on protection that does not exist:

- A **compromised operating system**, kernel, administrator, or root account.
- **Malware or an AI agent with arbitrary code execution under the owner's account** that
  actively attacks MaxonKey (keyloggers, memory readers, replaced binaries, fake prompts).
  MaxonKey can make this harder; it cannot prevent it.
- **Secrets after release**: once a value is printed, piped, or given to another program,
  that program and its logs are outside MaxonKey's control.
- A **weak, reused, or disclosed passphrase**.
- An **untrusted computer**, including hardware keyloggers and screen capture.
- **Physical deletion guarantees** on SSDs and copy-on-write filesystems (deleted ciphertext
  may persist; it stays encrypted).
- **Whole-vault rollback** to an older genuine copy.
- **Recovery** without the passphrase or the recovery key.

## AI agents running as the owner

This is MaxonKey's central design constraint.

An AI coding agent with shell access runs as the owner's OS user. To the operating system it
*is* the owner: it can read the owner's files, the OS keychain entries the owner can read,
and environment variables; it can start `maxonkey`; and on Windows it can open other
same-user processes. File permissions and keychains therefore do **not** separate the
owner from the agent.

What *does* separate them is knowledge and presence:

1. **The passphrase is the gate.** VK is never stored where the owner's account can read it
   without the passphrase (no keychain auto-unlock — see ADR 0001). A locked vault is safe
   from a same-user agent, short of keylogging.
2. **No secret on the command line or in the environment.** Arguments and environment
   variables are visible to same-user processes (`/proc/<pid>/cmdline`, `/proc/<pid>/environ`,
   process listings) and land in shell history.
3. **Interactive-only passphrase entry.** The passphrase is read from the controlling
   terminal (`/dev/tty`, `CONIN$`), not stdin. An agent invoking `maxonkey` cannot answer
   the prompt without knowing the passphrase.
4. **Approval for every sensitive agent request** (future broker): MCP and other
   non-interactive clients get plaintext or make changes only after the owner approves
   that specific request in a prompt owned by the broker, not by the requesting client.
5. **Honest residual risk**: if the agent can execute arbitrary code, it can wait for the
   owner to unlock and then attack the unlocked process or fake a prompt. The
   recommended mitigation is OS-level isolation: run agents under a separate OS account,
   container, WSL instance, or VM that cannot reach the owner's MaxonKey data or broker.

## Cryptography summary

Details and alternatives are in [ADR 0001](adr/0001-key-custody-and-storage.md).

- XChaCha20-Poly1305 (`chacha20poly1305 =0.11.0`) with a fresh random 24-byte nonce for every
  encryption, including every rewrite of the same record.
- Argon2id v1.3 (`argon2 =0.6.0`), default m=256 MiB, t=3, p=1; creation floor m=64 MiB, t=3;
  open-time ceiling m≤2 GiB, t≤10, p≤16.
- Random 256-bit Vault Key from the OS RNG (`getrandom =0.4.3`), wrapped per key slot
  (passphrase slot, optional recovery slot).
- SHA-256 (`sha2 =0.11.1`) digests inside the encrypted manifest.
- Associated data binds every ciphertext to its vault, record, and field.
- No custom algorithms, no hand-written primitives, no unauthenticated encryption, and no
  "encrypt-then-compare" password checks: a wrong passphrase is detected only by key-slot
  authentication failure.

## Secret handling rules

These apply to all future code, tests, and documentation:

- Never accept secret values or passphrases as command-line arguments or environment variables.
- Never write secret values, passphrases, keys, or secret names to logs, error messages,
  panic messages, or test snapshots. Wrapper types must redact `Debug` and `Display`.
- Wipe keys, passphrases, and decrypted values with `zeroize` when no longer needed.
- Reveal a value only on an explicit owner command; do not show values by default.
- No clipboard support until a clipboard policy is approved (auto-clear, Windows clipboard
  history exclusion).
- Tests use generated canary values only (for example `MK-CANARY-<random>`), never real secrets.

## Platform notes

| Topic | Windows (first) | Linux | macOS (future) |
| --- | --- | --- | --- |
| Data location | `%LOCALAPPDATA%\MaxonKey\data\` (not roaming) | `$XDG_DATA_HOME/maxonkey/` or `~/.local/share/maxonkey/` | `~/Library/Application Support/MaxonKey/` |
| File protection | Per-user profile ACL (owner, SYSTEM, Administrators); verify no broad grants | Directory `0700`, files `0600`, set at creation and verified on open | POSIX modes as Linux |
| Same-user memory access | Same-user processes can usually open MaxonKey with read access; no hard defence | `PR_SET_DUMPABLE=0` blocks same-user ptrace and `/proc/<pid>/mem`; Yama `ptrace_scope` helps | Hardened Runtime blocks debugger attach for signed builds |
| Keystroke injection | Same-user processes can inject console input | `TIOCSTI` disabled by default on recent kernels | Accessibility permission (TCC) required |
| Local IPC (future broker) | Named pipe with owner-only DACL, reject remote clients, first-instance flag | Unix socket in `$XDG_RUNTIME_DIR` (`0700`), peer credentials check | Unix socket, `getpeereid` |
| Shell history | PSReadLine saves typed commands to `ConsoleHost_history.txt` | Bash/Zsh history files | Zsh history |
| Swap/hibernation | Pagefile and `hiberfil.sys` | Swap (often unencrypted) | Encrypted swap by default |

## Decisions still open

The approval table in [Spec 002](../specs/002-encrypted-vault/spec.md#owner-approval-checkpoint)
lists every decision that needs the owner's approval before implementation. Deferred to
later specifications: clipboard policy, MCP transport/authentication and approval UI,
audit log design, hardware-backed key slots, and encrypted sharing.
