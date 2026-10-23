#!/usr/bin/env bash
# Decide the next step for one pull request: wait, request a Codex review, start a
# Claude correction, merge a low-risk change, or tell the owner it is ready or blocked.
# Runs only from trusted default-branch code (.github/workflows/factory.yml).
# Needs GH_TOKEN, REPO, OWNER, NUMBER, CODEX_LOGIN, CODEX_REVIEW (required|off).
# NO_FALLBACK=1 (after a fallback review) never requests another Codex CLI review.
# FALLBACK_LIST: file that collects fallback review requests, one JSON object per line.
set -euo pipefail

REQUIRED_CHECKS=("Verify (ubuntu-latest)" "Verify (windows-latest)" "Repository policy" "PR metadata")
STATUS_MARKER="<!-- maxonkey-factory-status"
FALLBACK_MARKER="<!-- maxonkey-codex-fallback sha="
FALLBACK_AFTER=1200   # seconds Codex gets after CI passes before Codex CLI reviews instead

pr=$(gh api "repos/$REPO/pulls/$NUMBER")
field() { jq -r "$1" <<< "$pr"; }
if [ "$(field .state)" != open ] || [ "$(field .draft)" = true ] \
   || [ "$(field .head.repo.full_name)" != "$REPO" ] || [ "$(field .base.ref)" != main ]; then
  echo "skip: not an open, ready, same-repository pull request into main"; exit 0
