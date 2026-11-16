//! Bluetooth bar widget. Devices are managed in Settings → Bluetooth.

use crate::{
    App, Message,
    notch::{font, icon},
};
use iced::{
    Alignment, Color, Element, border,
    widget::{Space, button, row, text},
};

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

/// Status icon (and how many devices are connected); opens the Bluetooth settings.
pub fn widget(app: &App) -> Element<'_, Message> {
    if !app.bt.available {
        return Space::new().into();
    }
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let connected = app.bt.devices.iter().filter(|d| d.connected).count();
    let glyph = match (app.bt.powered, connected) {
        (false, _) => "bluetooth_disabled",
        (true, 0) => "bluetooth",
        (true, _) => "bluetooth_connected",
    };
    let mut content = row![icon(c, glyph, c.bar.font_size * 1.3)]
        .spacing(2)
        .align_y(Alignment::Center);
    if connected > 1 {
        content = content.push(
            text(connected.to_string())
                .size(c.bar.font_size * 0.85)
                .font(font(&c.bar.font)),
        );
    }
    button(content)
        .padding([2, 4])
        .on_press(Message::OpenBluetooth)
        .style(move |_, s| button::Style {
            background: matches!(s, button::Status::Hovered)
                .then(|| Color { a: 0.12, ..fg }.into()),
            text_color: fg,
            border: border::rounded(8),
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_device_icons() {
        assert_eq!(super::device_icon("audio-headset"), "headphones");
        assert_eq!(super::device_icon("something-new"), "bluetooth");
    }
}
