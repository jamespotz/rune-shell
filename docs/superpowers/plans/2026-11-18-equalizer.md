# Equalizer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A system-wide 10-band equalizer with presets, controlled from a new Settings → Equalizer page that works in both the glass and pixel looks.

**Architecture:** A new `rune-services/src/equalizer.rs` supervises a child `pipewire -c` filter-chain process and edits its band gains live through `pw-cli set-param`. The UI keeps `[equalizer]` in `Config`, edits it through `settings::Msg`, and sends band changes to the service on a `watch` channel, so a fast slider drag sends only the latest values. The vertical slider extends the existing `controls/slider.rs`.

**Tech Stack:** Rust, GPUI (vendored), Tokio, PipeWire filter-chain (builtin biquads), `pw-cli`, `pw-dump`, `pactl`.

**Spec:** `docs/superpowers/specs/2026-11-18-equalizer-design.md`

## Global Constraints

- Follow `AGENTS.md`: no `unwrap`/`expect`/wrapping `as` in production code, nothing done in `render()`, stable domain IDs (`eq-band:{hz}`, `eq-preset:{id}`), no blocking on the UI thread or Tokio workers.
- Check the exit status of every `pw-cli`/`pactl` call and include the command and status in the error.
- `rune-shell quit` must still return only after the daemon exits. With the EQ on, that now includes restoring the default sink and reaping the child. Read the shutdown paragraph in `docs/ARCHITECTURE.md` first.
- Write the config file atomically (temp file + rename) in `$XDG_RUNTIME_DIR/rune-shell/`, never in `~/.config/pipewire`.
- Looks come only from the existing helpers `corner`, `fill`, `frame`, `pixel_look`, `heading`.
- Before finishing, run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo doc --workspace --no-deps`, and leave each clean.

## Review Focus

1. **The default sink is never stranded on `rune_equalizer`** after disabling, quitting, a child crash, or a failed start. The restore always runs (Task 3 tests).
2. **Output switching while the EQ is on** retargets the chain instead of bypassing it (Task 5 test).
3. **Slider drags** coalesce through `watch`. No process is spawned per pointer-move event (Task 3 test).
4. **Hostile config:** 9 or 11 bands, `NaN`, or ±1e9 dB are rejected or clamped (Task 1 tests).
5. **The Sound page and volume widget** don't list "Rune Equalizer" as a device. Volume keys still work, acting on the EQ sink (Task 5).

---

### Task 0 (optional, recommended): HTML mockup of both looks

- [ ] Build a throwaway `crates/rune-ui/prototype-equalizer.html` like the Menu Bar one: glass and pixel looks, a ← → switcher, live sliders and preset chips. Open it in the preview.
- [ ] Get the look approved, then delete the file. Only the decisions carry over into Task 6.

### Task 1: Config and presets

**Files:** `crates/rune-ui/src/config.rs`

- [ ] Test first: `EqualizerConfig::default()` is off, `flat`, all zeros; parsing `bands` with 9 entries fails with `ConfigError::EqualizerBands`; `NaN` fails; `30.0` clamps to `12.0`; unknown `preset` reads back as `custom`.
- [ ] Add `pub struct EqualizerConfig { enabled: bool, preset: String, bands: [f32; 10] }` with `#[serde(default, deny_unknown_fields)]`, and add `equalizer` to `Config`.
- [ ] In `Config::parse`, validate the bands (finite, then clamp) and normalise `preset`.
- [ ] Add `pub const BANDS_HZ: [u32; 10]` and `pub const PRESETS: [(&str, &str, [f32; 10]); 9]` (id, label, gains) from the spec table, plus `fn preset_of(bands) -> Option<&str>`. Test that every preset round-trips through `preset_of`.

### Task 2: Filter-chain config generator (pure)

**Files:** Create `crates/rune-services/src/equalizer.rs`; modify `crates/rune-services/src/lib.rs`.

- [ ] Test first: `chain_conf(&bands, Some("alsa_output.x"))` contains 10 band nodes named `b1`…`b10` with the right labels and frequencies, the `pre` linear node with `Mult` = `10^(-max/20)`, links in series, `node.name = "rune_equalizer"` and `target.object = "alsa_output.x"`.
- [ ] Test that a sink name containing `"` or a newline is rejected (`None`), because it is interpolated into the SPA config.
- [ ] Test `set_param_args(&bands)` → the `Props` string for `pw-cli`, including `pre:Mult`.
- [ ] Implement both as pure functions, with no I/O.

### Task 3: Supervisor service

**Files:** `crates/rune-services/src/equalizer.rs`

