# Hyprland Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Hyprland group in Rune Settings (Windows, Effects, Input, Window Motion, Displays, Review changes) based on hyprmod 0.6.0. Edits apply live. **Save** writes `~/.config/hypr/modules/rune.lua`, which `hyprland.lua` loads last with `require("modules.rune")`. Both the `smooth` and `pixel` looks are supported.

**Mockup:** `.superpowers/brainstorm/hyprland/hyprland-settings.html`. Add `#pixel`, `#effects` or `#ready` to the URL to deep-link. This file is not committed.

**Tech stack:** Rust, GPUI (vendored), Tokio, the Hyprland 0.57 IPC socket (`eval`, `j/getoption`, `j/animations`, `reload`).

## What hyprmod does (source read from hyprmod-0.6.0.zip)

hyprmod is Python + GTK4/libadwaita, about 31k lines. It ships as source, so no binary reverse-engineering was needed. The parts that matter here:

| Mechanism | hyprmod source | Rune equivalent |
|---|---|---|
| A single **overlay file** `hyprland-gui.lua`. It holds only the keys the user changed in the app, and other config files are never rewritten. | `core/config.py` `write_all`, `_build_document` | `modules/rune.lua`, created from Rune's own config |
| **First-run setup** adds `-- HyprMod managed settings` and `require("…")` to the end of `hyprland.lua`. The include check walks the require tree and resolves symlinks. | `core/setup.py` `run_setup`, `_append_lua_include`, `_tree_includes` | Task 4. Your `hypr/` files are symlinks into `~/dotfiles`, so the writer must follow them. |
| **Live preview** applies each key over IPC. In Lua mode it uses `keyword_to_lua`, then eval. | `core/state.py` `set_live` | `Hyprland::eval` already exists (`hyprland.rs:226`) |
| The **baseline** is the live value at startup. "Dirty" means changed in this session, so a manual `hyprctl` call doesn't show up as an unsaved change. | `core/state.py` `register` | Task 5 (`j/getoption`) |
| **Ownership**: a key is managed when it is in the overlay. *Remove override* drops the key and reloads. | `core/ownership.py`, `state.py` `remove_override` | The revert arrow in each row |
| After a reload, **unsaved values are applied again**. | `state.py` `reload_preserving_dirty` | Task 5. Rune's game mode and square corners both call `reload`. |
| A **Pending Changes** page, an undo stack and profiles | `pages/pending.py`, `core/undo.py`, `core/profiles.py` | Review page only. Undo and profiles are left out. |
| **Monitors** apply, then roll back on a timer unless confirmed | `pages/monitors/confirm_controller.py` | Task 8 |
| Section order: env, settings, curves, animations, monitors, …, exec last | `_build_document` | The emitter keeps the same order for the parts Rune emits |

Checked on this machine (Hyprland 0.57.2) by setting values to what they already were:
- `eval hl.config(...)`, `eval hl.curve(...)` and `eval hl.animation(...)` all reply `ok`.
- A bad value replies `error: … css_gap type requires an integer or a table …` and `hyprctl` exits non-zero. Show that text to the user.
- `getoption general:gaps_out -j` returns `"css": "0 16 16 16"` in the order **top right bottom left**.
- `j/animations` lists `name`, `overridden`, `bezier` (as `"spring:<name>"`), `speed`, `style` and `enabled` for every leaf.

## Decisions (where Rune differs from hyprmod)

1. **Rune never parses Lua.** Owned values are stored in Rune's config under `[hyprland]`, and `rune.lua` is created from them each time. hyprmod round-trips its overlay through a Lua subprocess. Rune doesn't need that, because the header of `rune.lua` says the file is generated.
2. **Keys come only from a fixed schema in Rust.** User input is only ever a *value*, which is clamped or chosen from a list. Strings go through the existing `lua_string`, and curve names must match `^[A-Za-z][A-Za-z0-9_]{0,31}$`. Nothing the user types can change the Lua that Rune generates.
3. **Your dotfiles are only ever read.** The one write outside `rune.lua` is the one-time `require` line, and it is shown as a diff before the user agrees.
4. **Theme-owned and unknown keys are read-only.** Border colours belong to the theme service (`Command::BorderColors`). Fork-only keys such as `decoration.blur.variant = "heat_shimmer"` are never emitted.
5. **Left out of v1:** keybinds, window and layer rules, autostart, env vars, workspaces, cursor, plugins, profiles, the undo stack, and deprecation migration. Your binds, rules and exec setup are Lua code with functions (`fn.resizer`, `cycle_layout`, `hl.on`), and an overlay can't safely round-trip them. Rune's keybind cheatsheet already shows the binds.

