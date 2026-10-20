#!/usr/bin/env bash
# Classify a pull request's changed files. Run only from trusted base-branch code.
# Needs GH_TOKEN, REPO, NUMBER, BASE_REF. Prints "eligible=true|false", then one
# "sensitive=<path>" line per security-sensitive path.
set -euo pipefail
files=$(gh api --paginate "repos/$REPO/pulls/$NUMBER/files" \
  --jq '.[] | .filename, (.previous_filename // empty)' | sort -u)
# Low-risk paths are the unowned lines of CODEOWNERS on the base branch,
# so a pull request cannot widen its own eligibility.
lowrisk=$(gh api -H 'Accept: application/vnd.github.raw' \
  "repos/$REPO/contents/.github/CODEOWNERS?ref=$BASE_REF" 2>/dev/null \
  | grep -E '^/[^[:space:]]+$' | cut -c2- || true)
eligible=true
[ -n "$files" ] || eligible=false
sensitive=""
while IFS= read -r f; do
  [ -n "$f" ] || continue
  printf '%s\n' "$lowrisk" | grep -Fxq -- "$f" || eligible=false
  case "$f" in
    src/crypto.rs|src/secret.rs|src/vault.rs|src/store.rs|src/paths.rs|src/prompt.rs|\
    src/commands.rs|src/main.rs|src/cli.rs|tests/cli.rs|Cargo.toml|Cargo.lock|deny.toml|\
    rust-toolchain.toml|.github/*|.githooks/*|.claude/*|.mcp.json|.agents/*|.specify/memory/*|\
    AGENTS.md|CLAUDE.md|docs/security.md|docs/adr/*|specs/*/spec.md|tests/vault_pty.py|\
    *mcp*|*auth*|*recover*|*broker*|*permission*|*secret*|*crypt*|*key*)
      sensitive="${sensitive}sensitive=$f"$'\n' ;;
  esac
done <<< "$files"
echo "eligible=$eligible"
printf '%s' "$sensitive"
