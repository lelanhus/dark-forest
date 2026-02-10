# SPEC.md — Rust TUI Arcade Console

> **Mission:** Ship a terminal-native “game console” that feels like a GUI: fast, beautiful,
> extensible, and safe-by-default. Runs great on Raspberry Pi-class hardware. Ships with native
> games on day one, and grows into a sandboxed marketplace ecosystem.

---

## 1) Product definition (end state)

### 1.1 What the app *is*

A **console OS inside the terminal**:

* A polished **launcher shell** (Home, Library, Installed, Settings)
* A **game runtime** (input, timing, rendering) that makes games feel consistent and high-quality
* A **content system** (install/update/rollback/verify) that feels like a real package manager
* A **sandbox + permissions** model that enables a trustworthy marketplace
* A **creator toolchain** (SDK + packaging + publishing) so an ecosystem can form

### 1.2 What “complete” looks like

When fully realized, the app supports:

* **GUI-like UX**: panes, modals, command palette, smooth navigation, consistent chrome
* **Great games**: arcade + roguelike + UI-heavy games with reliable input and pacing
* **Multiple sources**: official curated marketplace + community + private registries
* **Safe installs**: sandboxed game execution (WASM), capability permissions, signatures, verified publishers
* **Reliability**: atomic installs, instant rollback, offline-first behavior
* **Creator velocity**: templates, dev hot-reload inside the shell, packaging + publishing commands

Non-negotiables:

* Never block the UI on I/O
* No flicker (diff rendering)
* Hot-load games without restart
* Security model is coherent (native is trusted; third-party is sandboxed)

---

## 2) Target platforms and constraints

### 2.1 Platforms

* Primary: Linux (x86_64, aarch64), macOS, Windows
* Priority device class: Raspberry Pi 500-class (aarch64 Linux)

### 2.2 Terminal reality constraints

* Monospace grid; ANSI/TrueColor varies by terminal
* No real transparency, blur, or arbitrary fonts
* Rendering must be conservative; rely on contrast/spacing, not heavy decoration

---

## 3) Tech and architecture decisions

### 3.1 Language and TUI

* Language: **Rust**
* TUI: **Ratatui**
* Terminal I/O: **Crossterm**

### 3.2 High-level architecture

* **Shell** (UI): navigation, lists, details panes, modals, command palette, settings
* **Runtime** (engine): input, timing, framebuffer, diff renderer, game runner
* **Content system**: install/update/remove/rollback, caching, integrity checks
* **Registry system**: multiple providers (builtin, local, remote), catalog and resolution
* **Plugin execution**: native (trusted), WASM (sandboxed), optional process (escape hatch)

### 3.3 Trust model

* **Built-in / native games = trusted**
* **Third-party games = sandboxed by default (WASM)**
* Optional **process plugins** require explicit enablement + warnings

---

## 4) UX: routes, overlays, and keymap

### 4.1 Routes (screens)

* **Home**: Continue, Featured, Recently Played, Updates
* **Library**: search, categories/tags, all sources (builtin + installed + marketplace)
* **Installed**: installed titles, versions, update/verify/rollback actions
* **Game Detail**: description, controls, permissions, versions, changelog, source, install/remove
* **Game Runner**: pane mode + fullscreen mode
* **Settings**: theme, performance, keybindings, registries, permissions, diagnostics

### 4.2 Global overlays (available anywhere)

* **Command Palette** (`Ctrl+K`)
* **Search** (`/`) (contextual)
* **Help** (`?`)
* **Notifications/Toasts**
* **Progress Drawer** (downloads/installs/verifications)
* **Error Details Modal** (with copy-to-clipboard if permitted)

### 4.3 Keybindings (default)

Global:

* Navigate: `↑/↓` or `j/k`
* Select: `Enter`
* Back/close: `Esc`
* Search: `/`
* Command palette: `Ctrl+K`
* Help: `?`
* Quit: `Ctrl+Q`

Runner:

* Pause: `P`
* Restart: `R`
* Exit game: `Esc` (confirm)
* Toggle fullscreen/pane: `F`

Keymap customization (v1+):

* Per-profile and per-game overrides
* Import/export keymap as a file

---

## 5) Visual design system — Theme “Forge”

### 5.1 Theme tokens

Neutrals:

* `bg`: `#0B0F0E`
* `panel_bg`: `#101816`
* `panel_bg_2`: `#0E1412`
* `fg`: `#E7ECEA`
* `muted`: `#A7B3AE`
* `faint`: `#6F7D77`
* `border`: `#22302B`

Primary accent (deep green):

* `accent`: `#1F8A5B`
* `accent_bright`: `#2FBF71`
* `accent_muted`: `#145A3D`

Secondary accent (rust):

* `rust`: `#B45309`
* `rust_bright`: `#D97706`
* `rust_muted`: `#7C2D12`

Status:

* `danger`: `#DC2626`
* `warning`: `#F59E0B`
* `success`: `accent_bright`

### 5.2 Usage rules

