//! Notification popups (stacked like Sonner: the newest in front, older ones
//! peeking behind; hovering fans them out) and the notification center.

use crate::{
    App, Message,
    notch::{font, icon},
};
use chrono::Local;
use iced::{
    Alignment, Color, Element, Length, Padding, border,
    widget::{
        Space, button, column, container, image, mouse_area, row, scrollable, stack, text, toggler,
    },
};
use rune_services::notifications::{Image, Notification, Urgency};

/// Popup width, and the gap between toasts when fanned out.
pub const WIDTH: f32 = 360.0;
const GAP: f32 = 10.0;
/// How much each older toast peeks out below the one in front when stacked.
const PEEK: f32 = 14.0;
/// Older toasts visible behind the front one when stacked.
const BEHIND: usize = 2;
/// Most toasts shown when fanned out.
pub const MAX_SHOWN: usize = 5;

/// Fixed toast heights keep the stack layout (and the surface size) predictable.
fn toast_height(n: &Notification) -> f32 {
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

/// The popup stack.
pub fn popups(app: &App) -> Element<'_, Message> {
    let t = app.popup_expand.interpolate(0.0, 1.0, app.instant);
    let list = &app.notif.popups;
    let front_h = list.first().map_or(0.0, toast_height);
    let mut layers: Vec<Element<'_, Message>> = Vec::new();
    let mut fanned_y = 0.0;
    let mut positions = Vec::new();
    for (i, n) in list.iter().take(MAX_SHOWN).enumerate() {
        positions.push((i, n, fanned_y));
        fanned_y += toast_height(n) + GAP;
    }
    // Back to front, so the newest is drawn last (on top).
    for &(i, n, fanned) in positions.iter().rev() {
        if i > BEHIND && t < 0.05 {
            continue;
        }
        let depth = i.min(BEHIND + 1) as f32;
        let y = PEEK * depth + (fanned - PEEK * depth) * t;
        let scale = 1.0 - 0.05 * depth * (1.0 - t);
        let height = front_h + (toast_height(n) - front_h) * t;
        let inset = WIDTH * (1.0 - scale) / 2.0;
        // Behind the front, content only appears once the stack opens up.
        let show_content = i == 0 || t > 0.5;
        layers.push(
            container(toast(app, n, height, show_content, false))
                .padding(Padding {
                    top: y,
                    left: inset,
                    right: inset,
                    bottom: 0.0,
                })
                .width(WIDTH)
                .into(),
        );
    }
    mouse_area(stack(layers).width(WIDTH).height(Length::Fill))
        .on_enter(Message::PopupHover(true))
        .on_exit(Message::PopupHover(false))
        .into()
}

/// One notification card. In the center (`in_center`), closing removes it from history.
pub fn toast<'a>(
    app: &'a App,
    n: &'a Notification,
    height: f32,
    show_content: bool,
    in_center: bool,
) -> Element<'a, Message> {
    let c = &app.config.colors;
    let fg: Color = c.foreground.into();
    let muted: Color = c.muted.into();
    let base = Color {
        a: 1.0,
        ..c.background.into()
    };
    let surface = Color {
        r: base.r + (fg.r - base.r) * 0.07,
        g: base.g + (fg.g - base.g) * 0.07,
        b: base.b + (fg.b - base.b) * 0.07,
        a: 1.0,
    };
    let edge: Color = if n.urgency == Urgency::Critical {
        c.urgent.into()
    } else {
        Color { a: 0.12, ..fg }
    };
    let id = n.id;

    let content: Element<'a, Message> = if show_content {
        let picture: Element<'a, Message> = match (&n.image, app.notif_images.get(&id)) {
            (Some(Image::Rgba { .. }), Some(handle)) => image(handle.clone())
                .width(40)
                .height(40)
                .border_radius(10)
                .into(),
            (Some(Image::Path(p)), _) if p.extension().is_some_and(|e| e == "svg") => {
                iced::widget::svg(iced::widget::svg::Handle::from_path(p))
                    .width(40)
                    .height(40)
                    .into()
            }
            (Some(Image::Path(p)), _) => image(p.clone())
                .width(40)
                .height(40)
                .border_radius(10)
                .into(),
            _ => container(icon(&app.config, "notifications", 24.0))
                .center(40)
                .into(),
        };
        let close_msg = if in_center {
            Message::NotifRemove(id)
        } else {
            Message::NotifDismiss(id)
        };
        let close = button(icon(&app.config, "close", 16.0))
            .padding(2)
            .on_press(close_msg)
            .style(move |_, s| button::Style {
                background: matches!(s, button::Status::Hovered)
                    .then(|| Color { a: 0.12, ..fg }.into()),
                text_color: muted,
                border: border::rounded(6),
                ..Default::default()
            });
        let header = row![
            text(n.app_name.clone())
                .size(11)
                .color(muted)
                .wrapping(text::Wrapping::None),
            Space::new().width(Length::Fill),
            text(ago(n)).size(11).color(muted),
            close,
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let body = column![
            text(cut(&n.summary, 48))
                .size(14)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..font(&app.config.bar.font)
                })
                .shaping(text::Shaping::Advanced),
            text(cut(&n.body, 110))
                .size(12)
                .color(muted)
                .shaping(text::Shaping::Advanced)
                .height(32),
        ]
        .spacing(2);
        let mut col = column![header, row![picture, body].spacing(10)].spacing(4);
        let extra: Vec<&(String, String)> = n
            .actions
            .iter()
            .filter(|(k, _)| k != "default")
            .take(3)
            .collect();
        if !extra.is_empty() {
            col = col.push(
                row(extra.into_iter().map(|(key, label)| {
                    button(text(label.clone()).size(12).center())
                        .width(Length::Fill)
                        .padding([6, 8])
                        .on_press(Message::NotifAction(id, key.clone()))
                        .style(move |_, s| button::Style {
                            background: Some(
                                Color {
                                    a: if matches!(s, button::Status::Hovered) {
                                        0.16
                                    } else {
                                        0.08
                                    },
                                    ..fg
                                }
                                .into(),
                            ),
                            text_color: fg,
                            border: border::rounded(8),
                            ..Default::default()
                        })
                        .into()
                }))
                .spacing(6),
            );
        }
        col.into()
    } else {
        Space::new().into()
    };
    let has_default = n.actions.iter().any(|(k, _)| k == "default");
    let click = if has_default {
        Message::NotifAction(id, "default".into())
    } else if in_center {
        Message::Noop
    } else {
        Message::NotifDismiss(id)
    };
    mouse_area(
        container(content)
            .width(Length::Fill)
            .height(height)
            .padding([10, 12])
            .style(move |_| container::Style {
                background: Some(surface.into()),
                border: border::rounded(14).color(edge).width(1),
                text_color: Some(fg),
                ..Default::default()
            }),
    )
    .on_press(click)
    .into()
}

