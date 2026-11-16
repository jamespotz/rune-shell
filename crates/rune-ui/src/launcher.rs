//! The panel that grows out of the notch: app launcher (with Steam games,
//! emoji via a `:` prefix, and a calculator) and the keybind cheat sheet.

use crate::{config::Config, notch::icon};
use iced::{
    Alignment, Color, Element, Length, Task, border,
    widget::{self, Id, button, column, container, image, row, scrollable, svg, text, text_input},
};
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use rune_services::{apps::App, calc, clipboard::Entry as ClipEntry, hyprland::Bind};

/// Height of one result row; used to keep the selection scrolled into view.
pub const ROW_H: f32 = 52.0;
/// Panel size below the bar row while expanded.
pub const PANEL_W: f32 = 640.0;
pub const PANEL_H: f32 = 440.0;

const SEARCH: &str = "launcher-search";
const LIST: &str = "launcher-list";
const MAX_RESULTS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Apps,
    Keybinds,
    Clipboard,
}

#[derive(Debug, Clone, PartialEq)]
enum Hit {
    App(usize),
    Calc(String),
    Emoji(&'static emojis::Emoji),
    Bind(usize),
    Clip(usize),
}

#[derive(Debug, Clone)]
pub enum Msg {
    Query(String),
    Submit,
    Activate(usize),
}

/// What the launcher wants done after a message.
pub enum Outcome {
    Launch(String),
    Copy(String),
    /// Restore a clipboard history entry (cliphist id).
    Clip(String),
}

#[derive(Default)]
pub struct Launcher {
    pub kind: Kind,
    query: String,
    selected: usize,
    results: Vec<Hit>,
    apps: Vec<App>,
    binds: Vec<Bind>,
    clips: Vec<ClipEntry>,
}

impl Launcher {
    /// Starts a fresh search and focuses the search box.
    pub fn reset<M: Send + 'static>(&mut self, kind: Kind) -> Task<M> {
        self.kind = kind;
        self.query.clear();
        self.refilter();
        widget::operation::focus(Id::new(SEARCH))
    }

    pub fn set_apps(&mut self, apps: Vec<App>) {
        self.apps = apps;
        self.refilter();
    }

    pub fn set_clips(&mut self, clips: Vec<ClipEntry>) {
        self.clips = clips;
        self.refilter();
    }

    pub fn set_binds(&mut self, binds: Vec<Bind>) {
        self.binds = binds;
        self.refilter();
    }

    /// Moves the selection and scrolls it into view.
    pub fn select<M: Send + 'static>(&mut self, by: isize) -> Task<M> {
        if self.results.is_empty() {
            return Task::none();
        }
        let last = self.results.len() - 1;
        self.selected = self.selected.saturating_add_signed(by).min(last);
        // Keep a few rows of context above the selection.
        let y = (self.selected as f32 - 3.0).max(0.0) * ROW_H;
        widget::operation::scroll_to(
            Id::new(LIST),
            scrollable::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    }

    /// Applies `msg`; returns what to do if an entry was activated.
    pub fn update(&mut self, msg: Msg, config: &Config) -> Option<Outcome> {
        let index = match msg {
            Msg::Query(q) => {
                self.query = q;
                self.refilter();
                return None;
            }
            Msg::Submit => self.selected,
            Msg::Activate(i) => i,
        };
        match self.results.get(index)? {
            Hit::App(i) => {
                let app = self.apps.get(*i)?;
                let command = if app.terminal {
                    format!("{} -e {}", config.launcher.terminal, app.command)
                } else {
                    app.command.clone()
                };
                Some(Outcome::Launch(command))
            }
            Hit::Calc(result) => Some(Outcome::Copy(result.clone())),
            Hit::Emoji(e) => Some(Outcome::Copy(e.as_str().to_owned())),
            Hit::Bind(_) => None,
            Hit::Clip(i) => Some(Outcome::Clip(self.clips.get(*i)?.id.clone())),
        }
    }

