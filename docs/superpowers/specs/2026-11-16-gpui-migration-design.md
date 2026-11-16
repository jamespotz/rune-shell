# Full GPUI migration

## Intent and scope

Replace Iced and iced_layershell with GPUI throughout Rune Shell, preserving the
current shell's features and configuration. The user selected a full migration
with feature parity rather than shipping a GPUI bar alongside Iced. This is a
framework replacement, not a redesign. No particular performance improvement was
requested; measure the result rather than assuming GPU rendering improves it.

The working tree, including its existing uncommitted changes, is the behavioral
baseline. Before migration, `cargo test --workspace --no-fail-fast` passed all
100 tests: 65 service tests, 34 UI tests, and one IPC test.

## Approaches

1. Port the bar first and retain Iced during rollout. This gives an earlier
   runnable checkpoint but leaves two UI implementations to maintain. The user
   declined this approach.
2. Replace the entire UI while retaining the existing service boundary. This
   requires rewriting approximately 7,400 lines of UI code but keeps service I/O,
   commands, config, and persisted data compatible. The user selected this scope.

Implementation may use internal checkpoints, but the delivered shell must run
entirely on GPUI. Do not ship placeholder panels or leave CLI commands silently
ignored to make the migration appear finished.

## Compatibility and ownership

- Keep `rune-services` responsible for I/O and Tokio tasks. Preserve the current
  `UiEvent` receiver and `Action` sender contract and `rune_ui::run` entry point
  where possible. Changes to UI error types are expected.
- Keep CLI syntax, socket protocol, surface namespaces, configuration schema,
  defaults, validation, config path, and hot reload behavior compatible.
- Existing config, theme backups, clipboard metadata, and service caches need no
  data migration. Do not rewrite them merely by launching the new UI.
- Config loading remains in the daemon. Settings keep invalid intermediate input
  as drafts, then send valid edits through the existing save/apply paths.
- Preserve all eight currently modified source files. Do not reset, overwrite
  from HEAD, stage, or commit unrelated work.

## GPUI dependency and platform

Target Linux Wayland under Hyprland. GPUI upstream has `WindowKind::LayerShell`
and options for namespace, layer, anchors, exclusive zone, margins, and keyboard
interactivity. The downloaded crates.io `gpui 0.2.2` source does not contain
layer-shell support, even though upstream main does. Version numbers alone are
therefore insufficient. Select and pin an immutable upstream revision containing
the required platform implementation and its matching platform crate. Verify
that revision builds before converting widgets. Never depend on floating main.

GPU rendering replaces tiny-skia. Discover and document actual build and runtime
dependencies, including native font and keyboard dependencies. Remove claims that
the app only needs libc if they no longer hold. A software renderer and the old
14 MB memory measurement are not acceptance requirements for GPUI.

## UI organization and event flow

Keep one shared shell state entity for service snapshots and application behavior.
Each window renders a view of that state for its surface kind. Separate state
transitions and action generation from GPUI rendering so existing behavior tests
can remain meaningful without opening a compositor connection.

Bridge daemon events onto GPUI's foreground executor without blocking it. Use an
event-driven async bridge, not a repeating `try_recv` poll. Channel closure and
`UiEvent::Quit` must terminate cleanly and release windows and subscriptions.
Window callbacks send the same service actions as the existing UI.

Retain and adapt toolkit-independent logic: panel placement, workspace IDs, fuzzy
matching, calculator and emoji results, date helpers, icon selection, draft
validation, and power confirmation. Remove Iced types from these modules.

Implement shared GPUI text input, buttons, toggles, sliders, lists, and dropdowns
only to the extent required by the existing controls. Text editing must support
selection, caret movement, clipboard shortcuts, Unicode, and IME composition.
Focus must follow the launcher/settings surface lifecycle. Search supports Enter,
Up/Down, Page Up/Down, and Escape with existing selection behavior.

## Windows and rendering

- One transparent top-layer bar per selected Hyprland output. Preserve monitor
  filtering, hotplug, connector-to-GPUI-display mapping, and the bar's exclusive
  zone. Without Hyprland, retain the existing compositor-selected single bar.
- Resolve displays after the platform has discovered outputs. Never silently
  place every requested monitor bar on the default output. If GPUI cannot map a
  requested connector, surface a diagnostic and treat multi-monitor parity as
  unverified until resolved.
- Keep `rune-bar`, `rune-panel`, `rune-catcher`, `rune-notifications`, `rune-osd`,
  and `rune-settings` namespaces for compositor rules.
