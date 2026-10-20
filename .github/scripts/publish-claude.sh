#!/usr/bin/env bash
# Publish what a Claude job produced: push its commits to its claude/ branch, then open the
# pull request (implement) and post Claude's comment. Uses the Claude GitHub App token,
# which only this trusted job holds; Claude never runs here. Everything that came from the
# Claude job is untrusted data and is checked before use.
# Needs REPO, MODE (implement|correct), BRANCH, BASE, NUMBER (issue or pull request),
# BUNDLE, TITLE, BODY, COMMENT (base64), and id-token: write for the token exchange.
set -euo pipefail

decode() { printf '%s' "$1" | base64 -d 2>/dev/null || true; }
result() { printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"; }
reject() { result reason "$1"; echo "::error::$1"; exit 1; }

[[ "$BRANCH" =~ ^claude/[A-Za-z0-9._/-]+$ ]] || reject "Branch $BRANCH is not a claude/ branch"
[[ "$BASE" =~ ^[0-9a-f]{40}$ ]] || reject "Missing base commit"
git fetch -q origin main
git fetch -q origin "$BASE" 2>/dev/null || true
git cat-file -e "$BASE^{commit}" || reject "Base commit $BASE is not in the repository"

tip=""
if [ -n "$BUNDLE" ]; then
  decode "$BUNDLE" > "$RUNNER_TEMP/work.bundle"
  git bundle verify -q "$RUNNER_TEMP/work.bundle" >/dev/null 2>&1 || reject "Claude's commits could not be read"
  git fetch -q "$RUNNER_TEMP/work.bundle" HEAD
  tip=$(git rev-parse FETCH_HEAD)
  git merge-base --is-ancestor "$BASE" "$tip" || reject "Claude's commits do not build on ${BASE:0:7}"
  [ "$(git rev-list --count "$BASE..$tip")" -le 100 ] || reject "Claude made more than 100 commits"
  # Agents never change .github/. A file there may differ from the base only where it now
  # matches main (brought in by merging main).
  for path in $(git diff --no-renames --name-only "$BASE" "$tip" -- .github); do
    git diff --quiet origin/main "$tip" -- "$path" || reject "Claude changed $path; agents never edit .github/"
  done
fi

# The Claude GitHub App token, from the same exchange claude-code-action uses. It is
# revoked when this script ends.
oidc=$(curl -sSf -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
  "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=claude-code-github-action" | jq -r .value)
app=$(curl -sSf -X POST -H "Authorization: Bearer $oidc" \
  https://api.anthropic.com/api/github/github-app-token-exchange | jq -r '.token // .app_token // empty')
[ -n "$app" ] || reject "Could not get the Claude GitHub App token"
echo "::add-mask::$app"
trap 'curl -sS -X DELETE -H "Authorization: Bearer $app" "$GITHUB_API_URL/installation/token" >/dev/null || true' EXIT

published=no
if [ -n "$tip" ]; then
  # A normal push: GitHub rejects it unless it only adds commits to the branch.
  git -c "http.https://github.com/.extraheader=AUTHORIZATION: basic $(printf 'x-access-token:%s' "$app" | base64 -w0)" \
    push -q origin "$tip:refs/heads/$BRANCH"
  published=yes
  echo "Pushed ${tip:0:7} to $BRANCH"
fi

if [ "$MODE" = implement ] && [ -n "$tip" ]; then
  pr=$(GH_TOKEN=$app gh pr list --repo "$REPO" --head "$BRANCH" --state open --json number --jq '.[0].number // empty')
  if [ -z "$pr" ]; then
    title=$(decode "$TITLE" | head -1 | cut -c1-200)
    decode "$BODY" > "$RUNNER_TEMP/pr-body.md"
    [ -s "$RUNNER_TEMP/pr-body.md" ] || printf 'Closes #%s\n' "$NUMBER" > "$RUNNER_TEMP/pr-body.md"
    GH_TOKEN=$app gh pr create --repo "$REPO" --base main --head "$BRANCH" \
      --title "${title:-Chore: Implement issue #$NUMBER}" --body-file "$RUNNER_TEMP/pr-body.md"
  fi
fi

if [ -n "$COMMENT" ]; then
  decode "$COMMENT" > "$RUNNER_TEMP/comment.md"
  GH_TOKEN=$app gh api "repos/$REPO/issues/$NUMBER/comments" -F body=@"$RUNNER_TEMP/comment.md" >/dev/null
  published=yes
  echo "Posted Claude's comment on #$NUMBER"
fi
result published "$published"
