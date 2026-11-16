//! Weather from Open-Meteo (free, no API key), fetched with `curl` so no TLS
//! library is linked. Only a location the user picked is ever sent.

use chrono::{NaiveDate, NaiveDateTime};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::watch;
use tracing::warn;

/// How often the forecast refreshes.
const REFRESH: Duration = Duration::from_secs(30 * 60);

/// A place from the geocoding search.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Location {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub country: String,
    /// State or region.
    #[serde(default)]
    pub admin1: String,
}

impl Location {
    /// `Berlin, State of Berlin, Germany`
    pub fn label(&self) -> String {
        [&self.name, &self.admin1, &self.country]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Where and how to fetch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Query {
    pub latitude: f64,
    pub longitude: f64,
    /// °F and mph instead of °C and km/h.
    pub imperial: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub imperial: bool,
    pub current: Current,
    /// The next 24 hours, starting with the current one.
    pub hourly: Vec<Hour>,
    /// Today and the next 6 days.
    pub daily: Vec<Day>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Current {
    pub time: NaiveDateTime,
    pub temperature: f64,
    pub feels_like: f64,
    pub humidity: f64,
    pub wind: f64,
    pub code: u8,
    pub is_day: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hour {
    pub time: NaiveDateTime,
    pub temperature: f64,
    pub code: u8,
    pub precipitation: u8,
    pub is_day: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub max: f64,
    pub min: f64,
    pub code: u8,
    pub precipitation: u8,
    pub sunrise: Option<NaiveDateTime>,
    pub sunset: Option<NaiveDateTime>,
}

/// Material Symbols icon for a WMO weather code.
pub fn icon(code: u8, is_day: bool) -> &'static str {
    match code {
        0 | 1 if is_day => "sunny",
        0 | 1 => "clear_night",
        2 if is_day => "partly_cloudy_day",
        2 => "partly_cloudy_night",
        3 => "cloud",
        45 | 48 => "foggy",
        51..=57 => "rainy_light",
        61..=67 | 80 => "rainy",
        81 | 82 => "rainy_heavy",
        71..=77 | 85 | 86 => "weather_snowy",
        95..=99 => "thunderstorm",
        _ => "cloud",
    }
}

/// Short description of a WMO weather code.
pub fn describe(code: u8) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 | 73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 | 81 => "Rain showers",
        82 => "Violent showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

/// Keeps the forecast fresh for whatever location `query` holds (`None` = off).
/// Re-fetches every 30 minutes, whenever the query changes, and on `refresh`.
pub fn spawn(
    mut query: watch::Receiver<Option<Query>>,
    refresh: std::sync::Arc<tokio::sync::Notify>,
) -> watch::Receiver<Option<Weather>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        loop {
            let q = *query.borrow_and_update();
            match q {
                Some(q) => match fetch(q).await {
                    Ok(w) => {
                        tx.send_replace(Some(w));
                    }
                    Err(e) => warn!("weather: {e}"), // keep showing the last forecast
                },
                None => {
                    tx.send_replace(None);
                }
            }
            tokio::select! {
                changed = query.changed() => if changed.is_err() { return },
                () = refresh.notified() => {}
                () = tokio::time::sleep(REFRESH) => {}
            }
        }
    });
    rx
}

/// Searches places by name.
pub async fn geocode(name: &str) -> Result<Vec<Location>, String> {
    #[derive(Deserialize)]
    struct Reply {
        #[serde(default)]
        results: Vec<Location>,
    }
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?count=6&language=en&name={}",
        encode(name.trim())
    );
    let reply: Reply = serde_json::from_slice(&curl(&url).await?)
        .map_err(|e| format!("unexpected geocoding reply: {e}"))?;
    Ok(reply.results)
}

/// Fetches the forecast for `q`.
pub async fn fetch(q: Query) -> Result<Weather, String> {
    let units = if q.imperial {
        "&temperature_unit=fahrenheit&wind_speed_unit=mph"
    } else {
        ""
    };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&timezone=auto&forecast_days=7\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day\
         &hourly=temperature_2m,weather_code,precipitation_probability,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_probability_max{units}",
        q.latitude, q.longitude
    );
    parse(&curl(&url).await?, q.imperial)
}

async fn curl(url: &str) -> Result<Vec<u8>, String> {
    let out = tokio::process::Command::new("curl")
        .args(["--silent", "--fail", "--location", "--max-time", "15", url])
        .output()
        .await
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "request failed (curl exit {})",
            out.status.code().unwrap_or(-1)
        ));
    }
    Ok(out.stdout)
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Deserialize)]
struct Raw {
    current: RawCurrent,
    hourly: RawHourly,
    daily: RawDaily,
}

