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

/// What a saved NetworkManager profile connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKind {
    Wifi,
    Wired,
    Vpn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Activating,
    Active,
}

/// A saved NetworkManager profile (Wi-Fi, wired, or VPN).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedProfile {
    /// `connection.id`.
    pub name: String,
    /// Settings object path.
    pub profile: String,
    pub kind: ProfileKind,
    /// Wi-Fi network name.
    pub ssid: Option<String>,
    /// VPN flavour, e.g. `WireGuard` or `OpenVPN`.
    pub vpn_type: Option<&'static str>,
    /// NetworkManager connects it by itself (`connection.autoconnect`).
    pub auto_connect: bool,
    /// Active connection path and state while it is up or coming up.
    pub active: Option<(String, LinkState)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Wifi,
    Ethernet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Ethernet without a cable, or a radio that is off.
    Unavailable,
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

impl DeviceState {
    /// NetworkManager's `NMDeviceState`; `None` for unmanaged and unknown devices.
    fn from_nm(state: u32) -> Option<Self> {
        match state {
            20 => Some(Self::Unavailable),
            30 | 120 => Some(Self::Disconnected),
            40..=90 => Some(Self::Connecting),
            100 => Some(Self::Connected),
            110 => Some(Self::Disconnecting),
            _ => None,
        }
    }
}

/// A Wi-Fi or Ethernet device NetworkManager manages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetDevice {
    pub path: String,
    /// Kernel interface, e.g. `wlan0` or `enp3s0`.
    pub interface: String,
    pub kind: DeviceKind,
    pub state: DeviceState,
    /// Name of the profile it is using.
    pub connection: Option<String>,
    pub link: LinkDetails,
}

/// A device's addresses and link.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkDetails {
    /// IPv4 address with its prefix, e.g. `192.168.1.20/24`.
    pub ip: Option<String>,
    pub router: Option<String>,
    pub dns: Vec<String>,
    pub speed_mbps: Option<u32>,
    pub mac: Option<String>,
    /// Wi-Fi only.
    pub frequency_mhz: Option<u32>,
    /// Wi-Fi only.
    pub security: Option<&'static str>,
}

impl LinkDetails {
    pub fn band(&self) -> Option<&'static str> {
        match self.frequency_mhz? {
            2400..=2500 => Some("2.4 GHz"),
            4900..=5900 => Some("5 GHz"),
            5925..=7125 => Some("6 GHz"),
            _ => None,
        }
    }
}

/// Everything the Network page shows, read in one pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkList {
    pub devices: Vec<NetDevice>,
    pub nearby: Vec<WifiNetwork>,
    pub profiles: Vec<SavedProfile>,
}

const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const ACTIVE_CONNECTION: &str = "org.freedesktop.NetworkManager.Connection.Active";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const NM_SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";
const SETTINGS_CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const MAX_PROFILES: usize = 256;
const MAX_DEVICES: usize = 32;
const MAX_DNS: usize = 8;
pub const MAX_SSID_BYTES: usize = 32;
/// WPA passphrases are 8–63 characters; 64 hex digits are a raw key.
pub const PSK_LENGTHS: std::ops::RangeInclusive<usize> = 8..=64;

/// Devices, nearby Wi-Fi networks, and saved profiles.
pub async fn network_list(conn: &Connection) -> zbus::Result<NetworkList> {
    let profiles = saved_profiles(conn).await.unwrap_or_default();
    let saved = profiles
        .iter()
        .filter_map(|p| Some((p.ssid.as_ref()?.as_bytes().to_vec(), p.profile.clone())))
        .collect();
    Ok(NetworkList {
        devices: net_devices(conn).await?,
        nearby: wifi_networks(conn, &saved).await?,
        profiles,
    })
}

