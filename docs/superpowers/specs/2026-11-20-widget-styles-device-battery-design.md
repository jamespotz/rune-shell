# Per-widget styles, device battery widget, colour picker

## Intent

The bar is currently styled as a whole. The user wants three things:

- each placed widget can be styled on its own: border, corner radius, icon colour and a custom icon;
- a battery readout for wireless peripherals (mouse, keyboard, headset, ...), which can be placed once per device;
- a real colour picker with alpha. It opens from every colour preview in Settings, so nobody has to type hex codes.

Everything ships in both looks, `smooth` and `pixel`.

## Goals

- `bar.left/center/right` entries carry per-placement settings, and existing configs load unchanged.
- Style settings for every widget: border width and colour, corner radius, icon colour, and an icon override.
  The icon controls apply only where the widget draws icons.
- A new `device_battery` widget. It picks the device from UPower, chooses its icon from the device type (the user can override it),
  and shows the percentage. It can be added any number of times.
- A colour picker: a saturation/brightness square, a hue strip, an alpha strip over a checkerboard, a hex field, and old/new previews.
  It opens from every colour swatch in Settings.

## Non-goals

- Batteries from `headsetcontrol` or vendor tools. Only UPower is supported: Bluetooth (via BlueZ) and Logitech HID++ receivers.
- Per-widget fonts, padding or background fills.
- A shared global "widget style" preset.

## Anti-goals

- Configs that worked before must not stop parsing.
- Dragging in the picker must not cause per-frame config writes that stall the UI, or Wayland surface churn.
- A disconnected device must not leave an empty, clickable gap in the bar.

## Approaches (user picked A, A, A)

1. **Storage.** (A, picked) settings live on each list entry; a plain name is still valid.
   (B) a `[widgets.<name>]` table per type plus a numbered device list. Rejected: it can't style two copies independently,
   and indexes break when widgets are reordered.
2. **Editing.** (A, picked) clicking a chip in Settings → Bar selects it, and its settings show below the lanes.
   (B) a separate pop-up per widget. Rejected: extra window plumbing for no gain.
3. **Picker.** (A, picked) a floating HSV picker with alpha, anchored to the swatch.
   (B) four RGBA sliders. Rejected: poor for choosing colours by eye.

## Design

### Config (`crates/rune-ui/src/config.rs`)

- New `Widget::DeviceBattery`: serde name `device_battery`, display name "Device battery", glyph `battery_full` on the shelf.
  Add it to `Widget::ALL`.
- `BarItem { widget: Widget, style: WidgetStyle, device: String }` replaces `Widget` in `BarConfig::{left, center, right}`.
  - It deserializes from either `"clock"` or a table `{ widget = "clock", icon = "...", icon_color = "#rrggbbaa", border_width = 1.0, border_color = "...", radius = 4.0, device = "..." }`.
    Use an untagged repr; the table variant has `deny_unknown_fields`.
  - It serializes back to the plain name when the style is default and `device` is empty.
    This keeps `examples/config.toml` matching the defaults and keeps saved files tidy.
- `WidgetStyle` fields:
  - `icon: String`. Empty means automatic.
  - `icon_color: Option<Rgba>`. `None` means the colour each widget uses today.
  - `border_width: f32`. 0 means no border; clamped to 0..=4.
  - `border_color: Option<Rgba>`. `None` means foreground at 20 % opacity.
  - `radius: Option<f32>`. `None` keeps today's 5 px; clamped to 0..=height/2.
- `device` is used only by `DeviceBattery`. It holds the UPower `Serial` (the MAC address for Bluetooth), or `NativePath` when the serial is empty.
  Empty means "first available peripheral".
- Hostile input is bounded in `BarConfig::sanitize`:
  - non-finite numbers become 0, then everything is clamped;
  - `icon` and `device` are cut to 64 and 128 chars;
  - each section is truncated to 64 items.
