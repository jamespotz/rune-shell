# Widget Styles, Device Battery and Colour Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each bar placement styleable: border, radius, icon colour and a custom icon. Add a per-device wireless battery widget fed by UPower, and an HSV+alpha colour picker that opens from every colour swatch in Settings.

**Architecture:**

- `BarItem` replaces `Widget` in the bar lists. It is serde-untagged, so a plain name still parses.
- A new Tokio service publishes `Vec<Peripheral>` over `watch`, forwarded as a `UiEvent`.
- Rendering resolves each item's style through a single helper.
- Settings gains `Msg::Item` edits and a `ColorPicker` GPUI entity hosted by `SettingsView`.

**Tech Stack:** Rust, GPUI (vendored), zbus on Tokio, serde/toml.

**Spec:** `docs/superpowers/specs/2026-11-20-widget-styles-device-battery-design.md`

## Global Constraints

- Follow `AGENTS.md`:
  - no `unwrap`/`expect`/wrapping `as` on outside values;
  - D-Bus stays on Tokio;
  - `render()` stays declarative;
  - stable IDs on interactive elements;
  - both looks, styled via the `gpui_widgets` look helpers.
- The working tree has uncommitted polkit work (`polkit.rs`, `gpui_polkit.rs` and edits across `rune-ui`). Preserve it.
- Don't commit unless the user asks.
- Limits:
  - `border_width` 0..=4;
  - `radius` 0..=bar.height/2 (`None` = 5 px);
  - `icon` ≤ 64 chars, `device` ≤ 128 chars;
  - ≤ 64 items per bar section;
  - ≤ 32 peripherals, peripheral strings ≤ 128 chars, percent 0..=100.
- The low-battery threshold is ≤ 15 % (urgent colour).
- UPower peripheral reconnect backoff is 1 s doubling to 30 s, plus a 60 s verification re-read.
- `examples/config.toml` must still equal `Config::default()`.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo doc --workspace --no-deps`, all clean.

## Review Focus

1. **A styled item is dragged to another lane.** Its style and device must travel with it, because `Place` moves the whole `BarItem`. Test: Task 4 `place_keeps_item_style`.
2. **The selected chip's item is removed or moved while its settings are open.** The selection clears instead of editing a neighbour. Test: Task 4 `selection_clears_on_layout_change`.
3. **A config with a hostile table entry** (`border_width = inf`, `radius = -3`, a 10 kB `icon`, 500 items) loads bounded rather than failing or blowing up layout. Test: Task 1 `hostile_item_styles_are_bounded`.
4. **The saved device is disconnected.** The widget renders nothing, and the settings row shows "<key> (not connected)". Tests: Task 3 `device_battery_hidden_when_absent` and Task 4 `device_choices_include_absent_saved_key`.
5. **Hex typed in the picker is invalid or partial** (`#12`). The picker keeps its colour and the config is unchanged until the hex is valid. Test: Task 5 `partial_hex_keeps_color`.

---

### Task 1: `BarItem`, `WidgetStyle` and `Widget::DeviceBattery` in config

**Files:**

- Modify: `crates/rune-ui/src/config.rs`.
- Migrate every user of `bar.left/center/right` so the workspace compiles:
  - `notch.rs` (`bar_has`);
  - `runtime.rs:191-195`;
  - `gpui_notch.rs` (loop and test at ~306);
  - `settings.rs` (`section_mut`, `Msg::Add/Insert`, tests ~1755-1815);
  - `gpui_settings.rs` (`section_items`, `bar_preview`, `bar_layout`, `DraggedWidget`, `widget_glyph`);
  - `gpui_widgets.rs`.
- Update `examples/config.toml` comments only.

**Interfaces:**

