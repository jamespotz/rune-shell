//! Toolkit-independent notch layout helpers.
use crate::config::Config;
use gpui::Rgba as Color;
use rune_services::hyprland::HyprState;

/// Workspace ids to show: `1..=min` plus every existing normal workspace.
pub fn workspace_ids(config: &Config, hypr: &HyprState) -> Vec<i32> {
    let mut ids: Vec<i32> = (1..=config.workspaces.min).collect();
    // Negative ids are special workspaces (scratchpads); they don't get a pill.
    ids.extend(hypr.workspaces.iter().map(|w| w.id).filter(|&id| id > 0));
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Cuts `s` to `max` characters, ending in "…" when shortened.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub(crate) fn widget_panel(widget: crate::config::Widget) -> Option<crate::Panel> {
    use crate::{Panel, config::Widget};
    Some(match widget {
        Widget::Volume => Panel::Sound,
        Widget::Network => Panel::Wifi,
        Widget::Bluetooth => Panel::Bluetooth,
        Widget::Battery => Panel::Battery,
        Widget::Weather => Panel::Weather,
        Widget::Clock => Panel::Calendar,
        Widget::Notifications => Panel::Notifications,
        Widget::QuickSettings => Panel::Quick,
        Widget::Media => Panel::Media,
        Widget::Power => Panel::Power,
        Widget::Capture => Panel::Capture,
        _ => return None,
    })
}

pub(crate) fn bar_has(bar: &crate::config::BarConfig, widget: crate::config::Widget) -> bool {
    [&bar.left, &bar.center, &bar.right]
        .iter()
        .any(|section| section.contains(&widget))
}

pub(crate) fn media_label(media: &rune_services::media::Media, max_chars: usize) -> String {
    let label = match (media.title.is_empty(), media.artist.is_empty()) {
        (true, _) => media.identity.clone(),
        (false, true) => media.title.clone(),
        (false, false) => format!("{} — {}", media.title, media.artist),
    };
    truncate(&label, max_chars)
}

/// Linear blend from `a` (t = 0) to `b` (t = 1).
pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Color {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_widget_opens_capture_panel_and_is_found_in_any_section() {
        use crate::{Panel, config::Widget};
        assert_eq!(widget_panel(Widget::Capture), Some(Panel::Capture));
        let mut bar = Config::default().bar;
        assert!(!bar_has(&bar, Widget::Capture));
        bar.center.push(Widget::Capture);
        assert!(bar_has(&bar, Widget::Capture));
    }
    use rune_services::hyprland::Workspace;

    #[test]
    fn workspace_ids_merge_min_and_existing() {
        let ws = |id| Workspace {
            id,
            name: id.to_string(),
            monitor: "DP-1".into(),
            windows: 1,
            last_window_title: String::new(),
        };
        let hypr = HyprState {
            workspaces: vec![ws(7), ws(2), ws(-98)],
            ..Default::default()
        };
        assert_eq!(workspace_ids(&Config::default(), &hypr), [1, 2, 3, 4, 5, 7]);
    }

    #[test]
    fn truncates_by_chars() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("ünïcödé title", 5), "ünïc…");
    }

    #[test]
    fn widgets_map_to_their_dropdowns() {
        use crate::{Panel, config::Widget};
        assert_eq!(widget_panel(Widget::Volume), Some(Panel::Sound));
        assert_eq!(widget_panel(Widget::Network), Some(Panel::Wifi));
        assert_eq!(widget_panel(Widget::Clock), Some(Panel::Calendar));
        assert_eq!(widget_panel(Widget::Workspaces), None);
    }

    #[test]
    fn media_label_joins_and_truncates() {
        let media = |title: &str, artist: &str| rune_services::media::Media {
            player: "test".into(),
            identity: "Player".into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            art: None,
            length: None,
            status: rune_services::media::Status::Playing,
            position: std::time::Duration::ZERO,
            read_at: std::time::Instant::now(),
            track_id: None,
        };
        assert_eq!(media_label(&media("Song", "Band"), 40), "Song — Band");
        assert_eq!(media_label(&media("Song", ""), 40), "Song");
        assert_eq!(media_label(&media("", ""), 40), "Player");
        assert_eq!(
            media_label(&media(&"x".repeat(200), "Band"), 40)
                .chars()
                .count(),
            40
        );
    }

    #[test]
    fn mixes_colors() {
        let m = mix(gpui::rgb(0x000000), gpui::rgb(0xffffff), 0.5);
        assert!((m.r - 0.5).abs() < 1e-6 && (m.a - 1.0).abs() < 1e-6);
    }
}
