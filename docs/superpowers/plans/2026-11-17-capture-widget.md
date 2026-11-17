# Capture Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `capture` bar widget that opens a Screenshot / Record / Text panel, plus keybind hints for every recording target.

**Architecture:** The capture backend (`rune-services/src/capture.rs`, `Action::Screenshot/Ocr/Record`) is unchanged. The UI gains `Panel::Capture`, `Widget::Capture`, a small `capture_panel` logic module (modes, targets, shortcut table), and one new backend effect, `Effects::later`. That effect sends a message after the panel has closed, so the panel stays out of the shot.

**Tech Stack:** Rust, GPUI (vendored), Tokio, Hyprland, grim/slurp/gpu-screen-recorder/tesseract.

**Spec:** `docs/superpowers/specs/2026-11-17-capture-widget-design.md`

## Global Constraints

- Follow `AGENTS.md`: no `unwrap`/`expect` in production code, nothing done in `render()`, stable element IDs keyed by domain (mode + target), no UI-thread blocking.
- The config name of the widget is `capture`. Its CLI panel name is `capture` (`rune-shell toggle capture`).
- Screenshot targets: Area, Window, Screen, All. Record targets: Area, Window, Screen. Text: none (it starts an area OCR directly).
- New hint lines: `SUPER + SHIFT + Print` → `record window`, `SUPER + CTRL + Print` → `record screen`, `SUPER + SHIFT + R` → `toggle capture`.
- Settle delay before a capture started from the panel: `150 ms`.
- Looks come only from the existing helpers `corner`, `fill`, `material`, `text_font`, `surface`.
- Before finishing, run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo doc --workspace --no-deps`, and leave each clean.

## Review Focus

1. **Window mode with no focused window.** The service returns `"no focused window"`, and today a screenshot only logs it with `warn!`. Expect no crash and the panel already closed. This is existing behavior: leave it and note it in the final report.
2. **A recording starts or stops while the panel is open.** The Record tiles must switch to and from "Stop recording" on the next snapshot. Render from `snapshot.recording` and don't cache it (Task 4 test).
3. **The widget placed twice, or in the center section.** Each slot gets its own ID from the existing `section-{s}-widget-{i}` scheme. Both slots show the timer while recording, and the fallback chip does not also appear (Task 3 test).
4. **The panel closed during the settle delay** (Esc, or another panel opened). The capture still runs once. The delayed message must not reopen anything (Task 2 test).
5. **Text mode chosen from the switch.** It starts OCR right away and does not leave `capture_mode` as `Text` for the next opening (Task 2 test).

---

### Task 1: Target words and the capture panel model

**Files:**
- Modify: `crates/rune-services/src/capture.rs` (impl `Target`)
- Create: `crates/rune-ui/src/capture_panel.rs`
- Modify: `crates/rune-ui/src/lib.rs` (add `mod capture_panel;`)
- Modify: `crates/rune-ui/src/gpui_settings.rs:1588-1600` (Shortcuts group)

**Interfaces:**
- Produces:
  - `capture::Target::word(self) -> &'static str`, returning `"selection" | "window" | "screen" | "all"`.
  - `capture_panel::CaptureMode { Screenshot, Record, Text }` (`Debug, Clone, Copy, PartialEq, Eq, Default`, default `Screenshot`).
  - `capture_panel::targets(mode: CaptureMode) -> &'static [Target]`
  - `capture_panel::SHORTCUTS: [(&'static str, &'static str); 8]`, as (keys, CLI args) in display order: the existing 5 lines plus the 3 new ones, with `toggle capture` second to last and `ocr` last.
  - `capture_panel::shortcut(mode: CaptureMode, target: Target) -> Option<&'static str>`: the keys whose args equal `"{screenshot|record} {target.word()}"`, or `"ocr"` for Text.
  - `capture_panel::CAPTURE_W: f32 = 372.0`, `capture_panel::SETTLE: Duration = 150 ms`.

- [ ] **Step 1: Write failing tests**

```rust
// rune-services capture.rs tests
#[test]
fn target_words_round_trip() {
    for t in [Target::Screen, Target::All, Target::Window, Target::Selection] {
        assert_eq!(Target::from_word(t.word()), Some(t));
    }
}
// rune-ui capture_panel.rs tests
#[test]
fn record_has_no_all_and_text_has_no_targets() {
    assert_eq!(targets(CaptureMode::Screenshot), [Target::Selection, Target::Window, Target::Screen, Target::All]);
    assert_eq!(targets(CaptureMode::Record), [Target::Selection, Target::Window, Target::Screen]);
    assert!(targets(CaptureMode::Text).is_empty());
}
#[test]
fn tiles_show_the_settings_shortcuts() {
    assert_eq!(shortcut(CaptureMode::Screenshot, Target::Selection), Some("Print"));
    assert_eq!(shortcut(CaptureMode::Record, Target::Window), Some("SUPER + SHIFT + Print"));
    assert_eq!(shortcut(CaptureMode::Record, Target::Screen), Some("SUPER + CTRL + Print"));
    assert_eq!(shortcut(CaptureMode::Screenshot, Target::All), None);
    assert!(SHORTCUTS.contains(&("SUPER + SHIFT + R", "toggle capture")));
}
```

