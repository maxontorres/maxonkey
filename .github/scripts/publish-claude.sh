#!/usr/bin/env bash
# Publish what a Claude job produced: push its commits to its claude/ branch, then open the
# pull request (implement) and post Claude's comment. Uses the Claude GitHub App token,
# which only this trusted job holds; Claude never runs here. Everything that came from the
# Claude job is untrusted data and is checked before use.
# Needs REPO, MODE (implement|correct), BRANCH, BASE, NUMBER (issue or pull request),
# BUNDLE, TITLE, BODY, COMMENT (base64), GH_TOKEN (workflow token, read only here), and
# id-token: write for the token exchange.
set -euo pipefail
# Paths below are file names, never pathspec patterns.
export GIT_LITERAL_PATHSPECS=1
BOT_NAME="claude[bot]"
# Same patterns as the Repository policy and PR metadata checks (ci.yml, pr-policy.yml).
ATTRIBUTION='co-authored-by:|(generated|written|created|made|authored) (with|by|using) \[?(the )?(claude|codex|chatgpt|gpt|copilot|openai|anthropic|(an? )?ai)\b|ai[- ]generated|🤖'

decode() { printf '%s' "$1" | base64 -d 2>/dev/null || true; }
result() { printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"; }
# Reasons can contain names Claude chose, so keep them to one printable line.
reject() {
  local reason; reason=$(printf '%s' "$1" | tr -c '[:print:]' ' ' | cut -c1-300)
  result reason "$reason"; echo "::error::$reason"; exit 1
}

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
  [ "$(git rev-list --count "$BASE..$tip" --not origin/main)" -le 100 ] || reject "Claude made more than 100 commits"
  # Commits that are not on main must be authored and committed by the Claude GitHub App and
  # pass the same message checks as CI, because published commits are never rewritten.
  # Identities are read from the raw commit headers, which cannot hide a field boundary.
  ident="claude\[bot\] <41898282\+claude\[bot\]@users\.noreply\.github\.com> [0-9]+ [+-][0-9]{4}"
  for commit in $(git rev-list "$BASE..$tip" --not origin/main); do
    git cat-file commit "$commit" | sed '/^$/q' > "$RUNNER_TEMP/header"
    [ "$(grep -c '^author ' "$RUNNER_TEMP/header")" = 1 ] && [ "$(grep -c '^committer ' "$RUNNER_TEMP/header")" = 1 ] \
      && grep -Eqx "author $ident" "$RUNNER_TEMP/header" && grep -Eqx "committer $ident" "$RUNNER_TEMP/header" \
      || reject "Commit ${commit:0:7} is not authored and committed as $BOT_NAME"
    git cat-file commit "$commit" | sed '1,/^$/d' > "$RUNNER_TEMP/msg"
    sh .githooks/commit-msg "$RUNNER_TEMP/msg" >/dev/null 2>&1 \
      || reject "Commit ${commit:0:7} message is not one 'Type: Short description' line"
    ! grep -Eiq "$ATTRIBUTION" "$RUNNER_TEMP/msg" || reject "Commit ${commit:0:7} message contains AI self-attribution"
  done
  # Agents never change .github/, in any commit. A commit may change a file there only to the
  # version main had at that point (a merge of main).
  for commit in $(git rev-list "$BASE..$tip"); do
    main_point=$(git merge-base "$commit" origin/main)
    for parent in $(git rev-list --parents -n 1 "$commit" | cut -d' ' -f2-); do
      while IFS= read -r -d '' path; do
        git diff --quiet "$main_point" "$commit" -- "$path" \
          || reject "Claude changed .github/ ($path) in ${commit:0:7}; agents never edit .github/"
      done < <(git diff -z --no-renames --name-only "$parent" "$commit" -- .github)
    done
  done
fi

# Decide whether a pull request will be opened, also when an earlier attempt pushed the
# branch but did not open it, and check its title and description before anything is pushed.
head=${tip:-$BASE}
remote=$(git ls-remote origin "refs/heads/$BRANCH" | cut -f1)
open_pr=false
if [ "$MODE" = implement ] && { [ -n "$tip" ] || [ "$remote" = "$BASE" ]; } \
   && [ "$(git rev-list --count "origin/main..$head")" -gt 0 ] \
   && [ -z "$(gh pr list --repo "$REPO" --head "$BRANCH" --state open --json number --jq '.[0].number // empty')" ]; then
  open_pr=true
  decode "$TITLE" | head -1 > "$RUNNER_TEMP/pr-title.txt"
  sh .githooks/commit-msg "$RUNNER_TEMP/pr-title.txt" >/dev/null 2>&1 \
    || reject "Claude's pull request title is not 'Type: Short description'"
  decode "$BODY" > "$RUNNER_TEMP/pr-body.md"
  [ -s "$RUNNER_TEMP/pr-body.md" ] || reject "Claude wrote no pull request description"
  ! cat "$RUNNER_TEMP/pr-title.txt" "$RUNNER_TEMP/pr-body.md" | grep -Eiq "$ATTRIBUTION" \
    || reject "Claude's pull request title or description contains AI self-attribution"
  # Same rule as the PR metadata check: code and test changes, including files renamed out of
  # src/ or tests/, name an existing specification.
  if git diff --no-renames --name-only "origin/main...$head" | grep -Eq '^(src|tests)/'; then
    spec=$(grep -Eo 'Spec: specs/[0-9]{3}-[a-z0-9-]+' "$RUNNER_TEMP/pr-body.md" | head -1 | cut -c7- || true)
    { [ -n "$spec" ] && git cat-file -e "$head:$spec/spec.md" 2>/dev/null; } \
      || reject "Claude's pull request description has no 'Spec: specs/NNN-name' line for an existing specification"
  fi
fi

# The Claude GitHub App token, from the same exchange claude-code-action uses. It is
# revoked when this script ends. Tokens reach curl and git through stdin and the
# environment, never through command-line arguments.
auth() { printf 'header = "Authorization: Bearer %s"\n' "$1"; }
oidc=$(auth "$ACTIONS_ID_TOKEN_REQUEST_TOKEN" | curl -sSf -K - \
  "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=claude-code-github-action" | jq -r .value)
app=$(auth "$oidc" | curl -sSf -K - -X POST \
  https://api.anthropic.com/api/github/github-app-token-exchange | jq -r '.token // .app_token // empty')
unset oidc
[ -n "$app" ] || reject "Could not get the Claude GitHub App token"
echo "::add-mask::$app"
trap 'auth "$app" | curl -sS -K - -X DELETE "$GITHUB_API_URL/installation/token" >/dev/null || true' EXIT

published=no
if [ -n "$tip" ]; then
  # A normal push: GitHub rejects it unless it only adds commits to the branch.
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=http.https://github.com/.extraheader \
    GIT_CONFIG_VALUE_0="AUTHORIZATION: basic $(printf 'x-access-token:%s' "$app" | base64 -w0)" \
    git push -q origin "$tip:refs/heads/$BRANCH"
  published=yes
  echo "Pushed ${tip:0:7} to $BRANCH"
fi

if [ "$open_pr" = true ]; then
  GH_TOKEN=$app gh pr create --repo "$REPO" --base main --head "$BRANCH" \
    --title "$(cat "$RUNNER_TEMP/pr-title.txt")" --body-file "$RUNNER_TEMP/pr-body.md"
  published=yes
fi

if [ -n "$COMMENT" ]; then
  decode "$COMMENT" > "$RUNNER_TEMP/comment.md"
  GH_TOKEN=$app gh api "repos/$REPO/issues/$NUMBER/comments" -F body=@"$RUNNER_TEMP/comment.md" >/dev/null
  published=yes
  echo "Posted Claude's comment on #$NUMBER"
fi
result published "$published"
