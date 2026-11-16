//! `config.toml` schema. Every field has a default, so an empty file is valid.
//! The settings window edits a `Config` and writes it back with [`Config::to_toml`].

use serde::{Deserialize, Serialize};

/// Errors from parsing `config.toml`.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    #[error("clock.format {0:?} is not a valid strftime format")]
    ClockFormat(String),
}

/// Whole shell configuration.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bar: BarConfig,
    pub workspaces: WorkspacesConfig,
    pub clock: ClockConfig,
    pub window_title: WindowTitleConfig,
    pub animations: AnimationsConfig,
    pub launcher: LauncherConfig,
    pub notifications: NotificationsConfig,
    pub weather: WeatherConfig,
    pub capture: CaptureConfig,
    pub calendar: CalendarConfig,
    pub power: PowerConfig,
    pub theme: ThemeConfig,
    pub wallpaper: WallpaperConfig,
    pub colors: Colors,
}

impl Config {
    /// Parses and validates a TOML document.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text)?;
        if !valid_clock_format(&config.clock.format) {
            return Err(ConfigError::ClockFormat(config.clock.format));
        }
        Ok(config)
    }

    /// Serializes back to TOML (comments in the original file are not kept).
    pub fn to_toml(&self) -> String {
        // A plain struct of strings, numbers and lists always serializes.
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

/// chrono panics when displaying a bad format, so formats are checked first.
pub fn valid_clock_format(format: &str) -> bool {
    chrono::format::StrftimeItems::new(format).parse().is_ok()
}

/// A bar widget, referenced by name in `bar.left/center/right`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Widget {
    Workspaces,
    WindowTitle,
    Clock,
    /// Gear button that opens the settings window.
    Settings,
    /// Album art and visualizer; click to expand the player.
    Media,
    /// Bell with a count; opens the notification center.
    Notifications,
    /// System tray icons.
    Tray,
    /// Current conditions; click for the forecast.
    Weather,
    /// Bluetooth status; click for devices.
    Bluetooth,
    /// Output volume: click to mute, scroll to change.
    Volume,
    /// Wi-Fi or wired status.
    Network,
    /// Battery level (hidden on machines without one).
    Battery,
    /// CPU use, memory use and CPU temperature.
    Stats,
    /// Opens quick settings (Wi-Fi, Bluetooth, DND, volume...).
    QuickSettings,
    /// Opens the power menu.
    Power,
}

impl Widget {
    pub const ALL: [Widget; 15] = [
        Widget::Workspaces,
        Widget::WindowTitle,
        Widget::Clock,
        Widget::Media,
        Widget::Notifications,
        Widget::Tray,
        Widget::Weather,
        Widget::Bluetooth,
        Widget::Volume,
        Widget::Network,
        Widget::Battery,
        Widget::Stats,
        Widget::QuickSettings,
        Widget::Power,
        Widget::Settings,
    ];
}

impl std::fmt::Display for Widget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Widget::Workspaces => "Workspaces",
            Widget::WindowTitle => "Window title",
            Widget::Clock => "Clock",
            Widget::Settings => "Settings button",
            Widget::Media => "Media",
            Widget::Notifications => "Notifications",
            Widget::Tray => "System tray",
            Widget::Weather => "Weather",
            Widget::Bluetooth => "Bluetooth",
            Widget::Volume => "Volume",
            Widget::Network => "Network",
            Widget::Battery => "Battery",
            Widget::Stats => "System stats",
            Widget::QuickSettings => "Quick settings",
            Widget::Power => "Power menu",
        })
    }
}

/// `[bar]`: the notch at the top center of each monitor. Sizes are logical pixels.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BarConfig {
    pub height: u32,
    /// Bottom corner radius; also the size of the concave top shoulders.
    pub radius: f32,
    /// Space between the notch sides and the outermost widgets.
    pub padding: f32,
    /// Space between widgets.
    pub spacing: f32,
    pub font: String,
    pub font_size: f32,
    /// Blur what's behind the notch and panels (visible when `colors.background` has transparency).
    pub blur: bool,
    /// Icon font; icons are written by name, e.g. `"schedule"`.
    pub icon_font: String,
    /// Connector names (`DP-1`) to show a bar on. Empty means every monitor.
    pub outputs: Vec<String>,
    pub left: Vec<Widget>,
    pub center: Vec<Widget>,
    pub right: Vec<Widget>,
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            height: 32,
            radius: 14.0,
            padding: 6.0,
            spacing: 10.0,
            font: "Noto Sans".into(),
            font_size: 13.0,
            icon_font: "Material Symbols Rounded".into(),
            blur: false,
            outputs: Vec::new(),
            left: vec![Widget::Workspaces],
            center: vec![Widget::Media, Widget::WindowTitle],
            right: vec![
                Widget::Tray,
                Widget::Bluetooth,
                Widget::Network,
                Widget::Volume,
                Widget::Battery,
                Widget::Weather,
                Widget::Clock,
                Widget::Notifications,
                Widget::QuickSettings,
            ],
        }
    }
}

