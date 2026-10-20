# Development automation

This page explains how GitHub, Claude Code, and Codex work together on MaxonKey, what
each part is allowed to do, and the one-time setup the repository owner must complete.
The shared rules for every agent are in [AGENTS.md](../AGENTS.md).

- [How it works](#how-it-works)
- [Start a feature](#start-a-feature)
- [See CI and review results](#see-ci-and-review-results)
- [Ask Claude for corrections](#ask-claude-for-corrections)
- [Merge policy](#merge-policy)
- [One-time setup](#one-time-setup)
- [What is verified](#what-is-verified)
- [Troubleshooting](#troubleshooting)

## How it works

1. **The owner approves a feature.** The owner writes a GitHub issue that names the
   specification it follows (for example `Spec: specs/002-encrypted-vault`) and adds
   the `claude:implement` label.
2. **Claude Code implements it.** The [Claude workflow](../.github/workflows/claude.yml)
   runs [Claude Code](https://github.com/anthropics/claude-code-action) on a GitHub-hosted
   runner. Claude creates a `claude/issue-N` branch, follows the specification, runs the
   project checks, and opens a pull request (PR) against `main`.
3. **CI runs.** The [Rust CI workflow](../.github/workflows/ci.yml) runs formatting,
   Clippy, tests, and release builds on Windows and Linux, the Linux terminal tests,
   dependency vulnerability and license checks, the commit-message check, a secret scan
   of the full Git history with trusted scanner settings, and a check that specifications are
   complete and Markdown links and heading anchors resolve.
4. **PR policy runs.** The [PR policy workflow](../.github/workflows/pr-policy.yml)
   checks the PR title and description. The [PR risk workflow](../.github/workflows/pr-risk.yml)
   labels the PR `low-risk` (routine) or `security-sensitive` and mentions the owner on
   security-sensitive PRs.
5. **Codex reviews independently.** Codex reviews a new PR on its own. Codex reads the
   `Code Review Rules` in AGENTS.md and posts a normal GitHub review.
6. **The Factory decides the next step.** The [Factory workflow](../.github/workflows/factory.yml)
   runs whenever CI or a Codex review finishes. For each PR it:
   - asks Codex to review a new commit (`@codex review`) once CI is green, because new
     commits do not start a Codex review by themselves;
   - starts a Claude correction when CI fails or Codex reports findings on a Claude PR;
   - sets the **Independent review** status: pending until Codex reviewed the current
     commit, failed on findings, passed when clean;
   - merges a routine PR when every required check and the Codex review pass, then
     starts the next approved issue;
   - asks Claude to merge `main` into its branch when a merge fails because `main` moved on;
   - otherwise posts one comment that mentions you, when a PR is ready for your approval
     or is blocked.
7. **Corrections are limited.** Each PR gets at most two Claude correction cycles, and
   each commit is corrected at most once. After that the PR is labeled `needs-owner`
   and you get one comment.
8. **Tasks run one at a time.** Label as many issues `claude:implement` as you like.
   The oldest one starts when no other Claude PR is in progress; the rest wait in the
   queue. A Claude run that fails is retried once; after a second failure the issue is
   labeled `needs-owner`, you get one comment, and the queue moves on.

The PR risk, Factory, and Claude dispatch workflows always run their copy on `main`,
so a pull request cannot change the commands that hold write access.

Who can start the automation: only the repository owner. Issues written by other
accounts, comments from other accounts, and forked PRs never start Claude.

## Start a feature

1. Make sure an approved specification exists in `specs/`. If it does not, write and
   approve one first. Claude refuses work without one.
2. Open a new issue. Describe the change in plain English and include a line such as:

   ```text
   Spec: specs/002-encrypted-vault
   ```

3. Add the label `claude:implement`. The issue must be written by you, and you must
   add the label yourself.
4. Open the **Actions** tab to watch the **Claude** run. When it finishes, the new PR
   appears under **Pull requests**, linked to the issue. If Claude could not proceed,
   it explains why in an issue comment.

Claude will not change cryptography, key handling, the vault format, authentication,
recovery, or MCP behavior unless the specification and ADR already approve that change.

## See CI and review results

- **On the PR page:** the **Checks** section at the bottom lists every check with a
  green tick or red cross. Select **Details** to open the full log in the Actions tab.
- **Labels:** `low-risk` (routine, merged automatically), `security-sensitive` (needs your
  approval), and `needs-owner` (automation stopped for that issue or PR).
- **Independent review** status on each commit shows whether Codex reviewed it.
- **Security-sensitive PRs** get a comment that mentions you and lists the sensitive files.
- **Codex** posts its findings as a PR review in the **Conversation** tab. A 👀 reaction
  means a review is running.
- **Claude** posts a summary comment after each correction cycle listing what it fixed,
  what it did not fix, and which checks it ran.
- **Notifications:** GitHub sends mentions and review requests to your notification
  inbox and, if installed, the GitHub mobile app.

## Ask Claude for corrections

Corrections for CI failures and Codex findings on Claude's own PRs start automatically.
To ask for something else, comment on the PR, starting with `@claude`, for example:

```text
@claude Fix the Clippy failure and the Codex finding about the error message.
```

You can also start a review comment with `@claude` when you submit a review. Before
Claude runs, the workflow collects your comments, Codex review comments, and the current
check results into one findings file. Comments from any other account are left out.

- Claude only pushes to `claude/` branches of this repository.
- Each run posts a "correction cycle N of 2" comment. After two, Claude stops and adds
  `needs-owner`. To allow more cycles, delete those cycle comments.
- Serious security findings are not fixed automatically. Claude posts a comment starting
  with `Security escalation:` and mentions you.

Use `@claude`, not `@codex`, for fixes, so the two-cycle limit applies.

### False-positive Codex findings

If a Codex finding cites a commit that does not exist in the repository, the Factory
marks it unverified: no automatic correction runs, **Independent review** fails, and you
get one comment. If you check the finding and it is wrong, comment
`/accept-codex <first 7 characters of the commit> <reason>`. That marks **Independent
review** as passed for that one commit only; any new commit needs a new review. It does
not skip CI, and security-sensitive PRs still need your merge.

## Merge policy

| Change | Merge |
| --- | --- |
| Routine: touches no path listed in [CODEOWNERS](../.github/CODEOWNERS), opened by you or the Claude GitHub App | Merged by the Factory after all required checks and the Codex review pass |
| Security-sensitive: cryptography, secrets, authentication, recovery, CLI credential handling, MCP, permissions, dependencies, workflows, agent rules and settings, policy test scripts, security docs, specifications (the paths in CODEOWNERS) | Labeled `security-sensitive`; never merged automatically; needs your approval and a passing **Independent review** status |

The Factory merges with **rebase** so the validated one-line commits land on `main`
unchanged, and only if the PR head is still the commit it checked.
The classification runs from `main`, so a PR cannot make itself routine. The sensitive
list lives in CODEOWNERS and `.github/scripts/classify-pr.sh`; changing either is itself
security-sensitive.

## Agent permissions

- **Claude in GitHub Actions** may run any development command: Git (branch, commit,
  push to its own `claude/` branch, open PRs), file creation, editing, and deletion,
  Cargo including dependency installs, builds, and tests. A short deny list blocks force
  pushes, pushes to `main`, `git config`, merging, approving, `gh api`, secrets, and
  printing the environment. Secrets are removed from every command Claude runs, and those
  commands run isolated on the runner.
- **Real limits** do not depend on the agent following instructions: the Claude GitHub App
  token cannot change workflow files and is not an administrator, the `main` ruleset
  blocks direct pushes and requires the checks, and only the Factory (from `main`) merges.
- **Interactive Claude Code sessions** use `.claude/settings.json`: routine development
  commands run without confirmation prompts; force pushes, `git config`, merges, approvals,
  secrets, and `.env` files stay blocked.
- **Codex** reviews through its GitHub connector. Its own permissions and environment
  are set in Codex settings on the ChatGPT website.

GitHub does not let anyone approve their own PR. PRs that Claude opens through the
workflow are authored by the Claude GitHub App, so you can approve them normally. For a
PR you authored yourself (including PRs from Claude Code sessions that use your account),
merge with the ruleset's administrator bypass after checking the results yourself.

## One-time setup

Nothing in this list stores a secret in the repository. Steps 1–3 are required for the
Claude workflow. Each step is done in the GitHub or ChatGPT website by the owner.

### 1. Make `main` the default branch

Done. GitHub runs issue, comment, and Factory workflows from the default branch, so it
must stay `main` (**Settings → General → Default branch**).

### 2. Install the Claude GitHub App

Install [the Claude GitHub App](https://github.com/apps/claude) and choose **Only select
repositories → maxonkey**. The app pushes branches and opens PRs. It cannot edit
workflow files and has no administrator rights. Alternatively, run `/install-github-app`
in a local Claude Code session.

### 3. Add one Claude credential as a repository secret

Go to **Settings → Secrets and variables → Actions → New repository secret** and add
**one** of these:

- `CLAUDE_CODE_OAUTH_TOKEN`: uses a Claude Pro or Max subscription. Create it on your own
  computer with `claude setup-token`.
- `ANTHROPIC_API_KEY`: uses pay-as-you-go Claude API billing from the Claude Console.

Paste the value only into the GitHub secret form. Never put it in a file, issue, or comment.

### 4. Connect Codex code review

1. In [Codex](https://chatgpt.com/codex), connect your GitHub account and give the Codex
   GitHub connector access to `maxonkey` only (already done for this repository), then
   [create a Codex cloud environment](https://chatgpt.com/codex/cloud/settings/environments)
   for `maxonkey`. Without one, Codex replies "create an environment" instead of reviewing.
2. Open [Codex code review settings](https://chatgpt.com/settings/code-review), enable
   code review for `maxonkey`, and turn on **Automatic review**.
3. On the first reviewed PR, note the account name that posted the review. If it is not
   `chatgpt-codex-connector[bot]`, add a repository variable (not a secret) named
   `CODEX_REVIEWER_LOGIN` with that name under **Settings → Secrets and variables →
   Actions → Variables**, so Claude receives Codex findings.

Codex code review requires a ChatGPT plan that includes Codex; check current plan details
on the OpenAI site. The Factory requests a Codex review for each new commit after CI
passes. **Manual trigger:** comment `@codex review` on any PR.

### 5. Create the trigger label

Go to **Issues → Labels → New label** and create `claude:implement`. The other labels
(`low-risk`, `security-sensitive`, `needs-owner`) are created automatically.

### 6. Merge settings

In **Settings → General → Pull Requests**: keep **Allow rebase merging** on. Disable
**Allow merge commits**, because "Merge pull request #N" messages break the commit
convention. GitHub's **Allow auto-merge** is not needed; the Factory merges itself.

### 7. Protect `main` with a ruleset (required)

Claude can run any Git command on the runner, so this ruleset is what technically stops
an agent from pushing to `main` or merging without the checks.

Go to **Settings → Rules → Rulesets → New ruleset → New branch ruleset**:

- **Name:** `main`. **Enforcement status:** Active. **Target branches:** Include default
  branch (after step 1) or add `main` by name.
- **Bypass list:** add **Repository admin** and set it to **For pull requests only**.
- Enable **Restrict deletions** and **Block force pushes**.
- Enable **Require a pull request before merging** with **Required approvals: 0**, and
  turn on **Require review from Code Owners** and **Dismiss stale pull request approvals
  when new commits are pushed**. Under allowed merge methods, keep **Rebase** (and
  **Squash** if you use it manually).
- Enable **Require status checks to pass** and add: `Verify (ubuntu-latest)`,
  `Verify (windows-latest)`, `Repository policy`, `PR metadata`, and `Independent review`.
  These names appear after the checks have run once on a PR.

### 8. Recommended GitHub security settings

- **Settings → Code security:** enable **Secret scanning** and **Push protection** (free
  for public repositories). They block known credential formats before they are pushed.
- **Settings → Actions → General:** keep **Workflow permissions** at read-only and leave
  **Allow GitHub Actions to create and approve pull requests** turned off. Keep
  **Require approval for first-time contributors** for fork PRs.

## What is verified

| Part | Status |
| --- | --- |
| Rust CI on Windows and Ubuntu | Passed on GitHub runners for the PR that added this automation (#2). |
| CI policy job (commit messages, gitleaks 8.30.1 secret scan, spec and link check) | Passed on a GitHub runner (#2); also run locally against the full history. |
| PR risk classification | Observed on #2 while it ran as part of PR policy: labeled it `security-sensitive`, posted the owner escalation comment, and left auto-merge off. It now runs from `main` through `pull_request_target`, so it first runs in that form on the next PR after #2 merges. Other cases (docs-only, mixed, cryptography, CLI, MCP, non-owner, stale notice) exercised locally with a simulated GitHub CLI. |
| PR policy: metadata | Observed on #2: failed on an AI attribution footer added by the tool that created the PR, as intended. Title and spec rules exercised locally. |
| Workflow syntax | `actionlint` 1.7.12 with ShellCheck reports no problems. |
| Claude workflow | **Not yet operational.** Needs setup steps 1–3; then test with a small documentation issue. |
| Codex review | **Not yet operational.** The Codex connector is installed and replied on #2 as `chatgpt-codex-connector[bot]`, asking for a Codex cloud environment for this repository (setup step 4). No review has run yet. |
| Ruleset, code-owner approval, auto-merge | **Not yet configured.** Needs steps 6–7 in the GitHub settings. |

Merges made by the Factory do not start a new CI run on `main`, because GitHub does not
trigger workflows from its own workflow token. The PR's checks already tested the result.

## Troubleshooting

- **Labeling an issue does nothing:** check that `main` is the default branch, that the
  issue was written by the owner, and that the label is exactly `claude:implement`.
- **The Claude run fails at authentication:** check that the Claude GitHub App is
  installed on this repository and that one Claude secret exists.
- **`Repository policy` fails on commit messages:** a commit in the PR is not one line of
  `Type: Short description`. Rewrite the commit on its branch before merging.
- **`PR metadata` fails:** fix the PR title, remove AI attribution from the description,
  or add a `Spec: specs/NNN-name` line for code changes.
- **Codex did not review a new commit:** Codex reviews when a PR opens or a draft is
  marked ready, and when a person comments `@codex review`; it ignores that comment from the
  workflow bot. After 30 minutes the Factory tells you once. Set Codex to review every push
  in Codex settings, or comment `@codex review` yourself.
- **A low-risk PR did not merge:** open the latest **Factory** run. It prints the CI and
  Codex state it saw, for example `ci=passed codex=pending`.
