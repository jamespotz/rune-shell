# Focus timer and upcoming event widgets

## Intent

Add a focus countdown and a persistent calendar cue to the menu bar. Calendar
notifications continue to work independently of the upcoming-event widget.

## Agreed behavior

Two separate, optional widgets can be placed anywhere in the existing bar layout.
Both support the smooth and pixel looks, configured colors and fonts, reduced
motion, keyboard navigation, and Escape to close their panels.

The focus widget shows remaining time. Clicking opens focus, short-break, and
custom-countdown controls, with pause, resume, and stop. Completion posts a
notification. Optional Do Not Disturb restores the previous setting when the
session ends. There are no accounts or productivity statistics.

The upcoming-event widget uses existing calendar snapshots and meeting links.
It shows a truncated event title and time until the start, then indicates that
the event is happening now. Clicking opens the existing calendar panel on the
event's day, retaining its Join button. A small additional-event count indicates
overlaps. All-day events remain in the calendar and do not occupy this widget.

`auto_hide` defaults to `true`: show the widget 15 minutes before a timed event
through its end, then hide unless another event is within that window. With
`auto_hide = false`, show the current or next timed event, or "No upcoming
events" when none remain. Visibility is independent of reminder notifications,
including when calendar reminders are disabled.

## Approaches

Selected: separate focus and upcoming-event widgets. This allows independent
placement and styling through the existing widget configuration.

For upcoming events, an approaching-event indicator was selected over an
always-visible next-event indicator as the default. The latter remains available
by disabling auto-hide. Reusing the calendar panel avoids a second event browser.

## Proposed implementation defaults

These details are proposals for approval, rather than choices already made by
the user:

- Focus and short-break presets are 25 and 5 minutes. Custom durations are whole
  minutes from 1 to 1440. One timer runs at a time; starting another replaces it
  only through an explicit Start action. Breaks start manually.
- Focus has an idle icon when no timer runs. Timers do not survive daemon restarts.
  Pause freezes the remaining duration; elapsed time includes system suspend.
- Automatic Do Not Disturb is off by default. Restore it on stop or completion
  only if the user has not changed it during the session.
- An active event takes precedence over a future one. Ties use start time, end
  time, calendar order, then title for deterministic selection. The overlap count
  counts other timed events overlapping the selected event, not every future event.
- A zero-duration event remains eligible before its start and disappears at its
  start. End times are exclusive. Invalid end-before-start events are excluded.
- Settings expose upcoming-event auto-hide independently of reminder lead time.
  The 15-minute bar lead is fixed for the first version.

## Boundaries and constraints

Extend existing calendar helpers and widget/panel infrastructure. Reuse cached
calendar state; never fetch feeds or scan files during render. Keep timer state
and notifications in the existing service/action flow, with UI updates delivered
through asynchronous snapshots. Use monotonic elapsed-time accounting that
includes suspend for countdowns and local calendar timestamps for event selection.
Recompute event visibility on clock and snapshot changes, including after resume.

Keep titles bounded in layout and handle hostile calendar dates and durations
without panics. Rendering remains declarative. Do not add calendar editing,
automatic meeting joining, automatic breaks, or focus-history storage.

## Verification

Cover countdown start, pause, resume, stop, completion, replacement, suspend, and
Do Not Disturb ownership. Cover event selection before the visibility window, at
start and end, overlaps, all-day and malformed events, empty calendars, auto-hide
disabled, reminder notifications disabled, and clock changes. Inspect both looks
in a running Wayland session when available. Run the repository's required format,
Clippy, workspace test, and documentation checks after implementation.
