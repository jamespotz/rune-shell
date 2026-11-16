//! Calendar panel: a month grid and the selected day's events, with a Join
//! button for video meetings.

use crate::{App, Message, notch::icon};
use chrono::{Datelike, Days, Local, Months, NaiveDate, NaiveTime, TimeZone};
use iced::{
    Alignment, Color, Element, Length, Padding, border,
    widget::{Space, button, column, container, row, scrollable, text},
};
use rune_services::calendar::Event;

pub const PANEL_W: f32 = 640.0;
pub const PANEL_H: f32 = 400.0;

/// Events overlapping `day`.
pub fn on_day(events: &[Event], day: NaiveDate) -> impl Iterator<Item = &Event> {
    let start = Local
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest();
    let end = Local
        .from_local_datetime(&(day + Days::new(1)).and_time(NaiveTime::MIN))
        .earliest();
    events.iter().filter(move |e| match (start, end) {
        (Some(s), Some(n)) => e.start < n && (e.end > s || (e.start >= s && e.end == e.start)),
        _ => false,
    })
}

/// Colors for up to a handful of calendars, taken from the theme.
fn calendar_color(app: &App, index: usize) -> Color {
    let c = &app.config.colors;
    [c.accent, c.urgent, c.foreground, c.muted][index % 4].into()
}

pub fn panel(app: &App) -> Element<'_, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let muted: Color = c.colors.muted.into();
    let accent: Color = c.colors.accent.into();
    let on_accent: Color = c.colors.on_accent.into();
    let today = Local::now().date_naive();
    let month = app.cal_month;

    let nav = |glyph: &str, by: i32| {
        button(icon(c, glyph, 18.0))
            .padding(4)
            .on_press(Message::CalMonth(by))
            .style(move |_, s| button::Style {
                background: matches!(s, button::Status::Hovered)
                    .then(|| Color { a: 0.12, ..fg }.into()),
                text_color: fg,
                border: border::rounded(8),
                ..Default::default()
            })
    };
    let header = row![
        text(month.format("%B %Y").to_string())
            .size(15)
            .width(Length::Fill),
        nav("chevron_left", -1),
        nav("chevron_right", 1),
    ]
    .align_y(Alignment::Center);
    let weekdays = row(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|d| {
        text(d)
            .size(11)
            .color(muted)
            .width(36)
            .align_x(Alignment::Center)
            .into()
    }));
    // Six weeks starting on the Monday on or before the 1st.
    let first = month - Days::new(u64::from(month.weekday().num_days_from_monday()));
    let weeks = column((0..6).map(|w| {
        row((0..7).map(|d| {
            let day = first + Days::new(w * 7 + d);
            let selected = day == app.cal_day;
            let is_today = day == today;
            let busy = on_day(&app.calendar.events, day).next().is_some();
            let in_month = day.month() == month.month();
            let number = text(day.day().to_string()).size(12).color(if selected {
                on_accent
            } else if in_month {
                fg
            } else {
                Color { a: 0.35, ..fg }
            });
            let dot = container(Space::new().width(4).height(4)).style(move |_| container::Style {
                background: busy.then(|| {
                    if selected {
                        on_accent.into()
                    } else {
                        accent.into()
                    }
                }),
                border: border::rounded(2),
                ..Default::default()
            });
            button(
                column![number, dot]
                    .spacing(2)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            )
            .width(36)
            .height(36)
            .padding([5, 0])
            .on_press(Message::CalDay(day))
            .style(move |_, s| button::Style {
                background: Some(
                    if selected {
                        accent
                    } else if matches!(s, button::Status::Hovered) {
                        Color { a: 0.1, ..fg }
                    } else {
                        Color::TRANSPARENT
                    }
                    .into(),
                ),
                text_color: fg,
                border: border::rounded(18)
                    .color(if is_today && !selected {
                        accent
                    } else {
                        Color::TRANSPARENT
                    })
                    .width(1),
                ..Default::default()
            })
            .into()
        }))
        .into()
    }));
    let grid = column![header, weekdays, weeks].spacing(6).width(260);

    let day = app.cal_day;
    let title = if day == today {
        "Today".to_owned()
    } else if day == today + Days::new(1) {
        "Tomorrow".to_owned()
    } else {
        day.format("%A, %B %-d").to_string()
    };
    let events: Vec<Element<'_, Message>> = on_day(&app.calendar.events, day)
        .map(|e| event_card(app, e))
        .collect();
    let body: Element<'_, Message> =
        if !app.calendar.errors.is_empty() && app.calendar.events.is_empty() {
            text(app.calendar.errors.join("\n"))
                .size(12)
                .color(muted)
                .into()
        } else if c.calendar.urls.is_empty() {
            text("Add a calendar link in Settings → Calendar")
                .size(13)
                .color(muted)
                .into()
        } else if events.is_empty() {
            text("Nothing scheduled").size(13).color(muted).into()
        } else {
            scrollable(column(events).spacing(8).padding(Padding::ZERO.right(12)))
                .height(Length::Fill)
                .into()
        };
    let list = column![text(title).size(15), body]
        .spacing(10)
        .width(Length::Fill);
    row![grid, list].spacing(24).into()
}

