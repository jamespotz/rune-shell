//! Service snapshots shared by all shell windows, independent of the UI toolkit.

use chrono::{DateTime, Local};
use rune_services::{
    audio, bluetooth, calendar as cal, capture,
    hyprland::HyprState,
    media::Media,
    notifications,
    system::{Battery, Network, PowerProfile},
    tray::TrayItem,
    weather::Weather,
};

pub(crate) struct Snapshot {
    pub hypr: HyprState,
    pub now: DateTime<Local>,
    pub media: Option<Media>,
    pub bars: Vec<u8>,
    pub notif: notifications::State,
    pub tray: Vec<TrayItem>,
    pub weather: Option<Weather>,
    pub bt: bluetooth::State,
    pub recording: Option<capture::Recording>,
    pub volume: Option<audio::Volume>,
    pub audio: audio::Devices,
    pub night_light: Result<bool, String>,
    pub keep_awake: Result<bool, String>,
    pub power_profile: Option<PowerProfile>,
    pub stats: Option<rune_services::stats::Stats>,
    pub network: Option<Network>,
    pub battery: Option<Battery>,
    pub brightness: Option<u8>,
    pub calendar: cal::State,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            hypr: HyprState::default(),
            now: Local::now(),
            media: None,
            bars: Vec::new(),
            notif: notifications::State::default(),
            tray: Vec::new(),
            weather: None,
            bt: bluetooth::State::default(),
            recording: None,
            volume: None,
            audio: audio::Devices::default(),
            night_light: Ok(false),
            keep_awake: Ok(false),
            power_profile: None,
            stats: None,
            network: None,
            battery: None,
            brightness: None,
            calendar: cal::State::default(),
        }
    }
}