- Produces:
  - `pub struct WidgetStyle { pub icon: String, pub icon_color: Option<Rgba>, pub border_width: f32, pub border_color: Option<Rgba>, pub radius: Option<f32> }`, with `Default`, `PartialEq`, `serde(default, deny_unknown_fields)`.
  - `pub struct BarItem { pub widget: Widget, pub style: WidgetStyle, pub device: String }`.
    - It deserializes from a `Widget` name or a table that has the `widget` key plus the `WidgetStyle` fields and `device` flattened at the same level.
      Use a private `#[serde(untagged)] enum BarItemRepr { Name(Widget), Table(BarItemTable) }`, with `BarItemTable` flat and `deny_unknown_fields`.
    - It serializes as the name when `style == default && device.is_empty()`, otherwise as the flat table.
  - `impl From<Widget> for BarItem`.
  - `BarConfig::{left, center, right}: Vec<BarItem>`.
  - `Widget::DeviceBattery` (serde `device_battery`, Display "Device battery"). `Widget::ALL` grows to 18.
  - `WidgetStyle::MAX_BORDER_WIDTH: f32 = 4.0`.
  - `BarConfig::MAX_ITEMS: usize = 64`.
  - `BarConfig::sanitize` bounds each item's style and device per the Global Constraints.
- In `gpui_settings.rs`, `widget_glyph(Widget::DeviceBattery) = "battery_full"`.
- `DraggedWidget` carries a `BarItem`. Settings `Msg::Add`/`Msg::Insert` take a `BarItem`.

- [ ] **Step 1: Write failing tests** in `config.rs` `mod tests`.

```rust
#[test]
fn bar_items_parse_from_names_and_tables() {
    let c = Config::parse(r##"[bar]
right = ["clock", { widget = "device_battery", device = "F4:73", icon = "mouse", icon_color = "#ff000080", border_width = 1.5, radius = 4.0 }]"##).unwrap();
    assert_eq!(c.bar.right[0], BarItem::from(Widget::Clock));
    let item = &c.bar.right[1];
    assert_eq!(item.widget, Widget::DeviceBattery);
    assert_eq!(item.device, "F4:73");
    assert_eq!(item.style.icon, "mouse");
    assert_eq!(item.style.icon_color, Some(Rgba([0xff, 0, 0, 0x80])));
    assert_eq!((item.style.border_width, item.style.radius), (1.5, Some(4.0)));
    assert_eq!(Config::parse(&c.to_toml()).unwrap(), c);
    assert!(Config::parse("[bar]\nright = [{ widget = \"clock\", colour = \"#fff\" }]").is_err());
}

#[test]
fn default_items_serialize_as_plain_names() {
    let saved: toml::Value = toml::from_str(&Config::default().to_toml()).unwrap();
    assert_eq!(saved["bar"]["left"][0].as_str(), Some("workspaces"));
}

#[test]
fn hostile_item_styles_are_bounded() {
    let long = "x".repeat(10_000);
    let many = vec!["\"clock\""; 500].join(",");
    let c = Config::parse(&format!(
        "[bar]\nheight = 32\nleft = [{many}]\nright = [{{ widget = \"device_battery\", icon = \"{long}\", device = \"{long}\", border_width = inf, radius = -3.0 }}]"
    )).unwrap();
    assert_eq!(c.bar.left.len(), 64);
    let s = &c.bar.right[0].style;
    assert_eq!(s.icon.chars().count(), 64);
    assert_eq!(c.bar.right[0].device.chars().count(), 128);
    assert_eq!(s.border_width, 0.0);
    assert_eq!(s.radius, Some(0.0));
    let c = Config::parse("[bar]\nright = [{ widget = \"clock\", border_width = 99, radius = 99 }]").unwrap();
    assert_eq!((c.bar.right[0].style.border_width, c.bar.right[0].style.radius), (4.0, Some(16.0)));
}
```

Also update the existing tests (`capture_widget_parses`, `round_trips_through_toml`, `parses_overrides`, and the settings/notch tests) to compare against `BarItem::from(Widget::X)`.

- [ ] **Step 2: Run tests to verify they fail.** Run `cargo test -p rune-ui config::`. Expected: compile errors (`BarItem` is not defined).

- [ ] **Step 3: Implement the types and sanitize, then migrate the call sites.** Call sites that only need the widget read `item.widget`.
  `bar_has(bar, w)` checks `item.widget == w`.
  The shelf's "used" check compares `.widget`.
  `Msg::Add/Insert` call sites wrap with `BarItem::from(widget)`.

- [ ] **Step 4: Run tests to verify they pass.** Run `cargo test --workspace`. Expected: PASS, including `example_config_matches_defaults`.

---

### Task 2: UPower peripherals service

**Files:**

