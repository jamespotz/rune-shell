//! Quick settings and the power menu, both shown in the expanded notch.

use crate::{App, Message, notch::icon};
use iced::{
    Alignment, Color, Element, Length, border,
    widget::{Space, button, column, container, row, slider, text},
};
use rune_services::system::PowerAction;

pub const QUICK_W: f32 = 440.0;
pub const POWER_W: f32 = 520.0;

pub fn panel(app: &App) -> Element<'_, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let accent: Color = c.colors.accent.into();
    let on_accent: Color = c.colors.on_accent.into();
    let muted: Color = c.colors.muted.into();
    let tile = |glyph: &str, label: &str, detail: String, on: bool, msg: Option<Message>| {
        let content = row![
            icon(c, glyph, 22.0),
            column![
                text(label.to_owned()).size(13),
                text(detail).size(11).wrapping(text::Wrapping::None)
            ]
            .spacing(1),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut b = button(content)
            .width(Length::Fill)
            .padding([10, 12])
            .style(move |_, s| button::Style {
                background: Some(
                    if on {
                        accent
                    } else {
                        Color {
                            a: if matches!(s, button::Status::Hovered) {
                                0.14
                            } else {
                                0.08
                            },
                            ..fg
                        }
                    }
                    .into(),
                ),
                text_color: if on { on_accent } else { fg },
                border: border::rounded(14),
                ..Default::default()
            });
        if let Some(m) = msg {
            b = b.on_press(m);
        }
        b
    };
    let wifi_on = app.network.as_ref().is_some_and(|n| n.wifi_enabled);
    let wifi_detail = match &app.network {
        Some(n) if n.connected && !n.name.is_empty() => n.name.clone(),
        Some(n) if n.wifi_enabled => "Not connected".into(),
        Some(_) => "Off".into(),
        None => "Unavailable".into(),
    };
    let bt_detail = match app.bt.devices.iter().filter(|d| d.connected).count() {
        _ if !app.bt.available => "Unavailable".into(),
        _ if !app.bt.powered => "Off".into(),
        0 => "On".into(),
        n => format!("{n} connected"),
    };
    let on_off = |on: bool| if on { "On" } else { "Off" }.to_owned();
    // On, off, or why it couldn't start.
    let held = |r: &Result<bool, String>| match r {
        Ok(on) => (on_off(*on), *on),
        Err(e) => (e.clone(), false),
    };
    let (night_detail, night_on) = held(&app.night_light);
    let (awake_detail, awake_on) = held(&app.keep_awake);
    let mic = app.audio.mic();
    let profile = app.power_profile.as_ref();
    let tiles = [
        tile(
            "wifi",
            "Wi-Fi",
            wifi_detail,
            wifi_on,
            app.network.as_ref().map(|_| Message::WifiToggle(!wifi_on)),
        ),
        tile(
            "bluetooth",
            "Bluetooth",
            bt_detail,
            app.bt.powered,
            app.bt
                .available
                .then_some(Message::BtPower(!app.bt.powered)),
        ),
        tile(
            "sports_esports",
            "Game mode",
            on_off(app.game_mode),
            app.game_mode,
            Some(Message::GameModeToggle),
        ),
        tile(
            if mic.is_some_and(|m| m.muted) {
                "mic_off"
            } else {
                "mic"
            },
            "Microphone",
            match mic {
                Some(m) if m.muted => "Muted".into(),
                Some(_) => "On".into(),
                None => "None found".into(),
            },
            mic.is_some_and(|m| !m.muted),
            mic.map(|_| Message::MicMuteToggle),
        ),
        tile(
            "coffee",
            "Keep awake",
            awake_detail,
            awake_on,
            Some(Message::KeepAwakeToggle),
        ),
        tile(
            "speed",
            "Power mode",
            profile.map_or("Unavailable".into(), |p| profile_label(&p.active).into()),
            profile.is_some_and(|p| p.active != "balanced"),
            profile.map(|_| Message::PowerProfileNext),
        ),
        tile(
            "do_not_disturb_on",
            "Do not disturb",
            on_off(app.notif.dnd),
            app.notif.dnd,
            Some(Message::NotifDnd(!app.notif.dnd)),
        ),
        tile(
            "nightlight",
            "Night light",
            night_detail,
            night_on,
            Some(Message::NightLightToggle),
        ),
    ];
    let mut grid = column![].spacing(10);
    let mut tiles = tiles.into_iter().map(Element::from).peekable();
    while tiles.peek().is_some() {
        let mut r = row![].spacing(10);
        for _ in 0..2 {
            r = r.push(
                tiles
                    .next()
                    .unwrap_or_else(|| Space::new().width(Length::Fill).into()),
            );
        }
        grid = grid.push(r);
    }
    let tiles = grid;

    let level = |glyph: &'static str,
                 value: f32,
                 on_change: fn(f32) -> Message,
                 on_icon: Option<Message>| {
        let mut b = button(icon(c, glyph, 20.0))
            .padding(4)
            .style(move |_, s| button::Style {
                background: matches!(s, button::Status::Hovered)
                    .then(|| Color { a: 0.12, ..fg }.into()),
                text_color: fg,
                border: border::rounded(8),
                ..Default::default()
            });
        if let Some(m) = on_icon {
            b = b.on_press(m);
        }
        row![
            b,
            slider(0.0..=100.0, value, on_change).step(1.0_f32),
            text(format!("{value:.0}")).size(12).color(muted).width(30)
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    };
    let mut levels = column![].spacing(6);
    if let Some(v) = app.volume {
        levels = levels.push(level(
            crate::status::volume_icon(v),
            f32::from(v.percent.min(100)),
            Message::QuickVolume,
            Some(Message::VolumeMute),
        ));
    }
    if let Some(b) = app.brightness {
        levels = levels.push(level(
            "brightness_medium",
            f32::from(b),
            Message::QuickBrightness,
            None,
        ));
    }

    let action = |glyph: &str, label: &str, msg: Message| {
        button(
            column![icon(c, glyph, 20.0), text(label.to_owned()).size(11)]
                .spacing(2)
                .width(Length::Fill)
                .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([8, 4])
        .on_press(msg)
        .style(move |_, s| button::Style {
            background: Some(
                Color {
                    a: if matches!(s, button::Status::Hovered) {
                        0.14
                    } else {
                        0.06
                    },
                    ..fg
                }
                .into(),
            ),
            text_color: fg,
            border: border::rounded(12),
            ..Default::default()
        })
        .into()
    };
    let actions = row([
        action("screenshot_region", "Screenshot", Message::QuickScreenshot),
        action("content_paste", "Clipboard", Message::ToggleClipboard),
        action("settings", "Settings", Message::ToggleSettings),
        action("power_settings_new", "Power", Message::TogglePower),
    ])
    .spacing(8);
    column![tiles, levels, actions]
        .spacing(14)
        .width(QUICK_W)
        .into()
}

fn profile_label(id: &str) -> &str {
    match id {
        "power-saver" => "Power saver",
        "balanced" => "Balanced",
        "performance" => "Performance",
        other => other,
    }
}

pub fn power(app: &App) -> Element<'_, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    let red: Color = c.colors.urgent.into();
    let on_accent: Color = c.colors.on_accent.into();
    let muted: Color = c.colors.muted.into();
    let item = |action: PowerAction, glyph: &str, label: &str| {
        let confirming = app.power_confirm == Some(action);
        button(
            column![
                icon(c, glyph, 28.0),
                text(if confirming { "Confirm?" } else { label }.to_owned()).size(12)
            ]
            .spacing(6)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([16, 4])
        .on_press(Message::Power(action))
        .style(move |_, s| button::Style {
            background: Some(
                if confirming {
                    red
                } else {
                    Color {
                        a: if matches!(s, button::Status::Hovered) {
                            0.14
                        } else {
                            0.06
                        },
                        ..fg
                    }
                }
                .into(),
            ),
            text_color: if confirming { on_accent } else { fg },
            border: border::rounded(16),
            ..Default::default()
        })
        .into()
    };
    let hint: Element<'_, Message> = match app.power_confirm {
        Some(_) => text("Click again to confirm, or Esc to cancel")
            .size(12)
            .color(muted)
            .into(),
        None => Space::new().height(14).into(),
    };
    column![
        row([
            item(PowerAction::Lock, "lock", "Lock"),
            item(PowerAction::Suspend, "bedtime", "Suspend"),
            item(PowerAction::Logout, "logout", "Log out"),
            item(PowerAction::Reboot, "restart_alt", "Restart"),
            item(PowerAction::Shutdown, "power_settings_new", "Shut down"),
        ])
        .spacing(10),
        container(hint).center_x(Length::Fill),
    ]
    .spacing(10)
    .width(POWER_W)
    .into()
}

/// Quick settings (or power) button for the bar.
pub fn widget<'a>(app: &'a App, glyph: &str, msg: Message) -> Element<'a, Message> {
    let c = &app.config;
    let fg: Color = c.colors.foreground.into();
    button(icon(c, glyph, c.bar.font_size * 1.3))
        .padding(4)
        .on_press(msg)
        .style(move |_, s| button::Style {
            background: matches!(s, button::Status::Hovered)
                .then(|| Color { a: 0.12, ..fg }.into()),
            text_color: fg,
            border: border::rounded(8),
            ..Default::default()
        })
        .into()
}
