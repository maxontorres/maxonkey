#!/usr/bin/env bash
# Decide the next step for one pull request: wait, request a Codex review, start a
# Claude correction, merge a low-risk change, or tell the owner it is ready or blocked.
# Runs only from trusted default-branch code (.github/workflows/factory.yml).
# Needs GH_TOKEN, REPO, OWNER, NUMBER, CODEX_LOGIN, CODEX_REVIEW (required|off).
set -euo pipefail

REQUIRED_CHECKS=("Verify (ubuntu-latest)" "Verify (windows-latest)" "Repository policy" "PR metadata")
STATUS_MARKER="<!-- maxonkey-factory-status"
CODEX_MARKER="<!-- maxonkey-codex-request"

pr=$(gh api "repos/$REPO/pulls/$NUMBER")
field() { jq -r "$1" <<< "$pr"; }
if [ "$(field .state)" != open ] || [ "$(field .draft)" = true ] \
   || [ "$(field .head.repo.full_name)" != "$REPO" ] || [ "$(field .base.ref)" != main ]; then
  echo "skip: not an open, ready, same-repository pull request into main"; exit 0
fi
head=$(field .head.sha)
branch=$(field .head.ref)
author=$(field .user.login)
claude_pr=false
[ "$author" = "claude[bot]" ] && [[ "$branch" == claude/* ]] && claude_pr=true

comments=$(gh api --paginate "repos/$REPO/issues/$NUMBER/comments" | jq -s 'add // []')

# Post one status comment per state and commit, replacing the previous one.
notify() {
  local state=$1 text=$2 tag="$STATUS_MARKER state=$1 sha=$head -->" old
  if jq -e --arg t "$tag" 'any(.[]; .body | startswith($t))' <<< "$comments" >/dev/null; then
    return 0
  fi
  for old in $(jq -r --arg m "$STATUS_MARKER" \
      '.[] | select(.user.login == "github-actions[bot]" and (.body | startswith($m))) | .id' <<< "$comments"); do
    gh api -X DELETE "repos/$REPO/issues/comments/$old" >/dev/null || true
  done
  gh api "repos/$REPO/issues/$NUMBER/comments" -f body="$tag
@$OWNER $text" >/dev/null
  echo "notified: $state"
}

# Latest run of every check on the head commit.
runs=$(gh api --paginate "repos/$REPO/commits/$head/check-runs?per_page=100" \
  | jq -s '[.[].check_runs[]] | group_by(.name) | map(max_by(.started_at))')
ci=passed
for name in "${REQUIRED_CHECKS[@]}"; do
  state=$(jq -r --arg n "$name" '(.[] | select(.name == $n) | if .status != "completed" then "pending" else .conclusion end) // "pending"' <<< "$runs")
  case "$state" in
    success) ;;
    pending) [ "$ci" = failed ] || ci=pending ;;
    *) ci=failed ;;
  esac
done
if jq -e 'any(.[]; .status == "completed" and (.conclusion | IN("failure","timed_out","cancelled","action_required","startup_failure")))' <<< "$runs" >/dev/null; then
  ci=failed
fi

# Codex review state for the head commit, from Codex's own summary comment.
codex=passed
if [ "$CODEX_REVIEW" != off ]; then
  review=$(jq -c --arg c "$CODEX_LOGIN" '[.[] | select(.user.login == $c) | .body
      | gsub("&#123;"; "{") | gsub("&#125;"; "}")
      | capture("codex-review-state:v1 (?<j>\\{.*\\}) -->") | .j | fromjson | .code_review] | last // {}' <<< "$comments")
  if [ "$(jq -r '.commit_sha // ""' <<< "$review")" != "$head" ]; then
    codex=missing
  elif [ "$(jq -r '.status' <<< "$review")" != completed ]; then
    codex=pending
  elif [ "$(jq -r 'if .has_unresolved_findings == null then (.has_any_findings // false) else .has_unresolved_findings end' <<< "$review")" = true ]; then
    codex=findings
  fi
fi
echo "pr=#$NUMBER head=$head author=$author ci=$ci codex=$codex"

# Required check for manual merges: the head commit must have a clean independent review.
case "$codex" in
  passed) rstate=success; rdesc=$([ "$CODEX_REVIEW" = off ] && echo "Codex review disabled" || echo "Codex reviewed this commit with no open findings") ;;
  findings) rstate=failure; rdesc="Codex reported findings on this commit" ;;
  *) rstate=pending; rdesc="Waiting for a Codex review of this commit" ;;
esac
gh api "repos/$REPO/statuses/$head" -f state="$rstate" -f context="Independent review" -f description="$rdesc" >/dev/null

if [ "$ci" = failed ] || [ "$codex" = findings ]; then
  reason=$([ "$ci" = failed ] && echo ci || echo codex)
  if [ "$claude_pr" = true ]; then
    # claude.yml enforces the two-cycle limit and skips a commit it already corrected.
    gh workflow run claude.yml --repo "$REPO" -f pr="$NUMBER" -f reason="$reason"
    echo "dispatched correction: $reason"
  else
    notify blocked "$([ "$reason" = ci ] && echo "CI failed" || echo "Codex reported findings") on \`${head:0:7}\`. Automation does not change pull requests it did not open."
  fi
  exit 0
fi
[ "$ci" = passed ] || { echo "waiting for CI"; exit 0; }

if [ "$codex" = missing ]; then
  # A new commit does not start a Codex review by itself; request one once per commit.
  if ! jq -e --arg t "$CODEX_MARKER sha=$head -->" 'any(.[]; .body | contains($t))' <<< "$comments" >/dev/null; then
    gh api "repos/$REPO/issues/$NUMBER/comments" -f body="@codex review
$CODEX_MARKER sha=$head -->" >/dev/null
    echo "requested Codex review"
  fi
  exit 0
fi
[ "$codex" = passed ] || { echo "waiting for Codex"; exit 0; }

result=$(bash "$(dirname "$0")/classify-pr.sh")
eligible=$(sed -n 's/^eligible=//p' <<< "$result")
if grep -q '^sensitive=' <<< "$result"; then eligible=false; fi
if [ "$eligible" = true ] && { [ "$author" = "$OWNER" ] || [ "$author" = "claude[bot]" ]; }; then
  if gh pr merge "$NUMBER" --repo "$REPO" --rebase --match-head-commit "$head"; then
    echo "merged"
  else
    notify merge-failed "all checks passed but the automatic merge failed; see the pull request for conflicts."
  fi
  exit 0
fi
notify ready "CI and the Codex review passed on \`${head:0:7}\`. This change is not low-risk, so it needs your approval and merge."
