//! Bluetooth via BlueZ on the system bus, plus headset profile switching
//! (music vs. calls) through PipeWire's `pactl`.
//!
//! BlueZ signals trigger a re-read of every object, as in the other services.

use futures_util::{StreamExt, stream::select_all};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Notify, mpsc, watch};
use tracing::{info, warn};
use zbus::{
    Connection, MessageStream, Proxy,
    zvariant::{OwnedObjectPath, OwnedValue},
};

type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

/// A headset's PipeWire profiles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profiles {
    pub active: String,
    /// High-quality playback (A2DP).
    pub audio: Option<String>,
    /// Microphone on, lower quality (HFP/HSP).
    pub call: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// BlueZ object path.
    pub path: String,
    pub name: String,
    pub address: String,
    /// freedesktop icon name from BlueZ, e.g. `audio-headset`.
    pub icon: String,
    pub paired: bool,
    /// Trusted devices reconnect on their own ("auto connect").
    pub trusted: bool,
    pub connected: bool,
    pub battery: Option<u8>,
    pub rssi: Option<i16>,
    /// Only for connected audio devices.
    pub profiles: Option<Profiles>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// An adapter exists.
    pub available: bool,
    /// Its object path, e.g. `/org/bluez/hci0`.
    pub adapter: String,
    pub powered: bool,
    pub discovering: bool,
    /// Connected first, then paired, then by signal strength.
    pub devices: Vec<Device>,
    /// Last failed command, for the settings page.
    pub error: String,
    /// While pairing a keyboard: the code to type on it.
    pub pairing_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Power(bool),
    Scan(bool),
    Connect(String),
    Disconnect(String),
    /// Pair, then trust and connect.
    Pair(String),
    /// Auto connect on/off.
    Trust(String, bool),
    /// Unpair and forget.
    Remove(String),
    /// Switch a connected headset to this PipeWire profile (address, profile).
    Profile(String, String),
}

/// Confirms pairings that Rune itself starts. Registered without being the
/// default agent, so BlueZ only consults it for our own `Pair` calls.
struct Agent {
    code: Arc<Mutex<Option<String>>>,
    changed: Arc<Notify>,
}

impl Agent {
    fn show(&self, code: Option<String>) {
        if let Ok(mut c) = self.code.lock() {
            *c = code;
        }
        self.changed.notify_one();
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {
        self.show(None);
    }
    fn request_confirmation(&self, _device: OwnedObjectPath, _passkey: u32) {}
    fn request_authorization(&self, _device: OwnedObjectPath) {}
    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) {}
    /// Old devices (pre-2.1) ask for a PIN; almost all of them use 0000.
    fn request_pin_code(&self, _device: OwnedObjectPath) -> zbus::fdo::Result<String> {
        Ok("0000".into())
    }
    fn request_passkey(&self, _device: OwnedObjectPath) -> zbus::fdo::Result<u32> {
        Err(zbus::fdo::Error::Failed(
            "passkey entry is not supported".into(),
        ))
    }
    /// Keyboards: the user types this code on the device.
    fn display_pin_code(&self, _device: OwnedObjectPath, pincode: String) {
        self.show(Some(pincode));
    }
    fn display_passkey(&self, _device: OwnedObjectPath, passkey: u32, _entered: u16) {
        self.show(Some(format!("{passkey:06}")));
    }
    fn cancel(&self) {
        self.show(None);
    }
}

const AGENT_PATH: &str = "/org/rune/bluetooth_agent";

/// Starts the service. With no BlueZ (or no adapter) the state stays "unavailable".
pub async fn spawn() -> (watch::Receiver<State>, mpsc::UnboundedSender<Command>) {
    let (tx, rx) = watch::channel(State::default());
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        if let Err(e) = run(tx, cmd_rx).await {
            info!("bluetooth unavailable: {e}");
        }
    });
    (rx, cmd_tx)
}

