# macOS styling for Rune

## Intent

Replace the central notch presentation with a full-width menu bar and give Rune recognizable macOS visual structure. Preserve existing shell features, native GPUI rendering, user configuration, and service boundaries.

## Approaches

- Restyle the notch. Smaller patch, but retains the silhouette the user found insufficiently macOS-like.
- Full-width menu bar with coordinated launcher, settings, and feature panels. Recommended in the conversation; proceeding on the user's instruction to continue.
- Add a Dock and recreate macOS application menus. This introduces new product behavior beyond the styling request and is excluded.

## Design

The menu bar spans each configured output. Workspaces and the focused window title occupy the left, the configured center group remains centered, and status controls and the clock occupy the right. Preserve configurable widget groups. Use compact, quiet controls and a subtle lower edge; remove the notch shoulders and content-width silhouette. Reserve the configured bar height, preserving the initial tiny-window sizing workaround and positive viewport dimensions.

The launcher resembles Spotlight: a prominent search field, a clear selected result, subdued subtitles, and restrained separators inside a rounded floating surface. Retain application, Steam, emoji, calculator, and keyboard behavior.

Quick settings resembles Control Center with grouped connectivity controls, larger stateful tiles, and clearly labeled volume and brightness sliders. Other feature panels share consistent spacing, neutral surfaces, readable secondary text, and blue selections.

Settings uses a distinct sidebar, selected navigation rows, a clear page heading, and grouped setting rows. Replace textual checkmark toggles with visible switch controls while retaining accessible labels, stable IDs, and immediate press feedback. Use the existing icon font and available configurable text fonts.

Use translucent neutral materials where the existing compositor blur supports them, with sufficiently opaque backgrounds for readability without blur. Preserve configured colors and fonts. Avoid claiming platform vibrancy or adding rendering dependencies. Keep motion interruptible and restrained, and honor reduced motion.

## Constraints

Extend existing GPUI modules. Preserve disk and IPC work on the existing background/runtime paths. Do not introduce focus changes or surface commits during render, frame-rate surface polling, new services, a Dock, or application-menu integration. Preserve unrelated uncommitted work and image cache retirement.

## Acceptance

The running shell visibly presents a full-width bar, Spotlight-style launcher, grouped Control Center controls, and coordinated settings. Check clipping, panel placement, keyboard dismissal, switch behavior, and fractional-scale sizing on the available Wayland display. Run formatting, workspace Clippy, appropriate tests, and release build. Report unavailable visual checks and do not claim performance improvements without measurements.
