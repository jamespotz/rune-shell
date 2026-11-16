# Rune Shell

A desktop shell for Hyprland, written in Rust from end to end: no GTK and no C libraries.
The bar is a MacBook-style notch: a solid island at the top centre of each monitor,
flush with the screen edge, with concave shoulders and a width that follows its content.
Inside it: clickable workspaces (animated), the focused window's title, a clock and a
settings button. The settings window covers Appearance, Bar, Launcher and Animations.

The notch expands into a **launcher**: fuzzy search over apps and installed Steam games,
`:` for emoji (Enter copies), and a calculator (`12*3`, `sqrt(2)`; Enter copies). It also
expands into a **keybind cheat sheet** listing your Hyprland binds and their descriptions.

**Themes and wallpaper** (Settings → Theme / Wallpaper):
- Pick a wallpaper from a folder; images and GIFs go through `awww`, videos through `mpvpaper`.
- Pick a preset palette (Catppuccin, Gruvbox, Nord, Tokyo Night, Rosé Pine, Dracula, Everforest)
  or generate one from the wallpaper (Material You: tonal spot, vibrant, expressive, fruit salad,
  content, fidelity, rainbow, neutral, monochrome; light or dark).
- Choose which apps follow the theme: kitty, Ghostty, btop, Yazi, Neovim (`:colorscheme rune`),
  Zed, VS Code, Kiro, GTK 3, GTK 4/libadwaita, Qt (qt5ct/qt6ct), KDE. Running kitty, Ghostty and
  GTK apps refresh live.
- Before changing any file Rune backs it up (contents, symlinks, settings); "Restore original app
  themes" or `rune-shell restore-themes` puts everything back exactly. Symlinked configs (e.g. into
  a dotfiles repo) are replaced, never written through.

**Notifications**: Rune is a full `org.freedesktop.Notifications` daemon: popups in the top-right
corner stacked like Sonner (hover to fan them out; timers pause while you do), action buttons,
urgency (critical stays until dismissed, with a red edge), images, and a notification center in
the notch (`notifications` widget or `rune-shell toggle notifications`) with history and Do not
disturb. If another shell already runs a notification daemon, Rune leaves it alone.

**System tray**: the `tray` widget shows StatusNotifierItem icons. Left click activates, middle
click is the secondary action, right click opens the app's menu in the notch. Rune is the tray
registry itself unless another shell already is, in which case it joins as a host.

**Weather**: pick a place in Settings → Weather (a search, so only that place is ever sent to
Open-Meteo; no key needed). The `weather` widget shows the current conditions; click it (or
`rune-shell toggle weather`) for feels-like, humidity, wind, sunrise/sunset, the next 24 hours
and 7 days. Refreshes every 30 minutes.

**Bluetooth**: Settings → Bluetooth (or the `bluetooth` widget, or `rune-shell open bluetooth`)
lists devices with icons and battery. Connect, disconnect, pair, forget, and "connect
automatically". Pairing a keyboard shows the code to type on it; older devices get the usual
PIN 0000. Connected headsets get **Audio** / **Call** buttons to switch between
high-quality playback and the microphone profile (via `pactl`). Scanning runs only while the
page is open.

**Screenshots, recording, text**: `rune-shell screenshot screen|all|window|selection` (grim/slurp;
saved and copied), `rune-shell record screen|window|selection` (gpu-screen-recorder; run it
again, `record stop`, or click the red timer in the bar to stop), and `rune-shell ocr` (select an
area, its text is copied, via tesseract). Folders, audio, frame rate and OCR languages live in
Settings → Capture, which also lists keybind lines to paste.

**Game mode**: `rune-shell game-mode [on|off|toggle]` turns off Hyprland animations, blur,
shadows, gaps, rounding and transparency, and silences notifications. Turning it off reloads your
Hyprland config, which restores everything exactly. A controller badge shows while it's on.

**Calendar**: add ICS links in Settings → Calendar (Google's "secret address in iCal format",
Outlook, Apple, Nextcloud, Proton, `webcal://`). Click the clock (or `rune-shell toggle calendar`)
for a month view and the day's events, with a **Join** button for Meet, Zoom, Teams, Webex,
Whereby and Jitsi links. Recurring events, exceptions and time zones are handled. Read-only.

**Volume, network, battery and OSD**: the `volume` widget (click to mute, scroll to change),
`network` (Wi-Fi signal or wired) and `battery` (hidden without one). `rune-shell volume
up|down|mute|set N` and `rune-shell brightness up|down|set N` show an on-screen popup; bind your
media keys to them.

