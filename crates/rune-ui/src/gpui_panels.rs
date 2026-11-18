//! GPUI views for the notch's feature panels.
use crate::{
    App as Shell, Message, Panel,
    controls::slider::{Slider, SliderEvent},
    gpui_images::Images,
    gpui_notch::Dispatch,
    gpui_widgets::{
        button, color, column, icon, menu_row, module, row, section_label, separator,
        status_circle, switch, visualizer,
    },
};
use chrono::{Datelike, Days, Local};
use gpui::{AnyElement, Context, Entity, FocusHandle, Render, Window, div, img, prelude::*, px};
use rune_services::{
    media::{Control, Status},
    system::PowerAction,
    tray::{MenuEntry, MenuKind},
};

pub(crate) struct PanelView {
    shell: Entity<Shell>,
    panel: Panel,
    dispatch: Dispatch,
    focus: FocusHandle,
    volume: Entity<Slider>,
    mic_volume: Entity<Slider>,
    brightness: Entity<Slider>,
    seek: Entity<Slider>,
    images: Images,
    measurement: std::rc::Rc<std::cell::Cell<Option<(u32, u32)>>>,
    _subscriptions: Vec<gpui::Subscription>,
}
impl PanelView {
    pub fn new(
        shell: Entity<Shell>,
        panel: Panel,
        dispatch: Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let images = shell.read(cx).images.clone();
        cx.on_release(|this, cx| this.images.release_assets(cx))
            .detach();
        let accent = color(shell.read(cx).config.colors.accent);
        let volume = cx.new(|_| Slider::new((0., 100.), 0., accent));
        let mic_volume = cx.new(|_| Slider::new((0., 100.), 0., accent));
        let brightness = cx.new(|_| Slider::new((0., 100.), 0., accent));
        let seek = cx.new(|_| Slider::new((0., 1.), 0., accent));
        let mut subscriptions = vec![cx.observe(&shell, |_, _, cx| cx.notify())];
        for (slider, change, release) in [
            (&volume, Message::QuickVolume as fn(f32) -> Message, None),
            (&mic_volume, Message::QuickMicVolume, None),
            (&brightness, Message::QuickBrightness, None),
            (&seek, Message::SeekDrag, Some(Message::SeekCommit)),
        ] {
            let dispatch = dispatch.clone();
            subscriptions.push(cx.subscribe_in(
                slider,
                window,
                move |_, _, event: &SliderEvent, window, cx| {
                    let message = match event {
                        SliderEvent::Change(value) => Some(change(*value)),
                        SliderEvent::Release => release.clone(),
                    };
                    if let Some(message) = message {
                        dispatch(message, window, cx);
                    }
                },
            ));
        }
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            shell,
            panel,
            dispatch,
            focus,
            volume,
            mic_volume,
            brightness,
            seek,
            images,
            measurement: std::rc::Rc::new(std::cell::Cell::new(None)),
            _subscriptions: subscriptions,
        }
    }
}
impl Render for PanelView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = self.shell.clone();
        let (accent, volume, brightness, length, position, volume_pill, brightness_pill) = {
            let app = shell.read(cx);
            let pill = |glyph: &str| crate::controls::slider::Pill {
                glyph: glyph.to_owned().into(),
                font: app.config.bar.icon_font.clone().into(),
                fill: color(app.config.colors.foreground),
                ink: color(app.config.colors.background),
            };
            (
                color(app.config.colors.accent),
                app.snapshot
                    .volume
                    .map_or(0., |v| f32::from(v.percent.min(100))),
                app.snapshot.brightness.map_or(0., f32::from),
                app.snapshot
                    .media
                    .as_ref()
                    .and_then(|m| m.length)
                    .map_or(1., |d| d.as_secs_f32().max(1.)),
                app.seek.unwrap_or_else(|| {
                    app.snapshot
                        .media
                        .as_ref()
                        .map_or(0., |m| m.position_now().as_secs_f32())
                }),
                pill(
                    app.snapshot
                        .volume
                        .map_or("volume_up", crate::status::volume_icon),
                ),
                pill("light_mode"),
            )
        };
        self.volume.update(cx, |slider, _| {
            slider.sync((0., 100.), volume, accent);
            slider.set_pill(Some(volume_pill));
        });
        let mic = shell.read(cx).snapshot.audio.mic();
        let mic_value = mic
            .and_then(|device| device.volume)
            .map_or(0., |v| f32::from(v.min(100)));
        let mic_pill = crate::controls::slider::Pill {
            glyph: if mic.is_some_and(|device| device.muted) {
                "mic_off"
            } else {
                "mic"
            }
            .into(),
            font: shell.read(cx).config.bar.icon_font.clone().into(),
            fill: color(shell.read(cx).config.colors.foreground),
            ink: color(shell.read(cx).config.colors.background),
        };
        self.mic_volume.update(cx, |slider, _| {
            slider.sync((0., 100.), mic_value, accent);
            slider.set_pill(Some(mic_pill));
        });
        self.brightness.update(cx, |slider, _| {
            slider.sync((0., 100.), brightness, accent);
            slider.set_pill(Some(brightness_pill));
        });
        self.seek
            .update(cx, |slider, _| slider.sync((0., length), position, accent));
        let app = shell.read(cx);
        self.images.retain(|key| {
            (key == "media" && app.snapshot.media.as_ref().is_some_and(|m| m.art.is_some()))
                || app
                    .snapshot
                    .notif
                    .history
                    .iter()
                    .any(|item| key == format!("notification:{}", item.id))
        });
        let body = match self.panel {
            Panel::Media => self.media(app),
            Panel::Weather => weather(app),
            Panel::Calendar => calendar(app, &self.dispatch),
            Panel::Power => power(app, &self.dispatch),
            Panel::Capture => capture(app, &self.dispatch),
            Panel::Quick => self.quick(app),
            Panel::Notifications => notifications(app, &self.dispatch, &mut self.images),
            Panel::TrayMenu => tray_menu(app, &self.dispatch),
            Panel::Wifi => wifi(app, &self.dispatch),
            Panel::Bluetooth => bluetooth(app, &self.dispatch),
            Panel::Sound => self.sound(app),
            Panel::Battery => battery(app, &self.dispatch),
            Panel::System => system(app),
            Panel::Launcher(_) => div().into_any_element(),
        };
        let dispatch = self.dispatch.clone();
        let width = match self.panel {
            Panel::Media => 300.,
            Panel::Quick => crate::quick::QUICK_W,
            Panel::Power => crate::quick::POWER_W,
            Panel::Capture => crate::capture_panel::CAPTURE_W,
            Panel::Weather => crate::weather::PANEL_W,
            Panel::Calendar => 270.,
            Panel::Notifications => 340.,
            Panel::TrayMenu => crate::tray::MENU_W,
            Panel::Wifi | Panel::Bluetooth | Panel::Sound | Panel::Launcher(_) => DROPDOWN_W,
            Panel::Battery => 260.,
            Panel::System => crate::system_panel::SYSTEM_W,
        } + 2. * PANEL_PAD;
        let content =
            crate::gpui_widgets::surface(&app.config, crate::gpui_widgets::Material::Popover, 14.)
                .id("feature-panel")
                .flex()
                .flex_col()
                .flex_none()
                .w(px(width))
                .max_h(px(crate::panel::MAX_H))
                .overflow_hidden()
                .track_focus(&self.focus)
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "escape" {
                        dispatch(Message::Escape, window, cx);
                    }
                })
                .child(
                    div()
                        .id("feature-panel-scroll")
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p(px(PANEL_PAD))
                        .child(body),
                )
                .child(crate::gpui_widgets::measure_panel(
                    self.measurement.clone(),
                    self.dispatch.clone(),
                ));
        self.images.release_unused_assets(cx);
        crate::gpui_widgets::natural_content(content)
    }
}
impl PanelView {
    fn sound(&mut self, app: &Shell) -> AnyElement {
        let c = &app.config;
        let mut body = column().gap(px(2.)).child(title_row(app, "Sound", None));
        if app.snapshot.volume.is_some() {
            body = body.child(div().px(px(6.)).pb(px(4.)).child(self.volume.clone()));
        } else {
            body = body.child(unavailable(app, "Volume"));
        }
        if app
            .snapshot
            .audio
            .mic()
            .and_then(|device| device.volume)
            .is_some()
        {
            body = body
                .child(section_label(c, "Microphone"))
                .child(div().px(px(6.)).pb(px(4.)).child(self.mic_volume.clone()));
        }
        if !app.snapshot.audio.outputs.is_empty() {
            body = body.child(section_label(c, "Output"));
        }
        for device in &app.snapshot.audio.outputs {
            body = body.child(clickable(
                menu_row(
                    format!("output:{}", device.name),
                    c,
                    Some(("speaker", device.default)),
                    device.description.clone(),
                    None,
                ),
                Message::Settings(crate::settings::Msg::Audio(
                    rune_services::audio::DeviceCommand::SetDefault(true, device.name.clone()),
                )),
                &self.dispatch,
            ));
        }
        body.child(settings_link(
            app,
            "Sound Settings…",
            Message::OpenSettingsPage(crate::settings::Page::Sound),
            &self.dispatch,
        ))
        .into_any_element()
    }
    fn media(&mut self, app: &Shell) -> AnyElement {
        let c = &app.config;
        let Some(media) = &app.snapshot.media else {
            return column()
                .gap(px(2.))
                .child(title_row(app, "Now Playing", None))
                .child(crate::gpui_widgets::muted(c, "Nothing Playing").px(px(6.)))
                .into_any_element();
        };
        let position = app.seek.unwrap_or(media.position_now().as_secs_f32());
        let picture = media
            .art
            .as_ref()
            .and_then(|bytes| self.images.encoded("media", bytes));
        let picture = match picture {
            Some(source) => img(source)
                .size(px(ART))
                .object_fit(gpui::ObjectFit::Cover)
                .rounded(crate::gpui_widgets::corner(10.))
                .into_any_element(),
            None => status_circle(c, "music_note", false, ART).into_any_element(),
        };
        let muted = color(c.colors.muted);
        let info = column()
            .gap(px(2.))
            .flex_1()
            .min_w_0()
            .child(
                crate::gpui_widgets::heading(div())
                    .truncate()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(if media.title.is_empty() {
                        media.identity.clone()
                    } else {
                        media.title.clone()
                    }),
            )
            .child(
                crate::gpui_widgets::muted(
                    c,
                    [media.artist.as_str(), media.album.as_str()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                )
                .truncate(),
            )
            .child(
                row()
                    .justify_between()
                    .pt(px(4.))
                    .child(
                        row()
                            .gap(px(4.))
                            .min_w_0()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(icon(c, "headphones", 12.))
                            .child(div().truncate().child(media.identity.clone())),
                    )
                    .child(visualizer(app, 10, 3., 14.)),
            );
        let mut body = column().w(px(300.)).gap(px(12.)).p(px(4.)).child(
            row()
                .gap(px(12.))
                .child(crate::gpui_widgets::frame(div().flex_none().child(picture)))
                .child(info),
        );
        if let Some(length) = media.length {
            let elapsed = std::time::Duration::from_secs_f32(position.max(0.));
            body = body.child(
                column().gap(px(2.)).child(self.seek.clone()).child(
                    row()
                        .justify_between()
                        .text_size(px(11.))
                        .text_color(muted)
                        .child(crate::media::clock(elapsed))
                        .child(format!(
                            "-{}",
                            crate::media::clock(length.saturating_sub(elapsed))
                        )),
                ),
            );
        }
        let playing = media.status == Status::Playing;
        body.child(
            row()
                .justify_center()
                .gap(px(28.))
                .child(glyph_button(
                    app,
                    "media:previous",
                    "skip_previous",
                    28.,
                    Message::MediaControl(Control::Previous),
                    &self.dispatch,
                ))
                .child(play_button(app, playing, &self.dispatch))
                .child(glyph_button(
                    app,
                    "media:next",
                    "skip_next",
                    28.,
                    Message::MediaControl(Control::Next),
                    &self.dispatch,
                )),
        )
        .into_any_element()
    }
    fn quick(&mut self, app: &Shell) -> AnyElement {
        let s = &app.snapshot;
        let c = &app.config;
        let dispatch = &self.dispatch;
        let wifi = s.network.as_ref().is_some_and(|n| n.wifi_enabled);
        let mic = s.audio.mic();
        let held = |value: &Result<bool, String>| match value {
            Ok(true) => "On".to_owned(),
            Ok(false) => "Off".into(),
            Err(error) => error.clone(),
        };
        let connectivity = column()
            .flex_1()
            .gap(px(8.))
            .child(module(c).child(quick_tile(
                app,
                "wifi",
                "Wi-Fi",
                s.network.as_ref().map_or("Unavailable".into(), |n| {
                    if n.connected {
                        n.name.clone()
                    } else if n.wifi_enabled {
                        "Not Connected".into()
                    } else {
                        "Off".into()
                    }
                }),
                wifi,
                s.network.as_ref().map(|_| Message::WifiToggle(!wifi)),
                dispatch,
            )))
            .child(module(c).child(quick_tile(
                app,
                "bluetooth",
                "Bluetooth",
                if !s.bt.available {
                    "Unavailable".into()
                } else if !s.bt.powered {
                    "Off".into()
                } else {
                    format!(
                        "{} Connected",
                        s.bt.devices.iter().filter(|d| d.connected).count()
                    )
                },
                s.bt.powered,
                s.bt.available.then_some(Message::BtPower(!s.bt.powered)),
                dispatch,
            )))
            .child(module(c).child(quick_tile(
                app,
                "sports_esports",
                "Game Mode",
                if app.game_mode { "On" } else { "Off" }.into(),
                app.game_mode,
                Some(Message::GameModeToggle),
                dispatch,
            )));
        let modes = column()
            .flex_1()
            .gap(px(8.))
            .child(module(c).child(quick_tile(
                app,
                "do_not_disturb_on",
                "Focus",
                if s.notif.dnd { "On" } else { "Off" }.into(),
                s.notif.dnd,
                Some(Message::NotifDnd(!s.notif.dnd)),
                dispatch,
            )))
            .child(module(c).child(quick_tile(
                app,
                "coffee",
                "Keep Awake",
                held(&s.keep_awake),
                s.keep_awake == Ok(true),
                Some(Message::KeepAwakeToggle),
                dispatch,
            )))
            .child(module(c).child(quick_tile(
                app,
                "nightlight",
                "Night Shift",
                held(&s.night_light),
                s.night_light == Ok(true),
                Some(Message::NightLightToggle),
                dispatch,
            )));
        let mut shortcuts = module(c).flex().justify_around().items_start();
        for (glyph, label, on, message) in [
            (
                "mic",
                "Mic",
                mic.is_some_and(|m| !m.muted),
                mic.map(|_| Message::MicMuteToggle),
            ),
            (
                "speed",
                s.power_profile
                    .as_ref()
                    .map_or("Power Mode", |p| match p.active.as_str() {
                        "performance" => "Performance",
                        "power-saver" => "Power Saver",
                        _ => "Balanced",
                    }),
                s.power_profile
                    .as_ref()
                    .is_some_and(|p| p.active != "balanced"),
                s.power_profile.as_ref().map(|_| Message::PowerProfileNext),
            ),
            (
                "screenshot_region",
                "Capture",
                false,
                Some(Message::QuickScreenshot),
            ),
            (
                "content_paste",
                "Clipboard",
                false,
                Panel::from_name("clipboard").map(Message::Toggle),
            ),
            ("settings", "Settings", false, Some(Message::ToggleSettings)),
            (
                "power_settings_new",
                "Power",
                false,
                Some(Message::Toggle(Panel::Power)),
            ),
        ] {
            let content = column()
                .items_center()
                .gap(px(4.))
                .child(status_circle(c, glyph, on, 30.))
                .child(div().text_size(px(10.)).child(label.to_owned()));
            shortcuts = shortcuts.child(match message {
                Some(message) => clickable(
                    div()
                        .id(format!("shortcut:{glyph}"))
                        .rounded(crate::gpui_widgets::corner(8.))
                        .p(px(2.))
                        .active(|style| style.opacity(0.7))
                        .cursor_pointer()
                        .child(content),
                    message,
                    dispatch,
                ),
                None => content.opacity(0.5).into_any_element(),
            });
        }
        let mut body = column()
            .w(px(crate::quick::QUICK_W))
            .gap(px(8.))
            .child(
                row()
                    .items_start()
                    .gap(px(8.))
                    .child(connectivity)
                    .child(modes),
            )
            .child(shortcuts);
        if s.brightness.is_some() {
            body = body.child(
                module(c)
                    .child(
                        div()
                            .px(px(2.))
                            .pb(px(6.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Display"),
                    )
                    .child(self.brightness.clone()),
            );
        }
        if s.volume.is_some() {
            body = body.child(
                module(c)
                    .child(
                        div()
                            .px(px(2.))
                            .pb(px(6.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Sound"),
                    )
                    .child(self.volume.clone()),
            );
        }
        if s.audio.mic().and_then(|device| device.volume).is_some() {
            body = body.child(
                module(c)
                    .child(
                        div()
                            .px(px(2.))
                            .pb(px(6.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Microphone"),
                    )
                    .child(self.mic_volume.clone()),
            );
        }
        if let Some(media) = &s.media {
            let art = media
                .art
                .as_ref()
                .and_then(|bytes| self.images.encoded("media", bytes));
            let art = match art {
                Some(source) => img(source)
                    .size(px(38.))
                    .rounded(crate::gpui_widgets::corner(6.))
                    .into_any_element(),
                None => status_circle(c, "music_note", false, 38.).into_any_element(),
            };
            body = body.child(
                module(c)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(art)
                    .child(
                        column()
                            .flex_1()
                            .min_w_0()
                            .gap_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(media.title.clone()),
                            )
                            .child(crate::gpui_widgets::muted(c, media.artist.clone()).truncate()),
                    )
                    .child(glyph_button(
                        app,
                        "quick-play",
                        if media.status == Status::Playing {
                            "pause"
                        } else {
                            "play_arrow"
                        },
                        22.,
                        Message::MediaControl(Control::PlayPause),
                        dispatch,
                    ))
                    .child(glyph_button(
                        app,
                        "quick-next",
                        "skip_next",
                        22.,
                        Message::MediaControl(Control::Next),
                        dispatch,
                    )),
            );
        }
        body.into_any_element()
    }
}

fn quick_tile(
    app: &Shell,
    glyph: &str,
    label: &str,
    detail: String,
    on: bool,
    message: Option<Message>,
    dispatch: &Dispatch,
) -> AnyElement {
    let c = &app.config;
    let content = row()
        .gap(px(8.))
        .child(status_circle(c, glyph, on, 28.))
        .child(
            column()
                .gap_0()
                .min_w_0()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_size(px(12.))
                        .child(label.to_owned()),
                )
                .child(
                    crate::gpui_widgets::muted(c, detail)
                        .text_size(px(11.))
                        .truncate(),
                ),
        );
    match message {
        Some(message) => clickable(
            div()
                .id(format!("tile:{label}"))
                .p(px(4.))
                .rounded(crate::gpui_widgets::corner(8.))
                .hover(|style| style.bg(color(c.colors.foreground).opacity(0.06)))
                .active(|style| style.bg(color(c.colors.foreground).opacity(0.12)))
                .cursor_pointer()
                .child(content),
            message,
            dispatch,
        ),
        None => div()
            .p(px(4.))
            .opacity(0.5)
            .child(content)
            .into_any_element(),
    }
}

/// The media panel's primary control: a filled disc in the smooth look, a framed accent block in pixel.
fn play_button(app: &Shell, playing: bool, dispatch: &Dispatch) -> AnyElement {
    let c = &app.config;
    let (fill, ink) = if crate::gpui_widgets::pixel_look() {
        (color(c.colors.accent), color(c.colors.on_accent))
    } else {
        (color(c.colors.foreground), color(c.colors.background))
    };
    crate::gpui_widgets::frame(div())
        .child(clickable(
            div()
                .id("media:play-pause")
                .flex()
                .items_center()
                .justify_center()
                .size(px(PLAY))
                .rounded(crate::gpui_widgets::corner(PLAY / 2.))
                .bg(fill)
                .text_color(ink)
                .hover(|style| style.opacity(0.9))
                .active(|style| style.opacity(0.7))
                .cursor_pointer()
                .child(icon(c, if playing { "pause" } else { "play_arrow" }, 30.)),
            Message::MediaControl(Control::PlayPause),
            dispatch,
        ))
        .into_any_element()
}

const ART: f32 = 72.;
const PLAY: f32 = 48.;

fn glyph_button(
    app: &Shell,
    id: &str,
    glyph: &str,
    size: f32,
    message: Message,
    dispatch: &Dispatch,
) -> AnyElement {
    let fg = color(app.config.colors.foreground);
    clickable(
        div()
            .id(id.to_owned())
            .flex()
            .items_center()
            .justify_center()
            .p(px(4.))
            .rounded(crate::gpui_widgets::corner(7.))
            .hover(|style| style.bg(fg.opacity(0.10)))
            .active(|style| style.bg(fg.opacity(0.18)))
            .cursor_pointer()
            .child(icon(&app.config, glyph, size)),
        message,
        dispatch,
    )
}

const DROPDOWN_W: f32 = 280.;
const PANEL_PAD: f32 = 10.;

fn title_row(app: &Shell, title: &str, trailing: Option<AnyElement>) -> gpui::Div {
    row()
        .justify_between()
        .px(px(6.))
        .pt(px(2.))
        .pb(px(4.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(title.to_owned())
        .children(trailing)
        .text_color(color(app.config.colors.foreground))
}

fn settings_link(
    app: &Shell,
    label: &str,
    message: Message,
    dispatch: &Dispatch,
) -> impl IntoElement {
    let dispatch = dispatch.clone();
    column().gap_0().child(separator(&app.config)).child(
        menu_row(
            format!("link:{label}"),
            &app.config,
            None,
            label.to_owned(),
            None,
        )
        .on_click(move |_, window, cx| dispatch(message.clone(), window, cx)),
    )
}

fn clickable(row: gpui::Stateful<gpui::Div>, message: Message, dispatch: &Dispatch) -> AnyElement {
    let dispatch = dispatch.clone();
    row.on_click(move |_, window, cx| dispatch(message.clone(), window, cx))
        .into_any_element()
}

fn unavailable(app: &Shell, what: &str) -> AnyElement {
    div()
        .px(px(6.))
        .py(px(4.))
        .text_color(color(app.config.colors.muted))
        .child(format!("{what} Unavailable"))
        .into_any_element()
}

fn wifi(app: &Shell, dispatch: &Dispatch) -> AnyElement {
    let c = &app.config;
    let link = settings_link(
        app,
        "Wi-Fi Settings…",
        Message::OpenSettingsPage(crate::settings::Page::Wifi),
        dispatch,
    );
    let Some(network) = &app.snapshot.network else {
        return column()
            .gap(px(2.))
            .child(title_row(app, "Wi-Fi", None))
            .child(unavailable(app, "Wi-Fi"))
            .child(link)
            .into_any_element();
    };
    let enabled = network.wifi_enabled;
    let mut body = column().gap(px(2.)).child(title_row(
        app,
        "Wi-Fi",
        Some(
            switch(
                "wifi-power",
                &c.colors,
                enabled,
                Message::WifiToggle(!enabled),
                dispatch,
            )
            .into_any_element(),
        ),
    ));
    if enabled {
        let networks = &app.settings_ui.wifi;
        if network.connected {
            body = body
                .child(section_label(c, "Known Network"))
                .child(menu_row(
                    "wifi-current",
                    c,
                    Some((crate::status::network_icon(network), true)),
                    network.name.clone(),
                    None,
                ));
        }
        let others: Vec<_> = networks
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.active)
            .collect();
        if !others.is_empty() {
            body = body.child(section_label(c, "Other Networks"));
        } else if networks.is_empty() {
            body = body.child(
                crate::gpui_widgets::muted(c, "Searching…")
                    .px(px(6.))
                    .py(px(4.)),
            );
        }
        for (index, entry) in others {
            body = body.child(clickable(
                menu_row(
                    format!("wifi:{}:{}", entry.ssid, entry.ap),
                    c,
                    Some((crate::status::wifi_icon(entry.strength), false)),
                    entry.ssid.clone(),
                    entry
                        .secured
                        .then(|| icon(c, "lock", 14.).into_any_element()),
                ),
                Message::WifiDropdownPick(index),
                dispatch,
            ));
        }
    }
    body.child(link).into_any_element()
}

fn bluetooth(app: &Shell, dispatch: &Dispatch) -> AnyElement {
    let c = &app.config;
    let bt = &app.snapshot.bt;
    let link = settings_link(
        app,
        "Bluetooth Settings…",
        Message::OpenSettingsPage(crate::settings::Page::Bluetooth),
        dispatch,
    );
    if !bt.available {
        return column()
            .gap(px(2.))
            .child(title_row(app, "Bluetooth", None))
            .child(unavailable(app, "Bluetooth"))
            .child(link)
            .into_any_element();
    }
    let mut body = column().gap(px(2.)).child(title_row(
        app,
        "Bluetooth",
        Some(
            switch(
                "bt-power",
                &c.colors,
                bt.powered,
                Message::BtPower(!bt.powered),
                dispatch,
            )
            .into_any_element(),
        ),
    ));
    let paired: Vec<_> = bt.devices.iter().filter(|d| d.paired).collect();
    if bt.powered && !paired.is_empty() {
        body = body.child(section_label(c, "Devices"));
        for device in paired {
            let command = if device.connected {
                rune_services::bluetooth::Command::Disconnect(device.address.clone())
            } else {
                rune_services::bluetooth::Command::Connect(device.address.clone())
            };
            body = body.child(clickable(
                menu_row(
                    format!("bt:{}", device.address),
                    c,
                    Some((crate::bt::device_icon(&device.icon), device.connected)),
                    device.name.clone(),
                    device
                        .battery
                        .map(|level| format!("{level}%").into_any_element()),
                ),
                Message::Settings(crate::settings::Msg::Bt(command)),
                dispatch,
            ));
        }
    }
    body.child(link).into_any_element()
}

fn system(app: &Shell) -> AnyElement {
    use crate::system_panel::{net_scale, percent, rate, series, size, uptime};
    let title = column().gap(px(2.));
    let Some(stats) = &app.snapshot.stats else {
        return title
            .child(title_row(app, "System", None))
            .child(unavailable(app, "System Stats"))
            .into_any_element();
    };
    let history = &app.snapshot.stats_history;
    let join = |parts: &[Option<String>]| {
        parts
            .iter()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let cpu = stat_card(
        app,
        "system-cpu",
        ("memory", "CPU"),
        format!("{}%", stats.cpu),
        series(history, |s| percent(s.cpu)),
        [
            stats.cpu_name.as_deref().unwrap_or("Processor").to_owned(),
            join(&[
                Some(format!("{} threads", stats.threads)),
                stats
                    .mhz
                    .map(|mhz| format!("{:.1} GHz", f64::from(mhz) / 1000.)),
                stats.temp.map(|t| format!("{t}°C")),
            ]),
            format!(
                "Load {:.2} · {:.2} · {:.2}",
                stats.load[0], stats.load[1], stats.load[2]
            ),
        ],
    );
    let gpu = match &stats.gpu {
        Some(gpu) => stat_card(
            app,
            "system-gpu",
            ("developer_board", "GPU"),
            format!("{}%", gpu.busy),
            series(history, |s| s.gpu.as_ref().map_or(0., |g| percent(g.busy))),
            [
                gpu.name.as_deref().unwrap_or("Graphics").to_owned(),
                join(&[
                    gpu.temp.map(|t| format!("{t}°C")),
                    gpu.watts.map(|w| format!("{w:.0} W")),
                ]),
                gpu.vram.map_or_else(String::new, |v| {
                    format!("VRAM {} / {}", size(v.used), size(v.total))
                }),
            ],
        ),
        None => stat_card(
            app,
            "system-gpu",
            ("developer_board", "GPU"),
            "—".to_owned(),
            series(&std::collections::VecDeque::new(), |_| 0.),
            ["No supported GPU".to_owned(), String::new(), String::new()],
        ),
    };
    let memory = stat_card(
        app,
        "system-memory",
        ("memory_alt", "Memory"),
        format!("{}%", stats.ram),
        series(history, |s| percent(s.ram)),
        [
            format!("{} / {}", size(stats.mem.used), size(stats.mem.total)),
            if stats.swap.total > 0 {
                format!(
                    "Swap {} / {}",
                    size(stats.swap.used),
                    size(stats.swap.total)
                )
            } else {
                "No swap".to_owned()
            },
            String::new(),
        ],
    );
    let scale = net_scale(history);
    let network = stat_card(
        app,
        "system-network",
        ("swap_vert", "Network"),
        format!("↓ {}", rate(stats.net_down)),
        series(history, |s| s.net_down as f32 / scale),
        [
            format!("↑ {}", rate(stats.net_up)),
            format!("Peak {}", rate(scale as u64)),
            String::new(),
        ],
    );
    let pair = |a, b| div().flex().gap(px(8.)).child(a).child(b);
    title
        .child(title_row(
            app,
            "System",
            Some(
                div()
                    .text_color(color(app.config.colors.muted))
                    .font_weight(gpui::FontWeight::NORMAL)
                    .child(format!("Up {}", uptime(stats.uptime)))
                    .into_any_element(),
            ),
        ))
        .child(
            column()
                .gap(px(8.))
                .child(pair(cpu, gpu))
                .child(pair(memory, network)),
        )
        .into_any_element()
}

/// A metric tile: icon, label and current value over its live graph.
fn stat_card(
    app: &Shell,
    id: &'static str,
    (glyph, label): (&str, &str),
    value: String,
    graph: crate::system_panel::Series,
    details: [String; 3],
) -> gpui::Stateful<gpui::Div> {
    let c = &app.config;
    module(c)
        .id(id)
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            row()
                .justify_between()
                .child(
                    row()
                        .gap(px(4.))
                        .text_color(color(c.colors.muted))
                        .child(icon(c, glyph, 14.))
                        .child(
                            crate::gpui_widgets::heading(div())
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(label.to_owned()),
                        ),
                )
                .child(
                    crate::gpui_widgets::heading(div())
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(value),
                ),
        )
        .child(stat_graph(app, graph))
        .children(details.into_iter().filter(|d| !d.is_empty()).map(|detail| {
            crate::gpui_widgets::muted(c, detail)
                .text_size(px(11.))
                .truncate()
        }))
}

/// History bars, newest on the right: soft fading bars for smooth, a lit block grid for pixel.
fn stat_graph(app: &Shell, (values, len): crate::system_panel::Series) -> impl IntoElement {
    let accent = color(app.config.colors.accent);
    let track = color(app.config.colors.foreground).opacity(0.06);
    let pixel = crate::gpui_widgets::pixel_look();
    gpui::canvas(
        |bounds, _, _| bounds,
        move |_, bounds, window, _| {
            let count = values.len();
            let slot = bounds.size.width / count as f32;
            let height = bounds.size.height;
            let start = count.saturating_sub(len);
            if !pixel {
                window.paint_quad(gpui::fill(bounds, track).corner_radii(px(6.)));
            }
            let blocks = if pixel {
                (height / slot).floor().max(1.)
            } else {
                1.
            };
            for column in 0..count {
                let left = bounds.left() + slot * column as f32;
                let sample = column.checked_sub(start).and_then(|index| {
                    values
                        .get(index)
                        .filter(|_| index < len)
                        .map(|v| (index, *v))
                });
                if pixel {
                    let cell = (slot - px(1.)).max(px(1.));
                    let lit = sample.map_or(0., |(_, value)| (value * blocks).round());
                    for block in 0..blocks as u32 {
                        let top = bounds.bottom() - slot * (block + 1) as f32;
                        let fill = if (block as f32) < lit { accent } else { track };
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(gpui::point(left, top), gpui::size(cell, cell)),
                            fill,
                        ));
                    }
                } else if let Some((index, value)) = sample {
                    let bar = (height * value).max(px(1.5));
                    let age = (index + 1) as f32 / len as f32;
                    window.paint_quad(
                        gpui::fill(
                            gpui::Bounds::new(
                                gpui::point(left, bounds.bottom() - bar),
                                gpui::size((slot - px(1.)).max(px(1.)), bar),
                            ),
                            accent.opacity(0.25 + 0.65 * age),
                        )
                        .corner_radii(px(1.)),
                    );
                }
            }
        },
    )
    .w_full()
    .h(px(42.))
}

fn battery(app: &Shell, dispatch: &Dispatch) -> AnyElement {
    let c = &app.config;
    let mut body = column().gap(px(2.));
    match &app.snapshot.battery {
        Some(battery) => {
            body = body.child(title_row(
                app,
                "Battery",
                Some(
                    div()
                        .text_color(color(c.colors.muted))
                        .font_weight(gpui::FontWeight::NORMAL)
                        .child(format!("{:.0}%", battery.percent))
                        .into_any_element(),
                ),
            ));
            let source = if battery.charging {
                "Power Source: Power Adapter"
            } else {
                "Power Source: Battery"
            };
            let detail = match battery.time_left {
                Some(seconds) if !battery.full => format!(
                    "{source} · {}:{:02} {}",
                    seconds / 3600,
                    seconds % 3600 / 60,
                    if battery.charging {
                        "until full"
                    } else {
                        "left"
                    }
                ),
                _ => source.to_owned(),
            };
            body = body.child(
                div()
                    .px(px(6.))
                    .text_size(px(11.))
                    .text_color(color(c.colors.muted))
                    .child(detail),
            );
        }
        None => {
            body = body
                .child(title_row(app, "Battery", None))
                .child(unavailable(app, "Battery"));
        }
    }
    body.child(settings_link(
        app,
        "Power…",
        Message::Toggle(Panel::Power),
        dispatch,
    ))
    .into_any_element()
}

fn power(app: &Shell, dispatch: &Dispatch) -> AnyElement {
    let c = &app.config;
    let mut body = column()
        .w(px(crate::quick::POWER_W))
        .gap_0()
        .child(title_row(app, "Power", None));
    for (index, (glyph, label, action)) in [
        ("lock", "Lock Screen", PowerAction::Lock),
        ("bedtime", "Sleep", PowerAction::Suspend),
        ("logout", "Log Out…", PowerAction::Logout),
        ("restart_alt", "Restart…", PowerAction::Reboot),
        ("power_settings_new", "Shut Down…", PowerAction::Shutdown),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            body = body.child(separator(c));
        }
        let confirming = app.power_confirm == Some(action);
        body = body.child(clickable(
            menu_row(
                label,
                c,
                Some((glyph, confirming)),
                if confirming {
                    "Click Again to Confirm"
                } else {
                    label
                },
                None,
            ),
            Message::Power(action),
            dispatch,
        ));
    }
    if app.power_confirm.is_some() {
        body = body.child(
            crate::gpui_widgets::muted(c, "Escape cancels.")
                .px(px(6.))
                .pt(px(4.)),
        );
    }
    body.into_any_element()
}

fn capture(app: &Shell, dispatch: &Dispatch) -> AnyElement {
    use crate::capture_panel::{CAPTURE_W, CaptureMode, shortcut, targets};
    use crate::settings::{Flag, Msg};
    use rune_services::capture::Target;
    let c = &app.config;
    let fg = color(c.colors.foreground);
    let corner = crate::gpui_widgets::corner;
    let mode = app.capture_mode;
    let mut modes = row()
        .gap(px(2.))
        .p(px(3.))
        .rounded(corner(11.))
        .bg(fg.opacity(0.08));
    for (item_mode, glyph, label) in [
        (CaptureMode::Screenshot, "screenshot_region", "Screenshot"),
        (CaptureMode::Record, "videocam", "Record"),
        (CaptureMode::Text, "document_scanner", "Text"),
    ] {
        let mut item = row()
            .id(format!("capture-mode-{label}"))
            .flex_1()
            .justify_center()
            .gap(px(6.))
            .h(px(30.))
            .rounded(corner(8.))
            .text_color(color(c.colors.muted))
            .hover(|style| style.bg(fg.opacity(0.08)))
            .active(|style| style.bg(fg.opacity(0.14)))
            .cursor_pointer()
            .child(icon(c, glyph, 16.))
            .child(label);
        if item_mode == mode {
            item = if crate::gpui_widgets::pixel_look() {
                item.bg(color(c.colors.accent))
                    .text_color(color(c.colors.on_accent))
            } else {
                item.bg(fg.opacity(0.16))
                    .text_color(fg)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
            };
        }
        modes = modes.child(clickable(item, Message::CaptureMode(item_mode), dispatch));
    }
    let mut tiles = row().gap(px(8.)).items_start();
    if mode == CaptureMode::Record && app.snapshot.recording.is_some() {
        tiles = tiles.child(capture_tile(
            app,
            "capture-stop".into(),
            "stop_circle",
            "Stop recording",
            None,
            Message::StopRecording,
            dispatch,
        ));
    } else {
        for &target in targets(mode) {
            let (glyph, label) = match target {
                Target::Selection => ("crop_free", "Area"),
                Target::Window => ("web_asset", "Window"),
                Target::Screen => ("desktop_windows", "Screen"),
                Target::All => ("select_all", "All"),
            };
            tiles = tiles.child(capture_tile(
                app,
                format!("capture-{mode:?}-{}", target.word()),
                glyph,
                label,
                Some(
                    shortcut(mode, target)
                        .map_or_else(|| "—".to_owned(), |keys| keys.replace(" + ", " ")),
                ),
                Message::Capture(mode, target),
                dispatch,
            ));
        }
    }
    let opts = crate::capture_options(c);
    let (flag_label, flag, on, dir, dir_label) = match mode {
        CaptureMode::Record => (
            "Record audio",
            Flag::RecordAudio,
            c.capture.record_audio,
            opts.record_dir,
            "Recordings",
        ),
        CaptureMode::Screenshot | CaptureMode::Text => (
            "Copy to clipboard",
            Flag::CaptureCopy,
            c.capture.copy,
            opts.screenshot_dir,
            "Screenshots",
        ),
    };
    let footer = row()
        .justify_between()
        .px(px(4.))
        .child(
            row()
                .gap(px(8.))
                .child(switch(
                    format!("capture-flag-{flag:?}"),
                    &c.colors,
                    on,
                    Message::Settings(Msg::Flag(flag, !on)),
                    dispatch,
                ))
                .child(crate::gpui_widgets::muted(c, flag_label)),
        )
        .child(clickable(
            row()
                .id("capture-folder")
                .gap(px(4.))
                .text_size(px(12.))
                .text_color(color(c.colors.accent))
                .active(|style| style.opacity(0.7))
                .cursor_pointer()
                .child(icon(c, "folder_open", 15.))
                .child(dir_label),
            Message::OpenLink(dir.display().to_string()),
            dispatch,
        ));
    column()
        .w(px(CAPTURE_W))
        .gap(px(12.))
        .child(modes)
        .child(tiles)
        .child(separator(c))
        .child(footer)
        .into_any_element()
}

fn capture_tile(
    app: &Shell,
    id: String,
    glyph: &str,
    label: &'static str,
    keys: Option<String>,
    message: Message,
    dispatch: &Dispatch,
) -> AnyElement {
    let c = &app.config;
    let fg = color(c.colors.foreground);
    clickable(
        column()
            .id(id)
            .flex_1()
            .items_center()
            .gap(px(6.))
            .pt(px(14.))
            .pb(px(11.))
            .px(px(4.))
            .rounded(crate::gpui_widgets::corner(12.))
            .bg(fg.opacity(0.06))
            .hover(|style| style.bg(fg.opacity(0.13)))
            .active(|style| style.bg(fg.opacity(0.20)))
            .cursor_pointer()
            .child(icon(c, glyph, 26.))
            .child(div().text_size(px(12.)).child(label))
            .children(keys.map(|keys| {
                crate::gpui_widgets::muted(c, keys)
                    .text_size(px(10.5))
                    .text_center()
            })),
        message,
        dispatch,
    )
}

fn weather(app: &Shell) -> AnyElement {
    let c = &app.config;
    let width = crate::weather::PANEL_W;
    let Some(weather) = &app.snapshot.weather else {
        return column()
            .w(px(width))
            .gap(px(2.))
            .child(title_row(app, "Weather", None))
            .child(
                crate::gpui_widgets::muted(
                    c,
                    if c.weather.location.is_empty() {
                        "Pick a place in Settings → Weather"
                    } else {
                        "Loading the forecast…"
                    },
                )
                .px(px(6.)),
            )
            .into_any_element();
    };
    let current = &weather.current;
    let today = weather.daily.first();
    let place = c
        .weather
        .location
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let mut body = column().w(px(width)).gap(px(8.)).child(
        row()
            .justify_between()
            .items_start()
            .px(px(6.))
            .pt(px(2.))
            .child(
                column()
                    .gap_0()
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(place))
                    .child(
                        div()
                            .text_size(px(40.))
                            .line_height(px(46.))
                            .font_weight(gpui::FontWeight::LIGHT)
                            .child(format!("{:.0}°", current.temperature)),
                    ),
            )
            .child(
                column()
                    .gap(px(2.))
                    .items_end()
                    .child(icon(
                        c,
                        rune_services::weather::icon(current.code, current.is_day),
                        30.,
                    ))
                    .child(
                        div()
                            .text_size(px(12.))
                            .child(rune_services::weather::describe(current.code)),
                    )
                    .children(today.map(|day| {
                        crate::gpui_widgets::muted(
                            c,
                            format!("H:{:.0}° L:{:.0}°", day.max, day.min),
                        )
                    })),
            ),
    );
    let mut hours = row().items_start().gap(px(2.));
    for hour in &weather.hourly {
        hours = hours.child(
            column()
                .min_w(px(46.))
                .gap(px(4.))
                .items_center()
                .text_size(px(12.))
                .child(crate::gpui_widgets::muted(
                    c,
                    hour.time.format("%H:%M").to_string(),
                ))
                .child(icon(
                    c,
                    rune_services::weather::icon(hour.code, hour.is_day),
                    20.,
                ))
                .child(format!("{:.0}°", hour.temperature)),
        );
    }
    body = body.child(module(c).child(div().id("hourly-weather").overflow_x_scroll().child(hours)));
    let detail = |label: &str, value: String| {
        row()
            .justify_between()
            .child(crate::gpui_widgets::muted(c, label.to_owned()))
            .child(value)
    };
    let left = column()
        .flex_1()
        .gap(px(4.))
        .child(detail("Feels Like", format!("{:.0}°", current.feels_like)))
        .child(detail("Humidity", format!("{:.0}%", current.humidity)))
        .child(detail(
            "Wind",
            format!(
                "{:.0} {}",
                current.wind,
                if weather.imperial { "mph" } else { "km/h" }
            ),
        ));
    let mut right = column().flex_1().gap(px(4.));
    if let Some(today) = today {
        let time = |t: Option<chrono::NaiveDateTime>| {
            t.map_or("—".into(), |t| t.format("%H:%M").to_string())
        };
        right = right
            .child(detail("Sunrise", time(today.sunrise)))
            .child(detail("Sunset", time(today.sunset)));
    }
    let details = module(c)
        .flex()
        .items_start()
        .gap(px(16.))
        .text_size(px(12.))
        .child(left)
        .child(right);
    body = body.child(details);
    let mut days = module(c).flex().flex_col().gap(px(2.)).text_size(px(12.));
    for day in &weather.daily {
        days = days.child(
            row()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(56.))
                        .child(day.date.format("%a %-d").to_string()),
                )
                .child(icon(c, rune_services::weather::icon(day.code, true), 18.))
                .child(crate::gpui_widgets::muted(c, format!("{}%", day.precipitation)).flex_1())
                .child(format!("{:.0}° / {:.0}°", day.min, day.max)),
        );
    }
    body.child(days).into_any_element()
}

