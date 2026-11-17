//! A pointer-driven slider with an explicit release event for media seeking.
use gpui::{
    Bounds, Context, EventEmitter, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Render,
    Window, canvas, div, prelude::*, px,
};

fn value_at(x: f32, width: f32, range: (f32, f32)) -> Option<f32> {
    if !x.is_finite()
        || !width.is_finite()
        || width <= 0.
        || !range.0.is_finite()
        || !range.1.is_finite()
        || range.1 <= range.0
    {
        return None;
    }
    Some(range.0 + (x / width).clamp(0., 1.) * (range.1 - range.0))
}

fn value_at_y(y: f32, height: f32, range: (f32, f32)) -> Option<f32> {
    value_at(height - y, height, range)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SliderEvent {
    Change(f32),
    Release,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pill {
    pub glyph: gpui::SharedString,
    pub font: gpui::SharedString,
    pub fill: gpui::Rgba,
    pub ink: gpui::Rgba,
}

pub(crate) struct Slider {
    range: (f32, f32),
    value: f32,
    dragging: bool,
    bounds: Option<Bounds<Pixels>>,
    accent: gpui::Rgba,
    pill: Option<Pill>,
    /// Vertical, filled from this value (e.g. 0 dB) to the current one.
    vertical: Option<f32>,
}
impl EventEmitter<SliderEvent> for Slider {}
impl Slider {
    pub fn new(range: (f32, f32), value: f32, accent: gpui::Rgba) -> Self {
        Self {
            range,
            value,
            dragging: false,
            bounds: None,
            accent,
            pill: None,
            vertical: None,
        }
    }
    /// Draws the slider upright, filled outward from `center`; double-click returns to it.
    pub fn set_vertical(&mut self, center: f32) {
        self.vertical = Some(center);
    }
    pub fn set_pill(&mut self, pill: Option<Pill>) {
        self.pill = pill;
    }
    pub fn sync(&mut self, range: (f32, f32), value: f32, accent: gpui::Rgba) {
        self.range = range;
        self.accent = accent;
        if !self.dragging {
            self.value = if range.0.is_finite() && range.1.is_finite() && range.1 > range.0 {
                value.clamp(range.0, range.1)
            } else {
                range.0
            };
        }
    }
    fn move_to(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let value = if self.vertical.is_some() {
            value_at_y(
                f32::from(position.y - bounds.top()),
                f32::from(bounds.size.height),
                self.range,
            )
        } else {
            value_at(
                f32::from(position.x - bounds.left()),
                f32::from(bounds.size.width),
                self.range,
            )
        };
        if let Some(value) = value {
            self.value = value;
            cx.emit(SliderEvent::Change(value));
            cx.notify();
        }
    }
}
impl Render for Slider {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fraction_of = |value: f32| {
            if self.range.1 > self.range.0 {
                ((value - self.range.0) / (self.range.1 - self.range.0)).clamp(0., 1.)
            } else {
                0.
            }
        };
        let fraction = fraction_of(self.value);
        let entity = cx.entity();
        let drawing = entity.clone();
        let pixel = crate::gpui_widgets::pixel_look();
        let height = match (&self.pill, pixel) {
            (Some(_), true) => 10.,
            (Some(_), false) => 24.,
            (None, _) => 20.,
        };
        let mut slider = div()
            .id("slider")
            .cursor_pointer()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    if let Some(center) = this.vertical.filter(|_| event.click_count == 2) {
                        this.value = center;
                        cx.emit(SliderEvent::Change(center));
                        cx.notify();
                        return;
                    }
                    this.dragging = true;
                    this.move_to(event.position, cx);
                }),
            );
        slider = if let Some(center) = self.vertical {
            let (low, high) = {
                let c = fraction_of(center);
                (c.min(fraction), c.max(fraction))
            };
            vertical(slider, (low, high), fraction, self.accent, pixel)
        } else {
            slider.w_full().h(px(height))
        };
        slider = match &self.pill {
            _ if self.vertical.is_some() => slider,
            Some(pill) if pixel => slider
                .border_1()
                .border_color(gpui::rgba(0x8080807a))
                .bg(pill.fill.opacity(0.08))
                .child(
                    div()
                        .absolute()
                        .left(px(1.))
                        .top(px(1.))
                        .bottom(px(1.))
                        .w(gpui::relative(fraction))
                        .map(|bar| crate::gpui_widgets::fill(bar, pill.fill)),
                ),
            Some(pill) => slider
                .rounded(crate::gpui_widgets::corner(9999.))
                .overflow_hidden()
                .bg(pill.fill.opacity(0.18))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .min_w(px(height))
                        .w(gpui::relative(fraction))
                        .rounded(crate::gpui_widgets::corner(9999.))
                        .map(|bar| crate::gpui_widgets::fill(bar, pill.fill)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(5.))
                        .top(px(3.))
                        .font_family(pill.font.clone())
                        .text_size(px(16.))
                        .line_height(px(18.))
                        .text_color(pill.ink)
                        .child(pill.glyph.clone()),
                ),
            None => slider
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(7.))
                        .h(px(6.))
                        .rounded(crate::gpui_widgets::corner(9999.))
                        .bg(self.accent.opacity(0.2)),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(7.))
                        .h(px(6.))
                        .w(gpui::relative(fraction))
                        .rounded(crate::gpui_widgets::corner(9999.))
                        .map(|bar| crate::gpui_widgets::fill(bar, self.accent)),
                )
                .child(
                    div()
                        .absolute()
                        .left(gpui::relative(fraction))
                        .ml(px(-5.))
                        .top(px(5.))
                        .size(px(10.))
                        .rounded(crate::gpui_widgets::corner(9999.))
                        .bg(self.accent)
                        .border_1()
                        .border_color(gpui::rgba(0xffffff80)),
                ),
        };
        let track = slider.child(
            canvas(
                move |bounds, _, cx| {
                    drawing.update(cx, |this, _| this.bounds = Some(bounds));
                },
                move |_, _, window, _| {
                    let moved = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase.bubble() {
                            moved.update(cx, |this, cx| {
                                if this.dragging {
                                    this.move_to(event.position, cx);
                                }
                            });
                        }
                    });
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase.bubble() && event.button == MouseButton::Left {
                            entity.update(cx, |this, cx| {
                                if this.dragging {
                                    this.dragging = false;
                                    cx.emit(SliderEvent::Release);
                                    cx.notify();
                                }
                            });
                        }
                    });
                },
            )
            .size_full(),
        );
        match self.pill.as_ref().filter(|_| pixel) {
            Some(pill) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .w_full()
                .text_color(pill.fill)
                .child(
                    div()
                        .font_family(pill.font.clone())
                        .text_size(px(16.))
                        .child(pill.glyph.clone()),
                )
                .child(div().flex_1().child(track))
                .child(
                    div()
                        .min_w(px(24.))
                        .flex()
                        .justify_end()
                        .child(format!("{:.0}", self.value)),
                )
                .into_any_element(),
            None => track.into_any_element(),
        }
    }
}

