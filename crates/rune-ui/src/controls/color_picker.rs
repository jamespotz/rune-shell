//! HSV colour picker with alpha: saturation/brightness square, hue and alpha strips, hex field.
use crate::{
    config::Rgba,
    controls::text_input::{InputEvent, TextInput},
};
use gpui::{
    Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, MouseButton,
    MouseMoveEvent, MouseUpEvent, Pixels, Render, Window, canvas, div, prelude::*, px,
};

/// `[hue 0..360, saturation, value, alpha]` from a colour.
pub fn rgba_to_hsva(c: Rgba) -> [f32; 4] {
    let [r, g, b, a] = c.0.map(|channel| f32::from(channel) / 255.);
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let hue = if delta == 0. {
        0.
    } else if max == r {
        60. * ((g - b) / delta).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    let saturation = if max == 0. { 0. } else { delta / max };
    [hue, saturation, max, a]
}

/// A colour from hue (wrapping) and saturation, value and alpha (each clamped to 0..=1).
pub fn hsva_to_rgba(h: f32, s: f32, v: f32, a: f32) -> Rgba {
    let unit = |x: f32| if x.is_finite() { x.clamp(0., 1.) } else { 0. };
    let h = if h.is_finite() {
        h.rem_euclid(360.)
    } else {
        0.
    };
    let (s, v) = (unit(s), unit(v));
    let chroma = v * s;
    let x = chroma * (1. - ((h / 60.).rem_euclid(2.) - 1.).abs());
    let (r, g, b) = match h {
        h if h < 60. => (chroma, x, 0.),
        h if h < 120. => (x, chroma, 0.),
        h if h < 180. => (0., chroma, x),
        h if h < 240. => (0., x, chroma),
        h if h < 300. => (x, 0., chroma),
        _ => (chroma, 0., x),
    };
    let m = v - chroma;
    // Float-to-int `as` saturates, and every input is already clamped to 0..=255.
    let byte = |channel: f32| (channel * 255.).round().clamp(0., 255.) as u8;
    Rgba([byte(r + m), byte(g + m), byte(b + m), byte(unit(a))])
}

/// The colour typed into the hex field, when it parses and differs from `current`.
fn hex_edit(current: Rgba, text: &str) -> Option<Rgba> {
    Rgba::parse(text.trim()).filter(|value| *value != current)
}

/// A drag only continues while the left button is held, so a missed release cannot leave it stuck.
fn drag_continues(pressed: Option<MouseButton>) -> bool {
    pressed == Some(MouseButton::Left)
}

/// A grey checkerboard that shows through transparent colours.
pub(crate) fn checkerboard(columns: usize, rows: usize, cell: f32) -> gpui::Div {
    div().flex().flex_col().children((0..rows).map(move |row| {
        div().flex().children((0..columns).map(move |column| {
            div()
                .flex_none()
                .size(px(cell))
                .bg(if (row + column) % 2 == 0 {
                    gpui::rgb(0x9a9a9a)
                } else {
                    gpui::rgb(0x5f5f5f)
                })
        }))
    }))
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ColorPickerEvent {
    Change(Rgba),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Square,
    Hue,
    Alpha,
}

const WIDTH: f32 = 240.;
const SQUARE_HEIGHT: f32 = 150.;
const STRIP_HEIGHT: f32 = 14.;
const HANDLE: f32 = 12.;

pub(crate) struct ColorPicker {
    hsva: [f32; 4],
    /// Colour when the picker opened, shown beside the new one.
    original: Rgba,
    /// Last colour emitted or synced, so unchanged values are not re-sent.
    current: Rgba,
    drag: Option<Region>,
    /// Region the arrow keys nudge.
    focused: Region,
    bounds: [Option<Bounds<Pixels>>; 3],
    hex: Entity<TextInput>,
    focus: FocusHandle,
    colors: crate::config::Colors,
}

impl EventEmitter<ColorPickerEvent> for ColorPicker {}

impl ColorPicker {
    pub fn new(initial: Rgba, colors: crate::config::Colors, cx: &mut Context<Self>) -> Self {
        let hex = cx.new(|cx| {
            let mut input = TextInput::new(&String::from(initial), "#rrggbbaa", cx);
            input.set_text_size(13.);
            input
        });
        cx.subscribe(&hex, |this: &mut Self, _, event, cx| {
            if let InputEvent::Changed(text) = event
                && let Some(value) = hex_edit(this.current, text)
            {
                this.hsva = rgba_to_hsva(value);
                this.current = value;
                cx.emit(ColorPickerEvent::Change(value));
                cx.notify();
            }
        })
        .detach();
        Self {
            hsva: rgba_to_hsva(initial),
            original: initial,
            current: initial,
            drag: None,
            focused: Region::Square,
            bounds: [None; 3],
            hex,
            focus: cx.focus_handle(),
            colors,
        }
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Follows edits made elsewhere; keeps the hue while the colour is unchanged.
    pub fn sync(&mut self, value: Rgba, colors: crate::config::Colors, cx: &mut Context<Self>) {
        self.colors = colors;
        if value != self.current && self.drag.is_none() {
            self.hsva = rgba_to_hsva(value);
            self.current = value;
            self.hex.update(cx, |input, cx| {
                input.sync(&String::from(value), "#rrggbbaa", cx)
            });
            cx.notify();
        }
    }

    fn set_hsva(&mut self, hsva: [f32; 4], cx: &mut Context<Self>) {
        self.hsva = hsva;
        let [h, s, v, a] = hsva;
        let value = hsva_to_rgba(h, s, v, a);
        if value != self.current {
            self.current = value;
            self.hex.update(cx, |input, cx| {
                input.sync(&String::from(value), "#rrggbbaa", cx)
            });
            cx.emit(ColorPickerEvent::Change(value));
        }
        cx.notify();
    }

    fn index(region: Region) -> usize {
        match region {
            Region::Square => 0,
            Region::Hue => 1,
            Region::Alpha => 2,
        }
    }

    fn move_to(&mut self, region: Region, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(Some(bounds)) = self.bounds.get(Self::index(region)) else {
            return;
        };
        let fraction = |offset: Pixels, length: Pixels| {
            let length = f32::from(length);
            if length > 0. {
                (f32::from(offset) / length).clamp(0., 1.)
            } else {
                0.
            }
        };
        let x = fraction(position.x - bounds.left(), bounds.size.width);
        let y = fraction(position.y - bounds.top(), bounds.size.height);
        let [h, s, v, a] = self.hsva;
        let next = match region {
            Region::Square => [h, x, 1. - y, a],
            Region::Hue => [x * 360., s, v, a],
            Region::Alpha => [h, s, v, x],
        };
        self.set_hsva(next, cx);
    }

    fn nudge(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        let [h, s, v, a] = self.hsva;
        let next = match self.focused {
            Region::Square => [h, (s + dx).clamp(0., 1.), (v - dy).clamp(0., 1.), a],
            Region::Hue => [(h + dx * 360.).rem_euclid(360.), s, v, a],
            Region::Alpha => [h, s, v, (a + dx).clamp(0., 1.)],
        };
        self.set_hsva(next, cx);
    }

    fn region(&self, region: Region, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let drawing = cx.entity();
        div()
            .id(match region {
                Region::Square => "picker-square",
                Region::Hue => "picker-hue",
                Region::Alpha => "picker-alpha",
            })
            .relative()
            .w_full()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.drag = Some(region);
                    this.focused = region;
                    window.focus(&this.focus, cx);
                    this.move_to(region, event.position, cx);
                }),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        drawing.update(cx, |this, _| {
                            if let Some(slot) = this.bounds.get_mut(Self::index(region)) {
                                *slot = Some(bounds);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}

fn handle(fill: gpui::Rgba) -> gpui::Div {
    div()
        .absolute()
        .size(px(HANDLE))
        .rounded(crate::gpui_widgets::corner(9999.))
        .border_2()
        .border_color(gpui::rgb(0xffffff))
        .shadow_sm()
        .bg(fill)
}

impl Render for ColorPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let [h, s, v, a] = self.hsva;
        let current = crate::gpui_widgets::color(self.current);
        let hue = crate::gpui_widgets::color(hsva_to_rgba(h, 1., 1., 1.));
        let opaque = crate::gpui_widgets::color(hsva_to_rgba(h, s, v, 1.));
        let white = gpui::rgb(0xffffff);
        let black = gpui::rgb(0x000000);
        let square = self
            .region(Region::Square, cx)
            .h(px(SQUARE_HEIGHT))
            .rounded(crate::gpui_widgets::corner(8.))
            .overflow_hidden()
            .bg(hue)
            .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                90.,
                gpui::linear_color_stop(white, 0.),
                gpui::linear_color_stop(white.opacity(0.), 1.),
            )))
            .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(black.opacity(0.), 0.),
                gpui::linear_color_stop(black, 1.),
            )))
            .child(
                handle(opaque)
                    .left(gpui::relative(s))
                    .top(gpui::relative(1. - v))
                    .ml(px(-HANDLE / 2.))
                    .mt(px(-HANDLE / 2.)),
            );
        let strip_handle = |fraction: f32, fill: gpui::Rgba| {
            handle(fill)
                .left(gpui::relative(fraction))
                .ml(px(-HANDLE / 2.))
                .top(px((STRIP_HEIGHT - HANDLE) / 2.))
        };
        let hue_strip = self
            .region(Region::Hue, cx)
            .h(px(STRIP_HEIGHT))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .rounded(crate::gpui_widgets::corner(9999.))
                    .overflow_hidden()
                    .children((0..6u8).map(|segment| {
                        let at = |step: u8| {
                            crate::gpui_widgets::color(hsva_to_rgba(
                                f32::from(step) * 60.,
                                1.,
                                1.,
                                1.,
                            ))
                        };
                        div().flex_1().h_full().bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(at(segment), 0.),
                            gpui::linear_color_stop(at(segment + 1), 1.),
                        ))
                    })),
            )
            .child(strip_handle(h / 360., hue));
        let alpha_strip = self
            .region(Region::Alpha, cx)
            .h(px(STRIP_HEIGHT))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(crate::gpui_widgets::corner(9999.))
                    .overflow_hidden()
                    .child(checkerboard(36, 2, STRIP_HEIGHT / 2.))
                    .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(opaque.opacity(0.), 0.),
                        gpui::linear_color_stop(opaque, 1.),
                    ))),
            )
            .child(strip_handle(a, current));
        let preview = |id: &'static str, fill: gpui::Rgba| {
            div()
                .id(id)
                .relative()
                .size(px(26.))
                .rounded(crate::gpui_widgets::corner(6.))
                .overflow_hidden()
                .border_1()
                .border_color(gpui::rgba(0xffffff33))
                .child(checkerboard(4, 4, 6.5))
                .child(div().absolute().inset_0().bg(fill))
        };
        let pointer = cx.entity();
        let release = pointer.clone();
        crate::gpui_widgets::frame(div())
            .id("color-picker")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let typing = this.hex.read(cx).focus_handle(cx).is_focused(window);
                let step = if event.keystroke.modifiers.shift {
                    0.1
                } else {
                    0.01
                };
                match event.keystroke.key.as_str() {
                    "escape" => cx.emit(ColorPickerEvent::Close),
                    _ if typing => return,
                    "left" => this.nudge(-step, 0., cx),
                    "right" => this.nudge(step, 0., cx),
                    "up" => this.nudge(0., -step, cx),
                    "down" => this.nudge(0., step, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .occlude()
            .w(px(WIDTH + 24.))
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(crate::gpui_widgets::corner(12.))
            .bg(crate::gpui_widgets::color(self.colors.background))
            .text_color(crate::gpui_widgets::color(self.colors.foreground))
            .when(!crate::gpui_widgets::pixel_look(), |picker| {
                picker
                    .border_1()
                    .border_color(gpui::rgba(0xffffff26))
                    .shadow_lg()
            })
            .child(square)
            .child(hue_strip)
            .child(alpha_strip)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(preview(
                        "picker-old",
                        crate::gpui_widgets::color(self.original),
                    ))
                    .child(preview("picker-new", current))
                    .child(
                        div()
                            .flex_1()
                            .rounded(crate::gpui_widgets::corner(6.))
                            .bg(gpui::rgba(0xffffff12))
                            .child(self.hex.clone()),
                    ),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase.bubble() {
                                pointer.update(cx, |this, cx| {
                                    if !drag_continues(event.pressed_button) {
                                        this.drag = None;
                                    } else if let Some(region) = this.drag {
                                        this.move_to(region, event.position, cx);
                                    }
                                });
                            }
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase.bubble() && event.button == MouseButton::Left {
                                release.update(cx, |this, _| this.drag = None);
                            }
                        });
                    },
                )
                .absolute()
                .size_0(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsva_round_trips() {
        for c in [
            Rgba([0x89, 0xb4, 0xfa, 0xff]),
            Rgba([0, 0, 0, 0]),
            Rgba([255, 255, 255, 128]),
            Rgba([0x80, 0x80, 0x80, 0x33]),
            Rgba([0xff, 0x7b, 0x81, 0xff]),
        ] {
            let [h, s, v, a] = rgba_to_hsva(c);
            assert_eq!(hsva_to_rgba(h, s, v, a), c);
        }
        assert_eq!(hsva_to_rgba(360., 1., 1., 1.), hsva_to_rgba(0., 1., 1., 1.));
        assert_eq!(hsva_to_rgba(-120., 1., 1., 1.), Rgba([0, 0, 255, 255]));
        assert_eq!(hsva_to_rgba(f32::NAN, 2., -1., 9.), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn drag_ends_without_the_left_button() {
        assert!(drag_continues(Some(MouseButton::Left)));
        assert!(!drag_continues(None));
        assert!(!drag_continues(Some(MouseButton::Right)));
    }

    #[test]
    fn partial_hex_keeps_color() {
        assert_eq!(hex_edit(Rgba([1, 2, 3, 4]), "#12"), None);
        assert_eq!(hex_edit(Rgba([1, 2, 3, 4]), "#01020304"), None);
        assert_eq!(
            hex_edit(Rgba([1, 2, 3, 4]), " #ff000080 "),
            Some(Rgba([255, 0, 0, 128]))
        );
    }
}