fn calendar(app: &Shell, dispatch: &Dispatch) -> AnyElement {
    let c = &app.config;
    let month = app.cal_month;
    let today = Local::now().date_naive();
    let start = month - Days::new(month.weekday().num_days_from_monday() as u64);
    let cell = 270. / 7.;
    let mut grid = column().w(px(270.)).gap(px(2.)).child(
        row()
            .justify_between()
            .px(px(6.))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(month.format("%B %Y").to_string()),
            )
            .child(
                row()
                    .gap_0()
                    .child(glyph_button(
                        app,
                        "previous-month",
                        "chevron_left",
                        18.,
                        Message::CalMonth(-1),
                        dispatch,
                    ))
                    .child(glyph_button(
                        app,
                        "next-month",
                        "chevron_right",
                        18.,
                        Message::CalMonth(1),
                        dispatch,
                    )),
            ),
    );
    grid = grid.child(
        row()
            .gap_0()
            .children(["M", "T", "W", "T", "F", "S", "S"].map(|day| {
                div()
                    .w(px(cell))
                    .flex()
                    .justify_center()
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(color(c.colors.muted))
                    .child(day)
            })),
    );
    let fg = color(c.colors.foreground);
    for week in 0..6 {
        let mut line = row().gap_0();
        for weekday in 0..7 {
            let day = start + Days::new(week * 7 + weekday);
            let has_events = crate::calendar::on_day(&app.snapshot.calendar.events, day)
                .next()
                .is_some();
            let mut number = div()
                .size(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(crate::gpui_widgets::corner(9999.))
                .text_size(px(12.))
                .child(day.day().to_string());
            if day == today {
                number = number
                    .bg(color(c.colors.urgent))
                    .text_color(color(c.colors.on_accent))
                    .font_weight(gpui::FontWeight::BOLD);
            } else if day == app.cal_day {
                number = number.bg(fg.opacity(0.18));
            }
            if day.month() != month.month() {
                number = number.opacity(0.35);
            }
            line = line.child(clickable(
                column()
                    .id(format!("day-{day}"))
                    .w(px(cell))
                    .items_center()
                    .gap(px(1.))
                    .rounded(crate::gpui_widgets::corner(6.))
                    .hover(|style| style.bg(fg.opacity(0.06)))
                    .cursor_pointer()
                    .child(number)
                    .child(
                        div()
                            .size(px(4.))
                            .rounded(crate::gpui_widgets::corner(9999.))
                            .when(has_events, |d| d.bg(color(c.colors.accent))),
                    ),
                Message::CalDay(day),
                dispatch,
            ));
        }
        grid = grid.child(line);
    }
    grid = grid.child(separator(c)).child(section_label(
        c,
        app.cal_day.format("%A, %B %-d").to_string(),
    ));
    let mut count = 0;
    for event in crate::calendar::on_day(&app.snapshot.calendar.events, app.cal_day) {
        count += 1;
        let mut text = column()
            .gap_0()
            .flex_1()
            .min_w_0()
            .child(div().truncate().child(event.title.clone()))
            .child(crate::gpui_widgets::muted(
                c,
                if event.all_day {
                    "All Day".into()
                } else {
                    format!(
                        "{} – {}",
                        event.start.format("%H:%M"),
                        event.end.format("%H:%M")
                    )
                },
            ));
        if !event.location.is_empty() {
            text = text.child(crate::gpui_widgets::muted(c, event.location.clone()).truncate());
        }
        let mut entry = row()
            .items_start()
            .gap(px(8.))
            .px(px(6.))
            .py(px(4.))
            .child(
                div()
                    .w(px(3.))
                    .h(px(30.))
                    .rounded(crate::gpui_widgets::corner(2.))
                    .bg(color(c.colors.accent)),
            )
            .child(text);
        if let Some(url) = &event.meeting {
            entry = entry.child(
                button(
                    format!("join-{count}"),
                    "Join",
                    Message::OpenLink(url.clone()),
                    dispatch,
                )
                .bg(color(c.colors.accent))
                .text_color(color(c.colors.on_accent)),
            );
        }
        grid = grid.child(entry);
    }
    let note = if !app.snapshot.calendar.errors.is_empty() {
        Some(app.snapshot.calendar.errors.join("\n"))
    } else if c.calendar.urls.is_empty() {
        Some("Add a calendar link in Settings → Calendar".into())
    } else if count == 0 {
        Some("No Events".into())
    } else {
        None
    };
    grid.children(note.map(|note| crate::gpui_widgets::muted(c, note).px(px(6.)).py(px(4.))))
        .into_any_element()
}

