//! Toolkit-independent messages exchanged between the daemon and UI.

use crate::{Config, Panel};
use chrono::{DateTime, Local};
use rune_services::{
    apps::App as DesktopApp,
    audio, bluetooth, calendar as cal, capture, clipboard, equalizer,
    hyprland::{self, Bind, HyprState},
    media::{Control, Media},
    notifications, polkit,
    system::{self, Battery, Network, NetworkList, Peripheral, PowerProfile},
    tray::{MenuEntry, TrayItem},
    updates,
    weather::{Location, Query as WeatherQuery, Weather},
};

/// Something the user asked for; the daemon carries it out.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Workspaces, game mode, keybinds; keybinds answer with [`UiEvent::Binds`].
    Hypr(hyprland::Command),
    /// Lua to run in Hyprland as a live settings preview. Only the latest one
    /// matters, so the daemon drops any it hasn't sent yet.
    HyprPreview(String),
    /// Wi-Fi, power profile, keep awake, night light, power menu.
    System(system::Command),
    /// Volume, audio devices, brightness.
    Audio(audio::Command),
    Equalizer(equalizer::Command),
    /// Clipboard history; answered with [`UiEvent::Clipboard`].
    Clipboard(clipboard::Command),
    /// Write this config to `config.toml` (sent by the settings window).
    SaveConfig(Box<Config>),
    /// Run a shell command, detached from the shell.
    Launch(String),
    /// Rescan applications; answered with [`UiEvent::Apps`].
    LoadApps,
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
    /// Check for updates or run topgrade; answered with [`UiEvent::Updates`].
    Updates(updates::Command),
    Focus(rune_services::focus::Command),
    Screenshot(capture::Target, capture::Options),
    /// Extract text from a selected region.
    Ocr(capture::Options),
    Record(capture::RecordCommand),
    /// Calendar links to follow and the reminder lead time.
    Calendar(crate::config::CalendarConfig),
    RefreshCalendar,
    /// Answer or dismiss the open polkit authentication.
    Polkit(polkit::Command),
}

impl From<system::Reply> for UiEvent {
    fn from(reply: system::Reply) -> Self {
        match reply {
            system::Reply::Networks(list) => Self::Networks(Box::new(list)),
            system::Reply::NetworkStatus(status) => Self::NetworkStatus(status),
            system::Reply::KeepAwake(state) => Self::KeepAwake(state),
            system::Reply::NightLight(state) => Self::NightLight(state),
        }
    }
}

impl From<audio::Reply> for UiEvent {
    fn from(audio::Reply::Brightness(level): audio::Reply) -> Self {
        Self::Brightness(level)
    }
}

impl From<hyprland::Reply> for UiEvent {
    fn from(reply: hyprland::Reply) -> Self {
        match reply {
            hyprland::Reply::Binds(binds) => Self::Binds(binds),
            hyprland::Reply::Settings(result) => Self::HyprSettings(result),
        }
    }
}

impl From<clipboard::Reply> for UiEvent {
    fn from(clipboard::Reply::Entries(entries): clipboard::Reply) -> Self {
        Self::Clipboard(entries)
    }
}

/// Input from the daemon.
#[derive(Debug, Clone)]
pub enum UiEvent {
    Hypr(HyprState),
    Clock(DateTime<Local>),
    Config(Box<Config>),
    ThemeColors(crate::config::Colors),
    Apps(Vec<DesktopApp>),
    Binds(Vec<Bind>),
    /// Live Hyprland settings for the settings window, or why a request failed.
    HyprSettings(Result<Box<rune_services::hyprland_config::Snapshot>, String>),
    Media(Option<Media>),
    /// Visualizer bars, 0..=100 each; empty while nothing plays.
    Bars(Vec<u8>),
    /// Open, switch to, or close a panel.
    Toggle(Panel),
    ToggleSettings,
    /// Open settings on the Bluetooth page.
    OpenBluetooth,
    Weather(Option<Weather>),
    Bluetooth(bluetooth::State),
    Updates(updates::Updates),
    Focus(rune_services::focus::State),
    Geocoded(Result<Vec<Location>, String>),
    Recording(Option<capture::Recording>),
    Calendar(cal::State),
    Volume(Option<audio::Volume>),
    AudioDevices(audio::Devices),
    /// New backlight level after a brightness change (shows the OSD).
    Brightness(Option<u8>),
    Network(Option<Network>),
    Networks(Box<NetworkList>),
    /// Whether night light / keep awake is now on, or why it couldn't start.
    NightLight(Result<bool, String>),
    KeepAwake(Result<bool, String>),
    PowerProfile(Option<PowerProfile>),
    /// Result of joining or connecting (empty on success).
    NetworkStatus(String),
    Battery(Option<Battery>),
    Peripherals(Vec<Peripheral>),
    Stats(Option<rune_services::stats::Stats>),
    Clipboard(Vec<clipboard::Entry>),
    /// Turn game mode on, off, or (`None`) toggle it.
    GameMode(Option<bool>),
    Notifications(notifications::State),
    Tray(Vec<TrayItem>),
    /// An app's tray menu (item key, entries), answering [`TrayRequest::LoadMenu`].
    TrayMenu(String, Vec<MenuEntry>),
    Wallpapers(Vec<rune_services::wallpaper::Wallpaper>),
    /// The background picture for the Lock Screen preview, effect applied.
    LockPreview(Option<std::path::PathBuf>),
    /// The polkit authentication to show, or `None` to close the dialog.
    Polkit(Option<polkit::Request>),
    /// Outcome of a theme or wallpaper job, shown in settings.
    Status(String),
    Quit,
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
