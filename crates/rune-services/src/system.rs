//! Network (NetworkManager) and battery (UPower), both over the system bus.
//! Their property-change signals trigger a re-read; nothing polls.

use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::watch;
use tracing::info;
use zbus::{Connection, MessageStream, Proxy, zvariant::OwnedObjectPath};

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
