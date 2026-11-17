//! The panel that grows out of the notch: app launcher (with Steam games,
//! emoji via a `:` prefix, and a calculator) and the keybind cheat sheet.

#[path = "gpui_launcher.rs"]
pub(crate) mod gpui_view;

use crate::config::Config;
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use rune_services::{
    apps::App,
    calc,
    clipboard::{Entry as ClipEntry, Id as ClipId},
    hyprland::Bind,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// Height of one result row; used to keep the selection scrolled into view.
pub const ROW_H: f32 = 52.0;
pub const PANEL_W: f32 = 640.0;
/// Room for the result list so search and footer still fit under [`crate::panel::MAX_H`].
pub(super) const LIST_MAX_H: f32 = 330.0;

const MAX_RESULTS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
    /// Star or unstar a clipboard result.
    Favorite(usize),
    ClearClips,
}

/// What the launcher wants done after a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Launch(String),
    Copy(String),
    /// Put a clipboard entry back on the clipboard.
    Clip(ClipId),
    Favorite(ClipId),
    /// Wipe clipboard history (favorites stay).
    ClearClips,
}

#[derive(Debug, Default)]
pub struct Launcher {
    pub kind: Kind,
    query: String,
    selected: usize,
    results: Vec<Hit>,
    apps: Vec<App>,
    /// Icons without a usable extension, loaded from their bytes (see [`app_icon`]).
    sniffed: HashMap<PathBuf, Arc<[u8]>>,
    binds: Vec<Bind>,
    clips: Vec<ClipEntry>,
}

impl Launcher {
    /// Starts a fresh search and focuses the search box.
    pub fn reset(&mut self, kind: Kind) {
        self.kind = kind;
        self.query.clear();
        self.refilter();
    }

    pub fn set_apps(&mut self, apps: Vec<App>) {
        self.sniffed = apps
            .iter()
            .filter_map(|a| a.icon.as_ref())
            .filter(|p| {
                !p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(known_ext)
            })
            .filter_map(|p| {
                let bytes = std::fs::read(p).ok()?;
                // Decode extensionless raster icons from bytes.
                let png = bytes.starts_with(b"\x89PNG");
                let jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
                (png || jpeg).then(|| (p.clone(), Arc::from(bytes)))
            })
            .collect();
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
    pub fn select(&mut self, by: isize) {
        if !self.results.is_empty() {
            self.selected = self
                .selected
                .saturating_add_signed(by)
                .min(self.results.len() - 1);
        }
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
            Msg::Favorite(i) => match self.results.get(i)? {
                Hit::Clip(c) => return Some(Outcome::Favorite(self.clips.get(*c)?.id)),
                _ => return None,
            },
            Msg::ClearClips => return Some(Outcome::ClearClips),
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
            Hit::Clip(i) => Some(Outcome::Clip(self.clips.get(*i)?.id)),
        }
    }

    fn refilter(&mut self) {
        self.selected = 0;
        self.results = match self.kind {
            Kind::Clipboard => {
                let hay = |c: &ClipEntry| c.title.clone();
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

/// "5 min ago" for recent copies, the date for older ones.
fn ago(time: Option<i64>, now: i64) -> String {
    let Some(time) = time else {
        return String::new();
    };
    match (now - time).max(0) {
        0..60 => "Just now".into(),
        s @ 60..3600 => format!("{} min ago", s / 60),
        s @ 3600..86400 => format!("{} h ago", s / 3600),
        _ => chrono::DateTime::from_timestamp(time, 0)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%b %-d, %H:%M")
                    .to_string()
            })
            .unwrap_or_default(),
    }
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

/// Common image formats decoded by GPUI from a file path.
fn known_ext(ext: &str) -> bool {
    ["svg", "png", "jpg", "jpeg", "webp", "gif", "bmp", "ico"]
        .contains(&ext.to_ascii_lowercase().as_str())
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
    fn clip_times_read_naturally() {
        assert_eq!(ago(None, 100), "");
        assert_eq!(ago(Some(100), 130), "Just now");
        assert_eq!(ago(Some(0), 300), "5 min ago");
        assert_eq!(ago(Some(0), 7300), "2 h ago");
        assert!(!ago(Some(0), 200_000).is_empty());
    }

    #[test]
    fn colon_searches_emoji() {
        let mut l = Launcher::default();
        let config = Config::default();
        l.update(Msg::Query(":thumbs up".into()), &config);
        assert!(matches!(l.results.first(), Some(Hit::Emoji(e)) if e.as_str() == "👍"));
    }
}