- Modify `crates/rune-services/src/system.rs`: add types, the parser, `spawn_peripherals` and tests.
- Modify `crates/rune-ui/src/protocol.rs`: `UiEvent::Peripherals(Vec<Peripheral>)`.
- Modify `crates/rune-ui/src/state.rs`: `pub peripherals: Vec<Peripheral>`, default empty.
- Modify `crates/rune-ui/src/lib.rs`: add it to the redraw list near line 293, and a handler next to `UiEvent::Battery` at ~707.
- Modify `crates/rune-shell/src/main.rs`: forward next to `spawn_battery`.

**Interfaces:**

- Produces in `rune_services::system`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PeripheralKind { Mouse, Keyboard, Phone, Tablet, GamingInput, Pen, Touchpad, Headset, Speakers, Headphones, Other }`.
  - `impl PeripheralKind { pub fn from_upower(kind: u32) -> Option<Self>; pub fn icon(self) -> &'static str }`.
    - `from_upower` returns `None` for 1, 2 and 3.
    - Mapping: 5 Mouse, 6 Keyboard, 8 Phone, 10 Tablet, 12 GamingInput, 13 Pen, 14 Touchpad, 17 Headset, 18 Speakers, 19 Headphones, everything else Other.
    - Icons are the spec table: `mouse`, `keyboard`, `smartphone`, `tablet`, `sports_esports`, `stylus`, `touchpad_mouse`, `headset_mic`, `speaker`, `headphones`, `battery_full`.
  - `#[derive(Debug, Clone, PartialEq)] pub struct Peripheral { pub key: String, pub name: String, pub kind: PeripheralKind, pub percent: f64, pub charging: bool }`.
  - `pub fn peripheral_from(kind: u32, present: bool, serial: &str, native_path: &str, model: &str, percent: f64, state: u32) -> Option<Peripheral>`.
    - This is pure and does the bounding.
    - `key` = serial if it's non-empty, else `native_path`. `None` if both are empty.
    - `name` = model, else `kind` Debug name.
    - Non-finite percent → 0, then clamp to 0..=100.
    - `charging` = state 1 or 5.
    - Strings are cut to 128 chars.
  - `pub const MAX_PERIPHERALS: usize = 32`.
  - `pub fn spawn_peripherals(conn: Connection) -> watch::Receiver<Vec<Peripheral>>`.
    - It calls `EnumerateDevices` on `/org/freedesktop/UPower`, reads each device's `Type, IsPresent, Serial, NativePath, Model, Percentage, State`, and keeps the first 32.
    - It re-reads on any signal from a `MessageStream` with rule `type='signal',sender='org.freedesktop.UPower',path_namespace='/org/freedesktop/UPower'`, and every 60 s via `tokio::time::interval` (use `select!`).
    - It sends with `send_if_modified`.
    - If the stream fails or ends: `warn!` with the error, sleep with backoff 1 s → 30 s, and rebuild the stream. Reset the backoff after a successful read.

- [ ] **Step 1: Write failing tests** in `system.rs` tests.

```rust
#[test]
fn maps_upower_peripheral_kinds() {
    assert_eq!(PeripheralKind::from_upower(2), None);
    assert_eq!(PeripheralKind::from_upower(5), Some(PeripheralKind::Mouse));
    assert_eq!(PeripheralKind::from_upower(17).map(PeripheralKind::icon), Some("headset_mic"));
    assert_eq!(PeripheralKind::from_upower(u32::MAX), Some(PeripheralKind::Other));
}

#[test]
fn bounds_hostile_peripheral_values() {
    let long = "m".repeat(1000);
    let p = peripheral_from(6, true, "", "/dev/hid", &long, f64::NAN, 1).unwrap();
    assert_eq!((p.key.as_str(), p.percent, p.charging), ("/dev/hid", 0.0, true));
    assert_eq!(p.name.chars().count(), 128);
    assert_eq!(peripheral_from(6, true, "AA", "", "K", 250.0, 2).unwrap().percent, 100.0);
    assert!(peripheral_from(6, false, "AA", "", "K", 50.0, 2).is_none());
    assert!(peripheral_from(6, true, "", "", "K", 50.0, 2).is_none());
    assert!(peripheral_from(2, true, "BAT0", "", "", 50.0, 2).is_none());
}
```

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p rune-services system::`. Expected: compile errors.
- [ ] **Step 3: Implement the types, `peripheral_from` and `spawn_peripherals`, then wire `UiEvent::Peripherals`, `Snapshot.peripherals` and `main.rs`.**
- [ ] **Step 4: Run tests to verify they pass.** Run `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Live check.** Run the daemon. With no peripherals, `busctl --system call org.freedesktop.UPower /org/freedesktop/UPower org.freedesktop.UPower EnumerateDevices` returns `ao 0` and the log shows no warnings loop. Report that real devices were not available.

---

### Task 3: Render per-item styles and the device battery widget

**Files:**

- Modify `crates/rune-ui/src/gpui_widgets.rs`: `bar_widget`, `bar_button`, the new `item_style`, and the `DeviceBattery` arm.
- Modify `crates/rune-ui/src/gpui_notch.rs`: the loop passes `&BarItem` and wraps each element.
- Modify `crates/rune-ui/src/gpui_panels.rs`: `battery()` gains a Devices section.
- Modify `crates/rune-ui/src/status.rs`: a pure helper plus tests.

**Interfaces:**

- Consumes: `BarItem`/`WidgetStyle` (Task 1), `Snapshot.peripherals` and `PeripheralKind::icon` (Task 2).
- Produces:
  - `pub(crate) fn device_for<'a>(peripherals: &'a [Peripheral], key: &str) -> Option<&'a Peripheral>` in `status.rs`. An empty key returns the first peripheral; otherwise it returns the one whose key matches exactly.
  - `pub(crate) fn item_style<E: Styled>(element: E, style: &WidgetStyle, config: &Config) -> E` in `gpui_widgets.rs`.
    - It applies the border width (rounded in the pixel look), a border colour defaulting to foreground at 0.2 opacity, and `rounded(corner(style.radius.unwrap_or(5.)))`.
    - It's a no-op for the border when width is 0.
  - `bar_widget(app, item: &BarItem, id, dispatch, images)` replaces the `widget: Widget` parameter.
    - The `glyph` closure becomes `|auto: &str| icon(c, if item.style.icon.is_empty() { auto } else { &item.style.icon }, size)` with `.text_color(color(ic))` when `icon_color` is `Some`.
    - Stats uses the colour only.
    - Media uses the override only for the fallback.
    - The clock in `gpui_notch.rs` uses `config.clock.icon` as its automatic icon.
  - `bar_button` takes the item radius, so the hover fill uses `corner(radius)`.

- [ ] **Step 1: Write failing tests** in `status.rs`.

```rust
#[test]
fn device_battery_hidden_when_absent() {
    let mouse = Peripheral { key: "AA".into(), name: "MX".into(), kind: PeripheralKind::Mouse, percent: 40.0, charging: false };
    let list = [mouse.clone()];
    assert_eq!(device_for(&list, ""), Some(&mouse));
    assert_eq!(device_for(&list, "AA"), Some(&mouse));
    assert_eq!(device_for(&list, "BB"), None);
    assert_eq!(device_for(&[], ""), None);
}
```

- [ ] **Step 2: Run it to verify it fails.** Run `cargo test -p rune-ui status::`. Expected: compile error.
- [ ] **Step 3: Implement `device_for`, `item_style`, the glyph resolution and the `DeviceBattery` arm.**
  - The arm returns `None` when the device is absent.
  - Otherwise it renders a `bar_button` of `glyph(kind.icon())` + `format!("{:.0}%", percent)`. That text is coloured `urgent` at ≤ 15, followed by `glyph("bolt")` while charging.
  - It sends `Message::Toggle(Panel::Battery)`.
  - Add `Widget::DeviceBattery => Panel::Battery` in `notch::widget_panel`.
  - In `battery()`, when `snapshot.peripherals` is non-empty, append a "Devices" `module` listing icon, name and `NN%` per peripheral (row id `device:{key}`). Show the "no battery" text only when both the battery and the peripherals are absent.
- [ ] **Step 4: Run tests to verify they pass.** Run `cargo test --workspace`. Expected: PASS.

---

### Task 4: Widget settings in Settings → Bar

**Files:**

- Modify `crates/rune-ui/src/settings.rs`: `Msg::Item`, `ItemEdit`, `update`, and `SettingsState.selected` plus a getter.
- Modify `crates/rune-ui/src/gpui_settings.rs`: chip click/selection outline and a `widget_settings` group under the lanes.

Decision: put the selection on `SettingsState` (not on the view) so it's unit-testable alongside `update`. It's still interaction state, and it isn't serialized.

**Interfaces:**

- Consumes: `BarItem`, `WidgetStyle::MAX_BORDER_WIDTH` and `BarConfig::sanitize` (Task 1); `Snapshot.peripherals` (Task 2).
- Produces:
  - `#[derive(Debug, Clone, PartialEq)] pub enum ItemEdit { Icon(String), IconColor(Option<Rgba>), BorderWidth(f32), BorderColor(Option<Rgba>), Radius(Option<f32>), Device(String) }`.
  - `Msg::Item(Section, usize, ItemEdit)`. It applies to the item, then runs `config.bar.sanitize()`. Ignored if the index is out of range.
  - `Msg::Select(Option<(Section, usize)>)`.
  - `SettingsState::selected(&self) -> Option<(Section, usize)>`. `Remove`, `Place` and `Insert` set it to `None`.
  - `pub(crate) fn device_choices(peripherals: &[Peripheral], saved: &str) -> Vec<(String, String)>` in `settings.rs`.
    - It returns `(key, label)` pairs: first `("", "First available")`, then each peripheral `(key, name)`.
    - If `saved` is non-empty and absent, it appends `(saved, "<saved> (not connected)")`.
  - `pub(crate) enum IconSupport { Full, ColorOnly, None }` with `fn icon_support(widget: Widget) -> IconSupport`. Workspaces, WindowTitle and Tray → None; Stats → ColorOnly; the rest → Full.

- [ ] **Step 1: Write failing tests** in `settings.rs` tests.

```rust
#[test]
fn item_edits_apply_and_bound() {
    let (mut state, mut config) = (State::default(), Config::default());
    state.update(&mut config, Msg::Item(Section::Right, 0, ItemEdit::BorderWidth(9.)));
    assert_eq!(config.bar.right[0].style.border_width, 4.);
    state.update(&mut config, Msg::Item(Section::Right, 0, ItemEdit::IconColor(Some(Rgba([1, 2, 3, 4])))));
    assert_eq!(config.bar.right[0].style.icon_color, Some(Rgba([1, 2, 3, 4])));
    let before = config.clone();
    state.update(&mut config, Msg::Item(Section::Right, 999, ItemEdit::Icon("x".into())));
    assert_eq!(config, before);
}

#[test]
fn place_keeps_item_style() {
    let (mut state, mut config) = (State::default(), Config::default());
    state.update(&mut config, Msg::Item(Section::Right, 0, ItemEdit::Icon("star".into())));
    let moved = config.bar.right[0].clone();
    state.update(&mut config, Msg::Place((Section::Right, 0), (Section::Left, 0)));
    assert_eq!(config.bar.left[0], moved);
}

#[test]
fn selection_clears_on_layout_change() {
    let (mut state, mut config) = (State::default(), Config::default());
    for msg in [
        Msg::Remove(Section::Right, 0),
        Msg::Place((Section::Right, 0), (Section::Left, 0)),
        Msg::Insert(Section::Left, 0, BarItem::from(Widget::Clock)),
    ] {
        state.update(&mut config, Msg::Select(Some((Section::Right, 1))));
        state.update(&mut config, msg);
        assert_eq!(state.selected(), None);
    }
}

#[test]
fn device_choices_include_absent_saved_key() {
    let choices = device_choices(&[], "F4:73");
    assert_eq!(choices[0], (String::new(), "First available".into()));
    assert_eq!(choices.last().unwrap(), &("F4:73".into(), "F4:73 (not connected)".into()));
}
```

(Use the existing test constructor for `State` if `State::default()` isn't how the existing tests build it; check near line 1750.)

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p rune-ui settings::`. Expected: compile errors.
- [ ] **Step 3: Implement the messages and helpers, then the UI.**
  - Chip click → `Msg::Select`. The selected chip gets an accent border.
  - The `widget_settings` group renders only when `selected` resolves to an item. Its contents are gated by `icon_support`:
    - Icon: a `TextInput` keyed `item-icon:{section:?}:{index}` whose placeholder is the automatic name; it dispatches `ItemEdit::Icon`.
    - Icon colour and Border colour: swatch + hex text + an "Automatic" button dispatching `None`.
    - Border width: `number!`-style slider, 0..=4, step 0.5.
    - Radius: slider 0..=height/2 plus "Automatic". In the pixel look it shows the existing square-corner note.
    - Device (DeviceBattery only): the existing `segmented`/dropdown pattern over `device_choices`.
  - Footer note: "A custom icon replaces status icons such as muted volume. Leave it blank for automatic icons."
- [ ] **Step 4: Run tests to verify they pass.** Run `cargo test --workspace`. Expected: PASS.

---

### Task 5: `ColorPicker` control

**Files:**

- Create `crates/rune-ui/src/controls/color_picker.rs`.
- Modify `crates/rune-ui/src/controls/mod.rs` to export it.

**Interfaces:**

- Produces:
  - `pub fn rgba_to_hsva(c: Rgba) -> [f32; 4]` and `pub fn hsva_to_rgba(h: f32, s: f32, v: f32, a: f32) -> Rgba`.
    - h is in 0..360, wrapping (`rem_euclid`); s, v and a are in 0..=1, clamped.
    - Use `f32::round` with `clamp` before casting to `u8` (`as` on a clamped finite value).
  - `pub struct ColorPicker`, built with `ColorPicker::new(initial: Rgba, cx: &mut Context<Self>) -> Self`. It owns the hsva state and a hex `Entity<TextInput>`.
  - `impl EventEmitter<ColorPickerEvent> for ColorPicker`, with `pub enum ColorPickerEvent { Change(Rgba), Close }`.
  - `pub fn sync(&mut self, value: Rgba, cx: &mut Context<Self>)`. It updates only when the value differs from its current rgba, so hue isn't lost on greys.
  - `pub fn focus_handle(&self) -> FocusHandle`.
  - Render, about 240 px wide:
    - SV square (160 px tall): hue fill + white→transparent left-to-right gradient + transparent→black top-to-bottom.
    - Hue strip: 6 two-stop segments.
    - Alpha strip: checkerboard (6 px cells, 2 rows) under a transparent→opaque gradient of the current colour.
    - Row: old and new previews on a checkerboard + hex input.
  - Pointer handling: `on_mouse_down` and `on_mouse_move` (while the button is held) compute the fraction from the element bounds, captured with `canvas`/`on_children_prepainted`, or the pattern `controls/slider.rs` uses. Emit `Change` on every move.
  - Keys:
    - Escape → `Close`.
    - Arrow keys nudge the last-touched region by 1 % (shift = 10 %).
  - Looks:
    - smooth: `corner(12.)` container, circular 12 px handles with a white 2 px ring;
    - pixel: `frame()` container, square handles, whole-pixel borders.
  - Hex input: parse with `Rgba::parse`. Emit only on success; on failure keep the state.

- [ ] **Step 1: Write failing tests** in `color_picker.rs`.

```rust
#[test]
fn hsva_round_trips() {
    for c in [Rgba([0x89, 0xb4, 0xfa, 0xff]), Rgba([0, 0, 0, 0]), Rgba([255, 255, 255, 128]), Rgba([0x80, 0x80, 0x80, 0x33])] {
        let [h, s, v, a] = rgba_to_hsva(c);
        assert_eq!(hsva_to_rgba(h, s, v, a), c);
    }
    assert_eq!(hsva_to_rgba(360., 1., 1., 1.), hsva_to_rgba(0., 1., 1., 1.));
    assert_eq!(hsva_to_rgba(-120., 1., 1., 1.), Rgba([0, 0, 255, 255]));
    assert_eq!(hsva_to_rgba(f32::NAN, 2., -1., 9.), Rgba([0, 0, 0, 255]));
}

#[test]
fn partial_hex_keeps_color() {
    assert_eq!(hex_edit(Rgba([1, 2, 3, 4]), "#12"), None);
    assert_eq!(hex_edit(Rgba([1, 2, 3, 4]), "#ff000080"), Some(Rgba([255, 0, 0, 128])));
}
```

`fn hex_edit(current: Rgba, text: &str) -> Option<Rgba>` is a pure helper the hex subscription uses. It returns `None` for invalid input or when the value is unchanged. NaN maps to 0 before clamping.

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p rune-ui color_picker`. Expected: compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run tests to verify they pass.** Run `cargo test --workspace`. Expected: PASS.

---

### Task 6: Host the picker from every swatch

**Files:**

- Modify `crates/rune-ui/src/gpui_settings.rs`:
  - `swatch` becomes clickable and draws a checkerboard under the colour;
  - the Appearance colours (~3665), the bar border and shadow (~3800), and Task 4's item colours.

**Interfaces:**

- Consumes: `ColorPicker`/`ColorPickerEvent` (Task 5), `Msg::Item`/`ItemEdit` (Task 4), and the existing `Msg::Text(Field, String)` with `Field::{Background, Foreground, Accent, OnAccent, Muted, Urgent, BorderColor, ShadowColor}`.
- Produces:
  - `enum ColorTarget { Field(Field), Item(Section, usize, ItemColor) }` with `enum ItemColor { Icon, Border }`.
  - `SettingsView.picker: Option<(ColorTarget, gpui::Point<Pixels>, Entity<ColorPicker>)>`.
  - `fn color_swatch(&self, id: String, value: Rgba, target: ColorTarget, cx) -> AnyElement` replaces direct `swatch(..)` calls:
    - The id is `swatch:{target:?}`.
    - On click it records `event.position()` and creates a picker. The picker subscription maps `Change(c)`:
      - `Field(f)` → `Msg::Text(f, String::from(c))`;
      - `Item(s, i, Icon)` → `Msg::Item(s, i, ItemEdit::IconColor(Some(c)))`;
      - `Item(s, i, Border)` → `Msg::Item(s, i, ItemEdit::BorderColor(Some(c)))`.
    - `Close` → `picker = None`.
    - Focus the picker in the click handler.
  - Render the open picker with `deferred(anchored().position(point).snap_to_window_with_margin(px(8.)).child(entity))`, plus `on_mouse_down_out` → close.
  - Each render calls `entity.update(|p| p.sync(current_value))`, which is cheap when unchanged.

- [ ] **Step 1: Write a failing test** that pins the target → message mapping as a pure fn.

```rust
#[test]
fn color_targets_map_to_messages() {
    let c = Rgba([1, 2, 3, 4]);
    assert!(matches!(color_message(ColorTarget::Field(Field::Accent), c), Msg::Text(Field::Accent, ref s) if s == "#01020304"));
    assert!(matches!(color_message(ColorTarget::Item(Section::Left, 2, ItemColor::Border), c),
        Msg::Item(Section::Left, 2, ItemEdit::BorderColor(Some(x))) if x == c));
}
```

- [ ] **Step 2: Run it to verify it fails.** Run `cargo test -p rune-ui color_targets`. Expected: compile error.
- [ ] **Step 3: Implement `fn color_message(target: ColorTarget, value: Rgba) -> Msg`, `color_swatch` and the hosting.** Then check whether dragging stutters. Settings already saves per slider change; watch the frame pacing while dragging the SV square. Only if it stutters, coalesce by dispatching on mouse-up plus a live preview through `Msg::Text` drafts.
- [ ] **Step 4: Run tests to verify they pass.** Run `cargo test --workspace`. Expected: PASS.

---

### Task 7: Docs, full verification and visual checks

**Files:**

- Modify `docs/ARCHITECTURE.md`: peripherals feed (UPower, signals plus 60 s verify, backoff), `BarItem`/`WidgetStyle`, colour picker hosting.
- Modify `examples/config.toml`: comment showing the table form and `device_battery`. It must stay equal to the defaults.

- [ ] **Step 1: Run `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo doc --workspace --no-deps`.** Expected: every command exits 0 with no warnings.
- [ ] **Step 2: Visual check in both looks** with `bar.look = "smooth"` and then `"pixel"`.
  - Style a clock with a border, radius and icon colour.
  - Add two `device_battery` items.
  - Open the picker from an Appearance swatch and from an item swatch.
  - Drag in the square and the strips, type partial hex, press Esc, click outside.
  - Confirm: pixel corners are square and handles square, no layout oscillation, and both looks are inspected.
- [ ] **Step 3: Report.** List what was verified. State that no UPower peripherals were present, so device battery rendering was checked by unit tests only.
