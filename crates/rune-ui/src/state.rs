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
    /// Recent samples, oldest first, at most [`STATS_HISTORY`].
    pub stats_history: std::collections::VecDeque<rune_services::stats::Stats>,
    pub network: Option<Network>,
    pub battery: Option<Battery>,
    pub brightness: Option<u8>,
    pub calendar: cal::State,
    pub updates: rune_services::updates::Updates,
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
            stats_history: std::collections::VecDeque::with_capacity(STATS_HISTORY),
            network: None,
            battery: None,
            brightness: None,
            calendar: cal::State::default(),
            updates: rune_services::updates::Updates::default(),
        }
    }
}

/// Samples kept for the system panel's graphs (two minutes at the service's rate).
pub(crate) const STATS_HISTORY: usize = 60;

impl Snapshot {
    pub(crate) fn record_stats(&mut self, stats: Option<rune_services::stats::Stats>) {
        if let Some(sample) = &stats {
            if self.stats_history.len() >= STATS_HISTORY {
                self.stats_history.pop_front();
            }
            self.stats_history.push_back(sample.clone());
        }
        self.stats = stats;
    }
}
