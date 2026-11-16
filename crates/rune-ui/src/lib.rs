//! Rune Shell's UI, built on iced + iced_layershell.
//!
//! One `rune-bar` layer surface (the notch) per Hyprland monitor, plus an
//! on-demand `rune-settings` overlay. The notch on the focused monitor can
//! expand into the launcher or keybind cheat sheet. The UI never does I/O:
//! it receives [`UiEvent`]s from the daemon and sends [`Action`]s back.

mod bt;
mod calendar;
pub mod config;
mod launcher;
mod media;
mod notch;
mod notify;
mod quick;
mod settings;
mod status;
mod tray;
mod weather;

use chrono::{DateTime, Datelike, Local};
pub use config::Config;
use iced::{
    Color, Element, Event, Subscription, Task, Theme, event,
    futures::stream,
    keyboard::{self, key::Named},
    time::Instant,
    widget::text,
    window::{self, Id},
};
use iced_layershell::{
    build_pattern::daemon,
    reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption},
    settings::{LayerShellSettings, Settings, StartMode},
    to_layer_message,
};
use launcher::{Kind, Launcher, Outcome};
use rune_services::{
    apps::App as DesktopApp,
    audio, bluetooth, calendar as cal, capture, clipboard,
    hyprland::{Bind, HyprState},
    media::{Control, Media},
    notifications::{self, Image},
    system::{Battery, Network, PowerAction},
    tray::{MenuEntry, TrayItem},
    weather::{Location, Query as WeatherQuery, Weather},
};
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Something the user asked for; the daemon carries it out.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    FocusWorkspace(i32),
    /// Write this config to `config.toml` (sent by the settings window).
    SaveConfig(Box<Config>),
    /// Run a shell command, detached from the shell.
    Launch(String),
    /// Rescan applications; answered with [`UiEvent::Apps`].
    LoadApps,
    /// Fetch Hyprland keybinds; answered with [`UiEvent::Binds`].
    LoadBinds,
    /// Compute the theme in `config.theme`, write `[colors]` and the enabled apps' files;
    /// answered with [`UiEvent::Config`] and [`UiEvent::Status`].
    ApplyTheme(Box<Config>),
    /// Put back every file the theme engine changed.
    RestoreThemes,
    /// List a wallpaper folder; answered with [`UiEvent::Wallpapers`] (repeatedly, as thumbnails appear).
    LoadWallpapers(std::path::PathBuf),
    SetWallpaper(std::path::PathBuf),
    /// Turn Hyprland blur behind Rune's surfaces on or off.
    Blur(bool),
    /// Send a transport control to the current media player.
    Media(Control),
    /// Notification center and popup requests.
    Notif(notifications::Command),
    Tray(TrayRequest),
    /// Where to fetch weather for (`None` turns it off).
    Weather(Option<WeatherQuery>),
    RefreshWeather,
    /// Search places; answered with [`UiEvent::Geocoded`].
    Geocode(String),
    Bluetooth(bluetooth::Command),
    Screenshot(capture::Target, capture::Options),
    /// Extract text from a selected region.
    Ocr(capture::Options),
    Record(capture::RecordCommand),
    GameMode(bool),
    Volume(audio::Change),
    Brightness(audio::Change),
    Wifi(bool),
    Power(PowerAction),
    /// Fetch clipboard history; answered with [`UiEvent::Clipboard`].
    LoadClipboard,
    /// Put a history entry (cliphist id) back on the clipboard.
    ClipboardCopy(String),
    /// Calendar links to follow.
    Calendar(Vec<String>),
    RefreshCalendar,
}

/// Input from the daemon.
#[derive(Debug, Clone)]
pub enum UiEvent {
    Hypr(HyprState),
    Clock(DateTime<Local>),
    Config(Box<Config>),
    Apps(Vec<DesktopApp>),
    Binds(Vec<Bind>),
    Media(Option<Media>),
    /// Visualizer bars, 0..=100 each; empty while nothing plays.
    Bars(Vec<u8>),
    ToggleMedia,
    ToggleSettings,
    ToggleLauncher,
    ToggleKeybinds,
    ToggleNotifications,
    ToggleWeather,
    /// Open settings on the Bluetooth page.
    OpenBluetooth,
    Weather(Option<Weather>),
    Bluetooth(bluetooth::State),
    Geocoded(Result<Vec<Location>, String>),
    Recording(Option<capture::Recording>),
    Calendar(cal::State),
    ToggleCalendar,
    Volume(Option<audio::Volume>),
    /// New backlight level after a brightness change (shows the OSD).
    Brightness(Option<u8>),
    Network(Option<Network>),
    Battery(Option<Battery>),
    Clipboard(Vec<clipboard::Entry>),
    ToggleQuick,
    TogglePower,
    ToggleClipboard,
    /// Turn game mode on, off, or (`None`) toggle it.
    GameMode(Option<bool>),
    Notifications(notifications::State),
    Tray(Vec<TrayItem>),
    /// An app's tray menu (item key, entries), answering [`TrayRequest::LoadMenu`].
    TrayMenu(String, Vec<MenuEntry>),
    Wallpapers(Vec<rune_services::wallpaper::Wallpaper>),
    /// Outcome of a theme or wallpaper job, shown in settings.
    Status(String),
    Quit,
}

/// Fatal UI errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Iced(#[from] iced_layershell::Error),
}

