//! Settings window: a sidebar of categories and a page of controls per category.
//! Every edit updates the config immediately; `App` saves it to disk.

use crate::config::{Config, Rgba, ThemeSource, Widget, valid_clock_format};
use rune_services::{
    audio::{DeviceCommand, Devices},
    bluetooth::{Command as BtCommand, State as BtState},
    system::{Network, WifiNetwork},
    theme::{self, Palette, Variant, export::App as ThemeApp},
    wallpaper::Wallpaper,
    weather::Location,
};
use std::{collections::HashMap, path::PathBuf, sync::LazyLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Appearance,
    Theme,
    Wallpaper,
    Bar,
    Weather,
    Wifi,
    Bluetooth,
    Sound,
    Calendar,
    Capture,
    Launcher,
    Animations,
}

impl Page {
    #[cfg(test)]
    const ALL: [Page; 12] = [
        Page::Appearance,
        Page::Theme,
        Page::Wallpaper,
        Page::Bar,
        Page::Weather,
        Page::Wifi,
        Page::Bluetooth,
        Page::Sound,
        Page::Calendar,
        Page::Capture,
        Page::Launcher,
        Page::Animations,
    ];

    const GROUPS: [&'static [Page]; 3] = [
        &[Page::Wifi, Page::Bluetooth, Page::Sound],
        &[
            Page::Appearance,
            Page::Theme,
            Page::Wallpaper,
            Page::Bar,
            Page::Animations,
        ],
        &[Page::Launcher, Page::Calendar, Page::Weather, Page::Capture],
    ];

    fn tint(self) -> u32 {
        match self {
            Page::Wifi | Page::Bluetooth => 0x0a84ffff,
            Page::Sound => 0xff375fff,
            Page::Appearance => 0x3a3a3cff,
            Page::Theme => 0xbf5af2ff,
            Page::Wallpaper => 0x32ade6ff,
            Page::Bar => 0x636366ff,
            Page::Animations => 0x30d158ff,
            Page::Launcher => 0x8e8e93ff,
            Page::Calendar => 0xff453aff,
            Page::Weather => 0x64d2ffff,
            Page::Capture => 0xff9f0aff,
        }
    }

    fn label(self) -> (&'static str, &'static str) {
        match self {
            Page::Appearance => ("palette", "Appearance"),
            Page::Theme => ("format_paint", "Theme"),
            Page::Wallpaper => ("wallpaper", "Wallpaper"),
            Page::Weather => ("partly_cloudy_day", "Weather"),
            Page::Wifi => ("wifi", "Wi-Fi"),
            Page::Bluetooth => ("bluetooth", "Bluetooth"),
            Page::Sound => ("volume_up", "Sound"),
            Page::Capture => ("screenshot_monitor", "Capture"),
            Page::Calendar => ("calendar_month", "Calendar"),
            Page::Bar => ("toolbar", "Menu Bar"),
            Page::Launcher => ("search", "Launcher"),
            Page::Animations => ("animation", "Animations"),
        }
    }
}

