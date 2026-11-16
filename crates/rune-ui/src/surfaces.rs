//! Every layer surface Rune shows, by kind: one place that opens, resizes and
//! closes them, maps window ids back to kinds, and names their namespaces.

use crate::Message;
use iced::{Task, window};
use iced_layershell::reexport::NewLayerShellSettings;
use std::collections::HashMap;
use window::Id;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Surface {
    /// The notch bar on one monitor ("" = the compositor's choice).
    Bar(String),
    Panel,
    /// Transparent, full screen, under the panel: a click on it closes the panel.
    Catcher,
    /// Notification popups.
    Popups,
    Osd,
    Settings,
}

impl Surface {
    /// Layer namespace, for compositor rules (`hl.layer_rule`).
    pub fn namespace(&self) -> &'static str {
        match self {
            Self::Bar(_) => "rune-bar",
            Self::Panel => "rune-panel",
            Self::Catcher => "rune-catcher",
            Self::Popups => "rune-notifications",
            Self::Osd => "rune-osd",
            Self::Settings => "rune-settings",
        }
    }
}

#[derive(Debug, Default)]
pub struct Surfaces {
    ids: HashMap<Surface, Id>,
}

impl Surfaces {
    /// Opens `surface` unless it already is; the namespace is filled in here.
    pub fn open(&mut self, surface: Surface, settings: NewLayerShellSettings) -> Task<Message> {
        if self.ids.contains_key(&surface) {
            return Task::none();
        }
        let id = Id::unique();
        let settings = NewLayerShellSettings {
            namespace: Some(surface.namespace().to_owned()),
            ..settings
        };
        self.ids.insert(surface, id);
        Task::done(Message::NewLayerShell { settings, id })
    }

    pub fn close(&mut self, surface: &Surface) -> Task<Message> {
        self.ids
            .remove(surface)
            .map_or_else(Task::none, window::close)
    }

    pub fn resize(&self, surface: &Surface, size: (u32, u32)) -> Task<Message> {
        self.with(surface, |id| Message::SizeChange { id, size })
    }

    /// Margin: top, right, bottom, left.
    pub fn margin(&self, surface: &Surface, margin: (i32, i32, i32, i32)) -> Task<Message> {
        self.with(surface, |id| Message::MarginChange { id, margin })
    }

    pub fn is_open(&self, surface: &Surface) -> bool {
        self.ids.contains_key(surface)
    }

    /// Which surface a window id belongs to.
    pub fn kind(&self, id: Id) -> Option<&Surface> {
        self.ids.iter().find(|(_, i)| **i == id).map(|(s, _)| s)
    }

    /// Monitors that have a bar.
    pub fn bars(&self) -> Vec<String> {
        self.ids
            .keys()
            .filter_map(|s| match s {
                Surface::Bar(m) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }

    fn with(&self, surface: &Surface, msg: impl FnOnce(Id) -> Message) -> Task<Message> {
        self.ids
            .get(surface)
            .map_or_else(Task::none, |id| Task::done(msg(*id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_ids_by_kind() {
        let mut s = Surfaces::default();
        let _ = s.open(
            Surface::Bar("DP-1".into()),
            NewLayerShellSettings::default(),
        );
        let _ = s.open(Surface::Panel, NewLayerShellSettings::default());
        assert!(s.is_open(&Surface::Panel));
        assert_eq!(s.bars(), ["DP-1"]);
        let id = s.ids[&Surface::Panel];
        assert_eq!(s.kind(id), Some(&Surface::Panel));
        let _ = s.close(&Surface::Panel);
        assert!(!s.is_open(&Surface::Panel));
        assert_eq!(s.kind(id), None);
    }
}
