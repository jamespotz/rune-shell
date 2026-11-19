//! Settings window: a sidebar of categories and a page of controls per category.
//! Every edit updates the config immediately; `App` saves it to disk.

use crate::config::{Config, Rgba, ThemeSource, Widget, valid_clock_format};
use rune_services::{
    audio::{DeviceCommand, Devices},
    bluetooth::{Command as BtCommand, State as BtState},
    hyprland_config::{
        self, Bezier, Curve, Leaf, Overrides, Settings as HyprSettings, Snapshot, Spring, Touched,
        Value,
    },
    lock,
    system::{
        self, DeviceKind, DeviceState, LinkState, NetDevice, Network, ProfileKind, SavedProfile,
        WifiNetwork,
    },
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
    Network,
    Bluetooth,
    Sound,
    Equalizer,
    Calendar,
    Capture,
    Launcher,
    Animations,
    HyprWindows,
    HyprEffects,
    HyprInput,
    HyprMotion,
    HyprReview,
    Lock,
}

impl Page {
    #[cfg(test)]
    const ALL: [Page; 19] = [
        Page::Appearance,
        Page::Theme,
        Page::Wallpaper,
        Page::Bar,
        Page::Weather,
        Page::Network,
        Page::Bluetooth,
        Page::Sound,
        Page::Equalizer,
        Page::Calendar,
        Page::Capture,
        Page::Launcher,
        Page::Animations,
        Page::HyprWindows,
        Page::HyprEffects,
        Page::HyprInput,
        Page::HyprMotion,
        Page::HyprReview,
        Page::Lock,
    ];

    const GROUPS: [(&'static str, &'static [Page]); 4] = [
        (
            "Connections",
            &[Page::Network, Page::Bluetooth, Page::Sound, Page::Equalizer],
        ),
        (
            "Rune",
            &[
                Page::Appearance,
                Page::Theme,
                Page::Wallpaper,
                Page::Bar,
                Page::Animations,
            ],
        ),
        (
            "Apps",
            &[Page::Launcher, Page::Calendar, Page::Weather, Page::Capture],
        ),
        (
            "Hyprland",
            &[
                Page::HyprWindows,
                Page::HyprEffects,
                Page::HyprInput,
                Page::HyprMotion,
                Page::Lock,
                Page::HyprReview,
            ],
        ),
    ];

    /// Pages that edit Hyprland's own settings.
    pub fn is_hypr(self) -> bool {
        self.section().is_some() || matches!(self, Page::HyprMotion | Page::HyprReview)
    }

    /// Which [`hyprland_config::OPTIONS`] this page shows.
    pub fn section(self) -> Option<hyprland_config::Section> {
        match self {
            Page::HyprWindows => Some(hyprland_config::Section::Windows),
            Page::HyprEffects => Some(hyprland_config::Section::Effects),
            Page::HyprInput => Some(hyprland_config::Section::Input),
            _ => None,
        }
    }

    fn tint(self) -> u32 {
        match self {
            Page::Network | Page::Bluetooth => 0x0a84ffff,
            Page::Sound => 0xff375fff,
            Page::Equalizer => 0x5e5ce6ff,
            Page::Appearance => 0x3a3a3cff,
            Page::Theme => 0xbf5af2ff,
            Page::Wallpaper => 0x32ade6ff,
            Page::Bar => 0x636366ff,
            Page::Animations => 0x30d158ff,
            Page::Launcher => 0x8e8e93ff,
            Page::Calendar => 0xff453aff,
            Page::Weather => 0x64d2ffff,
            Page::Capture => 0xff9f0aff,
            Page::HyprWindows => 0x5e5ce6ff,
            Page::HyprEffects => 0xbf5af2ff,
            Page::HyprInput => 0x8e8e93ff,
            Page::HyprMotion => 0xff9f0aff,
            Page::HyprReview => 0x636366ff,
            Page::Lock => 0x5e5ce6ff,
        }
    }

    /// Other names the sidebar search finds this page by, lowercase.
    fn aliases(self) -> &'static [&'static str] {
        match self {
            Page::Network => &["wi-fi", "wifi", "ethernet", "lan", "wired", "vpn"],
            Page::Lock => &["hyprlock", "lock", "password"],
            _ => &[],
        }
    }

    fn label(self) -> (&'static str, &'static str) {
        match self {
            Page::Appearance => ("palette", "Appearance"),
            Page::Theme => ("format_paint", "Theme"),
            Page::Wallpaper => ("wallpaper", "Wallpaper"),
            Page::Weather => ("partly_cloudy_day", "Weather"),
            Page::Network => ("lan", "Network"),
            Page::Bluetooth => ("bluetooth", "Bluetooth"),
            Page::Sound => ("volume_up", "Sound"),
            Page::Equalizer => ("equalizer", "Equalizer"),
            Page::Capture => ("screenshot_monitor", "Capture"),
            Page::Calendar => ("calendar_month", "Calendar"),
            Page::Bar => ("toolbar", "Menu Bar"),
            Page::Launcher => ("search", "Launcher"),
            Page::Animations => ("animation", "Animations"),
            Page::HyprWindows => ("space_dashboard", "Windows"),
            Page::HyprEffects => ("blur_on", "Effects"),
            Page::HyprInput => ("keyboard", "Input"),
            Page::HyprMotion => ("motion_mode", "Window Motion"),
            Page::HyprReview => ("difference", "Review Changes"),
            Page::Lock => ("lock", "Lock Screen"),
        }
    }
}