fn notification_click(
    notification: &rune_services::notifications::Notification,
    center: bool,
) -> Message {
    if notification.actions.iter().any(|(key, _)| key == "default") {
        Message::NotifAction(notification.id, "default".into())
    } else if center {
        Message::Noop
    } else {
        Message::NotifDismiss(notification.id)
    }
}

pub(crate) fn notification(
    app: &Shell,
    notification: &rune_services::notifications::Notification,
    center: bool,
    dispatch: &Dispatch,
    images: &mut Images,
) -> AnyElement {
    let c = &app.config;
    let id = notification.id;
    let critical = notification.urgency == rune_services::notifications::Urgency::Critical;
    let mut close = Some(
        button(
            format!("dismiss-{id}"),
            icon(c, "close", 12.),
            if center {
                Message::NotifRemove(id)
            } else {
                Message::NotifDismiss(id)
            },
            dispatch,
        )
        .p_0()
        .size(px(20.))
        .rounded(crate::gpui_widgets::corner(9999.))
        .text_color(color(c.colors.foreground))
        .bg(crate::gpui_widgets::material(
            c,
            crate::gpui_widgets::Material::Card,
        ))
        .border_1()
        .border_color(color(c.colors.foreground).opacity(0.12)),
    );
    let description = if notification.body.is_empty() {
        notification.app_name.clone()
    } else {
        crate::notify::cut(&notification.body, 110)
    };
    let text = column()
        .gap_0()
        .flex_1()
        .min_w_0()
        .text_size(px(13.))
        .child(
            crate::gpui_widgets::heading(div())
                .line_height(px(19.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .truncate()
                .child(crate::notify::cut(&notification.summary, 48)),
        )
        .child(
            div()
                .max_h(px(36.))
                .line_height(px(18.))
                .overflow_hidden()
                .text_color(color(c.colors.muted))
                .child(description),
        );
    let mut content = row().size_full().gap(px(10.));
    if critical {
        content = content.child(
            icon(c, "error", 16.)
                .flex_none()
                .text_color(color(c.colors.urgent)),
        );
    } else if let Some(source) = notification
        .image
        .as_ref()
        .and_then(|image| images.source(&format!("notification:{id}"), image))
    {
        content = content.child(
            img(source)
                .flex_none()
                .size(px(32.))
                .object_fit(gpui::ObjectFit::Contain)
                .rounded(crate::gpui_widgets::corner(4.)),
        );
    }
    content = content.child(text);
    for (index, (key, label)) in notification
        .actions
        .iter()
        .filter(|(key, _)| key != "default")
        .take(2)
        .enumerate()
    {
        let primary = index == 0;
        content = content.child(
            button(
                format!("action-{id}-{key}"),
                div().truncate().child(label.clone()),
                Message::NotifAction(id, key.clone()),
                dispatch,
            )
            .flex_none()
            .max_w(px(96.))
            .h(px(24.))
            .px(px(8.))
            .rounded(crate::gpui_widgets::corner(4.))
            .text_size(px(12.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .when(primary, |action| {
                action
                    .bg(color(c.colors.foreground))
                    .text_color(color(c.colors.background))
            })
            .when(!primary, |action| {
                action.bg(color(c.colors.foreground).opacity(0.08))
            }),
        );
    }
    if center {
        content = content.child(
            column()
                .flex_none()
                .h_full()
                .gap(px(4.))
                .items_end()
                .children(close.take())
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(color(c.colors.muted))
                        .child(crate::notify::ago(notification)),
                ),
        );
    }
    let message = notification_click(notification, center);
    let dispatch = dispatch.clone();
    let hovered = crate::gpui_widgets::tinted(c, crate::gpui_widgets::Material::Card, 0.08);
    let pressed = crate::gpui_widgets::tinted(c, crate::gpui_widgets::Material::Card, 0.14);
    let card = div()
        .id(("notification", id as u64))
        .size_full()
        .p(px(16.))
        .rounded(crate::gpui_widgets::corner(8.))
        .border_1()
        .border_color(if critical {
            color(c.colors.urgent).into()
        } else {
            gpui::Hsla::from(color(c.colors.foreground)).opacity(0.12)
        })
        .bg(crate::gpui_widgets::material(
            c,
            crate::gpui_widgets::Material::Card,
        ))
        .when(!crate::gpui_widgets::pixel_look(), |card| {
            card.shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.1),
                offset: gpui::point(px(0.), px(4.)),
                blur_radius: px(12.),
                spread_radius: px(0.),
                inset: false,
            }])
        })
        .hover(|style| style.bg(hovered))
        .active(|style| style.bg(pressed))
        .cursor_pointer()
        .child(content)
        .on_click(move |_, window, cx| dispatch(message.clone(), window, cx));
    if center {
        return card.h(px(crate::notify::HEIGHT)).into_any_element();
    }
    let group = gpui::SharedString::from(format!("toast-{id}"));
    div()
        .group(group.clone())
        .relative()
        .size_full()
        .child(card)
        .children(close.map(|close| {
            close
                .absolute()
                .top(px(-7.))
                .left(px(-7.))
                .opacity(0.)
                .group_hover(group, |style| style.opacity(1.))
        }))
        .into_any_element()
}

