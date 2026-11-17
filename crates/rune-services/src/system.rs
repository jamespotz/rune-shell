//! Network (NetworkManager) and battery (UPower), both over the system bus.
//! Their property-change signals trigger a re-read; nothing polls.

use futures_util::StreamExt;
use std::{collections::HashMap, time::Duration};
use tokio::sync::watch;
use tracing::info;
use zbus::{
    Connection, MessageStream, Proxy,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetKind {
    Wifi,
    Wired,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub connected: bool,
    /// Can reach the internet (NetworkManager "full" connectivity).
    pub online: bool,
    pub kind: NetKind,
    /// Wi-Fi name or the connection's name.
    pub name: String,
    /// Wi-Fi signal, 0-100.
    pub strength: Option<u8>,
    /// The Wi-Fi radio is on.
    pub wifi_enabled: bool,
}

/// Turns the Wi-Fi radio on or off (NetworkManager asks polkit; the active user is normally allowed).
pub async fn set_wifi(conn: &Connection, on: bool) -> zbus::Result<()> {
    Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM)
        .await?
        .set_property("WirelessEnabled", on)
        .await
        .map_err(Into::into)
}

/// A Wi-Fi network in range (the strongest access point for its name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0-100.
    pub strength: u8,
    pub secured: bool,
    pub active: bool,
    /// Saved NetworkManager profile, so no password is needed.
    pub saved: Option<String>,
    pub device: String,
    pub ap: String,
}

const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const NM_SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";

/// Networks in range, strongest first. Uses NetworkManager's last scan.
pub async fn wifi_networks(conn: &Connection) -> zbus::Result<Vec<WifiNetwork>> {
    let saved = saved_wifi(conn).await.unwrap_or_default();
    let mut nets: Vec<WifiNetwork> = Vec::new();
    for dev in wifi_devices(conn).await? {
        let w = Proxy::new(conn, NM, dev.as_str(), WIRELESS).await?;
        let active: OwnedObjectPath = w.get_property("ActiveAccessPoint").await?;
        let aps: Vec<OwnedObjectPath> = w.call("GetAllAccessPoints", &()).await?;
        for ap in aps {
            let p = Proxy::new(
                conn,
                NM,
                ap.as_str(),
                "org.freedesktop.NetworkManager.AccessPoint",
            )
            .await?;
            let ssid: Vec<u8> = p.get_property("Ssid").await.unwrap_or_default();
            if ssid.is_empty() {
                continue; // hidden network
            }
            let flags = |name: &'static str| {
                let p = p.clone();
                async move { p.get_property::<u32>(name).await.unwrap_or(0) }
            };
            let net = WifiNetwork {
                saved: saved.get(&ssid).cloned(),
                ssid: String::from_utf8_lossy(&ssid).into_owned(),
                strength: p.get_property("Strength").await.unwrap_or(0),
                secured: flags("WpaFlags").await | flags("RsnFlags").await | flags("Flags").await
                    != 0,
                active: ap == active,
                device: dev.to_string(),
                ap: ap.to_string(),
            };
            merge_network(&mut nets, net);
        }
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
    Ok(nets)
}

/// Keeps one entry per name: the strongest access point, active if any of them is.
fn merge_network(nets: &mut Vec<WifiNetwork>, net: WifiNetwork) {
    match nets.iter_mut().find(|n| n.ssid == net.ssid) {
        Some(old) => {
            let active = old.active || net.active;
            if net.strength > old.strength || net.active {
                *old = net;
            }
            old.active = active;
        }
        None => nets.push(net),
    }
}

/// Asks the Wi-Fi devices to scan; results show up a few seconds later.
pub async fn wifi_scan(conn: &Connection) -> zbus::Result<()> {
    for dev in wifi_devices(conn).await? {
        let w = Proxy::new(conn, NM, dev.as_str(), WIRELESS).await?;
        // Fails when a scan ran moments ago; the cached list is fine then.
        let _ = w
            .call_method("RequestScan", &(HashMap::<&str, Value>::new(),))
            .await;
    }
    Ok(())
}

async fn wifi_devices(conn: &Connection) -> zbus::Result<Vec<OwnedObjectPath>> {
    let nm = Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM).await?;
    let mut out = Vec::new();
    for dev in nm
        .call::<_, _, Vec<OwnedObjectPath>>("GetDevices", &())
        .await?
    {
        let d = Proxy::new(
            conn,
            NM,
            dev.as_str(),
            "org.freedesktop.NetworkManager.Device",
        )
        .await?;
        if d.get_property::<u32>("DeviceType").await.unwrap_or(0) == 2 {
            out.push(dev);
        }
    }
    Ok(out)
}

