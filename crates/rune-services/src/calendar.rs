//! Calendars from ICS links (Google's "secret address in iCal format",
//! Outlook, Nextcloud, Proton...), read-only. Fetched with `curl`; recurring
//! events are expanded for a window around today.

use chrono::{
    DateTime, Datelike, Days, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Weekday,
};
use std::collections::{HashMap, HashSet};
use tokio::sync::{Notify, watch};
use tracing::warn;

use crate::notifications::{Notification, Urgency};

const REFRESH: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// Expansion window around today.
const PAST_DAYS: u64 = 45;
const FUTURE_DAYS: u64 = 120;
/// Safety cap per recurring event.
const MAX_INSTANCES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub title: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub all_day: bool,
    pub location: String,
    /// Video meeting link (Meet, Zoom, Teams...), if any.
    pub meeting: Option<String>,
    /// Which configured calendar it came from.
    pub calendar: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// Sorted by start.
    pub events: Vec<Event>,
    /// Per-calendar problems, e.g. a dead link.
    pub errors: Vec<String>,
}

/// Keeps calendars fresh: every 15 minutes, whenever `urls` changes, and on `refresh`.
pub fn spawn(
    mut urls: watch::Receiver<Vec<String>>,
    refresh: std::sync::Arc<Notify>,
) -> watch::Receiver<State> {
    let (tx, rx) = watch::channel(State::default());
    tokio::spawn(async move {
        loop {
            let list = urls.borrow_and_update().clone();
            let mut state = State::default();
            let today = Local::now().date_naive();
            for (i, url) in list.iter().enumerate() {
                match fetch(url).await {
                    Ok(text) => state.events.extend(parse(&text, i, today)),
                    Err(e) => {
                        warn!("calendar {}: {e}", i + 1);
                        state.errors.push(format!("Calendar {}: {e}", i + 1));
                    }
                }
            }
            state.events.sort_by_key(|e| (e.start, e.end));
            tx.send_replace(state);
            tokio::select! {
                changed = urls.changed() => if changed.is_err() { return },
                () = refresh.notified() => {}
                () = tokio::time::sleep(REFRESH) => {}
            }
        }
    });
    rx
}

/// Timed events whose reminder, `lead` before the start, is due at `now` and that have not started.
pub fn due(events: &[Event], lead: Duration, now: DateTime<Local>) -> impl Iterator<Item = &Event> {
    events
        .iter()
        .filter(move |e| !e.all_day && e.start - lead <= now && now < e.start)
}

/// Posts a notification `lead` minutes before each timed event; a lead of 0 turns reminders off.
pub fn spawn_reminders(
    events: watch::Receiver<State>,
    lead: watch::Receiver<u32>,
    post: impl Fn(Notification) -> bool + Send + 'static,
) {
    tokio::spawn(async move {
        let mut reminded: HashSet<(String, DateTime<Local>)> = HashSet::new();
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            let minutes = *lead.borrow();
            if minutes == 0 {
                continue;
            }
            let now = Local::now();
            reminded.retain(|(_, start)| *start > now);
            let state = events.borrow();
            for event in due(&state.events, Duration::minutes(minutes.into()), now) {
                if reminded.insert((event.title.clone(), event.start)) && !post(reminder(event)) {
                    return;
                }
            }
        }
    });
}

fn reminder(event: &Event) -> Notification {
    let mut body = event.start.format("%H:%M").to_string();
    if !event.location.is_empty() {
        body = format!("{body} · {}", event.location);
    }
    Notification {
        id: 0,
        app_name: "Calendar".into(),
        summary: event.title.clone(),
        body,
        image: None,
        actions: Vec::new(),
        urgency: Urgency::Normal,
        time: Local::now(),
        resident: false,
    }
}

async fn fetch(url: &str) -> Result<String, String> {
    // Many apps share links as webcal://; it is plain https underneath.
    let url = url.trim().replacen("webcal://", "https://", 1);
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("not an http(s) or webcal link".into());
    }
    let out = tokio::process::Command::new("curl")
        .args([
            "--silent",
            "--fail",
            "--location",
            "--max-time",
            "20",
            "--max-filesize",
            "20000000",
            &url,
        ])
        .output()
        .await
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "download failed (curl exit {})",
            out.status.code().unwrap_or(-1)
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !text.contains("BEGIN:VCALENDAR") {
        return Err("that link doesn't return a calendar (ICS)".into());
    }
    Ok(text)
}

/// One `NAME;PARAM=x:VALUE` line.
struct Prop {
    name: String,
    params: HashMap<String, String>,
    value: String,
}

