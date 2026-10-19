## Summary

<!-- What changed and why, in plain English. -->

Closes #
Spec: specs/

## Verification

<!-- List only checks that actually ran, with their real results. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --all-targets -- -D warnings`
- [ ] `cargo test --locked`
- [ ] Linux terminal tests (`tests/tui_pty.py`, `tests/vault_pty.py`), if behavior changed

## Security impact

<!-- None, or which security-sensitive areas this touches (see AGENTS.md). -->

## Limitations
