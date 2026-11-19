//! Backend services for Rune Shell. Each service runs on tokio and publishes its
//! state through a `tokio::sync::watch` channel; the UI only ever reads those.

pub mod apps;
pub mod art;
pub mod audio;
pub mod bluetooth;
pub mod calc;
pub mod calendar;
pub mod capture;
pub mod clipboard;
pub mod clock;
pub mod equalizer;
pub mod hyprland;
pub mod hyprland_config;
pub mod icons;
pub mod lock;
pub mod media;
pub mod notifications;
pub mod stats;
pub mod system;
pub mod theme;
pub mod tray;
pub mod updates;
pub mod visualizer;
pub mod wallpaper;
pub mod weather;
