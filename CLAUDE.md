# Claude Code Project Instructions

MLAppInstaller is pre-code: scaffolding + spec-driven planning phase. This file covers
the mechanics of working in this repo — build commands, git workflow, TA-mediated
development. For the behavioral/product contract (what this project will and won't do,
and why), see [`docs/CONSTITUTION.md`](docs/CONSTITUTION.md) — the equivalent of TA's
own `docs/TA-CONSTITUTION.md`. This file will grow with the project, the way
TrustedAutonomy's own `CLAUDE.md` did, but starts light on purpose.

## Build & Verify

Once the Rust workspace exists, before every commit:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

All four must pass. No Nix wrapper needed here (unlike TA) unless a future phase
requires it.

## Git Workflow — Feature Branches + Pull Requests

Never commit directly to `main`. Every change, human or TA-mediated, lands on a branch
(`feature/`, `fix/`, `refactor/`, `docs/`) and merges via PR.

```bash
git checkout -b feature/<short-description>
# ...commits in logical units...
git push -u origin feature/<short-description>
gh pr create --title "..." --body "## Summary\n...\n\n## Test plan\n..."
```

## TA-Mediated Development

This repo develops itself using TA (`.ta/` is live here). Overlay flow:

1. `ta_goal_start` — copies project to `.ta/staging/`, agent works there, TA invisible to it.
2. `ta_pr_build` — diffs staging vs source, builds a draft package with artifacts.
3. Draft is reviewed (`ta_ask_human` / `ta_human_verify` as needed), then applied — changes copy back to source, optional git commit.

**Before any `git checkout`/`commit`/`push`**: check `.ta/` for an active apply lock. If a draft apply is in progress, wait — concurrent git operations mid-apply cause rollbacks.

## Rules

- Never disable or skip tests; run tests after every code change, before committing.
- Run `cargo fmt --all -- --check` before every push.
- Commit in logical working units.
- Use `tempfile::tempdir()` for all test fixtures needing filesystem access.
- After every commit, `git status` must show a clean tree — commit or restore stragglers before moving on.
- Never run interactive TUI output into goal titles, PR titles, or commit summaries (ANSI escapes corrupt metadata).

## Deferred Items Policy

A phase isn't done while it still has an open "remaining" list.

1. Review every planned item: done, partial, or not started.
2. Anything not done: decide with the user — still needed? which future phase owns it? drop it?
3. Record the decision inline (`→ vX.Y` or "dropped: <reason>").
4. Never leave unchecked `[ ]` items inside a phase marked done.

## Observability Mandate

This is an installer. Every failure mode it has is a failure mode a real user hits with
no one to ask. So every outcome must be **observable** and **actionable**:

- Error messages state what happened, what was being attempted, and what to do next — never a bare "failed."
- Timeouts name the operation, the duration, and how to configure it.
- CLI output confirms what happened (paths, counts, IDs), not just exit-code success.
- Use structured logging (`tracing`) for operational issues, not print-and-forget.

## Current State

**Status**: scaffolding — no Rust workspace yet. Next step is Superpowers spec-driven
planning (design → `docs/superpowers/specs/`, plan → implementation).

**Version**: not yet cut. Once a `Cargo.toml [workspace.package]` exists, that becomes
the single source of truth for version; this file gets updated to match on every bump,
the same discipline TA enforces on itself.
