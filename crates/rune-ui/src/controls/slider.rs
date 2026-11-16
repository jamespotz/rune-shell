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
        }
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
        if let Some(value) = value_at(
            f32::from(position.x - bounds.left()),
            f32::from(bounds.size.width),
            self.range,
        ) {
            self.value = value;
            cx.emit(SliderEvent::Change(value));
            cx.notify();
        }
    }
}
impl Render for Slider {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fraction = if self.range.1 > self.range.0 {
            ((self.value - self.range.0) / (self.range.1 - self.range.0)).clamp(0., 1.)
        } else {
            0.
        };
        let entity = cx.entity();
        let drawing = entity.clone();
        let height = if self.pill.is_some() { 24. } else { 20. };
        let mut slider = div()
            .id("slider")
            .w_full()
            .h(px(height))
            .cursor_pointer()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    this.dragging = true;
                    this.move_to(event.position, cx);
                }),
            );
        slider = match &self.pill {
            Some(pill) => slider
                .rounded_full()
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
                        .rounded_full()
                        .bg(pill.fill),
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
                        .rounded_full()
                        .bg(self.accent.opacity(0.2)),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(7.))
                        .h(px(6.))
                        .w(gpui::relative(fraction))
                        .rounded_full()
                        .bg(self.accent),
                )
                .child(
                    div()
                        .absolute()
                        .left(gpui::relative(fraction))
                        .ml(px(-5.))
                        .top(px(5.))
                        .size(px(10.))
                        .rounded_full()
                        .bg(self.accent)
                        .border_1()
                        .border_color(gpui::rgba(0xffffff80)),
                ),
        };
        slider.child(
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
        )
    }
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
