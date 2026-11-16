# Architecture

## Data flow

```
            tokio runtime (2 worker threads)                 main thread (calloop)
 ┌───────────────────────────────────────────┐        ┌──────────────────────────────┐
 │ hyprland::watch ──watch<HyprState>──┐     │        │ rune_ui::run                 │
 │ clock::spawn ─────watch<DateTime>───┼─fwd─┼─chan──▶│  UiEvent → state → redraw    │
 │ config watcher (notify) ──Config────┤     │        │  if the item list changed    │
 │ control socket (rune-shell.sock) ───┘     │        │                              │
 │ action loop ◀────────mpsc<Action>─────────┼────────┤  pointer click → Action      │
 └───────────────────────────────────────────┘        └──────────────────────────────┘
```

- **Services** own every piece of I/O. Each one publishes a single `watch` value. The
  forwarder task in `main.rs` pushes each new value into the UI's calloop channel.
- **UI** keeps the latest copy of every state, builds `Item`s for each bar, and repaints only
  when those items differ from what is already on screen. When idle it makes no syscalls apart
  from the clock tick, which redraws once a minute.
- **Actions** (clicks) travel back through an `mpsc`, and the daemon runs them on tokio.

## Decisions

**UI: iced + iced_layershell, rendered in software (tiny-skia).** Phase 1 hand-drew the bar
with smithay-client-toolkit, as yami does. The feature list that followed (a settings window,
launcher search, sliders, a calendar, animations everywhere) needs a real widget toolkit,
so the UI moved to iced, which is pure Rust. Measured with a test build on this machine:

| backend | memory | idle CPU |
|---|---|---|
| iced, GPU (wgpu) | ~190 MB | small but nonzero |
| iced, software (tiny-skia) | ~14 MB | none |
| old hand-drawn version | ~10 MB | none |

Software rendering is the default (`ICED_BACKEND=wgpu` switches to the GPU). Notches are opened
and closed to match Hyprland's monitor list, which also covers hotplug.
Known wrinkle: iced_layershell 0.19.1 doesn't compile against winit-core beta.4, so
`rune-ui/Cargo.toml` pins beta.2. Remove the pin once that's fixed upstream.

**Hyprland IPC: a hand-written client (~150 lines), not the `hyprland` crate.** Hyprland 0.56
changed `dispatch` so that it evaluates its payload as Lua (`hl.dsp.focus({ workspace = 3 })`),
and the old `dispatch workspace 3` form now returns a Lua syntax error. A wrapper crate that
hasn't caught up breaks without warning, while a payload we own is one line to fix.
Events are used only as a trigger to re-query `j/monitors` and `j/workspaces` (bursts are
coalesced over 5 ms), so new or renamed events can't leave the state stale. The single
exception is `urgent`, which carries data that no query exposes.

**Audio: not decided yet (Phase 2).** The mainstream crates (`pipewire`, `libpulse-binding`)
link against C. The no-C route is to speak the PulseAudio wire protocol to `pipewire-pulse`
using a pure-Rust protocol crate. If that turns out immature, `pipewire-rs` is the
pragmatic fallback, with one C dependency.

**Config: TOML, with the palette defined in config instead of CSS.** There's no CSS engine
without GTK. `deny_unknown_fields` catches typos.

## Risks

- **Hyprland IPC churn.** Mitigated by the query-driven design above. The JSON field names
  `lastwindowtitle` and `activeWorkspace` are the remaining coupling.
- **Keyboard input.** iced_layershell loads libxkbcommon at runtime rather than linking it,
  and every Hyprland system already has it. The settings window's text fields already use it.

## Roadmap

1. **MVP bar** *(done)*: a bar per monitor with hotplug, workspaces (active, occupied,
   empty, urgent, click to focus), window title, clock, control socket and CLI, TOML
   config with hot reload.
1b. **iced + settings** *(done)*: notch rebuilt in iced, Material icons, animated workspace
   pills, settings window (Appearance, Bar with widget add/remove/reorder, Animations with
   reduce motion), and settings saved to `config.toml`.
1c. **Launcher** *(done)*: the notch expands into the panel; apps from `.desktop` files and
   Steam manifests, icons from the GTK icon theme (indexed once, ~0.3 s), nucleo fuzzy
   matching, emoji, calculator, and the keybind cheat sheet from `j/binds`. Apps launch via
   `hl.dsp.exec_cmd`.
1d. **Music** *(done)*: MPRIS over zbus (signals trigger a re-read; position extrapolated),
   art as bytes (local or curl-cached), cava visualizer started only while playing, compact
   widget plus an expanded player in the notch.
1e. **Wallpaper and theming** *(done)*: awww/mpvpaper wallpapers with cached 16:9 thumbnails;
   presets plus Material You schemes (material-colors) from the wallpaper; exporters for 12 apps
   that write `rune` files plus a one-line include or key; a backup store (manifest written
   before each change) that restores files, symlinks, created folders and gsettings; Hyprland blur
   via a runtime `eval` layer rule. Not yet: Firefox, Chrome and Vivaldi.
1f. **Notifications and tray** *(done)*: notification daemon (polite name request, expiry with
   hover pause, history, DnD, actions, image-data), Sonner-style popup stack on its own layer
   surface sized to the stack, notification center panel; StatusNotifierWatcher (when free) plus
   host, item icons (theme, IconThemePath or pixmaps), dbusmenu menus as a notch panel.
1g. **Weather and Bluetooth** *(done)*: Open-Meteo via curl (geocoding search, current/hourly/
   daily), widget plus forecast panel; BlueZ over the system bus (object-manager re-reads,
   non-default confirm-only agent for our own pairings, trust = auto connect), PipeWire profile
   switching through `pactl`, scanning tied to the settings page.
1h. **Capture, game mode, calendar** *(done)*: grim/slurp/gpu-screen-recorder/tesseract driven
   from the control socket (config re-read per capture), results announced through whichever
   notification daemon runs; game mode via `hl.config` eval and `reload` to restore; ICS calendar
   with RRULE expansion (DAILY/WEEKLY/MONTHLY/YEARLY, BYDAY incl. ordinals, BYMONTHDAY, COUNT,
   UNTIL, EXDATE, RECURRENCE-ID), chrono-tz zones, meeting-link detection.
1i. **Status, OSD, quick settings, power, clipboard** *(done)*: volume via `pactl subscribe`
   (no polling), NetworkManager and UPower over the system bus, brightnessctl; an OSD layer
   surface hidden by the frame clock; quick-settings and power panels in the notch; cliphist
   history through the launcher's search.
2. **Full bar**: MPRIS, UPower battery, NetworkManager, audio volume and the
   StatusNotifierItem tray, all over zbus (pure Rust). Each service is optional: it
   publishes `None` when its backend is missing, and its widget hides. Adds icons
   (resvg + an icon-theme lookup) and hover cursors.
3. **Launcher**: `.desktop` parsing, fuzzy matching (`nucleo`), and an overlay layer surface
   with keyboard focus. Launches through `hl.dsp.exec_cmd`. `rune-shell toggle launcher`
   opens it.
4. **Notifications**: `org.freedesktop.Notifications` on zbus, popups, actions, urgency,
   and a history panel.
5. **Polish**: animate the notch width on resize (Dynamic Island style; it currently snaps), volume/brightness OSDs, power menu, quick settings, hyprlock integration,
   wallpaper, clipboard history, fractional scaling.
