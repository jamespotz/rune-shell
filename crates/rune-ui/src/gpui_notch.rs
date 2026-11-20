//! GPUI rendering for the notch's core workspace/title/clock controls.

use crate::{App as Shell, Message, Panel, config::Widget};
use gpui::{App, Context, Entity, Render, Window, div, prelude::*, px};
use std::rc::Rc;

pub(crate) type Dispatch = Rc<dyn Fn(Message, &mut Window, &mut App)>;

pub(crate) struct Notch {
    shell: Entity<Shell>,
    monitor: String,
    dispatch: Dispatch,
    images: crate::gpui_images::Images,
    output_width: std::rc::Rc<std::cell::Cell<Option<u32>>>,
    _subscription: gpui::Subscription,
}

impl Notch {
    pub fn new(
        shell: Entity<Shell>,
        monitor: String,
        dispatch: Dispatch,
        cx: &mut Context<Self>,
    ) -> Self {
        let images = shell.read(cx).images.clone();
        cx.on_release(|this, cx| this.images.release_assets(cx))
            .detach();
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            monitor,
            dispatch,
            images,
            output_width: std::rc::Rc::new(std::cell::Cell::new(None)),
            _subscription: subscription,
        }
    }
}

fn color(value: crate::config::Rgba) -> gpui::Rgba {
    let [r, g, b, a] = value.0;
    gpui::rgba(u32::from_be_bytes([r, g, b, a]))
}