/// Unfolds continuation lines and splits properties.
fn props(text: &str) -> Vec<Prop> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        match (
            raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')),
            lines.last_mut(),
        ) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => lines.push(raw.trim_end_matches('\r').to_owned()),
        }
    }
    lines
        .iter()
        .filter_map(|line| {
            // The value starts at the first ':' outside a quoted parameter.
            let mut quoted = false;
            let split = line.char_indices().find(|&(_, c)| {
                if c == '"' {
                    quoted = !quoted;
                }
                c == ':' && !quoted
            })?;
            let (head, value) = (&line[..split.0], &line[split.0 + 1..]);
            let mut parts = head.split(';');
            let name = parts.next()?.to_ascii_uppercase();
            let params = parts
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_owned()))
                .collect();
            Some(Prop {
                name,
                params,
                value: value.to_owned(),
            })
        })
        .collect()
}

fn unescape(v: &str) -> String {
    v.replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// A DTSTART/DTEND/EXDATE value: the wall-clock time and how to place it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum When {
    Date(NaiveDate),
    Utc(NaiveDateTime),
    Zoned(NaiveDateTime, chrono_tz::Tz),
    /// No zone given: the user's local time.
    Floating(NaiveDateTime),
}

impl When {
    fn parse(p: &Prop) -> Option<Self> {
        let v = p.value.trim();
        if p.params.get("VALUE").is_some_and(|t| t == "DATE") || v.len() == 8 {
            return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(When::Date);
        }
        if let Some(utc) = v.strip_suffix('Z') {
            return NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S")
                .ok()
                .map(When::Utc);
        }
        let t = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
        // Unknown zone names (e.g. Outlook's Windows names) fall back to local time.
        Some(
            match p
                .params
                .get("TZID")
                .and_then(|z| z.parse::<chrono_tz::Tz>().ok())
            {
                Some(tz) => When::Zoned(t, tz),
                None => When::Floating(t),
            },
        )
    }

    fn naive(self) -> NaiveDateTime {
        match self {
            When::Date(d) => d.and_time(NaiveTime::MIN),
            When::Utc(t) | When::Zoned(t, _) | When::Floating(t) => t,
        }
    }

    /// The same kind of time with a different wall-clock value (for recurrences).
    fn with(self, t: NaiveDateTime) -> Self {
        match self {
            When::Date(_) => When::Date(t.date()),
            When::Utc(_) => When::Utc(t),
            When::Zoned(_, tz) => When::Zoned(t, tz),
            When::Floating(_) => When::Floating(t),
        }
    }

    fn local(self) -> Option<DateTime<Local>> {
        match self {
            When::Date(d) => Local
                .from_local_datetime(&d.and_time(NaiveTime::MIN))
                .earliest(),
            When::Utc(t) => Some(chrono::Utc.from_utc_datetime(&t).with_timezone(&Local)),
            When::Zoned(t, tz) => tz
                .from_local_datetime(&t)
                .earliest()
                .map(|d| d.with_timezone(&Local)),
            When::Floating(t) => Local.from_local_datetime(&t).earliest(),
        }
    }
}

