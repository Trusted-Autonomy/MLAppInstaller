# MLAppInstaller

A generic, reusable, cross-platform installer foundation for agentic apps that need configurable local-vs-cloud model backends at install time. Scaffolding only — architecture and implementation are deliberately deferred to a Superpowers spec-driven planning session. This README exists so that session starts with full context instead of a blank page.

## Origin / requirements captured 2026-08-14

Two existing Rust-based installers already solve pieces of this problem independently, and are duplicating effort:

1. **TrustedAutonomy's own installer** (`~/development/TrustedAutonomy/install_local.sh` + `scripts/bump-version.sh`, `scripts/setup-windows-dev.ps1`, `scripts/sign-windows.ps1` etc.) — handles Rust toolchain/binary install, Windows code-signing, cross-platform packaging for TA's own CLI + daemon.
2. **CinePipeAi's installer** (`github.com/CinePipeAi/cinepipe-installer`, private repo) — a Rust-based installer with configurable local vs. API-key hosted model selection at install time for the CinePipeAi pipeline.

Both solve the same underlying problem (cross-platform install, configurable local/cloud model backend selection, secure credential handling at install time) for different products. **MLAppInstaller's purpose**: extract the common foundation from both into one standalone project, so that:
- TA's own installer
- CinePipeAi's installer
- The new `agentic-pm` app's installer (see sibling repo, also scaffolded 2026-08-14 — explicitly wants to build its installer on top of this once ready)

...can all eventually migrate to build on this shared foundation instead of maintaining three parallel implementations.

## Scope (from the founding conversation)

- Cross-platform (matches TA and CinePipeAi's existing cross-platform targets).
- Configurable at install time: local model backend vs. API-key-based hosted model backend — same pattern CinePipeAi already uses, generalized.
- Secure credential handling for the hosted-model-key path — TA's `ta-credentials` crate (age-encryption-at-rest, OS-keychain-first custody, chmod-0600 fallback — see TA's `crates/ta-credentials/src/encryption.rs`) is a proven reference implementation worth reviewing before designing this from scratch.
- Cloud-hosting support: needs to support installing/configuring a daemon/engine for cloud deployment (e.g. Render.com), not just local desktop install — this came up specifically in the context of `agentic-pm`'s cloud-template requirement, but should be a general capability of this installer, not bolted on later.

## Explicitly NOT decided yet

- Whether this becomes a Rust library (`ta-package`-style, note TA's own `PLAN.md` already plans a similar extraction — "v0.18.2: Extract App Packaging as `ta-package` + Cross-Platform Installer" — check whether these two efforts should merge or stay separate) or a standalone CLI tool other installers shell out to, or both.
- Migration plan/timeline for TA and CinePipeAi's existing installers to adopt this — not blocking, do later once this is proven.
- Relationship to `agentic-pm`'s installer — `agentic-pm` wants to build on this once ready, but is being scaffolded in parallel, not blocked on this landing first.

## Status

Scaffolding only, 2026-08-14. Next step is a Superpowers spec-driven planning session — start by diffing what TA's install scripts and CinePipeAi's `cinepipe-installer` actually do today (both are real, working prior art) before designing the shared abstraction, and check TA's own planned `v0.18.2` packaging-extraction phase for overlap before committing to a design that might duplicate it.