const CENTER_W: f32 = 460.0;

/// The notification center panel (in the expanded notch).
pub fn center(app: &App) -> Element<'_, Message> {
    let muted: Color = app.config.colors.muted.into();
    let fg: Color = app.config.colors.foreground.into();
    let header = row![
        text("Notifications").size(16),
        Space::new().width(Length::Fill),
        text("Do not disturb").size(12).color(muted),
        toggler(app.notif.dnd).on_toggle(Message::NotifDnd).size(18),
        button(text("Clear all").size(12))
            .padding([4, 10])
            .on_press(Message::NotifClear)
            .style(move |_, s| button::Style {
                background: Some(
                    Color {
                        a: if matches!(s, button::Status::Hovered) {
                            0.16
                        } else {
                            0.08
                        },
                        ..fg
                    }
                    .into()
                ),
                text_color: fg,
                border: border::rounded(8),
                ..Default::default()
            }),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let body: Element<'_, Message> = if !app.notif.active {
        container(text("Another notification daemon is running (probably another shell), so Rune's notifications are off.").size(13).color(muted))
            .center(Length::Fill)
            .into()
    } else if app.notif.history.is_empty() {
        container(text("No notifications").color(muted))
            .center(Length::Fill)
            .into()
    } else {
        scrollable(
            column(
                app.notif
                    .history
                    .iter()
                    .map(|n| toast(app, n, toast_height(n), true, true)),
            )
            .spacing(8)
            .padding(Padding::ZERO.right(14)), // keeps the scrollbar off the cards
        )
        .height(Length::Fill)
        .into()
    };
    column![header, body].spacing(12).width(CENTER_W).into()
}

/// Bar widget: a bell, struck through in do-not-disturb, with a count.
pub fn widget(app: &App) -> Element<'_, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let glyph = if app.notif.dnd {
        "notifications_off"
    } else {
        "notifications"
    };
    let count = app.notif.history.len();
    let mut content = row![icon(c, glyph, c.bar.font_size * 1.3)]
        .spacing(3)
        .align_y(Alignment::Center);
    if count > 0 {
        content = content.push(
            text(if count > 99 {
                "99+".into()
            } else {
                count.to_string()
            })
            .size(c.bar.font_size * 0.85)
            .font(font(&c.bar.font)),
        );
    }
    button(content)
        .padding([2, 4])
        .on_press(Message::Toggle(crate::Panel::Notifications))
        .style(move |_, s| button::Style {
            background: matches!(s, button::Status::Hovered)
                .then(|| Color { a: 0.12, ..fg }.into()),
            text_color: fg,
            border: border::rounded(8),
            ..Default::default()
        })
        .into()
}

fn ago(n: &Notification) -> String {
    let mins = (Local::now() - n.time).num_minutes();
    match mins {
        ..1 => "now".into(),
        1..60 => format!("{mins}m"),
        60..1440 => format!("{}h", mins / 60),
        _ => n.time.format("%b %d").to_string(),
    }
}

fn cut(s: &str, max: usize) -> String {
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
