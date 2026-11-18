//! Rune Shell GPUI state and application lifecycle.
//!
//! One `rune-bar` layer surface (the notch) per Hyprland monitor, plus an
//! on-demand `rune-settings` overlay. The notch on the focused monitor can
//! expand into the launcher or keybind cheat sheet. The UI never does I/O:
//! it receives [`UiEvent`]s from the daemon and sends [`Action`]s back.

mod animation;
mod backend;
mod gpui_images;
mod gpui_notch;
mod gpui_overlays;
mod gpui_panels;
mod gpui_surfaces;
mod gpui_widgets;
mod outputs;
mod protocol;
#[cfg(test)]
mod runtime;
pub use protocol::{Action, TrayRequest, UiEvent};

mod bt;
mod calendar;
mod capture_panel;
pub mod config;
mod controls;
mod launcher;
mod media;
mod notch;
mod notify;
mod panel;
mod quick;
mod settings;
mod state;
mod status;
mod surface;
mod surfaces;
mod system_panel;
mod tray;
mod weather;

use backend::Effects;
use capture_panel::CaptureMode;
use chrono::{Datelike, Local};
pub use config::Config;
use launcher::{Kind, Launcher, Outcome};
pub use panel::Panel;
use panel::Panels;
use rune_services::{
    audio, bluetooth, capture, clipboard, equalizer, hyprland,
    media::Control,
    notifications::{self, Image, Notification},
    system::{self, Network, PowerAction, PowerProfile},
    tray::MenuEntry,
    weather::Query as WeatherQuery,
};
use std::time::{Duration, Instant};

const PENDING_POWER_TIMEOUT: Duration = Duration::from_secs(3);
const OWN_SAVE_ECHO: Duration = Duration::from_millis(1500);
use std::{collections::HashMap, sync::Arc};
pub use surfaces::Surface;
use surfaces::Surfaces;
use surfaces::{OutputOption, WindowSpec};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Fatal UI errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GPUI could not open a shell surface: {0}")]
    Window(String),
}

/// Runs the GPUI shell on the calling thread until Quit or channel closure.
pub fn run(
    config: Config,
    events: UnboundedReceiver<UiEvent>,
    actions: UnboundedSender<Action>,
    hyprland: bool,
) -> Result<(), Error> {
    backend::run(config, events, actions, hyprland)
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Ui(UiEvent),
    Focus(i32),
    ToggleSettings,
    Settings(settings::Msg),
    Launcher(launcher::Msg),
    /// Open, switch to, or close a panel (a bar widget was clicked).
    Toggle(Panel),
    MediaControl(Control),
    OpenSettingsPage(settings::Page),
    WifiDropdownPick(usize),
    StopRecording,
    GameModeOff,
    VolumeMute,
    VolumeStep(i8),
    /// Natural size of the open panel's content.
    PanelSize(gpui::Size<gpui::Pixels>),
    /// Pointer over a monitor's bar (monitor, x), or `None` once it leaves.
    BarCursor(Option<(String, f32)>),
    /// A monitor's bar width (= the monitor's logical width).
    BarWidth(String, f32),
    /// Dragging the player's position slider (seconds), and letting go.
    SeekDrag(f32),
    SeekCommit,
    Power(PowerAction),
    WifiToggle(bool),
    BtPower(bool),
    MicMuteToggle,
    NightLightToggle,
    KeepAwakeToggle,
    PowerProfileNext,
    GameModeToggle,
    QuickVolume(f32),
    QuickMicVolume(f32),
    QuickBrightness(f32),
    QuickScreenshot,
    /// Switch the capture panel's mode; `Text` starts OCR at once.
    CaptureMode(CaptureMode),
    /// A capture panel tile: close the panel, then capture.
    Capture(CaptureMode, capture::Target),
    /// Runs the capture once the panel is gone.
    RunCapture(CaptureMode, capture::Target),
    /// Move the calendar by this many months.
    CalMonth(i32),
    CalDay(chrono::NaiveDate),
    /// Open a URL in the default browser (meeting links).
    OpenLink(String),
    NotifDismiss(u32),
    /// A toast was swiped away by this many pixels.
    NotifSwipe(u32, f32),
    NotifAction(u32, String),
    NotifRemove(u32),
    NotifClear,
    NotifDnd(bool),
    PopupHover(bool),
    TrayClick(String, tray::Click),
    TrayMenuClick(i32),
    /// Move the launcher selection by this many rows.
    Nav(isize),
    ClosePanel,
    Frame(Instant),
    Escape,
    Noop,
}

pub(crate) struct App {
    pub snapshot: state::Snapshot,
    pub config: Config,
    pub images: gpui_images::Images,
    /// Frame clock for animations.
    pub instant: Instant,
    actions: UnboundedSender<Action>,
    hyprland: bool,
    surfaces: Surfaces,
    pub panels: Panels,
    settings_ui: settings::State,
    /// Active-workspace pill animations, per (monitor, workspace).
    pub pills: HashMap<(String, i32), animation::Animation<bool>>,
    pub launcher: Launcher,
    /// Decoded once per track, not per frame.
    pub art: Option<Arc<[u8]>>,
    /// Decoded once per notification, not per frame.
    pub notif_images: HashMap<u32, Image>,
    /// Height of the popup surface, while there are popups.
    popup_size: u32,
    popup_hover: bool,
    wifi_pending: Option<(bool, bool, Instant)>,
    bt_pending: Option<(bool, bool, Instant)>,
    pub popup_expand: animation::Animation<bool>,
    pub toast_motion: HashMap<u32, notify::Motion>,
    pub toast_leave: Vec<notify::Leaving>,
    swiped: Option<(u32, f32)>,
    weather_at: Option<Instant>,
    /// Per monitor: notch width animating from `.0` to `.1`.
    /// Keep frames running until then so the notch can measure its new content.
    measure_until: Instant,
    /// Where the position slider is being dragged to, in seconds.
    pub seek: Option<f32>,
    /// A power action waiting for its confirming second click.
    pub power_confirm: Option<PowerAction>,
    /// What the OSD shows and until when.
    pub osd: Option<status::Osd>,
    osd_until: Instant,
    saved_at: Option<Instant>,
    /// First day of the month shown, and the selected day.
    pub cal_month: chrono::NaiveDate,
    pub cal_day: chrono::NaiveDate,
    pub game_mode: bool,
    /// The capture panel's mode, kept for this session.
    pub capture_mode: CaptureMode,
    /// Do-not-disturb before game mode turned it on.
    dnd_before_game: Option<bool>,
    pub tray_images: HashMap<String, Image>,
    /// The tray item whose menu is open, and its entries once loaded.
    tray_menu_for: Option<String>,
    pub tray_menu: Option<(String, Vec<MenuEntry>)>,
}

