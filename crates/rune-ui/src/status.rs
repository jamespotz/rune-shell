//! Status icons and OSD data.
use rune_services::{
    audio::Volume,
    system::{Battery, NetKind, Network},
};

/// What the OSD shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Osd {
    Volume(Volume),
    Brightness(u8),
}

pub const OSD_W: u32 = 240;
pub const OSD_H: u32 = 48;

pub fn volume_icon(v: Volume) -> &'static str {
    match (v.muted, v.percent) {
        (true, _) | (false, 0) => "volume_off",
        (false, 1..=33) => "volume_mute",
        (false, 34..=66) => "volume_down",
        _ => "volume_up",
    }
}

pub(crate) fn network_icon(n: &Network) -> &'static str {
    match (n.connected, n.kind, n.strength) {
        (false, _, _) => "signal_wifi_off",
        (true, NetKind::Wired, _) => "lan",
        (true, NetKind::Wifi, Some(s)) if !n.online => {
            if s > 0 {
                "signal_wifi_statusbar_not_connected"
            } else {
                "signal_wifi_off"
            }
        }
        (true, NetKind::Wifi, s) => wifi_icon(s.unwrap_or(100)),
        (true, NetKind::Other, _) => "lan",
    }
}

/// Wi-Fi bars for a signal strength, 0-100.
pub fn wifi_icon(strength: u8) -> &'static str {
    match strength {
        0..=24 => "network_wifi_1_bar",
        25..=49 => "network_wifi_2_bar",
        50..=74 => "network_wifi_3_bar",
        _ => "signal_wifi_4_bar",
    }
}

/// The peripheral a device battery widget shows: its saved key, or the first one when unset.
pub(crate) fn device_for<'a>(
    peripherals: &'a [rune_services::system::Peripheral],
    key: &str,
) -> Option<&'a rune_services::system::Peripheral> {
    if key.is_empty() {
        peripherals.first()
    } else {
        peripherals.iter().find(|p| p.key == key)
    }
}

pub(crate) fn battery_icon(b: &Battery) -> &'static str {
    if b.charging || b.full {
        return "battery_charging_full";
    }
    const LEVELS: [&str; 8] = [
        "battery_0_bar",
        "battery_1_bar",
        "battery_2_bar",
        "battery_3_bar",
        "battery_4_bar",
        "battery_5_bar",
        "battery_6_bar",
        "battery_full",
    ];
    LEVELS[((b.percent / 100.0 * 7.0).round() as usize).min(7)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_icons() {
        assert_eq!(
            volume_icon(Volume {
                percent: 80,
                muted: true
            }),
            "volume_off"
        );
        assert_eq!(
            volume_icon(Volume {
                percent: 50,
                muted: false
            }),
            "volume_down"
        );
        let wifi = |s| Network {
            connected: true,
            online: true,
            kind: NetKind::Wifi,
            name: String::new(),
            strength: Some(s),
            wifi_enabled: true,
        };
        assert_eq!(network_icon(&wifi(66)), "network_wifi_3_bar");
        assert_eq!(network_icon(&wifi(90)), "signal_wifi_4_bar");
        let bat = |p, charging| Battery {
            percent: p,
            charging,
            full: false,
            time_left: None,
        };
        assert_eq!(battery_icon(&bat(100.0, false)), "battery_full");
        assert_eq!(battery_icon(&bat(3.0, false)), "battery_0_bar");
        assert_eq!(battery_icon(&bat(50.0, true)), "battery_charging_full");
    }

    #[test]
    fn device_battery_hidden_when_absent() {
        use rune_services::system::{Peripheral, PeripheralKind};
        let mouse = Peripheral {
            key: "AA".into(),
            name: "MX".into(),
            kind: PeripheralKind::Mouse,
            percent: 40.0,
            charging: false,
        };
        let list = [mouse.clone()];
        assert_eq!(device_for(&list, ""), Some(&mouse));
        assert_eq!(device_for(&list, "AA"), Some(&mouse));
        assert_eq!(device_for(&list, "BB"), None);
        assert_eq!(device_for(&[], ""), None);
    }
}
