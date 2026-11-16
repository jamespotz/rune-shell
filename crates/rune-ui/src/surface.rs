//! Toolkit-independent window identities and compositor namespaces.

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