/// Saved Wi-Fi profiles by SSID.
async fn saved_wifi(conn: &Connection) -> zbus::Result<HashMap<Vec<u8>, String>> {
    let settings = Proxy::new(
        conn,
        NM,
        NM_SETTINGS,
        "org.freedesktop.NetworkManager.Settings",
    )
    .await?;
    let mut out = HashMap::new();
    for path in settings
        .call::<_, _, Vec<OwnedObjectPath>>("ListConnections", &())
        .await?
    {
        let c = Proxy::new(
            conn,
            NM,
            path.as_str(),
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .await?;
        let s: HashMap<String, HashMap<String, OwnedValue>> = c.call("GetSettings", &()).await?;
        if let Some(ssid) = s
            .get("802-11-wireless")
            .and_then(|w| w.get("ssid"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::<u8>::try_from(v).ok())
        {
            out.insert(ssid, path.to_string());
        }
    }
    Ok(out)
}

/// Joins a network: a saved one by its profile, a new one with `password`.
/// Waits for the result; a new profile that fails to connect is deleted again.
pub async fn wifi_connect(
    conn: &Connection,
    net: &WifiNetwork,
    password: Option<&str>,
) -> Result<(), String> {
    let e = |e: zbus::Error| e.to_string();
    let nm = Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM)
        .await
        .map_err(e)?;
    let dev = ObjectPath::try_from(net.device.as_str()).map_err(|x| x.to_string())?;
    let ap = ObjectPath::try_from(net.ap.as_str()).map_err(|x| x.to_string())?;
    let (active, created) = match (&net.saved, password) {
        (Some(saved), None) => {
            let saved = ObjectPath::try_from(saved.as_str()).map_err(|x| x.to_string())?;
            let a: OwnedObjectPath = nm
                .call("ActivateConnection", &(saved, dev, ap))
                .await
                .map_err(e)?;
            (a, None)
        }
        (saved, password) => {
            if let Some(old) = saved {
                wifi_forget(conn, old).await.map_err(e)?; // replacing a wrong password
            }
            // NetworkManager fills in the rest (WPA2/WPA3 key type) from the access point.
            // shortcut: no enterprise (802.1X) or hidden networks; add when someone needs them.
            let mut settings: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
            if let Some(pw) = password.filter(|_| net.secured) {
                settings.insert(
                    "802-11-wireless-security",
                    HashMap::from([("psk", Value::from(pw))]),
                );
            }
            let (profile, a): (OwnedObjectPath, OwnedObjectPath) = nm
                .call("AddAndActivateConnection", &(settings, dev, ap))
                .await
                .map_err(e)?;
            (a, Some(profile))
        }
    };
    // Active connection states: 1 activating, 2 activated, 3-4 going away.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let ok = loop {
        let state = match Proxy::new(
            conn,
            NM,
            active.as_str(),
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .await
        {
            Ok(p) => p.get_property::<u32>("State").await.unwrap_or(4),
            Err(_) => 4,
        };
        if state == 2 || state >= 3 || tokio::time::Instant::now() > deadline {
            break state == 2;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    if ok {
        return Ok(());
    }
    if let Some(profile) = created {
        let _ = wifi_forget(conn, profile.as_str()).await;
    }
    Err(if net.secured {
        format!("Couldn't join {}. Check the password.", net.ssid)
    } else {
        format!("Couldn't join {}.", net.ssid)
    })
}

/// Deletes a saved profile.
pub async fn wifi_forget(conn: &Connection, profile: &str) -> zbus::Result<()> {
    Proxy::new(
        conn,
        NM,
        profile,
        "org.freedesktop.NetworkManager.Settings.Connection",
    )
    .await?
    .call_method("Delete", &())
    .await
    .map(drop)
}

/// Things the power menu can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Lock,
    Suspend,
    Logout,
    Reboot,
    Shutdown,
}

impl PowerAction {
    /// The ones that end the session, so the menu asks twice.
    pub fn needs_confirmation(self) -> bool {
        matches!(
            self,
            PowerAction::Logout | PowerAction::Reboot | PowerAction::Shutdown
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    pub percent: f64,
    pub charging: bool,
    pub full: bool,
    /// Seconds until empty (discharging) or full (charging), when known.
    pub time_left: Option<u64>,
}

const NM: &str = "org.freedesktop.NetworkManager";

/// Watches NetworkManager. `None` when it isn't running.
pub fn spawn_network(conn: Connection) -> watch::Receiver<Option<Network>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let rule = "type='signal',sender='org.freedesktop.NetworkManager',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'";
        let Ok(mut events) = MessageStream::for_match_rule(rule, &conn, Some(64)).await else {
            return;
        };
        loop {
            let net = read_network(&conn).await.ok().flatten();
            tx.send_if_modified(|n| {
                let changed = *n != net;
                if changed {
                    *n = net;
                }
                changed
            });
            if events.next().await.is_none() {
                return;
            }
            // Signal strength updates come in bursts; settle first.
            while let Ok(Some(_)) =
                tokio::time::timeout(Duration::from_millis(500), events.next()).await
            {}
        }
    });
    rx
}

async fn read_network(conn: &Connection) -> zbus::Result<Option<Network>> {
    let nm = Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM).await?;
    let state: u32 = nm.get_property("State").await?;
    let connectivity: u32 = nm.get_property("Connectivity").await.unwrap_or(0);
    let primary: OwnedObjectPath = nm.get_property("PrimaryConnection").await?;
    let mut net = Network {
        connected: state >= 50, // CONNECTED_LOCAL and up
        online: connectivity == 4,
        kind: NetKind::Other,
        name: String::new(),
        strength: None,
        wifi_enabled: nm.get_property("WirelessEnabled").await.unwrap_or(false),
    };
    if primary.as_str() == "/" {
        return Ok(Some(net));
    }
    let active = Proxy::new(
        conn,
        NM,
        primary.as_str(),
        "org.freedesktop.NetworkManager.Connection.Active",
    )
    .await?;
    net.name = active.get_property("Id").await.unwrap_or_default();
    let kind: String = active.get_property("Type").await.unwrap_or_default();
    net.kind = match kind.as_str() {
        "802-11-wireless" => NetKind::Wifi,
        "802-3-ethernet" => NetKind::Wired,
        _ => NetKind::Other,
    };
    if net.kind == NetKind::Wifi {
        let devices: Vec<OwnedObjectPath> =
            active.get_property("Devices").await.unwrap_or_default();
        if let Some(dev) = devices.first() {
            let wifi = Proxy::new(
                conn,
                NM,
                dev.as_str(),
                "org.freedesktop.NetworkManager.Device.Wireless",
            )
            .await?;
            let ap: OwnedObjectPath = wifi.get_property("ActiveAccessPoint").await?;
            if ap.as_str() != "/" {
                let ap = Proxy::new(
                    conn,
                    NM,
                    ap.as_str(),
                    "org.freedesktop.NetworkManager.AccessPoint",
                )
                .await?;
                net.strength = ap.get_property::<u8>("Strength").await.ok();
                let ssid: Vec<u8> = ap.get_property("Ssid").await.unwrap_or_default();
                if !ssid.is_empty() {
                    net.name = String::from_utf8_lossy(&ssid).into_owned();
                }
            }
        }
    }
    Ok(Some(net))
}