/// Runs the UI on the calling thread until [`UiEvent::Quit`].
/// `hyprland` says whether monitor info will arrive; without it a single notch
/// opens on whichever output the compositor picks.
pub fn run(
    config: Config,
    events: UnboundedReceiver<UiEvent>,
    actions: UnboundedSender<Action>,
    hyprland: bool,
) -> Result<(), Error> {
    let boot = Mutex::new(Some((config, events, actions)));
    daemon(
        move || {
            // iced calls boot exactly once, so the inputs are always still there.
            #[allow(clippy::unwrap_used)]
            let (config, events, actions) = boot.lock().unwrap().take().unwrap();
            App::new(config, events, actions, hyprland)
        },
        || "rune-shell".to_owned(),
        App::update,
        App::view,
    )
    .subscription(App::subscription)
    .theme(App::theme)
    .style(|_, theme: &Theme| iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    })
    .settings(Settings {
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()?;
    Ok(())
}

/// What the expanded notch shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Panel {
    Launcher(Kind),
    Media,
    Notifications,
    TrayMenu,
    Weather,
    Calendar,
    Quick,
    Power,
}

/// Tray requests for the daemon; items are named by [`TrayItem::key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayRequest {
    Activate {
        key: String,
        secondary: bool,
    },
    /// For apps without a dbusmenu: ask them to show their own menu.
    ContextMenu(String),
    /// Fetch the menu; answered with [`UiEvent::TrayMenu`].
    LoadMenu(String),
    Click(String, i32),
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Ui(UiEvent),
    Focus(i32),
    ToggleSettings,
    Settings(settings::Msg),
    Launcher(launcher::Msg),
    ToggleMedia,
    MediaControl(Control),
    ToggleNotifications,
    ToggleWeather,
    /// Open settings on the Bluetooth page.
    OpenBluetooth,
    StopRecording,
    GameModeOff,
    VolumeMute,
    VolumeStep(i8),
    /// The notch content's natural width on a monitor.
    NotchWidth(String, f32),
    /// Dragging the player's position slider (seconds), and letting go.
    SeekDrag(f32),
    SeekCommit,
    ToggleQuick,
    TogglePower,
    ToggleClipboard,
    Power(PowerAction),
    WifiToggle(bool),
    BtPower(bool),
    GameModeToggle,
    QuickVolume(f32),
    QuickBrightness(f32),
    QuickScreenshot,
    ToggleCalendar,
    /// Move the calendar by this many months.
    CalMonth(i32),
    CalDay(chrono::NaiveDate),
    /// Open a URL in the default browser (meeting links).
    OpenLink(String),
    NotifDismiss(u32),
    NotifAction(u32, String),
    NotifRemove(u32),
    NotifClear,
    NotifDnd(bool),
    PopupHover(bool),
    TrayClick(String, tray::Click),
    TrayMenuClick(i32),
    /// Move the launcher selection by this many rows.
    Nav(isize),
    CloseLauncher,
    Frame(Instant),
    Escape,
    Noop,
}

pub(crate) struct App {
    pub config: Config,
    pub hypr: HyprState,
    pub now: DateTime<Local>,
    /// Frame clock for animations.
    pub instant: Instant,
    actions: UnboundedSender<Action>,
    hyprland: bool,
    /// Notch windows and the monitor each is on ("" = compositor's choice).
    notches: HashMap<Id, String>,
    settings: Option<Id>,
    settings_ui: settings::State,
    /// Active-workspace pill animations, per (monitor, workspace).
    pub pills: HashMap<(String, i32), iced::Animation<bool>>,
    pub launcher: Launcher,
    pub media: Option<Media>,
    /// Decoded once per track, not per frame.
    pub art: Option<iced::widget::image::Handle>,
    pub bars: Vec<u8>,
    pub panel_kind: Panel,
    pub notif: notifications::State,
    /// Decoded once per notification, not per frame.
    pub notif_images: HashMap<u32, iced::widget::image::Handle>,
    /// The popup surface, while there are popups.
    popup: Option<Id>,
    popup_size: u32,
    popup_hover: bool,
    pub popup_expand: iced::Animation<bool>,
    pub tray: Vec<TrayItem>,
    pub weather: Option<Weather>,
    weather_at: Option<Instant>,
    pub bt: bluetooth::State,
    pub recording: Option<capture::Recording>,
    pub volume: Option<audio::Volume>,
    pub network: Option<Network>,
    pub battery: Option<Battery>,
    /// Per monitor: notch width animating from `.0` to `.1`.
    notch_widths: HashMap<String, (f32, f32, iced::Animation<bool>)>,
    /// Keep frames running until then so the notch can measure its new content.
    measure_until: Instant,
    /// Where the position slider is being dragged to, in seconds.
    pub seek: Option<f32>,
    pub brightness: Option<u8>,
    /// A power action waiting for its confirming second click.
    pub power_confirm: Option<PowerAction>,
    /// The OSD surface, what it shows and until when.
    osd_window: Option<Id>,
    pub osd: Option<status::Osd>,
    osd_until: Instant,
    pub calendar: cal::State,
    /// First day of the month shown, and the selected day.
    pub cal_month: chrono::NaiveDate,
    pub cal_day: chrono::NaiveDate,
    pub game_mode: bool,
    /// Do-not-disturb before game mode turned it on.
    dnd_before_game: Option<bool>,
    pub tray_images: HashMap<String, iced::widget::image::Handle>,
    /// The tray item whose menu is open, and its entries once loaded.
    tray_menu_for: Option<String>,
    pub tray_menu: Option<(String, Vec<MenuEntry>)>,
    /// The notch that is expanded (or still shrinking back), and its monitor.
    pub panel: Option<(Id, String)>,
    /// Whether the panel should be open; `expand` animates towards it.
    pub panel_open: bool,
    pub expand: iced::Animation<bool>,
}

