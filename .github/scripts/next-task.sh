#!/usr/bin/env bash
# Start the oldest approved task when no other Claude work is in progress.
# Approved = open issue written by the owner, labeled claude:implement, not needs-owner.
# claude.yml re-checks approval (who applied the label) before Claude runs.
# Runs only from trusted default-branch code. Needs GH_TOKEN, REPO, OWNER.
set -euo pipefail

busy=$(gh api "repos/$REPO/pulls?state=open&per_page=100" --jq \
  '[.[] | select(.user.login == "claude[bot]" and (.labels | any(.name == "needs-owner") | not))] | length')
# The calling run never counts: claude.yml calls this from a run that is about to end.
running=$(gh api "repos/$REPO/actions/workflows/claude.yml/runs?per_page=20" --jq \
  "[.workflow_runs[] | select(.id != ${GITHUB_RUN_ID:-0} and .status != \"completed\" and (.event == \"issues\" or .event == \"workflow_dispatch\"))] | length")
if [ "$busy" != 0 ] || [ "$running" != 0 ]; then
  echo "busy: $busy open Claude pull requests, $running Claude runs in progress"; exit 0
fi

next=$(gh api "repos/$REPO/issues?state=open&labels=claude:implement&creator=$OWNER&sort=created&direction=asc&per_page=50" \
  --jq '[.[] | select(.pull_request == null and (.labels | any(.name == "needs-owner") | not)) | .number] | .[0] // empty')
if [ -z "$next" ]; then echo "queue empty"; exit 0; fi
if [ "$(gh api "repos/$REPO/pulls?state=all&head=${REPO%%/*}:claude/issue-$next" --jq length)" != 0 ]; then
  echo "issue #$next already has a pull request"; exit 0
fi
gh workflow run claude.yml --repo "$REPO" -f issue="$next" -f attempt=1
echo "started issue #$next"
