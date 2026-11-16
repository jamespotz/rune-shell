# macOS Menu Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the bar, every dropdown, launcher, notifications, OSD, and Settings match the approved Sonoma-style mockup.

**Architecture:** Shared material and row primitives live in `gpui_widgets.rs`; every view consumes them. Four new `Panel` variants reuse existing Settings data and actions through the existing message/action channel.

**Tech Stack:** Rust, vendored GPUI (Wayland layer shell), Hyprland Lua layer rules.

**Spec:** `docs/superpowers/specs/2026-11-17-macos-menu-bar-design.md`

## Global Constraints

- No `unwrap`/`expect`/panics in production paths, no code comments, no render-time side effects (focus, notify, surface setters).
- Stable unique element IDs, domain keys in loops.
- Preserve `measure_panel`, `natural_content`, positive viewport sizes; painted edges must not consume layout space.
- User colours win; only the Settings page icon squares use a fixed palette.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; native tests individually with `--ignored`.

## Review Focus

- Blur off: every surface must stay ≥ 0.96 opaque and readable. Test in Task 1.
- Network/Bluetooth unavailable (`None` / `!available`): new dropdowns render an "Unavailable" row, no panic. Native test in Task 3 with empty snapshot.
- Secured unsaved Wi-Fi picked from dropdown: hands off to Settings Wi-Fi page with the password prompt. Unit test in Task 3.
- Settings search matching nothing: shows "No Results", Escape still closes. Unit test in Task 5.
- Long media title / window title: truncated, bar height unchanged. Unit test in Task 2.

---

### Task 1: Materials, primitives, blur coverage, user config

**Files:**
- Modify: `crates/rune-ui/src/gpui_widgets.rs`, `crates/rune-shell/src/main.rs` (`blur_rule`), `~/.config/rune-shell/config.toml` (font `Inter`, `blur = true`)

**Interfaces — Produces:**
- `pub(crate) enum Material { Bar, Popover, Sidebar, Content, Card }`
- `pub(crate) fn material_alpha(material: Material, blur: bool) -> f32` — blur on: Bar 0.70, Popover 0.78, Sidebar 0.80, Content 0.92, Card 0.70; blur off: all 0.96.
- `pub(crate) fn surface(config: &Config, material: Material, radius: f32) -> Div` (replaces the current one-argument `surface`; painted 0.5px `foreground`@0.18 edge).
- `pub(crate) fn menu_row(id, config, leading: Option<(&str, bool)>, label, trailing: Option<String>) -> Stateful<Div>` — 7px radius, hover `foreground`@0.10, press @0.16; leading glyph in 26px circle, accent fill + `on_accent` glyph when on.
- `section_label(config, &str) -> Div` (11px semibold `muted`), `separator(config) -> Div` (1px `foreground`@0.12), `switch(id, config, on, Message, &Dispatch) -> Stateful<Div>` (32×19 track, accent when on), `pill_slider(slider: Entity<Slider>, glyph) -> Div`.

- [ ] Test `material_alpha_blur_off_is_opaque_enough`: for every material, `material_alpha(m, false) >= 0.96`; `material_alpha(Material::Bar, true) == 0.70`.
- [ ] Test in `main.rs`: `blur_rule(true)` contains `rune-panel`, `rune-notifications`, `rune-osd`, `rune-bar`, `rune-settings`.
- [ ] Run tests, expect FAIL; implement; run, expect PASS.
- [ ] Update every `surface(` caller to the new signature (Popover/14 for panels, Card/14 for notifications, Popover/22 launcher, Popover/18 OSD).
- [ ] Back up then edit user config: `font = "Inter"`, `blur = true`.
- [ ] fmt, clippy, commit `feat(ui): shared macOS materials and menu primitives`.

### Task 2: Menu bar

**Files:** Modify `crates/rune-ui/src/gpui_notch.rs`, `crates/rune-ui/src/gpui_widgets.rs` (`bar_widget`), `crates/rune-ui/src/notch.rs`