impl App {
    fn new(
        config: Config,
        events: UnboundedReceiver<UiEvent>,
        actions: UnboundedSender<Action>,
        hyprland: bool,
    ) -> (Self, Task<Message>) {
        let mut app = Self {
            expand: iced::Animation::new(false).duration(config.animations.duration()),
            config,
            hypr: HyprState::default(),
            now: Local::now(),
            instant: Instant::now(),
            actions,
            hyprland,
            notches: HashMap::new(),
            settings: None,
            settings_ui: settings::State::default(),
            pills: HashMap::new(),
            launcher: Launcher::default(),
            panel: None,
            panel_open: false,
            media: None,
            art: None,
            bars: Vec::new(),
            panel_kind: Panel::Launcher(Kind::Apps),
            notif: notifications::State::default(),
            notif_images: HashMap::new(),
            popup: None,
            popup_size: 0,
            popup_hover: false,
            popup_expand: iced::Animation::new(false),
            tray: Vec::new(),
            weather: None,
            weather_at: None,
            bt: bluetooth::State::default(),
            recording: None,
            volume: None,
            network: None,
            battery: None,
            notch_widths: HashMap::new(),
            measure_until: Instant::now(),
            seek: None,
            brightness: None,
            power_confirm: None,
            osd_window: None,
            osd: None,
            osd_until: Instant::now(),
            calendar: cal::State::default(),
            cal_month: Local::now().date_naive().with_day(1).unwrap_or_default(),
            cal_day: Local::now().date_naive(),
            game_mode: false,
            dnd_before_game: None,
            tray_images: HashMap::new(),
            tray_menu_for: None,
            tray_menu: None,
        };
        app.send(Action::LoadApps); // warm the list so the first open is instant
        app.send(Action::Weather(weather_query(&app.config)));
        app.send(Action::Calendar(app.config.calendar.urls.clone()));
        let events = stream::unfold(
            events,
            |mut rx| async move { rx.recv().await.map(|e| (e, rx)) },
        );
        let sync = app.sync_notches();
        (app, Task::batch([Task::run(events, Message::Ui), sync]))
    }