**Quick settings, power, clipboard**: the `quick_settings` widget (or `rune-shell toggle
quick-settings`) has Wi-Fi, Bluetooth, Do not disturb and Game mode tiles, a volume (and
brightness) slider, and shortcuts. The power menu (`rune-shell toggle power`) locks, suspends,
logs out, restarts or shuts down; the last three ask for a second click. `rune-shell lock` runs
`[power] lock_command` (default `loginctl lock-session`, which uses hypridle's locker).
`rune-shell toggle clipboard` searches `cliphist` history; Enter copies an entry back.

**Browsers** (Settings → Theme → Apply to apps):
- **Firefox**: Rune writes `chrome/rune.css` in your default profile, adds `@import "rune.css";` to
  `userChrome.css` and enables `toolkit.legacyUserProfileCustomizations.stylesheets` in `user.js`.
  Restart Firefox to see it.
- **Chrome**: Rune keeps a theme in `~/.config/rune-shell/chrome-theme`. Load it once: open
  `chrome://extensions`, turn on Developer mode, "Load unpacked", pick that folder. After that,
  restarting Chrome picks up new colors.
- **Vivaldi**: Rune writes `~/.config/vivaldi/rune/rune.css`. Once: enable "Allow CSS
  modifications" in `vivaldi://experiments`, then set Settings → Appearance → Custom UI
  Modifications to `~/.config/vivaldi/rune`. Restart Vivaldi to see changes.

**Music**: the `media` widget shows album art and a visualizer for whatever MPRIS player is
active (Spotify, browsers, mpv...). Click it to expand the notch into a player with a position slider
(drag to seek) and prev / play-pause / next / stop. The visualizer uses `cava` (runs only while playing) and
web album art is fetched with `curl`; both are optional.

## Build

Fedora needs only the Rust toolchain. There are no `-devel` packages, because nothing links
against C libraries except libc:

```sh
sudo dnf install rust cargo   # or rustup, latest stable
cargo build --release
install -Dm755 target/release/rune-shell ~/.local/bin/rune-shell
```

Fonts: the defaults are `JetBrainsMono Nerd Font` for text and `Material Symbols Rounded`
for icons (icons are written by name, e.g. `schedule`). Any installed font works. Run tests with
`cargo test`.

## Hyprland setup

Hyprland 0.56 and later (Lua config):

```lua
hl.exec_once("rune-shell")
hl.bind("SUPER + SHIFT + R", hl.dsp.exec_cmd("rune-shell reload"))
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("rune-shell toggle launcher"))
hl.bind("SUPER + SLASH", hl.dsp.exec_cmd("rune-shell toggle keybinds"))
hl.bind("SUPER + COMMA", hl.dsp.exec_cmd("rune-shell toggle settings"))
-- optional: blur behind the bar
-- hl.layer_rule({ match = { namespace = "rune-bar" }, blur = true })
```

Check the layer-rule syntax against your Hyprland version. The layer namespace is `rune-bar`.

## CLI

| Command | Effect |
|---|---|
| `rune-shell` | start the shell (refuses if one is already running) |
| `rune-shell reload` | re-read the config (this also happens on every save) |
| `rune-shell ping` | prints `pong` if the shell is running |
| `rune-shell quit` | stop the shell |
| `rune-shell toggle launcher` | open or close the launcher (Esc or a click outside closes it) |
| `rune-shell toggle keybinds` | open or close the keybind cheat sheet |
| `rune-shell toggle media` | open or close the music player |
| `rune-shell toggle notifications` | open or close the notification center |
| `rune-shell toggle weather` | open or close the forecast |
| `rune-shell open bluetooth` | open settings on the Bluetooth page |
| `rune-shell toggle calendar` | open or close the calendar |
| `rune-shell screenshot [screen\|all\|window\|selection]` | take a screenshot |
| `rune-shell record [screen\|window\|selection\|stop]` | start or stop a screen recording |
| `rune-shell ocr` | copy the text in a selected area |
| `rune-shell game-mode [on\|off\|toggle]` | game mode |
| `rune-shell restore-themes` | put back every app theme file Rune changed (works without the shell running) |
| `rune-shell toggle settings` | open or close the settings window (Esc also closes it) |

Commands exit with status 1 on error. Logs go to stderr; set the level with `RUST_LOG=debug`.
Rendering is software (tiny-skia) by default; `ICED_BACKEND=wgpu` switches to the GPU at the
cost of roughly 190 MB of memory.

## Configuration

`~/.config/rune-shell/config.toml`. Every key is optional, and a missing file means
defaults. See [`examples/config.toml`](examples/config.toml) for every key and its default.
Unknown keys and invalid values are rejected with an error that names them. A broken
config at startup falls back to the defaults, and a broken config on reload keeps the
previous one. The settings window writes this file too; it keeps your values but not your
comments.

## Layout

```
crates/
  rune-services/  Hyprland IPC client and clock. tokio tasks publishing via watch channels
  rune-ui/        config schema, notch and settings window (iced + iced_layershell)
  rune-shell/     binary: CLI client, control socket, config loading and hot reload
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for design decisions and the roadmap.
