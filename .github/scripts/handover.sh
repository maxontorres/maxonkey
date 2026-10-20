#!/usr/bin/env bash
# Hand Claude's work to the publish job as step outputs: its commits since BASE as a git
# bundle, the files it wrote to OUT (pr-title.txt, pr-body.md, comment.md), and a short
# report. Runs in its own step after Claude, with no Claude credential and no GitHub token,
# so Git settings Claude left in the repository have nothing to reach. Everything it
# outputs is untrusted data that the publish job checks. Needs BASE, OUT, LOG, and
# CLAUDE_OUTCOME (the Claude step's outcome).
set -uo pipefail

# Read a file Claude wrote only if it is a regular file with one link, without following
# links, so it cannot point this step at another file.
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
out() { printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"; }

failed=""
[ "$CLAUDE_OUTCOME" = success ] || failed="Claude did not finish (step outcome: $CLAUDE_OUTCOME)."
# Only committed work is handed over.
# Claude controls this repository's Git settings, so check for leftovers in ways those
# settings cannot hide: tracked changes with file modes, untracked files that only the
# committed .gitignore files exclude, and files marked assume-unchanged or skip-worktree.
leftovers() {
  git -c core.fileMode=true status --porcelain --untracked-files=no
  git ls-files --others --exclude-per-directory=.gitignore
  git ls-files -v | grep -E '^([a-z]|S) ' || true
}
if [ -z "$failed" ] && [ -n "$(leftovers)" ]; then
  leftovers | head -20
  failed="Claude left uncommitted changes, so nothing was published."
fi

# The publish job receives the bundle in one environment variable, and Linux limits a single
# variable to 128 KiB, so the encoded bundle gets at most 120,000 characters (about 90 KB of
# compressed commits). All outputs together stay well under GitHub's 1 MB job output limit.
if git bundle create "$RUNNER_TEMP/work.bundle" "$BASE..HEAD" 2>/dev/null; then
  bundle=$(base64 -w0 < "$RUNNER_TEMP/work.bundle")
  if [ "${#bundle}" -gt 120000 ]; then out toobig true; else out bundle "$bundle"; fi
elif [ "$(git rev-parse HEAD)" != "$BASE" ]; then
  # Commits exist but could not be packaged, so nothing may be published as if none existed.
  [ -n "$failed" ] || failed="The commits Claude made could not be packaged for publishing."
fi
for name in pr-title.txt:title:500 pr-body.md:body:20000 comment.md:comment:20000; do
  IFS=: read -r file key limit <<< "$name"
  value=$(read_handoff "$OUT/$file" "$limit" | base64 -w0)
  [ -z "$value" ] || out "$key" "$value"
done

# Denied tool calls show their command; failed calls are only counted, because command
# output is not safe to publish.
denied=$(jq -rs '[.[] | select(.type == "result") | .permission_denials[]?
    | "- `\(.tool_name)`: `\((.tool_input.command // .tool_input.file_path // "") | tostring | .[0:150] | gsub("[`\n]"; " "))`"]
    | unique | .[0:10] | if length > 0 then "Blocked tool calls:\n" + join("\n") else "" end' "$LOG" 2>/dev/null || true)
errors=$(jq -rs '[.[] | select(.type == "user") | .message.content[]?
    | select(.type? == "tool_result" and .is_error == true) | .content | tostring]
    | select(length > 0) | "\(length) tool calls failed"
      + (if any(.[]; test("bwrap")) then ", including sandbox (bubblewrap) errors" else "" end)
      + "; see the log."' "$LOG" 2>/dev/null || true)
report="${failed:-Claude finished.}${denied:+$'\n\n'$denied}${errors:+$'\n\n'$errors}"
out report "$(printf '%s' "$report" | head -c 4000 | base64 -w0)"

# A failed or unclean run is not published: the failed step sends the publish job to its
# retry and notice path.
[ -z "$failed" ] || { echo "::error::$failed"; exit 1; }