/// Networks in range, strongest first. Uses NetworkManager's last scan.
async fn wifi_networks(
    conn: &Connection,
    saved: &HashMap<Vec<u8>, String>,
) -> zbus::Result<Vec<WifiNetwork>> {
    let mut nets: Vec<WifiNetwork> = Vec::new();
    for dev in wifi_devices(conn).await? {
        let w = Proxy::new(conn, NM, dev.as_str(), WIRELESS).await?;
        let active: OwnedObjectPath = w.get_property("ActiveAccessPoint").await?;
        let aps: Vec<OwnedObjectPath> = w.call("GetAllAccessPoints", &()).await?;
        for ap in aps {
            let p = Proxy::new(conn, NM, ap.as_str(), ACCESS_POINT).await?;
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
        let d = Proxy::new(conn, NM, dev.as_str(), DEVICE).await?;
        if d.get_property::<u32>("DeviceType").await.unwrap_or(0) == 2 {
            out.push(dev);
        }
    }
    Ok(out)
}

/// Saved Wi-Fi, wired and VPN profiles, sorted by name.
async fn saved_profiles(conn: &Connection) -> zbus::Result<Vec<SavedProfile>> {
    let settings = Proxy::new(
        conn,
        NM,
        NM_SETTINGS,
        "org.freedesktop.NetworkManager.Settings",
    )
    .await?;
    let active = active_profiles(conn).await.unwrap_or_default();
    let mut out = Vec::new();
    for path in settings
        .call::<_, _, Vec<OwnedObjectPath>>("ListConnections", &())
        .await?
        .into_iter()
        .take(MAX_PROFILES)
    {
        let c = Proxy::new(conn, NM, path.as_str(), SETTINGS_CONNECTION).await?;
        let s: HashMap<String, HashMap<String, OwnedValue>> = c.call("GetSettings", &()).await?;
        let Some(connection) = s.get("connection") else {
            continue;
        };
        let kind_name = dict_str(connection, "type").unwrap_or_default();
        let (kind, ssid, vpn_type) = match kind_name.as_str() {
            "802-11-wireless" => {
                let Some(ssid) = s
                    .get("802-11-wireless")
                    .and_then(|w| w.get("ssid"))
                    .and_then(|v| Vec::<u8>::try_from(v.try_clone().ok()?).ok())
                    .filter(|ssid| !ssid.is_empty() && ssid.len() <= MAX_SSID_BYTES)
                else {
                    continue;
                };
                let ssid = String::from_utf8_lossy(&ssid).into_owned();
                (ProfileKind::Wifi, Some(ssid), None)
            }
            "802-3-ethernet" => (ProfileKind::Wired, None, None),
            "wireguard" => (ProfileKind::Vpn, None, Some("WireGuard")),
            "vpn" => {
                let service = s
                    .get("vpn")
                    .and_then(|v| dict_str(v, "service-type"))
                    .unwrap_or_default();
                (ProfileKind::Vpn, None, Some(vpn_label(&service)))
            }
            _ => continue,
        };
        let auto_connect = connection
            .get("autoconnect")
            .and_then(|v| bool::try_from(v.try_clone().ok()?).ok())
            .unwrap_or(true);
        out.push(SavedProfile {
            name: dict_str(connection, "id").unwrap_or_else(|| ssid.clone().unwrap_or_default()),
            active: active.get(path.as_str()).cloned(),
            profile: path.to_string(),
            kind,
            ssid,
            vpn_type,
            auto_connect,
        });
    }
    out.sort_by_cached_key(|p| p.name.to_lowercase());
    Ok(out)
}

/// Active connections by the settings profile they run, with their state.
async fn active_profiles(conn: &Connection) -> zbus::Result<HashMap<String, (String, LinkState)>> {
    let nm = Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM).await?;
    let paths: Vec<OwnedObjectPath> = nm.get_property("ActiveConnections").await?;
    let mut out = HashMap::new();
    for path in paths.into_iter().take(MAX_PROFILES) {
        let active = Proxy::new(conn, NM, path.as_str(), ACTIVE_CONNECTION).await?;
        let state = match active.get_property::<u32>("State").await.unwrap_or(0) {
            1 => LinkState::Activating,
            2 => LinkState::Active,
            _ => continue,
        };
        let Ok(profile) = active.get_property::<OwnedObjectPath>("Connection").await else {
            continue;
        };
        out.insert(profile.to_string(), (path.to_string(), state));
    }
    Ok(out)
}

/// A readable name for a NetworkManager VPN plugin's `service-type`.
fn vpn_label(service: &str) -> &'static str {
    let plugin = service.rsplit('.').next().unwrap_or_default();
    match plugin {
        "openvpn" => "OpenVPN",
        "wireguard" => "WireGuard",
        "openconnect" => "OpenConnect",
        "vpnc" => "Cisco VPN",
        "l2tp" => "L2TP",
        "pptp" => "PPTP",
        "strongswan" | "libreswan" => "IPsec",
        "fortisslvpn" => "Fortinet SSL VPN",
        "sstp" => "SSTP",
        _ => "VPN",
    }
}

