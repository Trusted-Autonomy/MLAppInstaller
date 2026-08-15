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
A component's live install is never replaced in place. It is copied aside before the new version lands, so a failed download or unpack leaves the working install intact. This is cinepipe-installer's proven model — carried forward exactly, not reinvented.

### 1.5 Data-Defined Components & Providers
Components, backend options (local vs. hosted model choices), and cloud provider adapters are declared as data — manifest entries and plugin protocols — never as hardcoded per-product logic inside core crates. If adding a new component or provider requires a core code change, that's a constitution violation: the extensibility point is missing, not the exception.

**Why this rule exists**: this project exists specifically to stop TA, CinePipe, and future consumers from each hardcoding their own component list into their own installer. The same failure mode one level down (hardcoding a component's *options* or a cloud *provider*) would just recreate the problem this project was built to solve.

### 1.6 Reuse Before Reinventing
Before adding a new manifest field, protocol, or crate, check whether cinepipe-installer's Setup Options Protocol, TA's `ta-credentials` vault, or an existing MLAppInstaller abstraction already covers the need. Extend it, or justify in the PR why a new mechanism is required.

### 1.7 Single Cross-Platform Codebase
One Rust implementation, cross-compiled for Windows/macOS/Linux. No maintained per-OS script twins. This is the specific duplication (cinepipe's `install.ps1`/`install.sh` pair) this project exists to eliminate — reintroducing it anywhere in this codebase is a constitution violation.

---

## 2. Credential & Secrets Handling

### 2.1 Vault-Only Storage
Hosted-model API keys and any other secret are never written to disk in plaintext. They go through `mlai-credentials` (age encryption, OS-keychain-first custody), the same pattern as `ta-credentials`, generalized to a caller-supplied keyring namespace.

### 2.2 Fallback Custody Is Disclosed, Never Silent
When the OS keychain is unreachable and the vault falls back to a chmod-0600 file, that weaker guarantee is surfaced loudly to the user (equivalent to `ta doctor`), not just logged at debug level.

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
AWS, Render.com, and any other live-provisioning support ships as a discoverable external adapter (JSON-over-stdio, mirroring cinepipe's Setup Options Protocol shape), never as special-cased provider code merged into core.

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
