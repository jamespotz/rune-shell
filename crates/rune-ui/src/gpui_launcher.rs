//! GPUI launcher view, using the existing search and activation model.
use super::{Hit, Kind, LIST_MAX_H, Msg, PANEL_W, ROW_H, ago};
use crate::{
    App as Shell, Message,
    controls::text_input::{InputEvent, TextInput},
    gpui_notch::Dispatch,
};
use gpui::{Context, Entity, Focusable, Render, ScrollHandle, Window, div, prelude::*, px};

pub(crate) struct LauncherView {
    shell: Entity<Shell>,
    input: Entity<TextInput>,
    dispatch: Dispatch,
    scroll: ScrollHandle,
    images: crate::gpui_images::Images,
    measure: std::rc::Rc<std::cell::Cell<Option<(u32, u32)>>>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl LauncherView {
    pub fn new(
        shell: Entity<Shell>,
        dispatch: Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let images = shell.read(cx).images.clone();
        cx.on_release(|this, cx| this.images.release_assets(cx))
            .detach();
        let placeholder = match shell.read(cx).launcher.kind {
            Kind::Apps => "Search apps and games · :emoji · 2+2",
            Kind::Keybinds => "Search keybinds",
            Kind::Clipboard => "Search clipboard history",
        };
        let query = shell.read(cx).launcher.query.clone();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(&query, placeholder, cx);
            input.set_text_size(20.);
            input
        });
        input.focus_handle(cx).focus(window, cx);
        let scroll = ScrollHandle::new();
        let changed = cx.subscribe_in(&input, window, {
            let dispatch = dispatch.clone();
            move |_, _, event: &InputEvent, window, cx| {
                let msg = match event {
                    InputEvent::Changed(text) => Msg::Query(text.clone()),
                    InputEvent::Submit => Msg::Submit,
                };
                dispatch(Message::Launcher(msg), window, cx);
            }
        });
        let observed = cx.observe(&shell, |this, shell, cx| {
            this.scroll.scroll_to_item(shell.read(cx).launcher.selected);
            cx.notify();
        });
        Self {
            shell,
            input,
            dispatch,
            scroll,
            images,
            measure: Default::default(),
            _subscriptions: vec![changed, observed],
        }
    }
}

