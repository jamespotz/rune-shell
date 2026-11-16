# macOS menu bar, dropdowns, and System Settings

## Intent

The two earlier passes (`2026-11-17-apple-design`, `2026-11-17-macos-styling`) kept Rune looking like a themed Linux bar rather than macOS. The user reviewed browser mockups (`.superpowers/brainstorm/*/content/macos-v1.html`) and chose bar **A — Sonoma frosted strip**, and approved the dropdown and Settings looks shown there. This pass makes the running shell match those mockups while keeping every feature, the user's Catppuccin colours, and the service architecture.

## Goals

- The bar, every widget dropdown, the launcher, notifications, the OSD, and Settings visibly match the mockup.
- Translucent frosted surfaces when Hyprland blur is on; legible near-opaque surfaces when it is off.
- macOS interaction feel: soft rounded hover highlight, press feedback on pointer-down, the bar item stays highlighted while its dropdown is open.

## Non-goals and anti-goals

- No Dock, application menus, or Apple logo menu.
- No new rendering dependencies, custom fonts, or icon pipeline. SF Pro is not available; use the configured text font.
- Not a failure mode to accept: hardcoded macOS colours replacing the user's palette (except the fixed per-page Settings icon squares), unreadable text with blur off, layout oscillation or window recreation at fractional scale.

## Approaches

1. **Restyle existing GPUI modules in place (chosen).** Shared surface/row/switch/slider helpers in `gpui_widgets.rs`; bar in `gpui_notch.rs`; dropdowns in `gpui_panels.rs`; Settings in `gpui_settings.rs`. Keeps behaviour, tests, and measurement code.
2. Rewrite the UI layer around a new component set. Throws away working behaviour for no visual gain.
3. Change only defaults/config. The previous passes showed this is not enough.

## User decisions

- Bar style A: single full-width frosted strip, monochrome glyphs, text for weather and clock.
- Keep the user's Catppuccin Mocha colours; accent (mauve) replaces the mockup's blue everywhere it signals "on" or "selected".
- Change the user's config text font from JetBrains Mono to **Inter** and enable **blur**. Defaults in `config.rs`/`examples/config.toml` keep Noto Sans (Inter may not be installed elsewhere).
- Dropdowns and Settings as in the mockup, for every widget.

## Design

### Materials

- One shared material helper produces a frosted surface: `colors.background` at reduced alpha when `bar.blur` is on (bar ≈ 0.70, dropdowns ≈ 0.78, Settings sidebar ≈ 0.80, Settings content ≈ 0.92), and ≈ 0.96 when blur is off. A 0.5px inner light edge (`foreground` ≈ 0.18) is painted, not laid out, preserving measured sizes.
- Extend the Hyprland `rune-blur` layer rule to cover `rune-panel`, `rune-notifications`, and `rune-osd` in addition to bar and settings.
- Radii: dropdowns 14, launcher 22, Settings window 12, Settings groups 10, rows 7, bar items 5.

### Bar (`gpui_notch.rs`, `bar_widget`)

- Full-width strip, configured height, no bottom border.
- Every item is a rounded-5 hover target: `foreground` 0.18 on hover, 0.26 on press, and the same 0.18 fill while its dropdown is open (map widget → panel/settings page from current open state).
- Workspaces: plain numbers in `muted`; occupied in `foreground`; active gets a rounded-5 `foreground` 0.22 fill (the spring animates the fill). Urgent uses `urgent` text. Every slot remains clickable.
- Window title: semibold, truncated as configured.
- Media: small art (or `music_note`) plus "title — artist" text, truncated. The visualizer moves to the media dropdown only.
- Status glyphs (bluetooth, network, volume, battery, notifications, quick settings): icon only. Weather: icon + temperature. Clock: configured format and icon.
- Scroll-to-change-volume, tray clicks, recording and game-mode indicators keep working.

### Dropdowns (`gpui_panels.rs`)

Shared building blocks: title row with optional trailing switch, grey section label, hover row (icon circle on the left, accent-filled when the item is on, trailing secondary text), separator, and a final "… Settings…" row that opens the matching Settings page.

- **Quick settings (Control Center):** connectivity module (Wi-Fi, Bluetooth, plus any existing toggles) with accent circles and subtitles; small modules for the remaining existing toggles; Display and Sound slider pills (white fill, glyph inside); media module with art, title, play/pause, next. Only controls that exist today; nothing new.
- **Wi-Fi (new dropdown, was: opens Settings):** title + power switch, "Known Network" (current) and "Other Networks" lists, Wi-Fi Settings… row. Reuses the Wi-Fi data and actions behind the Settings page.
- **Bluetooth (new dropdown, was: opens Settings):** title + power switch, Devices list with connect on click and battery where known, Bluetooth Settings… row. Reuses Settings' data and actions; scanning stays on the Settings page only.
- **Sound (new dropdown, was: click mutes):** volume slider pill, Output device list, Sound Settings… row. Middle-click on the bar item mutes; scroll still changes volume.
- **Battery (new dropdown):** percentage, source/time remaining when known, Battery/Power… row to the existing Power panel. No new power-profile service.
- **Weather:** location, large light-weight temperature, condition and high/low, hourly strip in an inner module.
- **Calendar (clock):** month header with chevrons, weekday initials, today in a filled `urgent` circle, days outside the month muted, events with a coloured leading bar.
- **Notifications:** "Notifications / Clear" header over stacked frosted cards (app icon, bold app name, relative time, body). Popups use the same card.
- **Media:** art, title, artist · album, player, progress bar with elapsed/remaining, transport controls, visualizer.
- **Tray menus, Power:** plain menu rows with separators.
- **Launcher / clipboard (Spotlight):** large light search field, separator, results with accent-filled selected row and secondary kind label.
- **OSD:** frosted rounded pill containing the slider pill.

### Settings (`gpui_settings.rs`)

- Frosted sidebar: red close dot (closes Settings), a search field that filters the page list by label, and pages in groups with coloured 22px rounded-square icons (fixed macOS palette per page). Groups: Wi-Fi, Bluetooth, Sound · Appearance, Theme, Wallpaper, Menu Bar, Animations · Launcher, Calendar, Weather, Capture. Selected row filled with accent.
- Content: page title in a toolbar row; centred column (max ≈ 620px) of grouped rounded lists with grey group headers. Rows: label left, control right (switch, pop-up value, slider, text field), hairline separators between rows.
- Same controls, validation, keyboard navigation, focus, Escape, and every existing setting.

### Motion

Keep the existing critically damped spring and reduced-motion handling. Highlights appear on pointer-down. No new animations.

## Constraints

From `AGENTS.md`: no panics in production paths, no comments, no render-time side effects, stable element IDs, preserve measurement/viewport guards and the tiny-window sizing workaround, Wi-Fi/Bluetooth/audio actions through the existing action channel, `cargo fmt`, Clippy with `-D warnings`, tests.

## Left to the builder

Exact alpha values within the stated ranges, spacing tokens, the per-page icon colours, which existing Settings messages back each dropdown, and how the Settings search field reuses `TextInput`.

## Acceptance

- Running shell on DP-1 at 1.25 scale visibly matches the mockup for the bar, each dropdown, launcher, notifications, OSD, and Settings, with blur on and off.
- New Wi-Fi, Bluetooth, Sound, Battery dropdowns open under their bar items and perform their actions.
- Existing native tests pass individually; workspace tests, Clippy, fmt, release build pass. Report what could not be checked visually.