/// The access point security, from its `Flags`, `WpaFlags` and `RsnFlags`.
fn security_label(flags: u32, wpa: u32, rsn: u32) -> &'static str {
    const PSK: u32 = 0x100;
    const EAP: u32 = 0x200 | 0x2000;
    const SAE: u32 = 0x400;
    const OWE: u32 = 0x800 | 0x1000;
    if (wpa | rsn) & EAP != 0 {
        "WPA Enterprise"
    } else if rsn & SAE != 0 && rsn & PSK != 0 {
        "WPA2/WPA3 Personal"
    } else if rsn & SAE != 0 {
        "WPA3 Personal"
    } else if rsn & PSK != 0 && wpa & PSK != 0 {
        "WPA/WPA2 Personal"
    } else if rsn & PSK != 0 {
        "WPA2 Personal"
    } else if wpa & PSK != 0 {
        "WPA Personal"
    } else if rsn & OWE != 0 {
        "Enhanced Open"
    } else if flags & 1 != 0 {
        "WEP"
    } else {
        "None"
    }
}

fn dict_str(entry: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    String::try_from(entry.get(key)?.try_clone().ok()?).ok()
}

/// Managed Wi-Fi and Ethernet devices with their link details.
async fn net_devices(conn: &Connection) -> zbus::Result<Vec<NetDevice>> {
    let nm = Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM).await?;
    let mut out = Vec::new();
    for path in nm
        .call::<_, _, Vec<OwnedObjectPath>>("GetDevices", &())
        .await?
        .into_iter()
        .take(MAX_DEVICES)
    {
        let device = Proxy::new(conn, NM, path.as_str(), DEVICE).await?;
        let kind = match device.get_property::<u32>("DeviceType").await.unwrap_or(0) {
            1 => DeviceKind::Ethernet,
            2 => DeviceKind::Wifi,
            _ => continue,
        };
        let Some(state) = DeviceState::from_nm(device.get_property("State").await.unwrap_or(0))
        else {
            continue;
        };
        let active: OwnedObjectPath = device
            .get_property("ActiveConnection")
            .await
            .unwrap_or_default();
        let connection = match active.as_str() {
            "" | "/" => None,
            active => Proxy::new(conn, NM, active, ACTIVE_CONNECTION)
                .await?
                .get_property::<String>("Id")
                .await
                .ok(),
        };
        let mut link = LinkDetails {
            mac: device
                .get_property::<String>("HwAddress")
                .await
                .ok()
                .filter(|m| !m.is_empty()),
            ..LinkDetails::default()
        };
        if state == DeviceState::Connected {
            read_link(conn, &device, kind, path.as_str(), &mut link).await?;
        }
        out.push(NetDevice {
            path: path.to_string(),
            interface: device.get_property("Interface").await.unwrap_or_default(),
            kind,
            state,
            connection,
            link,
        });
    }
    out.sort_by_key(|d| (d.kind == DeviceKind::Ethernet, d.interface.clone()));
    Ok(out)
}

