# Hyprland Settings Implementation Plan

**Status updated 2026-11-19:** Tasks 0–7 are implemented for the current scope. Displays is deferred. Documentation, automated checks and native UI checks pass; the live reload-replay check and real-dotfile setup/save check remain open under Task 9. Checked items below describe the implementation as shipped in the working tree, including departures from the original proposed API. Changes are uncommitted.

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

1. **Rune generates its overlay from stored state.** Owned values live under `[hyprland]` in Rune's config. Rune does not execute Lua or parse its generated overlay back into settings. A bounded read-only scan of literal user definitions supplies source labels and curve baselines; it does not evaluate arbitrary Lua.
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

### Task 0: Mockup approval (complete)

- [x] Adopt the approved mockup layout and copy for both looks. Later Window Motion changes are recorded under Task 7.

### Task 1: Schema (complete)

**Files:** `crates/rune-services/src/hyprland_config.rs`, `crates/rune-services/src/lib.rs`.

- [x] Define `OptionSpec`, `Kind` and `Section` for typed settings grouped by page.
- [x] Define `OPTIONS` for layout, scrolling, gaps, borders, rounding, opacity, blur, shadows and keyboard settings. Informational options are read-only.
- [x] Test unique keys, valid ranges and positive steps with `schema_is_consistent`; test IPC option decoding with `parses_getoption_replies`.

### Task 2: Stored state in Rune's config (complete for current scope)

**Files:** `crates/rune-ui/src/config.rs`, `crates/rune-services/src/hyprland_config.rs`.

- [x] Store `[hyprland]` as `Overrides`, with `BTreeMap`s for options, named curves and animation leaves. `Curve` supports springs and bezier control points; the untagged encoding preserves existing spring settings.
- [x] Sanitize against the fixed schema, clamp numbers, reject non-finite values and unsafe names/styles, limit stored curves to 32, and accept only known animation leaves. Choice strings must match an allowed value. Monitor state is deferred with Task 8.
- [x] Cover hostile keys, strings, names, non-finite numbers and oversized numeric values with `hostile_config_is_dropped_or_clamped` and bezier validation tests.

### Task 3: Lua output (complete for current scope)

**Files:** `crates/rune-services/src/hyprland_config.rs`.

- [x] Implement `Overrides::render()` for the generated file header, nested `hl.config` tables in sorted order, curve definitions and animation leaves. Monitor output is deferred with Task 8.
- [x] Implement `Overrides::preview()` and `Overrides::lua()` for live snippets. Previewed animation leaves include their curve dependencies, and referenced bezier presets are emitted automatically.
- [x] Test exact nested Lua output with `renders_nested_lua`, hostile-input rejection, live-baseline merging and bezier dependency emission. Keyboard layout is informational, so Rune does not emit arbitrary `kb_layout` strings.

### Task 4: Setup and save (complete; real-dotfile check remains in Task 9)

**Files:** `crates/rune-services/src/hyprland_config.rs`.

- [x] Resolve the Hyprland directory from nonempty `XDG_CONFIG_HOME`, falling back to `~/.config/hypr`.
- [x] Detect the require line with `requires_rune()`, ignoring commented-out lines.
- [x] Resolve symlinks, back up `hyprland.lua`, create a missing `modules/rune.lua`, and append the managed require line atomically. Repeated setup is idempotent.
- [x] Save the generated overlay atomically through the resolved path. The service explicitly reloads Hyprland after saving, then reads a new snapshot.
- [x] Verify symlink preservation, backup contents, idempotent setup and emitted overlay contents in temporary directories. Test require-line detection and errors naming a missing config path.

### Task 5: Service and live preview (implemented)

**Files:** `crates/rune-services/src/hyprland_config.rs`, `crates/rune-services/src/hyprland.rs`, `crates/rune-shell/src/main.rs`, `crates/rune-ui/src/lib.rs`, `crates/rune-ui/src/protocol.rs`.