impl Render for Notch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.shell.read(cx);
        let config = &app.config;
        let bar = &config.bar;
        let height = bar.height as f32;
        let radius = bar.radius.clamp(0., height / 2.);
        let mut content = div()
            .relative()
            .w_full()
            .flex()
            .items_center()
            .size_full()
            .gap(px(bar.spacing));
        for (section_index, widgets) in [&bar.left, &bar.center, &bar.right].into_iter().enumerate()
        {
            let mut section = div().flex().flex_1().min_w_0().items_center().gap(px(2.));
            section = match section_index {
                0 => section.justify_start(),
                1 => section.justify_center(),
                _ => section.justify_end(),
            };
            for (index, item) in widgets.iter().enumerate() {
                let dispatch = self.dispatch.clone();
                let id = format!("section-{section_index}-widget-{index}");
                let element = match item.widget {
                    Widget::Workspaces => {
                        let foreground = color(config.colors.foreground);
                        let mut slots = crate::gpui_widgets::item_style(
                            div().id(id.clone()),
                            &item.style,
                            config,
                        )
                        .flex()
                        .items_center()
                        .gap(px(2.));
                        for workspace in crate::notch::workspace_ids(config, &app.snapshot.hypr) {
                            let active = app
                                .snapshot
                                .hypr
                                .monitor(&self.monitor)
                                .is_some_and(|m| m.active_workspace.id == workspace);
                            let urgent = app.snapshot.hypr.urgent.contains(&workspace);
                            let occupied = app
                                .snapshot
                                .hypr
                                .workspaces
                                .iter()
                                .find(|w| w.id == workspace);
                            let label =
                                occupied.map_or_else(|| workspace.to_string(), |w| w.name.clone());
                            let t = app
                                .pills
                                .get(&(self.monitor.clone(), workspace))
                                .map_or(if active { 1. } else { 0. }, |animation| {
                                    animation.interpolate(0., 1., app.instant)
                                });
                            let rest = if urgent {
                                color(config.colors.urgent)
                            } else if occupied.is_some_and(|w| w.windows > 0) {
                                foreground.opacity(0.8)
                            } else {
                                color(config.colors.muted)
                            };
                            let fill = foreground.opacity(0.22 * t);
                            let dispatch = self.dispatch.clone();
                            slots = slots.child(
                                div()
                                    .id(("workspace", workspace as u64))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .min_w(px(22.))
                                    .h(px((height - 10.).max(12.)))
                                    .px(px(5.))
                                    .rounded(crate::gpui_widgets::corner(5.))
                                    .text_size(px(bar.font_size * 0.92))
                                    .text_color(crate::notch::mix(rest, foreground, t))
                                    .when(active, |slot| {
                                        slot.font_weight(crate::gpui_widgets::font_weight(
                                            gpui::FontWeight::SEMIBOLD,
                                        ))
                                    })
                                    .bg(fill)
                                    .hover(|style| style.bg(foreground.opacity(0.18)))
                                    .active(|style| style.bg(foreground.opacity(0.26)))
                                    .cursor_pointer()
                                    .child(label)
                                    .on_click(move |_, window, cx| {
                                        dispatch(Message::Focus(workspace), window, cx)
                                    }),
                            );
                        }
                        slots.into_any_element()
                    }
                    Widget::WindowTitle => {
                        let active = app
                            .snapshot
                            .hypr
                            .monitor(&self.monitor)
                            .map(|m| m.active_workspace.id);
                        let title = app
                            .snapshot
                            .hypr
                            .workspaces
                            .iter()
                            .find(|w| Some(w.id) == active && w.windows > 0)
                            .map(|w| {
                                crate::notch::truncate(
                                    &w.last_window_title,
                                    config.window_title.max_chars,
                                )
                            })
                            .unwrap_or_default();
                        crate::gpui_widgets::item_style(div(), &item.style, config)
                            .px(px(4.))
                            .font_weight(crate::gpui_widgets::font_weight(
                                gpui::FontWeight::SEMIBOLD,
                            ))
                            .truncate()
                            .child(title)
                            .into_any_element()
                    }
                    Widget::Clock => {
                        let mut clock = crate::gpui_widgets::heading(crate::gpui_widgets::row());
                        if !config.clock.icon.is_empty() || !item.style.icon.is_empty() {
                            clock = clock.child(crate::gpui_widgets::item_icon(
                                config,
                                item,
                                &config.clock.icon,
                                bar.font_size * 1.25,
                            ));
                        }
                        crate::gpui_widgets::bar_button(
                            app,
                            item,
                            id,
                            clock.child(app.snapshot.now.format(&config.clock.format).to_string()),
                            Message::Toggle(Panel::Calendar),
                            &dispatch,
                        )
                        .into_any_element()
                    }
                    Widget::Settings => crate::gpui_widgets::bar_button(
                        app,
                        item,
                        id,
                        crate::gpui_widgets::item_icon(
                            config,
                            item,
                            "settings",
                            bar.font_size * 1.25,
                        ),
                        Message::ToggleSettings,
                        &dispatch,
                    )
                    .into_any_element(),
                    _ => match crate::gpui_widgets::bar_widget(
                        app,
                        item,
                        id,
                        &self.dispatch,
                        &mut self.images,
                    ) {
                        Some(element) => element,
                        None => continue,
                    },
                };
                section = section.child(element);
            }
            content = content.child(section);
        }
        if app.snapshot.recording.is_some() && !crate::notch::bar_has(bar, Widget::Capture) {
            content = content.child(crate::gpui_widgets::recording_chip(
                app,
                "recording".into(),
                &self.dispatch,
            ));
        }
        if app.game_mode {
            content = content.child(crate::gpui_widgets::button(
                "game-mode",
                crate::gpui_widgets::icon(config, "sports_esports", bar.font_size * 1.3),
                Message::GameModeOff,
                &self.dispatch,
            ));
        }
        self.images.retain(|key| {
            (key == "media" && app.snapshot.media.as_ref().is_some_and(|m| m.art.is_some()))
                || app
                    .snapshot
                    .tray
                    .iter()
                    .any(|item| key == format!("tray:{}", item.key))
        });
        let moved = self.dispatch.clone();
        let exited = self.dispatch.clone();
        let monitor = self.monitor.clone();
        let width_cache = self.output_width.clone();
        let width_dispatch = self.dispatch.clone();
        let width_monitor = self.monitor.clone();
        let content = div()
            .relative()
            .on_mouse_move(move |event, window, cx| {
                moved(
                    Message::BarCursor(Some((monitor.clone(), f32::from(event.position.x)))),
                    window,
                    cx,
                )
            })
            .on_mouse_exit(move |_, window, cx| exited(Message::BarCursor(None), window, cx))
            .size_full()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(bar.margin_top as f32))
            .px(px(bar.margin_x as f32))
            .font_family(crate::gpui_widgets::text_font(config))
            .text_size(px(bar.font_size))
            .text_color(color(config.colors.foreground))
            .font_weight(crate::gpui_widgets::font_weight(gpui::FontWeight::MEDIUM))
            .child(
                crate::gpui_widgets::bar_outline(div(), bar)
                    .w_full()
                    .when(bar.width > 0, |inner| inner.max_w(px(bar.width as f32)))
                    .h(px(height))
                    .px(px(bar.padding + 4.))
                    .bg(crate::gpui_widgets::material(
                        config,
                        crate::gpui_widgets::Material::Bar,
                    ))
                    .rounded_b(crate::gpui_widgets::corner(radius))
                    .when(bar.margin_top > 0, |inner| {
                        inner.rounded_t(crate::gpui_widgets::corner(radius))
                    })
                    .child(content),
            )
            .child(
                gpui::canvas(
                    move |bounds, window, cx| {
                        let width = f32::from(bounds.size.width).ceil() as u32;
                        if width_cache.get() != Some(width) {
                            width_cache.set(Some(width));
                            window.defer(cx, move |window, cx| {
                                width_dispatch(
                                    Message::BarWidth(width_monitor, width as f32),
                                    window,
                                    cx,
                                )
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
                .size_full(),
            );
        self.images.release_unused_assets(cx);
        content
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "opens a GPUI bar; requires Wayland"]
    fn all_widgets_and_repeated_widgets_render() {
        use gpui::{AppContext, QuitMode};
        gpui_platform::application()
            .with_quit_mode(QuitMode::Explicit)
            .run(|cx| {
                let mut shell = crate::gpui_panels::tests::populated_shell();
                shell.config.bar.left = vec![Widget::Workspaces.into(), Widget::Tray.into()];
                shell.config.bar.center.clear();
                shell.config.bar.right = Widget::ALL.map(Into::into).to_vec();
                let styled = crate::config::WidgetStyle {
                    icon: "star".into(),
                    icon_color: Some(crate::config::Rgba([0xff, 0, 0, 0xff])),
                    border_width: 1.5,
                    border_color: None,
                    radius: Some(0.),
                };
                for item in &mut shell.config.bar.right {
                    item.style = styled.clone();
                }
                shell.config.bar.left[0].style = styled;
                shell.config.bar.right.push(crate::config::BarItem {
                    device: "absent".into(),
                    ..Widget::DeviceBattery.into()
                });
                shell.game_mode = true;
                shell.config.bar.border_width = 2.;
                shell.config.bar.border_style = crate::config::BorderStyle::Dashed;
                shell.config.bar.shadow = true;
                shell.snapshot.recording = Some(rune_services::capture::Recording {
                    path: "/tmp/rune-test.mp4".into(),
                    started: std::time::Instant::now(),
                });
                let shell = cx.new(|_| shell);
                cx.open_window(
                    crate::gpui_surfaces::options(
                        &crate::Surface::Bar(String::new()),
                        (0, 32),
                        None,
                        false,
                        0,
                    ),
                    |_, cx| {
                        cx.new(|cx| {
                            Notch::new(shell, String::new(), std::rc::Rc::new(|_, _, _| {}), cx)
                        })
                    },
                )
                .unwrap();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(200))
                        .await;
                    cx.update(|cx| cx.quit());
                })
                .detach();
            });
    }
}
