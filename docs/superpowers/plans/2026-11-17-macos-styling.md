# macOS styling implementation plan

Goal: apply the approved full-width menu bar and coordinated macOS control styling.
Spec: ../specs/2026-11-17-macos-styling-design.md

- [x] Replace notch silhouette and intrinsic content width with three flexible bar groups in gpui_notch.rs. Preserve output measurement and cursor tracking.
- [x] Refine shared surfaces, Spotlight search field, settings switches and navigation, and Control Center tiles in existing GPUI modules.
- [x] Check empty/repeated widget groups, configured fonts/colors, fractional-scale sizing, Escape dismissal, and disabled controls using existing tests and live inspection.
- [x] Run cargo fmt --all, workspace tests, Clippy with warnings denied, and release build. Restart the current Rune session and inspect the available display.

Keep existing runtime boundaries, positive viewport guard, image caches, and unrelated work. Do not add dependencies or claim performance changes.
