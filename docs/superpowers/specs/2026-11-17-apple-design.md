# Rune appearance and interaction redesign

## Intent

Apply the requested Apple design principles to the native GPUI desktop shell, including its overall appearance. The result should feel calm, readable, and immediately responsive while preserving Rune's workflows, configurable bar composition, and service architecture.

## Approaches

1. Refine the existing appearance. This minimizes visual change but retains the current flat panels and text-like controls. The user requested an overall redesign instead.
2. Redesign the shared visual language within GPUI. Recommended: layered graphite surfaces, deliberate type hierarchy, consistent controls, and restrained motion. Existing panels gain a coherent appearance without replacing their domain models or adding rendering dependencies.
3. Rebuild around elaborate glass effects and gesture-driven sheets. This adds compositor and GPU costs, and new interaction conventions that are unnecessary for a pointer-first shell. Excluded from this proposal.

Implementation uses the second approach. The visual choices below apply the requested overall redesign within the existing GPUI architecture.

## Visual design

- Use neutral graphite defaults, a clear blue accent, near-white primary text, and readable secondary text. User-configured colors continue to override defaults; update the example config alongside defaults.
- Use the installed Noto Sans family as the Linux sans-serif default. Preserve the configurable text and icon font families. Use weight with size to distinguish titles, row labels, and secondary information.
- Establish shared GPUI surface and control styling in the existing widgets module. Floating panels use rounded corners and a subtle defined edge; settings use a darker structural sidebar and a distinct content region. Avoid adding clipping-prone outer shadows to tightly sized layer windows.
- Distinguish selection, hover, press, disabled, destructive, and keyboard focus states. Buttons highlight on pointer-down and commit on release. Keep compact bar targets within the configured bar height; give panel controls larger hit areas.
- Give launcher results clear title/subtitle hierarchy and consistent selected/hovered/pressed states. Make settings navigation visibly indicate its selected page through shape and color rather than a text bullet.
- Apply the same visual language to feature panels, notification cards, OSDs, input fields, and sliders. Preserve existing measured dimensions unless a corresponding layout calculation is updated.
- Use existing compositor blur support when enabled. Do not add new full-screen effects or large image resources. Keep structural surfaces sufficiently opaque for readability without blur.

## Interaction and motion

- Replace the boolean smoothstep tween with an interruptible critically damped spring that retains presentation position and velocity when its target changes. Preserve the configured timing control and finite settling so idle rendering stops.
- Reduced motion and disabled animation settle immediately. Do not introduce mandatory movement or bounce, input locks, periodic surface commits, or animations that keep the shell awake at rest.
- Preserve continuous slider updates and release-based seek commits. Improve thumb/track legibility and hit area while preserving out-of-bounds drag tracking.
- Retain keyboard navigation, text editing, password masking, Escape dismissal, and focus transfer. Do not move focus during render.

## Scope and constraints

Use existing crates/modules, GPUI builder patterns, and the current service/action boundary. No service rewrites, new dependencies, replacement icon pipeline, audio/haptic feedback, custom fonts, or new drag-to-dismiss gestures. Keep the image memory fixes and Wayland viewport guard intact. Do not refactor unrelated existing render side effects in this design pass.

## Acceptance

- Shared styling reaches the bar, launcher, settings, feature panels, notifications, and OSD without removing features or ignoring configured colors/fonts.
- Controls provide immediate press feedback and visible selected/focused states where supported.
- Spring regression tests prove position and velocity continuity on reversal, finite settling, and immediate reduced-motion behavior.
- Formatting, workspace tests, and Clippy pass. Run relevant native tests individually and build the release executable.
- Inspect the running shell on the available Wayland display for clipping, readability, selected states, and panel sizing. Report limits of single-display/manual verification.