impl App {
    fn new(
        config: Config,
        _events: UnboundedReceiver<UiEvent>,
        actions: UnboundedSender<Action>,
        hyprland: bool,
    ) -> (Self, Effects<Message>) {
        let mut app = Self {
            snapshot: state::Snapshot::default(),
            images: gpui_images::Images::default(),
            config,
            instant: Instant::now(),
            actions,
            hyprland,
            surfaces: Surfaces::default(),
            panels: Panels::default(),
            settings_ui: settings::State::default(),
            pills: HashMap::new(),
            launcher: Launcher::default(),
            art: None,
            notif_images: HashMap::new(),
            popup_size: 0,
            popup_hover: false,
            wifi_pending: None,
            bt_pending: None,
            popup_expand: animation::Animation::new(false),
            toast_motion: HashMap::new(),
            toast_leave: Vec::new(),
            swiped: None,
            weather_at: None,
            measure_until: Instant::now(),
            seek: None,
            power_confirm: None,
            osd: None,
            osd_until: Instant::now(),
            saved_at: None,
            cal_month: Local::now().date_naive().with_day(1).unwrap_or_default(),
            cal_day: Local::now().date_naive(),
            game_mode: false,
            capture_mode: CaptureMode::default(),
            dnd_before_game: None,
            tray_images: HashMap::new(),
            tray_menu_for: None,
            tray_menu: None,
        };
        app.send(Action::LoadApps); // warm the list so the first open is instant
        app.send(Action::Weather(weather_query(&app.config)));
        app.send(Action::Calendar(app.config.calendar.clone()));
        if app.config.bar.look == config::Look::Pixel {
            app.send(Action::Hypr(hyprland::Command::SquareCorners(true)));
        }
        app.send_border_colors(&app.config.colors);
        let sync = app.sync_notches();
        (app, sync)
    }

    fn send(&self, action: Action) {
        let _ = self.actions.send(action); // fails only while exiting
    }

    fn send_border_colors(&self, colors: &config::Colors) {
        let mut inactive = colors.muted.0;
        inactive[3] = 0x66;
        self.send(Action::Hypr(hyprland::Command::BorderColors(
            colors.accent.0,
            inactive,
        )));
    }