/// Fills in a connected device's addresses, speed and, for Wi-Fi, band and security.
async fn read_link(
    conn: &Connection,
    device: &Proxy<'_>,
    kind: DeviceKind,
    path: &str,
    link: &mut LinkDetails,
) -> zbus::Result<()> {
    match kind {
        DeviceKind::Ethernet => {
            let wired = Proxy::new(
                conn,
                NM,
                path,
                "org.freedesktop.NetworkManager.Device.Wired",
            )
            .await?;
            link.speed_mbps = wired
                .get_property::<u32>("Speed")
                .await
                .ok()
                .filter(|s| *s > 0);
        }
        DeviceKind::Wifi => {
            let wireless = Proxy::new(conn, NM, path, WIRELESS).await?;
            let bitrate: u32 = wireless.get_property("Bitrate").await.unwrap_or(0);
            link.speed_mbps = Some(bitrate / 1000).filter(|m| *m > 0);
            let ap: OwnedObjectPath = wireless.get_property("ActiveAccessPoint").await?;
            if ap.as_str() != "/" {
                let ap = Proxy::new(conn, NM, ap.as_str(), ACCESS_POINT).await?;
                let flag = |name: &'static str| {
                    let ap = ap.clone();
                    async move { ap.get_property::<u32>(name).await.unwrap_or(0) }
                };
                link.frequency_mhz = ap
                    .get_property::<u32>("Frequency")
                    .await
                    .ok()
                    .filter(|f| *f > 0);
                link.security = Some(security_label(
                    flag("Flags").await,
                    flag("WpaFlags").await,
                    flag("RsnFlags").await,
                ));
            }
        }
    }
    let ip4: OwnedObjectPath = device.get_property("Ip4Config").await?;
    if ip4.as_str() == "/" {
        return Ok(());
    }
    let config = Proxy::new(
        conn,
        NM,
        ip4.as_str(),
        "org.freedesktop.NetworkManager.IP4Config",
    )
    .await?;
    let addresses: Vec<HashMap<String, OwnedValue>> =
        config.get_property("AddressData").await.unwrap_or_default();
    link.ip = addresses.first().and_then(|a| {
        let prefix = u32::try_from(a.get("prefix")?.try_clone().ok()?).ok()?;
        Some(format!("{}/{prefix}", dict_str(a, "address")?))
    });
    link.router = config
        .get_property::<String>("Gateway")
        .await
        .ok()
        .filter(|g| !g.is_empty());
    let servers: Vec<HashMap<String, OwnedValue>> = config
        .get_property("NameserverData")
        .await
        .unwrap_or_default();
    link.dns = servers
        .iter()
        .filter_map(|server| dict_str(server, "address"))
        .take(MAX_DNS)
        .collect();
    Ok(())
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
                forget_profile(conn, old).await.map_err(e)?; // replacing a wrong password
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
    let failure = if net.secured {
        format!("Couldn't join {}. Check the password.", net.ssid)
    } else {
        format!("Couldn't join {}.", net.ssid)
    };
    finish_activation(conn, &active, created, failure).await
}

/// Why a hidden network's name or password can't be used, if either can't.
pub fn check_hidden(ssid: &str, password: Option<&str>) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > MAX_SSID_BYTES {
        return Err(format!(
            "Network names are 1 to {MAX_SSID_BYTES} bytes; this one is {}.",
            ssid.len()
        ));
    }
    if password.is_some_and(|pw| !PSK_LENGTHS.contains(&pw.len())) {
        return Err("Wi-Fi passwords are 8 to 64 characters.".into());
    }
    Ok(())
}

/// Joins a network that doesn't broadcast its name, saving a new profile for it.
pub async fn wifi_connect_hidden(
    conn: &Connection,
    ssid: &str,
    password: Option<&str>,
) -> Result<(), String> {
    check_hidden(ssid, password)?;
    let e = |e: zbus::Error| e.to_string();
    let dev = wifi_devices(conn)
        .await
        .map_err(e)?
        .into_iter()
        .next()
        .ok_or("No Wi-Fi device.")?;
    let nm = Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM)
        .await
        .map_err(e)?;
    let mut settings: HashMap<&str, HashMap<&str, Value>> = HashMap::from([
        (
            "connection",
            HashMap::from([
                ("id", Value::from(ssid)),
                ("type", Value::from("802-11-wireless")),
            ]),
        ),
        (
            "802-11-wireless",
            HashMap::from([
                ("ssid", Value::from(ssid.as_bytes().to_vec())),
                ("hidden", Value::from(true)),
                ("mode", Value::from("infrastructure")),
            ]),
        ),
    ]);
    if let Some(pw) = password {
        settings.insert(
            "802-11-wireless-security",
            HashMap::from([
                ("key-mgmt", Value::from("wpa-psk")),
                ("psk", Value::from(pw)),
            ]),
        );
    }
    let any = ObjectPath::try_from("/").map_err(|x| x.to_string())?;
    let (profile, active): (OwnedObjectPath, OwnedObjectPath) = nm
        .call("AddAndActivateConnection", &(settings, dev, any))
        .await
        .map_err(e)?;
    let failure = if password.is_some() {
        format!("Couldn't join {ssid}. Check the name and password.")
    } else {
        format!("Couldn't join {ssid}. Check the name.")
    };
    finish_activation(conn, &active, Some(profile), failure).await
}

/// Waits for an activation; a profile created for it that fails is deleted again.
async fn finish_activation(
    conn: &Connection,
    active: &OwnedObjectPath,
    created: Option<OwnedObjectPath>,
    failure: String,
) -> Result<(), String> {
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
        let _ = forget_profile(conn, profile.as_str()).await;
    }
    Err(failure)
}