- Keep one open panel and its transparent click-outside catcher. Preserve pointer
  placement, screen clamping, remembered measured sizes, monitor removal cleanup,
  resize, and ordered close/open on panel switches.
- Launcher surfaces use exclusive keyboard focus; settings and other panels keep
  their current on-demand behavior. Catcher and popups do not grab keyboard focus.
  OSD remains input-transparent.
- Preserve notch shoulders, configurable radius, fonts and icon ligatures,
  spacing, widget order, colors, transparency, workspace transitions, animated
  content width, and notification stack expansion. Use GPUI painting for custom
  geometry. Respect reduced motion and duration settings.
- Request frames while transitions or timed visual behavior need them; do not
  introduce a permanent redraw loop. Decode/cache art and icons when data changes.

## Feature parity checklist

| Area | Required behavior |
| --- | --- |
| Bar | All configured widgets, workspaces including urgency, title, clock, status badges, click/scroll actions, per-monitor active workspace |
| Launcher | Apps and Steam, fuzzy matching, emoji, calculator, terminal launch rules, keybind sheet, clipboard search, images, favorites, clear history |
| Media | Art, visualizer, metadata, playback controls, position updates, drag/commit seek |
| Notifications | Popup images and actions, urgency, hover pause/fan-out, dismiss, history, clear, DND |
| Tray | Icons, left/middle/right clicks, dbusmenu nesting, enabled/check states and item actions |
| Weather | Current conditions, hourly/daily forecast, location search, freshness refresh |
| Calendar | Month navigation, selected day, events, meeting links, refresh on open |
| Quick settings | Wi-Fi, Bluetooth, DND, game mode, mic, night light, keep awake, power profile, volume/brightness, shortcuts |
| Power and OSD | Existing two-click session-ending confirmation, lock/suspend behavior, confirmation reset, 1.5-second OSD expiration |
| Settings | All 12 pages: Appearance, Theme, Wallpaper, Bar, Weather, Wi-Fi, Bluetooth, Sound, Calendar, Capture, Launcher, Animations |
| Service lifecycles | Bluetooth scan only while its page is open, Wi-Fi scan on page open, startup calendar/weather/options initialization, errors and missing-service states |
| Theme/wallpaper | Existing presets/generation/export/restore controls, enabled-app choices, thumbnails and progress/results |
| Runtime controls | Every existing CLI toggle/open command, config reload, game-mode DND restoration, quit and single-instance handling |

## Verification and completion

1. Preserve the baseline's behavior tests, porting toolkit-specific assertions
   to state/action tests where necessary. Tests must not merely assert GPUI
   element construction. Add focused regression coverage for event bridging,
   surface reconciliation, focus/close transitions, and controls changed by the
   port.
2. Run formatting, workspace checks, tests, and the release build. Inspect the
   final dependency tree to prove Iced, iced_layershell, the Iced-only winit pin,
   and the softbuffer patch are no longer used.
3. On Hyprland, exercise every parity row, including text input, outside-click
   dismissal, focus restoration, sliders, images, transparency, hot reload,
   multi-monitor placement/hotplug, and fractional scale. Record exactly which
   checks were performed; compilation alone is not proof of feature parity.
4. Measure startup/idle memory and CPU on the same session, documenting GPU cost
   separately where measurable. Report regressions; do not invent a numeric
   performance target.
5. Update README and architecture/build instructions to describe the implementation
   that actually ships. Remove Iced-only code and dependencies after replacement
   coverage exists. Keep unrelated vendored file cleanup out of scope.

If an interactive Hyprland session or required hardware is unavailable, finish
the executable checks and clearly list the unverified runtime cases. Do not claim
full runtime parity without that evidence.

## Rollback

No persisted-data conversion is needed. Rollback means running the previous
binary or reverting only the migration changes while preserving the pre-existing
working-tree edits. Keep a baseline diff before editing to distinguish those
changes. Do not launch both binaries as simultaneous notification/tray owners.

## Decisions left to implementation

The implementer chooses the immutable GPUI revision, exact entity/view breakdown,
minimal control implementations, event-bridge adapter, animation helpers, and
test organization. The visible behavior and compatibility requirements above
constrain those choices. If upstream lacks a required operation, investigate a
small pinned upstream fix before proposing any loss of functionality.

## References

- [Upstream layer-shell example](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/layer_shell.rs)
- [Upstream layer-shell options](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform/layer_shell.rs)
- Behavioral baseline: current `crates/rune-ui/src`, `crates/rune-shell/src`, and
  `README.md`. The architecture document includes historical claims and is not
  the authority where it conflicts with current code.
