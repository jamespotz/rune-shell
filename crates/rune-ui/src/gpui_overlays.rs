//! Notification stack, OSD, and click-outside catcher rendered by GPUI.
use crate::{
    App as Shell, Message, Surface,
    gpui_images::Images,
    gpui_notch::Dispatch,
    gpui_widgets::{color, icon},
};
use gpui::{Context, Entity, MouseButton, Render, Window, div, prelude::*, px};

pub(crate) struct Overlay {
    shell: Entity<Shell>,
    surface: Surface,
    dispatch: Dispatch,
    images: Images,
    swipe: Option<Swipe>,
    _subscription: gpui::Subscription,
}

/// A toast being dragged sideways.
struct Swipe {
    id: u32,
    origin: f32,
    offset: f32,
    started: std::time::Instant,
}

impl Swipe {
    fn dismisses(&self) -> bool {
        crate::notify::swipe_dismisses(self.offset, self.started.elapsed())
    }
}
impl Overlay {
    pub fn new(
        shell: Entity<Shell>,
        surface: Surface,
        dispatch: Dispatch,
        cx: &mut Context<Self>,
    ) -> Self {
        let images = shell.read(cx).images.clone();
        cx.on_release(|this, cx| this.images.release_assets(cx))
            .detach();
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            surface,
            dispatch,
            images,
            swipe: None,
            _subscription: subscription,
        }
    }
}
impl Render for Overlay {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.shell.read(cx);
        let dispatch = self.dispatch.clone();
        let content = match self.surface {
            Surface::Catcher => div()
                .size_full()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    dispatch(Message::ClosePanel, window, cx)
                })
                .on_mouse_down(MouseButton::Right, {
                    let dispatch = self.dispatch.clone();
                    move |_, window, cx| dispatch(Message::ClosePanel, window, cx)
                })
                .into_any_element(),
            Surface::Osd => {
                let Some(osd) = app.osd else {
                    return div().into_any_element();
                };
                let (glyph, value, muted) = match osd {
                    crate::status::Osd::Volume(v) => (
                        crate::status::volume_icon(v),
                        f32::from(v.percent.min(100)),
                        v.muted,
                    ),
                    crate::status::Osd::Brightness(level) => {
                        ("brightness_medium", f32::from(level), false)
                    }
                };
                crate::gpui_widgets::surface(
                    &app.config,
                    crate::gpui_widgets::Material::Popover,
                    18.,
                )
                .flex()
                .items_center()
                .w(px(crate::status::OSD_W as f32))
                .h(px(crate::status::OSD_H as f32))
                .p(px(12.))
                .when(crate::gpui_widgets::pixel_look(), |osd| {
                    let fill = color(app.config.colors.foreground);
                    let bar = if muted { fill.opacity(0.45) } else { fill };
                    osd.gap(px(8.))
                        .text_color(fill)
                        .child(icon(&app.config, glyph, 16.))
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .h(px(10.))
                                .border_1()
                                .border_color(gpui::rgba(0x8080807a))
                                .bg(fill.opacity(0.08))
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(1.))
                                        .top(px(1.))
                                        .bottom(px(1.))
                                        .w(gpui::relative(value / 100.))
                                        .map(|level| crate::gpui_widgets::fill(level, bar)),
                                ),
                        )
                        .child(
                            div()
                                .min_w(px(24.))
                                .flex()
                                .justify_end()
                                .child(format!("{value:.0}")),
                        )
                })
                .when(!crate::gpui_widgets::pixel_look(), |osd| {
                    osd.child({
                        let fill = color(app.config.colors.foreground);
                        div()
                            .relative()
                            .flex_1()
                            .h(px(24.))
                            .rounded(crate::gpui_widgets::corner(9999.))
                            .overflow_hidden()
                            .bg(fill.opacity(0.18))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .h_full()
                                    .min_w(px(24.))
                                    .w(gpui::relative(value / 100.))
                                    .rounded(crate::gpui_widgets::corner(9999.))
                                    .map(|bar| {
                                        crate::gpui_widgets::fill(
                                            bar,
                                            if muted { fill.opacity(0.45) } else { fill },
                                        )
                                    }),
                            )
                            .child(
                                icon(&app.config, glyph, 16.)
                                    .absolute()
                                    .left(px(5.))
                                    .top(px(3.))
                                    .line_height(px(18.))
                                    .text_color(color(app.config.colors.background)),
                            )
                    })
                })
                .into_any_element()
            }
            Surface::Popups => {
                use crate::notify::{GAP, HEIGHT, PAD, VISIBLE, WIDTH};
                let t = app.popup_expand.interpolate(0., 1., app.instant);
                let list = &app.snapshot.notif.popups;
                let leaving = &app.toast_leave;
                self.images.retain(|key| {
                    list.iter()
                        .chain(leaving.iter().map(|l| &l.notification))
                        .any(|n| key == format!("notification:{}", n.id))
                });
                let place = |depth: f32| {
                    let scale = 1. - 0.05 * depth * (1. - t);
                    (PAD + (GAP + HEIGHT * t) * depth, WIDTH * (1. - scale) / 2.)
                };
                let swipe_end = |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    let Some(swipe) = this.swipe.take() else {
                        return false;
                    };
                    cx.notify();
                    if swipe.dismisses() {
                        (this.dispatch)(Message::NotifSwipe(swipe.id, swipe.offset), window, cx);
                    }
                    swipe.offset.abs() > 4.
                };
                let mut stack = div()
                    .id("popup-stack")
                    .relative()
                    .size_full()
                    .text_color(color(app.config.colors.foreground))
                    .font_family(crate::gpui_widgets::text_font(&app.config))
                    .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                        if !*hovered {
                            swipe_end(this, window, cx);
                            (this.dispatch)(Message::PopupHover(false), window, cx)
                        }
                    }))
                    .on_mouse_move(
                        cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                            if let Some(swipe) = this.swipe.as_mut() {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    let dx = f32::from(event.position.x) - swipe.origin;
                                    swipe.offset = if dx < 0. { dx * 0.2 } else { dx };
                                    cx.notify();
                                } else {
                                    this.swipe = None;
                                    cx.notify();
                                }
                            }
                            if !this.shell.read(cx).popup_hover {
                                (this.dispatch)(Message::PopupHover(true), window, cx)
                            }
                        }),
                    )
                    .capture_any_mouse_up(cx.listener(
                        move |this, _: &gpui::MouseUpEvent, window, cx| {
                            if swipe_end(this, window, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ));
                for (index, notification) in list.iter().take(VISIBLE).enumerate().rev() {
                    let motion = app.toast_motion.get(&notification.id);
                    let enter = motion.map_or(1., |m| m.entered(app.instant));
                    let depth = motion.map_or(index as f32, |m| m.depth(app.instant));
                    let (y, inset) = place(depth);
                    let offset = self
                        .swipe
                        .as_ref()
                        .filter(|swipe| swipe.id == notification.id)
                        .map_or(0., |swipe| swipe.offset);
                    let content = if index == 0 || t > 0.5 {
                        crate::gpui_panels::notification(
                            app,
                            notification,
                            false,
                            &self.dispatch,
                            &mut self.images,
                        )
                    } else {
                        placeholder(app)
                    };
                    let id = notification.id;
                    stack = stack.child(
                        div()
                            .absolute()
                            .top(px(y - HEIGHT * (1. - enter)))
                            .left(px(PAD + inset + offset))
                            .w(px(WIDTH - 2. * inset))
                            .h(px(HEIGHT))
                            .opacity(enter * (1. - (offset.abs() / WIDTH).min(1.)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &gpui::MouseDownEvent, _, _| {
                                    this.swipe = Some(Swipe {
                                        id,
                                        origin: f32::from(event.position.x),
                                        offset: 0.,
                                        started: std::time::Instant::now(),
                                    });
                                }),
                            )
                            .child(content),
                    );
                }
                for leaving in leaving {
                    let fade = leaving.fade.interpolate(0., 1., app.instant);
                    let (y, inset) = place(leaving.depth);
                    let (x, lift) = match leaving.swipe {
                        Some(offset) => (offset + (WIDTH - offset).max(0.) * (1. - fade), 0.),
                        None => (0., HEIGHT * 0.5 * (1. - fade)),
                    };
                    stack = stack.child(
                        div()
                            .absolute()
                            .top(px(y - lift))
                            .left(px(PAD + inset + x))
                            .w(px(WIDTH - 2. * inset))
                            .h(px(HEIGHT))
                            .opacity(fade)
                            .child(crate::gpui_panels::notification(
                                app,
                                &leaving.notification,
                                false,
                                &self.dispatch,
                                &mut self.images,
                            )),
                    );
                }
                stack.into_any_element()
            }
            _ => unreachable!("overlay only renders catchers, popups and OSD"),
        };
        self.images.release_unused_assets(cx);
        content
    }
}

fn placeholder(app: &Shell) -> gpui::AnyElement {
    div()
        .size_full()
        .rounded(crate::gpui_widgets::corner(8.))
        .bg(crate::gpui_widgets::material(
            &app.config,
            crate::gpui_widgets::Material::Card,
        ))
        .border_1()
        .border_color(color(app.config.colors.foreground).opacity(0.12))
        .into_any_element()
}
