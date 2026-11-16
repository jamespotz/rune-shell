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
    _subscription: gpui::Subscription,
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
                .child({
                    let fill = color(app.config.colors.foreground);
                    div()
                        .relative()
                        .flex_1()
                        .h(px(24.))
                        .rounded_full()
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
                                .rounded_full()
                                .bg(if muted { fill.opacity(0.45) } else { fill }),
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
                .into_any_element()
            }
            Surface::Popups => {
                let t = app.popup_expand.interpolate(0., 1., app.instant);
                let list = &app.snapshot.notif.popups;
                self.images
                    .retain(|key| list.iter().any(|n| key == format!("notification:{}", n.id)));
                let front_height = list.first().map_or(0., crate::notify::toast_height);
                let mut positions = Vec::new();
                let mut fanned = 0.;
                for (index, notification) in list.iter().take(crate::notify::MAX_SHOWN).enumerate()
                {
                    positions.push((index, notification, fanned));
                    fanned += crate::notify::toast_height(notification) + crate::notify::GAP;
                }
                let mut stack = div()
                    .id("popup-stack")
                    .relative()
                    .size_full()
                    .text_color(color(app.config.colors.foreground))
                    .font_family(app.config.bar.font.clone())
                    .on_hover(move |hovered, window, cx| {
                        dispatch(Message::PopupHover(*hovered), window, cx)
                    });
                for (index, notification, fanned) in positions.into_iter().rev() {
                    if index > crate::notify::BEHIND && t < 0.05 {
                        continue;
                    }
                    let depth = index.min(crate::notify::BEHIND + 1) as f32;
                    let y =
                        crate::notify::PEEK * depth + (fanned - crate::notify::PEEK * depth) * t;
                    let scale = 1. - 0.05 * depth * (1. - t);
                    let height = front_height
                        + (crate::notify::toast_height(notification) - front_height) * t;
                    let inset = crate::notify::WIDTH * (1. - scale) / 2.;
                    let content = if index == 0 || t > 0.5 {
                        crate::gpui_panels::notification(
                            app,
                            notification,
                            false,
                            &self.dispatch,
                            &mut self.images,
                        )
                    } else {
                        div()
                            .size_full()
                            .rounded(px(14.))
                            .bg(color(app.config.colors.background))
                            .border_1()
                            .border_color(color(app.config.colors.foreground).opacity(0.12))
                            .into_any_element()
                    };
                    stack = stack.child(
                        div()
                            .absolute()
                            .top(px(y))
                            .left(px(inset))
                            .w(px(crate::notify::WIDTH - 2. * inset))
                            .h(px(height))
                            .overflow_hidden()
                            .child(content),
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