    fn refilter(&mut self) {
        self.selected = 0;
        self.results = match self.kind {
            Kind::Clipboard => {
                let hay = |c: &ClipEntry| c.preview.clone();
                ranked(&self.query, &self.clips, hay)
                    .into_iter()
                    .map(Hit::Clip)
                    .collect()
            }
            Kind::Keybinds => {
                let hay = |b: &Bind| format!("{} {}", b.description, b.keys);
                ranked(&self.query, &self.binds, hay)
                    .into_iter()
                    .map(Hit::Bind)
                    .collect()
            }
            Kind::Apps => match self.query.strip_prefix(':') {
                Some(q) => emoji(q).into_iter().map(Hit::Emoji).collect(),
                None => {
                    let hay = |a: &App| format!("{} {}", a.name, a.keywords.join(" "));
                    let apps = ranked(&self.query, &self.apps, hay)
                        .into_iter()
                        .map(Hit::App);
                    calc::eval(&self.query)
                        .map(Hit::Calc)
                        .into_iter()
                        .chain(apps)
                        .collect()
                }
            },
        };
    }

    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        config: &'a Config,
        wrap: fn(Msg) -> M,
    ) -> Element<'a, M> {
        let fg: Color = config.colors.foreground.into();
        let muted: Color = config.colors.muted.into();
        let accent: Color = config.colors.accent.into();
        let placeholder = match self.kind {
            Kind::Apps => "Search apps and games  ·  :emoji  ·  2+2",
            Kind::Keybinds => "Search keybinds",
            Kind::Clipboard => "Search clipboard history",
        };
        let search = text_input(placeholder, &self.query)
            .id(Id::new(SEARCH))
            .on_input(move |q| wrap(Msg::Query(q)))
            .on_submit(wrap(Msg::Submit))
            .padding([10, 14])
            .size(16)
            .style(move |theme: &iced::Theme, status| text_input::Style {
                background: Color { a: 0.06, ..fg }.into(),
                border: border::rounded(12).color(Color { a: 0.12, ..fg }).width(1),
                ..text_input::default(theme, status)
            });

        let rows = self.results.iter().enumerate().map(|(i, hit)| {
            let (lead, title, sub): (Element<'a, M>, String, String) = match hit {
                Hit::App(a) => {
                    let app = &self.apps[*a];
                    (app_icon(config, app), app.name.clone(), app.comment.clone())
                }
                Hit::Calc(r) => (
                    icon(config, "calculate", 28.0),
                    format!("= {r}"),
                    "Enter copies the result".into(),
                ),
                Hit::Emoji(e) => (
                    text(e.as_str())
                        .size(26)
                        .shaping(text::Shaping::Advanced)
                        .into(),
                    e.name().to_owned(),
                    e.shortcode().map(|s| format!(":{s}:")).unwrap_or_default(),
                ),
                Hit::Clip(i) => {
                    let clip = &self.clips[*i];
                    let image = clip.preview.starts_with("[[ binary");
                    let glyph = if image { "image" } else { "content_paste" };
                    (
                        icon(config, glyph, 24.0),
                        clip.preview.chars().take(90).collect(),
                        String::new(),
                    )
                }
                Hit::Bind(b) => {
                    let bind = &self.binds[*b];
                    let sub = if bind.submap.is_empty() {
                        String::new()
                    } else {
                        format!("submap: {}", bind.submap)
                    };
                    (keys(config, &bind.keys), bind.description.clone(), sub)
                }
            };
            let selected = i == self.selected;
            let label = column![
                text(title)
                    .size(15)
                    .wrapping(text::Wrapping::None)
                    .shaping(text::Shaping::Advanced),
                text(sub)
                    .size(12)
                    .color(muted)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(2);
            button(row![lead, label].spacing(14).align_y(Alignment::Center))
                .width(Length::Fill)
                .height(ROW_H)
                .padding([6, 12])
                .on_press(wrap(Msg::Activate(i)))
                .style(move |_, status| button::Style {
                    background: Some(
                        if selected {
                            Color { a: 0.18, ..accent }
                        } else if matches!(status, button::Status::Hovered) {
                            Color { a: 0.07, ..fg }
                        } else {
                            Color::TRANSPARENT
                        }
                        .into(),
                    ),
                    text_color: fg,
                    border: border::rounded(10),
                    ..Default::default()
                })
                .into()
        });
        let empty = self.results.is_empty();
        let list: Element<'a, M> = if empty {
            container(text("No matches").color(muted))
                .center(Length::Fill)
                .into()
        } else {
            scrollable(column(rows).spacing(2))
                .id(Id::new(LIST))
                .height(Length::Fill)
                .into()
        };
        let hint = match self.kind {
            Kind::Apps => "↑↓ select   Enter open   Esc close",
            Kind::Keybinds => "↑↓ scroll   Esc close",
            Kind::Clipboard => "↑↓ select   Enter copy   Esc close",
        };
        column![search, list, text(hint).size(11).color(muted)]
            .spacing(10)
            .into()
    }
}