- [ ] **Step 2: Run them and confirm they fail.** Run `cargo test -p rune-services target_words && cargo test -p rune-ui capture_panel`. Expected: compile errors (`word`, `capture_panel` missing).
- [ ] **Step 3: Implement** the items above. In `gpui_settings.rs`, build the Shortcuts group from `SHORTCUTS` with `format!("hl.bind(\"{keys}\", hl.dsp.exec_cmd(\"rune-shell {args}\"))")` in place of the literal list.
- [ ] **Step 4: Run the tests.** Same command; expected PASS.
- [ ] **Step 5: Commit** `feat: capture panel model and record window/screen keybind hints`

### Task 2: Delayed effect and capture messages

**Files:**
- Modify: `crates/rune-ui/src/backend.rs` (`Effect`, `Effects`, the `dispatch` loop)
- Modify: `crates/rune-ui/src/lib.rs` (`Message`, `App`, `update`; replace the `QuickScreenshot` body)

**Interfaces:**
- Consumes: Task 1's `CaptureMode`, `SETTLE`.
- Produces:
  - `Effects::later(after: Duration, message: Message) -> Effects<Message>`. The backend runs it as `cx.spawn` → `cx.background_executor().timer(after).await` → `cx.update(|cx| dispatch(shell, backend, message, cx))`.
  - `App.capture_mode: CaptureMode` (pub, default `Screenshot`).
  - `Message::CaptureMode(CaptureMode)`: sets the mode. For `Text`, it behaves like `Message::Capture(Text, Selection)` and leaves `capture_mode` unchanged.
  - `Message::Capture(CaptureMode, Target)`: closes the panel and chains `Effects::later(SETTLE, Message::RunCapture(..))`.
  - `Message::RunCapture(CaptureMode, Target)`: sends `Action::Screenshot(t, opts)`, `Action::Record(RecordCommand::Start(t, opts))`, or `Action::Ocr(opts)`, with `opts = capture_options(&self.config)`. It touches no panel state.
  - `Message::QuickScreenshot` becomes `self.update(Message::Capture(Screenshot, Selection))`.

- [ ] **Step 1: Write failing tests** in `lib.rs` tests, using `test_app()`:

```rust
#[test]
fn panel_capture_waits_for_the_panel_to_close() {
    let (mut app, mut rx) = test_app();
    app.update(Message::Toggle(Panel::Capture));
    app.update(Message::Capture(CaptureMode::Screenshot, capture::Target::Screen));
    assert_eq!(app.panels.current(), None);
    assert!(rx.try_recv().is_err());
    app.update(Message::RunCapture(CaptureMode::Screenshot, capture::Target::Screen));
    assert!(matches!(rx.try_recv(), Ok(Action::Screenshot(capture::Target::Screen, _))));
    assert_eq!(app.panels.current(), None);
}
#[test]
fn text_mode_starts_ocr_and_keeps_the_previous_mode() {
    let (mut app, _rx) = test_app();
    app.update(Message::CaptureMode(CaptureMode::Record));
    app.update(Message::CaptureMode(CaptureMode::Text));
    assert_eq!(app.capture_mode, CaptureMode::Record);
}
#[test]
fn record_run_starts_not_toggles() {
    let (mut app, mut rx) = test_app();
    app.update(Message::RunCapture(CaptureMode::Record, capture::Target::Window));
    assert!(matches!(rx.try_recv(), Ok(Action::Record(capture::RecordCommand::Start(capture::Target::Window, _)))));
}
```

(`Panel::Capture` comes from Task 3. If you do this task first, add the variant now and register it in Task 3.)

- [ ] **Step 2: Run them and confirm they fail.** Run `cargo test -p rune-ui capture`; expected compile errors.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests.** Same command; expected PASS.
- [ ] **Step 5: Commit** `feat: delayed effect so panel captures miss the panel`

### Task 3: Panel and widget registration

**Files:**
- Modify: `crates/rune-ui/src/panel.rs` (`Panel::Capture`, `NAMED` grows to 14 entries with `("capture", Panel::Capture)`)
- Modify: `crates/rune-ui/src/config.rs` (`Widget::Capture`, `ALL` grows to 16, `Display` → `"Capture"`)
- Modify: `crates/rune-ui/src/notch.rs` (`widget_panel`: `Widget::Capture => Panel::Capture`; new `bar_has(bar: &config::BarConfig, widget: Widget) -> bool`)
- Modify: the exhaustive `match self.panel` arms in `lib.rs:1102-1135` and `gpui_panels.rs` (a placeholder body is fine until Task 4)

