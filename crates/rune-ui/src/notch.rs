//! The notch: a solid island hanging from the top edge, with concave
//! "shoulders" flaring into the edge like a MacBook display notch.

use crate::{
    App, Message, Panel, bt, calendar,
    config::{Config, Widget},
    media, notify, quick, status, tray, weather,
};
use iced::{
    Alignment, Color, Element, Font, Length, Point, Rectangle, Renderer, Theme, border, mouse,
    widget::{Space, button, canvas, container, mouse_area, row, scrollable, sensor, text},
};
use rune_services::hyprland::HyprState;
use std::sync::Mutex;

/// Workspace ids to show: `1..=min` plus every existing normal workspace.
pub fn workspace_ids(config: &Config, hypr: &HyprState) -> Vec<i32> {
    let mut ids: Vec<i32> = (1..=config.workspaces.min).collect();
    // Negative ids are special workspaces (scratchpads); they don't get a pill.
    ids.extend(hypr.workspaces.iter().map(|w| w.id).filter(|&id| id > 0));
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// iced wants `&'static str` font names; config names change at runtime, so
/// each distinct name is leaked once and reused.
pub fn font(name: &str) -> Font {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let Ok(mut names) = NAMES.lock() else {
        return Font::DEFAULT;
    };
    let name = match names.iter().find(|n| **n == name) {
        Some(n) => *n,
        None => {
            let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
            names.push(leaked);
            leaked
        }
    };
    Font::with_name(name)
}

/// An icon from the icon font, written by its ligature name (`"schedule"`).
pub fn icon<'a, M: 'a>(config: &Config, name: &str, size: f32) -> Element<'a, M> {
    text(name.to_owned())
        .font(font(&config.bar.icon_font))
        .size(size)
        .line_height(1.0)
        .shaping(text::Shaping::Advanced)
        .into()
}

pub fn view<'a>(app: &'a App, monitor: &str) -> Element<'a, Message> {
    let bar = &app.config.bar;
    let h = bar.height as f32;
    let r = bar.radius.clamp(0.0, h / 2.0);
    let mut sections: Vec<Element<'a, Message>> = [&bar.left, &bar.center, &bar.right]
        .into_iter()
        .filter(|ws| !ws.is_empty())
        .map(|ws| {
            row(ws.iter().map(|w| widget(app, monitor, *w)))
                .spacing(bar.spacing)
                .align_y(Alignment::Center)
                .into()
        })
        .collect();
    // Recording and game mode show up on their own, whatever the layout.
    if let Some(status) = status(app) {
        sections.push(status);
    }
    let bg: Color = app.config.colors.background.into();
    let row_content = container(
        row(sections)
            .spacing(bar.spacing * 2.0)
            .align_y(Alignment::Center),
    )
    .center_y(h);
    // The sensor reports the content's natural width; the notch then glides to it.
    // Only `on_resize`: with `on_show` too, sensor stops reporting resizes inside a scrollable.
    let resized = monitor.to_owned();
    let measured =
        sensor(row_content).on_resize(move |s| Message::NotchWidth(resized.clone(), s.width));
    let bar_row: Element<'a, Message> = match app.notch_width(monitor) {
        // A horizontal viewport never squeezes its content, so the sensor always
        // measures the real width while the visible width animates.
        // The row matters: a scrollable stretches its content to at least the viewport
        // width, which would hide shrinking; inside a row the sensor gets its natural size.
        Some(w) => scrollable(row![measured])
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0),
            ))
            .width(w)
            .height(h)
            .into(),
        None => measured.into(),
    };

    let body = container(bar_row)
        .padding([0.0, bar.padding + r / 2.0])
        .style(move |_| container::Style {
            background: Some(bg.into()),
            border: border::rounded(border::bottom(r)),
            ..Default::default()
        });
    let shoulder = |left| canvas(Shoulder { left, color: bg }).width(r).height(r);
    let notch = row![shoulder(true), body, shoulder(false)];
    let page = container(notch)
        .center_x(Length::Fill)
        .align_top(Length::Fill);
    // Where the pointer is on the bar, so a panel can open under the widget clicked.
    // The page spans the monitor, so its width is the monitor's width.
    let (moved, sized) = (monitor.to_owned(), monitor.to_owned());
    sensor(
        mouse_area(page)
            .on_move(move |p| Message::BarCursor(Some((moved.clone(), p.x))))
            .on_exit(Message::BarCursor(None)),
    )
    .on_resize(move |s| Message::BarWidth(sized.clone(), s.width))
    .into()
}

