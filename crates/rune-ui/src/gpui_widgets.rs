//! Shared GPUI primitives and status widgets used by the notch and panels.
use crate::{
    App as Shell, Config, Message, Panel,
    config::{Rgba, Widget},
    gpui_images::Images,
    gpui_notch::Dispatch,
};
use gpui::{AnimationExt, AnyElement, Div, MouseButton, Stateful, div, img, prelude::*, px};

static PIXEL_FONT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
pub(crate) fn set_look(look: crate::config::Look, font: crate::config::PixelFont) {
    let value = match (look, font) {
        (crate::config::Look::Smooth, _) => 0,
        (crate::config::Look::Pixel, crate::config::PixelFont::Tiny5) => 1,
        (crate::config::Look::Pixel, crate::config::PixelFont::Monocraft) => 2,
    };
    PIXEL_FONT.store(value, std::sync::atomic::Ordering::Relaxed);
}
fn pixel_font() -> Option<&'static str> {
    match PIXEL_FONT.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(crate::config::PixelFont::Tiny5.family()),
        2 => Some(crate::config::PixelFont::Monocraft.family()),
        _ => None,
    }
}
pub(crate) fn pixel_look() -> bool {
    pixel_font().is_some()
}
pub(crate) fn corner(radius: f32) -> gpui::Pixels {
    if pixel_look() { px(0.) } else { px(radius) }
}
/// The bar's configured outline and drop shadow. A notch flush with the top edge
/// has no top stroke; the pixel look keeps strokes whole.
pub(crate) fn bar_outline<E: Styled>(element: E, bar: &crate::config::BarConfig) -> E {
    let width = if pixel_look() {
        bar.border_width.round()
    } else {
        bar.border_width
    };
    let mut element = element;
    if width > 0. {
        element = if bar.margin_top == 0 {
            element.border_x(px(width)).border_b(px(width))
        } else {
            element.border(px(width))
        }
        .border_color(color(bar.border_color));
        if bar.border_style == crate::config::BorderStyle::Dashed {
            element = element.border_dashed();
        }
    }
    if bar.shadow {
        element = element.shadow(vec![gpui::BoxShadow {
            color: color(bar.shadow_color).into(),
            offset: gpui::point(px(0.), px(bar.shadow_offset)),
            blur_radius: px(bar.shadow_blur),
            spread_radius: px(0.),
            inset: false,
        }]);
    }
    element
}
pub(crate) fn text_font(config: &Config) -> gpui::SharedString {
    match pixel_font() {
        Some(family) => family.into(),
        None => config.bar.font.clone().into(),
    }
}
pub(crate) fn fill<E: Styled + ParentElement>(element: E, fill: impl Into<gpui::Hsla>) -> E {
    let fill = fill.into();
    if !pixel_look() {
        return element.bg(fill);
    }
    element.child(
        gpui::canvas(
            |bounds, _, _| bounds,
            move |_, bounds, window, _| {
                let mut left = bounds.left();
                while left < bounds.right() {
                    let width = (bounds.right() - left).min(px(4.));
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(left, bounds.top()),
                            gpui::size(width, bounds.size.height),
                        ),
                        fill,
                    ));
                    left += px(6.);
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full(),
    )
}
pub(crate) fn heading<E: Styled>(element: E) -> E {
    match pixel_font() {
        Some(family) => element.font_family(family),
        None => element,
    }
}
pub(crate) fn frame<E: Styled + ParentElement>(element: E) -> E {
    if !pixel_look() {
        return element;
    }
    let shadow = px(3.);
    element
        .relative()
        .border_1()
        .border_color(gpui::rgba(0x8080807a))
        .child(
            gpui::canvas(
                |bounds, _, _| bounds,
                move |_, bounds, window, _| {
                    let black = gpui::black();
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(bounds.right(), bounds.top() + shadow),
                            gpui::size(shadow, bounds.size.height),
                        ),
                        black,
                    ));
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(bounds.left() + shadow, bounds.bottom()),
                            gpui::size(bounds.size.width - shadow, shadow),
                        ),
                        black,
                    ));
                },
            )
            .absolute()
            .top(px(-1.))
            .left(px(-1.))
            .right(px(-1.))
            .bottom(px(-1.)),
        )
}
pub(crate) fn color(value: Rgba) -> gpui::Rgba {
    gpui::rgba(u32::from_be_bytes(value.0))
}
pub(crate) fn icon(config: &Config, name: &str, size: f32) -> Div {
    div()
        .font_family(config.bar.icon_font.clone())
        .text_size(px(size))
        .child(name.to_owned())
}
pub(crate) fn row() -> Div {
    div().flex().items_center().gap(px(6.))
}
pub(crate) fn column() -> Div {
    div().flex().flex_col().gap(px(8.))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Material {
    Bar,
    Popover,
    Sidebar,
    Content,
    Card,
}
pub(crate) fn material_alpha(material: Material, blur: bool) -> f32 {
    if !blur {
        return 1.0;
    }
    match material {
        Material::Bar | Material::Card => 0.70,
        Material::Popover => 0.78,
        Material::Sidebar => 0.80,
        Material::Content => 0.92,
    }
}
pub(crate) fn material(config: &Config, material: Material) -> gpui::Rgba {
    color(config.colors.background).opacity(material_alpha(material, config.bar.blur))
}
pub(crate) fn tinted(config: &Config, material: Material, amount: f32) -> gpui::Rgba {
    let base = self::material(config, material);
    let tint = color(config.colors.foreground);
    let mix = |from: f32, to: f32| from + (to - from) * amount;
    gpui::Rgba {
        r: mix(base.r, tint.r),
        g: mix(base.g, tint.g),
        b: mix(base.b, tint.b),
        a: base.a,
    }
}
pub(crate) fn edge(config: &Config, radius: f32) -> impl IntoElement {
    let edge = color(config.colors.foreground).opacity(0.18);
    gpui::canvas(
        |bounds, _, _| bounds,
        move |_, bounds, window, _| {
            window.paint_quad(
                gpui::outline(bounds, edge, Default::default())
                    .corner_radii(crate::gpui_widgets::corner(radius)),
            );
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}
pub(crate) fn surface(config: &Config, kind: Material, radius: f32) -> Div {
    div()
        .relative()
        .rounded(crate::gpui_widgets::corner(radius))
        .child(edge(config, radius))
        .bg(material(config, kind))
        .text_color(color(config.colors.foreground))
        .font_family(text_font(config))
        .text_size(px(13.))
}
pub(crate) fn section_label(config: &Config, label: impl Into<gpui::SharedString>) -> Div {
    heading(div())
        .px(px(6.))
        .pt(px(4.))
        .text_size(px(11.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(color(config.colors.muted))
        .child(label.into())
}
pub(crate) fn separator(config: &Config) -> Div {
    div()
        .mx(px(6.))
        .my(px(4.))
        .h(px(1.))
        .bg(color(config.colors.foreground).opacity(0.12))
}
pub(crate) fn status_circle(config: &Config, glyph: &str, on: bool, size: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(crate::gpui_widgets::corner(9999.))
        .bg(if on {
            color(config.colors.accent)
        } else {
            color(config.colors.foreground).opacity(0.14)
        })
        .text_color(color(if on {
            config.colors.on_accent
        } else {
            config.colors.foreground
        }))
        .child(icon(config, glyph, size * 0.62))
}
pub(crate) fn menu_row(
    id: impl Into<gpui::ElementId>,
    config: &Config,
    leading: Option<(&str, bool)>,
    label: impl Into<gpui::SharedString>,
    trailing: Option<AnyElement>,
) -> Stateful<Div> {
    let fg = color(config.colors.foreground);
    let mut line = row()
        .id(id)
        .gap(px(10.))
        .px(px(6.))
        .py(px(5.))
        .rounded(crate::gpui_widgets::corner(7.))
        .hover(|style| style.bg(fg.opacity(0.10)))
        .active(|style| style.bg(fg.opacity(0.16)))
        .cursor_pointer();
    if let Some((glyph, on)) = leading {
        line = line.child(status_circle(config, glyph, on, 26.));
    }
    line = line.child(div().flex_1().min_w_0().truncate().child(label.into()));
    if let Some(trailing) = trailing {
        line = line.child(
            div()
                .text_size(px(12.))
                .text_color(color(config.colors.muted))
                .flex()
                .items_center()
                .child(trailing),
        );
    }
    line
}
pub(crate) fn switch(
    id: impl Into<gpui::ElementId>,
    colors: &crate::config::Colors,
    on: bool,
    message: Message,
    dispatch: &Dispatch,
) -> impl IntoElement {
    let dispatch = dispatch.clone();
    let id = id.into();
    let accent = color(colors.accent);
    let foreground = color(colors.foreground);
    let off = gpui::Rgba {
        a: foreground.a * 0.2,
        ..foreground
    };
    div()
        .id(id.clone())
        .relative()
        .flex_none()
        .w(px(32.))
        .h(px(19.))
        .rounded(crate::gpui_widgets::corner(9999.))
        .active(|style| style.opacity(0.8))
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            dispatch(message.clone(), window, cx);
        })
        .with_spring(
            id,
            gpui::SpringAnimation::new(gpui::SpringConfig::new(500., 44.7, 1.)).to(on),
            move |track, phase| {
                let t = phase.clamp(0.0..=1.0);
                let t = if pixel_look() {
                    gpui::AnimationPhase((t.0 * 4.).round() / 4.)
                } else {
                    t
                };
                track.bg(t.interpolate(off, accent)).child(
                    div()
                        .absolute()
                        .top(px(1.))
                        .left(t.interpolate(px(1.), px(14.)))
                        .size(px(17.))
                        .rounded(crate::gpui_widgets::corner(9999.))
                        .bg(gpui::rgb(0xffffff)),
                )
            },
        )
}
pub(crate) fn muted(config: &Config, text: impl Into<gpui::SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .text_color(color(config.colors.muted))
        .child(text.into())
}
pub(crate) fn module(config: &Config) -> Div {
    frame(div())
        .p(px(8.))
        .rounded(crate::gpui_widgets::corner(12.))
        .bg(color(config.colors.foreground).opacity(0.09))
}
pub(crate) fn button(
    id: impl Into<gpui::ElementId>,
    content: impl IntoElement,
    message: Message,
    dispatch: &Dispatch,
) -> Stateful<Div> {
    let dispatch = dispatch.clone();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .px(px(6.))
        .py(px(3.))
        .rounded(crate::gpui_widgets::corner(8.))
        .bg(gpui::rgba(0xffffff0a))
        .hover(|style| style.opacity(0.92))
        .active(|style| style.opacity(0.75))
        .cursor_pointer()
        .child(content)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            dispatch(message.clone(), window, cx);
        })
}
pub(crate) fn visualizer(app: &Shell, count: usize, width: f32, height: f32) -> Div {
    row().gap(px(2.)).h(px(height)).children(
        crate::media::downsample(&app.snapshot.bars, count)
            .into_iter()
            .map(|value| {
                div()
                    .w(px(width))
                    .h(px((value * height).max(2.)))
                    .rounded(crate::gpui_widgets::corner(2.))
                    .bg(color(app.config.colors.accent))
            }),
    )
}

pub(crate) fn bar_button(
    app: &Shell,
    widget: Widget,
    id: impl Into<gpui::ElementId>,
    content: impl IntoElement,
    message: Message,
    dispatch: &Dispatch,
) -> Stateful<Div> {
    let fg = color(app.config.colors.foreground);
    let open =
        crate::notch::widget_panel(widget).is_some_and(|panel| app.panels.current() == Some(panel));
    let dispatch = dispatch.clone();
    row()
        .id(id)
        .gap(px(5.))
        .px(px(8.))
        .h(px((app.config.bar.height as f32 - 6.).max(12.)))
        .rounded(crate::gpui_widgets::corner(5.))
        .when(open, |item| item.bg(fg.opacity(0.18)))
        .hover(|style| style.bg(fg.opacity(0.18)))
        .active(|style| style.bg(fg.opacity(0.26)))
        .cursor_pointer()
        .child(content)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            dispatch(message.clone(), window, cx);
        })
}

