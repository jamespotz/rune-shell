# Capture widget design

Date: 2026-11-17. Mockup: `.superpowers/brainstorm/136745-1794928001/content/capture-widget.html` (layout A).

## Intent

Screenshots, recordings and OCR work today only from the CLI or keybinds. The user wants to reach
them with the mouse too, from a bar widget they can add, move or remove like any other. The
keybind hints should also cover every recording target, not only selection.

## Goals

- A `capture` bar widget: a camera button that opens a capture panel.
- The panel (layout A): a segmented **Screenshot / Record / Text** switch on top, then target tiles.
  - Screenshot: Area, Window, Screen, All.
  - Record: Area, Window, Screen.
  - Text: no tiles. Choosing it starts OCR at once (OCR only works on an area).
  - Each tile shows its keybind label, taken from the Settings hint list.
  - Footer: in Screenshot mode a "Copy to clipboard" toggle (`capture.copy`); in Record mode a
    "Record audio" toggle (`capture.record_audio`); a link that opens the matching folder.
- While recording, the widget is replaced by the existing red `● m:ss` timer in its slot, and a
  click stops the recording. With no capture widget in the bar, the timer chip stays where it is today.
- `rune-shell toggle capture` opens and closes the panel.
- Settings → Capture keybind hints gain `record window`, `record screen` and `toggle capture` lines.
- Both looks (glass and pixel) work through the existing helpers (`corner`, `fill`, `material`,
  `text_font`). No look-specific code paths beyond what those give.

## Non-goals

- New capture features: delay timers, annotation, a region editor, picking the audio source.
- Editing the user's `~/.config/hypr/rune-binds.lua`. Rune only shows hint lines.
- Choosing which tiles appear in the panel. Customization means placing the widget in Settings → Menu Bar.

## Anti-goals

- The panel shows up in the screenshot or recording it started.
- The UI thread blocks on grim, slurp, tesseract or the recorder.
- A second recording state or timer separate from `snapshot.recording`.

## Constraints

- The backend already supports every target: `capture::{screenshot, ocr, spawn_recorder}`, and the
  `Action::Screenshot/Ocr/Record` actions built with `capture_options(&config)` (`lib.rs:1362`).
- Panels: add `Panel::Capture` to `panel.rs`, add `("capture", Panel::Capture)` to `NAMED`, map
  `Widget::Capture` → `Panel::Capture` in `notch.rs::widget_panel`, and render it in
  `gpui_panels.rs` next to `power`. Add it to the ignored native panel render test list.
- `Widget::Capture` goes into `Widget::ALL`, with its display name and `bar_widget` button. That is
  enough for it to appear in the Settings → Menu Bar editor. Its config name is `capture`.
- Toggles save through the existing `save_config` path. The open-folder link uses the existing
  `Action::Launch("xdg-open …")` pattern with `calendar::shell_quote` on the expanded path.
- Esc closes the panel the way other panels already do (`gpui_panels.rs:177`).
- Follow AGENTS.md: stable element IDs per tile (keyed by mode + target), press feedback, reduced
  motion, no work in `render()`.

## Approaches

1. **One Capture button + panel (chosen).** One bar slot, every option visible, reuses the panel system.
2. Three separate widgets (Screenshot, Record, Text), each with a right-click target picker.
   Rejected: hides most targets behind right clicks and uses three slots.
3. A capture row in Quick Settings. Rejected: it isn't a bar widget.

Within approach 1, the user picked layout **A (mode + tiles)** over B (one row per action).

## Decisions made by the user

- Mouse access through a bar widget, placeable like other widgets.
- Approach 1, layout A.
- Glass and pixel looks both supported.

## Calls I made (correct any)

- **Keeping the panel out of the shot:** clicking a tile closes the panel first, then dispatches the
  action after a short settle delay, using a GPUI timer on the UI side. The service and the CLI path
  stay as they are. About 150 ms; the builder tunes it against the close animation.
- **Mode memory:** the panel reopens in the last mode used this session (default Screenshot). Not persisted.
- **Opening the panel while recording** (only possible through `toggle capture`): Record mode shows a
  single "Stop recording" tile instead of targets.
- **Hint keys:** `SUPER + SHIFT + Print` → record window, `SUPER + CTRL + Print` → record screen,
  `SUPER + SHIFT + R` → toggle capture. Tile labels come from the same table so they can't drift.
- **Icons:** Material Symbols `photo_camera`, `screenshot_region`, `videocam`, `document_scanner`,
  `crop_free`, `web_asset`, `desktop_windows`, `select_all`. The builder checks that they render in
  the configured font and swaps any that don't.

## Left to the builder

- Panel width constant (similar to `POWER_W`) and tile sizing that keeps intrinsic size stable.
- Exact press/hover styling within the existing widget helpers.
- Tests: `Panel::from_name("capture")`, widget → panel mapping, the IPC `toggle capture` path,
  the mode → targets table (Record has no All, Text has none), and the timer-in-slot render case
  in the existing notch render test.
