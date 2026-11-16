//! Output volume (PipeWire/PulseAudio via `pactl`) and screen brightness
//! (`brightnessctl`). Volume updates come from `pactl subscribe` events, so
//! there is no polling.

use serde::Deserialize;
use std::{collections::HashMap, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command as Process,
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

/// A speaker/headset (output) or microphone (input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// PipeWire node name, what `pactl` takes.
    pub name: String,
    pub description: String,
    pub muted: bool,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Devices {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
}

impl Devices {
    /// The default microphone, if there is a real one.
    pub fn mic(&self) -> Option<&Device> {
        self.inputs.iter().find(|d| d.default)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceCommand {
    /// Make this output (`true`) or input the default.
    SetDefault(bool, String),
    ToggleMicMute,
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

/// Watches the default sink, and the device lists. `None` when pactl isn't available.
pub fn spawn_volume() -> (watch::Receiver<Option<Volume>>, watch::Receiver<Devices>) {
    let (tx, rx) = watch::channel(None);
    let (dev_tx, dev_rx) = watch::channel(Devices::default());
    tokio::spawn(async move {
        loop {
            let child = Process::new("pactl")
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
            let d = read_devices().await;
            dev_tx.send_if_modified(|cur| replace(cur, d));
            let Some(stdout) = child.stdout.take() else {
                return;
            };
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // Sink changes and default-sink switches ("server") both matter.
                if !["sink", "server", "source", "card"]
                    .iter()
                    .any(|w| line.contains(w))
                {
                    continue;
                }
                // Plain volume changes are most events; device lists only change otherwise.
                let mut lists = changes_devices(&line);
                while let Ok(Ok(Some(more))) =
                    tokio::time::timeout(Duration::from_millis(30), lines.next_line()).await
                {
                    lists |= changes_devices(&more);
                }
                if lists {
                    let d = read_devices().await;
                    dev_tx.send_if_modified(|cur| replace(cur, d));
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
    (rx, dev_rx)
}

fn replace<T: PartialEq>(slot: &mut T, new: T) -> bool {
    let changed = *slot != new;
    if changed {
        *slot = new;
    }
    changed
}

async fn read_volume() -> Option<Volume> {
    let vol = Process::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .await
        .ok()?;
    let mute = Process::new("pactl")
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

/// A `pactl subscribe` line that can change the device lists, defaults or mic mute.
fn changes_devices(line: &str) -> bool {
    [
        "on server",
        "on source #",
        "on card #",
        "'new' on sink #",
        "'remove' on sink #",
    ]
    .iter()
    .any(|p| line.contains(p))
}

async fn read_devices() -> Devices {
    let run = async |args: &[&str]| {
        Process::new("pactl")
            .args(args)
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    };
    Devices {
        outputs: parse_devices(
            &run(&["-f", "json", "list", "sinks"]).await,
            &run(&["get-default-sink"]).await,
        ),
        inputs: parse_devices(
            &run(&["-f", "json", "list", "sources"]).await,
            &run(&["get-default-source"]).await,
        ),
    }
}

/// `pactl -f json list sinks|sources`, without the "Monitor of …" sources.
fn parse_devices(json: &str, default: &str) -> Vec<Device> {
    #[derive(Deserialize)]
    struct Node {
        name: String,
        description: String,
        mute: bool,
        #[serde(default)]
        properties: HashMap<String, serde_json::Value>,
    }
    let nodes: Vec<Node> = serde_json::from_str(json).unwrap_or_default();
    nodes
        .into_iter()
        .filter(|n| n.properties.get("device.class").and_then(|v| v.as_str()) != Some("monitor"))
        .map(|n| Device {
            default: n.name == default,
            name: n.name,
            description: n.description,
            muted: n.mute,
        })
        .collect()
}

pub async fn device_command(cmd: DeviceCommand) -> Result<(), String> {
    let args = match &cmd {
        DeviceCommand::SetDefault(true, name) => vec!["set-default-sink", name.as_str()],
        DeviceCommand::SetDefault(false, name) => vec!["set-default-source", name.as_str()],
        DeviceCommand::ToggleMicMute => vec!["set-source-mute", "@DEFAULT_SOURCE@", "toggle"],
    };
    let ok = Process::new("pactl")
        .args(&args)
        .status()
        .await
        .map_err(|e| format!("cannot run pactl: {e}"))?
        .success();
    if ok {
        Ok(())
    } else {
        Err("pactl failed".into())
    }
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
    let ok = Process::new("pactl")
        .args(&args)
        .status()
        .await
        .map_err(|e| format!("cannot run pactl: {e}"))?
        .success();
    // Raising the volume also unmutes, as people expect.
    if ok && matches!(change, Change::Up(_)) && current.is_some_and(|v| v.muted) {
        let _ = Process::new("pactl")
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
    let out = Process::new("brightnessctl")
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
    let ok = Process::new("brightnessctl")
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
    fn parses_devices_without_monitors() {
        let json = r#"[
            {"name":"hdmi","description":"HDMI","mute":false,"properties":{"device.class":"sound"}},
            {"name":"hdmi.monitor","description":"Monitor of HDMI","mute":false,"properties":{"device.class":"monitor"}},
            {"name":"mic","description":"Mic","mute":true}
        ]"#;
        let d = parse_devices(json, "mic");
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[1].name.as_str(), d[1].muted, d[1].default),
            ("mic", true, true)
        );
        assert!(!d[0].default);
        assert!(parse_devices("not json", "").is_empty());
        assert!(changes_devices("Event 'change' on server #-1"));
        assert!(changes_devices("Event 'new' on sink #60"));
        assert!(!changes_devices("Event 'change' on sink #54"));
        assert!(!changes_devices("Event 'new' on sink-input #91"));
        assert!(!changes_devices("Event 'change' on source-output #3"));
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

/// Requests for volume, audio devices and brightness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Volume(Change),
    /// Default output/input and microphone mute.
    Device(DeviceCommand),
    Brightness(Change),
}

/// What [`Controls::handle`] reports back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// New backlight level after a change.
    Brightness(Option<u8>),
}

/// Carries out [`Command`]s.
#[derive(Clone)]
pub struct Controls {
    volume: watch::Receiver<Option<Volume>>,
}

impl Controls {
    /// `volume` is the current volume, which relative changes start from.
    pub fn new(volume: watch::Receiver<Option<Volume>>) -> Self {
        Self { volume }
    }

    pub async fn handle(
        &self,
        command: Command,
        reply: impl Fn(Reply) + Send + Sync,
    ) -> Result<(), String> {
        match command {
            Command::Volume(change) => {
                let current = *self.volume.borrow();
                change_volume(change, current).await
            }
            Command::Device(cmd) => device_command(cmd).await,
            Command::Brightness(change) => {
                reply(Reply::Brightness(change_brightness(change).await?));
                Ok(())
            }
        }
    }
}
