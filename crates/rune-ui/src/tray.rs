//! Tray icons in the bar, and an app's tray menu in the expanded notch.

use crate::{App, Message, notch::icon};
use iced::{
    Alignment, Color, Element, Length, border, mouse,
    widget::{
        Space, button, column, container, image, mouse_area, row, rule, scrollable, svg, text,
    },
};
use rune_services::{
    notifications::Image,
    tray::{MenuEntry, MenuKind},
};

/// Which mouse button clicked a tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Left,
    Middle,
    Right,
}

pub const MENU_W: f32 = 300.0;
pub const MENU_H: f32 = 360.0;

pub fn widget(app: &App) -> Element<'_, Message> {
    let size = (app.config.bar.font_size * 1.35).round();
    row(app.tray.iter().map(|item| {
        let picture: Element<'_, Message> = match (&item.image, app.tray_images.get(&item.key)) {
            (Some(Image::Rgba { .. }), Some(h)) => image(h.clone()).width(size).height(size).into(),
            (Some(Image::Path(p)), _) if p.extension().is_some_and(|e| e == "svg") => {
                svg(svg::Handle::from_path(p))
                    .width(size)
                    .height(size)
                    .into()
            }
            (Some(Image::Path(p)), _) => image(p.clone()).width(size).height(size).into(),
            _ => icon(&app.config, "apps", size),
        };
        let key = item.key.clone();
        mouse_area(container(picture).padding(2))
            .on_press(Message::TrayClick(key.clone(), Click::Left))
            .on_middle_press(Message::TrayClick(key.clone(), Click::Middle))
            .on_right_press(Message::TrayClick(key, Click::Right))
            .interaction(mouse::Interaction::Pointer)
            .into()
    }))
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// The open tray menu.
pub fn menu(app: &App) -> Element<'_, Message> {
    let muted: Color = app.config.colors.muted.into();
    let Some((key, entries)) = &app.tray_menu else {
        return container(text("Loading…").color(muted))
            .center(Length::Fill)
            .into();
    };
    let title = app.tray.iter().find(|i| &i.key == key).map(|i| {
        if i.title.is_empty() {
            i.id.clone()
        } else {
            i.title.clone()
        }
    });
    let mut rows: Vec<Element<'_, Message>> =
        vec![text(title.unwrap_or_default()).size(13).color(muted).into()];
    push_entries(app, entries, 0, &mut rows);
    if entries.is_empty() {
        rows.push(
            text("This app's menu is empty")
                .size(13)
                .color(muted)
                .into(),
        );
    }
    scrollable(column(rows).spacing(2))
        .height(Length::Fill)
        .into()
}

fn push_entries<'a>(
    app: &'a App,
    entries: &'a [MenuEntry],
    depth: u16,
    rows: &mut Vec<Element<'a, Message>>,
) {
    let fg: Color = app.config.colors.foreground.into();
    let muted: Color = app.config.colors.muted.into();
    for e in entries {
        if e.kind == MenuKind::Separator {
            rows.push(container(rule::horizontal(1)).padding([4, 8]).into());
            continue;
        }
        let mark = match e.kind {
            MenuKind::Check(on) => Some(if on {
                "check_box"
            } else {
                "check_box_outline_blank"
            }),
            MenuKind::Radio(on) => Some(if on {
                "radio_button_checked"
            } else {
                "radio_button_unchecked"
            }),
            _ => None,
        };
        let mut line = row![Space::new().width(f32::from(depth) * 14.0)]
            .spacing(8)
            .align_y(Alignment::Center);
        if let Some(glyph) = mark {
            line = line.push(icon(&app.config, glyph, 16.0));
        }
        line = line.push(
            text(e.label.clone())
                .size(13)
                .shaping(text::Shaping::Advanced),
        );
        if !e.children.is_empty() {
            // Submenus are shown inline, indented under their label.
            rows.push(
                container(line)
                    .padding([6, 10])
                    .style(move |_| container::Style {
                        text_color: Some(muted),
                        ..Default::default()
                    })
                    .into(),
            );
            push_entries(app, &e.children, depth + 1, rows);
            continue;
        }
        let enabled = e.enabled;
        let mut b = button(line)
            .width(Length::Fill)
            .padding([6, 10])
            .style(move |_, s| button::Style {
                background: (enabled && matches!(s, button::Status::Hovered))
                    .then(|| Color { a: 0.1, ..fg }.into()),
                text_color: if enabled { fg } else { muted },
                border: border::rounded(8),
                ..Default::default()
            });
        if enabled {
            b = b.on_press(Message::TrayMenuClick(e.id));
        }
        rows.push(b.into());
    }
}