/// `[workspaces]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspacesConfig {
    /// Workspaces `1..=min` are always shown, even when empty.
    pub min: i32,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self { min: 5 }
    }
}

/// `[clock]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClockConfig {
    /// strftime format, see <https://docs.rs/chrono/latest/chrono/format/strftime>.
    pub format: String,
    /// Icon name shown before the time; empty for none.
    pub icon: String,
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            format: "%a %d %b  %H:%M".into(),
            icon: "schedule".into(),
        }
    }
}

/// `[window_title]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowTitleConfig {
    /// Longer titles are cut with an ellipsis.
    pub max_chars: usize,
}

impl Default for WindowTitleConfig {
    fn default() -> Self {
        Self { max_chars: 50 }
    }
}

/// `[notifications]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationsConfig {
    /// How long popups stay when the app doesn't say.
    pub timeout_ms: u64,
    /// Notifications kept in the notification center.
    pub history: usize,
    /// Do not disturb: new notifications skip the popups (critical ones still show).
    pub dnd: bool,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            history: 100,
            dnd: false,
        }
    }
}

/// `[weather]`: set from Settings → Weather (a place search fills these in).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeatherConfig {
    /// Place name shown in the forecast; empty turns weather off.
    pub location: String,
    pub latitude: f64,
    pub longitude: f64,
    /// °F and mph instead of °C and km/h.
    pub imperial: bool,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            location: String::new(),
            latitude: 0.0,
            longitude: 0.0,
            imperial: false,
        }
    }
}

/// `[calendar]`: read-only ICS links (Google: calendar settings → "Secret address in iCal format").
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CalendarConfig {
    pub urls: Vec<String>,
}

/// `[power]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PowerConfig {
    /// Run by "Lock" (`loginctl lock-session` hands off to hypridle's lock_cmd).
    pub lock_command: String,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            lock_command: "loginctl lock-session".into(),
        }
    }
}

/// `[capture]`: screenshots, recordings and text extraction.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureConfig {
    pub screenshot_dir: String,
    /// Also copy screenshots to the clipboard.
    pub copy: bool,
    pub record_dir: String,
    /// Record what the speakers play.
    pub record_audio: bool,
    pub fps: u32,
    /// Tesseract language(s) for text extraction, e.g. "eng" or "eng+deu".
    pub ocr_lang: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            screenshot_dir: "~/Pictures/Screenshots".into(),
            copy: true,
            record_dir: "~/Videos/Recordings".into(),
            record_audio: true,
            fps: 60,
            ocr_lang: "eng".into(),
        }
    }
}

/// `~/x` → `$HOME/x`.
pub fn expand_home(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
        _ => path.into(),
    }
}

/// `[launcher]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LauncherConfig {
    /// Runs apps marked `Terminal=true` as `<terminal> -e <command>`.
    pub terminal: String,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            terminal: "kitty".into(),
        }
    }
}

/// Where theme colors come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSource {
    #[default]
    Preset,
    Wallpaper,
}

/// `[theme]`: applying a theme rewrites `[colors]` and the enabled apps' theme files.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub source: ThemeSource,
    /// Preset name, e.g. "Catppuccin Mocha".
    pub preset: String,
    /// Material You scheme for wallpaper themes: tonal_spot, vibrant, expressive,
    /// fruit_salad, content, fidelity, rainbow, neutral, monochrome.
    pub scheme: String,
    /// Wallpaper themes only; presets have their own.
    pub dark: bool,
    /// Apps to theme: kitty, ghostty, btop, yazi, nvim, zed, vscode, kiro, gtk3, gtk4, qt, kde.
    pub apps: Vec<String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            source: ThemeSource::Preset,
            preset: "Catppuccin Mocha".into(),
            scheme: "tonal_spot".into(),
            dark: true,
            apps: Vec::new(),
        }
    }
}

