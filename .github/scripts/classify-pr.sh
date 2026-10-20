#!/usr/bin/env bash
# Classify a pull request's changed files. Run only from trusted base-branch code.
# Needs GH_TOKEN, REPO, NUMBER. Prints "eligible=true|false", then one
# "sensitive=<path>" line per security-sensitive path. Routine changes (no sensitive
# path) are eligible for automatic merge. Keep this list in sync with .github/CODEOWNERS.
set -euo pipefail
files=$(gh api --paginate "repos/$REPO/pulls/$NUMBER/files" \
  --jq '.[] | .filename, (.previous_filename // empty)' | sort -u)
eligible=true
[ -n "$files" ] || eligible=false
sensitive=""
while IFS= read -r f; do
  [ -n "$f" ] || continue
  case "$f" in
    src/crypto.rs|src/secret.rs|src/vault.rs|src/store.rs|src/paths.rs|src/prompt.rs|\
    src/commands.rs|src/main.rs|src/cli.rs|tests/cli.rs|tests/vault_pty.py|\
    tests/commit_msg.sh|tests/spec_check.py|Cargo.toml|Cargo.lock|deny.toml|\
    rust-toolchain.toml|build.rs|.cargo/*|.github/*|.githooks/*|.claude/*|.mcp.json|\
    .agents/*|.specify/memory/*|.gitleaks*|AGENTS.md|CLAUDE.md|docs/security.md|\
    docs/adr/*|specs/*/spec.md|*mcp*|*auth*|*recover*|*broker*|*permission*|*secret*|\
    *crypt*|*key*)
      eligible=false
      sensitive="${sensitive}sensitive=$f"$'\n' ;;
  esac
done <<< "$files"
echo "eligible=$eligible"
printf '%s' "$sensitive"
