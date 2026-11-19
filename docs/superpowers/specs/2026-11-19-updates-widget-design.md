# Updates widget

## Intent

Show at a glance whether the system has pending updates, and start a full
`topgrade` run from the bar. Topgrade only reports what it *would run*
(`--dry-run`), not what is out of date, so counts come from the package
managers themselves; topgrade is the "update everything" button.

Mockup: `.superpowers/brainstorm/7022-1795093479/content/updates-widget.html`
(approved, option A).

## Goals

- Bar widget `updates`: `system_update_alt` icon with a count badge; dimmed with
  no badge when nothing is pending; `sync` icon while topgrade runs (pulses in
  smooth, blinks in pixel, static under reduced motion; GPUI can't rotate glyphs); a `!` badge in the
  urgent color after a failed run.
- Panel `Panel::Updates` (also `rune-shell toggle updates`), 400 px wide like the
  System panel: header "Updates" + "checked N min ago", one row per source with
  its count or "up to date", then **Check now** and **Run topgrade** buttons, and
  an error line when the last check or run failed.
- Both looks through the `gpui_widgets` helpers (`corner`, `frame`, `heading`,
  `text_font`). Panel buttons use solid fills in both looks; the pixel stripe
  `fill` made labels unreadable.

## Non-goals

- Listing individual packages, per-step topgrade progress, or embedding a
  terminal in the panel.
- Counting updates for mise, cargo, rustup and other topgrade steps.
- A config section (interval, sources). The check interval is one hour, fixed.
- Adding `updates` to the default bar layout; it is opt-in like `stats`.

## Anti-goals

- Never block the UI thread or a Tokio worker on a check.
- Never run anything as root or prompt for a password outside the terminal.
- A hung or hostile command must not grow memory or freeze the widget.

## Approach

Picked: **A — real counts per source** (user's choice). Rejected: **B — "last
updated N days ago"** only, which can't tell whether anything is waiting.

## Design

**Service** `rune_services::updates`, same shape as `stats`/`weather`:
`spawn()` returns a `watch::Receiver<Updates>` forwarded as `UiEvent::Updates`;
commands arrive as `Action::Updates(updates::Command)` with `Check` and
`Run { terminal: String }`.

Snapshot:

```rust
pub struct Updates {
    pub sources: Vec<Source>,          // only sources whose tool is installed
    pub checked_at: Option<SystemTime>,
    pub checking: bool,
    pub run: Run,                      // Idle | Running | Failed(String)
    pub topgrade_installed: bool,
}
pub struct Source { pub kind: SourceKind, pub pending: Result<u32, String> }
pub enum SourceKind { Dnf, Flatpak, Firmware }
```

Checks (via `tokio::process`, each with a 3-minute timeout and stdout capped at
1 MiB; sources run concurrently):

| Source   | Command                                               | Count |
|----------|-------------------------------------------------------|-------|
| Dnf      | `dnf check-update -q` with `NO_COLOR=1`               | package lines (`name.arch version repo`), ANSI stripped; exit 0 = none, 100 = updates, other = error |
| Flatpak  | `flatpak remote-ls --updates --columns=application`   | non-empty lines |
| Firmware | `fwupdmgr get-updates --json`                         | devices with releases; its "nothing to do" exit is 0 pending, not an error |

A missing binary hides that source. The first check runs 30 s after startup,
then hourly, and on `Check` (ignored while one is already running).

**Run:** `Command::spawn` of `<launcher.terminal> -e topgrade --keep` (so the
output stays readable until a key press). Ignored while already running. A
background task waits on the child; a non-zero status sets
`Run::Failed("topgrade exited with status N")` (terminals pass the child's exit
status through), otherwise `Idle`; either way a check follows.

**UI:** `Widget::Updates` in `config.rs` (`ALL`, label "Updates", settings icon
`system_update_alt`), `widget_panel` → `Panel::Updates`, `NAMED` gains
`("updates", Panel::Updates)`. Rendering lives next to the System panel in
`gpui_panels.rs`; "N min ago" formatting and the badge total are plain
functions with unit tests.

## Left to the builder

- Exact row icons and spacing, matched against the System panel in both looks.
- Parsing details for each tool's output, covered by tests on captured samples
  plus hostile input (huge output, garbage lines, overflow-sized counts).