/// Drops the device's connection; NetworkManager won't reconnect it until asked.
pub async fn device_disconnect(conn: &Connection, device: &str) -> zbus::Result<()> {
    Proxy::new(conn, NM, device, DEVICE)
        .await?
        .call_method("Disconnect", &())
        .await
        .map(drop)
}

/// Brings up a saved profile, on `device` or wherever NetworkManager picks (VPNs).
pub async fn activate_profile(
    conn: &Connection,
    profile: &str,
    device: Option<&str>,
    name: &str,
) -> Result<(), String> {
    let e = |e: zbus::Error| e.to_string();
    let path = |p: &str| ObjectPath::try_from(p.to_owned()).map_err(|x| format!("{p}: {x}"));
    let nm = Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM)
        .await
        .map_err(e)?;
    let active: OwnedObjectPath = nm
        .call(
            "ActivateConnection",
            &(path(profile)?, path(device.unwrap_or("/"))?, path("/")?),
        )
        .await
        .map_err(e)?;
    finish_activation(conn, &active, None, format!("Couldn't connect {name}.")).await
}

/// Takes down an active connection, e.g. a VPN.
pub async fn deactivate(conn: &Connection, active: &str) -> zbus::Result<()> {
    let active = ObjectPath::try_from(active)?;
    Proxy::new(conn, NM, "/org/freedesktop/NetworkManager", NM)
        .await?
        .call_method("DeactivateConnection", &(active,))
        .await
        .map(drop)
}

/// Turns `connection.autoconnect` on or off for a saved profile. NetworkManager's
/// `Update` replaces whole profiles; nmcli merges stored secrets and WireGuard peers back.
pub async fn set_auto_connect(profile: &str, on: bool) -> Result<(), String> {
    if !profile
        .strip_prefix(NM_SETTINGS)
        .is_some_and(|id| id.len() > 1 && id.starts_with('/'))
    {
        return Err(format!("not a NetworkManager profile: {profile}"));
    }
    let value = if on { "yes" } else { "no" };
    let command = format!("nmcli connection modify path {profile} connection.autoconnect {value}");
    let output = tokio::process::Command::new("nmcli")
        .args(["connection", "modify", "path", profile])
        .args(["connection.autoconnect", value])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("{command}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "{command}: {}: {}",
        output.status,
        stderr.trim().chars().take(200).collect::<String>()
    ))
}

