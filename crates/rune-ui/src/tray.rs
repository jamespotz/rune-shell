//! Tray mouse actions and dimensions.

/// Which mouse button clicked a tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Left,
    Middle,
    Right,
}

pub const MENU_W: f32 = 220.0;
