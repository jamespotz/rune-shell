//! Calendar date, event, and command helpers.
use chrono::{Days, Local, Months, NaiveDate, NaiveTime, TimeZone};
use rune_services::calendar::Event;

pub struct Upcoming {
    pub index: usize,
    pub overlaps: usize,
}

pub fn upcoming(
    events: &[Event],
    now: chrono::DateTime<Local>,
    auto_hide: bool,
) -> Option<Upcoming> {
    let (index, event) = events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            !event.all_day && event.end >= event.start && (event.end > now || event.start > now)
        })
        .min_by(|(_, a), (_, b)| {
            (a.start, a.end, a.calendar, &a.title).cmp(&(b.start, b.end, b.calendar, &b.title))
        })?;
    if auto_hide && event.start.signed_duration_since(now) > chrono::Duration::minutes(15) {
        return None;
    }
    let overlaps = events
        .iter()
        .enumerate()
        .filter(|(other_index, other)| {
            *other_index != index
                && !other.all_day
                && other.end >= other.start
                && other.end > now
                && other.start < event.end
                && other.end > event.start
        })
        .count();
    Some(Upcoming { index, overlaps })
}

pub fn upcoming_label(event: &Event, now: chrono::DateTime<Local>) -> String {
    let mut characters = event.title.chars();
    let mut title: String = characters
        .by_ref()
        .take(28)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if characters.next().is_some() {
        title.pop();
        title.push('…');
    }
    let seconds = event.start.signed_duration_since(now).num_seconds();
    let timing = if seconds <= 0 {
        "now".into()
    } else if seconds < 60 {
        "in <1 min".into()
    } else if seconds < 3600 {
        format!("in {} min", seconds / 60)
    } else if seconds < 86400 {
        format!("in {} hr", seconds / 3600)
    } else {
        event.start.format("%a %-d %b %H:%M").to_string()
    };
    format!("{title} · {timing}")
}

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
    fn upcoming_visibility_boundaries_and_overlaps() {
        let mut events = vec![
            event("2026-11-21 10:00", "2026-11-21 11:00"),
            event("2026-11-21 10:30", "2026-11-21 11:30"),
            event("2026-11-21 12:00", "2026-11-21 13:00"),
        ];
        let at = |text: &str| event(text, text).start;
        assert!(upcoming(&events, at("2026-11-21 09:44"), true).is_none());
        let selected = upcoming(&events, at("2026-11-21 09:45"), true).unwrap();
        assert_eq!(events[selected.index].start, events[0].start);
        assert_eq!(selected.overlaps, 1);
        assert_eq!(
            upcoming(&events, at("2026-11-21 10:30"), true)
                .unwrap()
                .index,
            0
        );
        assert_eq!(
            upcoming(&events, at("2026-11-21 11:00"), true)
                .unwrap()
                .index,
            1
        );
        assert!(upcoming(&events, at("2026-11-21 11:30"), true).is_none());
        assert_eq!(
            upcoming(&events, at("2026-11-21 11:30"), false)
                .unwrap()
                .index,
            2
        );
        events[2].all_day = true;
        assert!(upcoming(&events, at("2026-11-21 11:30"), false).is_none());
        assert!(upcoming(&[], at("2026-11-21 09:00"), false).is_none());
    }

    #[test]
    fn upcoming_excludes_malformed_and_expired_zero_length_events() {
        let events = [
            event("2026-11-21 10:00", "2026-11-21 09:00"),
            event("2026-11-21 10:00", "2026-11-21 10:00"),
        ];
        assert!(upcoming(&events, events[0].start, false).is_none());
        assert!(
            upcoming(
                &events,
                event("2026-11-21 09:45", "2026-11-21 09:45").start,
                true
            )
            .is_some()
        );
    }

    #[test]
    fn upcoming_labels_bound_hostile_titles_and_show_now() {
        let mut meeting = event("2026-11-21 10:00", "2026-11-21 11:00");
        meeting.title = format!("Hello\n{}", "界".repeat(100_000));
        let label = upcoming_label(&meeting, meeting.start);
        assert!(label.chars().count() <= 34);
        assert!(!label.contains('\n'));
        assert!(label.ends_with("… · now"));
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
