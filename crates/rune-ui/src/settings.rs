//! Settings window: a sidebar of categories and a page of controls per category.
//! Every edit updates the config immediately; `App` saves it to disk.

use crate::{
    config::{Config, Rgba, ThemeSource, Widget, valid_clock_format},
    notch::{font, icon},
};
use iced::{
    Alignment, Color, Element, Length, border,
    widget::{
        Space, button, column, container, pick_list, row, scrollable, slider, text, text_input,
        toggler,
    },
};
use rune_services::{
    bluetooth::{Command as BtCommand, State as BtState},
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
    Bluetooth,
    Calendar,
    Capture,
    Launcher,
    Animations,
}

impl Page {
    const ALL: [Page; 10] = [
        Page::Appearance,
        Page::Theme,
        Page::Wallpaper,
        Page::Bar,
        Page::Weather,
        Page::Bluetooth,
        Page::Calendar,
        Page::Capture,
        Page::Launcher,
        Page::Animations,
    ];

    fn label(self) -> (&'static str, &'static str) {
        match self {
            Page::Appearance => ("palette", "Appearance"),
            Page::Theme => ("format_paint", "Theme"),
            Page::Wallpaper => ("wallpaper", "Wallpaper"),
            Page::Weather => ("partly_cloudy_day", "Weather"),
            Page::Bluetooth => ("bluetooth", "Bluetooth"),
            Page::Capture => ("screenshot_monitor", "Capture"),
            Page::Calendar => ("calendar_month", "Calendar"),
            Page::Bar => ("toolbar", "Bar"),
            Page::Launcher => ("search", "Launcher"),
            Page::Animations => ("animation", "Animations"),
        }
    }
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
    Radius,
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
    SearchSubmit,
    PickPlace(usize),
    ClearWeather,
    Imperial(bool),
    Bt(BtCommand),
    NewLink(String),
    AddLink,
    RemoveLink(usize),
}

