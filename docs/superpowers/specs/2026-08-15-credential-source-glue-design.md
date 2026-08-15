# Credential-Source Glue: Future Design Exploration

**Status**: On hold — exploration notes, not an approved spec. Do not implement against this document without a proper design review first.
**Supersedes**: Plan B's `mlai-credentials` vault + `mlai credential set` command (reverted 2026-08-15). See `docs/CONSTITUTION.md` §2.1 for why.

## The corrected problem statement

`mlai` is an installer. It runs once, installs components, and exits. It must never become a place secret *values* live, even transiently on disk. That was Plan B's mistake: it ported TA's `ta-credentials` vault (age-encrypted store, OS-keychain-first custody) into the installer itself, complete with a `mlai credential set` command. That pattern is correct for TA — a long-running agent runtime that must broker scoped, revocable credentials to untrusted agent processes repeatedly, hence session tokens and TTLs — and wrong for an installer, which has no ongoing relationship with the secret at all.

The real need, restated: when a component's manifest offers a hosted-model backend choice (cinepipe's existing local-vs-API pattern, generalized by this project's backend-options protocol), the user needs a way to tell that component *where to look* for its API key. The installer's job stops at collecting and passing through that pointer — never the value.

## Mechanism that (mostly) already exists

The backend-options protocol built in Plan B (`docs/SETUP-OPTIONS-PROTOCOL.md`'s `--describe-options` / `--set key=value`, generalized in `mlai-core::options_protocol`) is arguably sufficient as the transport, with no new code:

- A component's `--describe-options` output can declare an option like `{"key": "api_key_source", "label": "Where is your OpenAI API key?", "type": "choice", "choices": [{"value": "keychain"}, {"value": "1password"}, {"value": "env:OPENAI_API_KEY"}, {"value": "vault:secret/openai"}]}`.
- The user picks one; `mlai` passes it straight through: `--set api_key_source=keychain:service=my-app-openai`.
- The component's own setup script resolves that reference itself — shells out to `security find-generic-password` (macOS), `op read` (1Password CLI), reads the env var, hits a Vault path — using whatever tooling makes sense for that component. `mlai` never parses, validates, or interprets the reference string beyond passing it through unexamined.

If this is sufficient, there is no new crate to build at all — just documentation and maybe a small, optional convention for how source-reference strings are shaped (see "Open questions" below).

## Where this gets genuinely hard (why it's on hold, not done now)

- **Reference string format isn't designed.** `keychain:service=X` above is illustrative, not specified. Does it need a formal grammar? A `credential_source` sub-object instead of a flat string? Different providers (keychain vs. 1Password vs. Vault vs. plain env var) have different addressing needs (service+account vs. item UUID vs. secret path vs. var name) — a one-size string format may not fit all of them cleanly.
- **Should `mlai` validate the reference is *resolvable* before finishing install?** E.g., confirm the 1Password CLI is actually installed and authenticated, or that the named keychain entry actually exists — versus just passing the string through blind and letting the component's setup fail with its own error if the reference is bad. Validating means `mlai` needs to know *something* about each provider's shape (a small amount of provider awareness creeping back in) — a tension worth resolving deliberately, not by default.
- **Does `mlai` ever prompt to *create* the credential entry?** E.g., "you chose keychain but no entry exists yet — want to enter the key now and I'll write it to the OS keychain via the OS's own API?" This is subtly different from Plan B's mistake (a one-time, OS-native write during install, versus an installer-owned encrypted store it manages long-term) but needs its own scrutiny — is a one-time keychain write during install still "the installer touching secrets," and is that a meaningful distinction or a rationalization back into the same mistake?
- **Cross-platform provider parity.** macOS Keychain, Windows Credential Manager, Linux Secret Service, 1Password CLI, HashiCorp Vault, GitHub Actions secrets (a CI-context-only source, not applicable to interactive local installs) — which of these does v1 need to support meaningfully, versus just leaving the reference-string mechanism generic enough that a component can point at anything without `mlai` needing built-in knowledge of the provider at all?

## A plausible shape, not a decision

A `CredentialSource` enum in `mlai-core` — `Keychain { service: String }`, `Env { var: String }`, `Opaque(String)` (any other provider's own addressing scheme, passed through unexamined) — with `mlai` only ever constructing and passing through the reference, never resolving it. This is deliberately not fleshed out further here; it needs its own brainstorming session before becoming a plan.

## What NOT to do

- Do not build another installer-owned secret store, encrypted or not.
- Do not make `mlai` a dependency of a component's runtime secret-resolution code — the relationship is one-directional (installer → reference string → component), not a shared library both sides link against.
- Do not resurrect `mlai credential set` in any form without a real design review establishing why the installer needs to write a secret value anywhere, given the mechanism above.
