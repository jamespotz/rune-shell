# Updates Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A bar widget + panel that counts pending dnf/flatpak/firmware updates and runs `topgrade` in the user's terminal.

**Architecture:** New Tokio service `rune_services::updates` shaped like `bluetooth` (`spawn() -> (watch::Receiver<Updates>, UnboundedSender<Command>)`), forwarded as `UiEvent::Updates`, driven by `Action::Updates`. UI adds `Widget::Updates` and `Panel::Updates`, rendered next to the System panel.

**Tech Stack:** Rust, tokio (`process`, `time`), serde_json (fwupd), GPUI.

**Spec:** `docs/superpowers/specs/2026-11-19-updates-widget-design.md`

## Global Constraints

- Check interval 1 h, first check 30 s after start; per-command timeout 3 min; stdout capped at 1 MiB.
- Run command: `<launcher.terminal> -e topgrade --keep`; failure copy `topgrade exited with status N`.
- Missing binary hides that source; no root, no config section, widget not in default layout.
- Both looks via `gpui_widgets` helpers; panel width 400 px.

## Review Focus

- Clicking Run twice → one terminal only (test: `Run` while running is ignored).
- dnf output with ANSI colors, "Obsoleting packages" section, or metadata lines → only package lines count (test in Task 1).
- A tool that prints endlessly or hangs → capped/timed out, row shows an error, other rows still update (test in Task 2).
- Terminal binary missing → `Run::Failed` names the command, no panic (test in Task 2).
- fwupd "no updates" non-zero exit → 0 pending, not an error (test in Task 1).

---

### Task 1: Output parsers

**Files:** Create `crates/rune-services/src/updates.rs`; modify `crates/rune-services/src/lib.rs` (`pub mod updates;`).

**Produces:** `fn dnf_pending(stdout: &str) -> u32`, `fn flatpak_pending(stdout: &str) -> u32`, `fn fwupd_pending(json: &str) -> Result<u32, String>`.

- [ ] Tests: dnf sample with ANSI escapes and an `Obsoleting packages` block counts only the lines above it shaped `name.arch version repo`; empty → 0; flatpak counts non-empty trimmed lines; fwupd `{"Devices":[]}` → 0, one device with non-empty `Releases` → 1, garbage → `Err`. Counts saturate at `u32::MAX`.
- [ ] Run `cargo test -p rune-services updates` → fail; implement; pass.

### Task 2: Service loop

**Files:** `crates/rune-services/src/updates.rs`.

**Produces:**
```rust
pub enum Command { Check, Run { terminal: String } }
pub struct Updates { pub sources: Vec<Source>, pub checked_at: Option<SystemTime>,
                     pub checking: bool, pub run: Run, pub topgrade_installed: bool }
pub struct Source { pub kind: SourceKind, pub pending: Result<u32, String> }
pub enum SourceKind { Dnf, Flatpak, Firmware }
pub enum Run { Idle, Running, Failed(String) }
pub fn spawn() -> (watch::Receiver<Updates>, mpsc::UnboundedSender<Command>);
async fn capture(program: &str, args: &[&str], limit: usize, timeout: Duration) -> Result<(ExitStatus, String), String>;
```
`capture` reads stdout through `take(limit)`, kills on timeout (`kill_on_drop`), sets `NO_COLOR=1`; `Err(NotFound)` maps to "hide this source". dnf: exit 0 → 0, 100 → parse, other → `Err("dnf check-update exited with status N")`. Installed check: `which`-style search of `PATH` (no spawn). Run: `Command::spawn`, wait in a task, then `Check`.

- [ ] Tests (tokio): `capture("sh", ["-c","yes"], 1024, 5s)` returns ≤ 1024 bytes; `capture("sleep", ["5"], _, 100ms)` → `Err` mentioning timeout; `capture("definitely-missing-binary", ...)` → not-found; a run with terminal `definitely-missing-terminal` → `Run::Failed` containing that name; second `Run` while `Running` spawns nothing (use `sh -c "sleep 1"` as terminal stand-in via a `run_command` seam taking program+args).
- [ ] Implement, pass, `cargo clippy -p rune-services --all-targets -- -D warnings`.

### Task 3: Daemon wiring

**Files:** `crates/rune-ui/src/protocol.rs` (`Action::Updates(updates::Command)`, `UiEvent::Updates(updates::Updates)`), `crates/rune-shell/src/main.rs` (`let (updates_state, updates_cmds) = updates::spawn(); forward(updates_state, ui.clone(), UiEvent::Updates);` and the dispatch arm), `crates/rune-ui/src/state.rs` (`pub updates: Updates`), `crates/rune-ui/src/lib.rs` (store it; add to `reshapes`).

- [ ] `cargo build --workspace` clean.

### Task 4: Widget and panel

**Files:** `config.rs` (`Widget::Updates`, `ALL` → 17, label "Updates"), `gpui_settings.rs` (icon `system_update_alt`), `notch.rs` (`Widget::Updates => Panel::Updates` + test), `panel.rs` (`Panel::Updates`, `NAMED` gains `("updates", Panel::Updates)`), `system_panel.rs` (`fn ago(since: Duration) -> String`, `fn pending_total(&Updates) -> u32`), `gpui_widgets.rs` (bar item), `gpui_panels.rs` (`fn updates(app, dispatch)`, width `SYSTEM_W`), `lib.rs` (`Message::Updates(updates::Command)` → `self.send(Action::Updates(..))`; opening the panel triggers nothing), `examples/config.toml` + README widget list.

- [ ] Tests: `ago(30s)`="just now", `ago(4 min)`="4 min ago", `ago(3 h)`="3 h ago", `ago(2 d)`="2 d ago"; `pending_total` sums `Ok` counts and ignores `Err`, saturating; `widget_panel(Widget::Updates) == Some(Panel::Updates)`; `Panel::from_name("updates")`.
- [ ] Bar: icon `system_update_alt` + badge when total > 0, muted when 0; `sync` while running (rotation only when motion allowed; pixel blinks); urgent `!` badge on `Run::Failed`.
- [ ] Panel: title row "Updates" + ago; rows per source (`deployed_code` System (dnf), `apps` Flatpak, `memory` Firmware) with count / "up to date" / error text; buttons Check now / Run topgrade (disabled while checking/running or topgrade missing → "topgrade not found").
- [ ] Full verification: `cargo fmt --all`, clippy `-D warnings`, `cargo test --workspace`, `cargo doc --workspace --no-deps`; run the shell and inspect both looks.
