# MLAppInstaller Constitution

> The canonical behavioral contract for MLAppInstaller.
> Every crate, manifest field, and integration must adhere to these rules.
> Design reviews (spec, plan, PR) validate conformance against this document.

**Last updated**: pre-v0.1, scaffolding phase.
**Status**: Living document — update when behavior changes, the way TA's own constitution does.

---

## 1. Core Principles

### 1.1 Local-First, Cloud-Explicit
Every install defaults to local. A hosted/cloud backend — a hosted model API, a cloud deploy target — is only used when the user explicitly opts in. Nothing silently reaches out to a paid API or a remote provider because it happened to be available.

### 1.2 Human-in-the-Loop for Irreversible Actions
Uninstall, `--force` reinstall, and any path removal always confirm or back up first, never both skip confirmation and skip backup. `--dry-run` is always available and must reflect exactly what a real run would do.

### 1.3 Observable & Actionable
Every outcome — success, failure, timeout, skipped step — is logged with enough detail to act on: what happened, what was being attempted, what to do next. No bare "failed." No silent success either: a component that installs but fails its health check is reported NEEDS ATTENTION, not swallowed.

### 1.4 Backup Before Overwrite
A component's live install is never replaced in place. It is copied aside before the new version lands, so a failed download or unpack leaves the working install intact. This is a proven model, carried forward exactly, not reinvented.

### 1.5 Data-Defined Components & Providers
Components, backend options (local vs. hosted model choices), and cloud provider adapters are declared as data — manifest entries and plugin protocols — never as hardcoded per-product logic inside core crates. If adding a new component or provider requires a core code change, that's a constitution violation: the extensibility point is missing, not the exception.

**Why this rule exists**: this project exists specifically to stop TA and every other consumer from each hardcoding their own component list into their own installer. The same failure mode one level down (hardcoding a component's *options* or a cloud *provider*) would just recreate the problem this project was built to solve.

### 1.6 Reuse Before Reinventing
Before adding a new manifest field, protocol, or crate, check whether an existing MLAppInstaller abstraction already covers the need — the manifest schema, pipeline, Setup Options Protocol, model catalog, and project-binding mechanisms already exist precisely so nobody has to reinvent them per-adopter. Extend it, or justify in the PR why a new mechanism is required. This applies to secrets management too: reuse an existing credential tool (OS keychain, 1Password, Vault) via §2's credential-source-reference pattern rather than building a new store.

### 1.7 Single Cross-Platform Codebase
One Rust implementation, cross-compiled for Windows/macOS/Linux. No maintained per-OS script twins. This is the specific duplication this project exists to eliminate — reintroducing it anywhere in this codebase is a constitution violation.

---

## 2. Credential & Secrets Handling

### 2.1 The Installer Never Touches Secret Values
`mlai` does not store, manage, broker, or ever see a hosted-model API key or any other secret. It may collect and pass through a *credential-source reference* (e.g. "read this from the OS keychain, service X" or "from 1Password item Y") via the backend-options protocol's `--set key=value`, but resolving that reference into an actual secret value is entirely the responsibility of the component's own setup command. An installer that stores secret values is solving a problem (broker credentials to processes, revocation, TTLs) that already has better-built tools (OS keychains, 1Password, Vault) — reinventing it here would violate §1.6 (Reuse Before Reinventing) against the very tools this rule points to.

**Why this rule exists**: an earlier draft of this project (Plan B, since reverted) built an installer-owned encrypted vault (`mlai-credentials`, ported from TA's `ta-credentials`) and a `mlai credential set` command. That pattern is right for TA — a long-running agent runtime brokering scoped, revocable credentials to untrusted agent processes — and wrong for an installer, which runs once and exits. Corrected 2026-08-15; see `docs/superpowers/specs/2026-08-15-credential-source-glue-design.md` for the on-hold replacement design.

### 2.2 Credential-Source Glue Is a Future, Separate Design
The "point at where the secret lives" mechanism described in §2.1 is not yet designed in detail — see the design-exploration doc referenced above. Until that design is approved, no component should be built assuming a specific credential-source protocol beyond the generic `--set key=value` passthrough that already exists.

---

## 3. Component Install Lifecycle

### 3.1 Idempotent & Resumable
Install state (`downloaded → unpacked → setup → healthy`) persists after every stage. Re-running a partially-completed or already-healthy install resumes or no-ops; it never blindly restarts from scratch.

### 3.2 Health-Checked Completion
A component is not "installed" until its declared health check passes. There is no implicit success state.

### 3.3 Guarded Removals
Any path removed during an upgrade or uninstall must be validated to resolve inside the install root. A malformed manifest can never delete a file outside the install tree — no exceptions, no override flag.

---

## 4. Cloud & Provider Extensibility

### 4.1 Config-Generation Only in Core
`mlai-cloud` generates deploy configuration (Dockerfile, deploy manifest, secrets template). Core never calls a cloud provider's API directly — that boundary is what keeps core provider-agnostic.

### 4.2 Provider Adapters Are External, Optional, and Community-Contributable
AWS, Render.com, and any other live-provisioning support ships as a discoverable external adapter (JSON-over-stdio, mirroring the Setup Options Protocol's own shape), never as special-cased provider code merged into core.

---

## 5. Build & Verify

Before every commit, once the Rust workspace exists:
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
All four must pass. Test fixtures needing filesystem access use `tempfile::tempdir()` — no hardcoded paths, no cross-run pollution.

---

## Appendix: Compliance Checklist

Filled in as CLI surfaces land — placeholder until v1's `mlai install`/`repair`/`uninstall`/`update`/`cloud generate` commands exist to check against.

| Command | Key Rules |
|---------|-----------|
| `mlai install` | 1.4 (backup before overwrite), 3.1 (idempotent), 3.2 (health-checked) |
| `mlai repair` | 1.2 (confirm/backup), 3.1, 3.2 |
| `mlai uninstall` | 1.2 (confirm), 3.3 (guarded removals) |
| `mlai cloud generate` | 1.1 (explicit opt-in), 4.1 (config-gen only) |
