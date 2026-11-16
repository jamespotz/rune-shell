# GPUI migration implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Replace Iced entirely while preserving Rune Shell's current features.

**Architecture:** Keep service I/O and daemon channels intact. Separate shell state transitions from GPUI views, with one shared state entity and a window registry keyed by the existing surface kinds. Adapt existing domain logic rather than rewriting it.

**Tech stack:** Rust, GPUI with Linux Wayland layer-shell support, existing Tokio services.

**Spec:** `docs/superpowers/specs/2026-11-16-gpui-migration-design.md`

## Global constraints

- The current working tree, including eight modified files, is the baseline.
- Preserve CLI, socket protocol, config schema/defaults, persisted data, and surface namespaces.
- Deliver the whole shell on GPUI; internal checkpoints are not the final deliverable.
- Pin an immutable upstream revision with layer-shell support; published 0.2.2 lacks it.
- No permanent polling/redraw loop. No service I/O in view callbacks.
- Runtime parity requires Hyprland evidence; report unavailable checks explicitly.

## Review focus

- Delayed display discovery and unplugged outputs must not duplicate bars on the default monitor.
- Closing/reopening a search surface must restore keyboard focus and reset selection correctly.
- Unicode selection and IME composition must survive edits without invalid byte slicing.
- Late service replies after panel closure must update state without reopening windows.
- Channel shutdown and idle OSD expiration must work without a continuous frame loop.

## File responsibilities

- `crates/rune-ui/src/lib.rs`: preserve public events/actions and entry point; delegate application lifecycle.
- Create `state.rs`: shell snapshots, transitions, emitted effects; no GPUI types.
- Create `runtime.rs`: foreground event bridge and GPUI startup/shutdown.
- Rewrite `surfaces.rs`: GPUI windows and display reconciliation, preserving `Surface` names.
- Create `controls.rs`: reusable text input, sliders, toggles and dropdowns needed by existing pages.
- Create `animation.rs`: timed transitions respecting current config.
- Port existing `notch.rs`, `launcher.rs`, `settings.rs`, and feature view modules in place.
- Preserve `panel.rs` placement logic and `config.rs` schema; replace only toolkit coupling.

### Task 1: Verify the dependency and preserve rollback evidence

- [ ] Save `git diff --binary` outside the repository and record HEAD before editing.
- [ ] Run `cargo test --workspace --no-fail-fast`; confirm the 100-test baseline.
- [ ] Inspect immutable GPUI upstream revision `917559da35dd4d626205da705b071c800c9c2553` and matching platform package; select another immutable revision only if necessary and record why.
- [ ] In a temporary standalone Cargo project, compile and run that revision's layer-shell example. Verify transparent window, exclusive zone, output selection, and input modes. Record build/runtime packages required.
- [ ] Add matching pinned dependencies to `crates/rune-ui/Cargo.toml` only after the standalone build succeeds. Keep Iced internally until replacement is complete.
- [ ] Run `cargo check --workspace`; commit only migration changes, excluding pre-existing edits.

### Task 2: Extract state and preserve transition behavior

**Interface:** `state::ShellState` owns existing App snapshots; `apply(&mut self, event: UiEvent) -> Vec<Effect>` emits service actions, surface requests and quit. `Effect` variants carry existing `Action` and `Surface` values, with toolkit-independent size/margin data.

- [ ] Move existing behavior tests and add failing cases for late replies after panel close, Escape confirmation ordering, game-mode DND restoration, config-triggered weather/calendar/options actions, and output removal while a panel is open.
- [ ] Run `cargo test -p rune-ui`; confirm new cases fail before implementing extraction.
- [ ] Extract transition logic from `lib.rs`, retaining launcher/settings draft state and existing feature helpers. Replace `iced::Animation`, image handles and `Task` effects with state values and explicit effects.
- [ ] Remove the `Rgba -> iced::Color` conversion from domain config when callers are ported; keep TOML roundtrip/default tests unchanged.
- [ ] Run UI and workspace tests; inspect diff to ensure no service behavior changes. Commit this checkpoint.

### Task 3: GPUI runtime and window lifecycle

**Interface:** preserve `run(config: Config, events: UnboundedReceiver<UiEvent>, actions: UnboundedSender<Action>, hyprland: bool) -> Result<(), Error>`. `runtime.rs` owns event delivery; `surfaces.rs` translates state effects into windows.

- [ ] Add failing tests for channel closure, delayed display discovery, connector mapping, hotplug, idempotent open/close, panel switches, and stale window handles.
- [ ] Implement event-driven foreground delivery using an async channel adapter; never block GPUI on Tokio reception. Retain the existing daemon runtime ownership.
- [ ] Implement bar, panel, catcher, settings, popup and OSD options with the existing namespaces, anchors, keyboard modes and exclusive zones. Queue output reconciliation until discovery completes; diagnose unresolved mappings.
- [ ] Keep panel placement commands and measured size feedback. Implement outside-click dismissal and ordered window replacement.
- [ ] Run state/lifecycle tests and `cargo check --workspace`. On Hyprland verify two outputs, hotplug, transparent hit testing, focus restoration, and quit. Commit checkpoint.

