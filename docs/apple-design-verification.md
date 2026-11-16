# Apple design verification

The GPUI shell now uses shared rounded graphite surfaces with an inset edge,
blue accent defaults, Noto Sans typography, and distinct settings navigation.
Configured colors, fonts, bar composition, and existing workflows remain supported.
Hover and press feedback covers shared buttons, bar controls, launcher results,
and settings navigation. Inputs have a tracked focus border; sliders retain
continuous dragging and release events with a clearer thumb.

The boolean animation uses a critically damped spring. Retargeting preserves
position and velocity, repeated targets do not restart it, and finite settling
allows frame scheduling to stop. Reduced motion also applies to popup expansion
and configuration changes during an active transition.

## Checks

- `cargo test --workspace --no-fail-fast`: 128 passed; ten native tests ignored by default.
- All ten native tests run individually on Wayland: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `cargo build --release`: passed; `target/release/rune-shell` rebuilt.
- Release smoke test with a temporary copy of the example config: ping, launcher,
  settings, quick settings, calendar, close/reopen, and normal quit passed.
- Inspected release screenshots on DP-1 at 1.25 scale. Launcher selection, input
  focus, typography, calendar selection, and panel edges remain readable.
- Final live log had no font-loading errors, Wayland protocol errors, or window
  recreation errors. Existing application SVG parser warnings can still occur.

## Regressions caught

Native launcher measurement reported 670.4 pixels for a 672-pixel surface after
adding a layout border. The resulting size feedback could recreate windows
repeatedly at fractional scale. The shared edge is now painted without consuming
layout space. An assertion in the native launcher test failed before that fix
and passes afterward with the full 672-pixel measurement.

Independent review caught selected controls losing their accent background during
hover/press while retaining dark text. Settings and launcher selections now retain
their accent treatment, and shared button feedback preserves an explicit fill.

The GPUI Linux system-font alias maps to IBM Plex Sans, which is absent on this
machine. The default is the installed Noto Sans family; custom font settings remain
unchanged. No fonts or rendering dependencies were added.

## Limits

Manual visual inspection covered one output at one fractional scale. Multi-output
layouts, additional scales, and broad user accessibility testing remain unverified.
Native tests cover populated feature panels, all settings pages, editing, slider
interaction, output targeting, cache retirement, and window lifecycle. This pass
makes no new memory or GPU-performance improvement claim.
