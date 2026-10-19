# Dependency and tooling decisions

Checked 2026-10-19 before implementation:

| Component | Selected | Official version-specific reference |
| --- | --- | --- |
| clap | 4.6.7 | https://docs.rs/clap/4.6.7/clap/_derive/_tutorial/index.html |
| Ratatui | 0.30.2 | https://docs.rs/ratatui/0.30.2/ratatui/fn.try_init.html |
| Ratatui cleanup | 0.30.2 | https://docs.rs/ratatui/0.30.2/ratatui/fn.try_restore.html |
| Ratatui frame | 0.30.2 | https://docs.rs/ratatui/0.30.2/ratatui/struct.Frame.html |
| Crossterm events | 0.29.0 | https://docs.rs/crossterm/0.29.0/crossterm/event/index.html |

Decision: current stable direct releases, exact direct pins and committed Cargo.lock.
Rationale: new project, no existing compatibility constraints; reviewed matching official APIs.
Alternative: older tutorial versions rejected because they create unnecessary API drift.
Ratatui uses Crossterm 0.29; confirm the resolved tree after fetch.

Decision: Spec Kit CLI 1.2.1, official bundled Codex skills, shell scripts.
References: https://github.github.io/spec-kit/reference/integrations.html and
https://github.com/github/spec-kit (current official README).
Command: `specify init --here --force --integration codex --integration-options='--skills' --script sh --non-interactive --ignore-agent-tools`.
The agent-tool check is skipped because this hosted session is not a local Codex CLI installation.
Skills are in `.agents/skills`; invoke `$speckit-specify`, `$speckit-plan`, `$speckit-tasks`,
`$speckit-implement`, `$speckit-converge` in a fresh local Codex session.
Feature docs intentionally combine model/contract/quickstart details into the spec, plan,
and README to honor Task 001's minimal documentation requirement.
