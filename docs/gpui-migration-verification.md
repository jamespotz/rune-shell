# GPUI migration verification

The default `rune_ui::run` entry point now runs GPUI. Iced, iced_layershell,
the Iced-only winit pin, and the workspace softbuffer patch have been removed.
GPUI and gpui_platform use the same immutable upstream revision.

## Checks performed

On the current Fedora/Hyprland Wayland session, with Rust 1.100.0:

- `cargo test --workspace --no-fail-fast`: 118 passed, zero failures; nine native
  compositor tests are ignored by the default suite.
- All nine ignored native tests were run individually, one process per test, and
  passed. This includes full backend open/switch/quit, popup lifecycle and OSD
  expiration, intrinsic panel sizing from initial 1×1 bounds, pointer-driven
  slider clamping/release, native Unicode/IME editing, launcher search, all bar
  widgets including repeated widgets, populated/empty feature panels, named
  `DP-1` targeting, window close/reopen, and all 12 settings pages.
- Settings native checks included wallpaper thumbnails, both theme sources,
  invalid/valid draft edits, focus preservation, page focus transfer and Escape.
- `cargo build --release`: passed. `target/release/rune-shell --help`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `cargo tree --workspace` and Cargo.lock contain no Iced, iced_layershell,
  softbuffer or winit-core dependencies.
- Pre-existing changes to the seven service/CLI files match the saved baseline
  patch exactly. The UI changes were integrated with the pre-existing App edits.

Independent source review found popup content clipping, missing ordinary-popup
click dismissal, and silent unresolved connector mapping. The fixes restore
bounded thumbnail/text/action geometry, preserve popup/history click behavior,
and report unresolved connectors without repeating the message every frame.
A click-behavior regression test was observed failing before the fix and passing
after it. Native backend tests additionally caught and fixed reentrant window
recreation, OSD deadline delivery, and collapsed initial panel geometry.

## Limits of this verification

The live display is a single `DP-1` output at 1.25 scale. Connector reordering,
missing outputs and duplicate requests have deterministic model tests, but actual
multi-monitor hotplug and mixed-scale output transitions were not exercised.

Services and their action protocol were preserved. Rendering tests use service
fixtures; they do not constitute live Bluetooth pairing, Wi-Fi joining, recording,
notification-daemon takeover, or theme-export tests against real devices/apps.
Clipboard editing logic passed native tests, but a compositor clipboard ownership
and cross-application paste roundtrip was not independently verified.

No comparable before/after memory or GPU-memory benchmark was made. Historical
software-renderer memory figures were removed from current-backend claims.

## Fractional-scale release startup regression

The first real daemon startup exposed a GPUI platform race that the fixture
window tests had missed. On `DP-1` at scale 1.25, the unpatched release binary
sent `wp_viewport.set_destination(0, 32)` before the compositor configured the
bar to `2752×32`. Hyprland responded `Size was <= 0`, then the Wayland backend
repeated protocol errors.

The local `gpui_linux` patch keeps zero dimensions in the layer-shell stretch
request but defers viewport destinations until their integer width and height
are positive. Two focused tests failed before the validator existed and pass
with it, including subpixel-to-zero conversion. The regression tests can run
without a compositor using the commands in `vendor/gpui_linux/README.rune.md`.

The rebuilt release binary was run with the original daemon command and
`WAYLAND_DEBUG=client` for six seconds on the same display. It survived startup,
sent `set_destination(2752, 32)` with no zero destination or protocol error, and
exited with status 0 after `rune-shell quit`. Catcher and quick-settings viewport
sizes were also positive. The workspace's 118 tests passed with the patch.
