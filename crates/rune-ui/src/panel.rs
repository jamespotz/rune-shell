//! Panels: at most one open at a time, each in its own layer surface under the
//! bar. This module decides what is open, where, and how big; it knows nothing
//! about iced. Callers feed it pointer, bar and size facts and get back plain
//! [`Command`]s, which `lib.rs` turns into layer-shell tasks.

use crate::launcher::Kind;
use std::collections::HashMap;

/// Which panel; each has its own content and surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Launcher(Kind),
    Media,
    Notifications,
    TrayMenu,
    Weather,
    Calendar,
    Quick,
    Power,
}

/// Panels reachable from the CLI (`rune-shell toggle <name>`).
const NAMED: [(&str, Panel); 9] = [
    ("launcher", Panel::Launcher(Kind::Apps)),
    ("keybinds", Panel::Launcher(Kind::Keybinds)),
    ("clipboard", Panel::Launcher(Kind::Clipboard)),
    ("media", Panel::Media),
    ("notifications", Panel::Notifications),
    ("weather", Panel::Weather),
    ("calendar", Panel::Calendar),
    ("quick-settings", Panel::Quick),
    ("power", Panel::Power),
];

impl Panel {
    pub fn from_name(name: &str) -> Option<Self> {
        NAMED.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
    }

    /// CLI names, in help order.
    pub fn names() -> impl Iterator<Item = &'static str> {
        NAMED.iter().map(|(n, _)| *n)
    }

    /// Whether the panel takes the keyboard while open (it has a text field).
    pub fn grabs_keyboard(self) -> bool {
        matches!(self, Self::Launcher(_))
    }
}

/// Gap between the bar and a panel, and between a panel and the screen edge.
pub const GAP: i32 = 8;
/// The tallest a panel gets; longer content scrolls inside it.
pub const MAX_H: f32 = 440.0;

/// What the surface layer should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open `panel` (and its click-outside catcher) on `monitor`.
    /// `left`: distance from the screen's left edge; `None` = centered.
    Open {
        panel: Panel,
        monitor: String,
        size: (u32, u32),
        left: Option<i32>,
    },
    Resize {
        size: (u32, u32),
        left: Option<i32>,
    },
    /// Close the panel and its catcher.
    Close,
}

#[derive(Debug)]
struct Open {
    panel: Panel,
    monitor: String,
    /// Center x on the monitor; `None` = centered.
    x: Option<f32>,
    size: (u32, u32),
}

#[derive(Debug, Default)]
pub struct Panels {
    open: Option<Open>,
    /// Last size per panel, so reopening needs no resize.
    sizes: HashMap<Panel, (u32, u32)>,
    /// Pointer over a bar: (monitor, x).
    pointer: Option<(String, f32)>,
    /// Bar width per monitor (= the monitor's logical width).
    widths: HashMap<String, f32>,
}

impl Panels {
    pub fn current(&self) -> Option<Panel> {
        self.open.as_ref().map(|o| o.panel)
    }

    /// Monitor the open panel is on.
    pub fn monitor(&self) -> Option<&str> {
        self.open.as_ref().map(|o| o.monitor.as_str())
    }

    pub fn pointer(&mut self, at: Option<(String, f32)>) {
        self.pointer = at;
    }

    pub fn bar_width(&mut self, monitor: String, width: f32) {
        self.widths.insert(monitor, width);
    }

    /// Opens `panel`, switches to it in the same spot, or closes it if it's
    /// already open. Opened from a bar it sits under the pointer; otherwise
    /// it's centered on `focused`.
    pub fn toggle(&mut self, panel: Panel, focused: &str) -> Vec<Command> {
        if self.current() == Some(panel) {
            return self.close();
        }
        // Switching keeps the spot: the pointer is on the old panel, not the bar.
        let (spot, mut commands) = match self.open.take() {
            Some(old) => ((old.monitor, old.x), vec![Command::Close]),
            None => (self.spot(focused), Vec::new()),
        };
        commands.push(self.open_at(panel, spot));
        commands
    }

    /// Opens `panel` fresh under the pointer, never closing it (another tray
    /// icon's menu moves to that icon).
    pub fn reopen(&mut self, panel: Panel, focused: &str) -> Vec<Command> {
        let mut commands = self.close();
        let spot = self.spot(focused);
        commands.push(self.open_at(panel, spot));
        commands
    }

    pub fn close(&mut self) -> Vec<Command> {
        self.open
            .take()
            .map(|_| Command::Close)
            .into_iter()
            .collect()
    }

    /// The content's natural size, as measured by the panel's sensor.
    pub fn measured(&mut self, width: f32, height: f32) -> Option<Command> {
        let fit = (width.ceil() as u32, height.ceil() as u32);
        let open = self.open.as_mut()?;
        if fit.0 == 0 || fit.1 == 0 || fit == open.size {
            return None;
        }
        open.size = fit;
        self.sizes.insert(open.panel, fit);
        let left = left(open.x, fit.0, self.widths.get(&open.monitor).copied());
        Some(Command::Resize { size: fit, left })
    }