- Widget lookups (`bar_has`, the settings `contains`, `section_mut`, the runtime defaults, tests) move to `BarItem`.
  `impl From<Widget> for BarItem` keeps call sites short.
- `clock.icon` stays as the clock's automatic icon. A per-item `icon` overrides it.

### Rendering (`gpui_notch.rs`, `gpui_widgets.rs`)

- The notch loop passes `&BarItem` to `bar_widget` and the inline widgets.
- The `glyph` closure in `bar_widget` resolves `style.icon` (non-empty), falling back to the automatic name. It applies `style.icon_color` as `text_color`.
  Icon overrides are static, so a custom icon replaces state icons such as muted volume. The settings note says so.
- Widgets without a single glyph:
  - Workspaces, Window title and Tray hide both icon controls in Settings.
  - Stats hides the icon override but takes the icon colour on all three glyphs.
  - Media uses the override only for its no-art fallback.
- A single helper, `gpui_widgets::item_style(element, &style, &config)`, applies the border and radius around each placed widget.
  - Radius goes through `corner()`, so the pixel look stays square.
  - In the pixel look the border width is rounded to whole pixels, as `bar_outline` does.
  - `bar_button` takes the resolved radius so its hover fill matches the border.

### Device battery service (`crates/rune-services/src/system.rs`)

- `spawn_peripherals(conn) -> watch::Receiver<Vec<Peripheral>>`, wired in `main.rs` next to `spawn_battery`, forwarded as `UiEvent::Peripherals` into `snapshot.peripherals`.
- `Peripheral { key, name, kind: PeripheralKind, percent: f64, charging: bool }`.
  - `PeripheralKind` maps UPower `Type`: Mouse 5, Keyboard 6, Phone 8, Tablet 10, GamingInput 12, Pen 13, Touchpad 14, Headset 17, Speakers 18, Headphones 19, Other.
  - Types 1–3 (line power, laptop battery, UPS) are skipped, as are devices with `IsPresent = false`.
- Refresh triggers: `EnumerateDevices` re-reads on `DeviceAdded`/`DeviceRemoved` and on `PropertiesChanged` under `path_namespace='/org/freedesktop/UPower/devices'`.
  A 60 s verification re-read catches missed signals.
  - If the stream ends, log `warn!` and reconnect with bounded backoff (1 s doubling to 30 s).
  - Results are capped at 32 devices, with strings cut to 128 chars and percentages clamped to 0..=100.
- Automatic icons (pure function, tested):

  | Device type | Icon |
  |---|---|
  | Mouse | `mouse` |
  | Keyboard | `keyboard` |
  | Headset | `headset_mic` |
  | Headphones | `headphones` |
  | GamingInput | `sports_esports` |
  | Phone | `smartphone` |
  | Tablet | `tablet` |
  | Pen | `stylus` |
  | Touchpad | `touchpad_mouse` |
  | Speakers | `speaker` |
  | Other | `battery_full` |

### Device battery widget

- Shows the icon plus `NN%`. The percentage turns `urgent` at ≤ 15 % and is followed by a small `bolt` glyph while charging.
- Hidden (returns `None`) when its device is absent.
- Click toggles `Panel::Battery`. That panel gains a "Devices" section listing every peripheral (icon, name, percentage),
  and it no longer says "no battery" when peripherals exist. *(Builder's call; the user didn't specify the click.)*

### Settings → Bar (`gpui_settings.rs`, `settings.rs`)

- `SettingsView` gets `selected: Option<(Section, usize)>`, which is view interaction state. Clicking a chip selects it and outlines it in the accent colour.
  Place, Insert and Remove clear the selection, so it can't point at the wrong item.
- When an item is selected, a "Widget settings" group appears under the lanes. It holds:
  - Icon: text field, placeholder shows the automatic name, empty resets.
  - Icon colour: swatch, hex field and "Automatic" reset.
  - Border width: number from 0 to 4.
  - Border colour: swatch, hex field and "Automatic" reset.
  - Radius: number with "Automatic" reset. In the pixel look it shows the existing note that corners stay square.
  - Device (device battery only): a list of current peripherals plus "First available". A saved device that's absent is shown as "<key> (not connected)".