/// The open panel, in its own layer surface; sized to its content.
pub fn panel(app: &App) -> Element<'_, Message> {
    let inner: Element<'_, Message> = match app.panels.current() {
        None => return Space::new().into(),
        Some(Panel::Launcher(_)) => app.launcher.view(&app.config, Message::Launcher),
        Some(Panel::Media) => media::panel(app),
        Some(Panel::Notifications) => notify::center(app),
        Some(Panel::TrayMenu) => tray::menu(app),
        Some(Panel::Weather) => weather::panel(app),
        Some(Panel::Calendar) => calendar::panel(app),
        Some(Panel::Quick) => quick::panel(app),
        Some(Panel::Power) => quick::power(app),
    };
    let bg: Color = app.config.colors.background.into();
    let fg: Color = app.config.colors.foreground.into();
    // Each panel sets its own width and is as tall as its content (capped); the
    // sensor reports that and the surface is resized to match. Inside a scrollable
    // the content is never squeezed by the (possibly still too small) surface.
    let measured = sensor(
        // Explicit Shrink: a Fill child would otherwise make the container Fill,
        // and Fill inside the row below collapses to 0.
        container(inner)
            .width(Length::Shrink)
            .height(Length::Shrink)
            .padding(16)
            .max_height(crate::panel::MAX_H)
            .style(move |_| container::Style {
                background: Some(bg.into()),
                border: border::rounded(20).color(Color { a: 0.08, ..fg }).width(1),
                ..Default::default()
            }),
    )
    .on_resize(Message::PanelSize);
    let hidden = scrollable::Scrollbar::new().width(0).scroller_width(0);
    // Clicks on the panel itself must not reach anything that would close it.
    mouse_area(
        scrollable(row![measured])
            .direction(scrollable::Direction::Both {
                vertical: hidden,
                horizontal: hidden,
            })
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(Message::Noop)
    .into()
}

/// Transparent full-screen layer under the panel: a click anywhere else closes it.
pub fn catcher(_: &App) -> Element<'_, Message> {
    mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(Message::ClosePanel)
        .on_right_press(Message::ClosePanel)
        .into()
}

fn widget<'a>(app: &'a App, monitor: &str, w: Widget) -> Element<'a, Message> {
    let c = &app.config;
    let colors = &c.colors;
    let size = c.bar.font_size;
    let label = |s: String, color: Color| text(s).font(font(&c.bar.font)).size(size).color(color);
    match w {
        Widget::Clock => {
            let time = label(
                app.now.format(&c.clock.format).to_string(),
                colors.foreground.into(),
            );
            let content: Element<'a, Message> = if c.clock.icon.is_empty() {
                time.into()
            } else {
                row![icon(c, &c.clock.icon, size * 1.3), time]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into()
            };
            let fg: Color = colors.foreground.into();
            // The clock opens the calendar.
            button(content)
                .padding([2, 6])
                .on_press(Message::Toggle(Panel::Calendar))
                .style(move |_, s| button::Style {
                    background: matches!(s, button::Status::Hovered)
                        .then(|| Color { a: 0.12, ..fg }.into()),
                    text_color: fg,
                    border: border::rounded(8),
                    ..Default::default()
                })
                .into()
        }
        Widget::WindowTitle => {
            let active = app.hypr.monitor(monitor).map(|m| m.active_workspace.id);
            let title = app
                .hypr
                .workspaces
                .iter()
                .find(|ws| Some(ws.id) == active && ws.windows > 0)
                .map(|ws| truncate(&ws.last_window_title, c.window_title.max_chars))
                .unwrap_or_default();
            label(title, colors.foreground.into())
                .wrapping(text::Wrapping::None)
                .shaping(text::Shaping::Advanced)
                .into()
        }
        Widget::Notifications => notify::widget(app),
        Widget::Tray => tray::widget(app),
        Widget::Weather => weather::widget(app),
        Widget::Bluetooth => bt::widget(app),
        Widget::Volume => status::volume(app),
        Widget::Network => status::network(app),
        Widget::Battery => status::battery(app),
        Widget::Stats => status::stats(app),
        Widget::QuickSettings => quick::widget(app, "tune", Message::Toggle(Panel::Quick)),
        Widget::Power => quick::widget(app, "power_settings_new", Message::Toggle(Panel::Power)),
        Widget::Media => media::widget(app, (c.bar.height as f32 - 10.0).max(12.0)),
        Widget::Settings => {
            let fg: Color = colors.foreground.into();
            let hover = Color { a: 0.12, ..fg };
            button(icon(c, "settings", size * 1.3))
                .padding(4)
                .on_press(Message::ToggleSettings)
                .style(move |_, status| button::Style {
                    background: matches!(status, button::Status::Hovered).then(|| hover.into()),
                    text_color: fg,
                    border: border::rounded(8),
                    ..Default::default()
                })
                .into()
        }
        Widget::Workspaces => {
            let pill_h = (c.bar.height as f32 - 10.0).max(12.0);
            row(workspace_ids(c, &app.hypr).into_iter().map(|id| {
                let ws = app.hypr.workspaces.iter().find(|w| w.id == id);
                let urgent = app.hypr.urgent.contains(&id);
                let (base_fg, base_bg): (Color, Color) = if urgent {
                    (colors.on_accent.into(), colors.urgent.into())
                } else if ws.is_some_and(|w| w.windows > 0) {
                    (colors.foreground.into(), Color::TRANSPARENT)
                } else {
                    (colors.muted.into(), Color::TRANSPARENT)
                };
                let t = app
                    .pills
                    .get(&(monitor.to_owned(), id))
                    .map_or(0.0, |a| a.interpolate(0.0, 1.0, app.instant));
                let fg = mix(base_fg, colors.on_accent.into(), t);
                let bg = mix(base_bg, colors.accent.into(), t);
                let hover = Color {
                    a: 0.12,
                    ..Color::from(colors.foreground)
                };
                let name = ws.map_or_else(|| id.to_string(), |w| w.name.clone());
                let width = if name.chars().count() <= 2 {
                    Length::Fixed(pill_h)
                } else {
                    Length::Shrink
                };
                button(label(name, fg).center().height(Length::Fill))
                    .width(width)
                    .height(pill_h)
                    .padding([0.0, pill_h / 3.0])
                    .on_press(Message::Focus(id))
                    .style(move |_, status| button::Style {
                        background: Some(
                            if t < 0.5 && matches!(status, button::Status::Hovered) {
                                hover
                            } else {
                                bg
                            }
                            .into(),
                        ),
                        text_color: fg,
                        border: border::rounded(pill_h / 2.0),
                        ..Default::default()
                    })
                    .into()
            }))
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        }
    }
}

