//! Output volume (PipeWire/PulseAudio via `pactl`) and screen brightness
//! (`brightnessctl`). Volume updates come from `pactl subscribe` events, so
//! there is no polling.

use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
};
use tracing::info;

/// Default output device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    /// Percent; can exceed 100 if something boosted it.
    pub percent: u16,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Up(u8),
    Down(u8),
    Set(u8),
    ToggleMute,
}

impl Change {
    /// `up`, `down`, `mute`, `set 40` (with an optional step: `up 10`).
    pub fn parse(words: &[&str]) -> Option<Self> {
        let n = |i: usize, default: u8| {
            words
                .get(i)
                .map_or(Some(default), |w| w.trim_end_matches('%').parse().ok())
        };
        Some(match *words.first()? {
            "up" | "raise" => Change::Up(n(1, 5)?),
            "down" | "lower" => Change::Down(n(1, 5)?),
            "set" => Change::Set(n(1, 0)?.min(100)),
            "mute" | "toggle-mute" => Change::ToggleMute,
            _ => return None,
        })
    }
}

/// Watches the default sink. `None` when pactl isn't available.
pub fn spawn_volume() -> watch::Receiver<Option<Volume>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        loop {
            let child = Command::new("pactl")
                .arg("subscribe")
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn();
            let Ok(mut child) = child else {
                info!("pactl not found; volume disabled");
                return;
            };
            let v = read_volume().await;
            tx.send_if_modified(|cur| replace(cur, v));
            let Some(stdout) = child.stdout.take() else {
                return;
            };
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // Sink changes and default-sink switches ("server") both matter.
                if !(line.contains("sink") || line.contains("server")) {
                    continue;
                }
                while let Ok(Ok(Some(_))) =
                    tokio::time::timeout(Duration::from_millis(30), lines.next_line()).await
                {
                }
                let v = read_volume().await;
                if tx.send_if_modified(|cur| replace(cur, v)) {
                    continue;
                }
                if tx.is_closed() {
                    return;
                }
            }
            // The audio server restarted; resubscribe.
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    rx
}

fn replace<T: PartialEq>(slot: &mut T, new: T) -> bool {
    let changed = *slot != new;
    if changed {
        *slot = new;
    }
    changed
}

async fn read_volume() -> Option<Volume> {
    let vol = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .await
        .ok()?;
    let mute = Command::new("pactl")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
        .await
        .ok()?;
    parse_volume(
        &String::from_utf8_lossy(&vol.stdout),
        &String::from_utf8_lossy(&mute.stdout),
    )
}

/// `Volume: front-left: 65668 / 100% / 0.05 dB, ...` and `Mute: no`.
fn parse_volume(vol: &str, mute: &str) -> Option<Volume> {
    let percent = vol
        .split('/')
        .find_map(|part| part.trim().strip_suffix('%')?.trim().parse().ok())?;
    Some(Volume {
        percent,
        muted: mute.trim().ends_with("yes"),
    })
}

/// Changes the default sink's volume (capped at 100%).
pub async fn change_volume(change: Change, current: Option<Volume>) -> Result<(), String> {
    let args: Vec<String> = match change {
        Change::ToggleMute => vec![
            "set-sink-mute".into(),
            "@DEFAULT_SINK@".into(),
            "toggle".into(),
        ],
        Change::Set(p) => vec![
            "set-sink-volume".into(),
            "@DEFAULT_SINK@".into(),
            format!("{p}%"),
        ],
        Change::Up(step) => {
            let now = current.map_or(0, |v| v.percent);
            let target = (now + u16::from(step)).min(100.max(now));
            vec![
                "set-sink-volume".into(),
                "@DEFAULT_SINK@".into(),
                format!("{target}%"),
            ]
        }
        Change::Down(step) => vec![
            "set-sink-volume".into(),
            "@DEFAULT_SINK@".into(),
            format!("-{step}%"),
        ],
    };
    let ok = Command::new("pactl")
        .args(&args)
        .status()
        .await
        .map_err(|e| format!("cannot run pactl: {e}"))?
        .success();
    // Raising the volume also unmutes, as people expect.
    if ok && matches!(change, Change::Up(_)) && current.is_some_and(|v| v.muted) {
        let _ = Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "0"])
            .status()
            .await;
    }
    if ok {
        Ok(())
    } else {
        Err("pactl failed".into())
    }
}

/// Backlight brightness in percent, or `None` without a backlight (desktops).
pub async fn read_brightness() -> Option<u8> {
    let out = Command::new("brightnessctl")
        .args(["-m", "-c", "backlight"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_brightness(&String::from_utf8_lossy(&out.stdout))
}

/// `intel_backlight,backlight,400,42%,960`
fn parse_brightness(line: &str) -> Option<u8> {
    line.lines()
        .next()?
        .split(',')
        .nth(3)?
        .trim_end_matches('%')
        .parse()
        .ok()
}

/// Changes the backlight; returns the new percentage.
pub async fn change_brightness(change: Change) -> Result<Option<u8>, String> {
    let value = match change {
        Change::Up(s) => format!("{s}%+"),
        Change::Down(s) => format!("{s}%-"),
        Change::Set(p) => format!("{p}%"),
        Change::ToggleMute => return Ok(read_brightness().await),
    };
    // Never all the way to black: 1% stays visible.
    let ok = Command::new("brightnessctl")
        .args(["-c", "backlight", "-n", "1%", "set", &value])
        .stdout(Stdio::null())
        .status()
        .await;
    match ok {
        Ok(s) if s.success() => Ok(read_brightness().await),
        Ok(_) => Err("no backlight to change".into()),
        Err(e) => Err(format!("cannot run brightnessctl: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pactl_and_brightnessctl() {
        let v = parse_volume(
            "Volume: front-left: 42598 /  65% / -11.23 dB,   front-right: 42598 /  65% / -11.23 dB\n        balance 0.00",
            "Mute: yes\n",
        );
        assert_eq!(
            v,
            Some(Volume {
                percent: 65,
                muted: true
            })
        );
        assert_eq!(parse_volume("garbage", "Mute: no"), None);
        assert_eq!(
            parse_brightness("intel_backlight,backlight,400,42%,960\n"),
            Some(42)
        );
        assert_eq!(parse_brightness(""), None);
    }

    #[test]
    fn parses_changes() {
        assert_eq!(Change::parse(&["up"]), Some(Change::Up(5)));
        assert_eq!(Change::parse(&["down", "10%"]), Some(Change::Down(10)));
        assert_eq!(Change::parse(&["set", "150"]), Some(Change::Set(100)));
        assert_eq!(Change::parse(&["mute"]), Some(Change::ToggleMute));
        assert_eq!(Change::parse(&["louder"]), None);
        assert_eq!(Change::parse(&["up", "lots"]), None);
    }
}
