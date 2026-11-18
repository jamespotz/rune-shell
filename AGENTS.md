# Agent rules

## Rust

- Write idiomatic Rust with descriptive names and focused functions.
- Production code handles failures through `Result`, `?`, or explicit recovery. Values from outside reach no panicking path: use `get` for indexing, `checked_*` for arithmetic, and `try_from` for casts, never `unwrap()`, `expect()`, panicking operators such as `NaiveDate + Days` or `Instant - Duration`, or wrapping `as` casts.
- A poisoned lock or a closed channel in a service loop is logged with `warn!` before the loop exits or restarts through its supervisor.
- Treat ICS feeds, D-Bus messages, tray data, and control-socket input as untrusted: bound sizes and counts, and test with hostile values.
- `rune-shell` uses `anyhow` with `.context(..)`. Service errors name what failed (the path, command, or value) alongside the underlying error.
- Avoid unnecessary clones, allocations, and per-frame heap churn.
- Comments carry only what the code cannot show: `// SAFETY:` on every `unsafe` block, protocol or spec quirks, and `shortcut:` markers. Put architectural rationale in the relevant documentation. Delete dead code; add TODOs only when requested.
- Lints live in `[workspace.lints]` and shared dependency versions in `[workspace.dependencies]`. Silence a lint with `#[expect(clippy::name, reason = "...")]` on the narrowest item.
- Preserve existing user changes and avoid unrelated refactors.

## Architecture and runtime separation

- Extend existing modules first. Add a module or crate only for a clear architectural boundary. Keep related functionality together; prefer composition and introduce traits only when multiple implementations are expected.
- Keep application state in the shell entity and domain models. View entities own interaction state such as focus, selection, and active drags.
- Never block the UI thread. Run disk access, decoding, indexing, and blocking work in background tasks.
- Keep Tokio workers free too: async code reaches files through `tokio::fs` and runs CPU-heavy work such as image decoding in `spawn_blocking`. Hold locks and `watch::Ref` guards only for quick reads and writes: no heavy work, callbacks, or `.await` under them.
- GPUI workers do not have a Tokio reactor. D-Bus, `zbus` connections/proxies, Tokio sockets, and IPC monitoring loops belong on the Tokio runtime.
- The daemon creates and enters its Tokio runtime in `crates/rune-shell/src/main.rs`. Dispatch service actions through the existing action channel. Do not assume that runtime context propagates into `cx.spawn` or GPUI background tasks.
- Bridge service state through asynchronous channels and apply it using GPUI entity/window updates. Existing service snapshots use `watch`; UI actions/events use unbounded `mpsc`, which suits their low rate. Give any high-rate producer a bounded channel. Use `broadcast` only when fan-out is needed.
- GPUI UI timers use `cx.background_executor().timer(duration).await`. Tokio service timers use `tokio::time`.
- Avoid nested runtime bridges and task locks for immediate process launches. Use `Command::spawn` in the existing service action path; put process waits and potentially blocking setup in background work.
- Hyprland commands currently use async Tokio sockets. Keep them on Tokio through the action channel; do not block on them or call an async socket API as if it were synchronous.

## GPUI and Wayland

- Keep `render()` declarative. Business actions, task spawning, focus changes, notifications, and surface synchronization belong in event handlers or lifecycle/update paths.
- Never call `window.focus()`, `window.activate_window()`, `window.refresh()`, or `cx.notify()` directly during render. Request focus when opening a window or explicitly changing modes.
- Cache and compare desired Wayland state before applying surface setters. Keyboard interactivity, input regions, and exclusive zones can commit surfaces and cause configure/redraw loops.
- Give interactive elements unique, stable IDs, including elements built in loops. Prefer domain keys over positions when lists can reorder.
- Keep intrinsic measurements and requested panel dimensions consistent. Prevent layout oscillation and preserve the initial tiny-window intrinsic sizing workaround.
- Never send zero or negative `wp_viewport` destinations. Preserve the vendored GPUI guard; zero layer-shell stretch requests have different semantics.
- Synchronize window bounds and regions only for explicit state, output, or size changes. Do not add frame-rate window synchronization polling.
- Reuse GPUI components and builder styles. Avoid boxed element collections in hot render paths when declarative children suffice.
- Normalize compositor rates above 1000 from millihertz to hertz, clamp finite rates to 30–360 Hz, and handle invalid rates with a safe fallback. Frame timers must have a minimum duration of 2 ms.

## Interaction and appearance

- Follow the apple-design principles for immediate press feedback, continuous direct manipulation, interruptible motion, predictable spatial relationships, and clear hierarchy.
- Preserve pointer position and velocity when retargeting interactive motion. Use restrained, critically damped springs by default; reserve overshoot for momentum-driven gestures.
- Every feature ships in both looks, `smooth` and `pixel` (`bar.look`). Style through the look helpers in `gpui_widgets` (`corner`, `fill`, `frame`, `heading`, `text_font`, `pixel_look`), and inspect the running UI in each look.
- Respect reduced motion, user colors, fonts, and accessibility preferences. Maintain legible contrast, visible focus, and keyboard escape/navigation paths.
- Use native GPUI builder patterns, not CSS. The current icon system uses the configured Material Symbols font plus indexed application/tray images. Reuse it rather than assuming an assets crate or SVG directory exists.
- Keep decoded image bounds and shared-cache retirement intact. Avoid full-resolution images for icons and thumbnails.

## Services and integration

- Search, modal opening, and other UI interactions must not synchronously scan desktop entries or icon directories. Maintain indexes through background watchers and cached snapshots.
- Monitoring loops must recover from transient failures and lagged/closed subscriptions with reconnection and bounded backoff. Use separate periodic verification where event streams can miss state.
- Keep socket line readers independent of timeout-based fallback polling. Preserve partial reads by using a continuous read loop rather than repeatedly cancelling and recreating readers.
- Workspace slots remain clickable even when empty. Map each visible slot to its configured workspace number, using the slot index plus one only when an explicit number is absent.
- Use the modern Hyprland dispatch syntax implemented by the service. If legacy compatibility is added, fall back only on a rejected/unsupported response and test both forms against the supported protocol; do not assume version-specific syntax from the template is universal.
- Check the exit status of every spawned command whose result is used, and include the command and status in the error.
- Write text clipboard selections through `cx.write_to_clipboard(gpui::ClipboardItem::new_string(...))` alongside daemon integration. Validate decoded output before replacing the clipboard. Use explicit MIME types for binary content and avoid shell pipelines that hide decode failures.
- `rune-shell quit` returns only after the daemon has exited, so `quit && rune-shell` restarts cleanly. Keep that contract; read the shutdown paragraph under Data flow in `docs/ARCHITECTURE.md` before changing IPC or shutdown.

## Verification

- Before finishing, run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo doc --workspace --no-deps`, and leave each clean.
- Fix a bug by first writing a test that fails on it.
- Check behavior at the changed boundary and report what was actually verified. Run native GPUI tests individually when they require a Wayland session.
- For visual work, inspect the running UI when the display is available. Report unavailable visual or hardware checks explicitly.
- Measure performance changes before claiming an improvement. Distinguish resident RAM/PSS from GPU allocations and document measurement limits.
- Keep patches to `vendor/gpui_linux` narrow and document them in its Rune README. Preserve the pinned upstream revision and required licenses.