fn event_card<'a>(app: &'a App, e: &'a Event) -> Element<'a, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let muted: Color = c.colors.muted.into();
    let accent: Color = c.colors.accent.into();
    let on_accent: Color = c.colors.on_accent.into();
    let color = calendar_color(app, e.calendar);
    let when = if e.all_day {
        "All day".to_owned()
    } else {
        format!("{} – {}", e.start.format("%H:%M"), e.end.format("%H:%M"))
    };
    let mut info = column![
        text(e.title.clone())
            .size(14)
            .shaping(text::Shaping::Advanced),
        text(when).size(12).color(muted),
    ]
    .spacing(2)
    .width(Length::Fill);
    if !e.location.is_empty() && e.meeting.as_deref() != Some(e.location.as_str()) {
        info = info.push(
            text(e.location.clone())
                .size(12)
                .color(muted)
                .wrapping(text::Wrapping::None),
        );
    }
    let mut line = row![
        container(Space::new().width(4).height(36)).style(move |_| container::Style {
            background: Some(color.into()),
            border: border::rounded(2),
            ..Default::default()
        }),
        info,
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if let Some(url) = &e.meeting {
        line = line.push(
            button(
                row![icon(c, "videocam", 16.0), text("Join").size(12)]
                    .spacing(4)
                    .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .on_press(Message::OpenLink(url.clone()))
            .style(move |_, _| button::Style {
                background: Some(accent.into()),
                text_color: on_accent,
                border: border::rounded(8),
                ..Default::default()
            }),
        );
    }
    container(line)
        .padding(10)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Color { a: 0.05, ..fg }.into()),
            border: border::rounded(10),
            ..Default::default()
        })
        .into()
}

/// `month` moved by `by` months (kept on the 1st).
pub fn shift_month(month: NaiveDate, by: i32) -> NaiveDate {
    let m = Months::new(by.unsigned_abs());
    if by >= 0 {
        month.checked_add_months(m)
    } else {
        month.checked_sub_months(m)
    }
    .unwrap_or(month)
}

/// Shell-quotes `s` for `sh -c` (single quotes, with embedded ones escaped).
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(start: &str, end: &str) -> Event {
        let t = |s: &str| {
            Local
                .from_local_datetime(
                    &chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap(),
                )
                .unwrap()
        };
        Event {
            title: String::new(),
            start: t(start),
            end: t(end),
            all_day: false,
            location: String::new(),
            meeting: None,
            calendar: 0,
        }
    }

    #[test]
    fn finds_events_on_a_day() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        let events = [
            event("2026-11-16 09:00", "2026-11-16 10:00"),
            event("2026-11-15 22:00", "2026-11-16 01:00"), // overnight, ends on the 16th
            event("2026-11-16 23:00", "2026-11-17 00:00"),
            event("2026-11-17 00:00", "2026-11-17 01:00"), // next day
            event("2026-11-16 12:00", "2026-11-16 12:00"), // zero length
        ];
        assert_eq!(on_day(&events, day).count(), 4);
    }

    #[test]
    fn months_and_quoting() {
        let jan = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(
            shift_month(jan, -1),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
        assert_eq!(
            shift_month(jan, 13),
            NaiveDate::from_ymd_opt(2027, 2, 1).unwrap()
        );
        assert_eq!(
            shell_quote("https://x.com/a'b?c=1&d"),
            "'https://x.com/a'\\''b?c=1&d'"
        );
    }
}