/// `PT1H30M`, `P1D`, `P2W`.
fn duration(v: &str) -> Option<Duration> {
    let (neg, v) = v
        .strip_prefix('-')
        .map_or((false, v.trim_start_matches('+')), |r| (true, r));
    let v = v.strip_prefix('P')?;
    let (mut total, mut num, mut in_time) = (Duration::zero(), String::new(), false);
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (c, in_time) {
                    ('W', false) => Duration::weeks(n),
                    ('D', false) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

/// Raw VEVENT fields before expansion.
#[derive(Default)]
struct Raw {
    uid: String,
    title: String,
    location: String,
    description: String,
    url: String,
    conference: String,
    start: Option<When>,
    end: Option<When>,
    duration: Option<Duration>,
    rrule: Option<String>,
    exdates: Vec<NaiveDateTime>,
    recurrence_id: Option<NaiveDateTime>,
    cancelled: bool,
}

/// Parses an ICS document into events within the window around `today`.
pub fn parse(text: &str, calendar: usize, today: NaiveDate) -> Vec<Event> {
    let mut raws = Vec::new();
    let mut current: Option<Raw> = None;
    for p in props(text) {
        match (p.name.as_str(), p.value.as_str()) {
            ("BEGIN", "VEVENT") => current = Some(Raw::default()),
            ("END", "VEVENT") => raws.extend(current.take()),
            _ => {
                let Some(r) = current.as_mut() else { continue };
                match p.name.as_str() {
                    "UID" => r.uid = p.value.clone(),
                    "SUMMARY" => r.title = unescape(&p.value),
                    "LOCATION" => r.location = unescape(&p.value),
                    "DESCRIPTION" => r.description = unescape(&p.value),
                    "URL" => r.url = p.value.clone(),
                    "X-GOOGLE-CONFERENCE" => r.conference = p.value.clone(),
                    "DTSTART" => r.start = When::parse(&p),
                    "DTEND" => r.end = When::parse(&p),
                    "DURATION" => r.duration = duration(&p.value),
                    "RRULE" => r.rrule = Some(p.value.clone()),
                    "EXDATE" => {
                        for v in p.value.split(',') {
                            let one = Prop {
                                name: String::new(),
                                params: p.params.clone(),
                                value: v.to_owned(),
                            };
                            r.exdates.extend(When::parse(&one).map(When::naive));
                        }
                    }
                    "RECURRENCE-ID" => r.recurrence_id = When::parse(&p).map(When::naive),
                    "STATUS" => r.cancelled = p.value.eq_ignore_ascii_case("CANCELLED"),
                    _ => {}
                }
            }
        }
    }
    // Moved or edited occurrences replace the generated ones.
    let overridden: HashSet<(String, NaiveDateTime)> = raws
        .iter()
        .filter_map(|r| Some((r.uid.clone(), r.recurrence_id?)))
        .collect();
    let (from, to) = (today - Days::new(PAST_DAYS), today + Days::new(FUTURE_DAYS));
    let mut events = Vec::new();
    for r in raws.iter().filter(|r| !r.cancelled) {
        let Some(start) = r.start else { continue };
        let all_day = matches!(start, When::Date(_));
        let length = match (r.end, r.duration) {
            (Some(end), _) => end.naive() - start.naive(),
            (None, Some(d)) => d,
            (None, None) if all_day => Duration::days(1),
            (None, None) => Duration::zero(),
        };
        let starts = match (&r.rrule, r.recurrence_id) {
            (Some(rule), None) => expand(rule, start.naive(), from, to),
            _ => vec![start.naive()],
        };
        for s in starts {
            if r.exdates.contains(&s)
                || (r.recurrence_id.is_none() && overridden.contains(&(r.uid.clone(), s)))
            {
                continue;
            }
            if s.date() > to || (s + length).date() < from {
                continue;
            }
            let (Some(begin), Some(end)) = (start.with(s).local(), start.with(s + length).local())
            else {
                continue;
            };
            events.push(Event {
                title: if r.title.is_empty() {
                    "(No title)".into()
                } else {
                    r.title.clone()
                },
                start: begin,
                end,
                all_day,
                location: r.location.clone(),
                meeting: meeting_link(&[&r.conference, &r.url, &r.location, &r.description]),
                calendar,
            });
        }
    }
    events
}

/// Occurrence start times of an RRULE between `from` and `to` (and up to COUNT).
/// Supports FREQ=DAILY/WEEKLY/MONTHLY/YEARLY with INTERVAL, COUNT, UNTIL,
/// BYDAY (incl. `2TU`, `-1FR` for monthly) and BYMONTHDAY.
fn expand(rule: &str, start: NaiveDateTime, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDateTime> {
    let parts: HashMap<&str, &str> = rule
        .split(';')
        .filter_map(|kv| kv.split_once('='))
        .collect();
    let interval: u32 = parts
        .get("INTERVAL")
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1);
    let count: Option<usize> = parts.get("COUNT").and_then(|v| v.parse().ok());
    let until = parts.get("UNTIL").and_then(|v| {
        let v = v.trim_end_matches('Z');
        NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S")
            .ok()
            .or_else(|| {
                NaiveDate::parse_from_str(v, "%Y%m%d")
                    .ok()
                    .map(|d| d.and_hms_opt(23, 59, 59).unwrap_or_default())
            })
    });
    let bydays: Vec<(Option<i32>, Weekday)> = parts
        .get("BYDAY")
        .map_or(Vec::new(), |v| v.split(',').filter_map(byday).collect());
    let bymonthday: Vec<u32> = parts.get("BYMONTHDAY").map_or(Vec::new(), |v| {
        v.split(',').filter_map(|d| d.parse().ok()).collect()
    });
    let time = start.time();
    let mut out = Vec::new();
    let mut emitted = 0usize;
    let mut push = |d: NaiveDate, out: &mut Vec<NaiveDateTime>| -> bool {
        let t = d.and_time(time);
        if t < start {
            return true;
        }
        if until.is_some_and(|u| t > u) || count.is_some_and(|c| emitted >= c) || d > to {
            return false;
        }
        emitted += 1;
        if d + Days::new(1) >= from {
            out.push(t);
        }
        out.len() < MAX_INSTANCES
    };
    let first = start.date();
    match parts.get("FREQ").copied() {
        Some("DAILY") => {
            let mut d = first;
            while push(d, &mut out) {
                d = d + Days::new(u64::from(interval));
            }
        }
        Some("WEEKLY") => {
            let days: Vec<Weekday> = if bydays.is_empty() {
                vec![first.weekday()]
            } else {
                bydays.iter().map(|(_, w)| *w).collect()
            };
            let mut week = first - Days::new(u64::from(first.weekday().num_days_from_monday()));
            'weeks: loop {
                for offset in 0..7 {
                    let d = week + Days::new(offset);
                    if days.contains(&d.weekday()) && !push(d, &mut out) {
                        break 'weeks;
                    }
                }
                week = week + Days::new(7 * u64::from(interval));
            }
        }
        Some("MONTHLY") => {
            let mut month = first.with_day(1).unwrap_or(first);
            'months: loop {
                let mut days: Vec<NaiveDate> = if !bymonthday.is_empty() {
                    bymonthday
                        .iter()
                        .filter_map(|&d| month.with_day(d))
                        .collect()
                } else if !bydays.is_empty() {
                    bydays
                        .iter()
                        .filter_map(|&(n, w)| nth_weekday(month, n.unwrap_or(1), w))
                        .collect()
                } else {
                    month.with_day(first.day()).into_iter().collect()
                };
                days.sort();
                for d in days {
                    if !push(d, &mut out) {
                        break 'months;
                    }
                }
                let Some(next) = month.checked_add_months(Months::new(interval)) else {
                    break;
                };
                month = next;
            }
        }
        Some("YEARLY") => {
            let mut year = first.year();
            while let Some(d) = NaiveDate::from_ymd_opt(year, first.month(), first.day())
                .or_else(|| NaiveDate::from_ymd_opt(year, 3, 1))
            {
                if !push(d, &mut out) {
                    break;
                }
                year += interval as i32;
            }
        }
        _ => out.push(start), // unsupported rule: at least show the first one
    }
    out
}

