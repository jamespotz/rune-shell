//! Calendar date, event, and command helpers.
use chrono::{Days, Local, Months, NaiveDate, NaiveTime, TimeZone};
use rune_services::calendar::Event;

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