- A new message `Msg::Item(Section, usize, ItemEdit)` is applied in `SettingsState::update`.
  `ItemEdit` has the variants `Icon(String)`, `IconColor(Option<Rgba>)`, `BorderWidth(f32)`, `BorderColor(Option<Rgba>)`, `Radius(Option<f32>)` and `Device(String)`.
  Out-of-range indexes are ignored, and values are sanitized like config input.

### Colour picker (`crates/rune-ui/src/controls/color_picker.rs`)

- `ColorPicker` entity: it holds HSV plus alpha as `f32` (so hue survives grey and black), emits `ColorPickerEvent::Change(Rgba)`, and has `sync(Rgba)` for outside edits.
  Pure conversions `rgba_to_hsva` and `hsva_to_rgba` are unit tested: round trip, greys, and hue wrap.
- Layout:
  - SV square: hue fill + white→transparent horizontal gradient + transparent→black vertical gradient.
  - Hue strip: six two-stop gradient segments, because GPUI gradients take two stops.
  - Alpha strip: a checkerboard of a few dozen cells under a transparent→colour gradient.
  - Hex field: the existing `TextInput`, accepting `#rrggbb`/`#rrggbbaa`.
  - Previews: old and new side by side.
- Behaviour:
  - Pointer down + drag updates continuously, with immediate feedback. Escape and clicking outside close the picker.
  - Arrow keys nudge the focused strip or square. The hex field is the keyboard path.
- Looks: smooth uses rounded corners and circular handles. Pixel uses `frame()`, square handles, whole-pixel strokes, and the configured pixel font.
- Hosting: `SettingsView` holds `picker: Option<(ColorTarget, Entity<ColorPicker>)>` and renders it with `deferred(anchored())` at the clicked swatch.
  - `ColorTarget` is `Field(Field)`, covering the six theme colours, the bar border and the bar shadow, or `Item(Section, usize, ItemColor)`.
  - Changes dispatch the existing `Msg::Text(field, hex)` or `Msg::Item(..)`. They go through the same save path the sliders already use. If saving per change shows up as stutter, the builder coalesces writes to the end of the drag.
- Every existing `swatch(..)` becomes clickable, with a stable ID per target. Swatches draw over a checkerboard, so alpha is visible.

## Constraints (from AGENTS.md)

- No panics on config, D-Bus or input values. D-Bus runs on Tokio, and the UI gets updates through `watch`.
- `render()` stays declarative: opening the picker and focusing happen in click handlers.
- Interactive elements get stable IDs.
- Both looks get inspected in the running UI. Reduced motion is respected (the picker has no animation beyond direct manipulation).
- Preserve the uncommitted polkit work in the tree.
- Update `docs/ARCHITECTURE.md` (peripherals feed, `BarItem`) and the `examples/config.toml` comments.
- Finish with `fmt`, `clippy -D warnings`, `test` and `doc`, all clean.

## Tests

- Config:
  - plain names and tables both parse;
  - default items serialize as names;
  - a styled item round-trips;
  - unknown table keys are rejected;
  - nan, inf, huge sizes, long strings and more than 64 items are bounded;
  - the example config still equals the defaults.
- UPower: type → kind mapping, kind → icon, property parsing with hostile values (NaN or out-of-range percentage, oversized strings, too many devices).
- Settings: `Msg::Item` edits, bad indexes are ignored, and the selection clears on Place/Insert/Remove.
- Picker: HSV ↔ RGBA conversions; a hex parse feeds `sync`.
- Not testable here: real peripherals. This machine has no UPower peripherals, so live device checks are reported as unverified.

## Left to the builder

- Exact sizes and spacing of the picker and the widget settings rows.
- Whether picker saves need coalescing (measure first).