fn update_badge(config: &Config, text: String, fill: Rgba) -> Div {
    div()
        .px(px(5.))
        .rounded(crate::gpui_widgets::corner(8.))
        .bg(color(fill))
        .text_color(color(config.colors.on_accent))
        .text_size(px(11.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(text)
}

/// The red elapsed-time chip shown while recording; a click stops it.
pub(crate) fn recording_chip(app: &Shell, id: String, dispatch: &Dispatch) -> AnyElement {
    let secs = app
        .snapshot
        .recording
        .as_ref()
        .map_or(0, |recording| recording.started.elapsed().as_secs());
    button(
        id,
        format!("● {}:{:02}", secs / 60, secs % 60),
        Message::StopRecording,
        dispatch,
    )
    .text_color(color(app.config.colors.urgent))
    .into_any_element()
}

pub(crate) fn bar_widget(
    app: &Shell,
    widget: Widget,
    id: String,
    dispatch: &Dispatch,
    images: &mut Images,
) -> Option<AnyElement> {
    let c = &app.config;
    let size = c.bar.font_size * 1.25;
    let glyph = |name: &str| icon(c, name, size);
    let toggle = |panel: Panel| Message::Toggle(panel);
    Some(match widget {
        Widget::Volume => {
            let volume = app.snapshot.volume?;
            let scroll = dispatch.clone();
            let mute = dispatch.clone();
            bar_button(
                app,
                widget,
                id,
                glyph(crate::status::volume_icon(volume)),
                toggle(Panel::Sound),
                dispatch,
            )
            .on_mouse_down(MouseButton::Middle, move |_, window, cx| {
                mute(Message::VolumeMute, window, cx)
            })
            .on_scroll(move |event, window, cx| {
                let delta = event.delta.pixel_delta(px(16.)).y;
                if delta != px(0.) {
                    scroll(
                        Message::VolumeStep(if delta > px(0.) { 5 } else { -5 }),
                        window,
                        cx,
                    );
                }
            })
            .into_any_element()
        }
        Widget::Network => {
            let network = app.snapshot.network.as_ref()?;
            bar_button(
                app,
                widget,
                id,
                glyph(crate::status::network_icon(network)),
                toggle(Panel::Wifi),
                dispatch,
            )
            .into_any_element()
        }
        Widget::Battery => {
            let battery = app.snapshot.battery.as_ref()?;
            bar_button(
                app,
                widget,
                id,
                glyph(crate::status::battery_icon(battery)),
                toggle(Panel::Battery),
                dispatch,
            )
            .into_any_element()
        }
        Widget::Updates => {
            use rune_services::updates::Run;
            let updates = &app.snapshot.updates;
            let total = crate::system_panel::pending_total(updates);
            let content = match &updates.run {
                Run::Running => {
                    let still = c.animations.reduce_motion || !c.animations.enabled;
                    let pixel = pixel_look();
                    let spinner = glyph("sync");
                    let spinner =
                        if still {
                            spinner.into_any_element()
                        } else {
                            spinner
                                .with_animation(
                                    "updates-running",
                                    gpui::Animation::new(std::time::Duration::from_millis(
                                        if pixel { 1000 } else { 1600 },
                                    ))
                                    .repeat(),
                                    move |icon, t| {
                                        icon.opacity(if pixel {
                                            if t < 0.5 { 1. } else { 0.3 }
                                        } else {
                                            1. - 0.6 * (t * std::f32::consts::PI).sin()
                                        })
                                    },
                                )
                                .into_any_element()
                        };
                    row().child(spinner)
                }
                Run::Failed(_) => row().child(glyph("system_update_alt")).child(update_badge(
                    c,
                    "!".into(),
                    c.colors.urgent,
                )),
                Run::Idle if total > 0 => row()
                    .child(glyph("system_update_alt"))
                    .child(update_badge(c, total.to_string(), c.colors.accent)),
                Run::Idle => row()
                    .text_color(color(c.colors.muted))
                    .child(glyph("system_update_alt")),
            };
            bar_button(app, widget, id, content, toggle(Panel::Updates), dispatch)
                .into_any_element()
        }
        Widget::Stats => {
            let stats = app.snapshot.stats.as_ref()?;
            let content = |name: &str, label: String| row().child(glyph(name)).child(label);
            let mut items = row()
                .child(content("memory", format!("{}%", stats.cpu)))
                .child(content("memory_alt", format!("{}%", stats.ram)));
            if let Some(temp) = stats.temp {
                items = items.child(content("thermostat", format!("{temp}°")));
            }
            bar_button(app, widget, id, items, toggle(Panel::System), dispatch).into_any_element()
        }
        Widget::Bluetooth => {
            let bt = &app.snapshot.bt;
            if !bt.available {
                return None;
            }
            let connected = bt.devices.iter().any(|d| d.connected);
            let name = if !bt.powered {
                "bluetooth_disabled"
            } else if connected {
                "bluetooth_connected"
            } else {
                "bluetooth"
            };
            bar_button(
                app,
                widget,
                id,
                glyph(name),
                toggle(Panel::Bluetooth),
                dispatch,
            )
            .into_any_element()
        }
        Widget::Weather => {
            let weather = app.snapshot.weather.as_ref()?;
            bar_button(
                app,
                widget,
                id,
                row()
                    .gap(px(4.))
                    .child(glyph(rune_services::weather::icon(
                        weather.current.code,
                        weather.current.is_day,
                    )))
                    .child(format!("{:.0}°", weather.current.temperature)),
                toggle(Panel::Weather),
                dispatch,
            )
            .into_any_element()
        }
        Widget::Notifications => bar_button(
            app,
            widget,
            id,
            glyph(if app.snapshot.notif.dnd {
                "notifications_off"
            } else {
                "notifications"
            }),
            toggle(Panel::Notifications),
            dispatch,
        )
        .into_any_element(),
        Widget::QuickSettings => bar_button(
            app,
            widget,
            id,
            glyph("toggle_on"),
            toggle(Panel::Quick),
            dispatch,
        )
        .into_any_element(),
        Widget::Power => bar_button(
            app,
            widget,
            id,
            glyph("power_settings_new"),
            toggle(Panel::Power),
            dispatch,
        )
        .into_any_element(),
        Widget::Capture if app.snapshot.recording.is_some() => recording_chip(app, id, dispatch),
        Widget::Capture => bar_button(
            app,
            widget,
            id,
            glyph("photo_camera"),
            toggle(Panel::Capture),
            dispatch,
        )
        .into_any_element(),
        Widget::Media => {
            let media = app.snapshot.media.as_ref()?;
            let art = media
                .art
                .as_ref()
                .and_then(|bytes| images.encoded("media", bytes));
            let lead = match art {
                Some(source) => img(source)
                    .size(px((c.bar.height as f32 - 12.).max(12.)))
                    .rounded(crate::gpui_widgets::corner(4.))
                    .into_any_element(),
                None => icon(c, "music_note", size * 0.9).into_any_element(),
            };
            bar_button(
                app,
                widget,
                id,
                row()
                    .gap(px(6.))
                    .min_w_0()
                    .child(lead)
                    .child(crate::notch::media_label(media, c.window_title.max_chars)),
                toggle(Panel::Media),
                dispatch,
            )
            .into_any_element()
        }
        Widget::Tray => {
            let fg = color(c.colors.foreground);
            let mut tray = row().id(id).gap(px(2.));
            for item in &app.snapshot.tray {
                let picture = item
                    .image
                    .as_ref()
                    .and_then(|image| images.source(&format!("tray:{}", item.key), image));
                let picture = match picture {
                    Some(source) => img(source).size(px(size)).into_any_element(),
                    None => icon(c, "apps", size).into_any_element(),
                };
                let mut entry = div()
                    .id(format!("tray:{}", item.key))
                    .px(px(5.))
                    .py(px(3.))
                    .rounded(crate::gpui_widgets::corner(5.))
                    .hover(|style| style.bg(fg.opacity(0.18)))
                    .active(|style| style.bg(fg.opacity(0.26)))
                    .cursor_pointer()
                    .child(picture);
                for (mouse, click) in [
                    (MouseButton::Left, crate::tray::Click::Left),
                    (MouseButton::Middle, crate::tray::Click::Middle),
                    (MouseButton::Right, crate::tray::Click::Right),
                ] {
                    let key = item.key.clone();
                    let dispatch = dispatch.clone();
                    entry = entry.on_mouse_down(mouse, move |_, window, cx| {
                        dispatch(Message::TrayClick(key.clone(), click), window, cx)
                    });
                }
                tray = tray.child(entry);
            }
            tray.into_any_element()
        }
        Widget::Workspaces | Widget::WindowTitle | Widget::Clock | Widget::Settings => return None,
    })
}

/// Report natural panel bounds after layout, once per actual size change.
/// Window operations may synchronously render; deferring avoids re-entering a
/// shell update while a view's shared state is still borrowed.
pub(crate) fn measure_panel(
    previous: std::rc::Rc<std::cell::Cell<Option<(u32, u32)>>>,
    dispatch: Dispatch,
) -> impl IntoElement {
    gpui::canvas(
        move |bounds, window, cx| {
            let dimensions = (
                f32::from(bounds.size.width).ceil() as u32,
                f32::from(bounds.size.height).ceil() as u32,
            );
            if previous.get() != Some(dimensions) {
                previous.set(Some(dimensions));
                window.defer(cx, move |window, cx| {
                    dispatch(Message::PanelSize(bounds.size), window, cx)
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
    .size_full()
}

/// Lay out content at its intrinsic size before the compositor has configured
/// the initially tiny panel window. The outer window can then follow measurement
/// instead of squeezing its children into the initial 1×1 bounds forever.
pub(crate) fn natural_content(child: impl IntoElement) -> NaturalContent {
    NaturalContent {
        child: child.into_any_element(),
    }
}
pub(crate) struct NaturalContent {
    child: gpui::AnyElement,
}
impl gpui::Element for NaturalContent {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<gpui::ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        let size = self.child.layout_as_root(
            gpui::size(
                gpui::AvailableSpace::MaxContent,
                gpui::AvailableSpace::MaxContent,
            ),
            window,
            cx,
        );
        let mut style = gpui::Style::default();
        style.size.width = size.width.into();
        style.size.height = size.height.into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.child.prepaint_at(bounds.origin, window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<gpui::Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bar_shadow_keeps_its_softness_in_the_pixel_look() {
        let bar = crate::config::BarConfig {
            shadow: true,
            ..Default::default()
        };
        set_look(crate::config::Look::Pixel, crate::config::PixelFont::Tiny5);
        let mut shadowed = bar_outline(div(), &bar);
        set_look(crate::config::Look::Smooth, crate::config::PixelFont::Tiny5);
        let blurs: Vec<_> = shadowed
            .style()
            .box_shadow
            .iter()
            .flatten()
            .map(|shadow| shadow.blur_radius)
            .collect();
        assert_eq!(blurs, [px(bar.shadow_blur)]);
    }
    #[test]
    fn material_alpha_blur_off_is_opaque_enough() {
        for material in [
            Material::Bar,
            Material::Popover,
            Material::Sidebar,
            Material::Content,
            Material::Card,
        ] {
            assert!(material_alpha(material, false) >= 0.96);
        }
        assert_eq!(material_alpha(Material::Bar, true), 0.70);
    }
}
