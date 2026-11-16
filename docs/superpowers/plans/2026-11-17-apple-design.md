# Apple Design Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Redesign Rune's shared appearance and feedback while preserving its workflows.

**Architecture:** Extend existing GPUI widgets with shared surface and interaction styles. Keep service and window lifecycle boundaries intact; replace the existing boolean tween internally without changing its public interface.

**Tech Stack:** Rust, GPUI, existing Tokio services.

**Spec:** `docs/superpowers/specs/2026-11-17-apple-design.md`

## Global Constraints

- Preserve configured colors/fonts, bar composition, reduced motion, and image cache limits.
- Add no dependencies or code comments. Preserve measured panel dimensions and Wayland guards.
- Work in the existing checkout so the uncommitted migration remains available. Do not commit unrelated user changes.

## Review Focus

- Rapid motion reversal retains position and velocity, with finite idle settling.
- Zero-duration animations resolve immediately without division by zero.
- User palettes remain the source of surface/text colors.
- Surface borders and controls fit existing measured window bounds.
- Focus, launcher keys, settings navigation, and slider release behavior remain functional.

### Task 1: Interruptible motion

**Files:** `crates/rune-ui/src/animation.rs`
**Interfaces:** Preserve `Animation<bool>` construction, duration, target, interpolation, and retarget methods. Internally sample position and velocity analytically using a critically damped spring.

- [ ] Add reversal-velocity and finite-settling tests; run and observe failure on the current tween.
- [ ] Replace smoothstep with a critically damped spring. Treat configured duration as response, settle within a bounded multiple of response, and preserve immediate zero-duration behavior.
- [ ] Run animation regression tests.

### Task 2: Shared appearance and controls

**Files:** `gpui_widgets.rs`, `config.rs`, `gpui_notch.rs`, `gpui_panels.rs`, `gpui_launcher.rs`, `gpui_settings.rs`, `gpui_overlays.rs`, `controls/text_input.rs`, `controls/slider.rs` under `crates/rune-ui/src/`; `examples/config.toml`.
**Interfaces:** Add `surface(&Config) -> Div` and `interactive(Stateful<Div>) -> Stateful<Div>` in the existing widgets module. Reuse palette and existing view models.

- [ ] Update default and example colors to graphite/blue and default font to sans-serif; retain serde/config override behavior.
- [ ] Add shared rounded surface edges and immediate hover/press/focus feedback, then apply across shell views.
- [ ] Replace settings navigation bullets with selected navigation surfaces; refine title weights, inputs, launcher rows, slider hit targets and thumb.
- [ ] Run workspace tests, Clippy, formatting, native UI tests, and release build. Inspect live panels on Wayland where available.

### Task 3: Review and evidence

- [ ] Request an independent read-only review of this redesign and address important findings.
- [ ] Record checks, visual evidence, and remaining limitations in the design verification document. Mark completed tasks here.

## Task 1: Motion

**Files:** `crates/rune-ui/src/animation.rs`, `crates/rune-ui/src/lib.rs`.
**Interface:** Preserve `Animation<bool>` methods; carry velocity internally.

- [x] Observe reversal and repeated-target regression tests fail.
- [x] Implement critically damped sampling, finite settling, and zero-duration handling.
- [x] Prove popup reduced-motion behavior across configuration updates.

## Task 2: Shared appearance

**Files:** Existing `gpui_widgets`, `gpui_notch`, `gpui_launcher`, `gpui_panels`, `gpui_overlays`, `gpui_settings`, `controls`, and config/example files.
**Interface:** `surface(&Config) -> Div`, `interactive(Stateful<Div>, gpui::Rgba) -> Stateful<Div>`.

- [x] Add graphite defaults, native sans-serif typography, and shared surface treatment.
- [x] Apply press/hover/selection feedback and tracked input focus styling.
- [x] Preserve accent text contrast and measured bounds. Native launcher measurement fails with layout border and passes with painted edge.

## Task 3: Verification and review

- [x] Independently review visual state handling and motion.
- [x] Run workspace tests, strict Clippy, and formatting checks.
- [x] Run final native suite, release build, and visual smoke check.
- [x] Record verification and remaining hardware limits.
