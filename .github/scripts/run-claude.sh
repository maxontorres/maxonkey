#!/usr/bin/env bash
# Run Claude Code in the current repository with no GitHub credentials. This step only runs
# Claude; the next step (handover.sh), which never receives the Claude credential, packages
# the result. Runs only in a Claude job of .github/workflows/claude.yml, whose token is
# read-only and is never placed in this environment. Needs PROMPT_FILE, IN, OUT, LOG,
# CLAUDE_TOOLS, CLAUDE_DENIED, CLAUDE_SETTINGS, and CLAUDE_CODE_OAUTH_TOKEN or
# ANTHROPIC_API_KEY.
set -uo pipefail

# Backstop: the workflow never passes a GitHub token to this step.
unset GH_TOKEN GITHUB_TOKEN
[ -n "${ANTHROPIC_API_KEY:-}" ] || unset ANTHROPIC_API_KEY
[ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ] || unset CLAUDE_CODE_OAUTH_TOKEN
# Settings, hooks, MCP servers, and CLAUDE.md files come only from this workflow, never from
# the checked-out branch (a pull request branch may change .claude/, .mcp.json, CLAUDE.md).
export CLAUDE_CODE_DISABLE_CLAUDE_MDS=1
exec claude -p "$(cat "$PROMPT_FILE")" --verbose --output-format stream-json \
  --setting-sources user --strict-mcp-config \
  --add-dir "$IN" --add-dir "$OUT" \
  --allowedTools "$CLAUDE_TOOLS" --disallowedTools "$CLAUDE_DENIED" \
  --settings "$CLAUDE_SETTINGS" > "$LOG"