const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// power-profiles-daemon state, e.g. `balanced` out of `power-saver`, `balanced`, `performance`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerProfile {
    pub active: String,
    pub available: Vec<String>,
}

impl PowerProfile {
    /// The profile after the active one, wrapping around.
    pub fn next(&self) -> Option<&str> {
        let i = self.available.iter().position(|p| *p == self.active)?;
        self.available
            .get((i + 1) % self.available.len())
            .map(String::as_str)
    }
}

/// Watches power-profiles-daemon. `None` when it isn't running.
pub fn spawn_power_profile(conn: Connection) -> watch::Receiver<Option<PowerProfile>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let rule = format!(
            "type='signal',path='{PROFILES_PATH}',interface='org.freedesktop.DBus.Properties'"
        );
        let Ok(mut events) = MessageStream::for_match_rule(rule.as_str(), &conn, Some(16)).await
        else {
            return;
        };
        loop {
            let p = read_power_profile(&conn).await.ok();
            tx.send_if_modified(|cur| {
                let changed = *cur != p;
                if changed {
                    *cur = p;
                }
                changed
            });
            if events.next().await.is_none() {
                return;
            }
        }
    });
    rx
}

async fn read_power_profile(conn: &Connection) -> zbus::Result<PowerProfile> {
    let p = Proxy::new(conn, PROFILES, PROFILES_PATH, PROFILES).await?;
    let profiles: Vec<HashMap<String, OwnedValue>> = p.get_property("Profiles").await?;
    Ok(PowerProfile {
        active: p.get_property("ActiveProfile").await?,
        available: profiles
            .into_iter()
            .filter_map(|mut m| String::try_from(m.remove("Profile")?).ok())
            .collect(),
    })
}