    fn update(&mut self, message: Message) -> Effects<Message> {
        // The notch's width sensor only measures on frames, and frames only run while
        // something animates; so after anything that can change the bar, run them briefly.
        let reshapes = match &message {
            Message::Ui(UiEvent::Clock(now)) => {
                now.format("%H%M").to_string() != self.snapshot.now.format("%H%M").to_string()
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
                | UiEvent::Stats(_)
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
                if state.reloads != self.snapshot.hypr.reloads && !self.game_mode {
                    self.reapply_hypr_rules();
                    // A reload (e.g. leaving game mode) drops unsaved settings previews.
                    if let Some(lua) = self.settings_ui.hypr.preview() {
                        self.send(Action::HyprPreview(lua));
                    }
                }
                self.snapshot.hypr = state;
                self.instant = Instant::now();
                self.animate_pills();
                self.sync_notches()
            }
            Message::Ui(UiEvent::Clock(now)) => {
                self.snapshot.now = now;
                Effects::none()
            }
            Message::Ui(UiEvent::Config(config)) => {
                if self.saved_at.is_some_and(|at| at.elapsed() < OWN_SAVE_ECHO) {
                    return Effects::none();
                }
                self.set_config(*config)
            }
            Message::Ui(UiEvent::ThemeColors(colors)) => {
                if colors == self.config.colors {
                    return Effects::none();
                }
                let mut config = self.config.clone();
                config.colors = colors;
                self.save_config(&config);
                self.set_config(config)
            }
            Message::Ui(UiEvent::Apps(apps)) => {
                self.launcher.set_apps(apps);
                Effects::none()
            }
            Message::Ui(UiEvent::Media(media)) => {
                let old_art = self.snapshot.media.as_ref().and_then(|m| m.art.clone());
                let new_art = media.as_ref().and_then(|m| m.art.clone());
                if old_art != new_art {
                    self.art = new_art;
                }
                self.snapshot.media = media;
                self.seek = None;
                Effects::none()
            }
            Message::Ui(UiEvent::Bars(bars)) => {
                self.snapshot.bars = bars;
                Effects::none()
            }
            Message::Ui(UiEvent::Toggle(panel)) | Message::Toggle(panel) => {
                self.toggle_panel(panel)
            }
            Message::MediaControl(control) => {
                self.send(Action::Media(control));
                Effects::none()
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
                        self.notif_images
                            .entry(n.id)
                            .or_insert_with(|| Image::Rgba {
                                width: *width,
                                height: *height,
                                data: data.clone(),
                            });
                    }
                }
                self.instant = Instant::now();
                self.animate_toasts(&state.popups);
                self.snapshot.notif = state;
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
                        let handle = Image::Rgba {
                            width: *width,
                            height: *height,
                            data: data.clone(),
                        };
                        self.tray_images.insert(item.key.clone(), handle);
                    }
                }
                self.snapshot.tray = items;
                Effects::none()
            }
            Message::Ui(UiEvent::TrayMenu(key, entries)) => {
                if self.tray_menu_for.as_ref() == Some(&key) {
                    self.tray_menu = Some((key, entries));
                }
                Effects::none()
            }
            Message::NotifDismiss(id) => self.notif_command(notifications::Command::Dismiss(id)),
            Message::NotifSwipe(id, offset) => {
                self.swiped = Some((id, offset));
                self.notif_command(notifications::Command::Dismiss(id))
            }
            Message::NotifRemove(id) => self.notif_command(notifications::Command::Remove(id)),
            Message::NotifClear => self.notif_command(notifications::Command::ClearHistory),
            Message::NotifAction(id, key) => {
                self.notif_command(notifications::Command::Action(id, key))
            }
            Message::NotifDnd(on) => {
                self.send(Action::Notif(notifications::Command::SetDnd(on)));
                let mut config = self.config.clone();
                config.notifications.dnd = on;
                self.save_config(&config);
                self.set_config(config)
            }
            Message::PopupHover(on) => {
                if on == self.popup_hover {
                    return Effects::none();
                }
                self.popup_hover = on;
                self.instant = Instant::now();
                self.popup_expand = self
                    .popup_expand
                    .clone()
                    .duration(self.config.animations.duration());
                self.popup_expand = self.popup_expand.clone().go(on, self.instant);
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
                Effects::none()
            }
            Message::Ui(UiEvent::ToggleSettings) | Message::ToggleSettings => {
                self.toggle_settings()
            }
            Message::Ui(UiEvent::Quit) => Effects::quit(),
            Message::Focus(id) => {
                self.send(Action::Hypr(hyprland::Command::Focus(id)));
                Effects::none()
            }
            Message::Escape if self.power_confirm.is_some() => {
                self.power_confirm = None;
                Effects::none()
            }
            Message::Escape if self.panels.current().is_some() => self.close_panel(),
            Message::Settings(settings::Msg::Close) | Message::Escape => {
                if self.surfaces.is_open(&Surface::Settings) {
                    self.stop_bt_scan();
                }
                self.surfaces.close(&Surface::Settings)
            }
            Message::Ui(UiEvent::Calendar(state)) => {
                self.settings_ui.cal_errors = state.errors.clone();
                self.snapshot.calendar = state;
                Effects::none()
            }
            Message::CalMonth(by) => {
                self.cal_month = calendar::shift_month(self.cal_month, by);
                Effects::none()
            }
            Message::CalDay(day) => {
                self.cal_day = day;
                Effects::none()
            }
            Message::OpenLink(url) => {
                self.send(Action::Launch(format!(
                    "xdg-open {}",
                    calendar::shell_quote(&url)
                )));
                self.close_panel()
            }
            Message::Ui(UiEvent::AudioDevices(d)) => {
                self.settings_ui.audio = d.clone();
                self.snapshot.audio = d;
                Effects::none()
            }
            Message::Ui(UiEvent::Volume(v)) => {
                // The first reading is the starting point, not a change worth an OSD.
                let changed = self.snapshot.volume.is_some() && v != self.snapshot.volume;
                self.snapshot.volume = v;
                match v.filter(|_| changed) {
                    Some(v) => self.show_osd(status::Osd::Volume(v)),
                    None => Effects::none(),
                }
            }
            Message::Ui(UiEvent::Brightness(Some(b))) => {
                // The first reading (at startup) just sets the level.
                let first = self.snapshot.brightness.is_none();
                self.snapshot.brightness = Some(b);
                if first {
                    Effects::none()
                } else {
                    self.show_osd(status::Osd::Brightness(b))
                }
            }
            Message::Ui(UiEvent::Clipboard(entries)) => {
                self.launcher.set_clips(entries);
                Effects::none()
            }
            Message::Power(action) => {
                let (run, confirm) = power_click(self.power_confirm, action);
                self.power_confirm = confirm;
                if !run {
                    return Effects::none();
                }
                self.send(Action::System(system::Command::Power {
                    action,
                    lock_command: self.config.power.lock_command.clone(),
                }));
                self.close_panel()
            }
            Message::WifiToggle(on) => {
                self.request_wifi(on);
                Effects::none()
            }
            Message::MicMuteToggle => {
                self.send(Action::Audio(audio::Command::Device(
                    audio::DeviceCommand::ToggleMicMute,
                )));
                Effects::none()
            }
            Message::NightLightToggle => {
                self.send(Action::System(system::Command::NightLight(
                    self.snapshot.night_light != Ok(true),
                )));
                Effects::none()
            }
            Message::KeepAwakeToggle => {
                self.send(Action::System(system::Command::KeepAwake(
                    self.snapshot.keep_awake != Ok(true),
                )));
                Effects::none()
            }
            Message::PowerProfileNext => {
                if let Some(next) = self
                    .snapshot
                    .power_profile
                    .as_ref()
                    .and_then(PowerProfile::next)
                {
                    self.send(Action::System(system::Command::PowerProfile(
                        next.to_owned(),
                    )));
                }
                Effects::none()
            }
            Message::Ui(UiEvent::NightLight(r)) => {
                self.snapshot.night_light = r;
                Effects::none()
            }
            Message::Ui(UiEvent::KeepAwake(r)) => {
                self.snapshot.keep_awake = r;
                Effects::none()
            }
            Message::Ui(UiEvent::PowerProfile(p)) => {
                self.snapshot.power_profile = p;
                Effects::none()
            }
            Message::BtPower(on) => {
                self.request_bt(on);
                Effects::none()
            }
            Message::GameModeToggle => self.set_game_mode(!self.game_mode),
            Message::QuickMicVolume(value) => {
                if value.is_finite() {
                    let percent = format!("{:.0}", value.clamp(0., 100.));
                    if let Ok(percent) = percent.parse::<u8>() {
                        self.send(Action::Audio(audio::Command::Device(
                            audio::DeviceCommand::SetMicVolume(percent),
                        )));
                    }
                }
                Effects::none()
            }
            Message::QuickVolume(v) => {
                self.send(Action::Audio(audio::Command::Volume(audio::Change::Set(
                    v.round() as u8,
                ))));
                Effects::none()
            }
            Message::QuickBrightness(v) => {
                self.send(Action::Audio(audio::Command::Brightness(
                    audio::Change::Set((v.round() as u8).max(1)),
                )));
                Effects::none()
            }
            Message::QuickScreenshot => self.update(Message::Capture(
                CaptureMode::Screenshot,
                capture::Target::Selection,
            )),
            Message::CaptureMode(CaptureMode::Text) => self.update(Message::Capture(
                CaptureMode::Text,
                capture::Target::Selection,
            )),
            Message::CaptureMode(mode) => {
                self.capture_mode = mode;
                Effects::none()
            }
            Message::Capture(mode, target) => self.close_panel().chain(Effects::later(
                capture_panel::SETTLE,
                Message::RunCapture(mode, target),
            )),
            Message::RunCapture(mode, target) => {
                let opts = capture_options(&self.config);
                self.send(match mode {
                    CaptureMode::Screenshot => Action::Screenshot(target, opts),
                    CaptureMode::Record => {
                        Action::Record(capture::RecordCommand::Start(target, opts))
                    }
                    CaptureMode::Text => Action::Ocr(opts),
                });
                Effects::none()
            }
            Message::Ui(UiEvent::Network(n)) => {
                // Joining or leaving changes the list's "Connected" marks.
                let key = |n: &Option<Network>| n.as_ref().map(|n| (n.connected, n.name.clone()));
                if self.settings_ui.page() == settings::Page::Wifi
                    && self.surfaces.is_open(&Surface::Settings)
                    && key(&n) != key(&self.snapshot.network)
                {
                    self.send(Action::System(system::Command::WifiScan));
                }
                self.settings_ui.network = n.clone();
                self.snapshot.network = n;
                let reported = self
                    .snapshot
                    .network
                    .as_ref()
                    .is_some_and(|n| n.wifi_enabled);
                self.wifi_pending = self.wifi_pending.and_then(|(requested, _, deadline)| {
                    (requested != reported).then_some((requested, reported, deadline))
                });
                if let Some((requested, _, _)) = self.wifi_pending {
                    self.set_wifi_shown(requested);
                }
                Effects::none()
            }
            Message::Ui(UiEvent::WifiNetworks(list)) => {
                self.settings_ui.wifi = list;
                Effects::none()
            }
            Message::Ui(UiEvent::WifiStatus(status)) => {
                self.settings_ui.wifi_status = status;
                Effects::none()
            }
            Message::Ui(UiEvent::Stats(s)) => {
                self.snapshot.record_stats(s);
                Effects::none()
            }
            Message::Ui(UiEvent::Battery(b)) => {
                self.snapshot.battery = b;
                Effects::none()
            }
            Message::SeekDrag(secs) => {
                self.seek = Some(secs);
                Effects::none()
            }
            Message::SeekCommit => {
                if let Some(secs) = self.seek {
                    self.send(Action::Media(Control::Seek(
                        std::time::Duration::from_secs_f32(secs.max(0.0)),
                    )));
                }
                // Keep showing the dragged spot until the player reports the new position.
                Effects::none()
            }
            Message::PanelSize(size) => {
                let fit = self
                    .panels
                    .measured(f32::from(size.width), f32::from(size.height));
                self.run_panel(fit.into_iter().collect())
            }
            Message::BarCursor(at) => {
                self.panels.pointer(at);
                Effects::none()
            }
            Message::BarWidth(monitor, width) => {
                self.panels.bar_width(monitor, width);
                Effects::none()
            }
            Message::VolumeMute => {
                self.send(Action::Audio(audio::Command::Volume(
                    audio::Change::ToggleMute,
                )));
                Effects::none()
            }
            Message::VolumeStep(step) => {
                let change = if step > 0 {
                    audio::Change::Up(step.unsigned_abs())
                } else {
                    audio::Change::Down(step.unsigned_abs())
                };
                self.send(Action::Audio(audio::Command::Volume(change)));
                Effects::none()
            }
            Message::Ui(UiEvent::Recording(r)) => {
                self.snapshot.recording = r;
                Effects::none()
            }
            Message::StopRecording => {
                self.send(Action::Record(capture::RecordCommand::Stop));
                Effects::none()
            }
            Message::GameModeOff => self.set_game_mode(false),
            Message::Ui(UiEvent::GameMode(want)) => {
                self.set_game_mode(want.unwrap_or(!self.game_mode))
            }
            Message::Ui(UiEvent::Weather(w)) => {
                self.weather_at = w.as_ref().map(|_| Instant::now());
                self.snapshot.weather = w;
                Effects::none()
            }
            Message::Ui(UiEvent::Bluetooth(state)) => {
                self.settings_ui.bt = state.clone();
                self.snapshot.bt = state;
                let reported = self.snapshot.bt.powered;
                self.bt_pending = self.bt_pending.and_then(|(requested, _, deadline)| {
                    (requested != reported).then_some((requested, reported, deadline))
                });
                if let Some((requested, _, _)) = self.bt_pending {
                    self.set_bt_shown(requested);
                }
                Effects::none()
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
                Effects::none()
            }
            Message::Ui(UiEvent::OpenBluetooth) => {
                self.open_settings_page(settings::Page::Bluetooth)
            }
            Message::OpenSettingsPage(page) => {
                let close = self.close_panel();
                Effects::batch([close, self.open_settings_page(page)])
            }
            Message::WifiDropdownPick(index) => {
                let pick = self.update(Message::Settings(settings::Msg::WifiPick(index)));
                if self.settings_ui.wifi_pick.is_none() {
                    return pick;
                }
                Effects::batch([
                    pick,
                    self.update(Message::OpenSettingsPage(settings::Page::Wifi)),
                ])
            }
            Message::Settings(msg) => {
                let mut config = self.config.clone();
                let was = self.settings_ui.page();
                let request = self.settings_ui.update(&mut config, msg);
                if was != settings::Page::Wifi && self.settings_ui.page() == settings::Page::Wifi {
                    self.send(Action::System(system::Command::WifiScan));
                }
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
                        settings::Request::Bluetooth(bluetooth::Command::Power(on)) => {
                            self.request_bt(on)
                        }
                        settings::Request::Bluetooth(cmd) => self.send(Action::Bluetooth(cmd)),
                        settings::Request::Audio(cmd) => {
                            self.send(Action::Audio(audio::Command::Device(cmd)))
                        }
                        settings::Request::WifiScan => {
                            self.send(Action::System(system::Command::WifiScan))
                        }
                        settings::Request::WifiPower(on) => self.request_wifi(on),
                        settings::Request::WifiConnect(net, pw) => {
                            self.send(Action::System(system::Command::WifiConnect(net, pw)))
                        }
                        settings::Request::WifiForget(p) => {
                            self.send(Action::System(system::Command::WifiForget(p)))
                        }
                        settings::Request::Hypr(request) => {
                            self.send(Action::Hypr(hyprland::Command::Settings(request)))
                        }
                        settings::Request::HyprPreview(lua) => self.send(Action::HyprPreview(lua)),
                        settings::Request::SetWallpaper(path) => {
                            self.send(Action::SetWallpaper(path));
                            if config.theme.source == config::ThemeSource::Wallpaper {
                                self.send(Action::ApplyTheme(Box::new(config.clone())));
                            }
                        }
                    }
                }
                if config == self.config {
                    return Effects::none();
                }
                self.save_config(&config);
                self.set_config(config)
            }
            Message::Ui(UiEvent::HyprSettings(result)) => {
                self.settings_ui.hypr.received(result);
                Effects::none()
            }
            Message::Ui(UiEvent::Wallpapers(list)) => {
                self.settings_ui.wallpapers = list;
                Effects::none()
            }
            Message::Ui(UiEvent::Status(status)) => {
                self.settings_ui.status = status;
                Effects::none()
            }
            Message::Launcher(msg) => match self.launcher.update(msg, &self.config) {
                Some(Outcome::Launch(command)) => {
                    self.send(Action::Launch(command));
                    self.close_panel()
                }
                Some(Outcome::Copy(text)) => {
                    Effects::batch([Effects::copy(text), self.close_panel()])
                }
                Some(Outcome::Clip(id)) => {
                    self.send(Action::Clipboard(clipboard::Command::Copy(id)));
                    self.close_panel()
                }
                Some(Outcome::Favorite(id)) => {
                    self.send(Action::Clipboard(clipboard::Command::ToggleFavorite(id)));
                    Effects::none()
                }
                Some(Outcome::ClearClips) => {
                    self.send(Action::Clipboard(clipboard::Command::Clear));
                    Effects::none()
                }
                None => Effects::none(),
            },
            Message::Nav(by) if matches!(self.panels.current(), Some(Panel::Launcher(_))) => {
                self.launcher.select(by);
                Effects::none()
            }
            Message::ClosePanel => self.close_panel(),
            Message::Frame(at) => {
                self.instant = at;
                if let Some((_, reported, deadline)) = self.wifi_pending
                    && at >= deadline
                {
                    self.wifi_pending = None;
                    self.set_wifi_shown(reported);
                }
                if let Some((_, reported, deadline)) = self.bt_pending
                    && at >= deadline
                {
                    self.bt_pending = None;
                    self.set_bt_shown(reported);
                }
                let osd = if at >= self.osd_until {
                    self.surfaces.close(&Surface::Osd)
                } else {
                    Effects::none()
                };
                self.toast_leave
                    .retain(|leaving| leaving.fade.is_animating(at));
                Effects::batch([self.sync_popups(), osd])
            }
            _ => Effects::none(),
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        let now = Instant::now();
        let animating = self.pills.values().any(|a| a.is_animating(now))
            || self.popup_expand.is_animating(now)
            || self.toast_motion.values().any(|m| m.is_animating(now))
            || !self.toast_leave.is_empty()
            || now < self.measure_until;
        if animating {
            return Some(now + std::time::Duration::from_millis(16));
        }
        let osd = self
            .surfaces
            .is_open(&Surface::Osd)
            .then(|| self.osd_until.max(now));
        let pending = [self.wifi_pending, self.bt_pending]
            .into_iter()
            .flatten()
            .map(|(_, _, deadline)| deadline.max(now));
        pending.chain(osd).min()
    }

    fn save_config(&mut self, config: &Config) {
        self.saved_at = Some(Instant::now());
        self.send(Action::SaveConfig(Box::new(config.clone())));
    }

    fn set_wifi_shown(&mut self, on: bool) {
        if let Some(n) = self.snapshot.network.as_mut() {
            n.wifi_enabled = on;
        }
        if let Some(n) = self.settings_ui.network.as_mut() {
            n.wifi_enabled = on;
        }
    }

    fn set_bt_shown(&mut self, on: bool) {
        self.snapshot.bt.powered = on;
        self.settings_ui.bt.powered = on;
    }

    fn request_wifi(&mut self, on: bool) {
        let reported = self.wifi_pending.map_or_else(
            || {
                self.snapshot
                    .network
                    .as_ref()
                    .is_some_and(|n| n.wifi_enabled)
            },
            |(_, reported, _)| reported,
        );
        self.wifi_pending = Some((on, reported, Instant::now() + PENDING_POWER_TIMEOUT));
        self.set_wifi_shown(on);
        self.send(Action::System(system::Command::Wifi(on)));
    }

    fn request_bt(&mut self, on: bool) {
        let reported = self
            .bt_pending
            .map_or(self.snapshot.bt.powered, |(_, reported, _)| reported);
        self.bt_pending = Some((on, reported, Instant::now() + PENDING_POWER_TIMEOUT));
        self.set_bt_shown(on);
        self.send(Action::Bluetooth(bluetooth::Command::Power(on)));
    }

    fn set_config(&mut self, config: Config) -> Effects<Message> {
        let (eq, was) = (&config.equalizer, &self.config.equalizer);
        if eq.bands != was.bands {
            self.send(Action::Equalizer(equalizer::Command::Gains(eq.bands)));
        }
        if eq.enabled != was.enabled {
            self.send(Action::Equalizer(if eq.enabled {
                equalizer::Command::Enable
            } else {
                equalizer::Command::Disable
            }));
        }
        if config.calendar != self.config.calendar {
            self.send(Action::Calendar(config.calendar.clone()));
        }
        if config.weather != self.config.weather {
            self.send(Action::Weather(weather_query(&config)));
        }
        let n = (&config.notifications, &self.config.notifications);
        if (n.0.timeout_ms, n.0.history) != (n.1.timeout_ms, n.1.history) {
            self.send(Action::Notif(notifications::Command::SetOptions(
                notifications::Options {
                    timeout: std::time::Duration::from_millis(n.0.timeout_ms),
                    history: n.0.history,
                },
            )));
        }
        let rebuild = config.bar.height != self.config.bar.height
            || config.bar.margin_top != self.config.bar.margin_top
            || config.bar.gap != self.config.bar.gap
            || config.bar.outputs != self.config.bar.outputs;
        if config.bar.look != self.config.bar.look && !self.game_mode {
            let square = config.bar.look == config::Look::Pixel;
            self.send(Action::Hypr(hyprland::Command::SquareCorners(square)));
            if !square {
                if config.bar.blur {
                    self.send(Action::Blur(true));
                }
                self.send_border_colors(&config.colors);
            }
        }
        if config.colors != self.config.colors {
            self.send_border_colors(&config.colors);
        }
        self.config = config;
        self.popup_expand = self
            .popup_expand
            .clone()
            .duration(self.config.animations.duration());
        self.pills.clear(); // picks up the new animation duration
        self.animate_pills();
        if !rebuild {
            return Effects::none();
        }
        // The notch height is baked into the layer surface; reopen them.
        let mut close: Vec<_> = self
            .surfaces
            .bars()
            .into_iter()
            .map(|m| self.surfaces.close(&Surface::Bar(m)))
            .collect();
        close.push(self.close_panel());
        Effects::batch(close).chain(self.sync_notches())
    }

    /// Opens a notch on every monitor that lacks one and closes orphans.
    fn sync_notches(&mut self) -> Effects<Message> {
        let outputs = &self.config.bar.outputs;
        let mut wanted: Vec<String> = self
            .snapshot
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
        for monitor in self.surfaces.bars() {
            if !wanted.contains(&monitor) {
                tasks.push(self.surfaces.close(&Surface::Bar(monitor)));
            }
        }
        if self
            .panels
            .monitor()
            .is_some_and(|m| !wanted.iter().any(|w| w == m))
        {
            tasks.push(self.close_panel()); // its monitor was unplugged
        }
        let h = self.config.bar.height + self.config.bar.margin_top;
        let gap = i32::try_from(self.config.bar.gap).unwrap_or(i32::MAX);
        for name in wanted {
            let output_option = if name.is_empty() {
                OutputOption::Active
            } else {
                OutputOption::OutputName(name.clone())
            };
            tasks.push(self.surfaces.open(
                Surface::Bar(name),
                WindowSpec {
                    size: Some((0, h)),
                    margin: Some((0, 0, gap, 0)),
                    output_option,
                    ..Default::default()
                },
            ));
        }
        Effects::batch(tasks)
    }

    fn toggle_settings(&mut self) -> Effects<Message> {
        if self.surfaces.is_open(&Surface::Settings) {
            self.stop_bt_scan();
            return self.surfaces.close(&Surface::Settings);
        }
        if self.settings_ui.page() == settings::Page::Bluetooth {
            self.send(Action::Bluetooth(bluetooth::Command::Scan(true)));
        }
        if self.settings_ui.page() == settings::Page::Wifi {
            self.send(Action::System(system::Command::WifiScan));
        }
        self.surfaces.open(
            Surface::Settings,
            WindowSpec {
                size: Some((860, 580)),
                output_option: OutputOption::Active,
                ..Default::default()
            },
        )
    }

    /// Opens `panel`, switches to it, or closes it if it's already open.
    fn toggle_panel(&mut self, panel: Panel) -> Effects<Message> {
        let commands = self.panels.toggle(panel, &self.focused_monitor());
        self.run_panel(commands)
    }

    fn close_panel(&mut self) -> Effects<Message> {
        let commands = self.panels.close();
        self.run_panel(commands)
    }

    /// A panel's top edge: just under the bar, ignoring the gap kept for windows.
    fn panel_top(&self) -> i32 {
        let bar = &self.config.bar;
        i32::try_from(bar.height.saturating_add(bar.margin_top))
            .unwrap_or(i32::MAX)
            .saturating_add(panel::GAP)
    }

    /// The focused monitor's name, for panels not opened from a bar.
    fn focused_monitor(&self) -> String {
        match self.snapshot.hypr.monitors.iter().find(|m| m.focused) {
            Some(m) => m.name.clone(),
            None => self.surfaces.bars().into_iter().next().unwrap_or_default(),
        }
    }

    /// Carries out the Panel module's commands on the layer surfaces.
    fn run_panel(&mut self, commands: Vec<panel::Command>) -> Effects<Message> {
        let mut tasks = Vec::new();
        for command in commands {
            match command {
                panel::Command::Close => {
                    tasks.push(self.surfaces.close(&Surface::Panel));
                    tasks.push(self.surfaces.close(&Surface::Catcher));
                }
                panel::Command::Resize { size, left } => {
                    tasks.push(self.surfaces.resize(&Surface::Panel, size));
                    tasks.push(
                        self.surfaces
                            .margin(&Surface::Panel, left.map(|_| panel_margin(left))),
                    );
                }
                panel::Command::Open {
                    panel,
                    monitor,
                    size,
                    left,
                } => {
                    let output_option = if monitor.is_empty() {
                        OutputOption::Active
                    } else {
                        OutputOption::OutputName(monitor)
                    };
                    tasks.push(self.surfaces.open(
                        Surface::Catcher,
                        WindowSpec {
                            size: Some((0, 0)),
                            output_option: output_option.clone(),
                            ..Default::default()
                        },
                    ));
                    tasks.push(self.surfaces.open(
                        Surface::Panel,
                        WindowSpec {
                            size: Some(size),
                            margin: left.map(|_| panel_margin(left)),
                            grabs_keyboard: panel.grabs_keyboard(),
                            output_option,
                            top: self.panel_top(),
                        },
                    ));
                    // After the surface exists, so the launcher can focus its search box.
                    tasks.push(self.on_open(panel));
                }
            }
        }
        // In order: a switch must close the old surfaces before opening new ones.
        tasks
            .into_iter()
            .fold(Effects::none(), |chain, task| chain.chain(task))
    }

    /// What each panel does as it opens.
    fn on_open(&mut self, panel: Panel) -> Effects<Message> {
        match panel {
            Panel::Launcher(kind) => {
                self.send(match kind {
                    Kind::Apps => Action::LoadApps,
                    Kind::Keybinds => Action::Hypr(hyprland::Command::Binds),
                    Kind::Clipboard => Action::Clipboard(clipboard::Command::Load),
                });
                self.launcher.reset(kind);
                return Effects::none();
            }
            Panel::Weather => {
                // Refresh on open if the forecast is getting old.
                if self
                    .weather_at
                    .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(600))
                {
                    self.send(Action::RefreshWeather);
                }
            }
            Panel::Calendar => {
                // Open on today, and fetch fresh events.
                let today = Local::now().date_naive();
                self.cal_day = today;
                self.cal_month = today.with_day(1).unwrap_or(today);
                self.send(Action::RefreshCalendar);
            }
            Panel::Power => self.power_confirm = None,
            Panel::Wifi => self.send(Action::System(system::Command::WifiScan)),
            Panel::Media
            | Panel::Notifications
            | Panel::TrayMenu
            | Panel::Quick
            | Panel::Bluetooth
            | Panel::Sound
            | Panel::Battery
            | Panel::System
            | Panel::Capture => {}
        }
        Effects::none()
    }

    /// Starts a transition for every pill whose active state changed.
    fn animate_pills(&mut self) {
        let duration = self.config.animations.duration();
        for m in &self.snapshot.hypr.monitors {
            for id in notch::workspace_ids(&self.config, &self.snapshot.hypr) {
                let active = m.active_workspace.id == id;
                let anim = self
                    .pills
                    .entry((m.name.clone(), id))
                    .or_insert_with(|| animation::Animation::new(active).duration(duration));
                if anim.value() != active {
                    anim.go_mut(active, self.instant);
                }
            }
        }
    }

    fn notif_command(&mut self, command: notifications::Command) -> Effects<Message> {
        self.send(Action::Notif(command));
        Effects::none()
    }

    /// Opens, resizes or closes the popup surface to fit the popups.
    fn sync_popups(&mut self) -> Effects<Message> {
        if self.snapshot.notif.popups.is_empty() && self.toast_leave.is_empty() {
            self.popup_size = 0;
            if self.popup_hover {
                self.popup_hover = false;
                self.popup_expand = animation::Animation::new(false);
                self.send(Action::Notif(notifications::Command::Hover(false)));
            }
            return self.surfaces.close(&Surface::Popups);
        }
        // Fanned-out size while hovered or animating, so the stack never gets cut off.
        let open = self.popup_hover || self.popup_expand.is_animating(self.instant);
        let mut height =
            notify::stack_height(&self.snapshot.notif.popups, if open { 1.0 } else { 0.0 }).ceil()
                as u32;
        if !self.toast_leave.is_empty() {
            height = height.max(self.popup_size);
        }
        let width = (notify::WIDTH + 2. * notify::PAD) as u32;
        if self.surfaces.is_open(&Surface::Popups) {
            if self.popup_size == height {
                return Effects::none();
            }
            self.popup_size = height;
            return self.surfaces.resize(&Surface::Popups, (width, height));
        }
        self.popup_size = height;
        let top = i32::try_from(self.config.bar.height)
            .unwrap_or(i32::MAX)
            .saturating_add(8);
        self.surfaces.open(
            Surface::Popups,
            WindowSpec {
                size: Some((width, height)),
                margin: Some((top, 12, 0, 12)),
                output_option: OutputOption::Active,
                ..Default::default()
            },
        )
    }

    /// Moves shown toasts to their new slots and starts exits for the ones that closed.
    fn animate_toasts(&mut self, popups: &[Notification]) {
        let now = self.instant;
        let duration = self.config.animations.duration();
        let swiped = self.swiped;
        if swiped.is_some_and(|(id, _)| !popups.iter().any(|p| p.id == id)) {
            self.swiped = None;
        }
        for (slot, n) in self
            .snapshot
            .notif
            .popups
            .iter()
            .take(notify::VISIBLE)
            .enumerate()
        {
            if popups.iter().any(|p| p.id == n.id) {
                continue;
            }
            let depth = self
                .toast_motion
                .get(&n.id)
                .map_or(slot as f32, |m| m.depth(now));
            self.toast_leave.push(notify::Leaving {
                notification: n.clone(),
                depth,
                swipe: swiped
                    .filter(|(id, _)| *id == n.id)
                    .map(|(_, offset)| offset),
                fade: animation::Animation::new(true)
                    .duration(duration)
                    .go(false, now),
            });
        }
        self.toast_motion
            .retain(|id, _| popups.iter().any(|n| n.id == *id));
        for (slot, n) in popups.iter().enumerate() {
            let slot = slot.min(notify::VISIBLE) as f32;
            match self.toast_motion.get_mut(&n.id) {
                Some(motion) => motion.move_to(slot, now),
                None => {
                    self.toast_motion
                        .insert(n.id, notify::Motion::new(slot, duration, now));
                }
            }
        }
    }

    fn tray_click(&mut self, key: String, click: tray::Click) -> Effects<Message> {
        let Some(item) = self.snapshot.tray.iter().find(|i| i.key == key) else {
            return Effects::none();
        };
        let wants_menu =
            click == tray::Click::Right || (click == tray::Click::Left && item.item_is_menu);
        if click == tray::Click::Middle || !wants_menu {
            let secondary = click == tray::Click::Middle;
            self.send(Action::Tray(TrayRequest::Activate { key, secondary }));
            return Effects::none();
        }
        if item.menu.is_none() {
            self.send(Action::Tray(TrayRequest::ContextMenu(key)));
            return Effects::none();
        }
        let same = self.tray_menu_for.as_ref() == Some(&key);
        self.tray_menu = None;
        self.tray_menu_for = Some(key.clone());
        self.send(Action::Tray(TrayRequest::LoadMenu(key)));
        if self.panels.current() == Some(Panel::TrayMenu) && !same {
            // Another app's menu: move it under that app's icon.
            let commands = self.panels.reopen(Panel::TrayMenu, &self.focused_monitor());
            return self.run_panel(commands);
        }
        self.toggle_panel(Panel::TrayMenu)
    }

    /// Shows (or refreshes) the OSD for 1.5 s.
    fn show_osd(&mut self, osd: status::Osd) -> Effects<Message> {
        self.osd = Some(osd);
        self.instant = Instant::now();
        self.osd_until = self.instant + std::time::Duration::from_millis(1500);
        self.surfaces.open(
            Surface::Osd,
            WindowSpec {
                size: Some((status::OSD_W, status::OSD_H)),
                margin: Some((0, 0, 90, 0)),
                output_option: OutputOption::Active,
                ..Default::default()
            },
        )
    }

    /// Game mode strips Hyprland's effects and silences notifications until it ends.
    fn set_game_mode(&mut self, on: bool) -> Effects<Message> {
        if on == self.game_mode {
            return Effects::none();
        }
        self.game_mode = on;
        self.send(Action::Hypr(hyprland::Command::GameMode(on)));
        if on {
            self.dnd_before_game = Some(self.snapshot.notif.dnd);
            self.send(Action::Notif(notifications::Command::SetDnd(true)));
        } else {
            let dnd = self
                .dnd_before_game
                .take()
                .unwrap_or(self.config.notifications.dnd);
            self.send(Action::Notif(notifications::Command::SetDnd(dnd)));
            // Leaving game mode reloads Hyprland's config, which drops runtime rules.
            self.reapply_hypr_rules();
        }
        Effects::none()
    }

    /// Applies blur, square corners and border colours again after Hyprland
    /// reloaded its config (which resets everything set at runtime).
    fn reapply_hypr_rules(&self) {
        if self.config.bar.blur {
            self.send(Action::Blur(true));
        }
        if self.config.bar.look == config::Look::Pixel {
            self.send(Action::Hypr(hyprland::Command::SquareCorners(true)));
        }
        self.send_border_colors(&self.config.colors);
    }

    /// Opens settings (if closed) on `page`.
    fn open_settings_page(&mut self, page: settings::Page) -> Effects<Message> {
        let open = if !self.surfaces.is_open(&Surface::Settings) {
            self.toggle_settings()
        } else {
            Effects::none()
        };
        if self.settings_ui.page() == page {
            return open;
        }
        Effects::batch([
            open,
            self.update(Message::Settings(settings::Msg::Page(page))),
        ])
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

/// Layer margin (top, right, bottom, left) for a panel at `left` (`None` = centered).
fn panel_margin(left: Option<i32>) -> (i32, i32, i32, i32) {
    (0, 0, 0, left.unwrap_or(0))
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
    fn reduced_motion_settles_popup_expansion() {
        let (actions, _) = tokio::sync::mpsc::unbounded_channel();
        let (_, events) = tokio::sync::mpsc::unbounded_channel();
        let mut config = Config::default();
        config.animations.reduce_motion = true;
        let (mut app, _) = App::new(config.clone(), events, actions, false);
        app.update(Message::PopupHover(true));
        assert_eq!(app.popup_expand.interpolate(0., 1., app.instant), 1.);
        assert!(!app.popup_expand.is_animating(app.instant));
        config.animations.reduce_motion = false;
        app.set_config(config.clone());
        app.update(Message::PopupHover(false));
        assert!(app.popup_expand.is_animating(app.instant));
        config.animations.reduce_motion = true;
        app.set_config(config);
        assert_eq!(app.popup_expand.interpolate(0., 1., app.instant), 0.);
        assert!(!app.popup_expand.is_animating(app.instant));
    }

    #[test]
    fn closed_toasts_animate_out_from_their_swipe() {
        let (mut app, _received) = test_app();
        let toast = |id| Notification {
            id,
            app_name: String::new(),
            summary: String::new(),
            body: String::new(),
            image: None,
            actions: Vec::new(),
            urgency: notifications::Urgency::Normal,
            time: Local::now(),
            resident: false,
        };
        let shown = |popups| {
            Message::Ui(UiEvent::Notifications(notifications::State {
                popups,
                ..Default::default()
            }))
        };
        app.update(shown(vec![toast(2), toast(1)]));
        assert_eq!(app.toast_motion.len(), 2);
        app.update(Message::NotifSwipe(2, 60.));
        app.update(shown(vec![toast(3), toast(2), toast(1)]));
        assert!(
            app.toast_leave.is_empty(),
            "a new toast must not consume the swipe"
        );
        app.update(shown(vec![toast(3), toast(1)]));
        let [leaving] = app.toast_leave.as_slice() else {
            panic!("one toast should be leaving");
        };
        assert_eq!(leaving.notification.id, 2);
        assert_eq!(leaving.swipe, Some(60.));
        assert!(app.toast_motion.contains_key(&1) && !app.toast_motion.contains_key(&2));
    }

    fn test_app() -> (App, tokio::sync::mpsc::UnboundedReceiver<Action>) {
        let (actions, received) = tokio::sync::mpsc::unbounded_channel();
        let (_, events) = tokio::sync::mpsc::unbounded_channel();
        let (app, _) = App::new(Config::default(), events, actions, false);
        (app, received)
    }

    #[test]
    fn microphone_slider_routes_bounded_values_and_ignores_invalid_input() {
        let (mut app, mut rx) = test_app();
        while rx.try_recv().is_ok() {}
        app.update(Message::QuickMicVolume(140.));
        assert!(matches!(
            rx.try_recv(),
            Ok(Action::Audio(audio::Command::Device(
                audio::DeviceCommand::SetMicVolume(100)
            )))
        ));
        app.update(Message::QuickMicVolume(-10.));
        assert!(matches!(
            rx.try_recv(),
            Ok(Action::Audio(audio::Command::Device(
                audio::DeviceCommand::SetMicVolume(0)
            )))
        ));
        app.update(Message::QuickMicVolume(f32::NAN));
        app.update(Message::QuickMicVolume(f32::INFINITY));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn panel_capture_waits_for_the_panel_to_close() {
        let (mut app, mut rx) = test_app();
        app.update(Message::Toggle(Panel::Capture));
        assert_eq!(app.panels.current(), Some(Panel::Capture));
        app.update(Message::Capture(
            CaptureMode::Screenshot,
            capture::Target::Screen,
        ));
        assert_eq!(app.panels.current(), None);
        let early: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            !early.iter().any(|a| matches!(
                a,
                Action::Screenshot(..) | Action::Record(_) | Action::Ocr(_)
            )),
            "{early:?}"
        );
        app.update(Message::RunCapture(
            CaptureMode::Screenshot,
            capture::Target::Screen,
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(Action::Screenshot(capture::Target::Screen, _))
        ));
        assert_eq!(app.panels.current(), None);
    }

    #[test]
    fn text_mode_starts_ocr_and_keeps_the_previous_mode() {
        let (mut app, _rx) = test_app();
        app.update(Message::CaptureMode(CaptureMode::Record));
        assert_eq!(app.capture_mode, CaptureMode::Record);
        app.update(Message::CaptureMode(CaptureMode::Text));
        assert_eq!(app.capture_mode, CaptureMode::Record);
    }

    #[test]
    fn record_run_starts_not_toggles() {
        let (mut app, mut rx) = test_app();
        app.update(Message::RunCapture(
            CaptureMode::Record,
            capture::Target::Window,
        ));
        assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|a| matches!(
            a,
            Action::Record(capture::RecordCommand::Start(capture::Target::Window, _))
        )));
    }

    #[test]
    fn bluetooth_power_is_shown_at_once_and_reverts_without_a_report() {
        let (mut app, _received) = test_app();
        app.snapshot.bt.available = true;
        app.snapshot.bt.powered = false;
        app.update(Message::BtPower(true));
        assert!(app.snapshot.bt.powered && app.settings_ui.bt.powered);
        let stale = app.snapshot.bt.clone();
        app.update(Message::Ui(UiEvent::Bluetooth(
            rune_services::bluetooth::State {
                powered: false,
                ..stale
            },
        )));
        assert!(
            app.snapshot.bt.powered,
            "a stale report must not bounce the switch"
        );
        app.update(Message::Frame(Instant::now() + Duration::from_secs(4)));
        assert!(!app.snapshot.bt.powered && !app.settings_ui.bt.powered);
    }

    #[test]
    fn own_save_echo_does_not_undo_a_newer_toggle() {
        let (mut app, _received) = test_app();
        let stale = app.config.clone();
        app.update(Message::Settings(settings::Msg::Flag(
            settings::Flag::ThemeDark,
            !stale.theme.dark,
        )));
        app.update(Message::Ui(UiEvent::Config(Box::new(stale.clone()))));
        assert_eq!(app.config.theme.dark, !stale.theme.dark);
        let mut colors = app.config.colors;
        colors.accent = config::Rgba([1, 2, 3, 255]);
        app.update(Message::Ui(UiEvent::ThemeColors(colors)));
        assert_eq!(app.config.colors.accent, colors.accent);
        assert_eq!(app.config.theme.dark, !stale.theme.dark);
    }

    #[test]
    fn top_gap_grows_the_bar_surface() {
        let (mut app, _received) = test_app();
        let mut config = app.config.clone();
        config.bar.margin_top = 6;
        let _ = app.set_config(config);
        assert_eq!(
            app.surfaces.desired[&Surface::Bar(String::new())].size,
            Some((0, 38))
        );
    }

    #[test]
    fn hyprland_reload_restores_square_corners_and_rules() {
        let (mut app, mut received) = test_app();
        let mut config = app.config.clone();
        config.bar.look = config::Look::Pixel;
        config.bar.blur = true;
        let _ = app.set_config(config);
        while received.try_recv().is_ok() {}
        let mut state = app.snapshot.hypr.clone();
        state.reloads += 1;
        app.update(Message::Ui(UiEvent::Hypr(state.clone())));
        let mut sent = Vec::new();
        while let Ok(action) = received.try_recv() {
            sent.push(action);
        }
        assert!(sent.contains(&Action::Hypr(hyprland::Command::SquareCorners(true))));
        assert!(sent.contains(&Action::Blur(true)));
        // Ordinary workspace/monitor updates must not resend them.
        app.update(Message::Ui(UiEvent::Hypr(state)));
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn equalizer_edits_reach_the_service() {
        let (mut app, mut received) = test_app();
        let mut config = app.config.clone();
        config.equalizer.enabled = true;
        config.equalizer.bands[0] = 4.;
        let _ = app.set_config(config);
        let mut sent = Vec::new();
        while let Ok(action) = received.try_recv() {
            if let Action::Equalizer(cmd) = action {
                sent.push(cmd);
            }
        }
        let mut bands = [0.; 10];
        bands[0] = 4.;
        assert_eq!(
            sent,
            [equalizer::Command::Gains(bands), equalizer::Command::Enable]
        );
    }

    #[test]
    fn panels_open_just_below_the_bar_whatever_the_window_gap() {
        let (mut app, _received) = test_app();
        let mut config = app.config.clone();
        config.bar.height = 30;
        config.bar.margin_top = 4;
        config.bar.gap = 24;
        let _ = app.set_config(config);
        app.update(Message::Toggle(Panel::Quick));
        assert_eq!(
            app.surfaces.desired[&Surface::Panel].top,
            30 + 4 + panel::GAP
        );
    }

    #[test]
    fn centered_panel_stays_centered_after_measuring() {
        let (mut app, _received) = test_app();
        app.update(Message::Toggle(Panel::Quick));
        app.update(Message::PanelSize(gpui::size(
            gpui::px(350.),
            gpui::px(400.),
        )));
        assert_eq!(app.surfaces.desired[&Surface::Panel].margin, None);
    }

    #[test]
    fn wifi_dropdown_scans_on_open() {
        let (mut app, mut received) = test_app();
        app.update(Message::Toggle(Panel::Wifi));
        let mut scanned = false;
        while let Ok(action) = received.try_recv() {
            scanned |= matches!(action, Action::System(system::Command::WifiScan));
        }
        assert!(scanned);
    }

    #[test]
    fn wifi_dropdown_hands_secured_network_to_settings() {
        let (mut app, _received) = test_app();
        app.settings_ui.wifi = vec![system::WifiNetwork {
            ssid: "Office".into(),
            strength: 70,
            secured: true,
            active: false,
            saved: None,
            device: "wlan0".into(),
            ap: "/ap/1".into(),
        }];
        app.update(Message::Toggle(Panel::Wifi));
        app.update(Message::WifiDropdownPick(0));
        assert_eq!(app.settings_ui.wifi_pick.as_deref(), Some("Office"));
        assert_eq!(app.settings_ui.page(), settings::Page::Wifi);
        assert!(app.surfaces.is_open(&Surface::Settings));
        assert_eq!(app.panels.current(), None);
    }

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
