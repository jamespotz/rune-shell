//! Bluetooth device icons.

/// Material Symbols icon for a BlueZ device icon name.
pub fn device_icon(bluez_icon: &str) -> &'static str {
    match bluez_icon {
        "audio-headset" | "audio-headphones" => "headphones",
        "audio-card" | "audio-speakers" => "speaker",
        "input-mouse" => "mouse",
        "input-keyboard" => "keyboard",
        "input-gaming" => "sports_esports",
        "input-tablet" => "stylus",
        "phone" => "smartphone",
        "computer" => "computer",
        "camera-photo" | "camera-video" => "photo_camera",
        "printer" => "print",
        _ => "bluetooth",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_device_icons() {
        assert_eq!(super::device_icon("audio-headset"), "headphones");
        assert_eq!(super::device_icon("something-new"), "bluetooth");
    }
}