/// Sidebar groups as `(title, pages)`, keeping pages whose name matches `query`.
/// A query matching a group's title keeps the whole group.
pub fn visible_groups(query: &str) -> Vec<(&'static str, Vec<Page>)> {
    let query = query.trim().to_lowercase();
    Page::GROUPS
        .iter()
        .map(|&(title, group)| {
            let whole = title.to_lowercase().contains(&query);
            let pages = group
                .iter()
                .copied()
                .filter(|page| {
                    whole
                        || page.label().1.to_lowercase().contains(&query)
                        || page.aliases().iter().any(|alias| alias.contains(&query))
                })
                .collect::<Vec<_>>();
            (title, pages)
        })
        .filter(|(_, pages)| !pages.is_empty())
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
    LockHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Num {
    Height,
    Width,
    Radius,
    MarginTop,
    MarginX,
    Gap,
    Padding,
    Spacing,
    FontSize,
    WorkspacesMin,
    TitleChars,
    AnimDuration,
    Fps,
    LockStrength,
    LockDim,
    LockGrace,
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
    LockEnabled,
    LockClock24,
    LockDate,
    LockUser,
    LockMedia,
    LockStatus,
    LockHideCursor,
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
    Forget(String),
    DeviceDisconnect(String),
    /// Saved profile and whether it connects automatically.
    AutoConnect(String, bool),
    /// Bring up a saved profile; `device` is `None` for VPNs.
    Activate {
        profile: String,
        device: Option<String>,
        name: String,
    },
    /// Take down an active connection, e.g. a VPN.
    Deactivate(String),
    WifiConnectHidden {
        ssid: String,
        password: Option<String>,
    },
    Hypr(HyprSettings),
    /// Lua that previews the unsaved Hyprland settings.
    HyprPreview(String),
    LockNow,
}

/// An edit on the Hyprland pages. Option keys are [`hyprland_config::OPTIONS`] keys.
#[derive(Debug, Clone, PartialEq)]
pub enum HyprEdit {
    Num(&'static str, f32),
    /// One side (top, right, bottom, left) of a sides option.
    Side(&'static str, usize, f32),
    Set(&'static str, Value),
    /// Drop Rune's override and go back to the dotfile value.
    Revert(&'static str),
    Stiffness(String, f32),
    Dampening(String, f32),
    CurveRevert(String),
    CurveCreate(String, Curve),
    CurveRename(String, String),
    CurveDelete(String),
    BezierPoints(String, [f64; 4]),
    BezierCoordinate(String, usize, f64),
    LeafEnabled(String, bool),
    LeafSpeed(String, f32),
    LeafCurve(String, String),
    LeafRevert(String),
    Save,
    Discard,
    Setup,
    /// Text typed in the Input page's repeat test field.
    TestText(String),
}

/// Hyprland settings being edited: the draft over the live compositor values.
#[derive(Debug, Clone, Default)]
pub struct HyprUi {
    /// `None` until the first snapshot arrives.
    pub live: Option<Snapshot>,
    /// What `rune.lua` will contain once saved; starts as `config.hyprland`.
    pub draft: Overrides,
    pub touched: Touched,
    /// The last failure, shown on the Hyprland pages.
    pub status: String,
    /// A save, setup or discard is running.
    pub busy: bool,
    /// The Input page's test field.
    pub test_text: String,
    removed_curves: std::collections::BTreeSet<String>,
    loaded: bool,
}

impl HyprUi {
    pub fn option(&self, key: &str) -> Option<&Value> {
        self.draft
            .options
            .get(key)
            .or_else(|| self.live.as_ref()?.options.get(key))
    }

    pub fn curve(&self, name: &str) -> Option<Spring> {
        if self.removed_curves.contains(name) {
            return None;
        }
        self.draft
            .curves
            .get(name)
            .or_else(|| self.live.as_ref()?.curves.get(name))
            .copied()
            .and_then(Curve::spring)
    }

    pub fn bezier(&self, name: &str) -> Option<Bezier> {
        if self.removed_curves.contains(name) {
            return None;
        }
        match self
            .draft
            .curves
            .get(name)
            .or_else(|| self.live.as_ref()?.curves.get(name))
        {
            Some(Curve::Bezier(b)) => Some(*b),
            _ => hyprland_config::bezier_preset(name),
        }
    }

    pub fn curve_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.draft.curves.keys().cloned().collect();
        if let Some(live) = &self.live {
            names.extend(live.curves.keys().cloned());
        }
        names.retain(|n| !self.removed_curves.contains(n));
        names.sort();
        names.dedup();
        names
    }

    pub fn animation_curves(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .springs()
            .into_iter()
            .filter(|n| self.curve(n).is_some())
            .map(|n| format!("spring:{n}"))
            .collect();
        names.extend(
            self.curve_names().into_iter().filter(|n| {
                self.bezier(n).is_some() && hyprland_config::bezier_preset(n).is_none()
            }),
        );
        for name in ["default", "linear"]
            .into_iter()
            .chain(hyprland_config::BEZIER_PRESETS.iter().map(|(n, _)| *n))
        {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
        names
    }

    fn set_bezier(&mut self, name: String, bezier: Bezier) -> Option<()> {
        if !hyprland_config::valid_name(&name)
            || hyprland_config::bezier_preset(&name).is_some()
            || matches!(name.as_str(), "default" | "linear")
            || self.bezier(&name).is_none()
            || (!self.draft.curves.contains_key(&name) && self.draft.curves.len() >= 32)
        {
            return None;
        }
        self.draft
            .curves
            .insert(name.clone(), Curve::Bezier(bezier.sanitized()?));
        self.touched.curves.insert(name);
        Some(())
    }

    pub fn custom_curve(&self, name: &str) -> bool {
        self.draft.curves.contains_key(name)
            && !self
                .live
                .as_ref()
                .is_some_and(|live| live.sources.curves.contains_key(name))
    }

    fn available_curve_name(&mut self, name: &str) -> bool {
        if !hyprland_config::valid_name(name) {
            self.status =
                "Use a letter, then letters, digits or underscores, up to 32 characters.".into();
            return false;
        }
        if matches!(name, "default" | "linear")
            || hyprland_config::bezier_preset(name).is_some()
            || self.curve_names().iter().any(|n| n == name)
        {
            self.status = "That curve name is already in use.".into();
            return false;
        }
        self.status.clear();
        true
    }

    fn replace_curve_references(&mut self, old: &str, new: &str) {
        for name in hyprland_config::LEAVES {
            if let Some(mut leaf) = self.leaf(name).cloned()
                && leaf.curve == old
            {
                leaf.curve = new.to_owned();
                self.set_leaf((*name).to_owned(), leaf);
            }
        }
    }

    pub fn leaf(&self, name: &str) -> Option<&Leaf> {
        self.draft
            .animations
            .get(name)
            .or_else(|| self.live.as_ref()?.leaves.get(name))
    }

    /// Spring names that animations can use.
    pub fn springs(&self) -> Vec<String> {
        let mut names: Vec<String> = self.draft.curves.keys().cloned().collect();
        if let Some(live) = &self.live {
            names.extend(live.curves.keys().cloned());
            names.extend(
                live.leaves
                    .values()
                    .filter_map(|l| l.curve.strip_prefix("spring:").map(str::to_owned)),
            );
        }
        names.retain(|n| !self.removed_curves.contains(n));
        names.sort();
        names.dedup();
        names
    }

    pub fn is_dirty(&self, saved: &Overrides) -> bool {
        self.loaded && &self.draft != saved
    }

    /// How many options, curves and animations differ from what's saved.
    pub fn changes(&self, saved: &Overrides) -> usize {
        fn differ<V: PartialEq>(
            a: &std::collections::BTreeMap<String, V>,
            b: &std::collections::BTreeMap<String, V>,
        ) -> usize {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).count()
        }
        if !self.loaded {
            return 0;
        }
        differ(&self.draft.options, &saved.options)
            + differ(&self.draft.curves, &saved.curves)
            + differ(&self.draft.animations, &saved.animations)
    }

    /// Whether `page` shows an unsaved change.
    pub fn page_dirty(&self, page: Page, saved: &Overrides) -> bool {
        if !self.loaded {
            return false;
        }
        match page.section() {
            Some(section) => hyprland_config::OPTIONS
                .iter()
                .filter(|o| o.section == section)
                .any(|o| self.draft.options.get(o.key) != saved.options.get(o.key)),
            None if page == Page::HyprMotion => {
                self.draft.curves != saved.curves || self.draft.animations != saved.animations
            }
            None => false,
        }
    }

    /// Applies a snapshot or failure from the daemon.
    pub fn received(&mut self, result: Result<Box<Snapshot>, String>) {
        self.busy = false;
        match result {
            Ok(snapshot) => {
                self.live = Some(*snapshot);
                self.status.clear();
            }
            Err(e) => self.status = e,
        }
    }

    /// Lua re-applying every unsaved edit, e.g. after a reload dropped them.
    pub fn preview(&self) -> Option<String> {
        let live = self.live.as_ref()?;
        let lua = self.draft.preview(live, &self.touched).lua();
        (!lua.is_empty()).then_some(lua)
    }

    fn edit(&mut self, saved: &Overrides, edit: HyprEdit) -> Option<Request> {
        match edit {
            HyprEdit::Num(key, v) => {
                self.set(key, Value::Float(f64::from(v)));
            }
            HyprEdit::Side(key, side, v) => {
                let Some(Value::Sides(mut sides)) = self.option(key).cloned() else {
                    return None;
                };
                // Float-to-int `as` saturates (NaN becomes 0); the schema clamps the rest.
                *sides.get_mut(side)? = v.round().clamp(0., 1000.) as i64;
                self.set(key, Value::Sides(sides));
            }
            HyprEdit::Set(key, v) => self.set(key, v),
            HyprEdit::Revert(key) => {
                self.draft.options.remove(key);
                self.touched.options.insert(key.to_owned());
            }
            HyprEdit::Stiffness(name, v) => {
                let mut spring = self.curve(&name)?;
                spring.stiffness = f64::from(v);
                self.set_curve(name, spring);
            }
            HyprEdit::Dampening(name, v) => {
                let mut spring = self.curve(&name)?;
                spring.dampening = f64::from(v);
                self.set_curve(name, spring);
            }
            HyprEdit::CurveCreate(name, curve) => {
                if !self.available_curve_name(&name) {
                    return None;
                }
                if self.draft.curves.len() >= 32 {
                    self.status = "A maximum of 32 custom curves is supported.".into();
                    return None;
                }
                let mut one = Overrides::default();
                one.curves.insert(name.clone(), curve);
                one.sanitize();
                let curve = one.curves.remove(&name)?;
                self.removed_curves.remove(&name);
                self.draft.curves.insert(name.clone(), curve);
                self.touched.curves.insert(name);
            }
            HyprEdit::BezierCoordinate(name, index, value) => {
                let mut bezier = self.bezier(&name)?;
                *bezier.points.get_mut(index)? = value;
                self.set_bezier(name, bezier)?;
            }
            HyprEdit::BezierPoints(name, points) => {
                self.set_bezier(name, Bezier { points })?;
            }
            HyprEdit::CurveRename(old, new) => {
                if old == new {
                    return None;
                }
                if !self.custom_curve(&old) || !self.available_curve_name(&new) {
                    return None;
                }
                let curve = self.draft.curves.remove(&old)?;
                let (old_ref, new_ref) = match curve {
                    Curve::Spring(_) => (format!("spring:{old}"), format!("spring:{new}")),
                    Curve::Bezier(_) => (old.clone(), new.clone()),
                };
                self.draft.curves.insert(new.clone(), curve);
                self.removed_curves.remove(&new);
                self.removed_curves.insert(old.clone());
                self.touched.curves.insert(new);
                self.touched.curves.insert(old);
                self.replace_curve_references(&old_ref, &new_ref);
            }
            HyprEdit::CurveDelete(name) => {
                if !self.custom_curve(&name) {
                    return None;
                }
                let curve = self.draft.curves.remove(&name)?;
                let reference = match curve {
                    Curve::Spring(_) => format!("spring:{name}"),
                    Curve::Bezier(_) => name.clone(),
                };
                self.touched.curves.insert(name.clone());
                self.removed_curves.insert(name);
                self.replace_curve_references(&reference, "default");
            }
            HyprEdit::CurveRevert(name) => {
                self.draft.curves.remove(&name);
                self.touched.curves.insert(name);
            }
            HyprEdit::LeafEnabled(name, on) => {
                let mut leaf = self.leaf(&name)?.clone();
                leaf.enabled = on;
                self.set_leaf(name, leaf);
            }
            HyprEdit::LeafSpeed(name, v) => {
                let mut leaf = self.leaf(&name)?.clone();
                leaf.speed = f64::from(v);
                self.set_leaf(name, leaf);
            }
            HyprEdit::LeafCurve(name, spring) => {
                let mut leaf = self.leaf(&name)?.clone();
                leaf.curve = spring;
                self.set_leaf(name, leaf);
            }
            HyprEdit::LeafRevert(name) => {
                self.draft.animations.remove(&name);
                self.touched.leaves.insert(name);
            }
            HyprEdit::Save => {
                self.busy = true;
                self.touched = Touched::default();
                return Some(Request::Hypr(HyprSettings::Save(Box::new(
                    self.draft.clone(),
                ))));
            }
            HyprEdit::Discard => {
                self.busy = true;
                self.draft = saved.clone();
                self.removed_curves.clear();
                self.touched = Touched::default();
                return Some(Request::Hypr(HyprSettings::Discard));
            }
            HyprEdit::TestText(text) => {
                self.test_text = text;
                return None;
            }
            HyprEdit::Setup => {
                self.busy = true;
                return Some(Request::Hypr(HyprSettings::Setup(Box::new(saved.clone()))));
            }
        }
        self.preview().map(Request::HyprPreview)
    }

    fn set(&mut self, key: &'static str, value: Value) {
        if let Some(v) = hyprland_config::spec(key).and_then(|s| s.clamp(value)) {
            self.draft.options.insert(key.to_owned(), v);
            self.touched.options.insert(key.to_owned());
        }
    }

    fn set_curve(&mut self, name: String, spring: Spring) {
        let mut one = Overrides::default();
        one.curves.insert(name.clone(), Curve::Spring(spring));
        one.sanitize();
        if let Some(s) = one.curves.remove(&name) {
            self.draft.curves.insert(name.clone(), s);
            self.touched.curves.insert(name);
        }
    }

    fn set_leaf(&mut self, name: String, leaf: Leaf) {
        let mut one = Overrides::default();
        one.animations.insert(name.clone(), leaf);
        one.sanitize();
        if let Some(l) = one.animations.remove(&name) {
            self.draft.animations.insert(name.clone(), l);
            self.touched.leaves.insert(name);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Left,
    Center,
    Right,
}

/// One-click bar shapes; any other combination of the fields shows as custom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarShape {
    Notch,
    Floating,
    Edge,
}

impl BarShape {
    pub const ALL: [BarShape; 3] = [BarShape::Notch, BarShape::Floating, BarShape::Edge];

    pub fn label(self) -> &'static str {
        match self {
            BarShape::Notch => "Notch",
            BarShape::Floating => "Floating",
            BarShape::Edge => "Edge",
        }
    }

    /// `(margin_top, margin_x, width, radius)`
    fn fields(self) -> (u32, u32, u32, f32) {
        match self {
            BarShape::Notch => (0, 0, 1200, 14.),
            BarShape::Floating => (8, 12, 0, 12.),
            BarShape::Edge => (0, 0, 0, 0.),
        }
    }

    fn apply(self, bar: &mut crate::config::BarConfig) {
        (bar.margin_top, bar.margin_x, bar.width, bar.radius) = self.fields();
    }

    pub fn of(bar: &crate::config::BarConfig) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|shape| shape.fields() == (bar.margin_top, bar.margin_x, bar.width, bar.radius))
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Page(Page),
    Close,
    Text(Field, String),
    Num(Num, f32),
    Flag(Flag, bool),
    Remove(Section, usize),
    Add(Section, Widget),
    /// Drag result: move the widget at `from` to slot `to`, counted before it was lifted.
    Place((Section, usize), (Section, usize)),
    /// Drop a new widget at a slot; past the end appends.
    Insert(Section, usize, Widget),
    Shape(BarShape),
    EqEnabled(bool),
    /// Index into `equalizer::PRESETS`.
    EqPreset(usize),
    /// Band index and gain in dB.
    EqBand(usize, f32),
    EqReset,
    Source(ThemeSource),
    Look(crate::config::Look),
    PixelFont(crate::config::PixelFont),
    RemindMinutes(u32),
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
    DeviceDisconnect(String),
    /// Saved profile and whether it connects automatically.
    AutoConnect(String, bool),
    /// Forget a saved profile by its path.
    Forget(String),
    /// Bring up a saved profile by its path.
    Activate(String),
    /// Take down an active connection by its path.
    Deactivate(String),
    WifiHiddenOpen,
    WifiHiddenName(String),
    WifiHiddenSecured(bool),
    WifiHiddenPassword(String),
    WifiHiddenJoin,
    WifiHiddenCancel,
    NewLink(String),
    AddLink,
    RemoveLink(usize),
    Hypr(HyprEdit),
    LockBackground(rune_services::lock::Background),
    LockEffect(rune_services::lock::Effect),
    LockNow,
}

#[derive(Debug, Clone, Default)]
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
    pub net_devices: Vec<NetDevice>,
    pub net_profiles: Vec<SavedProfile>,
    /// The "Other Network…" form while it is open.
    pub(crate) wifi_hidden: Option<HiddenWifi>,
    /// Calendar link being typed, and fetch problems to show.
    new_link: String,
    pub cal_errors: Vec<String>,
    pub(crate) page_search: String,
    pub hypr: HyprUi,
    /// From [`crate::UiEvent::LockPreview`].
    pub lock_preview: Option<PathBuf>,
}

/// One line for a device's state, e.g. "Connected · Home" or "Cable unplugged".
pub(crate) fn device_status(device: &NetDevice) -> String {
    match device.state {
        DeviceState::Connected => match &device.connection {
            Some(name) => format!("Connected · {name}"),
            None => "Connected".to_owned(),
        },
        DeviceState::Connecting => "Connecting…".to_owned(),
        DeviceState::Disconnecting => "Disconnecting…".to_owned(),
        DeviceState::Disconnected => "Not connected".to_owned(),
        DeviceState::Unavailable => match device.kind {
            DeviceKind::Ethernet => "Cable unplugged".to_owned(),
            DeviceKind::Wifi => "Off".to_owned(),
        },
    }
}

/// A network that hides its name, as typed into the join form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HiddenWifi {
    pub ssid: String,
    pub secured: bool,
    pub password: String,
}

impl Default for HiddenWifi {
    fn default() -> Self {
        Self {
            ssid: String::new(),
            secured: true,
            password: String::new(),
        }
    }
}

static PRESETS: LazyLock<[Vec<Palette>; 2]> = LazyLock::new(|| {
    [
        theme::presets_for(theme::Appearance::Light),
        theme::presets_for(theme::Appearance::Dark),
    ]
});

impl State {
    /// Applies `msg` to `config`; returns daemon work it needs.
    pub fn update(&mut self, config: &mut Config, msg: Msg) -> Option<Request> {
        match msg {
            Msg::Page(page) => {
                let left_bluetooth = self.page == Page::Bluetooth && page != Page::Bluetooth;
                let entered_hypr = page.is_hypr() && !self.page.is_hypr();
                self.page = page;
                if entered_hypr && !self.hypr.busy {
                    if !self.hypr.loaded {
                        self.hypr.draft = config.hyprland.clone();
                        self.hypr.loaded = true;
                    }
                    return Some(Request::Hypr(HyprSettings::Snapshot));
                }
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
                return Some(Request::Forget(saved));
            }
            Msg::DeviceDisconnect(device) => return Some(Request::DeviceDisconnect(device)),
            Msg::AutoConnect(profile, on) => return Some(Request::AutoConnect(profile, on)),
            Msg::Forget(profile) => return Some(Request::Forget(profile)),
            Msg::Activate(profile) => {
                let saved = self.net_profiles.iter().find(|p| p.profile == profile)?;
                let device = match saved.kind {
                    system::ProfileKind::Vpn => None,
                    kind => {
                        let wanted = match kind {
                            system::ProfileKind::Wifi => DeviceKind::Wifi,
                            _ => DeviceKind::Ethernet,
                        };
                        let device = self
                            .net_devices
                            .iter()
                            .find(|d| d.kind == wanted && d.state != DeviceState::Unavailable);
                        let Some(device) = device else {
                            self.wifi_status = format!("No device can connect {}.", saved.name);
                            return None;
                        };
                        Some(device.path.clone())
                    }
                };
                self.wifi_status = format!("Connecting {}…", saved.name);
                return Some(Request::Activate {
                    profile,
                    device,
                    name: saved.name.clone(),
                });
            }
            Msg::Deactivate(active) => return Some(Request::Deactivate(active)),
            Msg::WifiHiddenOpen => {
                self.wifi_hidden.get_or_insert_default();
            }
            Msg::WifiHiddenName(name) => {
                if let Some(hidden) = &mut self.wifi_hidden {
                    hidden.ssid = name;
                }
            }
            Msg::WifiHiddenSecured(on) => {
                if let Some(hidden) = &mut self.wifi_hidden {
                    hidden.secured = on;
                }
            }
            Msg::WifiHiddenPassword(password) => {
                if let Some(hidden) = &mut self.wifi_hidden {
                    hidden.password = password;
                }
            }
            Msg::WifiHiddenCancel => self.wifi_hidden = None,
            Msg::WifiHiddenJoin => {
                let hidden = self.wifi_hidden.as_ref()?;
                let password = hidden.secured.then_some(hidden.password.as_str());
                if let Err(problem) = system::check_hidden(&hidden.ssid, password) {
                    self.wifi_status = problem;
                    return None;
                }
                let hidden = self.wifi_hidden.take()?;
                self.wifi_status = format!("Connecting to {}…", hidden.ssid);
                return Some(Request::WifiConnectHidden {
                    password: hidden.secured.then_some(hidden.password),
                    ssid: hidden.ssid,
                });
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
            Msg::Flag(Flag::LockEnabled, on) => config.lock.enabled = on,
            Msg::Flag(Flag::LockClock24, on) => config.lock.clock_24h = on,
            Msg::Flag(Flag::LockDate, on) => config.lock.show_date = on,
            Msg::Flag(Flag::LockUser, on) => config.lock.show_user = on,
            Msg::Flag(Flag::LockMedia, on) => config.lock.show_media = on,
            Msg::Flag(Flag::LockStatus, on) => config.lock.show_status = on,
            Msg::Flag(Flag::LockHideCursor, on) => config.lock.hide_cursor = on,
            Msg::LockBackground(background) => config.lock.background = background,
            Msg::LockEffect(effect) => config.lock.effect = Some(effect),
            Msg::LockNow => return Some(Request::LockNow),
            Msg::Flag(Flag::ThemeApp(app), on) => {
                let apps = &mut config.theme.apps;
                apps.retain(|a| a != app.id());
                if on {
                    apps.push(app.id().to_owned());
                    return Some(Request::ApplyTheme);
                }
            }
            Msg::Look(look) => config.bar.look = look,
            Msg::PixelFont(font) => config.bar.pixel_font = font,
            Msg::RemindMinutes(minutes) => config.calendar.remind_minutes = minutes,
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
            Msg::Remove(section, i) => {
                let list = section_mut(config, section);
                if i < list.len() {
                    list.remove(i);
                }
            }
            Msg::Add(section, w) => section_mut(config, section).push(w),
            Msg::Place((from, index), (to, mut at)) => {
                if index >= section_mut(config, from).len() {
                    return None;
                }
                let widget = section_mut(config, from).remove(index);
                if from == to && index < at {
                    at -= 1;
                }
                let list = section_mut(config, to);
                list.insert(at.min(list.len()), widget);
            }
            Msg::Insert(section, at, w) => {
                let list = section_mut(config, section);
                list.insert(at.min(list.len()), w);
            }
            Msg::Shape(shape) => shape.apply(&mut config.bar),
            Msg::EqEnabled(on) => config.equalizer.enabled = on,
            Msg::EqPreset(index) => {
                if let Some(preset) = rune_services::equalizer::PRESETS.get(index) {
                    config.equalizer.bands = preset.gains;
                }
            }
            Msg::EqBand(index, db) if db.is_finite() => {
                if let Some(band) = config.equalizer.bands.get_mut(index) {
                    // Half-dB steps, so dragging back to the middle lands on 0 and on presets.
                    *band = rune_services::equalizer::clamp_gain((db * 2.).round() / 2.);
                }
            }
            Msg::EqBand(..) => {}
            Msg::EqReset => config.equalizer.bands = [0.; 10],
            Msg::Hypr(edit) => {
                if edit == HyprEdit::Save {
                    config.hyprland = self.hypr.draft.clone();
                }
                return self.hypr.edit(&config.hyprland, edit);
            }
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
        Field::LockHint => config.lock.hint.clone(),
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
        Field::LockHint => {
            config.lock.hint = lock::clean_hint(value);
            return true;
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
        Num::Width => config.bar.width = v.round() as u32,
        Num::Radius => config.bar.radius = v,
        Num::MarginTop => config.bar.margin_top = v.round() as u32,
        Num::MarginX => config.bar.margin_x = v.round() as u32,
        Num::Gap => config.bar.gap = v.round() as u32,
        Num::Padding => config.bar.padding = v,
        Num::Spacing => config.bar.spacing = v,
        Num::FontSize => config.bar.font_size = v,
        Num::WorkspacesMin => config.workspaces.min = v.round() as i32,
        Num::TitleChars => config.window_title.max_chars = v.round() as usize,
        Num::AnimDuration => config.animations.duration_ms = v.round() as u64,
        Num::Fps => config.capture.fps = v.round() as u32,
        Num::LockStrength => config.lock.strength = clamp_u8(v, lock::MAX_STRENGTH),
        Num::LockDim => config.lock.dim = clamp_u8(v, lock::MAX_DIM),
        Num::LockGrace => config.lock.grace_seconds = clamp_u8(v, lock::MAX_GRACE_SECONDS),
    }
}

fn clamp_u8(v: f32, max: u8) -> u8 {
    // Float-to-int `as` saturates and maps NaN to 0; the clamp keeps it in range.
    v.round().clamp(0., f32::from(max)) as u8
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
        let pages =
            |q: &str| -> Vec<Vec<Page>> { visible_groups(q).into_iter().map(|(_, p)| p).collect() };
        assert_eq!(visible_groups("").len(), 4);
        assert_eq!(pages("").iter().map(Vec::len).sum::<usize>(), 19);
        assert_eq!(pages("motion"), [vec![Page::HyprMotion]]);
        assert_eq!(pages("wi-"), [vec![Page::Network]]);
        assert_eq!(pages("vpn"), [vec![Page::Network]]);
        assert_eq!(pages("ethernet"), [vec![Page::Network]]);
        assert_eq!(pages("SOUND"), [vec![Page::Sound]]);
        assert_eq!(pages("equal"), [vec![Page::Equalizer]]);
        assert_eq!(pages("hyprland").concat().len(), 6);
        assert_eq!(pages("hyprlock"), [vec![Page::Lock]]);
        assert!(visible_groups("zzz").is_empty());
        let mut config = Config::default();
        let mut state = State::default();
        state.update(&mut config, Msg::PageSearch("menu".into()));
        assert_eq!(state.page_search, "menu");
        assert_eq!(
            visible_groups(&state.page_search),
            [("Rune", vec![Page::Bar])]
        );
    }

    use super::*;

    #[test]
    fn lock_edits_stay_in_range_and_lock_now_uses_the_lock_command() {
        let mut state = State::default();
        let mut config = Config::default();
        state.update(&mut config, Msg::Num(Num::LockStrength, 1e9));
        state.update(&mut config, Msg::Num(Num::LockDim, f32::NAN));
        state.update(&mut config, Msg::Num(Num::LockGrace, -5.));
        assert_eq!(
            (
                config.lock.strength,
                config.lock.dim,
                config.lock.grace_seconds
            ),
            (100, 0, 0)
        );
        state.update(&mut config, Msg::Text(Field::LockHint, "x\n".repeat(100)));
        assert_eq!(config.lock.hint, "x".repeat(lock::MAX_HINT_CHARS));
        assert!(state.drafts.is_empty());
        state.update(&mut config, Msg::LockEffect(lock::Effect::Pixelate));
        assert_eq!(config.lock.effect, Some(lock::Effect::Pixelate));
        assert_eq!(
            state.update(&mut config, Msg::LockNow),
            Some(Request::LockNow)
        );

        assert_eq!(config.lock_command(), config.power.lock_command);
        state.update(&mut config, Msg::Flag(Flag::LockEnabled, true));
        assert!(config.lock_command().contains("hyprlock --config"));
        let reparsed = Config::parse(&config.to_toml()).unwrap();
        assert_eq!(reparsed.lock, config.lock);
        let hostile =
            Config::parse("[lock]\nstrength = 255\ndim = 99\ngrace_seconds = 200\n").unwrap();
        assert_eq!(
            (
                hostile.lock.strength,
                hostile.lock.dim,
                hostile.lock.grace_seconds
            ),
            (100, 90, 30)
        );
    }

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
    fn hidden_wifi_validates_then_sends_and_clears_the_form() {
        let mut state = State::default();
        let mut config = Config::default();
        assert_eq!(state.update(&mut config, Msg::WifiHiddenJoin), None);
        state.update(&mut config, Msg::WifiHiddenOpen);
        state.update(&mut config, Msg::WifiHiddenName("Lab".into()));
        state.update(&mut config, Msg::WifiHiddenPassword("short".into()));
        assert_eq!(state.update(&mut config, Msg::WifiHiddenJoin), None);
        assert!(state.wifi_status.contains("8 to 64"));
        assert!(state.wifi_hidden.is_some());
        state.update(&mut config, Msg::WifiHiddenPassword("long-password".into()));
        assert_eq!(
            state.update(&mut config, Msg::WifiHiddenJoin),
            Some(Request::WifiConnectHidden {
                ssid: "Lab".into(),
                password: Some("long-password".into()),
            })
        );
        assert!(state.wifi_hidden.is_none());
        state.update(&mut config, Msg::WifiHiddenOpen);
        state.update(&mut config, Msg::WifiHiddenName("Open lab".into()));
        state.update(
            &mut config,
            Msg::WifiHiddenPassword("ignored-secret".into()),
        );
        state.update(&mut config, Msg::WifiHiddenSecured(false));
        assert_eq!(
            state.update(&mut config, Msg::WifiHiddenJoin),
            Some(Request::WifiConnectHidden {
                ssid: "Open lab".into(),
                password: None,
            })
        );
    }

    fn profile(kind: system::ProfileKind, name: &str) -> SavedProfile {
        SavedProfile {
            name: name.into(),
            profile: format!("/settings/{name}"),
            kind,
            ssid: None,
            vpn_type: None,
            auto_connect: true,
            active: None,
        }
    }

    fn device(kind: DeviceKind, state: DeviceState, path: &str) -> NetDevice {
        NetDevice {
            path: path.into(),
            interface: path.trim_start_matches('/').into(),
            kind,
            state,
            connection: None,
            link: system::LinkDetails::default(),
        }
    }

    #[test]
    fn activating_picks_a_usable_device_and_vpns_need_none() {
        let mut state = State::default();
        let mut config = Config::default();
        assert_eq!(
            state.update(&mut config, Msg::Activate("/settings/missing".into())),
            None
        );
        state.net_profiles = vec![
            profile(system::ProfileKind::Wired, "Office"),
            profile(system::ProfileKind::Vpn, "Work"),
        ];
        state.net_devices = vec![device(
            DeviceKind::Ethernet,
            DeviceState::Unavailable,
            "/eth0",
        )];
        assert_eq!(
            state.update(&mut config, Msg::Activate("/settings/Office".into())),
            None
        );
        assert!(state.wifi_status.contains("No device"));
        state.net_devices.push(device(
            DeviceKind::Ethernet,
            DeviceState::Disconnected,
            "/eth1",
        ));
        assert_eq!(
            state.update(&mut config, Msg::Activate("/settings/Office".into())),
            Some(Request::Activate {
                profile: "/settings/Office".into(),
                device: Some("/eth1".into()),
                name: "Office".into(),
            })
        );
        assert_eq!(
            state.update(&mut config, Msg::Activate("/settings/Work".into())),
            Some(Request::Activate {
                profile: "/settings/Work".into(),
                device: None,
                name: "Work".into(),
            })
        );
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
        state.update(
            &mut config,
            Msg::Place((Section::Right, 0), (Section::Right, 2)),
        );
        assert_eq!(config.bar.right, [Widget::Settings, Widget::Clock]);
        state.update(
            &mut config,
            Msg::Place((Section::Right, 1), (Section::Right, 9)),
        );
        assert_eq!(config.bar.right, [Widget::Settings, Widget::Clock]);
        state.update(&mut config, Msg::Remove(Section::Right, 0));
        state.update(&mut config, Msg::Add(Section::Left, Widget::Clock));
        assert_eq!(config.bar.right, [Widget::Clock]);
        assert_eq!(config.bar.left, [Widget::Workspaces, Widget::Clock]);
    }

    #[test]
    fn places_dragged_widgets_and_ignores_hostile_indices() {
        let mut state = State::default();
        let mut config = Config::default();
        config.bar.left = vec![Widget::Workspaces];
        config.bar.right = vec![Widget::Clock, Widget::Battery, Widget::Power];
        // Within a section, `at` is the slot before the drag lifted the item.
        state.update(
            &mut config,
            Msg::Place((Section::Right, 0), (Section::Right, 2)),
        );
        assert_eq!(
            config.bar.right,
            [Widget::Battery, Widget::Clock, Widget::Power]
        );
        state.update(
            &mut config,
            Msg::Place((Section::Right, 2), (Section::Left, 0)),
        );
        assert_eq!(config.bar.left, [Widget::Power, Widget::Workspaces]);
        assert_eq!(config.bar.right, [Widget::Battery, Widget::Clock]);
        state.update(
            &mut config,
            Msg::Place((Section::Right, 0), (Section::Center, usize::MAX)),
        );
        assert_eq!(config.bar.center.last(), Some(&Widget::Battery));
        let before = config.bar.clone();
        state.update(
            &mut config,
            Msg::Place((Section::Left, 99), (Section::Right, 0)),
        );
        assert_eq!(config.bar, before);
        state.update(&mut config, Msg::Insert(Section::Left, 1, Widget::Stats));
        state.update(
            &mut config,
            Msg::Insert(Section::Left, usize::MAX, Widget::Capture),
        );
        assert_eq!(
            config.bar.left,
            [
                Widget::Power,
                Widget::Stats,
                Widget::Workspaces,
                Widget::Capture
            ]
        );
    }

    #[test]
    fn equalizer_edits_bands_presets_and_rejects_bad_values() {
        use rune_services::equalizer::{PRESETS, preset_of};
        let mut state = State::default();
        let mut config = Config::default();
        state.update(&mut config, Msg::EqEnabled(true));
        assert!(config.equalizer.enabled);
        state.update(&mut config, Msg::EqPreset(4));
        assert_eq!(
            preset_of(&config.equalizer.bands).map(|p| p.id),
            Some("rock")
        );
        state.update(&mut config, Msg::EqBand(3, 4.5));
        assert_eq!(config.equalizer.bands[3], 4.5);
        assert!(preset_of(&config.equalizer.bands).is_none());
        let before = config.equalizer.bands;
        state.update(&mut config, Msg::EqBand(10, 3.));
        state.update(&mut config, Msg::EqBand(0, f32::NAN));
        state.update(&mut config, Msg::EqPreset(99));
        assert_eq!(config.equalizer.bands, before);
        state.update(&mut config, Msg::EqBand(0, 40.));
        assert_eq!(config.equalizer.bands[0], 12.);
        state.update(&mut config, Msg::EqBand(1, 0.04)); // snaps to the 0.5 dB grid
        assert_eq!(config.equalizer.bands[1], 0.);
        state.update(&mut config, Msg::EqReset);
        assert_eq!(config.equalizer.bands, PRESETS[0].gains);
    }

    #[test]
    fn animation_curve_choices_are_unique_with_live_presets() {
        let mut live = Snapshot::default();
        live.curves.insert(
            "ease".into(),
            Curve::Bezier(hyprland_config::bezier_preset("ease").unwrap()),
        );
        let ui = HyprUi {
            live: Some(live),
            ..HyprUi::default()
        };
        let choices = ui.animation_curves();
        assert_eq!(choices.iter().filter(|n| n.as_str() == "ease").count(), 1);
        assert!(choices.contains(&"linear".into()));
    }

    #[test]
    fn deleted_and_renamed_live_custom_curves_leave_effective_choices() {
        let curve = Curve::Bezier(Bezier {
            points: [0., 0., 1., 1.],
        });
        let mut ui = HyprUi::default();
        ui.draft.curves.insert("custom1".into(), curve);
        let mut live = Snapshot::default();
        live.curves.insert("custom1".into(), curve);
        ui.live = Some(live);
        ui.edit(
            &Overrides::default(),
            HyprEdit::CurveRename("custom1".into(), "newName".into()),
        );
        assert!(!ui.curve_names().contains(&"custom1".into()));
        assert!(ui.bezier("custom1").is_none());
        ui.edit(
            &Overrides::default(),
            HyprEdit::CurveDelete("newName".into()),
        );
        assert!(ui.curve_names().is_empty());
    }

    #[test]
    fn builtin_presets_cannot_be_overwritten_by_point_edits() {
        let mut ui = HyprUi::default();
        ui.edit(
            &Overrides::default(),
            HyprEdit::BezierPoints("ease".into(), [0., 0., 1., 1.]),
        );
        assert!(ui.draft.curves.is_empty());
        ui.edit(
            &Overrides::default(),
            HyprEdit::BezierCoordinate("ease".into(), 0, 0.8),
        );
        assert!(ui.draft.curves.is_empty());
    }

    #[test]
    fn saved_custom_bezier_remains_deletable_after_compositor_reports_it() {
        let curve = Curve::Bezier(Bezier {
            points: [0., 0., 1., 1.],
        });
        let mut ui = HyprUi::default();
        ui.draft.curves.insert("custom1".into(), curve);
        let mut live = Snapshot::default();
        live.curves.insert("custom1".into(), curve);
        ui.live = Some(live);
        assert!(ui.custom_curve("custom1"));
        ui.edit(
            &Overrides::default(),
            HyprEdit::CurveDelete("custom1".into()),
        );
        assert!(ui.draft.curves.is_empty());
    }

    #[test]
    fn bezier_coordinate_edits_preserve_the_other_coordinates() {
        let mut ui = HyprUi::default();
        let saved = Overrides::default();
        ui.edit(
            &saved,
            HyprEdit::CurveCreate(
                "custom1".into(),
                Curve::Bezier(Bezier {
                    points: [0., 0., 1., 1.],
                }),
            ),
        );
        ui.edit(&saved, HyprEdit::BezierCoordinate("custom1".into(), 0, 0.3));
        ui.edit(
            &saved,
            HyprEdit::BezierCoordinate("custom1".into(), 1, -0.5),
        );
        ui.edit(&saved, HyprEdit::BezierCoordinate("custom1".into(), 99, 5.));
        assert_eq!(ui.bezier("custom1").unwrap().points, [0.3, -0.5, 1., 1.]);
    }

    #[test]
    fn custom_curve_lifecycle_rewires_animation_references() {
        let mut ui = HyprUi {
            live: Some(hypr_snapshot()),
            ..HyprUi::default()
        };
        let saved = Overrides::default();
        let curve = Curve::Bezier(Bezier {
            points: [0.3, 0., 0.6, 1.],
        });
        ui.edit(&saved, HyprEdit::CurveCreate("custom1".into(), curve));
        assert_eq!(ui.draft.curves.get("custom1"), Some(&curve));
        ui.edit(
            &saved,
            HyprEdit::LeafCurve("windowsMove".into(), "custom1".into()),
        );
        ui.edit(
            &saved,
            HyprEdit::CurveRename("custom1".into(), "gentle".into()),
        );
        assert_eq!(ui.leaf("windowsMove").unwrap().curve, "gentle");
        assert!(!ui.draft.curves.contains_key("custom1"));
        ui.edit(&saved, HyprEdit::CurveDelete("gentle".into()));
        assert_eq!(ui.leaf("windowsMove").unwrap().curve, "default");
        assert!(ui.draft.curves.is_empty());
    }

    #[test]
    fn custom_curve_names_cannot_replace_existing_or_reserved_curves() {
        let mut ui = HyprUi {
            live: Some(hypr_snapshot()),
            ..HyprUi::default()
        };
        let saved = Overrides::default();
        for name in [
            "default",
            "linear",
            "easeOut",
            "stripGlide",
            "bad\"name",
            "",
        ] {
            ui.edit(
                &saved,
                HyprEdit::CurveCreate(
                    name.into(),
                    Curve::Bezier(Bezier {
                        points: [0., 0., 1., 1.],
                    }),
                ),
            );
            assert!(ui.draft.curves.is_empty(), "{name}");
            assert!(!ui.status.is_empty());
        }
    }

    fn hypr_snapshot() -> Snapshot {
        let mut live = Snapshot::default();
        live.options.insert("general.gaps_in".into(), Value::Int(5));
        live.options
            .insert("general.gaps_out".into(), Value::Sides([0, 16, 16, 16]));
        live.curves.insert(
            "stripGlide".into(),
            Curve::Spring(Spring {
                mass: 1.,
                stiffness: 150.,
                dampening: 25.,
            }),
        );
        live.leaves.insert(
            "windowsMove".into(),
            Leaf {
                enabled: true,
                speed: 5.,
                curve: "spring:stripGlide".into(),
                style: String::new(),
            },
        );
        live.setup = hyprland_config::Setup::Added;
        live
    }

    #[test]
    fn hyprland_edits_preview_save_and_discard() {
        let mut state = State::default();
        let mut config = Config::default();
        config
            .hyprland
            .options
            .insert("general.border_size".into(), Value::Int(3));
        assert_eq!(
            state.update(&mut config, Msg::Page(Page::HyprWindows)),
            Some(Request::Hypr(HyprSettings::Snapshot))
        );
        assert_eq!(state.hypr.draft, config.hyprland);
        // Moving between Hyprland pages doesn't refetch.
        assert_eq!(
            state.update(&mut config, Msg::Page(Page::HyprEffects)),
            None
        );
        // No preview before the live values are known.
        let edit = |key, v| Msg::Hypr(HyprEdit::Num(key, v));
        assert_eq!(state.update(&mut config, edit("general.gaps_in", 9.)), None);
        state.hypr.received(Ok(Box::new(hypr_snapshot())));

        let Some(Request::HyprPreview(lua)) =
            state.update(&mut config, edit("general.gaps_in", 1e9))
        else {
            panic!("expected a preview");
        };
        assert!(lua.contains("gaps_in = 40,"), "{lua}");
        assert!(
            !lua.contains("border_size"),
            "untouched keys aren't re-sent: {lua}"
        );
        assert!(state.hypr.is_dirty(&config.hyprland));
        assert_eq!(state.hypr.changes(&config.hyprland), 1);
        assert!(state.hypr.page_dirty(Page::HyprWindows, &config.hyprland));
        assert!(!state.hypr.page_dirty(Page::HyprEffects, &config.hyprland));

        state.update(
            &mut config,
            Msg::Hypr(HyprEdit::Side("general.gaps_out", 0, 12.)),
        );
        assert_eq!(
            state.hypr.option("general.gaps_out"),
            Some(&Value::Sides([12, 16, 16, 16]))
        );
        // Hostile indices and values change nothing.
        state.update(
            &mut config,
            Msg::Hypr(HyprEdit::Side("general.gaps_out", 9, 1.)),
        );
        state.update(&mut config, edit("general.gaps_in", f32::NAN));
        assert_eq!(state.hypr.option("general.gaps_in"), Some(&Value::Int(40)));

        let Some(Request::HyprPreview(lua)) = state.update(
            &mut config,
            Msg::Hypr(HyprEdit::Stiffness("stripGlide".into(), 300.)),
        ) else {
            panic!("expected a preview");
        };
        assert!(lua.contains("stiffness = 300.0"), "{lua}");
        state.update(
            &mut config,
            Msg::Hypr(HyprEdit::LeafSpeed("windowsMove".into(), 2.)),
        );
        assert_eq!(state.hypr.leaf("windowsMove").map(|l| l.speed), Some(2.));

        // Revert previews the value from before the edit.
        let Some(Request::HyprPreview(lua)) =
            state.update(&mut config, Msg::Hypr(HyprEdit::Revert("general.gaps_in")))
        else {
            panic!("expected a preview");
        };
        assert!(lua.contains("gaps_in = 5,"), "{lua}");

        let saved = state.update(&mut config, Msg::Hypr(HyprEdit::Save));
        assert_eq!(config.hyprland, state.hypr.draft);
        assert_eq!(
            saved,
            Some(Request::Hypr(HyprSettings::Save(Box::new(
                config.hyprland.clone()
            ))))
        );
        assert!(state.hypr.touched.is_empty() && state.hypr.busy);
        assert!(!state.hypr.is_dirty(&config.hyprland));

        state.hypr.received(Ok(Box::new(hypr_snapshot())));
        state.update(&mut config, edit("general.gaps_in", 20.));
        assert_eq!(
            state.update(&mut config, Msg::Hypr(HyprEdit::Discard)),
            Some(Request::Hypr(HyprSettings::Discard))
        );
        assert_eq!(state.hypr.draft, config.hyprland);
        state
            .hypr
            .received(Err("Hyprland rejected the request".into()));
        assert!(!state.hypr.busy);
        assert_eq!(state.hypr.status, "Hyprland rejected the request");
    }

    #[test]
    fn shape_presets_round_trip() {
        let mut state = State::default();
        let mut config = Config::default();
        assert_eq!(BarShape::of(&config.bar), Some(BarShape::Edge));
        for shape in BarShape::ALL {
            state.update(&mut config, Msg::Shape(shape));
            assert_eq!(BarShape::of(&config.bar), Some(shape));
        }
        state.update(&mut config, Msg::Num(Num::Radius, 3.));
        assert_eq!(BarShape::of(&config.bar), None);
    }
}