- [ ] Define `pub enum Command { Enable { bands, output }, Disable, Retarget(String) }` and `pub fn spawn() -> (mpsc::UnboundedSender<Command>, watch::Sender<[f32; 10]>, JoinHandle)`. Bands travel on the `watch`, so the latest value wins.
- [ ] **Enable:** read `pactl get-default-sink` and remember it. Write the conf atomically with `tokio::fs`. Spawn `pipewire -c` with `kill_on_drop(true)`. Poll `pw-dump` for the node every 100 ms, giving up after 3 s. Then run `pactl set-default-sink rune_equalizer`. On any failure: `warn!` with the command and status, kill the child, restore the sink.
- [ ] **Band change** (`watch` changed): `pw-cli set-param <id> Props …`, checking the status. It must never spawn the child.
- [ ] **Disable or channel closed:** restore the remembered sink first, then kill the child and `wait()` for it.
- [ ] **Child exits unexpectedly:** `warn!`, restore the sink, restart with bounded backoff (1 s, 2 s, 4 s, then stop and report), as `AGENTS.md` requires for monitoring loops.
- [ ] Tests: put the process calls behind a small `Runner` trait (fake in tests; this is the one place where two implementations are expected). Cover: enable → disable restores the sink; a crash restores the sink; 100 band updates in a burst produce at most a few `set-param` calls; a failed start leaves the default unchanged.

### Task 4: Wiring and shutdown

**Files:** `crates/rune-ui/src/protocol.rs` (`Action::Equalizer`), `crates/rune-shell/src/main.rs`, `docs/ARCHITECTURE.md`

- [ ] Start the service on the Tokio runtime next to `audio::spawn_volume()`. Forward `Action::Equalizer` into it.
- [ ] At startup, if `config.equalizer.enabled`, send `Enable`.
- [ ] Shutdown: send `Disable` and await the service's join handle before the daemon exits, so `quit && rune-shell` never finds a stale `rune_equalizer` sink. Add one sentence about this to the shutdown paragraph in `ARCHITECTURE.md`.
- [ ] Test (in `rune-shell`): the shutdown path awaits the equalizer handle.

### Task 5: Audio integration

**Files:** `crates/rune-services/src/audio.rs`, `crates/rune-ui/src/settings.rs`

- [ ] Test first: device parsing drops the node named `rune_equalizer` from `outputs`, and marks the remembered sink as `default` while the EQ is on.
- [ ] With the EQ on, `DeviceCommand::SetDefault(true, name)` sends `Equalizer::Retarget(name)` instead of `set-default-sink`. Test that the request is routed.

### Task 6: Settings model

**Files:** `crates/rune-ui/src/settings.rs`

- [ ] Test first: `Msg::EqEnabled(true)` returns `Request::Equalizer(Enable…)`; `Msg::EqPreset("rock")` copies the bands and sets the preset; `Msg::EqBand(3, 4.5)` sets `preset = "custom"` unless the bands match a preset; `EqBand(10, _)` and `EqBand(_, NaN)` are ignored; `EqReset` → flat.
- [ ] Add `Page::Equalizer`: in `ALL`, in `GROUPS[0]` after `Sound`, tint `0x5e5ce6ff`, label `("equalizer", "Equalizer")`. Update the page-search test.

### Task 7: Vertical slider

**Files:** `crates/rune-ui/src/controls/slider.rs`

- [ ] Test first: `value_at` for the vertical axis (top = max, bottom = min, NaN/zero height → `None`).
- [ ] Add `orientation: Axis` (`Horizontal` by default) and `set_vertical()`. Map pointer `y` when vertical. Add a `center: Option<f32>` so the fill starts at 0 dB instead of at the minimum.
- [ ] Double-click emits `Change(center)`.
- [ ] Pixel look: draw 24 square cells through `fill`/`paint_quad`; cells between the centre and the value use the accent colour, the rest are dim. Square knob drawn with `frame`.
- [ ] Glass look: 4 px rounded track, accent fill from the centre, 16 px round knob with a shadow.

### Task 8: Equalizer page

**Files:** `crates/rune-ui/src/gpui_settings.rs`

- [ ] `Page::Equalizer` arm: hero with `toggle_switch` → preset chips (reuse the shelf-chip style from the Menu Bar page) → a band group of 10 sliders keyed `eq-band:{hz}`, each with its dB value above and its frequency label below → note and Reset (`self.action`).
- [ ] Sliders dispatch `Msg::EqBand`. The model update pushes the bands to the `watch` sender through the existing request path.
- [ ] When disabled, draw the band group at 0.5 opacity.
- [ ] Verify visually in the running shell in both looks (glass and pixel, Tiny5 and Monocraft) with grim screenshots. Play audio and check by ear that Bass Boost is audible and that turning the EQ off restores the original output. Report anything that couldn't be checked.