fn notifications(app: &Shell, dispatch: &Dispatch, images: &mut Images) -> AnyElement {
    let c = &app.config;
    let dnd = app.snapshot.notif.dnd;
    let mut body = column().w(px(340.)).gap(px(8.)).child(title_row(
        app,
        "Notifications",
        Some(
            row()
                .gap(px(8.))
                .child(crate::gpui_widgets::muted(c, "Focus"))
                .child(switch(
                    "dnd",
                    &c.colors,
                    dnd,
                    Message::NotifDnd(!dnd),
                    dispatch,
                ))
                .child(
                    menu_row("clear-notifications", c, None, "Clear", None)
                        .font_weight(gpui::FontWeight::NORMAL)
                        .on_click({
                            let dispatch = dispatch.clone();
                            move |_, window, cx| dispatch(Message::NotifClear, window, cx)
                        }),
                )
                .into_any_element(),
        ),
    ));
    for item in &app.snapshot.notif.history {
        body = body.child(notification(app, item, true, dispatch, images));
    }
    if app.snapshot.notif.history.is_empty() {
        body = body.child(
            crate::gpui_widgets::muted(c, "No Notifications")
                .px(px(6.))
                .py(px(4.)),
        );
    }
    body.into_any_element()
}
fn tray_menu(app: &Shell, dispatch: &Dispatch) -> AnyElement {
    let c = &app.config;
    let Some((_, entries)) = &app.tray_menu else {
        return column()
            .w(px(crate::tray::MENU_W))
            .child(
                crate::gpui_widgets::muted(c, "Loading…")
                    .px(px(6.))
                    .py(px(4.)),
            )
            .into_any_element();
    };
    fn entries_view(
        app: &Shell,
        entries: &[MenuEntry],
        depth: usize,
        dispatch: &Dispatch,
    ) -> gpui::Div {
        let c = &app.config;
        let mut list = column().gap_0();
        for entry in entries {
            if entry.kind == MenuKind::Separator {
                list = list.child(separator(c));
                continue;
            }
            let mark = match entry.kind {
                MenuKind::Check(true) => Some("check"),
                MenuKind::Radio(true) => Some("radio_button_checked"),
                MenuKind::Check(false) | MenuKind::Radio(false) => Some(""),
                _ => None,
            };
            let mut line = row()
                .gap(px(6.))
                .pl(px(depth as f32 * 12.))
                .children(mark.map(|glyph| div().w(px(14.)).child(icon(c, glyph, 14.))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(entry.label.clone()),
                );
            if !entry.children.is_empty() {
                line = line.child(icon(c, "chevron_right", 14.));
                list = list
                    .child(div().px(px(6.)).py(px(5.)).child(line))
                    .child(entries_view(app, &entry.children, depth + 1, dispatch));
            } else if entry.enabled {
                let fg = color(c.colors.foreground);
                list = list.child(clickable(
                    div()
                        .id(format!("menu-{}", entry.id))
                        .px(px(6.))
                        .py(px(5.))
                        .rounded(crate::gpui_widgets::corner(5.))
                        .hover(|style| {
                            style
                                .bg(color(c.colors.accent))
                                .text_color(color(c.colors.on_accent))
                        })
                        .active(|style| style.bg(fg.opacity(0.2)))
                        .cursor_pointer()
                        .child(line),
                    Message::TrayMenuClick(entry.id),
                    dispatch,
                ));
            } else {
                list = list.child(div().px(px(6.)).py(px(5.)).opacity(0.4).child(line));
            }
        }
        list
    }
    column()
        .w(px(crate::tray::MENU_W))
        .child(entries_view(app, entries, 0, dispatch))
        .into_any_element()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use gpui::AppContext;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        time::{Duration, Instant},
    };

    pub(crate) fn populated_shell() -> Shell {
        let (actions, _) = tokio::sync::mpsc::unbounded_channel();
        let (_, events) = tokio::sync::mpsc::unbounded_channel();
        let (mut shell, _) = Shell::new(crate::Config::default(), events, actions, false);
        let now = Local::now();
        shell.snapshot.volume = Some(rune_services::audio::Volume {
            percent: 65,
            muted: false,
        });
        shell.snapshot.brightness = Some(45);
        shell.snapshot.media = Some(rune_services::media::Media {
            player: "test".into(),
            identity: "Test player".into(),
            title: "A song".into(),
            artist: "An artist".into(),
            album: "An album".into(),
            art: None,
            length: Some(Duration::from_secs(180)),
            status: Status::Playing,
            position: Duration::from_secs(42),
            read_at: Instant::now(),
            track_id: None,
        });
        for cpu in [10, 40, 24] {
            shell
                .snapshot
                .record_stats(Some(rune_services::stats::Stats {
                    cpu,
                    ram: 50,
                    temp: Some(52),
                    cpu_name: Some("Test CPU".into()),
                    threads: 8,
                    mhz: Some(3200),
                    load: [0.5, 0.4, 0.3],
                    mem: rune_services::stats::Usage {
                        used: 8 << 20,
                        total: 16 << 20,
                    },
                    swap: rune_services::stats::Usage::default(),
                    gpu: Some(rune_services::stats::Gpu {
                        name: Some("Test GPU".into()),
                        busy: cpu / 2,
                        temp: Some(48),
                        vram: Some(rune_services::stats::Usage {
                            used: 2 << 20,
                            total: 12 << 20,
                        }),
                        watts: Some(40.),
                    }),
                    net_down: 1_500_000,
                    net_up: 20_000,
                    uptime: Duration::from_secs(3 * 3600 + 120),
                }));
        }
        shell.snapshot.battery = Some(rune_services::system::Battery {
            percent: 70.,
            charging: false,
            full: false,
            time_left: None,
        });
        shell.snapshot.network = Some(rune_services::system::Network {
            connected: true,
            online: true,
            kind: rune_services::system::NetKind::Wifi,
            name: "Test Wi-Fi".into(),
            strength: Some(80),
            wifi_enabled: true,
        });
        shell.snapshot.bt.available = true;
        shell.snapshot.bt.powered = true;
        shell
            .snapshot
            .bt
            .devices
            .push(rune_services::bluetooth::Device {
                path: "/org/bluez/hci0/dev_test".into(),
                name: "Test Headphones".into(),
                address: "00:11:22:33:44:55".into(),
                icon: "audio-headset".into(),
                paired: true,
                trusted: true,
                connected: true,
                battery: Some(80),
                rssi: None,
                profiles: None,
            });
        shell
            .snapshot
            .audio
            .outputs
            .push(rune_services::audio::Device {
                name: "test-sink".into(),
                description: "Test Speakers".into(),
                volume: Some(50),
                muted: false,
                default: true,
            });
        shell
            .snapshot
            .audio
            .inputs
            .push(rune_services::audio::Device {
                name: "test-mic".into(),
                description: "Test Microphone".into(),
                muted: false,
                default: true,
                volume: Some(65),
            });
        shell
            .settings_ui
            .wifi
            .push(rune_services::system::WifiNetwork {
                ssid: "Neighbour".into(),
                strength: 40,
                secured: true,
                active: false,
                saved: None,
                device: "wlan0".into(),
                ap: "/ap/2".into(),
            });
        shell.snapshot.tray.push(rune_services::tray::TrayItem {
            key: "test".into(),
            service: "test".into(),
            path: "/test".into(),
            id: "test".into(),
            title: "Test".into(),
            image: Some(rune_services::notifications::Image::Rgba {
                width: 1,
                height: 1,
                data: std::sync::Arc::from([20, 180, 90, 255]),
            }),
            menu: None,
            item_is_menu: false,
        });
        shell.snapshot.bars = vec![20, 70, 40, 90, 50];
        shell.snapshot.weather = Some(rune_services::weather::Weather {
            imperial: false,
            current: rune_services::weather::Current {
                time: now.naive_local(),
                temperature: 28.,
                feels_like: 30.,
                humidity: 75.,
                wind: 12.,
                code: 2,
                is_day: true,
            },
            hourly: vec![rune_services::weather::Hour {
                time: now.naive_local(),
                temperature: 28.,
                code: 2,
                precipitation: 20,
                is_day: true,
            }],
            daily: vec![rune_services::weather::Day {
                date: now.date_naive(),
                max: 30.,
                min: 24.,
                code: 2,
                precipitation: 20,
                sunrise: Some(now.naive_local()),
                sunset: Some(now.naive_local()),
            }],
        });
        shell
            .config
            .calendar
            .urls
            .push("https://example.invalid/calendar.ics".into());
        shell
            .snapshot
            .calendar
            .events
            .push(rune_services::calendar::Event {
                title: "Meeting".into(),
                start: now,
                end: now + chrono::Duration::hours(1),
                all_day: false,
                location: "Room 1".into(),
                meeting: Some("https://example.invalid/meeting".into()),
                calendar: 0,
            });
        shell
            .snapshot
            .notif
            .history
            .push(rune_services::notifications::Notification {
                id: 1,
                app_name: "Test".into(),
                summary: "A notification".into(),
                body: "Test body".into(),
                image: Some(rune_services::notifications::Image::Rgba {
                    width: 1,
                    height: 1,
                    data: std::sync::Arc::from([220, 20, 40, 255]),
                }),
                actions: vec![
                    ("default".into(), "Open".into()),
                    ("reply".into(), "Reply".into()),
                ],
                urgency: rune_services::notifications::Urgency::Critical,
                time: now,
                resident: false,
            });
        shell.tray_menu = Some((
            "test".into(),
            vec![MenuEntry {
                id: 1,
                label: "Menu".into(),
                enabled: true,
                kind: MenuKind::Normal,
                children: vec![
                    MenuEntry {
                        id: 2,
                        label: "Checked".into(),
                        enabled: true,
                        kind: MenuKind::Check(true),
                        children: vec![],
                    },
                    MenuEntry {
                        id: 3,
                        label: "Disabled".into(),
                        enabled: false,
                        kind: MenuKind::Normal,
                        children: vec![],
                    },
                ],
            }],
        ));
        shell
    }

    #[test]
    #[ignore = "opens GPUI panels; requires a running Wayland compositor"]
    fn populated_and_empty_feature_panels_render() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = std::thread::spawn(move || {
            for panel in [
                "media",
                "weather",
                "calendar",
                "power",
                "quick-settings",
                "notifications",
                "tray-menu",
                "wifi",
                "bluetooth",
                "sound",
                "battery",
                "capture",
                "system",
                "empty-media",
                "empty-weather",
                "empty-calendar",
                "empty-notifications",
                "empty-tray",
                "empty-wifi",
                "empty-bluetooth",
                "empty-sound",
                "empty-battery",
                "empty-system",
            ] {
                std::thread::sleep(Duration::from_millis(150));
                tx.send(crate::UiEvent::Status(panel.into())).unwrap();
            }
            std::thread::sleep(Duration::from_millis(150));
            tx.send(crate::UiEvent::Quit).unwrap();
        });
        let windows = Rc::new(RefCell::new(crate::gpui_surfaces::Windows::default()));
        let rendered = Rc::new(Cell::new(0));
        let observed = rendered.clone();
        crate::runtime::run(
            rx,
            |cx| cx.new(|_| populated_shell()),
            move |shell, event, cx| {
                let crate::UiEvent::Status(name) = event else {
                    return;
                };
                let panel = if name == "tray-menu" || name == "empty-tray" {
                    Panel::TrayMenu
                } else {
                    Panel::from_name(name.strip_prefix("empty-").unwrap_or(&name)).unwrap()
                };
                if name.starts_with("empty-") {
                    shell.snapshot.media = None;
                    shell.snapshot.weather = None;
                    shell.snapshot.calendar.events.clear();
                    shell.snapshot.notif.history.clear();
                    shell.tray_menu = None;
                    shell.snapshot.network = None;
                    shell.snapshot.bt.available = false;
                    shell.snapshot.battery = None;
                    shell.snapshot.record_stats(None);
                    shell.snapshot.volume = None;
                    shell.snapshot.audio.outputs.clear();
                }
                let entity = cx.entity();
                let windows = windows.clone();
                let observed = observed.clone();
                cx.defer(move |cx| {
                    let mut windows = windows.borrow_mut();
                    windows.close(&crate::Surface::Panel, cx);
                    windows
                        .open(
                            crate::Surface::Panel,
                            crate::gpui_surfaces::options(
                                &crate::Surface::Panel,
                                (672, 440),
                                None,
                                false,
                                0,
                            ),
                            cx,
                            move |window, cx| {
                                cx.new(|cx| {
                                    PanelView::new(entity, panel, Rc::new(|_, _, _| {}), window, cx)
                                })
                            },
                        )
                        .unwrap();
                    observed.set(observed.get() + 1);
                });
            },
        );
        sender.join().unwrap();
        assert_eq!(rendered.get(), 23);
    }
}

#[cfg(test)]
mod notification_regressions {
    use super::*;
    #[test]
    fn popup_click_dismisses_without_default_and_history_does_not() {
        let shell = super::tests::populated_shell();
        let mut notification = shell.snapshot.notif.history[0].clone();
        notification.actions.clear();
        assert!(matches!(
            notification_click(&notification, false),
            Message::NotifDismiss(1)
        ));
        assert!(matches!(
            notification_click(&notification, true),
            Message::Noop
        ));
        notification.actions.push(("default".into(), "Open".into()));
        assert!(
            matches!(notification_click(&notification, false), Message::NotifAction(1, key) if key == "default")
        );
    }
}
