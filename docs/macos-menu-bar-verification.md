# macOS menu bar verification

Implements `docs/superpowers/specs/2026-11-17-macos-menu-bar-design.md`.

The bar is a single frosted full-width strip. Workspaces are plain numbers with a rounded fill on the active slot. Status items are icon-only, and each item keeps a rounded highlight while its dropdown is open. Dropdowns, notification cards, the launcher, the OSD and Settings share one material helper (`gpui_widgets::material`). With blur on, the material is translucent at 0.70–0.92. With blur off, it is 0.96. The Hyprland `rune-blur` layer rule now covers `rune-panel`, `rune-notifications` and `rune-osd` as well as the bar and Settings.

New dropdowns: Wi-Fi, Bluetooth, Sound and Battery, also reachable as `rune-shell toggle wifi|bluetooth|sound|battery`. If you pick a secured Wi-Fi network that has no saved password, Settings opens on the Wi-Fi page and asks for it. Opening the Wi-Fi dropdown starts a scan. Middle-clicking the volume item mutes, and scrolling still changes the volume.

Settings has a frosted sidebar with a red close dot, a page search, grouped pages with tinted icons and accent selection, and grouped rounded lists with switches.

## Checks

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: all pass. New tests cover material opacity, blur rule coverage, widget→dropdown mapping, media label truncation, the Wi-Fi scan on open, secured-network handoff, CLI names and page search.
- All ten native tests passed when run individually on Wayland. `wayland_notch_on_named_output` needs `RUNE_TEST_OUTPUT=DP-1`. The panel test now renders the four new dropdowns with and without their services.
- Release build restarted, `ping` → `pong`, and the log showed no new errors.
- Inspected screenshots of the bar, every dropdown and two Settings pages on DP-1 at 1.25 scale, with blur on, plus the bar and Control Center with blur off.

## Limits

Only one output at one scale was checked. Another shell (`helios`) was running on the same output during inspection, so Rune's bar sat below it. Battery hardware is absent, so only the dropdown's "Unavailable" state was seen live. Launcher and OSD were checked through the native tests, not screenshots. No performance claims are made.

## Settings pages redesign

Each page now opens with a header showing its tinted icon, a one-line description and, where it applies, its main switch or picker. Below that come grouped lists. Explanations that used to sit in parentheses in a label now appear as a second line under it. Selections use a check mark instead of ✓/● text. Theme presets and wallpapers are tile grids with an accent ring on the current one. The Menu Bar page moves items with arrow buttons, removes them with a red button, and has a single "Add an Item" group with a Left/Center/Right picker. Each Bluetooth device shows its status and battery, an Audio/Call mode picker, automatic connection and Forget. Hyprland shortcuts appear as code lines. The close control is an X in the toolbar.

Checked: fmt, workspace Clippy and tests, and the native Settings, text input and backend tests run individually. All twelve pages were screenshotted on DP-1 at 1.25 scale by clicking through the sidebar.

## Bar gap

`bar.gap` (default 8, Settings → Menu Bar → Gap below) reserves free space between the bar and the windows below it: the bar's layer-shell exclusive zone is `height + gap`. Checked live: Hyprland reserved the top 40 px for Rune's 32 px bar. Dropdowns respect that reserved zone, so they now sit the same gap lower.

Settings → Menu Bar → Size also edits `bar.width` (0 fills the screen), `bar.radius`, `bar.margin_top` and `bar.margin_x`. The layer surface stays full width and `margin_top + height` tall so pointer x still maps to screen x for dropdown placement; the visible bar is a centered, rounded box inside it, and the exclusive zone is `margin_top + height + gap`. Only `margin_top` (with `height`, `gap` and `outputs`) reopens the bar surfaces; width, side gaps and rounding are render-only. With `margin_top = 0` only the bottom corners round, which gives a notch. Switches animate their knob and track with GPUI's interruptible spring keyed by the switch ID, so reduce-motion and mid-flight reversals are handled by GPUI.

Settings edits update the in-memory config at once and save it. The file watcher's reload of the shell's own save is ignored for 1.5 s after the last save, so an older echo cannot revert a newer toggle; an external edit made within that window is picked up by the next save or `rune-shell reload`. The theme worker returns only the generated colors (`UiEvent::ThemeColors`), which are merged into the current config, so a theme run started before another toggle cannot overwrite it. Without blur every material is fully opaque; notification card hover and press tint the card's material instead of replacing it with a translucent fill.

`bar.look` (Settings → Appearance → Look) selects `glass` (default) or `pixel`. Every corner radius goes through `gpui_widgets::corner`, which returns zero for the pixel look; `gpui_widgets::text_font` and `gpui_widgets::heading` switch all text to Tiny5; `gpui_widgets::fill` paints slider and OSD fills as 4 px dashes with 2 px gaps; switches quantize their spring phase to four whole steps; `gpui_widgets::frame` adds a 1 px hairline and a hard 3 px offset shadow to settings groups, page headers and quick-settings tiles. The look is mirrored into a process-wide flag in `reconcile` so render helpers without a config reference can read it. Window-level surfaces get no outer shadow because layer surfaces are sized to their content and would clip it. Tiny5 must be installed (SIL OFL, Google Fonts); without it GPUI falls back to the default font.
