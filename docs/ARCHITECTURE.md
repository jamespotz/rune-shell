# Architecture

## Data flow

The daemon owns a two-worker Tokio runtime and all service I/O. Services publish
snapshots through watch channels; the forwarder sends each snapshot to
`rune_ui::run` as a `UiEvent`. The UI runs GPUI on the calling thread and waits on
that channel without polling. User actions return through `mpsc<Action>` to the
daemon. CLI commands and configuration hot reload use the same event path.

`rune-shell quit` returns only after the daemon process has exited, so
`rune-shell quit && rune-shell` restarts safely. The daemon answers `ok` and then
leaks that connection's write half instead of dropping it. The kernel closes it at
process exit, after the control socket is removed and every D-Bus name is
released, and only then does the client read EOF. Before that, the daemon waits
up to three seconds for the equalizer to put the default output back and stop its
`pipewire -c` filter chain, so a restart never finds a stale `rune_equalizer`
sink. Shutdown waits at most one
second for blocking tasks, such as reapers for launched apps, before the process
exits. The client gives up after ten seconds.

`rune-ui/src/state.rs` holds service snapshots. `App` owns transitions, settings
drafts, launcher matching, panel placement and animation state. `surfaces.rs`
records desired window specifications. The GPUI backend reconciles those specs
with real windows, applies clipboard/quit effects, and observes output changes.
Views read the shared state entity and emit messages; they do not perform service
I/O. Window changes are deferred until state and window callbacks release their
borrows, because GPUI may render a new window synchronously.

## Rendering and windows

Rune uses native GPUI Wayland layer-shell windows and the wgpu renderer. GPUI and
its matching platform package are pinned to revision
`917559da35dd4d626205da705b071c800c9c2553`. The published GPUI 0.2.2 package does
not include the required layer-shell support. Updating the pin requires checking
both the platform API and connector identity implementation. A local patch of
`gpui_linux` defers viewport destinations until zero layer-shell stretch
dimensions have been resolved by configure, avoiding fractional-scale startup
protocol errors. See `vendor/gpui_linux/README.rune.md`.

Bars target selected Hyprland connector names. The pinned Wayland backend derives
a display UUID from the connector name using UUID v5 with the DNS namespace;
`outputs.rs` uses the same mapping rather than relying on enumeration order.
Unresolved outputs wait for discovery and never fall back to an unrelated output.
Display change notifications trigger reconciliation and monitor removal closes
orphaned bars and panels.

The stable compositor namespaces are `rune-bar`, `rune-panel`, `rune-catcher`,
`rune-settings`, `rune-notifications`, and `rune-osd`. The bar reserves its height.
Panels open near the clicked widget, clamp to the output, remember measured sizes,
and have a separate transparent click-outside catcher. Launcher windows take
exclusive keyboard focus; settings and other panels use on-demand focus. The OSD
has an empty input region so input passes through it.

Custom GPUI painting draws the notch shoulders. Shared controls implement native
text input, Unicode/IME selection, clipboard shortcuts, password masking and
pointer-driven sliders. Media seek changes are committed on release. Views cache
image conversions and replace them only when their source changes. Raw service
RGBA pixels are converted to GPUI's BGRA format.

Tween durations and reduced-motion settings retain their configuration schema.
Frame work is scheduled while transitions or measurement need it. Timed OSD
expiration is handled even when no animation is active. GPUI uses explicit quit
mode, so temporarily closing the last window does not terminate the shell.

## Service boundaries and compatibility

Hyprland IPC remains a small query-driven client. Events trigger snapshots;
workspace names and active IDs are read from Hyprland rather than reconstructed
from an event history. Commands retain the compositor's Lua dispatch syntax.

Hyprland settings (Settings → Windows, Effects, Input, Window Motion) follow
hyprmod's overlay design. Rune never edits the user's Lua. It writes only
`~/.config/hypr/modules/rune.lua`, which a one-time `require("modules.rune")` at
the end of `hyprland.lua` loads last, so Rune overrides just the values changed
there. Rune doesn't parse that file back: the overrides live in `[hyprland]` in
`config.toml`, and `rune.lua` is generated from them. Keys come from the fixed
`hyprland_config::OPTIONS` schema, and every value is clamped or matched against
a list before it becomes Lua, so stored config can't inject code. Writes resolve
symlinks first, then use a temp file and rename in the real directory, which keeps
dotfile-manager links intact. Edits preview through `eval` on a `watch` channel, so
a slider drag runs only the newest state. Each preview carries every entry touched
this session, which lets coalescing drop intermediate states safely, and the full
set is applied again after a config reload. Hyprland doesn't report spring
parameters, so baselines for those come from literal `hl.curve` numbers in the
user's Lua. Bezier baselines come from `j/animations`, with literal dotfile
control points retained by the source scan. Named overrides store either a spring
or four bezier control points. Window Motion provides 19 CSS easing presets and
custom curves; animation previews include their curve definitions before the
leaves that reference them. Renaming and deleting a custom curve updates those
references, and deleted names stay hidden from stale live snapshots until reload.
The same read-only scan follows `hyprland.lua` and its `require`s in
load order to label which file sets each value. Only literal assignments count. Display settings are left out: per-monitor VRR, bit depth and
colour mode can't be read over IPC, and an override would silently reset them.