/// Deletes a saved profile.
pub async fn forget_profile(conn: &Connection, profile: &str) -> zbus::Result<()> {
    Proxy::new(conn, NM, profile, SETTINGS_CONNECTION)
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
    fn labels_access_point_security() {
        assert_eq!(security_label(1, 0, 0x100 | 0x400), "WPA2/WPA3 Personal");
        assert_eq!(security_label(1, 0, 0x400), "WPA3 Personal");
        assert_eq!(security_label(1, 0x100, 0x100), "WPA/WPA2 Personal");
        assert_eq!(security_label(1, 0, 0x200), "WPA Enterprise");
        assert_eq!(security_label(0, 0, 0x800), "Enhanced Open");
        assert_eq!(security_label(1, 0, 0), "WEP");
        assert_eq!(security_label(0, 0, 0), "None");
    }

    #[test]
    fn names_frequency_bands() {
        let band = |mhz| {
            LinkDetails {
                frequency_mhz: mhz,
                ..LinkDetails::default()
            }
            .band()
        };
        assert_eq!(band(Some(2437)), Some("2.4 GHz"));
        assert_eq!(band(Some(5180)), Some("5 GHz"));
        assert_eq!(band(Some(5955)), Some("6 GHz"));
        assert_eq!(band(Some(u32::MAX)), None);
        assert_eq!(band(None), None);
    }

    #[test]
    fn maps_device_states() {
        assert_eq!(DeviceState::from_nm(10), None);
        assert_eq!(DeviceState::from_nm(20), Some(DeviceState::Unavailable));
        assert_eq!(DeviceState::from_nm(30), Some(DeviceState::Disconnected));
        assert_eq!(DeviceState::from_nm(70), Some(DeviceState::Connecting));
        assert_eq!(DeviceState::from_nm(100), Some(DeviceState::Connected));
        assert_eq!(DeviceState::from_nm(110), Some(DeviceState::Disconnecting));
        assert_eq!(DeviceState::from_nm(120), Some(DeviceState::Disconnected));
        assert_eq!(DeviceState::from_nm(u32::MAX), None);
    }

    #[test]
    fn names_vpn_plugins() {
        assert_eq!(
            vpn_label("org.freedesktop.NetworkManager.openvpn"),
            "OpenVPN"
        );
        assert_eq!(
            vpn_label("org.freedesktop.NetworkManager.strongswan"),
            "IPsec"
        );
        assert_eq!(vpn_label("com.example.unknown"), "VPN");
        assert_eq!(vpn_label(""), "VPN");
    }

    #[tokio::test]
    async fn auto_connect_refuses_paths_outside_networkmanager_settings() {
        for profile in [
            "--help",
            "/org/freedesktop/NetworkManager/Settings",
            "/tmp/x",
            "",
        ] {
            let err = set_auto_connect(profile, true).await.unwrap_err();
            assert!(err.starts_with("not a NetworkManager profile"), "{err}");
        }
    }

    #[test]
    fn hidden_networks_need_a_valid_name_and_password() {
        let long = "x".repeat(MAX_SSID_BYTES + 1);
        assert!(check_hidden("", None).is_err());
        assert!(check_hidden(&long, None).is_err());
        assert!(check_hidden(&"é".repeat(17), None).is_err());
        assert!(check_hidden("cafe", Some("short")).is_err());
        assert!(check_hidden("cafe", Some(&"k".repeat(65))).is_err());
        assert!(check_hidden("cafe", Some("long-enough")).is_ok());
        assert!(check_hidden("cafe", None).is_ok());
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
    /// Join a network that hides its name, with a password when it is secured.
    WifiConnectHidden {
        ssid: String,
        password: Option<String>,
    },
    /// Re-read devices, networks and profiles without scanning.
    NetworkRefresh,
    /// Bring up a saved profile; `device` is `None` for VPNs.
    Activate {
        profile: String,
        device: Option<String>,
        name: String,
    },
    /// Take down an active connection by its path.
    Deactivate(String),
    DeviceDisconnect(String),
    /// Saved profile and whether NetworkManager connects it automatically.
    AutoConnect(String, bool),
    /// Delete a saved profile.
    Forget(String),
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
    Networks(NetworkList),
    /// Result of joining or connecting (empty on success).
    NetworkStatus(String),
    /// Whether it is now on, or why it couldn't start.
    KeepAwake(Result<bool, String>),
    NightLight(Result<bool, String>),
}

/// Carries out [`Command`]s; holds what stays alive while a feature is on.
#[derive(Debug, Clone)]
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
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                if wifi_scan(conn).await.is_ok() {
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                }
                Ok(())
            }
            Command::WifiConnect(net, password) => {
                let conn = bus()?;
                let status = wifi_connect(conn, &net, password.as_deref()).await;
                reply(Reply::NetworkStatus(status.err().unwrap_or_default()));
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                Ok(())
            }
            Command::WifiConnectHidden { ssid, password } => {
                let conn = bus()?;
                let status = wifi_connect_hidden(conn, &ssid, password.as_deref()).await;
                reply(Reply::NetworkStatus(status.err().unwrap_or_default()));
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                Ok(())
            }
            Command::NetworkRefresh => {
                reply(Reply::Networks(network_list(bus()?).await.map_err(err)?));
                Ok(())
            }
            Command::Activate {
                profile,
                device,
                name,
            } => {
                let conn = bus()?;
                let status = activate_profile(conn, &profile, device.as_deref(), &name).await;
                reply(Reply::NetworkStatus(status.err().unwrap_or_default()));
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                Ok(())
            }
            Command::Deactivate(active) => {
                let conn = bus()?;
                let done = deactivate(conn, &active)
                    .await
                    .map_err(|e| format!("deactivate {active}: {e}"));
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                done
            }
            Command::DeviceDisconnect(device) => {
                let conn = bus()?;
                let done = device_disconnect(conn, &device)
                    .await
                    .map_err(|e| format!("disconnect {device}: {e}"));
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                done
            }
            Command::AutoConnect(profile, on) => {
                let conn = bus()?;
                let done = set_auto_connect(&profile, on).await;
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
                done
            }
            Command::Forget(profile) => {
                let conn = bus()?;
                let forgot = forget_profile(conn, &profile).await.map_err(err);
                reply(Reply::Networks(network_list(conn).await.map_err(err)?));
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
                        .map_err(|e| match e.kind() {
                            std::io::ErrorKind::NotFound => "Install hyprsunset".to_owned(),
                            _ => format!("cannot run hyprsunset: {e}"),
                        })
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