#[derive(Default)]
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
    /// Calendar link being typed, and fetch problems to show.
    new_link: String,
    pub cal_errors: Vec<String>,
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

    pub fn view<'a>(&'a self, config: &'a Config) -> Element<'a, Msg> {
        let colors = &config.colors;
        let fg: Color = colors.foreground.into();
        let accent: Color = colors.accent.into();
        let base = Color {
            a: 1.0,
            ..colors.background.into()
        };
        let surface = mix(base, fg, 0.06);
        let sidebar = column(Page::ALL.into_iter().map(|page| {
            let (glyph, name) = page.label();
            let selected = page == self.page;
            button(
                row![icon(config, glyph, 20.0), text(name).size(14)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([8, 12])
            .on_press(Msg::Page(page))
            .style(move |_, status| button::Style {
                background: Some(
                    if selected {
                        Color { a: 0.18, ..accent }
                    } else if matches!(status, button::Status::Hovered) {
                        Color { a: 0.08, ..fg }
                    } else {
                        Color::TRANSPARENT
                    }
                    .into(),
                ),
                text_color: if selected { accent } else { fg },
                border: border::rounded(10),
                ..Default::default()
            })
            .into()
        }))
        .spacing(4);
        let sidebar = container(
            column![
                text("Settings").size(20).font(font(&config.bar.font)),
                sidebar
            ]
            .spacing(18),
        )
        .padding(16)
        .width(210)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(surface.into()),
            ..Default::default()
        });

        let (_, title) = self.page.label();
        let close = button(icon(config, "close", 20.0))
            .padding(6)
            .on_press(Msg::Close)
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered)
                    .then(|| Color { a: 0.12, ..fg }.into()),
                text_color: fg,
                border: border::rounded(8),
                ..Default::default()
            });
        let header = row![
            text(title).size(22),
            Space::new().width(Length::Fill),
            close
        ]
        .align_y(Alignment::Center);
        let page = match self.page {
            Page::Appearance => self.appearance(config, surface),
            Page::Bar => self.bar(config, surface),
            Page::Launcher => self.launcher(config, surface),
            Page::Theme => self.theme(config, surface),
            Page::Wallpaper => self.wallpaper(config, surface),
            Page::Weather => self.weather(config, surface),
            Page::Bluetooth => self.bluetooth(config, surface),
            Page::Capture => self.capture(config, surface),
            Page::Calendar => self.calendar(config, surface),
            Page::Animations => animations(config, surface),
        };
        // Right padding keeps the scrollbar off the cards.
        let page = container(page).padding(iced::Padding::ZERO.right(14));
        let content = column![header, scrollable(page).height(Length::Fill)]
            .spacing(16)
            .padding(20);

        let border_color = Color { a: 0.15, ..fg };
        container(row![sidebar, content])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(move |_| container::Style {
                background: Some(base.into()),
                border: border::rounded(16).color(border_color).width(1),
                text_color: Some(fg),
                ..Default::default()
            })
            .into()
    }

    fn appearance<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let fonts = group(
            "Fonts",
            surface,
            vec![
                setting(
                    config,
                    "Text font",
                    "Used by the bar and panels",
                    self.input(config, Field::Font),
                ),
                setting(
                    config,
                    "Icon font",
                    "Icons are written by name, e.g. \"schedule\"",
                    self.input(config, Field::IconFont),
                ),
                number(
                    config,
                    "Font size",
                    Num::FontSize,
                    config.bar.font_size,
                    9.0..=20.0,
                    0.5,
                ),
            ],
        );
        let color = |f, label: &'a str| {
            let swatch: Color = color_of(config, f).into();
            setting(
                config,
                label,
                "",
                row![
                    container(Space::new().width(22).height(22)).style(move |_| container::Style {
                        background: Some(swatch.into()),
                        border: border::rounded(6)
                            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.2))
                            .width(1),
                        ..Default::default()
                    }),
                    self.input(config, f)
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
            )
        };
        let colors = group(
            "Colors",
            surface,
            vec![
                color(Field::Background, "Notch background"),
                color(Field::Foreground, "Text"),
                color(Field::Accent, "Accent"),
                color(Field::OnAccent, "Text on accent"),
                color(Field::Muted, "Muted text"),
                color(Field::Urgent, "Urgent"),
            ],
        );
        let blur = group(
            "Effects",
            surface,
            vec![setting(
                config,
                "Blur",
                "Blurs what's behind the notch and panels. Shows when the notch background has transparency, e.g. #11111bcc",
                toggler(config.bar.blur)
                    .on_toggle(|on| Msg::Flag(Flag::Blur, on))
                    .into(),
            )],
        );
        column![fonts, colors, blur].spacing(16).into()
    }

    fn bar<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let b = &config.bar;
        let shape = group(
            "Notch shape",
            surface,
            vec![
                number(
                    config,
                    "Height",
                    Num::Height,
                    b.height as f32,
                    24.0..=56.0,
                    1.0,
                ),
                number(
                    config,
                    "Corner radius",
                    Num::Radius,
                    b.radius,
                    0.0..=28.0,
                    1.0,
                ),
                number(
                    config,
                    "Side padding",
                    Num::Padding,
                    b.padding,
                    0.0..=32.0,
                    1.0,
                ),
                number(
                    config,
                    "Widget spacing",
                    Num::Spacing,
                    b.spacing,
                    0.0..=32.0,
                    1.0,
                ),
            ],
        );
        let widgets = group(
            "Widgets",
            surface,
            vec![
                section_editor(config, "Left", Section::Left, &b.left),
                section_editor(config, "Center", Section::Center, &b.center),
                section_editor(config, "Right", Section::Right, &b.right),
            ],
        );
        let details = group(
            "Widget options",
            surface,
            vec![
                number(
                    config,
                    "Always show workspaces",
                    Num::WorkspacesMin,
                    config.workspaces.min as f32,
                    1.0..=10.0,
                    1.0,
                ),
                setting(
                    config,
                    "Clock format",
                    "strftime, e.g. %H:%M",
                    self.input(config, Field::ClockFormat),
                ),
                setting(
                    config,
                    "Clock icon",
                    "Icon name; empty for none",
                    self.input(config, Field::ClockIcon),
                ),
                number(
                    config,
                    "Window title length",
                    Num::TitleChars,
                    config.window_title.max_chars as f32,
                    10.0..=120.0,
                    1.0,
                ),
            ],
        );
        column![shape, widgets, details].spacing(16).into()
    }

    fn theme<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let t = &config.theme;
        let fg: Color = config.colors.foreground.into();
        let accent: Color = config.colors.accent.into();
        let muted: Color = config.colors.muted.into();
        let chip = move |label: String, selected: bool, msg: Msg| {
            button(text(label).size(13))
                .padding([6, 14])
                .on_press(msg)
                .style(move |_, status| button::Style {
                    background: Some(
                        if selected {
                            Color { a: 0.2, ..accent }
                        } else if matches!(status, button::Status::Hovered) {
                            Color { a: 0.08, ..fg }
                        } else {
                            Color { a: 0.04, ..fg }
                        }
                        .into(),
                    ),
                    text_color: if selected { accent } else { fg },
                    border: border::rounded(10)
                        .color(if selected { accent } else { Color::TRANSPARENT })
                        .width(1),
                    ..Default::default()
                })
        };
        let source = row![
            chip(
                "Presets".into(),
                t.source == ThemeSource::Preset,
                Msg::Source(ThemeSource::Preset)
            ),
            chip(
                "From wallpaper".into(),
                t.source == ThemeSource::Wallpaper,
                Msg::Source(ThemeSource::Wallpaper)
            ),
        ]
        .spacing(8);
        let body: Element<'a, Msg> = match t.source {
            ThemeSource::Preset => column(PRESETS.chunks(2).map(|pair| {
                row(pair.iter().map(|p| {
                    let selected = p.name == t.preset;
                    let swatches = row([p.bg_dim, p.primary, p.secondary, p.tertiary, p.fg].map(
                        |c| {
                            let c = Color::from_rgb8(c.0[0], c.0[1], c.0[2]);
                            container(Space::new().width(22).height(22))
                                .style(move |_| container::Style {
                                    background: Some(c.into()),
                                    border: border::rounded(11)
                                        .color(Color::from_rgba(1.0, 1.0, 1.0, 0.15))
                                        .width(1),
                                    ..Default::default()
                                })
                                .into()
                        },
                    ))
                    .spacing(6);
                    button(column![text(p.name.clone()).size(14), swatches].spacing(8))
                        .width(Length::Fill)
                        .padding(12)
                        .on_press(Msg::Preset(p.name.clone()))
                        .style(move |_, status| button::Style {
                            background: Some(
                                if matches!(status, button::Status::Hovered) {
                                    Color { a: 0.08, ..fg }
                                } else {
                                    Color { a: 0.04, ..fg }
                                }
                                .into(),
                            ),
                            text_color: fg,
                            border: border::rounded(12)
                                .color(if selected { accent } else { Color::TRANSPARENT })
                                .width(2),
                            ..Default::default()
                        })
                        .into()
                }))
                .spacing(10)
                .into()
            }))
            .spacing(10)
            .into(),
            ThemeSource::Wallpaper => {
                let current = Variant::from_id(&t.scheme).unwrap_or(Variant::TonalSpot);
                column![
                    setting(
                        config,
                        "Dark mode",
                        "Light or dark version of the wallpaper colors",
                        toggler(t.dark)
                            .on_toggle(|on| Msg::Flag(Flag::ThemeDark, on))
                            .into(),
                    ),
                    text("Scheme").size(14),
                    row(Variant::ALL
                        .map(|v| chip(v.label().into(), v == current, Msg::Scheme(v)).into()))
                    .spacing(8)
                    .wrap(),
                ]
                .spacing(12)
                .into()
            }
        };
        let colors = group("Colors", surface, vec![source.into(), body]);
        let apps = group(
            "Apply to apps",
            surface,
            ThemeApp::ALL
                .iter()
                .map(|&app| {
                    let on = t.apps.iter().any(|a| a == app.id());
                    setting(
                        config,
                        app.label(),
                        "",
                        toggler(on)
                            .on_toggle(move |v| Msg::Flag(Flag::ThemeApp(app), v))
                            .into(),
                    )
                })
                .collect(),
        );
        let actions = group(
            "Theme files",
            surface,
            vec![
                text("Rune backs up every file it changes the first time. Restore puts all of them back as they were.")
                    .size(13)
                    .color(muted)
                    .into(),
                row![
                    chip("Apply again".into(), false, Msg::ApplyTheme),
                    chip("Restore original app themes".into(), false, Msg::RestoreThemes),
                ]
                .spacing(8)
                .into(),
                text(self.status.clone()).size(12).color(muted).into(),
            ],
        );
        column![colors, apps, actions].spacing(16).into()
    }

    fn wallpaper<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let fg: Color = config.colors.foreground.into();
        let accent: Color = config.colors.accent.into();
        let muted: Color = config.colors.muted.into();
        let refresh = button(icon(config, "refresh", 20.0))
            .padding(6)
            .on_press(Msg::RefreshWallpapers)
            .style(move |_, s| button::Style {
                background: matches!(s, button::Status::Hovered)
                    .then(|| Color { a: 0.12, ..fg }.into()),
                text_color: fg,
                border: border::rounded(8),
                ..Default::default()
            });
        let folder = setting(
            config,
            "Folder",
            "Images (jpg, png, webp, gif) and videos (mp4, webm, mkv)",
            row![self.input(config, Field::WallpaperDir), refresh]
                .spacing(6)
                .align_y(Alignment::Center)
                .into(),
        );
        let tiles: Element<'a, Msg> = if self.wallpapers.is_empty() {
            text("No images or videos here yet. Check the folder, then refresh.")
                .size(13)
                .color(muted)
                .into()
        } else {
            column(self.wallpapers.chunks(3).map(|chunk| {
                row(chunk.iter().map(|w| {
                    let selected = config.wallpaper.current == w.path.display().to_string();
                    let thumb: Element<'a, Msg> = match &w.thumb {
                        // Thumbnails are pre-cropped to 16:9, so they fill the tile exactly.
                        Some(t) => iced::widget::image(t.clone())
                            .width(Length::Fill)
                            .height(110)
                            .content_fit(iced::ContentFit::Fill)
                            .border_radius(8)
                            .into(),
                        None => container(icon(config, "hourglass_empty", 24.0))
                            .center_x(Length::Fill)
                            .center_y(110)
                            .into(),
                    };
                    let name = w
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let caption = row![
                        icon(config, if w.video { "movie" } else { "image" }, 14.0),
                        text(truncate(&name, 22))
                            .size(11)
                            .color(muted)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center);
                    button(column![thumb, caption].spacing(6))
                        .width(Length::Fill)
                        .padding(6)
                        .on_press(Msg::SetWallpaper(w.path.clone()))
                        .style(move |_, status| button::Style {
                            background: matches!(status, button::Status::Hovered)
                                .then(|| Color { a: 0.08, ..fg }.into()),
                            text_color: fg,
                            border: border::rounded(12)
                                .color(if selected { accent } else { Color::TRANSPARENT })
                                .width(2),
                            ..Default::default()
                        })
                        .into()
                }))
                .spacing(8)
                .into()
            }))
            .spacing(8)
            .into()
        };
        column![
            group("Wallpaper", surface, vec![folder]),
            tiles,
            text(self.status.clone()).size(12).color(muted)
        ]
        .spacing(16)
        .into()
    }

    pub fn page(&self) -> Page {
        self.page
    }

    fn weather<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let w = &config.weather;
        let fg: Color = config.colors.foreground.into();
        let muted: Color = config.colors.muted.into();
        let small = |label: &'a str, msg: Msg| {
            button(text(label).size(13))
                .padding([6, 12])
                .on_press(msg)
                .style(move |_, s| button::Style {
                    background: Some(
                        Color {
                            a: if matches!(s, button::Status::Hovered) {
                                0.16
                            } else {
                                0.08
                            },
                            ..fg
                        }
                        .into(),
                    ),
                    text_color: fg,
                    border: border::rounded(8),
                    ..Default::default()
                })
        };
        let current: Element<'a, Msg> = if w.location.is_empty() {
            text("Not set. Weather stays hidden until you pick a place.")
                .size(13)
                .color(muted)
                .into()
        } else {
            row![
                text(w.location.clone()).size(14).width(Length::Fill),
                small("Clear", Msg::ClearWeather)
            ]
            .align_y(Alignment::Center)
            .into()
        };
        let search = row![
            text_input("Search a city", &self.search)
                .on_input(Msg::Search)
                .on_submit(Msg::SearchSubmit)
                .padding([6, 10])
                .size(14),
            small("Search", Msg::SearchSubmit),
        ]
        .spacing(8);
        let mut rows: Vec<Element<'a, Msg>> = vec![current, search.into()];
        if !self.search_status.is_empty() {
            rows.push(
                text(self.search_status.clone())
                    .size(12)
                    .color(muted)
                    .into(),
            );
        }
        for (i, place) in self.places.iter().enumerate() {
            rows.push(
                button(text(place.label()).size(13))
                    .width(Length::Fill)
                    .padding([6, 10])
                    .on_press(Msg::PickPlace(i))
                    .style(move |_, s| button::Style {
                        background: matches!(s, button::Status::Hovered)
                            .then(|| Color { a: 0.1, ..fg }.into()),
                        text_color: fg,
                        border: border::rounded(8),
                        ..Default::default()
                    })
                    .into(),
            );
        }
        let units = group(
            "Units",
            surface,
            vec![setting(
                config,
                "Fahrenheit and mph",
                "Instead of Celsius and km/h",
                toggler(w.imperial).on_toggle(Msg::Imperial).into(),
            )],
        );
        let note = text(
            "Forecasts come from Open-Meteo. Only the place you pick is sent, never your position.",
        )
        .size(12)
        .color(muted);
        column![group("Location", surface, rows), units, note]
            .spacing(16)
            .into()
    }

    fn bluetooth<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let bt = &self.bt;
        let fg: Color = config.colors.foreground.into();
        let muted: Color = config.colors.muted.into();
        let accent: Color = config.colors.accent.into();
        if !bt.available {
            return text("No Bluetooth adapter found (or BlueZ isn't running).")
                .size(13)
                .color(muted)
                .into();
        }
        let action = move |label: String, msg: Msg, active: bool| {
            button(text(label).size(12))
                .padding([5, 10])
                .on_press(msg)
                .style(move |_, s| button::Style {
                    background: Some(
                        if active {
                            Color { a: 0.25, ..accent }
                        } else {
                            Color {
                                a: if matches!(s, button::Status::Hovered) {
                                    0.16
                                } else {
                                    0.08
                                },
                                ..fg
                            }
                        }
                        .into(),
                    ),
                    text_color: if active { accent } else { fg },
                    border: border::rounded(8),
                    ..Default::default()
                })
        };
        let status = if !bt.powered {
            "Off"
        } else if bt.discovering {
            "On · looking for devices…"
        } else {
            "On"
        };
        let mut top = vec![setting(
            config,
            "Bluetooth",
            status,
            toggler(bt.powered)
                .on_toggle(|on| Msg::Bt(BtCommand::Power(on)))
                .into(),
        )];
        if let Some(code) = &bt.pairing_code {
            let accent: Color = config.colors.accent.into();
            top.push(
                container(
                    column![
                        text("Type this code on the device, then press Enter:").size(13),
                        text(code.clone()).size(28).font(font(&config.bar.font)),
                    ]
                    .spacing(4),
                )
                .padding(12)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Color { a: 0.15, ..accent }.into()),
                    border: border::rounded(10).color(accent).width(1),
                    ..Default::default()
                })
                .into(),
            );
        }
        if !bt.error.is_empty() {
            top.push(
                text(bt.error.clone())
                    .size(12)
                    .color(Color::from(config.colors.urgent))
                    .into(),
            );
        }
        let devices: Vec<Element<'a, Msg>> = bt
            .devices
            .iter()
            .map(|d| {
                let mut info = vec![
                    if d.connected {
                        "Connected"
                    } else if d.paired {
                        "Paired"
                    } else {
                        "Available"
                    }
                    .to_owned(),
                ];
                if let Some(b) = d.battery {
                    info.push(format!("{b}% battery"));
                }
                info.push(d.address.clone());
                let mut controls = row![].spacing(6).align_y(Alignment::Center);
                if let Some(p) = d.profiles.as_ref().filter(|_| d.connected) {
                    // Music sounds best in A2DP; calls need the headset profile for the microphone.
                    if let Some(audio) = &p.audio {
                        controls = controls.push(action(
                            "Audio".into(),
                            Msg::Bt(BtCommand::Profile(d.address.clone(), audio.clone())),
                            &p.active == audio,
                        ));
                    }
                    if let Some(call) = &p.call {
                        controls = controls.push(action(
                            "Call".into(),
                            Msg::Bt(BtCommand::Profile(d.address.clone(), call.clone())),
                            &p.active == call,
                        ));
                    }
                }
                controls = controls.push(if d.connected {
                    action(
                        "Disconnect".into(),
                        Msg::Bt(BtCommand::Disconnect(d.path.clone())),
                        false,
                    )
                } else if d.paired {
                    action(
                        "Connect".into(),
                        Msg::Bt(BtCommand::Connect(d.path.clone())),
                        false,
                    )
                } else {
                    action(
                        "Pair".into(),
                        Msg::Bt(BtCommand::Pair(d.path.clone())),
                        false,
                    )
                });
                let mut card = column![
                    row![
                        icon(config, crate::bt::device_icon(&d.icon), 26.0),
                        column![
                            text(d.name.clone()).size(14),
                            text(info.join(" · ")).size(11).color(muted)
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        controls,
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center)
                ]
                .spacing(8);
                if d.paired {
                    let path = d.path.clone();
                    card = card.push(
                        row![
                            text("Connect automatically").size(12).color(muted),
                            toggler(d.trusted)
                                .on_toggle(move |on| Msg::Bt(BtCommand::Trust(path.clone(), on)))
                                .size(16),
                            Space::new().width(Length::Fill),
                            action(
                                "Forget".into(),
                                Msg::Bt(BtCommand::Remove(d.path.clone())),
                                false
                            ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                }
                container(card)
                    .padding(12)
                    .width(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(Color { a: 0.04, ..fg }.into()),
                        border: border::rounded(10),
                        ..Default::default()
                    })
                    .into()
            })
            .collect();
        let devices = if devices.is_empty() {
            vec![
                text(if bt.powered {
                    "No devices yet. Put your device in pairing mode."
                } else {
                    "Turn Bluetooth on to see devices."
                })
                .size(13)
                .color(muted)
                .into(),
            ]
        } else {
            devices
        };
        column![
            group("Adapter", surface, top),
            group("Devices", surface, devices)
        ]
        .spacing(16)
        .into()
    }

    fn calendar<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let fg: Color = config.colors.foreground.into();
        let muted: Color = config.colors.muted.into();
        let small = |label: &'a str, msg: Msg| {
            button(text(label).size(12))
                .padding([5, 10])
                .on_press(msg)
                .style(move |_, s| button::Style {
                    background: Some(
                        Color {
                            a: if matches!(s, button::Status::Hovered) {
                                0.16
                            } else {
                                0.08
                            },
                            ..fg
                        }
                        .into(),
                    ),
                    text_color: fg,
                    border: border::rounded(8),
                    ..Default::default()
                })
        };
        let mut rows: Vec<Element<'a, Msg>> = config
            .calendar
            .urls
            .iter()
            .enumerate()
            .map(|(i, url)| {
                // Links are secrets (anyone with one can read the calendar), so only show the start.
                let shown: String = url.chars().take(48).collect::<String>()
                    + if url.chars().count() > 48 { "…" } else { "" };
                row![
                    icon(config, "calendar_month", 18.0),
                    text(shown)
                        .size(12)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::None),
                    small("Remove", Msg::RemoveLink(i)),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            })
            .collect();
        if rows.is_empty() {
            rows.push(text("No calendars yet.").size(13).color(muted).into());
        }
        rows.push(
            row![
                text_input(
                    "https://calendar.google.com/calendar/ical/…/basic.ics",
                    &self.new_link
                )
                .on_input(Msg::NewLink)
                .on_submit(Msg::AddLink)
                .padding([6, 10])
                .size(13),
                small("Add", Msg::AddLink),
            ]
            .spacing(8)
            .into(),
        );
        for e in &self.cal_errors {
            rows.push(
                text(e.clone())
                    .size(12)
                    .color(Color::from(config.colors.urgent))
                    .into(),
            );
        }
        let help = group(
            "Where to find the link",
            surface,
            vec![
                text("Google Calendar: Settings → pick a calendar → Integrate calendar → \"Secret address in iCal format\".").size(13).into(),
                text("Outlook: Settings → Calendar → Shared calendars → Publish a calendar → ICS link.").size(13).into(),
                text("Apple, Nextcloud, Proton and others: their \"share\" or \"subscribe\" link (webcal:// works too).").size(13).into(),
                text("Calendars are read-only here and refresh every 15 minutes. Click the clock in the bar to see them.").size(12).color(muted).into(),
            ],
        );
        column![group("Calendars", surface, rows), help]
            .spacing(16)
            .into()
    }

    fn capture<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let c = &config.capture;
        let muted: Color = config.colors.muted.into();
        let shots = group(
            "Screenshots",
            surface,
            vec![
                setting(
                    config,
                    "Folder",
                    "",
                    self.input(config, Field::ScreenshotDir),
                ),
                setting(
                    config,
                    "Copy to clipboard",
                    "As well as saving the file",
                    toggler(c.copy)
                        .on_toggle(|on| Msg::Flag(Flag::CaptureCopy, on))
                        .into(),
                ),
            ],
        );
        let recording = group(
            "Screen recording",
            surface,
            vec![
                setting(config, "Folder", "", self.input(config, Field::RecordDir)),
                setting(
                    config,
                    "Record audio",
                    "What your speakers play",
                    toggler(c.record_audio)
                        .on_toggle(|on| Msg::Flag(Flag::RecordAudio, on))
                        .into(),
                ),
                number(
                    config,
                    "Frame rate",
                    Num::Fps,
                    c.fps as f32,
                    24.0..=144.0,
                    1.0,
                ),
            ],
        );
        let ocr = group(
            "Text extraction",
            surface,
            vec![setting(
                config,
                "Languages",
                "Tesseract codes, e.g. eng or eng+deu (install the language packs)",
                self.input(config, Field::OcrLang),
            )],
        );
        let snippet = |s: &'a str| text(s).size(12).font(font(&config.bar.font));
        let keys = group(
            "Keybinds",
            surface,
            vec![
                text("Add any of these to your Hyprland config. Run a record command again (or click the timer in the bar) to stop.").size(13).color(muted).into(),
                snippet("hl.bind(\"Print\", hl.dsp.exec_cmd(\"rune-shell screenshot selection\"))").into(),
                snippet("hl.bind(\"SHIFT + Print\", hl.dsp.exec_cmd(\"rune-shell screenshot window\"))").into(),
                snippet("hl.bind(\"CTRL + Print\", hl.dsp.exec_cmd(\"rune-shell screenshot screen\"))").into(),
                snippet("hl.bind(\"SUPER + Print\", hl.dsp.exec_cmd(\"rune-shell record selection\"))").into(),
                snippet("hl.bind(\"SUPER + SHIFT + T\", hl.dsp.exec_cmd(\"rune-shell ocr\"))").into(),
            ],
        );
        column![shots, recording, ocr, keys].spacing(16).into()
    }

    fn launcher<'a>(&'a self, config: &'a Config, surface: Color) -> Element<'a, Msg> {
        let general = group(
            "Launcher",
            surface,
            vec![setting(
                config,
                "Terminal",
                "Runs terminal apps (btop, nvim...) as <terminal> -e <app>",
                self.input(config, Field::Terminal),
            )],
        );
        let muted: Color = config.colors.muted.into();
        let snippet = |s: &'a str| text(s).size(13).font(font(&config.bar.font));
        let keybinds = group(
            "Keybinds",
            surface,
            vec![
                text("Keybinds live in your Hyprland config. Add these lines, using any keys you like:")
                    .size(13)
                    .color(muted)
                    .into(),
                snippet("hl.bind(\"SUPER + SPACE\", hl.dsp.exec_cmd(\"rune-shell toggle launcher\"))").into(),
                snippet("hl.bind(\"SUPER + SLASH\", hl.dsp.exec_cmd(\"rune-shell toggle keybinds\"))").into(),
                snippet("hl.bind(\"SUPER + COMMA\", hl.dsp.exec_cmd(\"rune-shell toggle settings\"))").into(),
                text("In the launcher: type to search apps and Steam games, start with : for emoji, or type a sum like 12*3 to calculate.")
                    .size(13)
                    .color(muted)
                    .into(),
            ],
        );
        column![general, keybinds].spacing(16).into()
    }

    fn input<'a>(&'a self, config: &Config, field: Field) -> Element<'a, Msg> {
        let value = self
            .drafts
            .get(&field)
            .cloned()
            .unwrap_or_else(|| text_of(config, field));
        let invalid = self.drafts.contains_key(&field);
        text_input("", &value)
            .on_input(move |v| Msg::Text(field, v))
            .padding([6, 10])
            .size(14)
            .style(move |theme: &iced::Theme, status| {
                let mut style = text_input::default(theme, status);
                if invalid {
                    style.border.color = theme.palette().danger;
                }
                style
            })
            .into()
    }
}

