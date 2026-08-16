# GitHub Releases Deploy Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The one real, working deploy destination for v1: publish built packages to GitHub Releases via the `gh` CLI, matching how both TA and CinePipe already publish today.

**Prerequisite:** `docs/superpowers/plans/2026-08-16-mlai-package-foundation.md` must already be merged — this plan adds a module to the `mlai-package` crate that plan creates and consumes its `DistributionProfile`/`DeployConfig` types. Do not start this plan if `crates/mlai-package/src/profile.rs` doesn't exist yet.

**Architecture:** A `deploy_command` function constructs a `gh release create <tag> <files...> --repo <repo> [--draft] [--prerelease] --notes <text> --title <text>` invocation as a `std::process::Command`, testable by inspecting its args without executing it (same pattern as `mlai-package`'s `packager_command`) — `gh` CLI auth/config is the caller's environment, not something this plan manages. `mlai-cli` gets a `package deploy` subcommand.

**Tech Stack:** No new dependencies. Shells out to `gh` (already used throughout this project's own development workflow, confirmed present and authenticated on GitHub-hosted CI runners by default).

## Global Constraints

- Verified directly (not guessed): `gh release create <tag> <file1> <file2> ... --repo <owner/name> [--draft] [--prerelease] --notes <text> --title <text>` creates the release and uploads assets in one command. `gh release upload <tag> <files...> --clobber` is the separate command for adding assets to an *existing* release — this plan only needs `create`.
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass on all three CI platforms. No test in this plan actually invokes `gh` — every test inspects a constructed `Command`'s arguments, matching `mlai-package`'s established testing posture for external-tool wrapping.

## Out of scope for this plan

- Any deploy destination other than GitHub Releases — the `DeployAdapter` trait shape here is intentionally minimal (one function), not over-generalized ahead of a second real destination actually existing.
- `gh` authentication/setup — the caller's (adopter's CI) responsibility, same as this project's own CI already assumes.

---

### Task 1: `deploy_command` construction

**Files:**
- Create: `crates/mlai-package/src/deploy.rs`
- Modify: `crates/mlai-package/src/lib.rs`

**Interfaces:**
- Produces: `mlai_package::deploy::{deploy_command, DeployOptions}`. `DeployOptions { repo: String, tag: String, files: Vec<PathBuf>, draft: bool, prerelease: bool, notes: String, title: String }`. `deploy_command(opts: &DeployOptions) -> Command`.

- [ ] **Step 1: Write the failing test**

`crates/mlai-package/src/deploy.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_options() -> DeployOptions {
        DeployOptions {
            repo: "CinePipeAi/cinepipe-director".to_string(),
            tag: "v1.2.3".to_string(),
            files: vec![PathBuf::from("dist/app.dmg"), PathBuf::from("dist/app.msi")],
            draft: false,
            prerelease: false,
            notes: "Release notes here".to_string(),
            title: "v1.2.3".to_string(),
        }
    }

    #[test]
    fn command_invokes_gh_release_create_with_tag_and_files() {
        let opts = sample_options();
        let cmd = deploy_command(&opts);

        assert_eq!(cmd.get_program(), "gh");
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(args[0], "release");
        assert_eq!(args[1], "create");
        assert_eq!(args[2], "v1.2.3");
        assert!(args.contains(&"dist/app.dmg".to_string()));
        assert!(args.contains(&"dist/app.msi".to_string()));
        assert!(args.contains(&"--repo".to_string()));
        assert!(args.contains(&"CinePipeAi/cinepipe-director".to_string()));
    }

    #[test]
    fn draft_and_prerelease_flags_are_included_only_when_set() {
        let mut opts = sample_options();
        opts.draft = true;
        opts.prerelease = true;
        let cmd = deploy_command(&opts);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"--draft".to_string()));
        assert!(args.contains(&"--prerelease".to_string()));

        let cmd_without = deploy_command(&sample_options());
        let args_without: Vec<String> = cmd_without.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(!args_without.contains(&"--draft".to_string()));
        assert!(!args_without.contains(&"--prerelease".to_string()));
    }

    #[test]
    fn notes_and_title_are_passed_through() {
        let opts = sample_options();
        let cmd = deploy_command(&opts);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        let notes_index = args.iter().position(|a| a == "--notes").unwrap();
        assert_eq!(args[notes_index + 1], "Release notes here");
        let title_index = args.iter().position(|a| a == "--title").unwrap();
        assert_eq!(args[title_index + 1], "v1.2.3");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-package && cargo test deploy::`
Expected: FAIL to compile — module `deploy` doesn't exist yet.

- [ ] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-package/src/deploy.rs`:
```rust
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DeployOptions {
    pub repo: String,
    pub tag: String,
    pub files: Vec<PathBuf>,
    pub draft: bool,
    pub prerelease: bool,
    pub notes: String,
    pub title: String,
}

/// Builds the `gh release create` invocation for publishing built packages,
/// without running it — kept separate from any execution wrapper so the
/// exact command shape is testable without `gh` installed or authenticated.
/// Verified directly: `gh release create <tag> <files...> --repo <repo>
/// [--draft] [--prerelease] --notes <text> --title <text>` creates the
/// release and uploads assets in one command.
pub fn deploy_command(opts: &DeployOptions) -> Command {
    let mut cmd = Command::new("gh");
    cmd.arg("release").arg("create").arg(&opts.tag);
    for file in &opts.files {
        cmd.arg(file);
    }
    cmd.arg("--repo").arg(&opts.repo);
    if opts.draft {
        cmd.arg("--draft");
    }
    if opts.prerelease {
        cmd.arg("--prerelease");
    }
    cmd.arg("--notes").arg(&opts.notes);
    cmd.arg("--title").arg(&opts.title);
    cmd
}
```

Add to `crates/mlai-package/src/lib.rs`:
```rust
pub mod deploy;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-package && cargo test deploy::`
Expected: PASS — 3 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/mlai-package/src/deploy.rs crates/mlai-package/src/lib.rs
git commit -m "feat(mlai-package): add GitHub Releases deploy command construction"
```

---

### Task 2: Execution wrapper + `mlai package deploy` CLI command

**Files:**
- Modify: `crates/mlai-package/src/deploy.rs`
- Create: `crates/mlai-cli/src/commands/package_deploy.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Modify: `crates/mlai-cli/src/main.rs`

**Interfaces:**
- Produces: `mlai_package::deploy::{deploy, DeployError}`. `deploy(opts: &DeployOptions) -> Result<(), DeployError>`. CLI: `mlai package deploy --profile <path> --tag <tag> --file <path> [--file <path> ...] [--draft] [--prerelease] --notes <text> --title <text>`.

- [ ] **Step 1: Add the execution wrapper**

Add to `crates/mlai-package/src/deploy.rs`, after `deploy_command`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("failed to launch gh: {source}")]
    Launch {
        #[source]
        source: std::io::Error,
    },
    #[error("gh release create exited with status {status}")]
    Failed { status: i32 },
}

pub fn deploy(opts: &DeployOptions) -> Result<(), DeployError> {
    let status = deploy_command(opts).status().map_err(|source| DeployError::Launch { source })?;
    if !status.success() {
        return Err(DeployError::Failed { status: status.code().unwrap_or(-1) });
    }
    Ok(())
}
```

- [ ] **Step 2: Write `commands/package_deploy.rs`**

`crates/mlai-cli/src/commands/package_deploy.rs`:
```rust
use anyhow::{bail, Context, Result};
use mlai_package::deploy::{deploy, DeployOptions};
use mlai_package::profile::DistributionProfile;
use std::fs;
use std::path::{Path, PathBuf};

#[allow(clippy::too_many_arguments)]
pub fn run(
    profile_path: &Path,
    tag: &str,
    files: Vec<PathBuf>,
    draft: bool,
    prerelease: bool,
    notes: &str,
    title: &str,
) -> Result<()> {
    let profile_str = fs::read_to_string(profile_path)
        .with_context(|| format!("reading distribution profile at {}", profile_path.display()))?;
    let profile = DistributionProfile::parse(&profile_str)
        .with_context(|| format!("parsing distribution profile at {}", profile_path.display()))?;

    let Some(deploy_config) = profile.deploy else {
        bail!(
            "distribution profile '{}' has no [deploy] section — nothing to deploy to",
            profile.distribution.name
        );
    };
    let Some(repo) = deploy_config.repo else {
        bail!("distribution profile's [deploy] section has no repo configured");
    };
    if deploy_config.adapter != "github-releases" {
        bail!(
            "unsupported deploy adapter '{}' — only 'github-releases' is implemented",
            deploy_config.adapter
        );
    }

    let opts = DeployOptions {
        repo,
        tag: tag.to_string(),
        files,
        draft,
        prerelease,
        notes: notes.to_string(),
        title: title.to_string(),
    };
    deploy(&opts).context("publishing to GitHub Releases")?;
    println!("Published {} to {}", opts.tag, opts.repo);
    Ok(())
}
```

Modify `crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod catalog;
pub mod install;
pub mod package;
pub mod package_deploy;
pub mod repair;
pub mod uninstall;
```

- [ ] **Step 3: Wire the CLI subcommand**

In `crates/mlai-cli/src/main.rs`, add a `Deploy` variant to the `PackageAction` enum (alongside `Build`):
```rust
    Deploy {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        tag: String,
        #[arg(long = "file")]
        files: Vec<PathBuf>,
        #[arg(long)]
        draft: bool,
        #[arg(long)]
        prerelease: bool,
        #[arg(long, default_value = "")]
        notes: String,
        #[arg(long)]
        title: Option<String>,
    },
```
Update the `PackageAction` match arm to add:
```rust
            PackageAction::Deploy { profile, tag, files, draft, prerelease, notes, title } => {
                let title = title.unwrap_or_else(|| tag.clone());
                commands::package_deploy::run(&profile, &tag, files, draft, prerelease, &notes, &title)
            }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS — this task adds no new automated tests of its own beyond Task 1's; confirm the whole workspace still builds and every existing test still passes.

- [ ] **Step 5: Commit**

```bash
git add crates/mlai-package/src/deploy.rs crates/mlai-cli
git commit -m "feat(mlai-cli): add package deploy command"
```

---

### Task 3: Docs + final verification

**Files:**
- Modify: `docs/USAGE.md`

**Interfaces:** none new.

- [ ] **Step 1: Run the full constitution-required check suite locally**

Run:
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all four PASS.

- [ ] **Step 2: Update `docs/USAGE.md`**

Add after "Building a distribution":
```markdown
## Publishing a distribution

```bash
mlai package deploy --profile distribution-profile.toml --tag v1.2.3 \
  --file dist/my-app.dmg --file dist/my-app.msi \
  --notes "Release notes" --title "v1.2.3"
```

Requires the profile's `[deploy]` section (`adapter = "github-releases"`,
`repo = "owner/name"`) and a `gh` CLI already authenticated in the
environment — `mlai` never manages GitHub credentials itself. `--draft`/
`--prerelease` are passed through unchanged. Only `github-releases` is
implemented; other deploy destinations are future, separately-designed
work.
```

- [ ] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document package deploy"
```

- [ ] **Step 4: Final full-workspace verification**

Run: `cargo test --workspace`
Expected: PASS on the local platform; CI verifies all three once pushed.

---

## Self-Review Notes

- **Spec coverage**: the GitHub Releases deploy adapter (one real, working destination, verified `gh` CLI syntax) is covered per `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`'s Decision 7. A general `DeployAdapter` trait for multiple destinations is deliberately not built ahead of a second real destination existing — over-generalizing now would guess at a shape no second implementation has validated yet.
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code, `gh` syntax verified via the research this plan's sibling (`mlai-package-foundation`) plan already confirmed.
- **Type consistency**: `DeployOptions`/`deploy_command`/`deploy` (Task 1–2) are defined once and consumed identically in the CLI layer.