pub async fn set_power_profile(conn: &Connection, name: &str) -> zbus::Result<()> {
    Proxy::new(conn, PROFILES, PROFILES_PATH, PROFILES)
        .await?
        .set_property("ActiveProfile", name)
        .await
        .map_err(Into::into)
}

/// Blocks idle actions (hypridle's screen-off, lock, suspend) until the returned fd is dropped.
pub async fn inhibit_idle(conn: &Connection) -> zbus::Result<zbus::zvariant::OwnedFd> {
    Proxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?
    .call(
        "Inhibit",
        &("idle", "Rune Shell", "Keep awake is on", "block"),
    )
    .await
}

const UPOWER: &str = "org.freedesktop.UPower";
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

/// Watches the combined battery. `None` on machines without one.
pub fn spawn_battery(conn: Connection) -> watch::Receiver<Option<Battery>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let rule = format!(
            "type='signal',sender='{UPOWER}',path='{DISPLAY}',interface='org.freedesktop.DBus.Properties'"
        );
        let Ok(mut events) = MessageStream::for_match_rule(rule.as_str(), &conn, Some(16)).await
        else {
            return;
        };
        loop {
            let battery = read_battery(&conn).await.ok().flatten();
            if battery.is_none() && tx.borrow().is_none() {
                info!("no battery");
            }
            tx.send_if_modified(|b| {
                let changed = *b != battery;
                if changed {
                    *b = battery;
                }
                changed
            });
            if events.next().await.is_none() {
                return;
            }
        }
    });
    rx
}