impl Render for LauncherView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (query, kind) = {
            let model = &self.shell.read(cx).launcher;
            (model.query.clone(), model.kind)
        };
        let placeholder = match kind {
            Kind::Apps => "Search apps and games · :emoji · 2+2",
            Kind::Keybinds => "Search keybinds",
            Kind::Clipboard => "Search clipboard history",
        };
        self.input
            .update(cx, |input, cx| input.sync(&query, placeholder, cx));
        let shell = self.shell.read(cx);
        let model = &shell.launcher;
        let config = &shell.config;
        let color = |c: crate::config::Rgba| gpui::rgba(u32::from_be_bytes(c.0));
        let muted = color(config.colors.muted);
        let accent = color(config.colors.accent);
        let mut rows = div().flex().flex_col().gap(px(2.));
        for (i, hit) in model.results.iter().enumerate() {
            let (title, subtitle, lead) = match hit {
                Hit::App(a) => (
                    model.apps[*a].name.clone(),
                    model.apps[*a].comment.clone(),
                    "apps".to_owned(),
                ),
                Hit::Calc(result) => (
                    format!("= {result}"),
                    "Enter copies the result".into(),
                    "calculate".into(),
                ),
                Hit::Emoji(emoji) => (
                    emoji.name().to_owned(),
                    emoji
                        .shortcode()
                        .map(|s| format!(":{s}:"))
                        .unwrap_or_default(),
                    emoji.as_str().into(),
                ),
                Hit::Bind(b) => (
                    model.binds[*b].description.clone(),
                    if model.binds[*b].submap.is_empty() {
                        String::new()
                    } else {
                        format!("submap: {}", model.binds[*b].submap)
                    },
                    model.binds[*b].keys.clone(),
                ),
                Hit::Clip(c) => (
                    model.clips[*c].title.clone(),
                    ago(model.clips[*c].time, chrono::Local::now().timestamp()),
                    "content_paste".into(),
                ),
            };
            let glyph = |name: &str| {
                div()
                    .font_family(config.bar.icon_font.clone())
                    .text_size(px(26.))
                    .child(name.to_owned())
                    .into_any_element()
            };
            let leading = match hit {
                Hit::App(a) => {
                    let app = &model.apps[*a];
                    let source = app.icon.as_ref().and_then(|path| {
                        if let Some(bytes) = model.sniffed.get(path) {
                            self.images.encoded_icon(&path.to_string_lossy(), bytes)
                        } else if path
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(super::known_ext)
                        {
                            self.images.source(
                                &path.to_string_lossy(),
                                &rune_services::notifications::Image::Path(path.clone()),
                            )
                        } else {
                            None
                        }
                    });
                    match source {
                        Some(source) => gpui::img(source)
                            .size(px(32.))
                            .object_fit(gpui::ObjectFit::Contain)
                            .into_any_element(),
                        None => glyph(if app.steam { "sports_esports" } else { "apps" }),
                    }
                }
                Hit::Clip(c) => match model.clips[*c].image.as_ref().and_then(|path| {
                    self.images.source(
                        &format!("clip:{}", path.display()),
                        &rune_services::notifications::Image::Path(path.clone()),
                    )
                }) {
                    Some(source) => gpui::img(source)
                        .size(px(40.))
                        .object_fit(gpui::ObjectFit::Contain)
                        .rounded(px(4.))
                        .into_any_element(),
                    None => glyph("content_paste"),
                },
                Hit::Calc(_) => glyph("calculate"),
                Hit::Emoji(_) => div()
                    .text_size(px(26.))
                    .child(lead.clone())
                    .into_any_element(),
                Hit::Bind(_) => div()
                    .flex()
                    .gap(px(4.))
                    .children(lead.split(" + ").map(|key| {
                        div()
                            .px(px(7.))
                            .py(px(2.))
                            .rounded(px(6.))
                            .bg(color(config.colors.foreground).opacity(0.1))
                            .text_size(px(12.))
                            .child(key.to_owned())
                    }))
                    .into_any_element(),
            };
            let lead_width = if matches!(hit, Hit::Bind(_)) {
                190.
            } else {
                40.
            };
            let dispatch = self.dispatch.clone();
            let selected = i == model.selected;
            let mut row = div()
                .id(("result", i))
                .hover(|style| {
                    if selected {
                        style
                    } else {
                        style.bg(color(config.colors.foreground).opacity(0.08))
                    }
                })
                .active(|style| {
                    style.bg(if selected {
                        accent.opacity(0.85)
                    } else {
                        color(config.colors.foreground).opacity(0.16)
                    })
                })
                .flex()
                .items_center()
                .gap(px(14.))
                .h(px(ROW_H))
                .px(px(12.))
                .rounded(px(8.))
                .cursor_pointer()
                .child(
                    div()
                        .w(px(lead_width))
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(leading),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if selected {
                                    color(config.colors.on_accent).opacity(0.75)
                                } else {
                                    muted
                                })
                                .child(subtitle),
                        ),
                )
                .on_click(move |_, window, cx| {
                    dispatch(Message::Launcher(Msg::Activate(i)), window, cx)
                });
            if selected {
                row = row.bg(accent).text_color(color(config.colors.on_accent));
            }
            if let Hit::Clip(c) = hit {
                let favorite = model.clips[*c].is_favorite();
                let dispatch = self.dispatch.clone();
                row = row.child(
                    div()
                        .id(("favorite", i))
                        .px(px(6.))
                        .child(if favorite { "★" } else { "☆" })
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            dispatch(Message::Launcher(Msg::Favorite(i)), window, cx);
                        }),
                );
            }
            rows = rows.child(row);
        }
        if model.results.is_empty() {
            rows = rows.child(div().p_4().text_color(muted).child("No matches"));
        }
        let dispatch = self.dispatch.clone();
        let mut footer = div()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(11.))
            .text_color(muted)
            .child(match model.kind {
                Kind::Apps => "↑↓ select   Enter open   Esc close",
                Kind::Keybinds => "↑↓ scroll   Esc close",
                Kind::Clipboard => "↑↓ select   Enter copy   ★ keep   Esc close",
            });
        if model.kind == Kind::Clipboard && model.clips.iter().any(|clip| !clip.is_favorite()) {
            let dispatch = self.dispatch.clone();
            footer = footer.child(
                div()
                    .id("clear-clips")
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .hover(|style| style.bg(color(config.colors.urgent).opacity(0.12)))
                    .active(|style| style.bg(color(config.colors.urgent).opacity(0.24)))
                    .cursor_pointer()
                    .child("Clear history")
                    .on_click(move |_, window, cx| {
                        dispatch(Message::Launcher(Msg::ClearClips), window, cx)
                    }),
            );
        }
        let content =
            crate::gpui_widgets::surface(config, crate::gpui_widgets::Material::Popover, 22.)
                .relative()
                .flex_none()
                .w(px(PANEL_W + 32.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(16.))
                .on_key_down(move |event, window, cx| {
                    let message = match event.keystroke.key.as_str() {
                        "up" => Message::Nav(-1),
                        "down" => Message::Nav(1),
                        "pageup" => Message::Nav(-8),
                        "pagedown" => Message::Nav(8),
                        "escape" => Message::Escape,
                        _ => return,
                    };
                    cx.stop_propagation();
                    dispatch(message, window, cx);
                })
                .child(
                    crate::gpui_widgets::row()
                        .gap(px(10.))
                        .px(px(6.))
                        .py(px(6.))
                        .text_size(px(20.))
                        .font_weight(gpui::FontWeight::LIGHT)
                        .child(crate::gpui_widgets::icon(config, "search", 24.).text_color(muted))
                        .child(div().flex_1().child(self.input.clone())),
                )
                .child(crate::gpui_widgets::separator(config).mx_0())
                .child(
                    rows.id("launcher-results")
                        .max_h(px(LIST_MAX_H))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll),
                )
                .child(footer)
                .child(crate::gpui_widgets::measure_panel(
                    self.measure.clone(),
                    self.dispatch.clone(),
                ));
        self.images.release_unused_assets(cx);
        crate::gpui_widgets::natural_content(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "opens a GPUI launcher; requires Wayland"]
    fn native_edit_updates_search_results() {
        use gpui::{EntityInputHandler, QuitMode};
        gpui_platform::application().with_quit_mode(QuitMode::Explicit).run(|cx| {
            TextInput::bind_keys(cx);
            let (actions, _) = tokio::sync::mpsc::unbounded_channel();
            let (_, events) = tokio::sync::mpsc::unbounded_channel();
            let (app, _) = Shell::new(crate::Config::default(), events, actions, false);
            let shell = cx.new(|_| app);
            let measured = std::rc::Rc::new(std::cell::Cell::new(0.));
            let dispatch: Dispatch = {
                let shell = shell.clone();
                let measured = measured.clone();
                std::rc::Rc::new(move |message, _, cx| {
                    if let Message::PanelSize(size) = &message {
                        measured.set(f32::from(size.width));
                    }
                    shell.update(cx, |shell, cx| {
                        let _ = shell.update(message);
                        cx.notify();
                    });
                })
            };
            let handle = cx.open_window(
                crate::gpui_surfaces::options(&crate::Surface::Panel, (672, 440), None, true),
                |window, cx| cx.new(|cx| LauncherView::new(shell.clone(), dispatch, window, cx)),
            ).unwrap();
            handle.update(cx, |view, window, cx| {
                view.input.update(cx, |input, cx| input.replace_text_in_range(None, "2+2", window, cx));
            }).unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                cx.update(|cx| {
                    let shell = shell.read(cx);
                    assert_eq!(measured.get(), PANEL_W + 32., "surface decoration must preserve measured bounds");
                    assert_eq!(shell.launcher.query, "2+2");
                    assert!(matches!(shell.launcher.results.first(), Some(Hit::Calc(result)) if result == "4"));
                    cx.quit();
                });
            }).detach();
        });
    }
}
