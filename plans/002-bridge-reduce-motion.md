# 002 — Apply the config's reduce-motion setting to GPUI animations

- **Status**: DONE
- **Commit**: 514eec5 (with uncommitted working tree)
- **Severity**: MEDIUM
- **Category**: Accessibility
- **Estimated scope**: 1 file (`crates/rune-ui/src/backend.rs`), ~4 lines

## Problem

Switches animate with GPUI's `with_spring` (`crates/rune-ui/src/gpui_widgets.rs:168`). It settles instantly only when `cx.reduce_motion()` is true, and nothing in the repo calls `cx.set_reduce_motion(..)` (`grep -rn set_reduce_motion crates/` returns nothing). So `animations.enabled = false` and `animations.reduce_motion = true` in `config.toml` are ignored by switches. The repo's own spring honors them through `AnimationsConfig::duration()` (`config.rs:447`), which returns zero when `!enabled || reduce_motion`.

```rust
// crates/rune-ui/src/backend.rs:83 — current start of reconcile
fn reconcile(shell: Entity<Shell>, backend: Shared, cx: &mut gpui::App) {
    let desired = shell.read(cx).surfaces.desired.clone();
```

`reconcile` runs after every dispatched message, including config reloads.

## Target

```rust
fn reconcile(shell: Entity<Shell>, backend: Shared, cx: &mut gpui::App) {
    let reduce = shell.read(cx).config.animations.duration().is_zero();
    if cx.reduce_motion() != reduce {
        cx.set_reduce_motion(reduce);
    }
    let desired = shell.read(cx).surfaces.desired.clone();
```

The comparison keeps this from writing the flag on every message.

## Steps

1. Insert the three lines above at the top of `reconcile` in `crates/rune-ui/src/backend.rs`. If `config` is not readable from `backend.rs` (it is `pub config` on `App` in `lib.rs`), STOP and report.

## Boundaries

- Do NOT change `AnimationsConfig` or any view.

## Verification

- **Mechanical**: `cargo fmt --all`, clippy clean, `cargo test -p rune-ui`.
- **Feel check**: Settings → Animations → turn on Reduce motion, then toggle any switch: the knob jumps with no slide. Turn it off: the slide is back.
- **Done when**: both states behave as described without a restart.
