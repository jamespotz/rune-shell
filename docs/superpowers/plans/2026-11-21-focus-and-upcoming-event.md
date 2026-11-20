# Focus and upcoming event implementation plan

Implement the approved design in `../specs/2026-11-21-focus-and-upcoming-event-design.md`.

1. Add deterministic tests for countdown transitions, completion, suspend elapsed
   time, duration bounds, and DND ownership. Implement a Tokio timer service with
   snapshots and commands. Use Linux boot time to include suspend without wall-clock
   adjustments. Route DND requests through this service to preserve ownership.
2. Test and implement calendar event selection, visibility boundaries, overlap
   counts, and bounded labels using existing calendar snapshots.
3. Add optional `focus` and `upcoming_event` widgets and the focus panel. Reuse
   per-placement auto-hide, which already defaults to true. Open the calendar on
   the selected event's day. Wire the timer through existing service actions.
4. Add panel controls, settings integration, examples and README documentation.
   Inspect both looks when a Wayland display is available.
5. Run cargo fmt, strict workspace Clippy, workspace tests, and workspace docs.
   Report any unavailable native visual checks explicitly.
