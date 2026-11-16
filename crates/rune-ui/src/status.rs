//! Volume, network and battery widgets, and the volume/brightness OSD.

use crate::{
    App, Message,
    notch::{font, icon},
};
use iced::{
    Alignment, Color, Element, Length, border, mouse,
    widget::{Space, button, container, mouse_area, progress_bar, row, text},
};
use rune_services::{
    audio::Volume,
    system::{Battery, NetKind, Network},
};

/// What the OSD shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Osd {
    Volume(Volume),
    Brightness(u8),
}

pub const OSD_W: u32 = 320;
pub const OSD_H: u32 = 64;

pub fn volume_icon(v: Volume) -> &'static str {
    match (v.muted, v.percent) {
        (true, _) | (false, 0) => "volume_off",
        (false, 1..=33) => "volume_mute",
        (false, 34..=66) => "volume_down",
        _ => "volume_up",
    }
}

fn network_icon(n: &Network) -> &'static str {
    match (n.connected, n.kind, n.strength) {
        (false, _, _) => "signal_wifi_off",
        (true, NetKind::Wired, _) => "lan",
        (true, NetKind::Wifi, Some(s)) if !n.online => {
            if s > 0 {
                "signal_wifi_statusbar_not_connected"
            } else {
                "signal_wifi_off"
            }
        }
        (true, NetKind::Wifi, Some(0..=24)) => "network_wifi_1_bar",
        (true, NetKind::Wifi, Some(25..=49)) => "network_wifi_2_bar",
        (true, NetKind::Wifi, Some(50..=74)) => "network_wifi_3_bar",
        (true, NetKind::Wifi, _) => "signal_wifi_4_bar",
        (true, NetKind::Other, _) => "lan",
    }
}

fn battery_icon(b: &Battery) -> &'static str {
    if b.charging || b.full {
        return "battery_charging_full";
    }
    const LEVELS: [&str; 8] = [
        "battery_0_bar",
        "battery_1_bar",
        "battery_2_bar",
        "battery_3_bar",
        "battery_4_bar",
        "battery_5_bar",
        "battery_6_bar",
        "battery_full",
    ];
    LEVELS[((b.percent / 100.0 * 7.0).round() as usize).min(7)]
}

fn plain<'a>(
    app: &'a App,
    glyph: &str,
    label: Option<String>,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let mut content = row![icon(c, glyph, c.bar.font_size * 1.3)]
        .spacing(4)
        .align_y(Alignment::Center);
    if let Some(l) = label {
        content = content.push(text(l).size(c.bar.font_size).font(font(&c.bar.font)));
    }
    let b = button(content)
        .padding([2, 4])
        .style(move |_, s| button::Style {
            background: matches!(s, button::Status::Hovered)
                .then(|| Color { a: 0.12, ..fg }.into()),
            text_color: fg,
            border: border::rounded(8),
            ..Default::default()
        });
    match msg {
        Some(m) => b.on_press(m).into(),
        None => b.into(),
    }
}

/// Click to mute, scroll to change.
pub fn volume(app: &App) -> Element<'_, Message> {
    let Some(v) = app.volume else {
        return Space::new().into();
    };
    let label = if v.muted {
        None
    } else {
        Some(format!("{}%", v.percent))
    };
    mouse_area(plain(app, volume_icon(v), label, Some(Message::VolumeMute)))
        .on_scroll(|delta| {
            let y = match delta {
                mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            Message::VolumeStep(if y > 0.0 { 5 } else { -5 })
        })
        .into()
}

pub fn network(app: &App) -> Element<'_, Message> {
    let Some(n) = &app.network else {
        return Space::new().into();
    };
    plain(app, network_icon(n), None, None)
}

pub fn battery(app: &App) -> Element<'_, Message> {
    let Some(b) = &app.battery else {
        return Space::new().into();
    };
    plain(
        app,
        battery_icon(b),
        Some(format!("{:.0}%", b.percent)),
        None,
    )
}

pub fn osd(app: &App) -> Element<'_, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let accent: Color = c.colors.accent.into();
    let bg: Color = c.colors.background.into();
    let (glyph, value, dim) = match app.osd {
        Some(Osd::Volume(v)) => (volume_icon(v), v.percent.min(100) as f32, v.muted),
        Some(Osd::Brightness(b)) => ("brightness_medium", f32::from(b), false),
        None => return Space::new().into(),
    };
    let bar_color = if dim { Color { a: 0.4, ..fg } } else { accent };
    container(
        row![
            icon(c, glyph, 26.0),
            progress_bar(0.0..=100.0, value)
                .girth(8)
                .length(Length::Fill)
                .style(move |_| progress_bar::Style {
                    background: Color { a: 0.15, ..fg }.into(),
                    bar: bar_color.into(),
                    border: border::rounded(4),
                }),
            text(format!("{value:.0}"))
                .size(15)
                .width(34)
                .align_x(Alignment::End)
                .font(font(&c.bar.font)),
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .padding([0, 18])
    .center_y(Length::Fill)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Color { a: 1.0, ..bg }.into()),
        border: border::rounded(OSD_H as f32 / 2.0)
            .color(Color { a: 0.12, ..fg })
            .width(1),
        text_color: Some(fg),
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_icons() {
        assert_eq!(
            volume_icon(Volume {
                percent: 80,
                muted: true
            }),
            "volume_off"
        );
        assert_eq!(
            volume_icon(Volume {
                percent: 50,
                muted: false
            }),
            "volume_down"
        );
        let wifi = |s| Network {
            connected: true,
            online: true,
            kind: NetKind::Wifi,
            name: String::new(),
            strength: Some(s),
            wifi_enabled: true,
        };
        assert_eq!(network_icon(&wifi(66)), "network_wifi_3_bar");
        assert_eq!(network_icon(&wifi(90)), "signal_wifi_4_bar");
        let bat = |p, charging| Battery {
            percent: p,
            charging,
            full: false,
            time_left: None,
        };
        assert_eq!(battery_icon(&bat(100.0, false)), "battery_full");
        assert_eq!(battery_icon(&bat(3.0, false)), "battery_0_bar");
        assert_eq!(battery_icon(&bat(50.0, true)), "battery_charging_full");
    }
}