* Focus + selection = **green only**
* Rust = **brand spice** only (Featured/New badges, header emblem)
* Rust is never warning/error
* Most polish comes from spacing + contrast + consistent chrome

### 5.3 Standard chrome layout

* Left: list
* Right: details
* Bottom: status/hints + progress
* Modals: centered; backdrop simulated via muted palette

---

## 6) Performance and responsiveness

### 6.1 Golden rules

* UI is always responsive (async tasks + progress)
* Avoid full clears; diff rendering only
* Keep CPU and I/O light for Pi-class devices

### 6.2 Auto 30/60 policy

Default: **Auto**.

* Prefer 60 fps if render budget allows
* Drop to 30 fps under load
* Hysteresis prevents flapping

Suggested logic:

* Track rolling average render time (e.g., 60 frames)
* Avg render ≤ 10ms → target 60
* Avg render > 10ms repeatedly → target 30
* Require stability 2–3 seconds before switching back up

Settings:

* `Performance`: `Auto / 60 / 30`

---

## 7) Game runtime (engine) contract

### 7.1 Runtime model: state machine

Each game implements:

* `init(ctx) -> state`
* `update(state, event) -> state`
* `render(state, frame) -> ()`

Events:

* `Input(KeyEvent)`
* `Tick(dt_ms)`
* `Resize(w,h)`
* `FocusGained/FocusLost`
* `Pause/Resume`

### 7.2 Framebuffer model

* `Frame` is a 2D grid of cells:

  * `glyph` (char)
  * `fg`, `bg`
  * `attrs` (bold/dim/underline)

### 7.3 Renderer

* Mandatory diff renderer:

  * compute delta between last frame and next frame
  * repaint only changed cells
  * avoid flicker and reduce terminal bandwidth

### 7.4 Canvas modes

* Pane mode: game inside a panel with shell chrome
* Fullscreen mode: game takes full terminal; minimal HUD on demand

### 7.5 Timing

* Fixed timestep simulation is the default
* Render cadence can be capped (30/60) independent of tick rate

### 7.6 Input policy

* Raw mode active only while runner is focused
* Optional held-key abstraction (v0.1+)

---

## 8) Built-in native games (v0 shipping set)

Ship 3 games that set the quality bar:

1. **Snake+** (arcade loop + pacing)
2. **Tetris-like** (timing + input precision + panels)
3. **Micro Roguelite** (multi-pane UI: map/log/stats; turn-based)

Shared UX requirements:

* consistent pause menu
* restart/quit confirmations
* local high scores + basic stats (plays, last played)

---

## 9) Content system: installs, updates, rollback, integrity

### 9.1 Install model

* Versioned installs:

  * `games/<id>/<version>/...`
* Active pointer:

  * `games/<id>/current -> <version>/` (symlink or pointer file)

### 9.2 Atomic operations

* Download/unpack into temp directory
* Verify integrity
* Atomic rename into version folder
* Atomic flip `current`

### 9.3 Rollback

* Rollback means repoint `current` to previous version
* Rollback must be instant and reliable

### 9.4 Offline-first

* Installed games are fully usable offline
* Registry catalogs cached
* Artifact cache retained with pruning policy

---

## 10) Registry and marketplace model (multi-source)

### 10.1 Registry provider interface (conceptual)

* `list() -> [GameListing]`
* `resolve(id, version) -> ArtifactRef`
* `install(ArtifactRef) -> InstalledGame`
* `update(id) -> InstalledGame`
* `remove(id)`

### 10.2 Providers

v0:

* `builtin://` (built-ins)
* `local://` (install from local artifact folder)

v1+:

* `index://url` (curated JSON index; later signed)
* `github://owner/repo@tag` (release-asset / tarball)
* optional `oci://` artifacts

Current implementation (`v2.0`):

* `index://` + `github://` are shipped
* `oci://` remains deferred

### 10.3 Marketplace UX (end state)

* Categories, tags, search facets
* Featured collections (Best on Pi, Arcade, Roguelike)
* Compatibility badges (host api range, permissions)
* Verified/unverified labeling

---

## 11) Plugin system and execution types

### 11.1 Game manifest (`game.json`)

Fields:

* `id` (stable slug)
* `name`
* `version`
* `author`
* `entry_type`: `native | wasm | process`
* `entry`: path/ref (unused for built-ins)
* `host_api`: semver range (e.g., `^0.1`)
* `permissions`: list (capabilities + scopes)
* optional: `description`, `controls`, `tags`, `homepage`, `license`, `changelog_url`

### 11.2 Execution types

* **native**: built-in/trusted
* **wasm**: sandboxed default for third-party
* **process**: any-language escape hatch; disabled by default

---

## 12) Security and permissions (capability model)

### 12.1 Capability vocabulary

* `fs.read` (scoped paths)
* `fs.write` (scoped paths)
* `net` (none | allowlist)
* `open_url` (prompt)
* `clipboard` (prompt)
* `clock` (usually allowed)
* `random` (allowed)
* `terminal.raw_input` (implied for runner)

