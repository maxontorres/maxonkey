#!/usr/bin/env bash
# Run Claude Code in the current repository with no GitHub credentials, then hand its work
# to the publish job as step outputs: its commits since BASE as a git bundle, the files it
# wrote to OUT (pr-title.txt, pr-body.md, comment.md), and a short report.
# Runs only in a Claude job of .github/workflows/claude.yml, whose token is read-only and is
# never placed in this environment. Needs BASE, PROMPT_FILE, IN, OUT, CLAUDE_TOOLS,
# CLAUDE_DENIED, CLAUDE_SETTINGS, and CLAUDE_CODE_OAUTH_TOKEN or ANTHROPIC_API_KEY.
set -uo pipefail

# Claude controls this repository's Git configuration, so never let it run a command here.
git() { command git -c core.fsmonitor=false "$@"; }
# Read a file Claude wrote only if it is a regular file with one link, without following
# links, so it cannot point this trusted step at another file.
read_handoff() {
  python3 -I - "$1" "$2" <<'PY'
import os, stat, sys
try:
    fd = os.open(sys.argv[1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
except OSError:
    sys.exit(0)
st = os.fstat(fd)
if stat.S_ISREG(st.st_mode) and st.st_nlink == 1:
    sys.stdout.buffer.write(os.read(fd, int(sys.argv[2])))
PY
}

# Backstop: the workflow never passes a GitHub token to this step.
unset GH_TOKEN GITHUB_TOKEN
[ -n "${ANTHROPIC_API_KEY:-}" ] || unset ANTHROPIC_API_KEY
[ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ] || unset CLAUDE_CODE_OAUTH_TOKEN
log="$RUNNER_TEMP/claude-log.jsonl"
# Settings, hooks, and MCP servers come only from this workflow, never from the checked-out
# branch (a pull request branch may change .claude/ or .mcp.json).
claude -p "$(cat "$PROMPT_FILE")" --verbose --output-format stream-json \
  --setting-sources user --strict-mcp-config \
  --add-dir "$IN" --add-dir "$OUT" \
  --allowedTools "$CLAUDE_TOOLS" --disallowedTools "$CLAUDE_DENIED" \
  --settings "$CLAUDE_SETTINGS" > "$log"
status=$?
unset ANTHROPIC_API_KEY CLAUDE_CODE_OAUTH_TOKEN
echo "Claude exited with status $status"
# Only committed work is handed over, so anything left uncommitted fails the run.
if [ "$status" = 0 ] && [ -n "$(git status --porcelain)" ]; then
  echo "::error::Claude left uncommitted changes:"; git status --short | head -20
  status=1
fi

# Written last, so these values replace anything Claude wrote to the same outputs.
out() { printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"; }
size=0
if git bundle create "$RUNNER_TEMP/work.bundle" "$BASE..HEAD" 2>/dev/null; then
  bundle=$(base64 -w0 < "$RUNNER_TEMP/work.bundle")
  size=${#bundle}
fi
# GitHub limits a job's outputs to 1 MB in total, counted as UTF-16 (2 bytes per character),
# so the bundle gets 400,000 characters and the other outputs stay under about 60,000.
if [ "$size" -gt 400000 ]; then
  out toobig true
else
  [ "$size" -eq 0 ] || out bundle "$bundle"
fi
for name in pr-title.txt:title:500 pr-body.md:body:20000 comment.md:comment:20000; do
  IFS=: read -r file key limit <<< "$name"
  value=$(read_handoff "$OUT/$file" "$limit" | base64 -w0)
  [ -z "$value" ] || out "$key" "$value"
done

# Denied tool calls show their command; failed calls are only counted, because command
# output is not safe to publish.
report=$(jq -rs '[.[] | select(.type == "result") | .permission_denials[]?
    | "- `\(.tool_name)`: `\((.tool_input.command // .tool_input.file_path // "") | tostring | .[0:150] | gsub("[`\n]"; " "))`"]
    | unique | .[0:10] | if length > 0 then "Blocked tool calls:\n" + join("\n") else "" end' "$log" 2>/dev/null || true)
errors=$(jq -rs '[.[] | select(.type == "user") | .message.content[]?
    | select(.type? == "tool_result" and .is_error == true) | .content | tostring]
    | select(length > 0) | "\(length) tool calls failed"
      + (if any(.[]; test("bwrap")) then ", including sandbox (bubblewrap) errors" else "" end)
      + "; see the log."' "$log" 2>/dev/null || true)
dirty=$([ -n "$(git status --porcelain)" ] && echo "Claude left uncommitted changes, so nothing was published.")
report="Claude exit status: $status${dirty:+$'\n\n'$dirty}${report:+$'\n\n'$report}${errors:+$'\n\n'$errors}"
out report "$(printf '%s' "$report" | head -c 4000 | base64 -w0)"
# A failed run is not published: the outputs above are kept for the owner notice, and the
# failed step sends the publish job to its retry and notice path.
exit "$status"