The lock screen (Settings → Lock Screen) follows the same rule. Settings live in
`[lock]` in `config.toml`; Rune generates `~/.config/hypr/rune-hyprlock.conf` from
them plus the theme colors, look and wallpaper, and never touches the user's
`hyprlock.conf`. When `lock.enabled` is on, Lock runs
`hyprlock --config rune-hyprlock.conf --grace N`, otherwise `power.lock_command`.
A daemon worker regenerates the file only when one of those inputs changes.
hyprlock can only blur, so the pixel effect is a pixelated copy of the wallpaper
cached under `~/.cache/rune-shell/wallpaper/`. Screenshots are taken by hyprlock
at lock time, too late to pixelate, so they always blur. Now playing and status
icons are hyprlock `cmd[]` labels polling `rune-shell lock-media` and
`rune-shell lock-status`, which read the daemon's service snapshots; their text is
escaped as Pango markup because track titles come from D-Bus. Every text value
written to the file is stripped of control characters with `#` escaped, so
stored config can't add hyprlang lines.

Rune is the polkit authentication agent for its login session (`polkit.rs`). It
registers on the system bus for `XDG_SESSION_ID`, or logind's display session
when that is unset, and serves one request at a time. Only calls from the owner of
`org.freedesktop.PolicyKit1` are accepted, so other bus clients can't raise a
fake dialog. Rune never checks passwords: the typed response goes to the setuid
`polkit-agent-helper-1`, which runs PAM and reports to polkitd itself. The helper
starts on the first submit, so PAM modules that act before a prompt (fingerprint)
wait for Enter. Responses travel as `polkit::Secret`, whose `Debug` is redacted.
The dialog is the `rune-polkit` surface: full screen, exclusive keyboard, over the
bar. If another agent already serves the session, registration fails and Rune's
dialog stays off.

Wireless peripheral batteries come from UPower: `system::spawn_peripherals` lists
devices with `EnumerateDevices`, re-reads on any UPower signal and every 60 s to
catch missed signals, and resubscribes with 1–30 s backoff when the stream ends.
Results are bounded (32 devices, 128-char strings, 0–100 %) and reach the UI as
`UiEvent::Peripherals`. Each `device_battery` bar item picks one by `Peripheral::key`.

Bar lists hold `BarItem`s: a widget plus its own `WidgetStyle` (icon, icon colour,
border, radius) and, for device batteries, a device key. A plain widget name still
parses and default items serialize back to names. Settings → Menu Bar selects an
item to edit it; its colours, like every colour in Settings, open the
`controls::color_picker` popover, which reports through the same settings
messages as the hex fields.

Calendar, notifications, tray, Bluetooth, audio, NetworkManager, UPower, weather,
capture, wallpaper and theme exports remain service responsibilities. Optional
backends publish unavailable states so the corresponding controls can hide or
report their status. Bluetooth scanning follows the settings page lifecycle.

Configuration remains TOML with defaults, strict unknown-field rejection and
validated clock formats. Invalid settings input stays in a draft. Loading a broken
config keeps the prior state, or defaults at startup. Existing config, theme
backups and clipboard metadata need no data conversion for this migration.

## Build and verification

Linux builds need the Rust toolchain plus native fontconfig, FreeType,
libxkbcommon and Wayland development libraries, and runtime Vulkan drivers.
See README for Fedora packages. The old software-renderer memory figures are
historical and are not a GPUI performance guarantee.

Run `cargo fmt --all -- --check`, `cargo check --workspace`,
`cargo test --workspace --no-fail-fast`, and `cargo build --release`. Native
Wayland tests are ignored in the default suite; invoke individual tests by name
with `--ignored --exact --test-threads=1`. They exercise settings pages, native
text editing, sliders, named-output targeting, window transitions, feature panels,
and the complete backend lifecycle. A single-output test cannot establish
multi-monitor hotplug or mixed-scale correctness; report those checks separately.
