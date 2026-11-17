//! Notification stack geometry and text helpers.
use crate::animation::Animation;
use chrono::Local;
use rune_services::notifications::Notification;
#[cfg(test)]
use rune_services::notifications::Urgency;
use std::time::{Duration, Instant};

/// Toast width, as in Sonner.
pub const WIDTH: f32 = 356.0;
/// Fixed toast height keeps the stack layout (and the surface size) predictable.
pub(crate) const HEIGHT: f32 = 88.0;
/// Offset between stacked toasts, and the gap between them when expanded.
pub(crate) const GAP: f32 = 14.0;
/// Toasts shown at once.
pub const VISIBLE: usize = 3;
/// Room around the stack for the close buttons and shadows.
pub(crate) const PAD: f32 = 12.0;

/// Surface height for `popups` at expand progress `t` (0 stacked, 1 expanded).
pub fn stack_height(popups: &[Notification], t: f32) -> f32 {
    let shown = popups.len().min(VISIBLE) as f32;
    if shown == 0.0 {
        return 0.0;
    }
    HEIGHT + GAP * (shown - 1.0) + HEIGHT * (shown - 1.0) * t + 2.0 * PAD
}

/// A shown toast's entrance and its slot in the stack, both animated.
#[derive(Debug, Clone)]
pub(crate) struct Motion {
    enter: Animation<bool>,
    from: f32,
    to: f32,
    shift: Animation<bool>,
    duration: Duration,
}

impl Motion {
    pub fn new(slot: f32, duration: Duration, now: Instant) -> Self {
        Self {
            enter: Animation::new(false).duration(duration).go(true, now),
            from: slot,
            to: slot,
            shift: Animation::new(true).duration(duration),
            duration,
        }
    }

    pub fn move_to(&mut self, slot: f32, now: Instant) {
        if self.to == slot {
            return;
        }
        self.from = self.depth(now);
        self.to = slot;
        self.shift = Animation::new(false).duration(self.duration).go(true, now);
    }

    /// Stack position: 0 is the front toast.
    pub fn depth(&self, now: Instant) -> f32 {
        self.shift.interpolate(self.from, self.to, now)
    }

    pub fn entered(&self, now: Instant) -> f32 {
        self.enter.interpolate(0., 1., now)
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.enter.is_animating(now) || self.shift.is_animating(now)
    }
}

/// A toast that was closed and is animating out.
#[derive(Debug, Clone)]
pub(crate) struct Leaving {
    pub notification: Notification,
    pub depth: f32,
    /// Horizontal drag offset when it was swiped away.
    pub swipe: Option<f32>,
    pub fade: Animation<bool>,
}

/// How far, or how fast in px/ms, a toast must be dragged to dismiss it.
pub(crate) const SWIPE_DISTANCE: f32 = 45.0;
pub(crate) const SWIPE_VELOCITY: f32 = 0.11;

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

    fn n() -> Notification {
        Notification {
            id: 1,
            app_name: String::new(),
            summary: String::new(),
            body: String::new(),
            image: None,
            actions: Vec::new(),
            urgency: Urgency::Normal,
            time: Local::now(),
            resident: false,
        }
    }

    #[test]
    fn stack_heights() {
        assert_eq!(stack_height(&[], 1.0), 0.0);
        let pad = 2.0 * PAD;
        assert_eq!(stack_height(&[n()], 1.0), HEIGHT + pad);
        let many: Vec<_> = (0..9).map(|_| n()).collect();
        assert_eq!(
            stack_height(&many, 0.0),
            HEIGHT + 2.0 * GAP + pad,
            "stacked: front plus two peeks"
        );
        assert_eq!(
            stack_height(&many, 1.0),
            3.0 * HEIGHT + 2.0 * GAP + pad,
            "expanded: three toasts"
        );
    }

    #[test]
    fn motion_shifts_from_where_it_is() {
        let now = Instant::now();
        let duration = Duration::from_millis(400);
        let mut motion = Motion::new(0., duration, now);
        assert_eq!(motion.entered(now), 0.);
        motion.move_to(1., now);
        assert_eq!(motion.depth(now), 0.);
        let later = now + Duration::from_secs(2);
        assert_eq!(motion.depth(later), 1.);
        assert_eq!(motion.entered(later), 1.);
        assert!(!motion.is_animating(later));
    }

    #[test]
    fn cuts_long_text() {
        assert_eq!(cut("a\nb", 10), "a b");
        assert_eq!(cut("abcdef", 4), "abc…");
    }
}
