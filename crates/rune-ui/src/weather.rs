//! Weather: a compact bar widget and the forecast panel in the expanded notch.

use crate::{
    App, Message,
    notch::{font, icon},
};
use iced::{
    Alignment, Color, Element, Length, Padding, border,
    widget::{Space, button, column, container, row, scrollable, text},
};
use rune_services::weather::{self, Weather};

pub const PANEL_W: f32 = 580.0;
pub const PANEL_H: f32 = 440.0;

fn temp(t: f64) -> String {
    format!("{:.0}°", t.round())
}

/// Icon and temperature; hidden until a forecast arrives.
pub fn widget(app: &App) -> Element<'_, Message> {
    let Some(w) = &app.weather else {
        return Space::new().into();
    };
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let content = row![
        icon(
            c,
            weather::icon(w.current.code, w.current.is_day),
            c.bar.font_size * 1.35
        ),
        text(temp(w.current.temperature))
            .size(c.bar.font_size)
            .font(font(&c.bar.font)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    button(content)
        .padding([2, 6])
        .on_press(Message::ToggleWeather)
        .style(move |_, s| button::Style {
            background: matches!(s, button::Status::Hovered)
                .then(|| Color { a: 0.12, ..fg }.into()),
            text_color: fg,
            border: border::rounded(8),
            ..Default::default()
        })
        .into()
}

/// Current conditions, the next 24 hours, and the week.
pub fn panel(app: &App) -> Element<'_, Message> {
    let c = &app.config;
    let muted: Color = c.colors.muted.into();
    let fg: Color = c.colors.foreground.into();
    let Some(w) = &app.weather else {
        let hint = if c.weather.location.is_empty() {
            "Pick a place in Settings → Weather"
        } else {
            "Loading the forecast…"
        };
        return container(text(hint).color(muted))
            .center(Length::Fill)
            .into();
    };
    let wind = if w.imperial { "mph" } else { "km/h" };
    let now = &w.current;
    let detail = |label: &str, value: String| {
        row![
            text(label.to_owned()).size(12).color(muted).width(80),
            text(value).size(13)
        ]
        .spacing(6)
    };
    let current = row![
        icon(c, weather::icon(now.code, now.is_day), 60.0),
        column![
            text(temp(now.temperature)).size(34),
            text(weather::describe(now.code)).size(14),
            text(c.weather.location.clone()).size(12).color(muted),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        column![
            detail("Feels like", temp(now.feels_like)),
            detail("Humidity", format!("{:.0}%", now.humidity)),
            detail("Wind", format!("{:.0} {wind}", now.wind)),
            today_sun(w),
        ]
        .spacing(4),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    let hours = row(w.hourly.iter().map(|h| {
        let rain: Element<'_, Message> = if h.precipitation >= 10 {
            text(format!("{}%", h.precipitation))
                .size(10)
                .color(muted)
                .into()
        } else {
            Space::new().height(12).into()
        };
        column![
            text(h.time.format("%H:%M").to_string())
                .size(11)
                .color(muted),
            icon(c, weather::icon(h.code, h.is_day), 22.0),
            text(temp(h.temperature)).size(13),
            rain,
        ]
        .spacing(4)
        .align_x(Alignment::Center)
        .width(52)
        .into()
    }))
    .spacing(4);
    let hourly = scrollable(container(hours).padding(Padding::ZERO.bottom(12))).direction(
        scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4)),
    );

    // Each day's range drawn on one shared scale, so days compare at a glance.
    let lo = w.daily.iter().map(|d| d.min).fold(f64::INFINITY, f64::min);
    let hi = w
        .daily
        .iter()
        .map(|d| d.max)
        .fold(f64::NEG_INFINITY, f64::max);
    let span = (hi - lo).max(1.0);
    let accent: Color = c.colors.accent.into();
    let days = column(w.daily.iter().enumerate().map(|(i, d)| {
        let name = if i == 0 {
            "Today".to_owned()
        } else {
            d.date.format("%a").to_string()
        };
        let start = ((d.min - lo) / span * 120.0) as f32;
        let width = (((d.max - d.min) / span * 120.0) as f32).max(4.0);
        let bar = row![
            Space::new().width(start),
            container(Space::new().width(width).height(5)).style(move |_| container::Style {
                background: Some(accent.into()),
                border: border::rounded(3),
                ..Default::default()
            }),
        ];
        let range = container(bar)
            .width(126)
            .height(5)
            .style(move |_| container::Style {
                background: Some(Color { a: 0.12, ..fg }.into()),
                border: border::rounded(3),
                ..Default::default()
            });
        let rain = if d.precipitation >= 10 {
            format!("{}%", d.precipitation)
        } else {
            String::new()
        };
        row![
            text(name).size(13).width(60),
            icon(c, weather::icon(d.code, true), 20.0),
            text(rain).size(11).color(muted).width(36),
            text(temp(d.min))
                .size(13)
                .color(muted)
                .width(36)
                .align_x(Alignment::End),
            range,
            text(temp(d.max)).size(13).width(36),
            text(weather::describe(d.code)).size(12).color(muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }))
    .spacing(6);

    scrollable(
        column![current, hourly, days]
            .spacing(14)
            .padding(Padding::ZERO.right(12)),
    )
    .height(Length::Fill)
    .into()
}

fn today_sun(w: &Weather) -> Element<'_, Message> {
    let Some(today) = w.daily.first() else {
        return Space::new().into();
    };
    let fmt = |t: Option<chrono::NaiveDateTime>| {
        t.map_or("–".to_owned(), |t| t.format("%H:%M").to_string())
    };
    text(format!(
        "Sun {} – {}",
        fmt(today.sunrise),
        fmt(today.sunset)
    ))
    .size(12)
    .into()
}
