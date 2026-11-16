# Architecture

## Data flow

The daemon owns a two-worker Tokio runtime and all service I/O. Services publish
snapshots through watch channels; the forwarder sends each snapshot to
`rune_ui::run` as a `UiEvent`. The UI runs GPUI on the calling thread and waits on
that channel without polling. User actions return through `mpsc<Action>` to the
daemon. CLI commands and configuration hot reload use the same event path.

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
