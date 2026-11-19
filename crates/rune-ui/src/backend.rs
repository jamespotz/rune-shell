//! GPUI effect interpreter and desired-window reconciliation.
use crate::{
    Action, App as Shell, Config, Message, Surface, UiEvent,
    gpui_notch::Dispatch,
    gpui_surfaces::Windows,
    surfaces::{OutputOption, WindowSpec},
};
use gpui::{AppContext, Entity};
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
#[derive(Default)]
pub(crate) struct Effects<T> {
    commands: Vec<Effect>,
    marker: PhantomData<T>,
}
enum Effect {
    Copy(String),
    Quit,
    Later(std::time::Duration, Box<Message>),
}
impl<T> Effects<T> {
    pub fn none() -> Self {
        Self {
            commands: vec![],
            marker: PhantomData,
        }
    }
    pub fn batch(tasks: impl IntoIterator<Item = Self>) -> Self {
        tasks.into_iter().fold(Self::none(), Self::chain)
    }
    pub fn chain(mut self, other: Self) -> Self {
        self.commands.extend(other.commands);
        self
    }
    pub fn copy(text: String) -> Self {
        Self {
            commands: vec![Effect::Copy(text)],
            marker: PhantomData,
        }
    }
    pub fn quit() -> Self {
        Self {
            commands: vec![Effect::Quit],
            marker: PhantomData,
        }
    }
    /// Delivers `message` after `after`, from a GPUI timer.
    pub fn later(after: std::time::Duration, message: Message) -> Self {
        Self {
            commands: vec![Effect::Later(after, Box::new(message))],
            marker: PhantomData,
        }
    }
}
#[derive(Default)]
struct Backend {
    windows: Windows,
    applied: HashMap<Surface, WindowSpec>,
    wake_sender: Option<UnboundedSender<Option<std::time::Instant>>>,
    wake_at: Option<std::time::Instant>,
    panel: Option<crate::Panel>,
    error: Option<String>,
    unresolved: std::collections::HashSet<String>,
}
type Shared = Rc<RefCell<Backend>>;
#[cfg(test)]
static EXPIRED_OSDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static MEASURED_HEIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn dispatch(shell: Entity<Shell>, backend: Shared, message: Message, cx: &mut gpui::App) {
    #[cfg(test)]
    if let Message::PanelSize(size) = &message {
        MEASURED_HEIGHT.fetch_max(
            f32::from(size.height) as usize,
            std::sync::atomic::Ordering::SeqCst,
        );
    }
    let effects = shell.update(cx, |state, cx| {
        let effects = state.update(message);
        cx.notify();
        effects
    });
    for effect in effects.commands {
        match effect {
            Effect::Copy(text) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)),
            Effect::Quit => cx.quit(),
            Effect::Later(after, message) => {
                let (shell, backend) = (shell.clone(), backend.clone());
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(|cx| dispatch(shell, backend, *message, cx));
                })
                .detach();
            }
        }
    }
    reconcile(shell, backend, cx);
}
fn reconcile(shell: Entity<Shell>, backend: Shared, cx: &mut gpui::App) {
    let bar = &shell.read(cx).config.bar;
    crate::gpui_widgets::set_look(bar.look, bar.pixel_font);
    let reduce = shell.read(cx).config.animations.duration().is_zero();
    if cx.reduce_motion() != reduce {
        cx.set_reduce_motion(reduce);
    }
    let desired = shell.read(cx).surfaces.desired.clone();
    let panel = shell.read(cx).panels.current();
    let mut state = backend.borrow_mut();
    #[cfg(test)]
    if state.applied.contains_key(&Surface::Osd) && !desired.contains_key(&Surface::Osd) {
        EXPIRED_OSDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    if state.panel != panel {
        state.windows.close(&Surface::Panel, cx);
        state.applied.remove(&Surface::Panel);
        state.panel = panel;
    }
    state
        .windows
        .retain(|surface| desired.contains_key(surface), cx);
    state
        .applied
        .retain(|surface, _| desired.contains_key(surface));
    let mut ordered: Vec<_> = desired.into_iter().collect();
    ordered.sort_by_key(|(surface, _)| match surface {
        Surface::Bar(_) => 0,
        Surface::Catcher => 1,
        Surface::Panel => 2,
        Surface::Settings => 3,
        Surface::Popups => 4,
        Surface::Osd => 5,
    });
    for (surface, spec) in ordered {
        let display = match &spec.output_option {
            OutputOption::Active => None,
            OutputOption::OutputName(name) => {
                let resolution = crate::outputs::connected(std::slice::from_ref(name), cx);
                let Some((_, display)) = resolution.ready.first() else {
                    if state.unresolved.insert(name.clone()) {
                        eprintln!(
                            "Rune: output {name} is not available to GPUI; waiting for display discovery"
                        );
                    }
                    state.windows.close(&surface, cx);
                    state.applied.remove(&surface);
                    continue;
                };
                state.unresolved.remove(name);
                Some(*display)
            }
        };
        if let Some(old) = state.applied.get(&surface)
            && old != &spec
        {
            let mut resized = old.clone();
            resized.size = spec.size;
            if resized == spec && !matches!(surface, Surface::Bar(_)) {
                state
                    .windows
                    .resize(&surface, spec.size.unwrap_or((0, 0)), cx);
            } else {
                state.windows.close(&surface, cx);
            }
        }
        let mut options = crate::gpui_surfaces::options(
            &surface,
            spec.size.unwrap_or((0, 0)),
            spec.margin,
            spec.grabs_keyboard,
            spec.top,
        );
        options.display_id = display;
        let dispatch_shell = shell.clone();
        let dispatch_backend = backend.clone();
        let callback: Dispatch = Rc::new(move |message, _, cx| {
            let shell = dispatch_shell.clone();
            let backend = dispatch_backend.clone();
            // Window callbacks temporarily remove their window from App's registry.
            // Reconcile after that borrow ends, or a live window looks closed.
            cx.defer(move |cx| dispatch(shell, backend, message, cx));
        });
        let view_shell = shell.clone();
        let result = match &surface {
            Surface::Bar(monitor) => {
                let monitor = monitor.clone();
                state
                    .windows
                    .open(surface.clone(), options, cx, move |window, cx| {
                        let bar = &view_shell.read(cx).config.bar;
                        if bar.shadow_room() > 0 {
                            // Only the bar takes input, not the shadow over the windows. The
                            // width outgrows any output; Wayland clips regions to the surface.
                            let rows = bar.height.saturating_add(bar.margin_top) as f32;
                            window.set_input_region(Some(&[gpui::Bounds::new(
                                gpui::point(gpui::px(0.), gpui::px(0.)),
                                gpui::size(gpui::px(65_536.), gpui::px(rows)),
                            )]));
                        }
                        cx.new(|cx| {
                            crate::gpui_notch::Notch::new(view_shell, monitor, callback, cx)
                        })
                    })
            }
            Surface::Panel => match panel {
                Some(crate::Panel::Launcher(_)) => {
                    state
                        .windows
                        .open(surface.clone(), options, cx, move |window, cx| {
                            cx.new(|cx| {
                                crate::launcher::gpui_view::LauncherView::new(
                                    view_shell, callback, window, cx,
                                )
                            })
                        })
                }
                Some(panel) => {
                    state
                        .windows
                        .open(surface.clone(), options, cx, move |window, cx| {
                            cx.new(|cx| {
                                crate::gpui_panels::PanelView::new(
                                    view_shell, panel, callback, window, cx,
                                )
                            })
                        })
                }
                None => {
                    state.windows.close(&surface, cx);
                    continue;
                }
            },
            Surface::Settings => {
                state
                    .windows
                    .open(surface.clone(), options, cx, move |window, cx| {
                        cx.new(|cx| {
                            crate::settings::gpui_view::SettingsView::new(
                                view_shell, callback, window, cx,
                            )
                        })
                    })
            }
            _ => {
                let kind = surface.clone();
                state
                    .windows
                    .open(surface.clone(), options, cx, move |_, cx| {
                        cx.new(|cx| {
                            crate::gpui_overlays::Overlay::new(view_shell, kind, callback, cx)
                        })
                    })
            }
        };
        if let Err(error) = result {
            state.error = Some(format!("{}: {error}", surface.namespace()));
            cx.quit();
            continue;
        }
        state.applied.insert(surface, spec);
    }
    let wake_at = shell.read(cx).next_wake();
    // Do not postpone an already scheduled earlier frame when another event arrives.
    let wake_at = match (state.wake_at, wake_at) {
        (Some(old), Some(new)) => Some(old.min(new)),
        (_, next) => next,
    };
    if state.wake_at != wake_at {
        state.wake_at = wake_at;
        if let Some(sender) = &state.wake_sender {
            let _ = sender.send(wake_at);
        }
    }
}

pub(crate) fn run(
    config: Config,
    events: UnboundedReceiver<UiEvent>,
    actions: UnboundedSender<Action>,
    hyprland: bool,
) -> Result<(), crate::Error> {
    let (wake_sender, mut wakes) = tokio::sync::mpsc::unbounded_channel();
    let backend = Rc::new(RefCell::new(Backend {
        wake_sender: Some(wake_sender),
        ..Default::default()
    }));
    let live_backend = backend.clone();
    gpui_platform::application()
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx| {
            crate::controls::text_input::TextInput::bind_keys(cx);
            let (_, unused) = tokio::sync::mpsc::unbounded_channel();
            let (shell, _) = Shell::new(config, unused, actions, hyprland);
            let shell = cx.new(|_| shell);
            let observed = shell.clone();
            let displays_backend = live_backend.clone();
            cx.observe_displays(move |_, cx| {
                reconcile(observed.clone(), displays_backend.clone(), cx)
            })
            .detach();
            let initial = shell.clone();
            let initial_backend = live_backend.clone();
            cx.defer(move |cx| reconcile(initial, initial_backend, cx));
            cx.spawn(async move |cx| {
                use futures::FutureExt;
                let mut events = events;
                let mut deadline: Option<std::time::Instant> = None;
                loop {
                    let timer = match deadline {
                        Some(at) => cx
                            .background_executor()
                            .timer(at.saturating_duration_since(std::time::Instant::now()))
                            .boxed(),
                        None => futures::future::pending().boxed(),
                    }
                    .fuse();
                    futures::pin_mut!(timer);
                    enum Ready {
                        Event(Option<UiEvent>),
                        Wake(Option<Option<std::time::Instant>>),
                        Timer,
                    }
                    let ready = futures::future::poll_fn(|task| {
                        if let std::task::Poll::Ready(wake) = wakes.poll_recv(task) {
                            return std::task::Poll::Ready(Ready::Wake(wake));
                        }
                        if std::future::Future::poll(timer.as_mut(), task).is_ready() {
                            return std::task::Poll::Ready(Ready::Timer);
                        }
                        events.poll_recv(task).map(Ready::Event)
                    })
                    .await;
                    match ready {
                        Ready::Event(Some(UiEvent::Quit)) | Ready::Event(None) => break,
                        Ready::Event(Some(event)) => cx.update(|cx| {
                            dispatch(shell.clone(), live_backend.clone(), Message::Ui(event), cx)
                        }),
                        Ready::Wake(wake) => deadline = wake.flatten(),
                        Ready::Timer => {
                            deadline = None;
                            live_backend.borrow_mut().wake_at = None;
                            cx.update(|cx| {
                                dispatch(
                                    shell.clone(),
                                    live_backend.clone(),
                                    Message::Frame(std::time::Instant::now()),
                                    cx,
                                )
                            });
                        }
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    let error = backend.borrow_mut().error.take();
    match error {
        Some(error) => Err(crate::Error::Window(error)),
        None => Ok(()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "opens the complete shell; requires Wayland"]
    fn complete_backend_opens_switches_updates_and_quits() {
        EXPIRED_OSDS.store(0, std::sync::atomic::Ordering::SeqCst);
        MEASURED_HEIGHT.store(0, std::sync::atomic::Ordering::SeqCst);
        let (events_tx, events) = tokio::sync::mpsc::unbounded_channel();
        let (actions, mut actions_rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = std::thread::spawn(move || {
            let notification = rune_services::notifications::Notification {
                id: 1,
                app_name: "Rune backend test".into(),
                summary: "Popup lifecycle".into(),
                body: "Native overlay".into(),
                image: None,
                actions: vec![],
                urgency: rune_services::notifications::Urgency::Normal,
                time: chrono::Local::now(),
                resident: false,
            };
            for event in [
                UiEvent::Brightness(Some(40)),
                UiEvent::Brightness(Some(60)),
                UiEvent::Notifications(rune_services::notifications::State {
                    popups: vec![notification],
                    active: true,
                    ..Default::default()
                }),
                UiEvent::Toggle(crate::Panel::Launcher(crate::launcher::Kind::Apps)),
                UiEvent::Toggle(crate::Panel::Calendar),
                UiEvent::ToggleSettings,
                UiEvent::ToggleSettings,
                UiEvent::Toggle(crate::Panel::Calendar),
            ] {
                std::thread::sleep(std::time::Duration::from_millis(150));
                events_tx.send(event).unwrap();
            }
            events_tx
                .send(UiEvent::Notifications(Default::default()))
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1600));
            events_tx.send(UiEvent::Quit).unwrap();
        });
        run(Config::default(), events, actions, false).unwrap();
        sender.join().unwrap();
        assert_eq!(
            EXPIRED_OSDS.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "OSD must close on its own deadline before Quit"
        );
        assert!(
            MEASURED_HEIGHT.load(std::sync::atomic::Ordering::SeqCst) > 100,
            "panel content must not collapse to padding at its initial 1x1 window size"
        );
        let mut warmed_apps = false;
        while let Ok(action) = actions_rx.try_recv() {
            if matches!(action, Action::LoadApps) {
                warmed_apps = true;
            }
        }
        assert!(warmed_apps, "startup commands must reach the daemon");
    }
}
