//! GPUI application lifecycle and event-driven delivery from the daemon.

use crate::UiEvent;
use gpui::{App, Context, Entity};
use tokio::sync::mpsc::UnboundedReceiver;

/// Keeps shell state alive for the application's lifetime. Tokio's channel
/// receiver supplies its own wakeups, so waiting here needs no timer or polling.
pub(crate) fn run<T: 'static>(
    events: UnboundedReceiver<UiEvent>,
    initialize: impl FnOnce(&mut App) -> Entity<T> + 'static,
    mut dispatch: impl FnMut(&mut T, UiEvent, &mut Context<T>) + 'static,
) {
    gpui_platform::application()
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx| {
            crate::controls::text_input::TextInput::bind_keys(cx);
            let state = initialize(cx);
            cx.spawn(async move |cx| {
                deliver(events, |event| {
                    state.update(cx, |state, cx| {
                        dispatch(state, event, cx);
                        cx.notify();
                    });
                })
                .await;
                // Closing the daemon channel is equivalent to an explicit quit.
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

async fn deliver(mut events: UnboundedReceiver<UiEvent>, mut dispatch: impl FnMut(UiEvent)) {
    while let Some(event) = events.recv().await {
        if matches!(event, UiEvent::Quit) {
            break;
        }
        dispatch(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn quit_stops_before_later_buffered_events() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(UiEvent::Status("before".into())).unwrap();
        tx.send(UiEvent::Quit).unwrap();
        tx.send(UiEvent::Status("after".into())).unwrap();
        let seen = RefCell::new(Vec::new());
        futures::executor::block_on(deliver(rx, |event| {
            if let UiEvent::Status(text) = event {
                seen.borrow_mut().push(text);
            }
        }));
        assert_eq!(*seen.borrow(), ["before"]);
        assert!(tx.is_closed());
    }

    #[test]
    fn channel_closure_delivers_pending_events_then_returns() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(UiEvent::Status("pending".into())).unwrap();
        drop(tx);
        let mut seen = Vec::new();
        futures::executor::block_on(deliver(rx, |event| {
            if let UiEvent::Status(text) = event {
                seen.push(text);
            }
        }));
        assert_eq!(seen, ["pending"]);
    }
    #[test]
    #[ignore = "opens a GPUI window; requires a running Wayland compositor"]
    fn wayland_window_lifecycle() {
        use crate::{
            Surface,
            gpui_surfaces::{Windows, options},
        };
        use gpui::{AppContext, Context, IntoElement, Render, Window, div};
        struct View;
        impl Render for View {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                use gpui::prelude::*;
                div()
                    .size_full()
                    .bg(gpui::rgb(0x181825))
                    .text_color(gpui::white())
                    .child("Rune GPUI migration lifecycle check")
            }
        }
        struct State {
            windows: Windows,
            transitions: usize,
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = std::thread::spawn(move || {
            for status in [
                "open",
                "duplicate",
                "resize",
                "external-close",
                "reopen",
                "close",
            ] {
                tx.send(UiEvent::Status(status.into())).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            tx.send(UiEvent::Quit).unwrap();
        });
        let completed = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = completed.clone();
        run(
            rx,
            |cx| {
                cx.new(|_| State {
                    windows: Windows::default(),
                    transitions: 0,
                })
            },
            move |state, event, cx| {
                let UiEvent::Status(status) = event else {
                    return;
                };
                match status.as_str() {
                    "open" | "duplicate" | "reopen" => state
                        .windows
                        .open(
                            Surface::Settings,
                            options(&Surface::Settings, (500, 160), None, false),
                            cx,
                            |_, cx| cx.new(|_| View),
                        )
                        .unwrap(),
                    "resize" => state.windows.resize(&Surface::Settings, (600, 180), cx),
                    "external-close" => {
                        let handle = cx.windows()[0];
                        handle
                            .update(cx, |_, window, _| window.remove_window())
                            .unwrap();
                    }
                    "close" => state.windows.retain(|_| false, cx),
                    _ => unreachable!(),
                }
                if status != "external-close" && status != "close" {
                    assert_eq!(
                        cx.windows().len(),
                        1,
                        "duplicate or missing window after {status}"
                    );
                }
                state.transitions += 1;
                observed.set(state.transitions);
            },
        );
        sender.join().unwrap();
        assert_eq!(completed.get(), 6);
    }
    #[test]
    #[ignore = "requires Wayland and RUNE_TEST_OUTPUT connector name"]
    fn wayland_notch_on_named_output() {
        use crate::{
            Config, Surface,
            gpui_notch::Notch,
            gpui_surfaces::{Windows, options},
        };
        use gpui::AppContext;
        let connector = std::env::var("RUNE_TEST_OUTPUT")
            .expect("set RUNE_TEST_OUTPUT to a connected output name");
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            tx.send(UiEvent::Status("open".into())).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(400));
            tx.send(UiEvent::Status("check".into())).unwrap();
            tx.send(UiEvent::Quit).unwrap();
        });
        let windows = std::rc::Rc::new(RefCell::new(Windows::default()));
        let checked = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = checked.clone();
        run(
            rx,
            |cx| {
                let (actions, _) = tokio::sync::mpsc::unbounded_channel();
                let (_, events) = tokio::sync::mpsc::unbounded_channel();
                let mut config = Config::default();
                config.bar.left = vec![crate::config::Widget::Workspaces];
                config.bar.center = vec![crate::config::Widget::WindowTitle];
                config.bar.right = vec![
                    crate::config::Widget::Clock,
                    crate::config::Widget::Settings,
                ];
                let (shell, _) = crate::App::new(config, events, actions, false);
                cx.new(|_| shell)
            },
            move |_, event, cx| {
                let UiEvent::Status(stage) = event else {
                    return;
                };
                let resolution = crate::outputs::connected(std::slice::from_ref(&connector), cx);
                assert!(resolution.pending.is_empty(), "named output not discovered");
                let display = resolution.ready[0].1;
                if stage == "open" {
                    let shell = cx.entity();
                    let mut options =
                        options(&Surface::Bar(connector.clone()), (0, 32), None, false);
                    options.display_id = Some(display);
                    let monitor = connector.clone();
                    let windows = windows.clone();
                    let surface = Surface::Bar(connector.clone());
                    // open_window can render immediately; release the shell entity's
                    // update borrow before constructing a view that reads it.
                    cx.defer(move |cx| {
                        windows
                            .borrow_mut()
                            .open(surface, options, cx, move |_, cx| {
                                cx.new(|cx| {
                                    Notch::new(shell, monitor, std::rc::Rc::new(|_, _, _| {}), cx)
                                })
                            })
                            .unwrap();
                    });
                } else {
                    let handle = cx.windows()[0];
                    let actual = handle
                        .update(cx, |_, window, cx| window.display(cx).map(|d| d.id()))
                        .unwrap();
                    assert_eq!(actual, Some(display));
                    observed.set(true);
                }
            },
        );
        sender.join().unwrap();
        assert!(checked.get());
    }
}