async fn run(
    tx: watch::Sender<State>,
    mut cmds: mpsc::UnboundedReceiver<Command>,
) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let code = Arc::new(Mutex::new(None));
    let code_changed = Arc::new(Notify::new());
    conn.object_server()
        .at(
            AGENT_PATH,
            Agent {
                code: code.clone(),
                changed: code_changed.clone(),
            },
        )
        .await?;
    let manager = Proxy::new(&conn, "org.bluez", "/org/bluez", "org.bluez.AgentManager1").await?;
    if let Err(e) = manager
        .call_method(
            "RegisterAgent",
            &(OwnedObjectPath::try_from(AGENT_PATH)?, "NoInputNoOutput"),
        )
        .await
    {
        warn!("bluetooth: cannot register a pairing agent ({e}); pairing may need another tool");
    }
    let rules = [
        "type='signal',sender='org.bluez',interface='org.freedesktop.DBus.ObjectManager'",
        "type='signal',sender='org.bluez',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
    ];
    let mut streams = Vec::new();
    for rule in rules {
        streams.push(MessageStream::for_match_rule(rule, &conn, Some(256)).await?);
    }
    let mut events = select_all(streams);
    let (err_tx, mut err_rx) = mpsc::unbounded_channel::<String>();
    let mut error = String::new();
    loop {
        let mut state = snapshot(&conn).await.unwrap_or_default();
        for d in state
            .devices
            .iter_mut()
            .filter(|d| d.connected && d.icon.starts_with("audio"))
        {
            d.profiles = profiles(&d.address).await;
        }
        state.error = error.clone();
        state.pairing_code = code.lock().ok().and_then(|c| c.clone());
        tx.send_if_modified(|s| {
            let changed = *s != state;
            if changed {
                *s = state;
            }
            changed
        });
        tokio::select! {
            event = events.next() => if event.is_none() { return Ok(()) },
            cmd = cmds.recv() => {
                let Some(cmd) = cmd else { return Ok(()) };
                error.clear();
                // Connecting can take seconds; don't hold up updates meanwhile.
                let (conn, err_tx, code, changed) = (conn.clone(), err_tx.clone(), code.clone(), code_changed.clone());
                tokio::spawn(async move {
                    let pairing = matches!(cmd, Command::Pair(_));
                    if let Err(e) = execute(&conn, cmd).await {
                        let _ = err_tx.send(e);
                    }
                    // Whatever happened, the code on screen is no longer needed.
                    if pairing && let Ok(mut c) = code.lock() {
                        *c = None;
                        changed.notify_one();
                    }
                });
            }
            () = code_changed.notified() => {}
            Some(e) = err_rx.recv() => {
                warn!("bluetooth: {e}");
                error = e;
            }
        }
        // Discovery sends a stream of signal-strength updates; batch them.
        while let Ok(Some(_)) =
            tokio::time::timeout(Duration::from_millis(150), events.next()).await
        {}
    }
}

async fn snapshot(conn: &Connection) -> zbus::Result<State> {
    let om = Proxy::new(conn, "org.bluez", "/", "org.freedesktop.DBus.ObjectManager").await?;
    let objects: Objects = om.call("GetManagedObjects", &()).await?;
    Ok(from_objects(&objects))
}

fn from_objects(objects: &Objects) -> State {
    let mut state = State::default();
    let get = |props: &HashMap<String, OwnedValue>, k: &str| {
        props.get(k).and_then(|v| v.try_clone().ok())
    };
    let flag = |props: &HashMap<String, OwnedValue>, k: &str| {
        get(props, k)
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false)
    };
    let text = |props: &HashMap<String, OwnedValue>, k: &str| {
        get(props, k)
            .and_then(|v| String::try_from(v).ok())
            .unwrap_or_default()
    };
    // First adapter only; multi-adapter setups are rare.
    let adapter = objects
        .iter()
        .filter_map(|(p, i)| i.get("org.bluez.Adapter1").map(|a| (p.as_str(), a)))
        .min_by_key(|(p, _)| *p);
    let Some((adapter_path, adapter)) = adapter else {
        return state;
    };
    state.available = true;
    adapter_path.clone_into(&mut state.adapter);
    state.powered = flag(adapter, "Powered");
    state.discovering = flag(adapter, "Discovering");
    for (path, ifaces) in objects {
        let Some(d) = ifaces.get("org.bluez.Device1") else {
            continue;
        };
        if !path.as_str().starts_with(adapter_path) {
            continue;
        }
        let address = text(d, "Address");
        let name = [text(d, "Alias"), text(d, "Name")]
            .into_iter()
            .find(|n| !n.is_empty())
            .unwrap_or_else(|| address.clone());
        state.devices.push(Device {
            path: path.to_string(),
            name,
            icon: text(d, "Icon"),
            paired: flag(d, "Paired"),
            trusted: flag(d, "Trusted"),
            connected: flag(d, "Connected"),
            battery: ifaces
                .get("org.bluez.Battery1")
                .and_then(|b| get(b, "Percentage"))
                .and_then(|v| u8::try_from(v).ok()),
            rssi: get(d, "RSSI").and_then(|v| i16::try_from(v).ok()),
            profiles: None,
            address,
        });
    }
    state.devices.sort_by(|a, b| {
        (b.connected, b.paired, b.rssi.unwrap_or(i16::MIN), &a.name).cmp(&(
            a.connected,
            a.paired,
            a.rssi.unwrap_or(i16::MIN),
            &b.name,
        ))
    });
    // Unnamed, unpaired devices found while scanning are mostly noise.
    state.devices.retain(|d| d.paired || d.name != d.address);
    state
}