async fn read_battery(conn: &Connection) -> zbus::Result<Option<Battery>> {
    let dev = Proxy::new(conn, UPOWER, DISPLAY, "org.freedesktop.UPower.Device").await?;
    if !dev.get_property::<bool>("IsPresent").await? {
        return Ok(None);
    }
    let state: u32 = dev.get_property("State").await?;
    let (charging, full) = (matches!(state, 1 | 5), state == 4);
    let key = if charging {
        "TimeToFull"
    } else {
        "TimeToEmpty"
    };
    let secs: i64 = dev.get_property(key).await.unwrap_or(0);
    Ok(Some(Battery {
        percent: dev.get_property("Percentage").await?,
        charging,
        full,
        time_left: u64::try_from(secs).ok().filter(|s| *s > 0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, strength: u8, active: bool) -> WifiNetwork {
        WifiNetwork {
            ssid: ssid.into(),
            strength,
            secured: true,
            active,
            saved: None,
            device: "/d".into(),
            ap: format!("/ap/{strength}"),
        }
    }

    #[test]
    fn cycles_power_profiles() {
        let mut p = PowerProfile {
            active: "balanced".into(),
            available: vec![
                "power-saver".into(),
                "balanced".into(),
                "performance".into(),
            ],
        };
        assert_eq!(p.next(), Some("performance"));
        p.active = "performance".into();
        assert_eq!(p.next(), Some("power-saver"));
        p.active = "unknown".into();
        assert_eq!(p.next(), None);
    }

    #[test]
    fn merges_access_points_by_name() {
        let mut nets = Vec::new();
        merge_network(&mut nets, net("home", 40, true));
        merge_network(&mut nets, net("home", 80, false));
        merge_network(&mut nets, net("cafe", 50, false));
        assert_eq!(nets.len(), 2);
        assert_eq!((nets[0].strength, nets[0].active), (80, true));
    }
}

/// Requests for the system controls (Wi-Fi, power, idle, night light).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Wifi(bool),
    /// List networks now, scan, and list again once the scan has had time.
    WifiScan,
    WifiConnect(WifiNetwork, Option<String>),
    WifiForget(String),
    PowerProfile(String),
    /// Hold off idle screen-off, lock and suspend.
    KeepAwake(bool),
    /// Warm screen colors (hyprsunset).
    NightLight(bool),
    /// A power-menu action; `Lock` runs `lock_command`.
    Power {
        action: PowerAction,
        lock_command: String,
    },
}

/// What [`Controls::handle`] reports back.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    WifiNetworks(Vec<WifiNetwork>),
    /// Result of joining a network (empty on success).
    WifiStatus(String),
    /// Whether it is now on, or why it couldn't start.
    KeepAwake(Result<bool, String>),
    NightLight(Result<bool, String>),
}

/// Carries out [`Command`]s; holds what stays alive while a feature is on.
#[derive(Clone)]
pub struct Controls {
    conn: Option<Connection>,
    hypr: Option<crate::hyprland::Hyprland>,
    /// Dropping the fd ends the idle inhibitor; the lock keeps toggles in order.
    keep_awake: std::sync::Arc<tokio::sync::Mutex<Option<zbus::zvariant::OwnedFd>>>,
    /// Dropping the child stops hyprsunset, which restores the colors.
    night_light: std::sync::Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>,
}

impl Controls {
    pub fn new(conn: Option<Connection>, hypr: Option<crate::hyprland::Hyprland>) -> Self {
        Self {
            conn,
            hypr,
            keep_awake: Default::default(),
            night_light: Default::default(),
        }
    }

    pub async fn handle(
        &self,
        command: Command,
        reply: impl Fn(Reply) + Send + Sync,
    ) -> Result<(), String> {
        let bus = || self.conn.as_ref().ok_or("no system bus");
        let err = |e: zbus::Error| e.to_string();
        match command {
            Command::Wifi(on) => set_wifi(bus()?, on).await.map_err(err),
            Command::WifiScan => {
                let conn = bus()?;
                reply(Reply::WifiNetworks(wifi_networks(conn).await.map_err(err)?));
                if wifi_scan(conn).await.is_ok() {
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    reply(Reply::WifiNetworks(wifi_networks(conn).await.map_err(err)?));
                }
                Ok(())
            }
            Command::WifiConnect(net, password) => {
                let conn = bus()?;
                let status = wifi_connect(conn, &net, password.as_deref()).await;
                reply(Reply::WifiStatus(status.err().unwrap_or_default()));
                reply(Reply::WifiNetworks(wifi_networks(conn).await.map_err(err)?));
                Ok(())
            }
            Command::WifiForget(profile) => {
                let conn = bus()?;
                let forgot = wifi_forget(conn, &profile).await.map_err(err);
                reply(Reply::WifiNetworks(wifi_networks(conn).await.map_err(err)?));
                forgot
            }
            Command::PowerProfile(name) => set_power_profile(bus()?, &name).await.map_err(err),
            Command::KeepAwake(on) => {
                let mut held = self.keep_awake.lock().await;
                drop(held.take());
                let state = match (&self.conn, on) {
                    (_, false) => Ok(false),
                    (Some(conn), true) => match inhibit_idle(conn).await {
                        Ok(fd) => {
                            *held = Some(fd);
                            Ok(true)
                        }
                        Err(e) => {
                            tracing::warn!("keep awake: {e}");
                            Err("Unavailable".to_owned())
                        }
                    },
                    (None, true) => Err("Unavailable".to_owned()),
                };
                reply(Reply::KeepAwake(state));
                Ok(())
            }
            Command::NightLight(on) => {
                let mut held = self.night_light.lock().await;
                drop(held.take());
                let state = if on {
                    tokio::process::Command::new("hyprsunset")
                        .args(["-t", "4500"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .kill_on_drop(true)
                        .spawn()
                        .map(|child| *held = Some(child))
                        .map(|()| true)
                        .map_err(|_| "Install hyprsunset".to_owned())
                } else {
                    Ok(false)
                };
                reply(Reply::NightLight(state));
                Ok(())
            }
            Command::Power {
                action,
                lock_command,
            } => self.power(action, &lock_command).await,
        }
    }

    async fn power(&self, action: PowerAction, lock_command: &str) -> Result<(), String> {
        let run = |program: &str, args: &[&str]| {
            let mut cmd = tokio::process::Command::new(program);
            cmd.args(args);
            cmd
        };
        let status = match action {
            PowerAction::Lock => run("sh", &["-c", lock_command]).status().await,
            PowerAction::Logout => {
                let hypr = self.hypr.as_ref().ok_or("logging out needs Hyprland")?;
                return hypr
                    .dispatch("hl.dsp.exit()")
                    .await
                    .map_err(|e| e.to_string());
            }
            PowerAction::Suspend => run("systemctl", &["suspend"]).status().await,
            PowerAction::Reboot => run("systemctl", &["reboot"]).status().await,
            PowerAction::Shutdown => run("systemctl", &["poweroff"]).status().await,
        };
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("{action:?}: command exited with {status}")),
            Err(e) => Err(format!("{action:?}: {e}")),
        }
    }
}
