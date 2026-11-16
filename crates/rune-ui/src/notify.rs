//! Notification stack geometry and text helpers.
use chrono::Local;
use rune_services::notifications::Notification;
#[cfg(test)]
use rune_services::notifications::Urgency;

/// Popup width, and the gap between toasts when fanned out.
pub const WIDTH: f32 = 360.0;
pub(crate) const GAP: f32 = 10.0;
/// How much each older toast peeks out below the one in front when stacked.
pub(crate) const PEEK: f32 = 14.0;
/// Older toasts visible behind the front one when stacked.
pub(crate) const BEHIND: usize = 2;
/// Most toasts shown when fanned out.
pub const MAX_SHOWN: usize = 5;

/// Fixed toast heights keep the stack layout (and the surface size) predictable.
pub(crate) fn toast_height(n: &Notification) -> f32 {
    if n.actions.iter().any(|(k, _)| k != "default") {
        122.0
    } else {
        86.0
    }
}

/// Surface height for `popups` at fan-out progress `t` (0 stacked, 1 fanned).
pub fn stack_height(popups: &[Notification], t: f32) -> f32 {
    let Some(front) = popups.first() else {
        return 0.0;
    };
    let stacked =
        toast_height(front) + PEEK * popups.len().min(BEHIND + 1).saturating_sub(1) as f32;
    let shown = &popups[..popups.len().min(MAX_SHOWN)];
    let fanned =
        shown.iter().map(toast_height).sum::<f32>() + GAP * shown.len().saturating_sub(1) as f32;
    stacked + (fanned - stacked) * t
}

pub(crate) fn ago(n: &Notification) -> String {
    let mins = (Local::now() - n.time).num_minutes();
    match mins {
        ..1 => "now".into(),
        1..60 => format!("{mins}m"),
        60..1440 => format!("{}h", mins / 60),
        _ => n.time.format("%b %d").to_string(),
    }
}

pub(crate) fn cut(s: &str, max: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    flat.chars()
        .take(max - 1)
        .chain(std::iter::once('…'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(actions: &[&str]) -> Notification {
        Notification {
            id: 1,
            app_name: String::new(),
            summary: String::new(),
            body: String::new(),
            image: None,
            actions: actions
                .iter()
                .map(|k| ((*k).to_owned(), (*k).to_owned()))
                .collect(),
            urgency: Urgency::Normal,
            time: Local::now(),
            resident: false,
        }
    }

    #[test]
    fn stack_heights() {
        assert_eq!(stack_height(&[], 1.0), 0.0);
        let three = [n(&[]), n(&["reply"]), n(&[])];
        assert_eq!(
            stack_height(&three, 0.0),
            86.0 + 2.0 * PEEK,
            "stacked: front plus two peeks"
        );
        assert_eq!(
            stack_height(&three, 1.0),
            86.0 + 122.0 + 86.0 + 2.0 * GAP,
            "fanned: every toast"
        );
        let many: Vec<_> = (0..9).map(|_| n(&["default"])).collect();
        assert_eq!(
            stack_height(&many, 0.0),
            86.0 + 2.0 * PEEK,
            "only two peek behind"
        );
        assert_eq!(
            stack_height(&many, 1.0),
            5.0 * 86.0 + 4.0 * GAP,
            "at most five fan out"
        );
    }

    #[test]
    fn cuts_long_text() {
        assert_eq!(cut("a\nb", 10), "a b");
        assert_eq!(cut("abcdef", 4), "abc…");
    }
}