async fn execute(conn: &Connection, cmd: Command) -> Result<(), String> {
    let device = |path: &str| {
        let path = path.to_owned();
        async move {
            Proxy::new(conn, "org.bluez", path, "org.bluez.Device1")
                .await
                .map_err(|e| e.to_string())
        }
    };
    let adapter = || async {
        let state = snapshot(conn).await.map_err(|e| e.to_string())?;
        if !state.available {
            return Err("no Bluetooth adapter".to_owned());
        }
        Proxy::new(conn, "org.bluez", state.adapter, "org.bluez.Adapter1")
            .await
            .map_err(|e| e.to_string())
    };
    let call = |r: zbus::Result<()>, what: &str| r.map_err(|e| format!("{what}: {e}"));
    match cmd {
        Command::Power(on) => call(
            adapter()
                .await?
                .set_property("Powered", on)
                .await
                .map_err(Into::into),
            "power",
        ),
        Command::Scan(true) => call(
            adapter()
                .await?
                .call_method("StartDiscovery", &())
                .await
                .map(drop),
            "scan",
        ),
        Command::Scan(false) => {
            // Fails harmlessly when nothing is scanning.
            let _ = adapter().await?.call_method("StopDiscovery", &()).await;
            Ok(())
        }
        Command::Connect(p) => call(
            device(&p)
                .await?
                .call_method("Connect", &())
                .await
                .map(drop),
            "connect",
        ),
        Command::Disconnect(p) => call(
            device(&p)
                .await?
                .call_method("Disconnect", &())
                .await
                .map(drop),
            "disconnect",
        ),
        Command::Trust(p, on) => call(
            device(&p)
                .await?
                .set_property("Trusted", on)
                .await
                .map_err(Into::into),
            "auto connect",
        ),
        Command::Pair(p) => {
            let d = device(&p).await?;
            call(d.call_method("Pair", &()).await.map(drop), "pair")?;
            call(
                d.set_property("Trusted", true).await.map_err(Into::into),
                "trust",
            )?;
            call(d.call_method("Connect", &()).await.map(drop), "connect")
        }
        Command::Remove(p) => {
            let path = OwnedObjectPath::try_from(p.as_str()).map_err(|e| e.to_string())?;
            call(
                adapter()
                    .await?
                    .call_method("RemoveDevice", &(path,))
                    .await
                    .map(drop),
                "forget",
            )
        }
        Command::Profile(address, profile) => {
            let card = format!("bluez_card.{}", address.replace(':', "_"));
            let out = tokio::process::Command::new("pactl")
                .args(["set-card-profile", &card, &profile])
                .output()
                .await
                .map_err(|e| format!("cannot run pactl: {e}"))?;
            if out.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "profile: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }
    }
}

/// The headset's PipeWire card profiles, from `pactl list cards`.
async fn profiles(address: &str) -> Option<Profiles> {
    let out = tokio::process::Command::new("pactl")
        .args(["list", "cards"])
        .output()
        .await
        .ok()?;
    parse_profiles(&String::from_utf8_lossy(&out.stdout), address)
}

fn parse_profiles(text: &str, address: &str) -> Option<Profiles> {
    let name = format!("Name: bluez_card.{}", address.replace(':', "_"));
    let block = text
        .split("\nCard #")
        .find(|b| b.lines().any(|l| l.trim() == name))?;
    let mut p = Profiles::default();
    let mut in_profiles = false;
    for line in block.lines() {
        let t = line.trim();
        if let Some(active) = t.strip_prefix("Active Profile: ") {
            p.active = active.to_owned();
        }
        if t == "Profiles:" {
            in_profiles = true;
            continue;
        }
        if in_profiles {
            // Profile lines are indented deeper than the section headers.
            if !line.starts_with("\t\t") {
                in_profiles = false;
                continue;
            }
            let Some((id, rest)) = t.split_once(": ") else {
                continue;
            };
            if rest.contains("available: no") {
                continue;
            }
            if id.starts_with("a2dp") && p.audio.is_none() {
                p.audio = Some(id.to_owned());
            } else if id.starts_with("headset") && p.call.is_none() {
                p.call = Some(id.to_owned());
            }
        }
    }
    if p.active.starts_with("a2dp") {
        p.audio = Some(p.active.clone());
    } else if p.active.starts_with("headset") {
        p.call = Some(p.active.clone());
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    #[test]
    fn agent_shows_codes() {
        let agent = Agent {
            code: Arc::new(Mutex::new(None)),
            changed: Arc::new(Notify::new()),
        };
        let dev = OwnedObjectPath::try_from("/org/bluez/hci0/dev_1").unwrap();
        agent.display_passkey(dev.clone(), 4321, 0);
        assert_eq!(
            agent.code.lock().unwrap().as_deref(),
            Some("004321"),
            "keyboards need the leading zeros"
        );
        agent.cancel();
        assert!(agent.code.lock().unwrap().is_none());
        assert_eq!(agent.request_pin_code(dev).unwrap(), "0000");
    }

    #[test]
    fn parses_pactl_cards() {
        let text = "Card #44\n\tName: alsa_card.pci\n\tProfiles:\n\t\toff: Off (sinks: 0, available: yes)\n\tActive Profile: off\n\
                    Card #60\n\tName: bluez_card.34_09_C9_A5_6B_2A\n\tDriver: module-bluez5-device.c\n\tProfiles:\n\
                    \t\ta2dp-sink-sbc: High Fidelity Playback (A2DP Sink, codec SBC) (sinks: 1, sources: 0, priority: 18, available: yes)\n\
                    \t\ta2dp-sink-aac: High Fidelity Playback (A2DP Sink, codec AAC) (sinks: 1, sources: 0, priority: 19, available: no)\n\
                    \t\theadset-head-unit: Headset Head Unit (HSP/HFP) (sinks: 1, sources: 1, priority: 1, available: yes)\n\
                    \t\toff: Off (sinks: 0, sources: 0, priority: 0, available: yes)\n\tActive Profile: a2dp-sink-sbc\n\tPorts:\n";
        let p = parse_profiles(text, "34:09:C9:A5:6B:2A").unwrap();
        assert_eq!(p.active, "a2dp-sink-sbc");
        assert_eq!(p.audio.as_deref(), Some("a2dp-sink-sbc"));
        assert_eq!(p.call.as_deref(), Some("headset-head-unit"));
        assert!(parse_profiles(text, "00:00:00:00:00:00").is_none());
        let call = text.replace(
            "Active Profile: a2dp-sink-sbc",
            "Active Profile: headset-head-unit-msbc",
        );
        let p = parse_profiles(&call, "34:09:C9:A5:6B:2A").unwrap();
        assert_eq!(p.call.as_deref(), Some("headset-head-unit-msbc"));
        assert_eq!(p.audio.as_deref(), Some("a2dp-sink-sbc"));
    }

    fn props(pairs: &[(&str, Value<'static>)]) -> HashMap<String, OwnedValue> {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_owned(),
                    OwnedValue::try_from(v.try_clone().unwrap()).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn builds_state_from_objects() {
        let mut objects: Objects = HashMap::new();
        let path = |p: &str| OwnedObjectPath::try_from(p).unwrap();
        objects.insert(
            path("/org/bluez/hci0"),
            HashMap::from([(
                "org.bluez.Adapter1".into(),
                props(&[("Powered", true.into())]),
            )]),
        );
        let dev = |name: &str, paired: bool, connected: bool, rssi: Option<i16>| {
            let mut p = vec![
                ("Address", Value::from("AA:BB")),
                ("Alias", Value::from(name.to_owned())),
                ("Paired", paired.into()),
                ("Connected", connected.into()),
            ];
            if let Some(r) = rssi {
                p.push(("RSSI", r.into()));
            }
            HashMap::from([("org.bluez.Device1".to_owned(), props(&p))])
        };
        objects.insert(
            path("/org/bluez/hci0/dev_1"),
            dev("Speaker", false, false, Some(-70)),
        );
        objects.insert(
            path("/org/bluez/hci0/dev_2"),
            dev("Headset", true, true, None),
        );
        objects.insert(
            path("/org/bluez/hci0/dev_3"),
            dev("AA:BB", false, false, Some(-40)),
        );
        let mut buds = dev("Buds", true, false, None);
        buds.insert(
            "org.bluez.Battery1".into(),
            props(&[("Percentage", 80u8.into())]),
        );
        objects.insert(path("/org/bluez/hci0/dev_4"), buds);
        let s = from_objects(&objects);
        assert!(s.available && s.powered && !s.discovering);
        let names: Vec<&str> = s.devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["Headset", "Buds", "Speaker"],
            "connected, paired, then found; nameless dropped"
        );
        assert_eq!(s.devices[1].battery, Some(80));
        assert!(!from_objects(&HashMap::new()).available);
    }
}