Check the real name of the bar config struct in `config.rs:132` before writing `bar_has`.

- [ ] **Step 1: Write failing tests**

```rust
// panel.rs
#[test] fn capture_is_toggleable_by_name() { assert_eq!(Panel::from_name("capture"), Some(Panel::Capture)); }
// config.rs
#[test] fn capture_widget_parses() {
    let c = Config::parse("[bar]\nright = [\"capture\"]").unwrap();
    assert_eq!(c.bar.right, [Widget::Capture]);
}
// notch.rs
#[test] fn capture_widget_opens_capture_panel_and_is_found_in_any_section() {
    assert_eq!(widget_panel(Widget::Capture), Some(Panel::Capture));
    let mut bar = crate::Config::default().bar;
    assert!(!bar_has(&bar, Widget::Capture));
    bar.center.push(Widget::Capture);
    assert!(bar_has(&bar, Widget::Capture));
}
```

- [ ] **Step 2: Confirm they fail** with `cargo test -p rune-ui capture`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests.** Expected PASS. Also run `cargo test -p rune-shell` (the IPC `toggle` path goes through `Panel::from_name`, so `toggle capture` needs no IPC change).
- [ ] **Step 5: Commit** `feat: register capture panel and widget`

### Task 4: Render the widget, timer and panel

**Files:**
- Modify: `crates/rune-ui/src/gpui_widgets.rs` (`bar_widget` arm `Widget::Capture`; extract `recording_chip(app, id, dispatch) -> AnyElement` from the notch's inline timer)
- Modify: `crates/rune-ui/src/gpui_notch.rs:200-211` (show the fallback chip only when `!notch::bar_has(bar, Widget::Capture)`; use `recording_chip`)
- Modify: `crates/rune-ui/src/gpui_panels.rs` (`Panel::Capture => capture(app, &self.dispatch)`, width `capture_panel::CAPTURE_W`; add `"capture"` to the ignored `populated_and_empty_feature_panels_render` list, plus a recording variant if that list supports named variants)

**Interfaces:**
- Consumes: Tasks 1–3.
- Widget: `snapshot.recording.is_some()` → `recording_chip` (click → `Message::StopRecording`). Otherwise a `bar_button` with glyph `photo_camera` that toggles `Panel::Capture`.
- Panel body `fn capture(app: &Shell, dispatch: &Dispatch) -> AnyElement`:
  - Segmented row: three items (`screenshot_region` Screenshot, `videocam` Record, `document_scanner` Text) → `Message::CaptureMode(m)`. The active item uses the accent fill in pixel look and a tinted fill in glass look.
  - Tiles: `targets(app.capture_mode)` → icon (`crop_free` / `web_asset` / `desktop_windows` / `select_all`), label (Area / Window / Screen / All) and `shortcut(..)` in muted text (`—` when `None`). Click → `Message::Capture(mode, target)`. ID: `format!("capture-{mode:?}-{}", target.word())`.
  - In Record mode while `snapshot.recording.is_some()`: one "Stop recording" tile → `Message::StopRecording`.
  - Footer: in Screenshot mode, the "Copy to clipboard" toggle → `Message::Settings(settings::Msg::Flag(Flag::CaptureCopy, !c.capture.copy))`. In Record mode, "Record audio" → `Flag::RecordAudio`. A folder link → `Message::OpenLink(capture_options(c).{screenshot_dir|record_dir}.display().to_string())`.
- Press feedback through the existing `clickable`/`button` helpers. Don't add new animation code.

- [ ] **Step 1:** Run `cargo build -p rune-ui`, then the ignored native tests one at a time in a Wayland session:
  `cargo test -p rune-ui all_widgets_and_repeated_widgets_render -- --ignored` and
  `cargo test -p rune-ui populated_and_empty_feature_panels_render -- --ignored`. Expected PASS.
- [ ] **Step 2: Visual check.** Run the shell with `bar.right` containing `capture`, then in both `look = "glass"` and `"pixel"`: open the panel, switch modes, take a Screen screenshot (confirm the panel isn't in the PNG), start and stop a Window recording from the panel and the timer, run Text OCR, and check the folder link. If any icon renders as blank or as text, swap it for a Material Symbols name that does render. Report anything you couldn't check.
- [ ] **Step 3: Commit** `feat: capture widget and panel UI`

### Task 5: Docs and final verification

**Files:**
- Modify: `README.md:53-57` (mention the `capture` widget and `toggle capture`)
- Modify: `examples/config.toml` (list `capture` in the widget comment, if the file enumerates widgets)
- Modify: `crates/rune-ui/src/gpui_settings.rs` Capture note: "Add the `capture` widget in Menu Bar to use these with the mouse."

- [ ] **Step 1:** Run `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo doc --workspace --no-deps`. Expected: everything clean.
- [ ] **Step 2: Commit** `docs: capture widget`