#[derive(Deserialize)]
struct RawCurrent {
    time: String,
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    weather_code: u8,
    wind_speed_10m: f64,
    is_day: u8,
}

#[derive(Deserialize)]
struct RawHourly {
    time: Vec<String>,
    temperature_2m: Vec<Option<f64>>,
    weather_code: Vec<Option<u8>>,
    precipitation_probability: Vec<Option<u8>>,
    is_day: Vec<Option<u8>>,
}

#[derive(Deserialize)]
struct RawDaily {
    time: Vec<String>,
    weather_code: Vec<Option<u8>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    sunrise: Vec<Option<String>>,
    sunset: Vec<Option<String>>,
    precipitation_probability_max: Vec<Option<u8>>,
}

fn time(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").ok()
}

fn parse(json: &[u8], imperial: bool) -> Result<Weather, String> {
    let raw: Raw =
        serde_json::from_slice(json).map_err(|e| format!("unexpected forecast reply: {e}"))?;
    let c = raw.current;
    let now = time(&c.time).ok_or("bad time in forecast")?;
    let h = raw.hourly;
    let hourly = (0..h.time.len())
        .filter_map(|i| {
            Some(Hour {
                time: time(&h.time[i])?,
                temperature: (*h.temperature_2m.get(i)?)?,
                code: h.weather_code.get(i).copied().flatten().unwrap_or(3),
                precipitation: h
                    .precipitation_probability
                    .get(i)
                    .copied()
                    .flatten()
                    .unwrap_or(0),
                is_day: h.is_day.get(i).copied().flatten() == Some(1),
            })
        })
        // The hour containing "now" onwards.
        .filter(|hour| hour.time + chrono::Duration::hours(1) > now)
        .take(24)
        .collect();
    let d = raw.daily;
    let daily = (0..d.time.len())
        .filter_map(|i| {
            Some(Day {
                date: NaiveDate::parse_from_str(&d.time[i], "%Y-%m-%d").ok()?,
                max: (*d.temperature_2m_max.get(i)?)?,
                min: (*d.temperature_2m_min.get(i)?)?,
                code: d.weather_code.get(i).copied().flatten().unwrap_or(3),
                precipitation: d
                    .precipitation_probability_max
                    .get(i)
                    .copied()
                    .flatten()
                    .unwrap_or(0),
                sunrise: d.sunrise.get(i).cloned().flatten().and_then(|s| time(&s)),
                sunset: d.sunset.get(i).cloned().flatten().and_then(|s| time(&s)),
            })
        })
        .collect();
    Ok(Weather {
        imperial,
        current: Current {
            time: now,
            temperature: c.temperature_2m,
            feels_like: c.apparent_temperature,
            humidity: c.relative_humidity_2m,
            wind: c.wind_speed_10m,
            code: c.weather_code,
            is_day: c.is_day == 1,
        },
        hourly,
        daily,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "current": {"time":"2026-11-16T10:15","temperature_2m":4.2,"apparent_temperature":1.0,"relative_humidity_2m":81,"weather_code":61,"wind_speed_10m":14.5,"is_day":1},
        "hourly": {"time":["2026-11-16T09:00","2026-11-16T10:00","2026-11-16T11:00"],"temperature_2m":[3.9,4.2,null],"weather_code":[3,61,61],"precipitation_probability":[20,70,80],"is_day":[1,1,1]},
        "daily": {"time":["2026-11-16","2026-11-17"],"weather_code":[61,0],"temperature_2m_max":[6.1,8.0],"temperature_2m_min":[1.2,-0.5],
                  "sunrise":["2026-11-16T07:30",null],"sunset":["2026-11-16T16:10","2026-11-17T16:09"],"precipitation_probability_max":[80,null]}
    }"#;

    #[test]
    fn parses_forecast() {
        let w = parse(SAMPLE.as_bytes(), false).unwrap();
        assert_eq!(w.current.code, 61);
        assert_eq!(
            w.hourly.len(),
            1,
            "past hours and hours without data are dropped"
        );
        assert_eq!(w.hourly[0].precipitation, 70);
        assert_eq!(w.daily.len(), 2);
        assert_eq!(w.daily[1].min, -0.5);
        assert_eq!(w.daily[1].sunrise, None);
        assert!(parse(b"{}", false).is_err());
    }

    #[test]
    fn icons_and_text() {
        assert_eq!(icon(0, true), "sunny");
        assert_eq!(icon(0, false), "clear_night");
        assert_eq!(icon(95, true), "thunderstorm");
        assert_eq!(describe(65), "Heavy rain");
    }

    #[test]
    fn encodes_queries() {
        assert_eq!(encode("São Paulo"), "S%C3%A3o%20Paulo");
    }
}