/// A recording timer (click to stop) and the game-mode badge (click to leave).
fn status(app: &App) -> Option<Element<'_, Message>> {
    let c = &app.config;
    let red: Color = c.colors.urgent.into();
    let accent: Color = c.colors.accent.into();
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    if let Some(rec) = &app.recording {
        let secs = rec.started.elapsed().as_secs();
        let dot = container(Space::new().width(8).height(8)).style(move |_| container::Style {
            background: Some(red.into()),
            border: border::rounded(4),
            ..Default::default()
        });
        let label = text(format!("{}:{:02}", secs / 60, secs % 60))
            .size(c.bar.font_size)
            .font(font(&c.bar.font));
        items.push(
            button(row![dot, label].spacing(6).align_y(Alignment::Center))
                .padding([2, 8])
                .on_press(Message::StopRecording)
                .style(move |_, _| button::Style {
                    background: Some(Color { a: 0.18, ..red }.into()),
                    text_color: red,
                    border: border::rounded(10),
                    ..Default::default()
                })
                .into(),
        );
    }
    if app.game_mode {
        items.push(
            button(icon(c, "sports_esports", c.bar.font_size * 1.3))
                .padding([2, 6])
                .on_press(Message::GameModeOff)
                .style(move |_, _| button::Style {
                    background: Some(Color { a: 0.18, ..accent }.into()),
                    text_color: accent,
                    border: border::rounded(10),
                    ..Default::default()
                })
                .into(),
        );
    }
    (!items.is_empty()).then(|| row(items).spacing(6).align_y(Alignment::Center).into())
}

/// Cuts `s` to `max` characters, ending in "…" when shortened.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Linear blend from `a` (t = 0) to `b` (t = 1).
fn mix(a: Color, b: Color, t: f32) -> Color {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Color::from_rgba(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b), l(a.a, b.a))
}

/// One concave shoulder: fills the corner between the screen edge and the
/// notch side with a quarter-circle cut out.
struct Shoulder {
    left: bool,
    color: Color,
}

impl<M> canvas::Program<M> for Shoulder {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        // Left shoulder: notch side is at x = w; mirror for the right one.
        let x = |v: f32| if self.left { v } else { w - v };
        let path = canvas::Path::new(|p| {
            p.move_to(Point::new(x(0.0), 0.0));
            p.line_to(Point::new(x(w), 0.0));
            p.line_to(Point::new(x(w), h));
            p.quadratic_curve_to(Point::new(x(w), 0.0), Point::new(x(0.0), 0.0));
            p.close();
        });
        frame.fill(&path, self.color);
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn mixes_colors() {
        let m = mix(Color::BLACK, Color::WHITE, 0.5);
        assert!((m.r - 0.5).abs() < 1e-6 && (m.a - 1.0).abs() < 1e-6);
    }
}