fn animations(config: &Config, surface: Color) -> Element<'_, Msg> {
    let a = &config.animations;
    let toggle = |label, desc, flag, on| {
        setting(
            config,
            label,
            desc,
            toggler(on).on_toggle(move |v| Msg::Flag(flag, v)).into(),
        )
    };
    group(
        "Motion",
        surface,
        vec![
            toggle(
                "Animations",
                "Animate state changes across the shell",
                Flag::AnimEnabled,
                a.enabled,
            ),
            number(
                config,
                "Duration (ms)",
                Num::AnimDuration,
                a.duration_ms as f32,
                50.0..=1000.0,
                10.0,
            ),
            toggle(
                "Reduce motion",
                "Turn off all animation, whatever the settings above say",
                Flag::ReduceMotion,
                a.reduce_motion,
            ),
        ],
    )
}

fn group<'a>(title: &'a str, surface: Color, rows: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    column![
        text(title).size(13).style(|t: &iced::Theme| text::Style {
            color: Some(t.palette().primary)
        }),
        container(column(rows).spacing(14))
            .padding(16)
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(surface.into()),
                border: border::rounded(12),
                ..Default::default()
            }),
    ]
    .spacing(8)
    .into()
}

fn setting<'a>(
    config: &Config,
    label: &'a str,
    desc: &'a str,
    control: Element<'a, Msg>,
) -> Element<'a, Msg> {
    let muted: Color = config.colors.muted.into();
    let mut labels = column![text(label).size(14)].spacing(2);
    if !desc.is_empty() {
        labels = labels.push(text(desc).size(12).color(muted));
    }
    row![labels.width(Length::Fill), container(control).width(280)]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
}