    fn spot(&self, focused: &str) -> (String, Option<f32>) {
        match &self.pointer {
            Some((monitor, x)) => (monitor.clone(), Some(*x)),
            None => (focused.to_owned(), None),
        }
    }

    fn open_at(&mut self, panel: Panel, (monitor, x): (String, Option<f32>)) -> Command {
        // Starts at last time's size; the sensor corrects it on the first frame.
        let size = self.sizes.get(&panel).copied().unwrap_or((1, 1));
        let left = left(x, size.0, self.widths.get(&monitor).copied());
        self.open = Some(Open {
            panel,
            monitor: monitor.clone(),
            x,
            size,
        });
        Command::Open {
            panel,
            monitor,
            size,
            left,
        }
    }
}

/// Left edge for a panel `width` wide centered on `x`, kept `GAP` from the
/// screen edges. Centered (`None`) without a spot or a known screen width.
fn left(x: Option<f32>, width: u32, screen: Option<f32>) -> Option<i32> {
    let (x, screen) = (x?, screen?);
    let (w, gap) = (width as f32, GAP as f32);
    Some(
        (x - w / 2.0)
            .clamp(gap, (screen - w - gap).max(gap))
            .round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels() -> Panels {
        let mut p = Panels::default();
        p.bar_width("DP-1".into(), 2000.0);
        p
    }

    #[test]
    fn opens_under_the_pointer_and_toggles_closed() {
        let mut p = panels();
        p.pointer(Some(("DP-1".into(), 1000.0)));
        let open = p.toggle(Panel::Quick, "HDMI-1");
        assert_eq!(
            open,
            [Command::Open {
                panel: Panel::Quick,
                monitor: "DP-1".into(),
                size: (1, 1),
                left: Some(1000),
            }]
        );
        assert_eq!(p.toggle(Panel::Quick, "HDMI-1"), [Command::Close]);
        assert_eq!(p.current(), None);
        assert!(p.close().is_empty(), "closing twice is a no-op");
    }

    #[test]
    fn without_pointer_it_centers_on_the_focused_monitor() {
        let mut p = panels();
        let open = p.toggle(Panel::Launcher(Kind::Apps), "HDMI-1");
        assert!(
            matches!(&open[..], [Command::Open { monitor, left: None, .. }] if monitor == "HDMI-1")
        );
    }

    #[test]
    fn switching_keeps_the_spot() {
        let mut p = panels();
        p.pointer(Some(("DP-1".into(), 600.0)));
        p.toggle(Panel::Quick, "");
        p.pointer(None); // pointer moved onto the panel
        let switch = p.toggle(Panel::Power, "HDMI-1");
        assert_eq!(switch[0], Command::Close);
        assert!(
            matches!(&switch[1], Command::Open { panel: Panel::Power, monitor, left: Some(600), .. } if monitor == "DP-1")
        );
    }

    #[test]
    fn reopen_moves_to_the_new_pointer_instead_of_closing() {
        let mut p = panels();
        p.pointer(Some(("DP-1".into(), 300.0)));
        p.toggle(Panel::TrayMenu, "");
        p.pointer(Some(("DP-1".into(), 900.0)));
        let again = p.reopen(Panel::TrayMenu, "");
        assert_eq!(again[0], Command::Close);
        assert!(matches!(
            &again[1],
            Command::Open {
                left: Some(900),
                ..
            }
        ));
        assert_eq!(p.current(), Some(Panel::TrayMenu));
    }

    #[test]
    fn measured_size_resizes_recenters_and_is_remembered() {
        let mut p = panels();
        p.pointer(Some(("DP-1".into(), 1000.0)));
        p.toggle(Panel::Quick, "");
        assert_eq!(
            p.measured(399.2, 300.0),
            Some(Command::Resize {
                size: (400, 300),
                left: Some(800),
            })
        );
        assert_eq!(p.measured(400.0, 300.0), None, "same size: nothing to do");
        assert_eq!(p.measured(0.0, 300.0), None, "ignore empty layouts");
        p.close();
        let reopen = p.toggle(Panel::Quick, "");
        assert!(matches!(
            &reopen[0],
            Command::Open {
                size: (400, 300),
                ..
            }
        ));
        assert!(p.measured(10.0, 10.0).is_some());
        p.close();
        assert_eq!(p.measured(10.0, 10.0), None, "nothing open");
    }

    #[test]
    fn placement_stays_on_screen() {
        assert_eq!(left(Some(10.0), 400, Some(2000.0)), Some(GAP));
        assert_eq!(
            left(Some(1990.0), 400, Some(2000.0)),
            Some(2000 - 400 - GAP)
        );
        assert_eq!(
            left(Some(100.0), 4000, Some(2000.0)),
            Some(GAP),
            "wider than screen"
        );
        assert_eq!(left(Some(100.0), 400, None), None, "unknown screen: center");
    }

    #[test]
    fn cli_names_round_trip() {
        for name in Panel::names() {
            assert!(Panel::from_name(name).is_some(), "{name}");
        }
        assert_eq!(Panel::from_name("quick-settings"), Some(Panel::Quick));
        assert_eq!(Panel::from_name("traymenu"), None);
    }
}
