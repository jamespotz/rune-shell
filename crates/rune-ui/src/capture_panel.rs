//! The capture panel's modes, their targets, and the keybinds shown for them.

use rune_services::capture::Target;
use std::time::Duration;

pub const CAPTURE_W: f32 = 372.0;
/// Time for the panel surface to leave the screen before grim or slurp runs.
pub const SETTLE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptureMode {
    #[default]
    Screenshot,
    Record,
    Text,
}

pub fn targets(mode: CaptureMode) -> &'static [Target] {
    match mode {
        CaptureMode::Screenshot => &[
            Target::Selection,
            Target::Window,
            Target::Screen,
            Target::All,
        ],
        CaptureMode::Record => &[Target::Selection, Target::Window, Target::Screen],
        CaptureMode::Text => &[],
    }
}

/// Suggested Hyprland keys and the `rune-shell` arguments they run, in display order.
pub const SHORTCUTS: [(&str, &str); 8] = [
    ("Print", "screenshot selection"),
    ("SHIFT + Print", "screenshot window"),
    ("CTRL + Print", "screenshot screen"),
    ("SUPER + Print", "record selection"),
    ("SUPER + SHIFT + Print", "record window"),
    ("SUPER + CTRL + Print", "record screen"),
    ("SUPER + SHIFT + R", "toggle capture"),
    ("SUPER + SHIFT + T", "ocr"),
];

pub fn shortcut(mode: CaptureMode, target: Target) -> Option<&'static str> {
    let verb = match mode {
        CaptureMode::Screenshot => "screenshot",
        CaptureMode::Record => "record",
        CaptureMode::Text => "ocr",
    };
    SHORTCUTS.iter().find_map(|(keys, args)| {
        let matches = match mode {
            CaptureMode::Text => *args == verb,
            _ => args.strip_prefix(verb).and_then(|rest| rest.strip_prefix(' '))
                == Some(target.word()),
        };
        matches.then_some(*keys)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_has_no_all_and_text_has_no_targets() {
        assert_eq!(
            targets(CaptureMode::Screenshot),
            [
                Target::Selection,
                Target::Window,
                Target::Screen,
                Target::All
            ]
        );
        assert_eq!(
            targets(CaptureMode::Record),
            [Target::Selection, Target::Window, Target::Screen]
        );
        assert!(targets(CaptureMode::Text).is_empty());
    }

    #[test]
    fn tiles_show_the_settings_shortcuts() {
        assert_eq!(
            shortcut(CaptureMode::Screenshot, Target::Selection),
            Some("Print")
        );
        assert_eq!(
            shortcut(CaptureMode::Record, Target::Window),
            Some("SUPER + SHIFT + Print")
        );
        assert_eq!(
            shortcut(CaptureMode::Record, Target::Screen),
            Some("SUPER + CTRL + Print")
        );
        assert_eq!(shortcut(CaptureMode::Screenshot, Target::All), None);
        assert!(SHORTCUTS.contains(&("SUPER + SHIFT + R", "toggle capture")));
    }
}