fn number<'a>(
    config: &Config,
    label: &'a str,
    num: Num,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
) -> Element<'a, Msg> {
    let shown = if step < 1.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    };
    setting(
        config,
        label,
        "",
        row![
            slider(range, value, move |v| Msg::Num(num, v)).step(step),
            text(shown).size(13).width(44).align_x(Alignment::End)
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    )
}

fn section_editor<'a>(
    config: &'a Config,
    label: &'a str,
    section: Section,
    list: &'a [Widget],
) -> Element<'a, Msg> {
    let fg: Color = config.colors.foreground.into();
    let small = |glyph: &str, msg: Msg| {
        button(icon(config, glyph, 16.0))
            .padding(2)
            .on_press(msg)
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered)
                    .then(|| Color { a: 0.15, ..fg }.into()),
                text_color: fg,
                border: border::rounded(6),
                ..Default::default()
            })
    };
    let chips = row(list.iter().enumerate().map(|(i, w)| {
        container(
            row![
                small("chevron_left", Msg::Move(section, i, -1)),
                text(w.to_string()).size(13),
                small("chevron_right", Msg::Move(section, i, 1)),
                small("close", Msg::Remove(section, i)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([4, 6])
        .style(move |_| container::Style {
            background: Some(Color { a: 0.08, ..fg }.into()),
            border: border::rounded(8),
            ..Default::default()
        })
        .into()
    }))
    .spacing(6)
    .wrap();
    let add = pick_list(&Widget::ALL[..], None::<Widget>, move |w| {
        Msg::Add(section, w)
    })
    .placeholder("Add widget")
    .text_size(13)
    .padding([4, 8]);
    column![
        row![text(label).size(14).width(Length::Fill), add].align_y(Alignment::Center),
        chips
    ]
    .spacing(8)
    .into()
}

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
        Num::Radius => config.bar.radius = v,
        Num::Padding => config.bar.padding = v,
        Num::Spacing => config.bar.spacing = v,
        Num::FontSize => config.bar.font_size = v,
        Num::WorkspacesMin => config.workspaces.min = v.round() as i32,
        Num::TitleChars => config.window_title.max_chars = v.round() as usize,
        Num::AnimDuration => config.animations.duration_ms = v.round() as u64,
        Num::Fps => config.capture.fps = v.round() as u32,
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Color::from_rgba(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b), l(a.a, b.a))
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