/// Upright track: `lit` is the filled span and `knob` the value, both as
/// fractions from the bottom.
fn vertical(
    slider: gpui::Stateful<gpui::Div>,
    lit: (f32, f32),
    knob: f32,
    accent: gpui::Rgba,
    pixel: bool,
) -> gpui::Stateful<gpui::Div> {
    let slider = slider.w(px(28.)).h_full();
    if pixel {
        const CELLS: u16 = 24;
        let cells = (0..CELLS).rev().map(|cell| {
            let middle = (f32::from(cell) + 0.5) / f32::from(CELLS);
            let on = (lit.0..=lit.1).contains(&middle);
            div()
                .flex_1()
                .w(px(14.))
                .bg(if on { accent } else { gpui::rgba(0xffffff14) })
        });
        return slider
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .children(cells),
            )
            .child(crate::gpui_widgets::frame(
                div()
                    .absolute()
                    .left(px(4.))
                    .top(gpui::relative(1. - knob))
                    .mt(px(-4.))
                    .w(px(20.))
                    .h(px(8.))
                    .bg(gpui::white()),
            ));
    }
    slider
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(12.))
                .w(px(4.))
                .rounded(px(2.))
                .bg(accent.opacity(0.2)),
        )
        .child(
            div()
                .absolute()
                .left(px(12.))
                .w(px(4.))
                .top(gpui::relative(1. - lit.1))
                .h(gpui::relative(lit.1 - lit.0))
                .rounded(px(2.))
                .bg(accent),
        )
        .child(
            div()
                .absolute()
                .left(px(6.))
                .top(gpui::relative(1. - knob))
                .mt(px(-8.))
                .size(px(16.))
                .rounded(px(8.))
                .bg(gpui::white())
                .shadow_md(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outside_track_clamps_and_zero_width_does_not_change_value() {
        assert_eq!(value_at(-20., 100., (0., 100.)), Some(0.));
        assert_eq!(value_at(150., 100., (0., 100.)), Some(100.));
        assert_eq!(value_at(25., 100., (10., 50.)), Some(20.));
        assert_eq!(value_at(20., 0., (0., 100.)), None);
        assert_eq!(value_at(f32::NAN, 100., (0., 100.)), None);
    }
    #[test]
    fn vertical_tracks_put_the_maximum_at_the_top() {
        assert_eq!(value_at_y(0., 100., (-12., 12.)), Some(12.));
        assert_eq!(value_at_y(100., 100., (-12., 12.)), Some(-12.));
        assert_eq!(value_at_y(50., 100., (-12., 12.)), Some(0.));
        assert_eq!(value_at_y(10., 0., (-12., 12.)), None);
        assert_eq!(value_at_y(f32::NAN, 100., (-12., 12.)), None);
    }
    #[test]
    #[ignore = "opens a native slider; requires Wayland"]
    fn drag_outside_track_emits_changes_and_one_release() {
        use gpui::{
            AppContext, InputEvent, MouseDownEvent, PlatformInput, QuitMode, WindowOptions, point,
        };
        use std::{cell::RefCell, rc::Rc, time::Duration};
        gpui_platform::application()
            .with_quit_mode(QuitMode::Explicit)
            .run(|cx| {
                let slider = cx.new(|_| Slider::new((0., 100.), 50., gpui::rgb(0x89b4fa)));
                let seen = Rc::new(RefCell::new(Vec::new()));
                let observed = seen.clone();
                let subscription = cx.subscribe(&slider, move |_, event: &SliderEvent, _| {
                    observed.borrow_mut().push(*event)
                });
                let root = slider.clone();
                let handle: gpui::AnyWindowHandle = cx
                    .open_window(WindowOptions::default(), move |_, _| root)
                    .unwrap()
                    .into();
                cx.spawn(async move |cx| {
                    let _subscription = subscription;
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    cx.update(|cx| {
                        let bounds = slider.read(cx).bounds.expect("slider was painted");
                        let start =
                            point(bounds.left() + bounds.size.width * 0.25, bounds.center().y);
                        let outside = point(bounds.right() + px(20.), bounds.center().y);
                        handle
                            .update(cx, |_, window, cx| {
                                window.dispatch_event(
                                    MouseDownEvent {
                                        position: start,
                                        button: MouseButton::Left,
                                        click_count: 1,
                                        ..Default::default()
                                    }
                                    .to_platform_input(),
                                    cx,
                                );
                                window.dispatch_event(
                                    MouseMoveEvent {
                                        position: outside,
                                        pressed_button: Some(MouseButton::Left),
                                        ..Default::default()
                                    }
                                    .to_platform_input(),
                                    cx,
                                );
                                window.dispatch_event(
                                    MouseUpEvent {
                                        position: outside,
                                        button: MouseButton::Left,
                                        click_count: 1,
                                        ..Default::default()
                                    }
                                    .to_platform_input(),
                                    cx,
                                );
                                window.dispatch_event(
                                    PlatformInput::MouseUp(MouseUpEvent {
                                        position: outside,
                                        button: MouseButton::Left,
                                        ..Default::default()
                                    }),
                                    cx,
                                );
                            })
                            .unwrap();
                    });
                    cx.background_executor()
                        .timer(Duration::from_millis(20))
                        .await;
                    cx.update(|cx| {
                        let events = seen.borrow();
                        assert!(
                            events.iter().any(
                                |e| matches!(e, SliderEvent::Change(v) if (*v - 25.).abs() < 0.1)
                            ),
                            "missing initial pointer value: {events:?}"
                        );
                        assert!(
                            events
                                .iter()
                                .any(|e| matches!(e, SliderEvent::Change(v) if *v == 100.)),
                            "outside drag did not clamp: {events:?}"
                        );
                        assert_eq!(
                            events
                                .iter()
                                .filter(|e| matches!(e, SliderEvent::Release))
                                .count(),
                            1
                        );
                        cx.quit();
                    });
                })
                .detach();
            });
    }
}