/// Indices of `items` matching `query`, best first. An empty query keeps the original order.
fn ranked<T>(query: &str, items: &[T], hay: impl Fn(&T) -> String) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            Some((
                pattern.score(Utf32Str::new(&hay(item), &mut buf), &mut matcher)?,
                i,
            ))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// Emoji whose name or shortcode contains every word of `query`.
fn emoji(query: &str) -> Vec<&'static emojis::Emoji> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    emojis::iter()
        .filter(|e| {
            let hay = format!(
                "{} {}",
                e.name(),
                e.shortcodes().collect::<Vec<_>>().join(" ")
            );
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .take(MAX_RESULTS)
        .collect()
}

fn app_icon<'a, M: 'a>(config: &Config, app: &App) -> Element<'a, M> {
    const SIZE: f32 = 32.0;
    match &app.icon {
        Some(path) if path.extension().is_some_and(|e| e == "svg") => {
            svg(svg::Handle::from_path(path))
                .width(SIZE)
                .height(SIZE)
                .into()
        }
        Some(path) => image(image::Handle::from_path(path))
            .width(SIZE)
            .height(SIZE)
            .into(),
        None => {
            let glyph = if app.steam { "sports_esports" } else { "apps" };
            container(icon(config, glyph, 26.0)).center(SIZE).into()
        }
    }
}

/// `SUPER + SHIFT + Q` as key caps.
fn keys<'a, M: 'a>(config: &Config, combo: &str) -> Element<'a, M> {
    let fg: Color = config.colors.foreground.into();
    row(combo.split(" + ").map(|k| {
        container(text(k.to_owned()).size(12))
            .padding([2, 7])
            .style(move |_| container::Style {
                background: Some(Color { a: 0.1, ..fg }.into()),
                border: border::rounded(6).color(Color { a: 0.2, ..fg }).width(1),
                ..Default::default()
            })
            .into()
    }))
    .spacing(4)
    .width(190)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, keywords: &[&str]) -> App {
        App {
            id: name.into(),
            name: name.into(),
            comment: String::new(),
            keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
            command: name.to_lowercase(),
            terminal: name == "btop",
            icon: None,
            steam: false,
        }
    }

    #[test]
    fn ranks_fuzzy_and_keeps_order_when_empty() {
        let apps = [
            app("Files", &[]),
            app("Firefox", &["web", "browser"]),
            app("Kitty", &[]),
        ];
        let hay = |a: &App| format!("{} {}", a.name, a.keywords.join(" "));
        assert_eq!(ranked("", &apps, hay), [0, 1, 2]);
        assert_eq!(ranked("ffx", &apps, hay), [1]);
        assert_eq!(ranked("browser", &apps, hay), [1]);
        assert!(ranked("zzz", &apps, hay).is_empty());
    }

    #[test]
    fn calculator_comes_first_and_copies() {
        let mut l = Launcher::default();
        l.set_apps(vec![app("Calculator", &[])]);
        let config = Config::default();
        l.update(Msg::Query("12*3".into()), &config);
        assert_eq!(l.results.first(), Some(&Hit::Calc("36".into())));
        assert!(matches!(l.update(Msg::Submit, &config), Some(Outcome::Copy(r)) if r == "36"));
    }

    #[test]
    fn terminal_apps_run_in_the_terminal() {
        let mut l = Launcher::default();
        l.set_apps(vec![app("btop", &[])]);
        let config = Config::default();
        l.update(Msg::Query("btop".into()), &config);
        let cmd = match l.update(Msg::Submit, &config) {
            Some(Outcome::Launch(c)) => c,
            _ => String::new(),
        };
        assert_eq!(cmd, format!("{} -e btop", config.launcher.terminal));
    }

    #[test]
    fn colon_searches_emoji() {
        let mut l = Launcher::default();
        let config = Config::default();
        l.update(Msg::Query(":thumbs up".into()), &config);
        assert!(matches!(l.results.first(), Some(Hit::Emoji(e)) if e.as_str() == "👍"));
    }
}
