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
   labels the PR `low-risk` or `security-sensitive`, mentions the owner on
   security-sensitive PRs, and turns on auto-merge only for low-risk PRs. PR risk always
   runs the copy on `main`, so a PR cannot change the commands that hold write access.
5. **Codex reviews independently**, once the owner connects it (see
   [setup](#4-connect-codex-code-review)). Codex reads the `Code Review Rules` in
   AGENTS.md and posts a normal GitHub review.
6. **Corrections.** The owner can ask Claude to fix findings with an `@claude` comment.
   At most two correction cycles run per PR; after that the PR is labeled `needs-owner`.
7. **Merge.** Low-risk documentation PRs merge automatically after every required check
   passes. Everything else waits for the owner's approval.

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
- **Labels:** `low-risk` (auto-merge eligible), `security-sensitive` (needs your
  approval), and `needs-owner` (automation stopped).
- **Security-sensitive PRs** get a comment that mentions you and lists the sensitive files.
- **Codex** posts its findings as a PR review in the **Conversation** tab. A 👀 reaction
  means a review is running.
- **Claude** posts a summary comment after each correction cycle listing what it fixed,
  what it did not fix, and which checks it ran.
- **Notifications:** GitHub sends mentions and review requests to your notification
  inbox and, if installed, the GitHub mobile app.

## Ask Claude for corrections

Comment on the PR, starting with `@claude`, for example:

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

## Merge policy

| Change | Merge |
| --- | --- |
| Only `README.md`, `docs/product.md`, or `docs/ui-design.md` (the unowned lines in [CODEOWNERS](../.github/CODEOWNERS)), opened by you or the Claude GitHub App | Automatic after all required checks pass |
| Anything else | Requires your approval through the code-owner rule |
| Cryptography, secrets, authentication, recovery, MCP, permissions, dependencies, workflows, agent rules, security docs, specifications | Labeled `security-sensitive`; auto-merge is turned off; needs your approval and a Codex review |

Auto-merge uses **rebase** so the validated one-line commits land on `main` unchanged.
The low-risk list is read from `main`, so a PR cannot make itself eligible. To change
the list, edit the unowned lines in CODEOWNERS (that edit itself needs your approval).

GitHub does not let anyone approve their own PR. PRs that Claude opens through the
workflow are authored by the Claude GitHub App, so you can approve them normally. For a
PR you authored yourself (including PRs from Claude Code sessions that use your account),
merge with the ruleset's administrator bypass after checking the results yourself.

## One-time setup

Nothing in this list stores a secret in the repository. Steps 1–3 are required for the
Claude workflow. Each step is done in the GitHub or ChatGPT website by the owner.

### 1. Make `main` the default branch

The default branch is currently `001-project-initialization`. GitHub runs issue and
comment workflows from the default branch, so the Claude workflow cannot start until
`main` is the default. Go to **Settings → General → Default branch**, select the switch
icon, choose `main`, and confirm.

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
on the OpenAI site. **Manual trigger:** comment `@codex review` on any PR. The
security-sensitive escalation comment contains that phrase, and Codex responded to it on
#2, so security-sensitive PRs request a Codex review automatically. Do this on
security-sensitive PRs whenever a new commit arrives after the last Codex review.

### 5. Create the trigger label

Go to **Issues → Labels → New label** and create `claude:implement`. The other labels
(`low-risk`, `security-sensitive`, `needs-owner`) are created automatically.

### 6. Allow auto-merge and rebase merging

In **Settings → General → Pull Requests**: enable **Allow rebase merging** and **Allow
auto-merge**. Disable **Allow merge commits**, because "Merge pull request #N" messages
break the commit convention. Squash merging can stay on for manual use if you edit the
message to one line.

### 7. Protect `main` with a ruleset

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
  `Verify (windows-latest)`, `Repository policy`, and `PR metadata`. These names appear
  after the checks have run once on a PR.

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

Merges made by auto-merge do not start a new CI run on `main`, because GitHub does not
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
- **Auto-merge was not enabled:** the run log shows a warning when **Allow auto-merge** is
  off in repository settings.