### 12.2 Permission UX

* Show permissions on install page
* Prompt on first use for sensitive capabilities
* Allow “remember choice”
* Settings → audit + revoke

### 12.3 Enforcement (v1+)

* WASM plugins receive only granted capabilities
* Process plugins (if enabled) are constrained as much as possible and clearly warned

---

## 13) Hot-load behavior

Watcher monitors:

* `games/**/game.json`
* `registry/installed.json`

On change:

* rescan manifests
* update Library/Installed immediately
* if running game updates: prompt Reload Now / Later

Reliability requirements:

* hot-load must not corrupt UI state
* install/update must never create half-installed games

---

## 14) Data model (host-managed)

Stored locally under `~/.dark-forest/` (exact file formats can evolve).

Core entities:

* **GameListing**: id, name, description, tags, author, source, permissions summary
* **InstalledGame**: id, installed_versions, current_version, source_ref, installed_at
* **PlayHistory**: last_played_at, play_count
* **HighScores**: per game table
* **Settings**: theme, performance, keymap, registries, security toggles
* **PermissionsGrants**: granted scopes per game
* **PublisherKeyring**: trusted publisher public keys
* **KeymapProfiles**: profile bindings + per-game runner overrides

Current on-disk schema baseline: `3` (legacy schema roots are backed up and reinitialized).

---

## 15) Creator experience (v1.1+ → v2+)

### 15.1 SDK goals

* Shared primitives for:

  * tile maps / glyph spritesheets
  * palette management
  * common UI overlays (pause menu, dialogs)
  * deterministic randomness helpers

### 15.2 CLI tooling (end state)

* `dev`: run a game inside the shell with hot reload
* `pack`: build artifact + manifest + hashes
* `publish`: submit to a registry (official or custom)
* `verify`: verify signatures/hashes

### 15.3 Quality gates (marketplace)

* declared permissions required
* host api range valid
* determinism tests (optional but encouraged)
* performance budget checks (frame time)

Current CLI enforcement in creator `publish`:

* `permissions` must be explicitly declared in `game.json` (array form)
* `host_api` must be a valid semver range compatible with host API `0.1.0`

---

## 16) Testing and quality strategy

* Unit tests for runtime primitives
* Snapshot tests for frame output (golden frames)
* Replay tests: record input events and ensure deterministic outcomes
* Fuzz testing for manifest parsing and registry data
* Manual QA checklist for terminals (truecolor/no-truecolor, resize, mouse)

---

## 17) Distribution and release

* Ship as a **single native binary per OS/arch**
* Primary targets:

  * `linux-aarch64` (Pi)
  * `linux-x86_64`
  * `macos` (arm64/x86_64 as feasible)
  * `windows-x86_64`

Release expectations:

* reproducible builds where possible
* signed artifacts (v2+)
* changelog included

---

## 18) Roadmap (full lifecycle)

### Phase 0 — Foundation

* Repo structure, CI, release artifacts
* Logging + crash capture

### Phase 1 — Shell skeleton

* Routes + overlays + Forge theme
* Command palette + progress drawer

### Phase 2 — Runtime v0

* Event model, framebuffer, diff renderer
* Runner modes + auto 30/60
* Minimal replay harness

### Phase 3 — Ship native games (v0.1)

* Snake+, Tetris-like, Micro Roguelite
* Shared UX overlays and high scores

### Phase 4 — Local installs + hot-load (v0.2)

* local provider, install/update/remove
* watcher + atomic flips + reload prompts

### Phase 5 — Registry abstraction + first remote provider (v0.3)

* `index://` or `github://` provider
* caching + error recovery

### Phase 6 — Permissions enforcement + WASM plugins (v1.0)

* enforce capability grants for WASM
* permission prompts + audit + revoke

### Phase 7 — Creator tooling (v1.1+)

* templates, dev hot reload, pack/publish

### Phase 8 — Marketplace maturity (v2.0, complete)

* multi-registry (`index://`, `github://`), verified publishers, signatures
* collections, compatibility badges, install reproducibility

### Phase 9 — Console OS maturity (v3.0+)

* profiles, kid mode (optional)
* mods, replays sharing (optional)
* gamepad support (optional)

---

## 19) Definitions of Done (key releases)

### DoD — v0.1

* shell polish + runtime stable
* 3 games shipped
* cross-platform binaries in CI

### DoD — v0.2

* local artifacts supported
* hot-load reliable
* atomic installs + rollback

### DoD — v1.0

* WASM third-party plugins supported
* permissions enforced + revocable
* at least one remote registry provider

---

## 20) Risks and mitigations

Terminal variability:

* Conservative rendering + capability detection + diagnostics screen

Performance/flicker:

* Diff renderer mandatory + render throttling + perf indicator

Plugin security:

* WASM default + capability enforcement + signatures (v2)

Supply chain:

* install by version + hash; cache artifacts; prefer release assets