    fn send(&self, action: Action) {
        let _ = self.actions.send(action); // fails only while exiting
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        // The notch's width sensor only measures on frames, and frames only run while
        // something animates; so after anything that can change the bar, run them briefly.
        let reshapes = match &message {
            Message::Ui(UiEvent::Clock(now)) => {
                now.format("%H%M").to_string() != self.now.format("%H%M").to_string()
            }
            Message::Ui(
                UiEvent::Hypr(_)
                | UiEvent::Config(_)
                | UiEvent::Media(_)
                | UiEvent::Notifications(_)
                | UiEvent::Tray(_)
                | UiEvent::Weather(_)
                | UiEvent::Bluetooth(_)
                | UiEvent::Volume(_)
                | UiEvent::Network(_)
                | UiEvent::Battery(_)
                | UiEvent::Recording(_)
                | UiEvent::GameMode(_),
            ) => true,
            _ => false,
        };
        if reshapes {
            self.instant = Instant::now();
            self.measure_until = self.instant + std::time::Duration::from_millis(60); // a few frames is enough
        }
        match message {
            Message::Ui(UiEvent::Hypr(state)) => {
                self.hypr = state;
                self.instant = Instant::now();
                self.animate_pills();
                self.sync_notches()
            }
            Message::Ui(UiEvent::Clock(now)) => {
                self.now = now;
                Task::none()
            }
            Message::Ui(UiEvent::Config(config)) => self.set_config(*config),
            Message::Ui(UiEvent::Apps(apps)) => {
                self.launcher.set_apps(apps);
                Task::none()
            }
            Message::Ui(UiEvent::Media(media)) => {
                let old_art = self.media.as_ref().and_then(|m| m.art.clone());
                let new_art = media.as_ref().and_then(|m| m.art.clone());
                if old_art != new_art {
                    self.art = new_art
                        .map(|bytes| iced::widget::image::Handle::from_bytes(bytes.to_vec()));
                }
                self.media = media;
                self.seek = None;
                Task::none()
            }
            Message::Ui(UiEvent::Bars(bars)) => {
                self.bars = bars;
                Task::none()
            }
            Message::Ui(UiEvent::ToggleMedia) | Message::ToggleMedia => {
                self.toggle_panel(Panel::Media)
            }
            Message::MediaControl(control) => {
                self.send(Action::Media(control));
                Task::none()
            }
            Message::Ui(UiEvent::Notifications(state)) => {
                let keep: Vec<u32> = state
                    .history
                    .iter()
                    .chain(&state.popups)
                    .map(|n| n.id)
                    .collect();
                self.notif_images.retain(|id, _| keep.contains(id));
                for n in state.history.iter().chain(&state.popups) {
                    if let Some(Image::Rgba {
                        width,
                        height,
                        data,
                    }) = &n.image
                    {
                        self.notif_images.entry(n.id).or_insert_with(|| {
                            iced::widget::image::Handle::from_rgba(*width, *height, data.to_vec())
                        });
                    }
                }
                self.notif = state;
                self.sync_popups()
            }
            Message::Ui(UiEvent::Tray(items)) => {
                self.tray_images.clear();
                for item in &items {
                    if let Some(Image::Rgba {
                        width,
                        height,
                        data,
                    }) = &item.image
                    {
                        let handle =
                            iced::widget::image::Handle::from_rgba(*width, *height, data.to_vec());
                        self.tray_images.insert(item.key.clone(), handle);
                    }
                }
                self.tray = items;
                Task::none()
            }
            Message::Ui(UiEvent::TrayMenu(key, entries)) => {
                if self.tray_menu_for.as_ref() == Some(&key) {
                    self.tray_menu = Some((key, entries));
                }
                Task::none()
            }
            Message::Ui(UiEvent::ToggleNotifications) | Message::ToggleNotifications => {
                self.toggle_panel(Panel::Notifications)
            }
            Message::NotifDismiss(id) => self.notif_command(notifications::Command::Dismiss(id)),
            Message::NotifRemove(id) => self.notif_command(notifications::Command::Remove(id)),
            Message::NotifClear => self.notif_command(notifications::Command::ClearHistory),
            Message::NotifAction(id, key) => {
                self.notif_command(notifications::Command::Action(id, key))
            }
            Message::NotifDnd(on) => {
                self.send(Action::Notif(notifications::Command::SetDnd(on)));
                let mut config = self.config.clone();
                config.notifications.dnd = on;
                self.send(Action::SaveConfig(Box::new(config.clone())));
                self.set_config(config)
            }
            Message::PopupHover(on) => {
                self.popup_hover = on;
                self.instant = Instant::now();
                self.popup_expand.go_mut(on, self.instant);
                self.notif_command(notifications::Command::Hover(on))
            }
            Message::TrayClick(key, click) => self.tray_click(key, click),
            Message::TrayMenuClick(id) => {
                if let Some(key) = self.tray_menu_for.clone() {
                    self.send(Action::Tray(TrayRequest::Click(key, id)));
                }
                self.close_panel()
            }
            Message::Ui(UiEvent::Binds(binds)) => {
                self.launcher.set_binds(binds);
                Task::none()
            }
            Message::Ui(UiEvent::ToggleSettings) | Message::ToggleSettings => {
                self.toggle_settings()
            }
            Message::Ui(UiEvent::ToggleLauncher) => self.toggle_panel(Panel::Launcher(Kind::Apps)),
            Message::Ui(UiEvent::ToggleKeybinds) => {
                self.toggle_panel(Panel::Launcher(Kind::Keybinds))
            }
            Message::Ui(UiEvent::Quit) => iced::exit(),
            Message::Focus(id) => {
                self.send(Action::FocusWorkspace(id));
                Task::none()
            }
            Message::Escape if self.power_confirm.is_some() => {
                self.power_confirm = None;
                Task::none()
            }
            Message::Escape if self.panel_open => self.close_panel(),
            Message::Settings(settings::Msg::Close) | Message::Escape => match self.settings.take()
            {
                Some(id) => {
                    self.stop_bt_scan();
                    window::close(id)
                }
                None => Task::none(),
            },
            Message::Ui(UiEvent::ToggleWeather) | Message::ToggleWeather => {
                // Refresh on open if the forecast is getting old.
                if self
                    .weather_at
                    .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(600))
                {
                    self.send(Action::RefreshWeather);
                }
                self.toggle_panel(Panel::Weather)
            }
            Message::Ui(UiEvent::Calendar(state)) => {
                self.settings_ui.cal_errors = state.errors.clone();
                self.calendar = state;
                Task::none()
            }
            Message::Ui(UiEvent::ToggleCalendar) | Message::ToggleCalendar => {
                // Open on today, and fetch fresh events.
                let today = Local::now().date_naive();
                self.cal_day = today;
                self.cal_month = today.with_day(1).unwrap_or(today);
                self.send(Action::RefreshCalendar);
                self.toggle_panel(Panel::Calendar)
            }
            Message::CalMonth(by) => {
                self.cal_month = calendar::shift_month(self.cal_month, by);
                Task::none()
            }
            Message::CalDay(day) => {
                self.cal_day = day;
                Task::none()
            }
            Message::OpenLink(url) => {
                self.send(Action::Launch(format!(
                    "xdg-open {}",
                    calendar::shell_quote(&url)
                )));
                self.close_panel()
            }
            Message::Ui(UiEvent::Volume(v)) => {
                // The first reading is the starting point, not a change worth an OSD.
                let changed = self.volume.is_some() && v != self.volume;
                self.volume = v;
                match v.filter(|_| changed) {
                    Some(v) => self.show_osd(status::Osd::Volume(v)),
                    None => Task::none(),
                }
            }
            Message::Ui(UiEvent::Brightness(Some(b))) => {
                // The first reading (at startup) just sets the level.
                let first = self.brightness.is_none();
                self.brightness = Some(b);
                if first {
                    Task::none()
                } else {
                    self.show_osd(status::Osd::Brightness(b))
                }
            }
            Message::Ui(UiEvent::Clipboard(entries)) => {
                self.launcher.set_clips(entries);
                Task::none()
            }
            Message::Ui(UiEvent::ToggleQuick) | Message::ToggleQuick => {
                self.toggle_panel(Panel::Quick)
            }
            Message::Ui(UiEvent::TogglePower) | Message::TogglePower => {
                self.power_confirm = None;
                if self.panel_open && self.panel_kind == Panel::Quick {
                    self.panel_kind = Panel::Power; // straight from quick settings
                    return Task::none();
                }
                self.toggle_panel(Panel::Power)
            }
            Message::Ui(UiEvent::ToggleClipboard) | Message::ToggleClipboard => {
                if self.panel_open && self.panel_kind != Panel::Launcher(Kind::Clipboard) {
                    // Coming from quick settings: switch content in place.
                    self.panel_kind = Panel::Launcher(Kind::Clipboard);
                    self.send(Action::LoadClipboard);
                    let keyboard = self.panel.as_ref().map(|(id, _)| {
                        Task::done(Message::KeyboardInteractivityChange {
                            id: *id,
                            keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        })
                    });
                    return Task::batch([
                        self.launcher.reset(Kind::Clipboard),
                        keyboard.unwrap_or_else(Task::none),
                    ]);
                }
                self.toggle_panel(Panel::Launcher(Kind::Clipboard))
            }
            Message::Power(action) => {
                let (run, confirm) = power_click(self.power_confirm, action);
                self.power_confirm = confirm;
                if !run {
                    return Task::none();
                }
                self.send(Action::Power(action));
                self.close_panel()
            }
            Message::WifiToggle(on) => {
                self.send(Action::Wifi(on));
                Task::none()
            }
            Message::BtPower(on) => {
                self.send(Action::Bluetooth(bluetooth::Command::Power(on)));
                Task::none()
            }
            Message::GameModeToggle => self.set_game_mode(!self.game_mode),
            Message::QuickVolume(v) => {
                self.send(Action::Volume(audio::Change::Set(v.round() as u8)));
                Task::none()
            }
            Message::QuickBrightness(v) => {
                self.send(Action::Brightness(audio::Change::Set(
                    (v.round() as u8).max(1),
                )));
                Task::none()
            }
            Message::QuickScreenshot => {
                self.send(Action::Screenshot(
                    capture::Target::Selection,
                    capture_options(&self.config),
                ));
                self.close_panel()
            }
            Message::Ui(UiEvent::Network(n)) => {
                self.network = n;
                Task::none()
            }
            Message::Ui(UiEvent::Battery(b)) => {
                self.battery = b;
                Task::none()
            }
            Message::SeekDrag(secs) => {
                self.seek = Some(secs);
                Task::none()
            }
            Message::SeekCommit => {
                if let Some(secs) = self.seek {
                    self.send(Action::Media(Control::Seek(
                        std::time::Duration::from_secs_f32(secs.max(0.0)),
                    )));
                }
                // Keep showing the dragged spot until the player reports the new position.
                Task::none()
            }
            Message::NotchWidth(monitor, width) => {
                let now = Instant::now();
                let duration = self.config.animations.duration();
                let current = self.notch_width(&monitor);
                match self.notch_widths.get(&monitor) {
                    Some((_, to, _)) if (to - width).abs() < 0.5 => {}
                    _ => {
                        // Start from wherever it is now, so mid-animation changes stay smooth.
                        let from = current.unwrap_or(width);
                        let anim = iced::Animation::new(false).duration(duration).go(true, now);
                        self.notch_widths.insert(monitor, (from, width, anim));
                        self.instant = now;
                    }
                }
                Task::none()
            }
            Message::VolumeMute => {
                self.send(Action::Volume(audio::Change::ToggleMute));
                Task::none()
            }
            Message::VolumeStep(step) => {
                let change = if step > 0 {
                    audio::Change::Up(step.unsigned_abs())
                } else {
                    audio::Change::Down(step.unsigned_abs())
                };
                self.send(Action::Volume(change));
                Task::none()
            }
            Message::Ui(UiEvent::Recording(r)) => {
                self.recording = r;
                Task::none()
            }
            Message::StopRecording => {
                self.send(Action::Record(capture::RecordCommand::Stop));
                Task::none()
            }
            Message::GameModeOff => self.set_game_mode(false),
            Message::Ui(UiEvent::GameMode(want)) => {
                self.set_game_mode(want.unwrap_or(!self.game_mode))
            }
            Message::Ui(UiEvent::Weather(w)) => {
                self.weather_at = w.as_ref().map(|_| Instant::now());
                self.weather = w;
                Task::none()
            }
            Message::Ui(UiEvent::Bluetooth(state)) => {
                self.settings_ui.bt = state.clone();
                self.bt = state;
                Task::none()
            }
            Message::Ui(UiEvent::Geocoded(result)) => {
                match result {
                    Ok(places) => {
                        self.settings_ui.search_status = if places.is_empty() {
                            "No places found".into()
                        } else {
                            String::new()
                        };
                        self.settings_ui.places = places;
                    }
                    Err(e) => self.settings_ui.search_status = e,
                }
                Task::none()
            }
            Message::Ui(UiEvent::OpenBluetooth) | Message::OpenBluetooth => {
                let open = if self.settings.is_none() {
                    self.toggle_settings()
                } else {
                    Task::none()
                };
                if self.settings_ui.page() != settings::Page::Bluetooth {
                    let mut config = self.config.clone();
                    if let Some(settings::Request::Bluetooth(cmd)) = self
                        .settings_ui
                        .update(&mut config, settings::Msg::Page(settings::Page::Bluetooth))
                    {
                        self.send(Action::Bluetooth(cmd));
                    }
                }
                open
            }
            Message::Settings(msg) => {
                let mut config = self.config.clone();
                let request = self.settings_ui.update(&mut config, msg);
                if let Some(request) = request {
                    self.settings_ui.status.clear();
                    match request {
                        settings::Request::ApplyTheme => {
                            self.send(Action::ApplyTheme(Box::new(config.clone())))
                        }
                        settings::Request::RestoreThemes => self.send(Action::RestoreThemes),
                        settings::Request::LoadWallpapers => {
                            self.send(Action::LoadWallpapers(config.wallpaper.dir_path()))
                        }
                        settings::Request::Blur(on) => self.send(Action::Blur(on)),
                        settings::Request::Geocode(q) => self.send(Action::Geocode(q)),
                        settings::Request::Bluetooth(cmd) => self.send(Action::Bluetooth(cmd)),
                        settings::Request::SetWallpaper(path) => {
                            self.send(Action::SetWallpaper(path));
                            if config.theme.source == config::ThemeSource::Wallpaper {
                                self.send(Action::ApplyTheme(Box::new(config.clone())));
                            }
                        }
                    }
                }
                if config == self.config {
                    return Task::none();
                }
                self.send(Action::SaveConfig(Box::new(config.clone())));
                self.set_config(config)
            }
            Message::Ui(UiEvent::Wallpapers(list)) => {
                self.settings_ui.wallpapers = list;
                Task::none()
            }
            Message::Ui(UiEvent::Status(status)) => {
                self.settings_ui.status = status;
                Task::none()
            }
            Message::Launcher(msg) => match self.launcher.update(msg, &self.config) {
                Some(Outcome::Launch(command)) => {
                    self.send(Action::Launch(command));
                    self.close_panel()
                }
                Some(Outcome::Copy(text)) => {
                    Task::batch([iced::clipboard::write(text), self.close_panel()])
                }
                Some(Outcome::Clip(id)) => {
                    self.send(Action::ClipboardCopy(id));
                    self.close_panel()
                }
                None => Task::none(),
            },
            Message::Nav(by)
                if self.panel_open && matches!(self.panel_kind, Panel::Launcher(_)) =>
            {
                self.launcher.select(by)
            }
            Message::CloseLauncher => self.close_panel(),
            Message::Frame(at) => {
                self.instant = at;
                let osd = match self.osd_window {
                    Some(id) if at >= self.osd_until => {
                        self.osd_window = None;
                        window::close(id)
                    }
                    _ => Task::none(),
                };
                Task::batch([self.finish_close(), self.sync_popups(), osd])
            }
            _ => Task::none(), // layer-shell control messages are handled by iced_layershell
        }
    }

    fn view(&self, id: Id) -> Element<'_, Message> {
        if Some(id) == self.osd_window {
            return status::osd(self);
        }
        if Some(id) == self.popup {
            return notify::popups(self);
        }
        if Some(id) == self.settings {
            return self.settings_ui.view(&self.config).map(Message::Settings);
        }
        match self.notches.get(&id) {
            Some(monitor) => notch::view(self, monitor),
            None => text("").into(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let keys = event::listen_with(|event, _, _| match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key),
                ..
            }) => match key {
                Named::Escape => Some(Message::Escape),
                Named::ArrowUp => Some(Message::Nav(-1)),
                Named::ArrowDown => Some(Message::Nav(1)),
                Named::PageUp => Some(Message::Nav(-8)),
                Named::PageDown => Some(Message::Nav(8)),
                _ => None,
            },
            _ => None,
        });
        let animating = self.pills.values().any(|a| a.is_animating(self.instant))
            || self.expand.is_animating(self.instant)
            || self.popup_expand.is_animating(self.instant)
            || self.osd_window.is_some() // the frame clock hides the OSD on time
            || self.notch_widths.values().any(|(_, _, a)| a.is_animating(self.instant))
            || self.instant < self.measure_until
            || (self.panel.is_some() && !self.panel_open); // a shrink is pending
        let frames = if animating {
            window::frames().map(Message::Frame)
        } else {
            Subscription::none()
        };
        Subscription::batch([keys, frames])
    }

    fn theme(&self, _: Id) -> Theme {
        let c = &self.config.colors;
        Theme::custom(
            "Rune".to_owned(),
            iced::theme::Palette {
                background: c.background.into(),
                text: c.foreground.into(),
                primary: c.accent.into(),
                success: Color::from_rgb8(0xa6, 0xe3, 0xa1),
                warning: Color::from_rgb8(0xf9, 0xe2, 0xaf),
                danger: c.urgent.into(),
            },
        )
    }

    /// The notch content's visible width on `monitor`, once measured.
    pub fn notch_width(&self, monitor: &str) -> Option<f32> {
        let (from, to, anim) = self.notch_widths.get(monitor)?;
        Some(anim.interpolate(*from, *to, self.instant))
    }

    /// How far the notch on `monitor` is expanded, 0 to 1.
    pub fn expansion(&self, monitor: &str) -> f32 {
        match &self.panel {
            Some((_, m)) if m == monitor => self.expand.interpolate(0.0, 1.0, self.instant),
            _ => 0.0,
        }
    }

    fn set_config(&mut self, config: Config) -> Task<Message> {
        if config.calendar != self.config.calendar {
            self.send(Action::Calendar(config.calendar.urls.clone()));
        }
        if config.weather != self.config.weather {
            self.send(Action::Weather(weather_query(&config)));
        }
        let rebuild = config.bar.height != self.config.bar.height
            || config.bar.outputs != self.config.bar.outputs;
        self.config = config;
        self.pills.clear(); // picks up the new animation duration
        self.animate_pills();
        let open = self.expand.value();
        self.expand = iced::Animation::new(open).duration(self.config.animations.duration());
        if !rebuild {
            return Task::none();
        }
        // The notch height is baked into the layer surface; reopen them.
        self.panel = None;
        self.panel_open = false;
        self.expand = iced::Animation::new(false).duration(self.config.animations.duration());
        let close: Vec<_> = self
            .notches
            .drain()
            .map(|(id, _)| window::close::<Message>(id))
            .collect();
        Task::batch(close).chain(self.sync_notches())
    }

    /// Opens a notch on every monitor that lacks one and closes orphans.
    fn sync_notches(&mut self) -> Task<Message> {
        let outputs = &self.config.bar.outputs;
        let mut wanted: Vec<String> = self
            .hypr
            .monitors
            .iter()
            .map(|m| m.name.clone())
            .filter(|n| outputs.is_empty() || outputs.contains(n))
            .collect();
        if !self.hyprland {
            wanted.push(String::new());
        }
        let mut tasks = Vec::new();
        self.notches.retain(|&id, name| {
            let keep = wanted.contains(name);
            if !keep {
                tasks.push(window::close(id));
            }
            keep
        });
        if self
            .panel
            .as_ref()
            .is_some_and(|(id, _)| !self.notches.contains_key(id))
        {
            self.panel = None; // its monitor was unplugged
            self.panel_open = false;
        }
        for name in wanted {
            if self.notches.values().any(|n| *n == name) {
                continue;
            }
            let id = Id::unique();
            let h = self.config.bar.height;
            let output_option = if name.is_empty() {
                OutputOption::Active
            } else {
                OutputOption::OutputName(name.clone())
            };
            self.notches.insert(id, name);
            tasks.push(Task::done(Message::NewLayerShell {
                settings: NewLayerShellSettings {
                    size: Some((0, h)),
                    layer: Layer::Top,
                    anchor: Anchor::Top | Anchor::Left | Anchor::Right,
                    exclusive_zone: Some(h as i32),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option,
                    namespace: Some("rune-bar".to_owned()),
                    ..Default::default()
                },
                id,
            }));
        }
        Task::batch(tasks)
    }

    fn toggle_settings(&mut self) -> Task<Message> {
        if let Some(id) = self.settings.take() {
            self.stop_bt_scan();
            return window::close(id);
        }
        if self.settings_ui.page() == settings::Page::Bluetooth {
            self.send(Action::Bluetooth(bluetooth::Command::Scan(true)));
        }
        let id = Id::unique();
        self.settings = Some(id);
        Task::done(Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some((860, 580)),
                layer: Layer::Overlay,
                anchor: Anchor::empty(), // centered
                exclusive_zone: None,
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                output_option: OutputOption::Active,
                namespace: Some("rune-settings".to_owned()),
                ..Default::default()
            },
            id,
        })
    }

    /// Opens the panel on the focused monitor's notch, switches its content, or closes it.
    fn toggle_panel(&mut self, panel: Panel) -> Task<Message> {
        if self.panel_open && self.panel_kind == panel {
            return self.close_panel();
        }
        let mut tasks = Vec::new();
        if let Panel::Launcher(kind) = panel {
            self.send(match kind {
                Kind::Apps => Action::LoadApps,
                Kind::Keybinds => Action::LoadBinds,
                Kind::Clipboard => Action::LoadClipboard,
            });
            tasks.push(self.launcher.reset(kind));
        }
        self.panel_kind = panel;
        // The launcher needs the keyboard; the player shouldn't steal it.
        let keyboard_interactivity = match panel {
            Panel::Launcher(_) => KeyboardInteractivity::Exclusive,
            Panel::Media
            | Panel::Notifications
            | Panel::TrayMenu
            | Panel::Weather
            | Panel::Calendar
            | Panel::Quick
            | Panel::Power => KeyboardInteractivity::OnDemand,
        };
        if self.panel_open {
            if let Some((id, _)) = self.panel {
                tasks.push(Task::done(Message::KeyboardInteractivityChange {
                    id,
                    keyboard_interactivity,
                }));
            }
            return Task::batch(tasks); // already expanded; just switch content
        }
        let focused = self
            .hypr
            .monitors
            .iter()
            .find(|m| m.focused)
            .map_or("", |m| m.name.as_str());
        let target = match &self.panel {
            Some(p) => Some(p.clone()), // reopening while it shrinks
            None => self
                .notches
                .iter()
                .find(|(_, n)| *n == focused)
                .or_else(|| self.notches.iter().next())
                .map(|(id, n)| (*id, n.clone())),
        };
        let Some((id, monitor)) = target else {
            return Task::none();
        };
        self.panel = Some((id, monitor));
        self.panel_open = true;
        self.instant = Instant::now();
        self.expand.go_mut(true, self.instant);
        // Sized for the tallest panel so switching content never needs a resize.
        let h = self.config.bar.height + launcher::PANEL_H as u32 + 16;
        tasks.push(Task::done(Message::SizeChange { id, size: (0, h) }));
        tasks.push(Task::done(Message::KeyboardInteractivityChange {
            id,
            keyboard_interactivity,
        }));
        Task::batch(tasks)
    }

    /// Size of the current panel's area below the bar row.
    pub fn panel_size(&self) -> (f32, f32) {
        match self.panel_kind {
            Panel::Launcher(_) => (launcher::PANEL_W, launcher::PANEL_H),
            Panel::Media => (media::PANEL_W, media::PANEL_H),
            Panel::Notifications => (460.0, launcher::PANEL_H),
            Panel::TrayMenu => (tray::MENU_W, tray::MENU_H),
            Panel::Weather => (weather::PANEL_W, weather::PANEL_H),
            Panel::Calendar => (calendar::PANEL_W, calendar::PANEL_H),
            Panel::Quick => (quick::QUICK_W, quick::QUICK_H),
            Panel::Power => (quick::POWER_W, quick::POWER_H),
        }
    }

    /// Starts collapsing; [`Self::finish_close`] shrinks the surface once the animation ends.
    fn close_panel(&mut self) -> Task<Message> {
        let Some((id, _)) = self.panel.clone().filter(|_| self.panel_open) else {
            return Task::none();
        };
        self.panel_open = false;
        self.instant = Instant::now();
        self.expand.go_mut(false, self.instant);
        Task::done(Message::KeyboardInteractivityChange {
            id,
            keyboard_interactivity: KeyboardInteractivity::None,
        })
    }

    fn finish_close(&mut self) -> Task<Message> {
        if self.panel_open || self.expand.is_animating(self.instant) {
            return Task::none();
        }
        match self.panel.take() {
            Some((id, _)) => Task::done(Message::SizeChange {
                id,
                size: (0, self.config.bar.height),
            }),
            None => Task::none(),
        }
    }

    /// Starts a transition for every pill whose active state changed.
    fn animate_pills(&mut self) {
        let duration = self.config.animations.duration();
        for m in &self.hypr.monitors {
            for id in notch::workspace_ids(&self.config, &self.hypr) {
                let active = m.active_workspace.id == id;
                let anim = self
                    .pills
                    .entry((m.name.clone(), id))
                    .or_insert_with(|| iced::Animation::new(active).duration(duration));
                if anim.value() != active {
                    anim.go_mut(active, self.instant);
                }
            }
        }
    }

    fn notif_command(&mut self, command: notifications::Command) -> Task<Message> {
        self.send(Action::Notif(command));
        Task::none()
    }

    /// Opens, resizes or closes the popup surface to fit the popups.
    fn sync_popups(&mut self) -> Task<Message> {
        if self.notif.popups.is_empty() {
            self.popup_size = 0;
            if self.popup_hover {
                self.popup_hover = false;
                self.popup_expand = iced::Animation::new(false);
                self.send(Action::Notif(notifications::Command::Hover(false)));
            }
            return self.popup.take().map_or_else(Task::none, window::close);
        }
        // Fanned-out size while hovered or animating, so the stack never gets cut off.
        let open = self.popup_hover || self.popup_expand.is_animating(self.instant);
        let height = notify::stack_height(&self.notif.popups, if open { 1.0 } else { 0.0 }).ceil()
            as u32
            + 4;
        let width = notify::WIDTH as u32;
        match self.popup {
            Some(id) if self.popup_size != height => {
                self.popup_size = height;
                Task::done(Message::SizeChange {
                    id,
                    size: (width, height),
                })
            }
            Some(_) => Task::none(),
            None => {
                let id = Id::unique();
                self.popup = Some(id);
                self.popup_size = height;
                let top = self.config.bar.height as i32 + 8;
                Task::done(Message::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some((width, height)),
                        layer: Layer::Overlay,
                        anchor: Anchor::Top | Anchor::Right,
                        exclusive_zone: None,
                        margin: Some((top, 12, 0, 12)),
                        keyboard_interactivity: KeyboardInteractivity::None,
                        output_option: OutputOption::Active,
                        namespace: Some("rune-notifications".to_owned()),
                        ..Default::default()
                    },
                    id,
                })
            }
        }
    }

    fn tray_click(&mut self, key: String, click: tray::Click) -> Task<Message> {
        let Some(item) = self.tray.iter().find(|i| i.key == key) else {
            return Task::none();
        };
        let wants_menu =
            click == tray::Click::Right || (click == tray::Click::Left && item.item_is_menu);
        if click == tray::Click::Middle || !wants_menu {
            let secondary = click == tray::Click::Middle;
            self.send(Action::Tray(TrayRequest::Activate { key, secondary }));
            return Task::none();
        }
        if item.menu.is_none() {
            self.send(Action::Tray(TrayRequest::ContextMenu(key)));
            return Task::none();
        }
        let same = self.tray_menu_for.as_ref() == Some(&key);
        self.tray_menu = None;
        self.tray_menu_for = Some(key.clone());
        self.send(Action::Tray(TrayRequest::LoadMenu(key)));
        if self.panel_open && self.panel_kind == Panel::TrayMenu && !same {
            return Task::none(); // switch to the other app's menu in place
        }
        self.toggle_panel(Panel::TrayMenu)
    }

    /// Shows (or refreshes) the OSD for 1.5 s.
    fn show_osd(&mut self, osd: status::Osd) -> Task<Message> {
        self.osd = Some(osd);
        self.instant = Instant::now();
        self.osd_until = self.instant + std::time::Duration::from_millis(1500);
        if self.osd_window.is_some() {
            return Task::none();
        }
        let id = Id::unique();
        self.osd_window = Some(id);
        Task::done(Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some((status::OSD_W, status::OSD_H)),
                layer: Layer::Overlay,
                anchor: Anchor::Bottom,
                exclusive_zone: None,
                margin: Some((0, 0, 90, 0)),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: OutputOption::Active,
                namespace: Some("rune-osd".to_owned()),
                events_transparent: true,
            },
            id,
        })
    }

    /// Game mode strips Hyprland's effects and silences notifications until it ends.
    fn set_game_mode(&mut self, on: bool) -> Task<Message> {
        if on == self.game_mode {
            return Task::none();
        }
        self.game_mode = on;
        self.send(Action::GameMode(on));
        if on {
            self.dnd_before_game = Some(self.notif.dnd);
            self.send(Action::Notif(notifications::Command::SetDnd(true)));
        } else {
            let dnd = self
                .dnd_before_game
                .take()
                .unwrap_or(self.config.notifications.dnd);
            self.send(Action::Notif(notifications::Command::SetDnd(dnd)));
            // Leaving game mode reloads Hyprland's config, which drops runtime rules.
            if self.config.bar.blur {
                self.send(Action::Blur(true));
            }
        }
        Task::none()
    }

    /// Scanning only runs while the Bluetooth settings page is visible.
    fn stop_bt_scan(&self) {
        if self.settings_ui.page() == settings::Page::Bluetooth {
            self.send(Action::Bluetooth(bluetooth::Command::Scan(false)));
        }
    }
}