pub fn visible_groups(query: &str) -> Vec<Vec<Page>> {
    let query = query.trim().to_lowercase();
    Page::GROUPS
        .iter()
        .map(|group| {
            group
                .iter()
                .copied()
                .filter(|page| page.label().1.to_lowercase().contains(&query))
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

/// Free-text settings. Edits are kept as drafts so a half-typed value (an
/// invalid color, a broken clock format) never reaches the config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Font,
    IconFont,
    ClockFormat,
    ClockIcon,
    Terminal,
    WallpaperDir,
    ScreenshotDir,
    RecordDir,
    OcrLang,
    Background,
    Foreground,
    Accent,
    OnAccent,
    Muted,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Num {
    Height,
    Padding,
    Spacing,
    FontSize,
    WorkspacesMin,
    TitleChars,
    AnimDuration,
    Fps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    AnimEnabled,
    ReduceMotion,
    Blur,
    CaptureCopy,
    RecordAudio,
    ThemeDark,
    ThemeApp(ThemeApp),
}

/// Work only the daemon can do (files, processes), asked for by a settings edit.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    ApplyTheme,
    RestoreThemes,
    SetWallpaper(PathBuf),
    LoadWallpapers,
    /// Search places by name for the weather location.
    Geocode(String),
    Bluetooth(BtCommand),
    Blur(bool),
    Audio(DeviceCommand),
    WifiScan,
    WifiPower(bool),
    /// Join a network, with a password for a new secured one.
    WifiConnect(WifiNetwork, Option<String>),
    /// Delete a saved profile.
    WifiForget(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Page(Page),
    Close,
    Text(Field, String),
    Num(Num, f32),
    Flag(Flag, bool),
    Move(Section, usize, isize),
    Remove(Section, usize),
    Add(Section, Widget),
    Source(ThemeSource),
    Preset(String),
    Scheme(Variant),
    ApplyTheme,
    RestoreThemes,
    SetWallpaper(PathBuf),
    RefreshWallpapers,
    Search(String),
    PageSearch(String),
    SearchSubmit,
    PickPlace(usize),
    ClearWeather,
    Imperial(bool),
    Bt(BtCommand),
    Audio(DeviceCommand),
    WifiPower(bool),
    WifiPick(usize),
    WifiPassword(String),
    WifiJoin,
    WifiCancel,
    WifiForget(usize),
    WifiRescan,
    NewLink(String),
    AddLink,
    RemoveLink(usize),
}

#[derive(Clone, Default)]
pub struct State {
    page: Page,
    drafts: HashMap<Field, String>,
    pub wallpapers: Vec<Wallpaper>,
    /// Result of the last theme/wallpaper job.
    pub status: String,
    /// Weather place search.
    search: String,
    pub places: Vec<Location>,
    pub search_status: String,
    pub bt: BtState,
    pub network: Option<Network>,
    pub audio: Devices,
    pub wifi: Vec<WifiNetwork>,
    /// "Connecting…" or the last Wi-Fi failure.
    pub wifi_status: String,
    /// The secured network asking for a password, and what's typed.
    pub(crate) wifi_pick: Option<String>,
    wifi_password: String,
    /// Calendar link being typed, and fetch problems to show.
    new_link: String,
    pub cal_errors: Vec<String>,
    pub(crate) page_search: String,
}

static PRESETS: LazyLock<Vec<Palette>> = LazyLock::new(theme::presets);

impl State {
    /// Applies `msg` to `config`; returns daemon work it needs.
    pub fn update(&mut self, config: &mut Config, msg: Msg) -> Option<Request> {
        match msg {
            Msg::Page(page) => {
                let left_bluetooth = self.page == Page::Bluetooth && page != Page::Bluetooth;
                self.page = page;
                if page == Page::Wallpaper && self.wallpapers.is_empty() {
                    return Some(Request::LoadWallpapers);
                }
                // Scanning runs only while the Bluetooth page is open.
                if page == Page::Bluetooth {
                    return Some(Request::Bluetooth(BtCommand::Scan(true)));
                }
                if left_bluetooth {
                    return Some(Request::Bluetooth(BtCommand::Scan(false)));
                }
            }
            Msg::Search(q) => self.search = q,
            Msg::PageSearch(q) => self.page_search = q,
            Msg::SearchSubmit if !self.search.trim().is_empty() => {
                self.search_status = "Searching…".into();
                return Some(Request::Geocode(self.search.clone()));
            }
            Msg::SearchSubmit => {}
            Msg::PickPlace(i) => {
                if let Some(place) = self.places.get(i) {
                    config.weather.location = place.label();
                    config.weather.latitude = place.latitude;
                    config.weather.longitude = place.longitude;
                    self.places.clear();
                    self.search.clear();
                    self.search_status.clear();
                }
            }
            Msg::ClearWeather => config.weather.location.clear(),
            Msg::Imperial(on) => config.weather.imperial = on,
            Msg::Bt(cmd) => return Some(Request::Bluetooth(cmd)),
            Msg::Audio(cmd) => return Some(Request::Audio(cmd)),
            Msg::WifiPower(on) => return Some(Request::WifiPower(on)),
            Msg::WifiRescan => return Some(Request::WifiScan),
            Msg::WifiPick(i) => {
                let net = self.wifi.get(i).filter(|n| !n.active)?;
                if net.secured && net.saved.is_none() {
                    self.wifi_pick = Some(net.ssid.clone());
                    self.wifi_password.clear();
                } else {
                    self.wifi_status = format!("Connecting to {}…", net.ssid);
                    return Some(Request::WifiConnect(net.clone(), None));
                }
            }
            Msg::WifiPassword(p) => self.wifi_password = p,
            Msg::WifiCancel => {
                self.wifi_pick = None;
                self.wifi_password.clear();
            }
            Msg::WifiJoin => {
                let pick = self.wifi_pick.as_ref()?;
                let net = self.wifi.iter().find(|n| &n.ssid == pick)?.clone();
                if self.wifi_password.len() < 8 {
                    self.wifi_status = "Wi-Fi passwords are at least 8 characters.".into();
                    return None;
                }
                self.wifi_pick = None;
                self.wifi_status = format!("Connecting to {}…", net.ssid);
                return Some(Request::WifiConnect(
                    net,
                    Some(std::mem::take(&mut self.wifi_password)),
                ));
            }
            Msg::WifiForget(i) => {
                let saved = self.wifi.get(i)?.saved.clone()?;
                return Some(Request::WifiForget(saved));
            }
            Msg::NewLink(l) => self.new_link = l,
            Msg::AddLink => {
                let link = self.new_link.trim().to_owned();
                let ok = ["https://", "http://", "webcal://"]
                    .iter()
                    .any(|p| link.starts_with(p));
                if ok && !config.calendar.urls.contains(&link) {
                    config.calendar.urls.push(link);
                    self.new_link.clear();
                }
            }
            Msg::RemoveLink(i) => {
                if i < config.calendar.urls.len() {
                    config.calendar.urls.remove(i);
                }
            }
            Msg::Close => {}
            Msg::Text(field, value) => {
                if apply_text(config, field, &value) {
                    self.drafts.remove(&field);
                } else {
                    self.drafts.insert(field, value);
                }
            }
            Msg::Num(num, value) => set_num(config, num, value),
            Msg::Flag(Flag::AnimEnabled, on) => config.animations.enabled = on,
            Msg::Flag(Flag::ReduceMotion, on) => config.animations.reduce_motion = on,
            Msg::Flag(Flag::Blur, on) => {
                config.bar.blur = on;
                return Some(Request::Blur(on));
            }
            Msg::Flag(Flag::CaptureCopy, on) => config.capture.copy = on,
            Msg::Flag(Flag::RecordAudio, on) => config.capture.record_audio = on,
            Msg::Flag(Flag::ThemeDark, on) => {
                config.theme.dark = on;
                return Some(Request::ApplyTheme);
            }
            Msg::Flag(Flag::ThemeApp(app), on) => {
                let apps = &mut config.theme.apps;
                apps.retain(|a| a != app.id());
                if on {
                    apps.push(app.id().to_owned());
                    return Some(Request::ApplyTheme);
                }
            }
            Msg::Source(source) => {
                config.theme.source = source;
                return Some(Request::ApplyTheme);
            }
            Msg::Preset(name) => {
                config.theme.source = ThemeSource::Preset;
                config.theme.preset = name;
                return Some(Request::ApplyTheme);
            }
            Msg::Scheme(v) => {
                config.theme.source = ThemeSource::Wallpaper;
                v.id().clone_into(&mut config.theme.scheme);
                return Some(Request::ApplyTheme);
            }
            Msg::ApplyTheme => return Some(Request::ApplyTheme),
            Msg::RestoreThemes => return Some(Request::RestoreThemes),
            Msg::RefreshWallpapers => return Some(Request::LoadWallpapers),
            Msg::SetWallpaper(path) => {
                config.wallpaper.current = path.display().to_string();
                return Some(Request::SetWallpaper(path));
            }
            Msg::Move(section, i, by) => {
                let list = section_mut(config, section);
                let j = i.checked_add_signed(by).filter(|&j| j < list.len());
                if let Some(j) = j {
                    list.swap(i, j);
                }
            }
            Msg::Remove(section, i) => {
                let list = section_mut(config, section);
                if i < list.len() {
                    list.remove(i);
                }
            }
            Msg::Add(section, w) => section_mut(config, section).push(w),
        }
        None
    }

    pub fn page(&self) -> Page {
        self.page
    }
}

#[path = "gpui_settings.rs"]
pub(crate) mod gpui_view;

fn section_mut(config: &mut Config, section: Section) -> &mut Vec<Widget> {
    match section {
        Section::Left => &mut config.bar.left,
        Section::Center => &mut config.bar.center,
        Section::Right => &mut config.bar.right,
    }
}

fn color_of(config: &Config, field: Field) -> Rgba {
    let c = &config.colors;
    match field {
        Field::Background => c.background,
        Field::Foreground => c.foreground,
        Field::Accent => c.accent,
        Field::OnAccent => c.on_accent,
        Field::Muted => c.muted,
        _ => c.urgent,
    }
}

fn text_of(config: &Config, field: Field) -> String {
    match field {
        Field::Font => config.bar.font.clone(),
        Field::IconFont => config.bar.icon_font.clone(),
        Field::ClockFormat => config.clock.format.clone(),
        Field::ClockIcon => config.clock.icon.clone(),
        Field::Terminal => config.launcher.terminal.clone(),
        Field::WallpaperDir => config.wallpaper.dir.clone(),
        Field::ScreenshotDir => config.capture.screenshot_dir.clone(),
        Field::RecordDir => config.capture.record_dir.clone(),
        Field::OcrLang => config.capture.ocr_lang.clone(),
        _ => color_of(config, field).into(),
    }
}

/// Writes `value` into the config if it is valid; returns whether it was.
fn apply_text(config: &mut Config, field: Field, value: &str) -> bool {
    let c = &mut config.colors;
    let slot = match field {
        Field::Font => return set(&mut config.bar.font, value),
        Field::IconFont => return set(&mut config.bar.icon_font, value),
        Field::ClockIcon => return set(&mut config.clock.icon, value),
        Field::Terminal => return set(&mut config.launcher.terminal, value),
        Field::WallpaperDir => return set(&mut config.wallpaper.dir, value),
        Field::ScreenshotDir => return set(&mut config.capture.screenshot_dir, value),
        Field::RecordDir => return set(&mut config.capture.record_dir, value),
        Field::OcrLang => {
            return !value.trim().is_empty() && set(&mut config.capture.ocr_lang, value.trim());
        }
        Field::ClockFormat => {
            return valid_clock_format(value) && set(&mut config.clock.format, value);
        }
        Field::Background => &mut c.background,
        Field::Foreground => &mut c.foreground,
        Field::Accent => &mut c.accent,
        Field::OnAccent => &mut c.on_accent,
        Field::Muted => &mut c.muted,
        Field::Urgent => &mut c.urgent,
    };
    match Rgba::parse(value) {
        Some(color) => {
            *slot = color;
            true
        }
        None => false,
    }
}

fn set(slot: &mut String, value: &str) -> bool {
    value.clone_into(slot);
    true
}

fn set_num(config: &mut Config, num: Num, v: f32) {
    match num {
        Num::Height => config.bar.height = v.round() as u32,
        Num::Padding => config.bar.padding = v,
        Num::Spacing => config.bar.spacing = v,
        Num::FontSize => config.bar.font_size = v,
        Num::WorkspacesMin => config.workspaces.min = v.round() as i32,
        Num::TitleChars => config.window_title.max_chars = v.round() as usize,
        Num::AnimDuration => config.animations.duration_ms = v.round() as u64,
        Num::Fps => config.capture.fps = v.round() as u32,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    s.chars()
        .take(max - 1)
        .chain(std::iter::once('…'))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn page_search_filters_groups() {
        assert_eq!(visible_groups("").len(), 3);
        assert_eq!(visible_groups("").iter().map(Vec::len).sum::<usize>(), 12);
        assert_eq!(visible_groups("wi"), [vec![Page::Wifi]]);
        assert_eq!(visible_groups("SOUND"), [vec![Page::Sound]]);
        assert!(visible_groups("zzz").is_empty());
        let mut config = Config::default();
        let mut state = State::default();
        state.update(&mut config, Msg::PageSearch("menu".into()));
        assert_eq!(state.page_search, "menu");
        assert_eq!(visible_groups(&state.page_search), [vec![Page::Bar]]);
    }

    use super::*;

    #[test]
    fn invalid_text_stays_a_draft() {
        let mut state = State::default();
        let mut config = Config::default();
        state.update(&mut config, Msg::Text(Field::Accent, "#12".into()));
        assert_eq!(config, Config::default());
        assert!(state.drafts.contains_key(&Field::Accent));

        state.update(&mut config, Msg::Text(Field::Accent, "#89b4fa".into()));
        assert_eq!(config.colors.accent, Rgba([0x89, 0xb4, 0xfa, 0xff]));
        assert!(state.drafts.is_empty());

        state.update(&mut config, Msg::Text(Field::ClockFormat, "%Q".into()));
        assert_eq!(config.clock.format, Config::default().clock.format);
    }

    #[test]
    fn theme_controls_request_an_apply() {
        let mut state = State::default();
        let mut config = Config::default();
        let r = state.update(&mut config, Msg::Scheme(Variant::Vibrant));
        assert_eq!(r, Some(Request::ApplyTheme));
        assert_eq!(
            (config.theme.source, config.theme.scheme.as_str()),
            (ThemeSource::Wallpaper, "vibrant")
        );
        let r = state.update(
            &mut config,
            Msg::Flag(Flag::ThemeApp(ThemeApp::Kitty), true),
        );
        assert_eq!(
            (r, config.theme.apps.clone()),
            (Some(Request::ApplyTheme), vec!["kitty".to_owned()])
        );
        // Turning an app off writes nothing; restoring is a separate, explicit action.
        assert_eq!(
            state.update(
                &mut config,
                Msg::Flag(Flag::ThemeApp(ThemeApp::Kitty), false)
            ),
            None
        );
        assert!(config.theme.apps.is_empty());
        let r = state.update(&mut config, Msg::SetWallpaper("/w/a.png".into()));
        assert_eq!(
            (r, config.wallpaper.current.as_str()),
            (Some(Request::SetWallpaper("/w/a.png".into())), "/w/a.png")
        );
    }

    #[test]
    fn weather_and_bluetooth_pages() {
        let mut state = State::default();
        let mut config = Config::default();
        state.places = vec![Location {
            name: "Berlin".into(),
            latitude: 52.5,
            longitude: 13.4,
            country: "Germany".into(),
            admin1: String::new(),
        }];
        assert_eq!(state.update(&mut config, Msg::PickPlace(0)), None);
        assert_eq!(
            (config.weather.location.as_str(), config.weather.latitude),
            ("Berlin, Germany", 52.5)
        );
        assert!(state.places.is_empty());
        // The scan follows the page: on when opened, off when left.
        assert_eq!(
            state.update(&mut config, Msg::Page(Page::Bluetooth)),
            Some(Request::Bluetooth(BtCommand::Scan(true)))
        );
        assert_eq!(
            state.update(&mut config, Msg::Page(Page::Bar)),
            Some(Request::Bluetooth(BtCommand::Scan(false)))
        );
        assert_eq!(state.update(&mut config, Msg::Page(Page::Appearance)), None);
    }

    #[test]
    fn secured_wifi_requires_password_then_clears_secret() {
        let mut state = State::default();
        let mut config = Config::default();
        let network = WifiNetwork {
            ssid: "Office".into(),
            strength: 90,
            secured: true,
            active: false,
            saved: None,
            device: "/device/1".into(),
            ap: "/ap/1".into(),
        };
        state.wifi.push(network.clone());
        assert_eq!(state.update(&mut config, Msg::WifiPick(0)), None);
        assert_eq!(state.wifi_pick.as_deref(), Some("Office"));
        state.update(&mut config, Msg::WifiPassword("short".into()));
        assert_eq!(state.update(&mut config, Msg::WifiJoin), None);
        assert!(state.wifi_pick.is_some());
        state.update(&mut config, Msg::WifiPassword("long-password".into()));
        assert_eq!(
            state.update(&mut config, Msg::WifiJoin),
            Some(Request::WifiConnect(network, Some("long-password".into())))
        );
        assert!(state.wifi_pick.is_none());
        assert!(state.wifi_password.is_empty());
        state.update(&mut config, Msg::WifiPick(0));
        state.update(&mut config, Msg::WifiPassword("discard-me".into()));
        state.update(&mut config, Msg::WifiCancel);
        assert!(state.wifi_pick.is_none());
        assert!(state.wifi_password.is_empty());
    }

    #[test]
    fn calendar_links_validate_and_deduplicate() {
        let mut state = State::default();
        let mut config = Config::default();
        state.update(&mut config, Msg::NewLink("not a link".into()));
        state.update(&mut config, Msg::AddLink);
        assert!(config.calendar.urls.is_empty());
        state.update(
            &mut config,
            Msg::NewLink(" webcal://calendar.example/private.ics ".into()),
        );
        state.update(&mut config, Msg::AddLink);
        assert_eq!(
            config.calendar.urls,
            ["webcal://calendar.example/private.ics"]
        );
        assert!(state.new_link.is_empty());
        state.update(
            &mut config,
            Msg::NewLink("webcal://calendar.example/private.ics".into()),
        );
        state.update(&mut config, Msg::AddLink);
        assert_eq!(config.calendar.urls.len(), 1);
        state.update(&mut config, Msg::RemoveLink(99));
        assert_eq!(config.calendar.urls.len(), 1);
        state.update(&mut config, Msg::RemoveLink(0));
        assert!(config.calendar.urls.is_empty());
    }

    #[test]
    fn ocr_and_clock_drafts_preserve_valid_config() {
        let mut state = State::default();
        let mut config = Config::default();
        let original = config.capture.ocr_lang.clone();
        state.update(&mut config, Msg::Text(Field::OcrLang, "   ".into()));
        assert_eq!(config.capture.ocr_lang, original);
        assert_eq!(state.drafts[&Field::OcrLang], "   ");
        state.update(&mut config, Msg::Text(Field::OcrLang, " eng+deu ".into()));
        assert_eq!(config.capture.ocr_lang, "eng+deu");
        assert!(!state.drafts.contains_key(&Field::OcrLang));
        state.update(&mut config, Msg::Text(Field::ClockFormat, "%Q".into()));
        assert_eq!(state.drafts[&Field::ClockFormat], "%Q");
        state.update(&mut config, Msg::Text(Field::ClockFormat, "%H:%M".into()));
        assert_eq!(config.clock.format, "%H:%M");
        assert!(!state.drafts.contains_key(&Field::ClockFormat));
    }

    #[test]
    fn edits_widget_sections() {
        let mut state = State::default();
        let mut config = Config::default();
        config.bar.right = vec![Widget::Clock, Widget::Settings];
        state.update(&mut config, Msg::Move(Section::Right, 0, 1));
        assert_eq!(config.bar.right, [Widget::Settings, Widget::Clock]);
        state.update(&mut config, Msg::Move(Section::Right, 1, 1)); // past the end: no-op
        assert_eq!(config.bar.right, [Widget::Settings, Widget::Clock]);
        state.update(&mut config, Msg::Remove(Section::Right, 0));
        state.update(&mut config, Msg::Add(Section::Left, Widget::Clock));
        assert_eq!(config.bar.right, [Widget::Clock]);
        assert_eq!(config.bar.left, [Widget::Workspaces, Widget::Clock]);
    }
}