/// `[wallpaper]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WallpaperConfig {
    /// Folder the picker shows; `~` is expanded.
    pub dir: String,
    /// Last wallpaper set from Rune (also the source for wallpaper themes).
    pub current: String,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            dir: "~/Pictures/Wallpapers".into(),
            current: String::new(),
        }
    }
}

impl WallpaperConfig {
    /// `dir` with `~` expanded.
    pub fn dir_path(&self) -> std::path::PathBuf {
        expand_home(&self.dir)
    }
}

/// `[animations]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnimationsConfig {
    pub enabled: bool,
    pub duration_ms: u64,
    /// Accessibility: turns every animation off, whatever `enabled` says.
    pub reduce_motion: bool,
}

impl Default for AnimationsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_ms: 250,
            reduce_motion: false,
        }
    }
}

impl AnimationsConfig {
    /// Duration to animate with; zero means "jump straight to the end".
    pub fn duration(&self) -> std::time::Duration {
        let on = self.enabled && !self.reduce_motion;
        std::time::Duration::from_millis(if on { self.duration_ms } else { 0 })
    }
}

/// `[colors]`, as `#rrggbb` or `#rrggbbaa`. Defaults use the graphite palette.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    pub background: Rgba,
    pub foreground: Rgba,
    /// Active workspace pill, highlights.
    pub accent: Rgba,
    /// Text drawn on top of `accent` and `urgent`.
    pub on_accent: Rgba,
    /// Empty workspaces, secondary text.
    pub muted: Rgba,
    pub urgent: Rgba,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            background: Rgba([0x20, 0x22, 0x27, 0xff]),
            foreground: Rgba([0xf5, 0xf5, 0xf7, 0xff]),
            accent: Rgba([0x66, 0xb1, 0xff, 0xff]),
            on_accent: Rgba([0x10, 0x1a, 0x28, 0xff]),
            muted: Rgba([0xa6, 0xaa, 0xb5, 0xff]),
            urgent: Rgba([0xff, 0x7b, 0x81, 0xff]),
        }
    }
}

/// Straight (non-premultiplied) RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgba(pub [u8; 4]);

impl TryFrom<String> for Rgba {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        Self::parse(&s).ok_or_else(|| format!("invalid color {s:?}, expected #rrggbb or #rrggbbaa"))
    }
}

impl From<Rgba> for String {
    fn from(c: Rgba) -> String {
        let [r, g, b, a] = c.0;
        if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

impl Rgba {
    /// Parses `#rrggbb` / `#rrggbbaa` (the `#` is optional).
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        let alpha = if hex.len() == 8 { byte(6)? } else { 0xff };
        Some(Self([byte(0)?, byte(2)?, byte(4)?, alpha]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn example_config_matches_defaults() {
        let example = include_str!("../../../examples/config.toml");
        assert_eq!(Config::parse(example).unwrap(), Config::default());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut c = Config::default();
        c.colors.background = Rgba([1, 2, 3, 4]);
        c.bar.left = vec![Widget::Settings, Widget::Clock];
        assert_eq!(Config::parse(&c.to_toml()).unwrap(), c);
    }

    #[test]
    fn parses_overrides() {
        let c = Config::parse(
            r##"
            [bar]
            left = ["clock", "workspaces"]
            right = []
            [colors]
            accent = "#89b4fa"
            background = "#11111b80"
            [animations]
            reduce_motion = true
            "##,
        )
        .unwrap();
        assert_eq!(c.bar.left, [Widget::Clock, Widget::Workspaces]);
        assert!(c.bar.right.is_empty());
        assert_eq!(c.colors.accent, Rgba([0x89, 0xb4, 0xfa, 0xff]));
        assert_eq!(c.colors.background, Rgba([0x11, 0x11, 0x1b, 0x80]));
        assert_eq!(c.bar.height, 32); // untouched fields keep defaults
        assert!(c.animations.duration().is_zero());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Config::parse("[bar]\nleft = [\"nope\"]").is_err());
        assert!(Config::parse("[bar]\nhieght = 3").is_err());
        assert!(Config::parse("[colors]\naccent = \"#12345\"").is_err());
        assert!(Config::parse("[colors]\naccent = \"#ééé\"").is_err());
        assert!(matches!(
            Config::parse("[clock]\nformat = \"%Q\""),
            Err(ConfigError::ClockFormat(_))
        ));
    }
}