/// `MO`, `2TU`, `-1FR`.
fn byday(s: &str) -> Option<(Option<i32>, Weekday)> {
    let s = s.trim();
    let (num, day) = s.split_at(s.len().checked_sub(2)?);
    let w = match day {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    };
    Some((
        if num.is_empty() {
            None
        } else {
            Some(num.trim_start_matches('+').parse().ok()?)
        },
        w,
    ))
}

/// The `n`th `w` of the month (`-1` = last).
fn nth_weekday(month: NaiveDate, n: i32, w: Weekday) -> Option<NaiveDate> {
    if n > 0 {
        NaiveDate::from_weekday_of_month_opt(month.year(), month.month(), w, u8::try_from(n).ok()?)
    } else {
        let last = month.checked_add_months(Months::new(1))? - Days::new(1);
        let back = (7 + last.weekday().num_days_from_monday() - w.num_days_from_monday()) % 7;
        let d = last - Days::new(u64::from(back) + 7 * u64::from((-n - 1).unsigned_abs()));
        (d.month() == month.month()).then_some(d)
    }
}

/// First link that looks like a video call, searching the given texts in order.
pub fn meeting_link(texts: &[&str]) -> Option<String> {
    const HOSTS: [&str; 8] = [
        "meet.google.com/",
        "zoom.us/j/",
        "zoom.us/my/",
        "teams.microsoft.com/l/meetup-join",
        "teams.live.com/meet",
        "webex.com/",
        "whereby.com/",
        "meet.jit.si/",
    ];
    texts.iter().find_map(|text| {
        text.match_indices("https://").find_map(|(i, _)| {
            let url: String = text[i..]
                .chars()
                .take_while(|c| !c.is_whitespace() && !"<>\"'()[]".contains(*c))
                .collect();
            HOSTS.iter().any(|h| url.contains(h)).then_some(url)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminders_fire_within_lead_before_start() {
        let now = Local::now();
        let event = |title: &str, minutes: i64, all_day: bool| Event {
            title: title.into(),
            start: now + Duration::minutes(minutes),
            end: now + Duration::minutes(minutes + 30),
            all_day,
            location: String::new(),
            meeting: None,
            calendar: 0,
        };
        let events = [
            event("started", -1, false),
            event("soon", 5, false),
            event("edge", 10, false),
            event("later", 11, false),
            event("holiday", 5, true),
        ];
        let titles: Vec<_> = due(&events, Duration::minutes(10), now)
            .map(|e| e.title.as_str())
            .collect();
        assert_eq!(titles, ["soon", "edge"]);
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Standup\\, daily\r\nDTSTART;TZID=Europe/Berlin:20261102T093000\r\n\
DTEND;TZID=Europe/Berlin:20261102T094500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;COUNT=6\r\nEXDATE;TZID=Europe/Berlin:20261104T093000\r\n\
DESCRIPTION:Join: https://meet.google.com/abc-defg-hij\\nThanks\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a\r\nRECURRENCE-ID;TZID=Europe/Berlin:20261106T093000\r\n\
SUMMARY:Standup (moved)\r\nDTSTART;TZID=Europe/Berlin:20261106T110000\r\nDTEND;TZID=Europe/Berlin:20261106T111500\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261118\r\nDTEND;VALUE=DATE:20261119\r\nLOCATION:Home\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:Gone\r\nSTATUS:CANCELLED\r\nDTSTART:20261118T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_recurrence_exceptions_and_overrides() {
        let events = parse(ICS, 0, d("2026-11-16"));
        let standups: Vec<&Event> = events
            .iter()
            .filter(|e| e.title.starts_with("Standup"))
            .collect();
        // 6 occurrences (Nov 2,4,6,9,11,13), minus the excluded Nov 4; Nov 6 replaced by the moved one.
        assert_eq!(standups.len(), 5);
        assert_eq!(
            standups
                .iter()
                .filter(|e| e.title == "Standup, daily")
                .count(),
            4
        );
        let moved = standups
            .iter()
            .find(|e| e.title == "Standup (moved)")
            .unwrap();
        let berlin = chrono_tz::Europe::Berlin;
        assert_eq!(
            moved
                .start
                .with_timezone(&berlin)
                .format("%m-%d %H:%M")
                .to_string(),
            "11-06 11:00"
        );
        let first = standups
            .iter()
            .find(|e| e.title == "Standup, daily")
            .unwrap();
        assert_eq!(
            first.meeting.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(first.end - first.start, Duration::minutes(15));
        let holiday = events.iter().find(|e| e.title == "Holiday").unwrap();
        assert!(holiday.all_day && holiday.location == "Home");
        assert!(
            !events.iter().any(|e| e.title == "Gone"),
            "cancelled events are dropped"
        );
    }

    #[test]
    fn expands_rules() {
        let at = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        let (from, to) = (d("2026-01-01"), d("2026-12-31"));
        let dates = |v: Vec<NaiveDateTime>| {
            v.iter()
                .map(|t| t.format("%m-%d").to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            dates(expand(
                "FREQ=DAILY;INTERVAL=2;COUNT=3",
                at("2026-03-01 08:00"),
                from,
                to
            )),
            ["03-01", "03-03", "03-05"]
        );
        assert_eq!(
            dates(expand(
                "FREQ=MONTHLY;BYDAY=-1FR;COUNT=2",
                at("2026-01-30 10:00"),
                from,
                to
            )),
            ["01-30", "02-27"]
        );
        assert_eq!(
            dates(expand(
                "FREQ=MONTHLY;BYDAY=2TU;UNTIL=20260331",
                at("2026-01-13 10:00"),
                from,
                to
            )),
            ["01-13", "02-10", "03-10"]
        );
        assert_eq!(
            dates(expand("FREQ=YEARLY", at("2025-06-01 00:00"), from, to)),
            ["06-01"]
        );
        assert_eq!(
            expand("FREQ=WEEKLY", at("2026-12-28 09:00"), from, to).len(),
            1,
            "stops at the window end"
        );
    }

    #[test]
    fn small_parsers() {
        assert_eq!(duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(duration("P1W"), Some(Duration::weeks(1)));
        assert_eq!(byday("-1FR"), Some((Some(-1), Weekday::Fri)));
        assert_eq!(
            meeting_link(&["", "Call: https://us02web.zoom.us/j/123?pwd=x (Zoom)"]).as_deref(),
            Some("https://us02web.zoom.us/j/123?pwd=x")
        );
        assert_eq!(meeting_link(&["https://example.com/agenda"]), None);
        let folded = props("DESCRIPTION:Line one\r\n  continues\r\nX;TZID=\"a:b\":v\r\n");
        assert_eq!(folded[0].value, "Line one continues");
        assert_eq!(
            (folded[1].params["TZID"].as_str(), folded[1].value.as_str()),
            ("a:b", "v")
        );
    }
}
