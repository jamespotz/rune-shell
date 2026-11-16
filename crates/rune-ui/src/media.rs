//! Now-playing views: the compact notch widget and the expanded player panel.

use crate::{
    App, Message,
    config::Config,
    notch::{font, icon},
};
use iced::{
    Alignment, Color, Element, Length, Rectangle, Renderer, Theme, border, mouse,
    widget::{Space, button, canvas, column, container, image, mouse_area, row, slider, text},
};
use rune_services::media::{Control, Status};
use std::time::Duration;

/// Expanded player size below the bar row.
pub const PANEL_W: f32 = 540.0;

/// Compact widget: album art and a few visualizer bars. Hidden with no player.
pub fn widget<'a>(app: &'a App, pill_h: f32) -> Element<'a, Message> {
    if app.media.is_none() {
        return Space::new().into();
    }
    let accent: Color = app.config.colors.accent.into();
    let content = row![
        art(app, pill_h, 6.0),
        canvas(Bars {
            values: downsample(&app.bars, 5),
            count: 5,
            color: accent,
            gap: 2.0
        })
        .width(24)
        .height(pill_h * 0.6),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    mouse_area(content)
        .on_press(Message::Toggle(crate::Panel::Media))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

/// Expanded player panel.
pub fn panel<'a>(app: &'a App) -> Element<'a, Message> {
    let c = &app.config;
    let Some(m) = &app.media else {
        return container(text("Nothing playing").color(Color::from(c.colors.muted)))
            .center(Length::Fill)
            .into();
    };
    let muted: Color = c.colors.muted.into();
    let accent: Color = c.colors.accent.into();

    let title = if m.title.is_empty() {
        m.identity.clone()
    } else {
        m.title.clone()
    };
    let subtitle = [m.artist.as_str(), m.album.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" · ");
    let pos = m.position_now();
    let progress: Element<'a, Message> = match m.length {
        Some(len) => row![
            text(clock(pos))
                .size(12)
                .color(muted)
                .font(font(&c.bar.font)),
            // A slider, so dragging seeks; the position is sent when released.
            slider(
                0.0..=len.as_secs_f32().max(1.0),
                app.seek.unwrap_or(pos.as_secs_f32()),
                Message::SeekDrag
            )
            .on_release(Message::SeekCommit)
            .step(1.0_f32)
            .height(14),
            text(clock(len))
                .size(12)
                .color(muted)
                .font(font(&c.bar.font)),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
        None => text(m.identity.clone()).size(12).color(muted).into(),
    };
    let play_icon = if m.status == Status::Playing {
        "pause"
    } else {
        "play_arrow"
    };
    let controls = row![
        control(c, "skip_previous", Control::Previous, false),
        control(c, play_icon, Control::PlayPause, true),
        control(c, "skip_next", Control::Next, false),
        control(c, "stop", Control::Stop, false),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let info = column![
        text(title)
            .size(18)
            .wrapping(text::Wrapping::None)
            .shaping(text::Shaping::Advanced),
        text(subtitle)
            .size(13)
            .color(muted)
            .wrapping(text::Wrapping::None)
            .shaping(text::Shaping::Advanced),
        Space::new().height(Length::Fill),
        progress,
        controls,
    ]
    .spacing(8)
    .height(140);
    let top = row![art(app, 140.0, 16.0), info].spacing(18).height(140);
    let viz = canvas(Bars {
        values: app.bars.iter().map(|b| f32::from(*b) / 100.0).collect(),
        count: rune_services::visualizer::BARS,
        color: accent,
        gap: 4.0,
    })
    .width(Length::Fill)
    .height(34);
    column![top, viz]
        .spacing(12)
        .padding([4, 6])
        .width(PANEL_W)
        .into()
}

fn control<'a>(c: &Config, glyph: &str, control: Control, primary: bool) -> Element<'a, Message> {
    let fg: Color = c.colors.foreground.into();
    let (accent, on_accent): (Color, Color) = (c.colors.accent.into(), c.colors.on_accent.into());
    let size = if primary { 30.0 } else { 24.0 };
    button(container(icon(c, glyph, size)).center(size + 10.0))
        .padding(0)
        .on_press(Message::MediaControl(control))
        .style(move |_, status| {
            let hover = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(match (primary, hover) {
                    (true, _) => accent.into(),
                    (false, true) => Color { a: 0.12, ..fg }.into(),
                    (false, false) => Color::TRANSPARENT.into(),
                }),
                text_color: if primary { on_accent } else { fg },
                border: border::rounded(size),
                ..Default::default()
            }
        })
        .into()
}

fn art<'a>(app: &'a App, size: f32, radius: f32) -> Element<'a, Message> {
    match &app.art {
        Some(handle) => image(handle.clone())
            .width(size)
            .height(size)
            .border_radius(radius)
            .into(),
        None => {
            let fg: Color = app.config.colors.foreground.into();
            container(icon(&app.config, "music_note", size * 0.6))
                .center(size)
                .style(move |_| container::Style {
                    background: Some(Color { a: 0.1, ..fg }.into()),
                    border: border::rounded(radius),
                    ..Default::default()
                })
                .into()
        }
    }
}

/// `m:ss`, or `h:mm:ss` past an hour.
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Averages `bars` (0..=100) down to `n` values in 0..=1.
pub fn downsample(bars: &[u8], n: usize) -> Vec<f32> {
    if bars.is_empty() {
        return Vec::new();
    }
    let chunk = bars.len().div_ceil(n);
    bars.chunks(chunk)
        .map(|c| c.iter().map(|b| f32::from(*b)).sum::<f32>() / (c.len() as f32 * 100.0))
        .collect()
}

/// Vertical bars, bottom-aligned; flat stubs when there is no data.
struct Bars {
    values: Vec<f32>,
    /// Bars drawn (as flat stubs when `values` is empty).
    count: usize,
    color: Color,
    gap: f32,
}

impl<M> canvas::Program<M> for Bars {
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
        let n = self.values.len().max(self.count);
        let w = ((bounds.width - self.gap * (n - 1) as f32) / n as f32).max(1.0);
        for i in 0..n {
            let v = self.values.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            let h = (bounds.height * v).max(w.min(3.0));
            let x = i as f32 * (w + self.gap);
            let bar = canvas::Path::rounded_rectangle(
                iced::Point::new(x, bounds.height - h),
                iced::Size::new(w, h),
                (w / 2.0).into(),
            );
            frame.fill(&bar, self.color);
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(clock(Duration::from_secs(65)), "1:05");
        assert_eq!(clock(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn downsamples_bars() {
        assert_eq!(downsample(&[100, 0, 50, 50], 2), [0.5, 0.5]);
        assert!(downsample(&[], 5).is_empty());
        assert_eq!(downsample(&[100; 24], 5).len(), 5);
    }
}
