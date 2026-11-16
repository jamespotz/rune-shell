# Glossary

Names used in Rune Shell's code and docs. Use these words, not synonyms.

**Notch**: the bar. A solid island hanging from the top edge of each monitor,
with concave shoulders. One per monitor (`Surface::Bar`).

**Panel**: a popover that opens under the notch: launcher, keybinds, clipboard,
media, notifications, weather, calendar, quick settings, power, tray menu. At
most one is open; it opens under the widget that was clicked, or centered when
opened from the CLI. Owned by `panel::Panels`. *Avoid*: popup, menu, dropdown.

**Catcher**: the transparent full-screen surface under an open panel. A click on
it closes the panel. Exists only while a panel is open.

**Surface**: a Wayland layer-shell window Rune shows: notch, panel, catcher,
popups, OSD, settings. Opened and closed only through `surfaces::Surfaces`.
*Avoid*: window (except for the settings window, which users see as one).

**Popups**: notification toasts in the top-right corner. Not a panel.

**OSD**: the on-screen display shown briefly after a volume or brightness change.

**Action**: a request from the UI to the daemon (`rune_ui::Action`).

**UiEvent**: news from the daemon to the UI: new state, or a command from the
CLI (`rune_ui::UiEvent`).

**Service**: a module in `rune-services` that talks to the outside world (D-Bus,
Hyprland, `cliphist`, files) and publishes state or answers commands.

**Clipboard history**: what was copied, newest first, recorded by `cliphist`,
plus Rune's own copy times and favorites (`clipboard::Clipboard`). Entries are
named by `clipboard::Id`, never by cliphist's text.

**Favorite**: a clipboard entry the user starred. Rune keeps its own copy, so
it survives *Clear history* and cliphist's size limit. While favorited, the
original history entry is hidden.