## Global constraints

- Follow `AGENTS.md`: no `unwrap`, `expect` or wrapping `as`. Keep `render()` declarative. Give rows stable domain IDs (`hypr-opt:{key}`, `hypr-curve:{name}`, `hypr-leaf:{leaf}`). Don't block the UI thread or Tokio workers. File IO goes through `tokio::fs` or `spawn_blocking`.
- Coalesce live applies through a `watch` channel, so dragging a slider sends only the latest value, never one IPC call per pointer move.
- Write files atomically with a temp file in the **resolved** target directory, then a rename onto the **resolved** path. Renaming onto `~/.config/hypr/hyprland.lua` itself would replace the dotfiles symlink with a plain file.
- Errors name the file or key, plus the Hyprland reply.
- Before finishing, run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo doc --workspace --no-deps`, and leave each clean.

## Review focus

1. `~/.config/hypr/hyprland.lua` and `modules` are **still symlinks** after setup and save (Task 4 test).
2. Running setup twice adds the line only once. A commented-out `-- require("modules.rune")` does not count as present.
3. Hostile `[hyprland]` config, such as unknown keys, `NaN`, ±1e9, a 10 kB string, or a curve name containing `")` or `os.execute`, is dropped or clamped before any Lua is generated (Task 2 and Task 3 tests).
4. Turning game mode or square corners off (both call `reload`) keeps unsaved previews (Task 5).
5. A Displays change rolls back unless it is confirmed within 15 s.

---

### Task 0: Mockup approval ✅ built

- [ ] The user picks the layout and copy in both looks. Only the decisions carry over to Task 6.

### Task 1: Schema

**Files:** create `crates/rune-services/src/hyprland_config.rs` (file ownership and Lua output form their own boundary) and register it in `lib.rs`.

- [ ] Add `pub struct OptionSpec { key: &'static str /* "general.gaps_in" */, kind: Kind, page: HyprPage, label: &'static str }` and `enum Kind { Int{min,max}, Float{min,max,step}, Bool, Choice(&'static [&'static str]), Sides{max} }`.
- [ ] Add `pub const OPTIONS: &[OptionSpec]` with about 25 rows, exactly the rows in the mockup: layout, scrolling column width and follow-focus, gaps in and out, border size, resize on border, rounding, both opacities, blur on/size/passes/vibrancy, shadow on/range, repeat delay and rate, numlock, kb_layout.
- [ ] Test: keys are unique, every `min < max`, and the IPC form (`key.replace('.', ':')`) round-trips.

### Task 2: Stored state in Rune's config

**Files:** `crates/rune-ui/src/config.rs`

- [ ] Add a `[hyprland]` section with `options: BTreeMap<String, Value>`, `curves: Vec<Spring { name, mass, stiffness, dampening }>`, `animations: Vec<Leaf { leaf, spring, speed, style, enabled }>` and `monitor: Option<MonitorSpec>`.
- [ ] Add `fn sanitize(&mut self)`. It drops keys missing from `OPTIONS`, clamps numbers, rejects non-finite floats, limits strings to 64 bytes and checks them against `Choice`, checks curve names against the regex, and allows at most 32 curves and 64 leaves.
- [ ] Write a failing hostile-input test first, covering the cases in review focus 3.

### Task 3: Lua output

**Files:** `hyprland_config.rs`

- [ ] Add `pub fn render(state: &HyprState) -> String`. It writes the header comment, then a single `hl.config({ … })` with nested tables in sorted key order (`Sides` is written as `{ top=, right=, bottom=, left= }`), then `hl.curve` calls, then `hl.animation` calls, then `hl.monitor` if present.
- [ ] Add `pub fn live_snippet(changed: &HyprState) -> String`. It uses the same output as `render`, and the result goes to `Hyprland::eval`.
- [ ] Golden test against the "Review changes" output in the mockup. A test that the output is the same on every run. A test that values are escaped, with `kb_layout = "us\"); os.execute(\"x"` passing through `lua_string`.

### Task 4: Setup and save

**Files:** `hyprland_config.rs`