fn weather_query(config: &Config) -> Option<WeatherQuery> {
    let w = &config.weather;
    (!w.location.is_empty()).then_some(WeatherQuery {
        latitude: w.latitude,
        longitude: w.longitude,
        imperial: w.imperial,
    })
}

/// Capture settings from the config (the control socket uses this too).
pub fn capture_options(c: &Config) -> capture::Options {
    let cap = &c.capture;
    capture::Options {
        screenshot_dir: config::expand_home(&cap.screenshot_dir),
        copy: cap.copy,
        record_dir: config::expand_home(&cap.record_dir),
        record_audio: cap.record_audio,
        fps: cap.fps.clamp(1, 240),
        ocr_lang: cap.ocr_lang.clone(),
    }
}

/// One click in the power menu: whether to run `action` now, and what is left
/// waiting for confirmation. Session-ending actions need two clicks in a row.
fn power_click(pending: Option<PowerAction>, action: PowerAction) -> (bool, Option<PowerAction>) {
    if action.needs_confirmation() && pending != Some(action) {
        (false, Some(action))
    } else {
        (true, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_menu_asks_twice() {
        assert_eq!(
            power_click(None, PowerAction::Lock),
            (true, None),
            "lock runs at once"
        );
        assert_eq!(
            power_click(None, PowerAction::Shutdown),
            (false, Some(PowerAction::Shutdown))
        );
        assert_eq!(
            power_click(Some(PowerAction::Shutdown), PowerAction::Shutdown),
            (true, None)
        );
        // Confirming one action never runs another.
        assert_eq!(
            power_click(Some(PowerAction::Reboot), PowerAction::Shutdown),
            (false, Some(PowerAction::Shutdown))
        );
    }
}
