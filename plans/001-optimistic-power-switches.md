# 001 — Flip Wi-Fi and Bluetooth power switches on click, not on service reply

- **Status**: DONE
- **Commit**: 514eec5 (working tree had uncommitted changes; line numbers are from that tree)
- **Severity**: HIGH
- **Category**: Purpose & frequency (immediate feedback)
- **Estimated scope**: 1 file (`crates/rune-ui/src/lib.rs`), ~40 lines plus 1 test

## Problem

The power switches only change when NetworkManager/BlueZ report back over D-Bus. The click sends a command and changes nothing, so the knob sits still, then slides late (BlueZ power can take over a second). That reads as jank.

```rust
// crates/rune-ui/src/lib.rs:460 — current
Message::WifiToggle(on) => {
    self.send(Action::System(system::Command::Wifi(on)));
    Effects::none()
}
// crates/rune-ui/src/lib.rs:507 — current
Message::BtPower(on) => {
    self.send(Action::Bluetooth(bluetooth::Command::Power(on)));
    Effects::none()
}
```

The settings page sends the same commands through `settings::Request::WifiPower(on)` and `settings::Request::Bluetooth(BtCommand::Power(on))` in the `Message::Settings(msg)` handler (`lib.rs:655` onward).

Views read the state from two copies:
- Panels/quick settings read `app.snapshot.network` (`wifi_enabled`) and `app.snapshot.bt.powered` (`gpui_panels.rs:347,390,727,812`).
- Settings reads `app.settings_ui.network` and `app.settings_ui.bt` (`gpui_settings.rs:1101,1232`).

## Target

On click, write the requested value into **both** copies right away, so every switch moves on the same frame. Remember the request with a 3 s deadline. When the service reports, its value overwrites the guess and the pending entry is dropped. If no report arrives by the deadline, revert both copies to the last reported value.

No view code changes. The switch animation (`gpui_widgets::switch`, a spring keyed by element ID) already animates whenever `on` changes.

## Repo conventions to follow

- Application state lives on the shell `App` struct (`lib.rs:140` onward); add fields next to `popup_size` / `popup_hover`.
- Timed state uses `Instant` deadlines checked in `Message::Frame(at)` (`lib.rs:739`), with wakeups requested from `next_wake` (`lib.rs:752`). Exemplar: `osd_until`.
- No `unwrap`/`expect` in production code. No code comments (AGENTS.md).

## Steps

1. Add fields to `App` and initialize them to `None` in `App::new`:
   ```rust
   wifi_pending: Option<(bool, bool, Instant)>,
   bt_pending: Option<(bool, bool, Instant)>,
   ```
   The tuple is `(requested, last_reported, deadline)`.
2. Add helpers on `App`:
   ```rust
   fn set_wifi_shown(&mut self, on: bool) {
       if let Some(n) = self.snapshot.network.as_mut() { n.wifi_enabled = on; }
       if let Some(n) = self.settings_ui.network.as_mut() { n.wifi_enabled = on; }
   }
   fn request_wifi(&mut self, on: bool) {
       let reported = self.wifi_pending.map_or_else(
           || self.snapshot.network.as_ref().is_some_and(|n| n.wifi_enabled),
           |(_, reported, _)| reported,
       );
       self.wifi_pending = Some((on, reported, Instant::now() + std::time::Duration::from_secs(3)));
       self.set_wifi_shown(on);
       self.send(Action::System(system::Command::Wifi(on)));
   }
   ```
   Write the same pair for Bluetooth: `set_bt_shown` sets `self.snapshot.bt.powered` and `self.settings_ui.bt.powered`, and `request_bt` sends `Action::Bluetooth(bluetooth::Command::Power(on))`.
3. Route every entry point through the helpers:
   - `Message::WifiToggle(on)` → `self.request_wifi(on); Effects::none()`
   - `Message::BtPower(on)` → `self.request_bt(on); Effects::none()`
   - In the `Message::Settings` request match: `settings::Request::WifiPower(on) => self.request_wifi(on)`. Also add `settings::Request::Bluetooth(bluetooth::Command::Power(on)) => self.request_bt(on)` **before** the generic `settings::Request::Bluetooth(cmd)` arm. Check the variant name in `settings.rs` (`BtCommand` is `rune_services::bluetooth::Command`).
4. Clear on report:
   - In `Message::Ui(UiEvent::Network(n))` (`lib.rs:531`), add `self.wifi_pending = None;` before the existing body. The incoming value is authoritative.
   - In `Message::Ui(UiEvent::Bluetooth(state))` (`lib.rs:619`), add `self.bt_pending = None;`.
   - Check where `self.snapshot.network` is assigned. It must still be overwritten by the incoming event; do not skip that.
5. Revert on timeout. In `Message::Frame(at)`, before the existing batch:
   ```rust
   if let Some((_, reported, deadline)) = self.wifi_pending && at >= deadline {
       self.wifi_pending = None;
       self.set_wifi_shown(reported);
   }
   ```
   Do the same for `bt_pending`.
6. In `next_wake`, return the earliest pending deadline when nothing sooner is due. Merge it with the existing logic using `min` and do not change the 16 ms animation branch.
7. Add a unit test in `lib.rs` `mod tests` using `test_app()`:
   - Seed `app.snapshot.bt.available = true; app.snapshot.bt.powered = false;`.
   - Call `app.update(Message::BtPower(true))`, then assert `app.snapshot.bt.powered && app.settings_ui.bt.powered`.
   - Call `app.update(Message::Frame(Instant::now() + Duration::from_secs(4)))`, then assert `!app.snapshot.bt.powered`.

## Boundaries

- Do NOT touch `gpui_widgets.rs`, `gpui_panels.rs`, `gpui_settings.rs`, or the services crate.
- Do NOT change the spring config or switch markup.
- If a step doesn't match the code (drift), STOP and report.

## Verification

- **Mechanical**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` (clean), `cargo test -p rune-ui` (all pass, including the new test).
- **Feel check** (release build, `cargo build --release`):
  - Quick settings → Bluetooth panel: the knob starts moving on the click frame, with no pause.
  - Double-click quickly: the knob reverses mid-slide smoothly and ends matching the real adapter state.
  - Wi-Fi switch in the panel and on the Settings → Wi-Fi page: same.
  - With the adapter blocked (`rfkill block bluetooth`), toggle on: the knob slides on, then back off after about 3 s.
  - Note whether the list appearing below causes a hitch (follow-up S2; do not fix here).
- **Done when**: the tests pass and all four switches move on click.
