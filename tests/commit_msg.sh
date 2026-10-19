#!/bin/sh
# Format acceptance tests; no Git history or user hooks are modified.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
message_file=$(mktemp)
trap 'rm -f "$message_file"' EXIT HUP INT TERM

accept() {
    printf '%s' "$1" > "$message_file"
    sh "$repo_dir/.githooks/commit-msg" "$message_file"
}

reject() {
    printf '%s' "$1" > "$message_file"
    if sh "$repo_dir/.githooks/commit-msg" "$message_file" >/dev/null 2>&1; then
        echo "Invalid commit message was accepted" >&2
        exit 1
    fi
}

for type in Feature Fix Bug Chore Refactor Test Docs Build Security; do
    accept "$type: Describe a specific change"
done
# A single terminating newline is normal for a Git commit message file.
accept 'Docs: Update shared rules
'
reject ''
reject 'feature: Wrong case'
reject 'Other: Unapproved prefix'
reject 'Docs:Missing space'
reject 'Docs:  Extra separator space'
reject 'Docs: '
reject 'Docs: Trailing space '
reject 'Docs: Subject
Body'
reject 'Docs: Subject

'
reject 'Docs: Subject
Co-authored-by: Example <example@example.com>'
reject "$(printf 'Docs: Tab\tinside')"
reject "$(printf 'Docs: CRLF\r\n')"
if sh "$repo_dir/.githooks/commit-msg" >/dev/null 2>&1; then
    echo "Missing message file was accepted" >&2
    exit 1
fi
if sh "$repo_dir/.githooks/commit-msg" "$message_file.missing" >/dev/null 2>&1; then
    echo "Unreadable message file was accepted" >&2
    exit 1
fi
printf '%s\n' 'PASS: commit-message format acceptance tests'