**Interfaces — Produces:**
- `pub(crate) fn widget_panel(widget: Widget) -> Option<Panel>` in `notch.rs`: Volume→Sound, Network→Wifi, Bluetooth→Bluetooth, Battery→Battery, Weather→Weather, Clock→Calendar, Notifications→Notifications, QuickSettings→Quick, Media→Media, Power→Power, else None.
- `pub(crate) fn media_label(media: &Media, max_chars: usize) -> String` in `notch.rs`: `"title — artist"`, falls back to identity, truncated via existing `truncate`.

- [ ] Tests: `widget_panel(Widget::Volume) == Some(Panel::Sound)`, `widget_panel(Widget::Workspaces) == None`; `media_label` with 200-char title returns ≤ max_chars+1 chars; empty artist yields title only.
- [ ] FAIL → implement → PASS.
- [ ] Bar background `surface(.., Material::Bar, 0.)` with no edge; items rounded 5, hover `foreground`@0.18, press @0.26, open fill @0.18 when `app.panels.current() == widget_panel(w)`.
- [ ] Workspaces: `muted`/`foreground` numbers, active fill `foreground`@0.22 animated by existing spring `t`, urgent → `urgent` text; slots stay clickable.
- [ ] Status widgets icon-only; Weather icon+temp; Media art/`music_note` + `media_label` (visualizer removed from bar); window title semibold.
- [ ] Clicks: Network→`Toggle(Panel::Wifi)`, Bluetooth→`Toggle(Panel::Bluetooth)`, Volume→`Toggle(Panel::Sound)` with middle-click `VolumeMute` and existing scroll, Battery→`Toggle(Panel::Battery)` (these variants come from Task 3; do Task 3's enum step first if executing out of order).
- [ ] Run `cargo test -p rune-ui all_widgets_and_repeated_widgets_render -- --ignored`; PASS. Commit `feat(ui): Sonoma-style menu bar`.

### Task 3: New Wi-Fi, Bluetooth, Sound, Battery dropdowns

**Files:** Modify `crates/rune-ui/src/panel.rs`, `crates/rune-ui/src/lib.rs`, `crates/rune-ui/src/gpui_panels.rs`

**Interfaces — Produces:**
- `Panel::{Wifi, Bluetooth, Sound, Battery}`; CLI names `wifi`, `bluetooth`, `sound`, `battery` in `NAMED`.
- `Message::WifiDropdownPick(usize)`: forwards `settings::Msg::WifiPick`; if `settings_ui.wifi_pick` becomes `Some`, closes the panel and opens Settings on `Page::Wifi`.
- `Message::OpenSettingsPage(settings::Page)`: closes panel, calls `open_settings_page`.
- Opening `Panel::Wifi` sends `system::Command::WifiScan`.

- [ ] Tests (lib.rs test module, reuse the shell constructor pattern from `gpui_panels::tests::populated_shell`): toggling `Panel::Wifi` emits `Action::System(WifiScan)` on the action receiver; `WifiDropdownPick` on a secured unsaved network sets `settings_ui.wifi_pick` and opens Settings on `Page::Wifi`; `Panel::from_name("sound") == Some(Panel::Sound)`.
- [ ] FAIL → implement → PASS.
- [ ] Render (width 280, Battery 260): Wi-Fi — title + `switch` (`WifiToggle`), "Known Network" active row, "Other Networks" from `settings_ui.wifi` (signal glyph by strength, lock trailing), "Wi-Fi Settings…" → `OpenSettingsPage(Wifi)`; "Unavailable" when `snapshot.network` is None. Bluetooth — title + switch (`BtPower`), paired devices from `snapshot.bt.devices` (on = connected, trailing battery %), click sends existing connect/disconnect command, "Bluetooth Settings…". Sound — `pill_slider(volume)`, "Output" from `snapshot.audio.outputs` (click `SetDefault`), "Sound Settings…". Battery — title + percent, charging/time-left line, separator, "Power…" → `Toggle(Panel::Power)`.
- [ ] Add the four variants to `PanelView` width match and `lib.rs` open hook (line ~937 match).
- [ ] Native test `new_dropdowns_render_with_and_without_services`: open each of the four with `populated_shell()` and with `Shell::new(Config::default(), ..)`; run individually with `--ignored`; PASS. Commit `feat(ui): Wi-Fi, Bluetooth, Sound and Battery dropdowns`.

### Task 4: Restyle existing dropdowns, launcher, notifications, OSD

**Files:** Modify `crates/rune-ui/src/gpui_panels.rs`, `crates/rune-ui/src/gpui_launcher.rs`, `crates/rune-ui/src/gpui_overlays.rs`

- [ ] Quick (width 330): two-column grid of inner modules (`foreground`@0.09, radius 12): connectivity module (Wi-Fi, Bluetooth with subtitles), remaining existing toggles as compact modules, Display and Sound `pill_slider`s, media module (art 38px, title, artist, play/pause, next), action row unchanged in function.
- [ ] Weather: location semibold, 40px light temperature, condition + H/L right, hourly strip in inner module; daily rows kept.
- [ ] Calendar: header month + chevron buttons, weekday initials S M T W T F S row starting Monday as today, today filled `urgent` circle, selected day accent ring, outside-month muted, events with 3px coloured leading bar.
- [ ] Notifications: header "Notifications" + "Clear" text button + DND switch; each item `surface(Card,14)`; popups same card.
- [ ] Media: art 56 rounded 8, title/artist·album, player line, seek, elapsed/`-remaining`, transport row, visualizer under.
- [ ] Tray and Power: `menu_row`s and `separator`s; check/radio as leading glyphs `check`/`radio_button_checked` instead of unicode.
- [ ] Launcher: 20px light query text, separator, selected row accent fill with `on_accent` text, kind label trailing `muted`.
- [ ] OSD: `surface(Popover,18)` around `pill_slider`.
- [ ] Run each existing native panel/launcher/overlay test individually with `--ignored`; PASS. Commit `feat(ui): macOS dropdown styling`.

### Task 5: System Settings

**Files:** Modify `crates/rune-ui/src/settings.rs`, `crates/rune-ui/src/gpui_settings.rs`

**Interfaces — Produces:**
- `Page::GROUPS: [&[Page]; 3]` = `[Wifi, Bluetooth, Sound]`, `[Appearance, Theme, Wallpaper, Bar, Animations]`, `[Launcher, Calendar, Weather, Capture]`; `Page::Bar` label becomes "Menu Bar".
- `Page::tint(self) -> u32` fixed RGBA: Wifi/Bluetooth `0x0a84ffff`, Sound `0xff375fff`, Appearance `0x3a3a3cff`, Theme `0xbf5af2ff`, Wallpaper `0x32ade6ff`, Bar `0x636366ff`, Animations `0x30d158ff`, Launcher `0x8e8e93ff`, Calendar `0xff453aff`, Weather `0x64d2ffff`, Capture `0xff9f0aff`.
- `State.search: String`, `Msg::Search(String)`; `pub fn visible_groups(query: &str) -> Vec<Vec<Page>>` (case-insensitive label match, empty groups dropped).

- [ ] Tests: `visible_groups("")` has 3 groups and 12 pages; `visible_groups("wi")` == `[[Wifi]]`; `visible_groups("SOUND")` == `[[Sound]]`; `visible_groups("zzz")` is empty.
- [ ] FAIL → implement → PASS.
- [ ] Render: sidebar 230 `surface(Sidebar)`, red close dot 12px (`Msg::Close`), search `TextInput` (id `settings-search`, `Msg::Search`), groups separated by 12px, rows 22px tinted icon squares radius 6, selected accent fill; "No Results" when empty. Content: toolbar row with page title 15px bold; centred column max 620; groups radius 10 `foreground`@0.05 with 12px `muted` headers; rows label left/control right, hairline separators; `toggle` becomes `switch`. Remove the Close text button.
- [ ] Run `native_all_pages_and_validated_edits` individually; PASS. Commit `feat(ui): System Settings layout`.

### Task 6: Verify and document

- [ ] `cargo fmt --all`, clippy, `cargo test --workspace`, every native test individually, `cargo build --release`.
- [ ] Restart release, `rune-shell ping`; screenshot bar and each dropdown/Settings with `grim` on DP-1, blur on and off; compare to mockup.
- [ ] Write `docs/macos-menu-bar-verification.md` (what was checked, limits). Update `examples/config.toml` comments only if keys changed. Commit `docs: macOS menu bar verification`.