- [ ] `fn hypr_dir()` returns `$XDG_CONFIG_HOME/hypr`, or `~/.config/hypr` if that variable is unset or empty.
- [ ] `fn is_set_up(text) -> bool` matches the regex `^\s*require\s*\(\s*["']modules\.rune["']\s*\)` per line. Commented-out lines don't match.
- [ ] `async fn setup()` canonicalizes `hyprland.lua`, copies it once to `hyprland.lua.rune-bak`, appends the comment and the `require` line, and writes atomically into the canonical directory. It creates an empty `modules/rune.lua` first, so the `require` never fails.
- [ ] `async fn save(state)` canonicalizes `modules/` and writes `render(state)` atomically. Hyprland's autoreload then picks the file up.
- [ ] Tests use a tempdir containing a symlinked `hypr/` and `modules/` dir: links are preserved, the line is added only once, a commented line is ignored, and a missing `hyprland.lua` returns an error naming the path.

### Task 5: Service and live preview

**Files:** `crates/rune-services/src/hyprland.rs`, `crates/rune-ui/src/backend.rs`

- [ ] Add `Hyprland::option(key) -> Result<Value>` (`j/getoption`, parsing the `int`, `float`, `css` and `str` variants) and `Hyprland::animations()` (`j/animations`).
- [ ] Add `Command::Snapshot`, which replies with the live baseline for every `OPTIONS` key plus the animations. It runs once when a Hyprland page opens and again after a reload.
- [ ] Add `Command::Preview` fed by a `watch::Sender<String>` snippet. The service evals only the latest value. A rejected reply goes back to the UI as an error toast that includes the reply text.
- [ ] Re-apply after a reload: `game_mode(false)` and `square_corners(false)` call `reload`, then eval the current dirty snippet. Failing test first.
- [ ] While square corners is on, the Rounding row is read-only, with the note "Square corners is on".
- [ ] Discard runs `reload`, then `Snapshot`.

### Task 6: Pages (Windows, Effects, Input, Review)

**Files:** `crates/rune-ui/src/settings.rs`, `gpui_settings.rs`

- [ ] Add `Page::{HyprWindows, HyprEffects, HyprInput, HyprMotion, HyprDisplays, HyprReview}` as a fourth `GROUPS` entry with the tints from the mockup. Update the `Page::ALL` test.
- [ ] Build rows generically from `OPTIONS`, using the existing `hero`, `group`, `flag!`, `number!` and segmented controls. Add a source chip ("Dotfiles" or "Rune"), a dirty dot and a revert button. Add a `Sides` control with four number fields.
- [x] The source chip shows the file that sets the value (`modules/general.lua`), found by scanning `hyprland.lua` and its `require`s in load order. Only literal assignments count, so a keybind changing the layout at runtime isn't mistaken for the source.
- [ ] Add a pending bar with Live, the change count, Review, Discard and Save. Save is disabled until setup is done.
- [ ] Add the setup card with the diff preview, an **Add to hyprland.lua** button and **Not now**.
- [ ] The Review page shows `render(state)`, with lines from dirty keys highlighted.
- [ ] Check both looks against the mockup in the running UI, plus keyboard focus and reduced motion.

### Task 7: Window Motion

- [ ] Show curve tiles drawn with a `canvas` that samples the spring step response. The pixel look snaps the curve to 3 px. Add stiffness and damping sliders.
- [ ] Add a preview box that animates on click using the shell's existing spring code, if there is any. It jumps straight to the end when reduced motion is on.
- [ ] Add animation rows: leaf, then a spring picker, speed and an enabled toggle. Seed them from `j/animations`, removing the `spring:` prefix.

### Task 8: Displays (deferred)

> Deferred during implementation. `j/monitors` reports the live `vrr` state, not the per-monitor setting (your config says `vrr = 3`, the reply says `false`), and doesn't report bit depth or colour mode in config form. A Rune `hl.monitor` override would silently reset whichever of those it didn't know. Revisit if Hyprland exposes the monitor rule over IPC.


- [ ] Add a monitor card (DP-1 from `j/monitors`) with mode, scale, VRR and 10-bit settings.
- [ ] Apply with `eval hl.monitor{…}`, then show a 15 s "Keep these display settings?" countdown that rolls back to the earlier values on timeout or Escape. The timer uses `cx.background_executor().timer`.

### Task 9: Docs and verification

- [ ] Explain the overlay design in `docs/ARCHITECTURE.md`: why Rune never parses Lua, and why writes follow symlinks.
- [ ] Do a manual check on DP-1: change gaps, check them live, save, check that `modules/rune.lua` loads after an autoreload, check that `git -C ~/dotfiles status` shows only `hyprland.lua` (the one line) and `modules/rune.lua`, then revert a key and save again.