- [x] Read option values and animations in `Hyprland::settings_snapshot()` using `j/getoption` and `j/animations`. Merge IPC bezier values with literal dotfile curve definitions and source labels.
- [x] Route snapshot/setup/save/discard requests through the service action channel. Read snapshots when opening Hyprland settings and after setup, save or discard.
- [x] Coalesce preview snippets through `spawn_preview()` and a `watch` channel. Surface rejected IPC replies in the settings error status.
- [x] Reapply the current touched preview when a compositor reload is observed outside game mode. Preview merging and model edits are tested; live replay across game-mode and square-corner reloads remains a Task 9 verification item.
- [x] Make rounding read-only in the pixel look, with an explanation that the look keeps window corners square.
- [x] Discard unsaved edits by reloading and reading a fresh snapshot.

### Task 6: Pages (complete for current scope)

**Files:** `crates/rune-ui/src/settings.rs`, `crates/rune-ui/src/gpui_settings.rs`.

- [x] Add Windows, Effects, Input, Window Motion and Review pages to the Hyprland group. Displays is deferred with Task 8.
- [x] Build option rows from `OPTIONS`, with source chips, dirty indicators, revert actions and four-sided gap fields.
- [x] Show the file that sets a value, found by a bounded read-only scan of `hyprland.lua` and its requires in load order. Only literal assignments count.
- [x] Add the pending bar with Live, change count, Review, Discard and Save. Save is disabled until setup is complete.
- [x] Add the setup card, proposed diff, Add to hyprland.lua action and Not now option.
- [x] Show generated Lua on Review, with pending output highlighted.
- [x] Inspect the running UI in smooth and pixel looks. The native all-pages test checks focus, Escape and validated text edits. Previews respect reduced motion.

### Task 7: Window Motion (complete, including subsequent curve editor work)

- [x] Draw spring response tiles with a canvas, including 3 px snapping in the pixel look, and provide stiffness and damping sliders.
- [x] Add a replayable spring preview using GPUI spring animation, settling immediately under reduced motion.
- [x] Seed animation rows from `j/animations`, with curve, speed and enabled controls. Curve references retain the `spring:` prefix internally.
- [x] Add all 19 hyprmod bezier presets, native default/linear choices, named custom bezier and spring curves, validated names and bounded control points.
- [x] Add draggable bezier handles, X1/Y1/X2/Y2 fields, preset loading, rename/delete actions and dotfile-curve revert. Rename/delete rewrites animation references; deleted names stay hidden from stale live snapshots. Built-in presets cannot be overwritten.
- [x] Keep the preset popup's scrolling inside the menu, including at either end. A native regression test verifies list scrolling, selection, dismissal and independent page scrolling in both looks.
- [x] Use two columns for the bezier editor: graph on the left, coordinates/presets/actions on the right. Keep a full-width preview below, sharing the spring preview's box, frame, gradient, pixel styling and replay behavior.
- [x] Test curve validation, Lua dependencies, lifecycle operations, stale snapshot handling, unique preset choices and sequential coordinate edits. Inspect the two-column layout in both looks.

### Task 8: Displays (deferred)

> `j/monitors` reports live VRR state rather than the per-monitor setting, and does not report bit depth or colour mode in config form. An incomplete `hl.monitor` override could reset settings Rune cannot preserve. Revisit when the monitor rule can be recovered reliably. Displays is outside the completed scope above.

- [ ] Add a monitor card with mode, scale, VRR and 10-bit settings.
- [ ] Apply monitor changes with a 15 s confirmation countdown, rolling back on timeout or Escape.

### Task 9: Docs and verification (partially complete)

- [x] Document overlay generation, literal source scanning, service boundaries, symlink-safe writes and named curves in `docs/ARCHITECTURE.md`.
- [x] Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo doc --workspace --no-deps`. Latest implementation verification on 2026-11-19 passed all four.
- [x] Run native GPUI tests individually: all pages and validated edits, plus preset-menu scrolling in smooth and pixel looks. Inspect the curve editor layout in both looks. Temporary screenshot tests were removed.
- [x] Read the running Hyprland snapshot successfully with the read-only integration test.
- [ ] Verify unsaved preview replay live across game-mode and square-corner reloads. Existing tests cover preview/model behavior and restoration of shell rules, but do not prove this full compositor path.
- [ ] Perform the real DP-1 setup/save/reload/revert check: change gaps, verify the live result, save, confirm overlay loading and the expected dotfile diff, then revert a key and save again. This check has not been performed because it writes to the user's dotfiles. Temporary-directory file tests pass; the user's Hyprland files and installed binary have been left untouched.
