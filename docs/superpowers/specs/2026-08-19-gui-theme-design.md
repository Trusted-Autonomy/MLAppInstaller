# GUI Theme (System Dark/Light Mode): Design

**Status**: Approved 2026-08-19 (user specified the exact behavior directly: default to system, no end-user toggle, adopter-configurable override).
**Extends**: `docs/superpowers/specs/2026-08-15-gui-wizard-design.md`.

## Problem

`mlai-gui` currently has no theme handling at all: `styles.css` declares no explicit background/text colors (so it just inherits the WebView's default, effectively always light), and `#log-view` is hardcoded to a dark terminal look regardless of the rest of the page. The installed wizard should follow the end user's OS light/dark preference by default, with no in-app toggle for the end user to fight with the OS setting — but the adopter building a distribution (an engineer at an adopting project authoring `manifest.toml`) should be able to pin it to always-light or always-dark for their own branding reasons.

## Scope

**In scope:**
- A `[gui]` table in `manifest.toml`: `theme = "system" | "light" | "dark"`, defaulting to `"system"` when the table is absent — zero effect on every existing manifest.
- Real light/dark CSS custom-property tokens in `styles.css`, replacing the current no-color-declared-at-all state.
- Frontend logic in `main.ts` that reads `manifest.gui.theme` (already delivered via the existing `list_components` Tauri command, since `Manifest` is already sent wholesale to the frontend) and applies an explicit override when it's `"light"`/`"dark"`.

**Explicitly out of scope:**
- Native window chrome (title bar) theming. Tauri's title bar theme is a `tauri.conf.json` build-time setting; `mlai-package`'s current packaging flow wraps an already-built binary via `cargo packager` and has no hook to rewrite `tauri.conf.json` per distribution. Left following the OS automatically (Tauri's own default when `theme` is omitted from that config) regardless of the manifest override. A cosmetic title-bar/content mismatch is possible if an adopter forces one mode against an opposite system setting — acceptable, not solved here.
- Any end-user-facing toggle in the GUI itself — explicitly ruled out by the user.

## Architecture

### 1. Manifest schema (`mlai-core::manifest`)

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct GuiConfig {
    #[serde(default)]
    pub theme: Theme,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}
```

Added to `Manifest` as `#[serde(default)] pub gui: GuiConfig`. TOML:

```toml
[gui]
theme = "dark"
```

Absent `[gui]` table (every existing manifest) parses to `GuiConfig { theme: Theme::System }` via `#[serde(default)]` at both levels — fully backward compatible, matching the pattern already established for `supports_options_protocol`/`binds_to_project_type`.

### 2. Frontend (`mlai-gui`)

`loadComponents()` already receives the full parsed `Manifest` via `invoke<Manifest>("list_components")`. Extend the TypeScript `Manifest` interface with `gui: { theme: "system" | "light" | "dark" }`, and immediately after receiving it:

```typescript
if (manifest.gui.theme !== "system") {
  document.documentElement.dataset.theme = manifest.gui.theme;
}
```

No new Tauri command, no new IPC round-trip — the data is already there.

### 3. CSS (`styles.css`)

Light tokens as the unconditional default on `:root`; dark tokens applied two ways so both system-preference and explicit override work correctly together:

```css
:root {
  --bg: #fff;
  --fg: #111;
  --muted: #666;
  --border: #ccc;
  --log-bg: #111;
  --log-fg: #ddd;
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #1a1a1a;
    --fg: #eee;
    --muted: #999;
    --border: #444;
    --log-bg: #000;
    --log-fg: #ccc;
  }
}

:root[data-theme="dark"] {
  --bg: #1a1a1a;
  --fg: #eee;
  --muted: #999;
  --border: #444;
  --log-bg: #000;
  --log-fg: #ccc;
}

body { background: var(--bg); color: var(--fg); }
.muted { color: var(--muted); }
#log-view { background: var(--log-bg); color: var(--log-fg); }
```

This is the same three-layer pattern used for theme-aware pages elsewhere: a bare `:root` default (light), a media-query override guarded against an explicit `data-theme="light"` winning over a dark system preference, and an explicit `data-theme="dark"` override that wins regardless of system preference. `theme = "system"` (the default) never sets `data-theme` at all, so only the media query applies — exactly OS-follow behavior with no JS-level special-casing needed.

## Decisions

1. **Config lives in `manifest.toml`, not the packaging-time `DistributionProfile`** — it's a presentation concern the adopter sets once per distribution, and `manifest.toml` is already the one file `mlai-gui` reads at startup; adding a second bundled config file for one setting would be unwarranted.
2. **No new Tauri command** — the frontend already receives the full manifest; threading `theme` through is a one-field interface extension, not new plumbing.
3. **Native title bar theming is out of scope** — no clean hook exists in the current packaging flow; stated as an explicit, accepted limitation rather than silently ignored.