### Task 4: Controls, input, animation and notch

- [ ] Add failing text-edit tests for Unicode ranges, selection replacement, IME composition/cancel, clipboard shortcuts and launcher reopening. Add slider clamp/commit and reduced-motion tests.
- [ ] Implement GPUI text input using its platform input-handler contract, with selection/caret and composition ranges. Implement buttons, sliders, toggles, scroll lists and dropdowns used by current UI.
- [ ] Port `notch.rs` preserving shoulders, natural-width measurement, widget ordering, workspace urgency and click/scroll actions. Add `animation.rs` using existing duration/reduced-motion config and scheduling only while needed.
- [ ] Cache image decode results outside render callbacks. Preserve configurable text/icon fonts and ligatures.
- [ ] Run tests; verify input and notch on Hyprland at integer and fractional scale. Commit checkpoint.

### Task 5: Launcher and feature panels

- [ ] Retain launcher matching, emoji/calculator, terminal command, clipboard favorite/clear and keybind tests. Add navigation boundary and empty/missing-service cases before changing views.
- [ ] Port `launcher.rs` to the controls from Task 4, preserving Enter, arrows, page navigation, search focus and result scroll position.
- [ ] Port `media.rs`, `notify.rs`, `tray.rs`, `weather.rs`, `calendar.rs`, `quick.rs`, `status.rs` and `bt.rs`, consuming shared ShellState snapshots and emitting existing actions.
- [ ] Preserve seek drag/commit, notification hover expiry and images/actions, nested/checkable tray entries, weather freshness, month/day navigation, quick toggles and two-click power confirmation.
- [ ] Add OSD expiry tests using a controlled clock, including no active animation, and hover pause/resume cases. Ensure late replies do not reopen closed panels.
- [ ] Run workspace tests. Exercise each feature-parity row from the spec on Hyprland, recording missing hardware/backend checks. Commit checkpoint.

### Task 6: All settings pages

- [ ] Preserve existing settings tests and add draft-invalid/reload interaction, Wi-Fi errors, Bluetooth scan close, and delayed thumbnail result cases before porting controls.
- [ ] Port all 12 pages in `settings.rs`. Preserve free-text drafts, palette and theme controls, wallpaper thumbnails, widget add/remove/reorder, weather lookup, Wi-Fi password flow, pairing codes, sound devices, calendar links, capture options, launcher options and animation settings.
- [ ] Keep settings save/apply requests routed through existing actions. Preserve initial service setup and scan lifecycle effects.
- [ ] Run UI/workspace tests. On Hyprland exercise every page with valid and invalid edits, config hot reload, service failure and close/reopen. Commit checkpoint.

### Task 7: Remove Iced and verify the complete shell

- [ ] Switch the public entry point exclusively to GPUI and remove obsolete Iced runtime, conversions, image handles and messages.
- [ ] Remove iced, iced_layershell, Iced-only winit pin, and root softbuffer patch; regenerate Cargo.lock. Leave unrelated vendored-file deletion out of scope.
- [ ] Run `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo test --workspace --no-fail-fast`, and `cargo build --release`. All must pass.
- [ ] Inspect `cargo tree --workspace` and `rg -n 'iced|iced_layershell|winit-core|softbuffer' Cargo.toml crates Cargo.lock`; confirm no active dependencies remain. Historical documentation mentions may remain where clearly historical.
- [ ] Complete the spec's runtime checklist on Hyprland, including all CLI commands, multi-output/hotplug, focus, text input/IME, reduced motion, missing services, image caching and idle CPU. Record exact unavailable checks instead of claiming parity.
- [ ] Measure memory and CPU against the baseline in the same session. Update `README.md` and `docs/ARCHITECTURE.md` with actual dependencies, GPU rendering and measured limits.
- [ ] Review the full migration diff against the baseline snapshot, run a fresh review and fix findings. Commit only migration changes and report validation evidence and remaining runtime limitations.

## Self-review

The tasks cover every feature row and compatibility requirement in the approved spec. State/effect interfaces are shared by runtime and views; existing service interfaces remain authoritative. Each review-focus condition has a test step in Tasks 2–5. Dependency verification precedes UI conversion, and Iced removal follows complete replacement coverage.


## Execution record

The migration implementation is complete and the default entry point uses GPUI.
See [verification record](../../gpui-migration-verification.md) for fresh build,
test, native compositor evidence and explicit hardware/performance limits.

Implementation decisions: passive service data is named `Snapshot`; `App` keeps
transitions and desired surface specs. `Effects` carries clipboard and quit work;
window effects are represented by reconciliation of desired `WindowSpec`s.
The final event pump polls daemon events, deadline updates and the active timer
without periodic idle polling. GUI callbacks defer state/window reconciliation
to avoid GPUI window-borrow reentrancy. A custom intrinsic content element measures
panels independently of their initial 1×1 compositor configuration.

The final work remains uncommitted so the user's pre-existing edits are not
included in an unsolicited implementation commit. The design-only commit made
before implementation remains unchanged. No deployment or shell restart was
performed.
