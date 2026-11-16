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

Each page now opens with a header showing its tinted icon, a one-line description and, where it applies, its main switch or picker. Below that come grouped lists. Explanations that used to sit in parentheses in a label now appear as a second line under it. Selections use a check mark instead of ✓/● text. Theme presets and wallpapers are tile grids with an accent ring on the current one. The Menu Bar page moves items with arrow buttons, removes them with a red button, and has a single "Add an Item" group with a Left/Center/Right picker. Each Bluetooth device shows its status and battery, an Audio/Call mode picker, automatic connection and Forget. Hyprland shortcuts appear as code lines. The close control is an X in the toolbar. The unused bar corner-radius setting is gone from the page, but the config key remains.

Checked: fmt, workspace Clippy and tests, and the native Settings, text input and backend tests run individually. All twelve pages were screenshotted on DP-1 at 1.25 scale by clicking through the sidebar.