fi
head=$(field .head.sha)
export BASE_REF; BASE_REF=$(field .base.ref)
branch=$(field .head.ref)
author=$(field .user.login)
claude_pr=false
[ "$author" = "claude[bot]" ] && [[ "$branch" == claude/* ]] && claude_pr=true

comments=$(gh api --paginate "repos/$REPO/issues/$NUMBER/comments" | jq -s 'add // []')

# Post one status comment per state and commit, replacing the previous one.
notify() {
  local state=$1 text=$2 tag="$STATUS_MARKER state=$1 sha=$head -->" old
  if jq -e --arg t "$tag" 'any(.[]; .user.login == "github-actions[bot]" and (.body | startswith($t)))' <<< "$comments" >/dev/null; then
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

# Codex CLI fallback result for this commit (posted by codex-fallback.yml).
fallback=""
if [ "$codex" = missing ]; then
  fallback=$(jq -r --arg m "$FALLBACK_MARKER$head" \
    '[.[] | select(.user.login == "github-actions[bot]" and (.body | startswith($m))) | .body | split("\n")[0]] | last // ""' <<< "$comments")
  case "$fallback" in
    *"state=done findings=0 "*) codex=passed ;;
    *"state=done findings="*) codex=findings ;;
  esac
fi

if [ "$codex" = findings ]; then
  # Owner override, bound to this exact commit: "/accept-codex <first 7 of the sha> <reason>".
  # Only the owner account counts; a new commit needs a new decision.
  if jq -e --arg o "$OWNER" --arg p "/accept-codex ${head:0:7}" \
      'any(.[]; .user.login == $o and .author_association == "OWNER" and (.body | startswith($p)))' <<< "$comments" >/dev/null; then
    codex=accepted
  else
    # A finding that cites a commit missing from this repository is unverified: it never
    # starts an automatic correction. Findings without commit references count as verified.
    findings=$(gh api --paginate "repos/$REPO/pulls/$NUMBER/comments" | jq -s --arg c "$CODEX_LOGIN" --arg h "$head" \
      '[add // [] | .[] | select(.user.login == $c and .original_commit_id == $h) | .body]')
    finding_count=$(jq length <<< "$findings")
    verified=false
    for ((k = 0; k < finding_count; k++)); do
      ok=true
      for ref in $(jq -r --argjson k "$k" '.[$k]' <<< "$findings" | grep -oE '`[0-9a-f]{7,40}`' | tr -d '`' | sort -u); do
        gh api "repos/$REPO/commits/$ref" --jq .sha >/dev/null 2>&1 || ok=false
      done
      if [ "$ok" = true ]; then verified=true; fi
    done
    if [ "$finding_count" -gt 0 ] && [ "$verified" = false ]; then codex=unverified; fi
  fi
fi
echo "pr=#$NUMBER head=$head author=$author ci=$ci codex=$codex"

# Required check for manual merges: the head commit must have a clean independent review.
case "$codex" in
  accepted) rstate=success; rdesc="Owner accepted the Codex findings on this commit" ;;
  unverified) rstate=failure; rdesc="Codex finding cites a commit not in this repository; owner decision needed" ;;
  passed) rstate=success; rdesc=$([ "$CODEX_REVIEW" = off ] && echo "Codex review disabled" || echo "Codex reviewed this commit with no open findings") ;;
  findings) rstate=failure; rdesc="Codex reported findings on this commit" ;;
  *) rstate=pending; rdesc="Waiting for a Codex review of this commit" ;;
esac
gh api "repos/$REPO/statuses/$head" -f state="$rstate" -f context="Independent review" -f description="$rdesc" >/dev/null

if [ "$codex" = unverified ] && [ "$ci" != failed ]; then
  notify unverified "Codex reported findings on \`${head:0:7}\` that cite a commit not in this repository, so no automatic correction ran. If they are false positives, comment \`/accept-codex ${head:0:7} <reason>\`; otherwise fix them or comment \`@claude\`."
  exit 0
fi
[ "$codex" = accepted ] && codex=passed
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
  # Codex reviews each push on its own. If it has not reviewed this commit, factory.yml
  # gives it the rest of FALLBACK_AFTER seconds after CI passed, then Codex CLI reviews it
  # in the same run (once per commit; retried while the OPENAI_API_KEY secret is missing).
  case "$fallback" in
    *"state=error"*)
      notify codex-unavailable "Codex has not reviewed \`${head:0:7}\` and the Codex CLI fallback review did not produce a result. Check the Codex integration and the \`OPENAI_API_KEY\` secret."
      exit 0 ;;
  esac
  if [ -n "${NO_FALLBACK:-}" ]; then
    case "$fallback" in
      *"state=unavailable"*)
        notify codex-unavailable "Codex has not reviewed \`${head:0:7}\` and the Codex CLI fallback cannot run: the repository has no \`OPENAI_API_KEY\` secret." ;;
      *) echo "waiting for Codex" ;;
    esac
    exit 0
  fi
  ci_done=$(jq -r 'map(.completed_at // empty) | max // empty' <<< "$runs")
  waited=$(( $(date -u +%s) - $(date -u -d "${ci_done:-now}" +%s) ))
  wait=$(( FALLBACK_AFTER - waited )); [ "$wait" -gt 0 ] || wait=0
  # factory.yml runs one fallback per listed pull request (a push sweep can list several).
  jq -nc --arg pr "$NUMBER" --arg sha "$head" --arg wait "$wait" '{pr: $pr, sha: $sha, wait: $wait}' \
    >> "${FALLBACK_LIST:-/dev/null}"
  echo "Codex CLI fallback review requested (waits ${wait}s for Codex first)"
  exit 0
fi
[ "$codex" = passed ] || { echo "waiting for Codex"; exit 0; }

result=$(bash "$(dirname "$0")/classify-pr.sh")
eligible=$(sed -n 's/^eligible=//p' <<< "$result")
if grep -q '^sensitive=' <<< "$result"; then eligible=false; fi
if [ "$eligible" = true ] && { [ "$author" = "$OWNER" ] || [ "$author" = "claude[bot]" ]; }; then
  if gh pr merge "$NUMBER" --repo "$REPO" --rebase --match-head-commit "$head"; then
    echo "merged"
    bash "$(dirname "$0")/next-task.sh" || true
  else
    state=$(gh api "repos/$REPO/pulls/$NUMBER" --jq .mergeable_state)
    if [ "$claude_pr" = true ] && { [ "$state" = dirty ] || [ "$state" = behind ]; }; then
      # Recoverable: main moved on. Claude merges main into its branch; its push re-runs CI.
      gh workflow run claude.yml --repo "$REPO" -f pr="$NUMBER" -f reason=conflict
      echo "dispatched correction: conflict ($state)"
    else
      notify merge-failed "all checks passed but the automatic merge failed (GitHub merge state: ${state:-unknown}). This is not a conflict automation can fix."
    fi
  fi
  exit 0
fi
notify ready "CI and the Codex review passed on \`${head:0:7}\`. This change is security-sensitive, so it needs your approval and merge."
