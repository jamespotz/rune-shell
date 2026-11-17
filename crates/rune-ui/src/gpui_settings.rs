//! Native GPUI settings controls; all edits use the shared validated settings model.
use super::*;
use crate::{
    controls::{
        slider::{Slider, SliderEvent},
        text_input::{InputEvent, TextInput},
    },
    gpui_notch::Dispatch,
    gpui_widgets::{button, color, column, row},
};
use gpui::{AnyElement, Context, Entity, Window, div, img, prelude::*, px};

pub(crate) struct SettingsView {
    shell: Entity<crate::App>,
    focus: gpui::FocusHandle,
    page: Page,
    dispatch: Dispatch,
    inputs: HashMap<String, (String, Entity<TextInput>)>,
    sliders: HashMap<String, Entity<Slider>>,
    colors: crate::config::Colors,
    icon_font: gpui::SharedString,
    add_target: Section,
}
impl SettingsView {
    pub fn new(
        shell: Entity<crate::App>,
        dispatch: Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&shell, |_, _, cx| cx.notify()).detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let page = shell.read(cx).settings_ui.page;
        let colors = shell.read(cx).config.colors;
        let icon_font = shell.read(cx).config.bar.icon_font.clone().into();
        Self {
            colors,
            icon_font,
            add_target: Section::Right,
            focus,
            page,
            shell,
            dispatch,
            inputs: HashMap::new(),
            sliders: HashMap::new(),
        }
    }
    fn action(&self, id: impl Into<String>, label: impl Into<String>, msg: Msg) -> AnyElement {
        button(
            id.into(),
            label.into(),
            crate::Message::Settings(msg),
            &self.dispatch,
        )
        .rounded(crate::gpui_widgets::corner(6.))
        .px(px(10.))
        .bg(gpui::rgba(0xffffff14))
        .into_any_element()
    }
    fn choice(
        &self,
        id: impl Into<String>,
        leading: Option<&str>,
        label: impl Into<gpui::SharedString>,
        detail: Option<String>,
        selected: bool,
        msg: Msg,
    ) -> AnyElement {
        let colors = self.colors;
        let dispatch = self.dispatch.clone();
        row()
            .id(id.into())
            .w_full()
            .gap(px(10.))
            .cursor_pointer()
            .active(|style| style.opacity(0.7))
            .children(leading.map(|glyph| {
                div()
                    .font_family(self.icon_font.clone())
                    .text_size(px(18.))
                    .text_color(color(colors.muted))
                    .child(glyph.to_owned())
            }))
            .child(
                column()
                    .gap_0()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(label.into()))
                    .children(detail.map(|detail| {
                        div()
                            .text_size(px(11.))
                            .text_color(color(colors.muted))
                            .child(detail)
                    })),
            )
            .when(selected, |line| {
                line.child(
                    div()
                        .font_family(self.icon_font.clone())
                        .text_size(px(18.))
                        .text_color(color(colors.accent))
                        .child("check"),
                )
            })
            .on_click(move |_, window, cx| {
                dispatch(crate::Message::Settings(msg.clone()), window, cx)
            })
            .into_any_element()
    }
    fn glyph(&self, id: impl Into<String>, glyph: &str, msg: Msg, destructive: bool) -> AnyElement {
        let colors = self.colors;
        let dispatch = self.dispatch.clone();
        div()
            .id(id.into())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded(crate::gpui_widgets::corner(6.))
            .font_family(self.icon_font.clone())
            .text_size(px(17.))
            .text_color(color(if destructive {
                colors.urgent
            } else {
                colors.foreground
            }))
            .hover(|style| style.bg(gpui::rgba(0xffffff1a)))
            .active(|style| style.bg(gpui::rgba(0xffffff2e)))
            .cursor_pointer()
            .child(glyph.to_owned())
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                dispatch(crate::Message::Settings(msg.clone()), window, cx)
            })
            .into_any_element()
    }
    fn segmented(&self, id: &str, options: Vec<(String, bool, Msg)>) -> AnyElement {
        let mut control = row()
            .gap(px(2.))
            .p(px(2.))
            .rounded(crate::gpui_widgets::corner(7.))
            .bg(gpui::rgba(0xffffff12));
        for (index, (label, selected, msg)) in options.into_iter().enumerate() {
            let dispatch = self.dispatch.clone();
            control = control.child(
                div()
                    .id(format!("{id}:{index}"))
                    .px(px(12.))
                    .py(px(3.))
                    .rounded(crate::gpui_widgets::corner(5.))
                    .text_size(px(12.))
                    .when(selected, |segment| {
                        segment
                            .bg(color(self.colors.accent))
                            .text_color(color(self.colors.on_accent))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .when(!selected, |segment| {
                        segment.hover(|style| style.bg(gpui::rgba(0xffffff12)))
                    })
                    .cursor_pointer()
                    .child(label)
                    .on_click(move |_, window, cx| {
                        dispatch(crate::Message::Settings(msg.clone()), window, cx)
                    }),
            );
        }
        control.into_any_element()
    }
    fn labeled(&self, label: &str, control: AnyElement) -> AnyElement {
        row()
            .w_full()
            .justify_between()
            .gap(px(20.))
            .child(label_block(&self.colors, label))
            .child(control)
            .into_any_element()
    }
    fn destructive(&self, id: impl Into<String>, label: &str, msg: Msg) -> AnyElement {
        let dispatch = self.dispatch.clone();
        div()
            .id(id.into())
            .text_color(color(self.colors.urgent))
            .cursor_pointer()
            .active(|style| style.opacity(0.6))
            .child(label.to_owned())
            .on_click(move |_, window, cx| {
                dispatch(crate::Message::Settings(msg.clone()), window, cx)
            })
            .into_any_element()
    }
    fn toggle_switch(&self, id: &str, on: bool, msg: Msg) -> AnyElement {
        crate::gpui_widgets::switch(
            id.to_owned(),
            &self.colors,
            on,
            crate::Message::Settings(msg),
            &self.dispatch,
        )
        .into_any_element()
    }
    fn primary(&self, id: &str, label: &str, msg: Msg) -> AnyElement {
        button(
            id.to_owned(),
            label.to_owned(),
            crate::Message::Settings(msg),
            &self.dispatch,
        )
        .rounded(crate::gpui_widgets::corner(6.))
        .px(px(12.))
        .bg(color(self.colors.accent))
        .text_color(color(self.colors.on_accent))
        .into_any_element()
    }
    fn toggle(&self, id: &str, label: &str, on: bool, msg: Msg) -> AnyElement {
        row()
            .w_full()
            .justify_between()
            .gap(px(20.))
            .child(label_block(&self.colors, label))
            .child(crate::gpui_widgets::switch(
                id.to_owned(),
                &self.colors,
                on,
                crate::Message::Settings(msg),
                &self.dispatch,
            ))
            .into_any_element()
    }

    fn input(
        &mut self,
        id: String,
        value: String,
        placeholder: &str,
        events: (fn(String) -> Msg, Option<Msg>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (changed, submit) = events;
        if let Some((last, entity)) = self.inputs.get_mut(&id) {
            if last != &value {
                entity.update(cx, |input, cx| input.sync(&value, placeholder, cx));
                *last = value.clone();
            }
        } else {
            let entity = cx.new(|cx| {
                let mut input = TextInput::new(&value, placeholder, cx);
                input.set_text_size(13.);
                input
            });
            if id == "wifi-password" {
                entity.update(cx, |input, cx| input.set_password(true, cx));
            }
            let dispatch = self.dispatch.clone();
            let key = id.clone();
            cx.subscribe_in(
                &entity,
                window,
                move |this, _, event, window, cx| match event {
                    InputEvent::Changed(value) => {
                        if let Some((last, _)) = this.inputs.get_mut(&key) {
                            *last = value.clone();
                        }
                        dispatch(crate::Message::Settings(changed(value.clone())), window, cx);
                    }
                    InputEvent::Submit => {
                        if let Some(msg) = &submit {
                            dispatch(crate::Message::Settings(msg.clone()), window, cx);
                        }
                    }
                },
            )
            .detach();
            self.inputs.insert(id.clone(), (value, entity));
        }
        let boxed = id != "settings-search";
        div()
            .w_full()
            .rounded(crate::gpui_widgets::corner(6.))
            .when(boxed, |field| field.bg(gpui::rgba(0xffffff12)))
            .child(self.inputs[&id].1.clone())
            .into_any_element()
    }
    fn field(
        &mut self,
        state: &State,
        c: &Config,
        field: Field,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = state
            .drafts
            .get(&field)
            .cloned()
            .unwrap_or_else(|| text_of(c, field));
        let id = format!("field:{field:?}");
        // Field-specific callbacks retain the enum without duplicating validation.
        let entity = if let Some((last, entity)) = self.inputs.get_mut(&id) {
            if last != &value {
                entity.update(cx, |input, cx| input.sync(&value, "", cx));
                *last = value.clone();
            }
            entity.clone()
        } else {
            let entity = cx.new(|cx| {
                let mut input = TextInput::new(&value, "", cx);
                input.set_text_size(13.);
                input
            });
            let dispatch = self.dispatch.clone();
            let key = id.clone();
            cx.subscribe_in(&entity, window, move |this, _, event, window, cx| {
                if let InputEvent::Changed(value) = event {
                    if let Some((last, _)) = this.inputs.get_mut(&key) {
                        *last = value.clone();
                    }
                    dispatch(
                        crate::Message::Settings(Msg::Text(field, value.clone())),
                        window,
                        cx,
                    );
                }
            })
            .detach();
            self.inputs.insert(id, (value, entity.clone()));
            entity
        };
        row()
            .w_full()
            .justify_between()
            .gap(px(20.))
            .child(label_block(&c.colors, label))
            .child(
                column()
                    .child(
                        div()
                            .w(px(240.))
                            .border_1()
                            .border_color(if state.drafts.contains_key(&field) {
                                color(c.colors.urgent)
                            } else {
                                gpui::rgba(0x00000000)
                            })
                            .bg(color(c.colors.foreground).opacity(0.08))
                            .rounded(crate::gpui_widgets::corner(6.))
                            .child(entity),
                    )
                    .children(state.drafts.contains_key(&field).then(|| {
                        div()
                            .text_size(px(11.))
                            .text_color(color(c.colors.urgent))
                            .child("Invalid value; saved value is unchanged")
                    })),
            )
            .into_any_element()
    }
    fn number(
        &mut self,
        c: &Config,
        label: &str,
        num: Num,
        parameters: (f32, (f32, f32), f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (value, range, step) = parameters;
        let key = format!("number:{num:?}");
        let entity = if let Some(entity) = self.sliders.get(&key) {
            entity.update(cx, |s, _| s.sync(range, value, color(c.colors.accent)));
            entity.clone()
        } else {
            let entity = cx.new(|_| Slider::new(range, value, color(c.colors.accent)));
            let dispatch = self.dispatch.clone();
            cx.subscribe_in(&entity, window, move |_, _, event, window, cx| {
                if let SliderEvent::Change(value) = event {
                    dispatch(
                        crate::Message::Settings(Msg::Num(num, (*value / step).round() * step)),
                        window,
                        cx,
                    )
                }
            })
            .detach();
            self.sliders.insert(key, entity.clone());
            entity
        };
        row()
            .w_full()
            .justify_between()
            .gap(px(20.))
            .child(label_block(&c.colors, label))
            .child(
                row().w(px(240.)).child(div().flex_1().child(entity)).child(
                    div()
                        .w(px(40.))
                        .text_color(color(c.colors.muted))
                        .child(if step >= 1. {
                            format!("{value:.0}")
                        } else {
                            format!("{value:.1}")
                        }),
                ),
            )
            .into_any_element()
    }
}
fn split_label(label: &str) -> (&str, Option<&str>) {
    match label.split_once(" (") {
        Some((title, rest)) => (title, rest.strip_suffix(')')),
        None => (label, None),
    }
}
fn label_block(colors: &crate::config::Colors, label: &str) -> gpui::Div {
    let (title, detail) = split_label(label);
    column()
        .gap_0()
        .flex_1()
        .min_w_0()
        .child(title.to_owned())
        .children(detail.map(|detail| {
            div()
                .text_size(px(11.))
                .text_color(color(colors.muted))
                .child(detail.to_owned())
        }))
}
fn note(c: &Config, text: impl Into<gpui::SharedString>) -> AnyElement {
    div()
        .px(px(4.))
        .mt(px(-10.))
        .text_size(px(11.))
        .text_color(color(c.colors.muted))
        .child(text.into())
        .into_any_element()
}
fn hero(
    c: &Config,
    page: Page,
    description: impl Into<gpui::SharedString>,
    trailing: Option<AnyElement>,
) -> AnyElement {
    crate::gpui_widgets::frame(row())
        .w_full()
        .gap(px(14.))
        .p(px(14.))
        .rounded(crate::gpui_widgets::corner(10.))
        .bg(gpui::rgba(0xffffff0d))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(44.))
                .rounded(crate::gpui_widgets::corner(10.))
                .bg(gpui::rgba(page.tint()))
                .text_color(gpui::rgb(0xffffff))
                .child(crate::gpui_widgets::icon(c, page.label().0, 26.)),
        )
        .child(
            column()
                .gap(px(2.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(page.label().1),
                )
                .child(crate::gpui_widgets::muted(c, description)),
        )
        .children(trailing)
        .into_any_element()
}
fn code(text: &str) -> AnyElement {
    div()
        .w_full()
        .px(px(8.))
        .py(px(4.))
        .rounded(crate::gpui_widgets::corner(6.))
        .bg(gpui::rgba(0x0000004d))
        .font_family("DejaVu Sans Mono")
        .text_size(px(11.))
        .child(text.to_owned())
        .into_any_element()
}
fn swatch(value: crate::config::Rgba, size: f32) -> gpui::Div {
    div()
        .flex_none()
        .size(px(size))
        .rounded(crate::gpui_widgets::corner(9999.))
        .border_1()
        .border_color(gpui::rgba(0xffffff33))
        .bg(color(value))
}
fn group(title: &str, children: Vec<AnyElement>) -> AnyElement {
    let count = children.len();
    let mut list = crate::gpui_widgets::frame(column())
        .gap_0()
        .rounded(crate::gpui_widgets::corner(10.))
        .bg(gpui::rgba(0xffffff0d));
    for (index, child) in children.into_iter().enumerate() {
        list = list.child(
            div()
                .px(px(12.))
                .py(px(9.))
                .min_h(px(40.))
                .flex()
                .flex_col()
                .items_start()
                .justify_center()
                .when(index + 1 < count, |line| {
                    line.border_b_1().border_color(gpui::rgba(0xffffff14))
                })
                .child(child),
        );
    }
    column()
        .gap(px(6.))
        .child(
            crate::gpui_widgets::heading(div())
                .px(px(4.))
                .text_size(px(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .opacity(0.6)
                .child(title.to_owned()),
        )
        .child(list)
        .into_any_element()
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.shell.read(cx);
        let c = app.config.clone();
        let s = app.settings_ui.clone();
        self.colors = c.colors;
        if self.icon_font.as_ref() != c.bar.icon_font {
            self.icon_font = c.bar.icon_font.clone().into();
        }
        if self.page != s.page {
            self.page = s.page;
            window.focus(&self.focus, cx);
        }
        let mut items = Vec::new();
        macro_rules! field {
            ($f:ident,$label:expr) => {
                self.field(&s, &c, Field::$f, $label, window, cx)
            };
        }
        macro_rules! number {
            ($n:ident,$label:expr,$value:expr,$lo:expr,$hi:expr,$step:expr) => {
                self.number(
                    &c,
                    $label,
                    Num::$n,
                    ($value as f32, ($lo, $hi), $step),
                    window,
                    cx,
                )
            };
        }
        macro_rules! flag {
            ($f:ident,$label:expr,$value:expr) => {
                self.toggle(stringify!($f), $label, $value, Msg::Flag(Flag::$f, !$value))
            };
        }
        match s.page {
            Page::Appearance => {
                items.push(hero(
                    &c,
                    Page::Appearance,
                    "Fonts, colours and materials used across Rune.",
                    None,
                ));
                items.push(group(
                    "Text",
                    vec![
                        field!(Font, "Font (any installed family)"),
                        field!(IconFont, "Icon font (Material Symbols family)"),
                        number!(FontSize, "Size", c.bar.font_size, 9., 20., 0.5),
                    ],
                ));
                let mut colors = Vec::new();
                for (field, label, value) in [
                    (Field::Background, "Background", c.colors.background),
                    (Field::Foreground, "Text", c.colors.foreground),
                    (Field::Accent, "Accent", c.colors.accent),
                    (Field::OnAccent, "Text on accent", c.colors.on_accent),
                    (Field::Muted, "Secondary text", c.colors.muted),
                    (Field::Urgent, "Alerts", c.colors.urgent),
                ] {
                    colors.push(
                        row()
                            .w_full()
                            .gap(px(10.))
                            .child(swatch(value, 18.))
                            .child(
                                div()
                                    .flex_1()
                                    .child(self.field(&s, &c, field, label, window, cx)),
                            )
                            .into_any_element(),
                    );
                }
                items.push(group("Colours", colors));
                items.push(note(
                    &c,
                    "Hex values such as #cba6f7. Picking a theme replaces these.",
                ));
                items.push(group(
                    "Look",
                    vec![self.segmented(
                        "look",
                        vec![
                            (
                                "Glass".into(),
                                c.bar.look == crate::config::Look::Glass,
                                Msg::Look(crate::config::Look::Glass),
                            ),
                            (
                                "Pixel".into(),
                                c.bar.look == crate::config::Look::Pixel,
                                Msg::Look(crate::config::Look::Pixel),
                            ),
                        ],
                    )],
                ));
                if c.bar.look == crate::config::Look::Pixel {
                    items.push(group(
                        "Pixel font",
                        vec![
                            self.segmented(
                                "pixel-font",
                                [
                                    crate::config::PixelFont::Tiny5,
                                    crate::config::PixelFont::Monocraft,
                                ]
                                .into_iter()
                                .map(|font| {
                                    (
                                        font.family().into(),
                                        c.bar.pixel_font == font,
                                        Msg::PixelFont(font),
                                    )
                                })
                                .collect(),
                            ),
                        ],
                    ));
                }
                items.push(group(
                    "Materials",
                    vec![flag!(Blur, "Translucent menu bar and panels", c.bar.blur)],
                ));
                items.push(note(
                    &c,
                    "Blurs what's behind the menu bar, panels and Settings. Turn off to make them solid.",
                ));
            }
            Page::Bar => {
                items.push(hero(
                    &c,
                    Page::Bar,
                    "Size, layout and what appears in the menu bar.",
                    None,
                ));
                items.push(group(
                    "Size",
                    vec![
                        number!(Height, "Height", c.bar.height, 24., 56., 1.),
                        number!(
                            Width,
                            "Width (0 fills the screen)",
                            c.bar.width,
                            0.,
                            3840.,
                            10.
                        ),
                        number!(Radius, "Corner rounding", c.bar.radius, 0., 28., 1.),
                        number!(
                            MarginTop,
                            "Gap above (0 attaches it to the top edge as a notch)",
                            c.bar.margin_top,
                            0.,
                            32.,
                            1.
                        ),
                        number!(MarginX, "Gap at the sides", c.bar.margin_x, 0., 400., 1.),
                        number!(
                            Gap,
                            "Gap below (space between the menu bar and windows)",
                            c.bar.gap,
                            0.,
                            32.,
                            1.
                        ),
                        number!(Padding, "Side padding", c.bar.padding, 0., 32., 1.),
                        number!(Spacing, "Space between items", c.bar.spacing, 0., 32., 1.),
                    ],
                ));
                for (label, section, list) in [
                    ("Left", Section::Left, &c.bar.left),
                    ("Center", Section::Center, &c.bar.center),
                    ("Right", Section::Right, &c.bar.right),
                ] {
                    let mut rows = Vec::new();
                    for (i, widget) in list.iter().enumerate() {
                        rows.push(
                            row()
                                .w_full()
                                .gap(px(4.))
                                .child(div().flex_1().child(widget.to_string()))
                                .child(self.glyph(
                                    format!("left:{section:?}:{i}"),
                                    "arrow_upward",
                                    Msg::Move(section, i, -1),
                                    false,
                                ))
                                .child(self.glyph(
                                    format!("right:{section:?}:{i}"),
                                    "arrow_downward",
                                    Msg::Move(section, i, 1),
                                    false,
                                ))
                                .child(self.glyph(
                                    format!("remove:{section:?}:{i}"),
                                    "remove_circle",
                                    Msg::Remove(section, i),
                                    true,
                                ))
                                .into_any_element(),
                        );
                    }
                    if rows.is_empty() {
                        rows.push(crate::gpui_widgets::muted(&c, "Empty").into_any_element());
                    }
                    items.push(group(label, rows));
                }
                let target = self.add_target;
                let mut targets = row()
                    .gap(px(2.))
                    .p(px(2.))
                    .rounded(crate::gpui_widgets::corner(7.))
                    .bg(gpui::rgba(0xffffff12));
                for (label, section) in [
                    ("Left", Section::Left),
                    ("Center", Section::Center),
                    ("Right", Section::Right),
                ] {
                    let selected = section == target;
                    targets = targets.child(
                        div()
                            .id(format!("add-target:{section:?}"))
                            .px(px(12.))
                            .py(px(3.))
                            .rounded(crate::gpui_widgets::corner(5.))
                            .text_size(px(12.))
                            .when(selected, |segment| {
                                segment
                                    .bg(gpui::rgba(0xffffff2e))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                            })
                            .cursor_pointer()
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.add_target = section;
                                cx.notify();
                            })),
                    );
                }
                let mut adds = row().w_full().flex_wrap().gap(px(6.));
                for widget in Widget::ALL {
                    let dispatch = self.dispatch.clone();
                    adds = adds.child(
                        row()
                            .id(format!("add:{widget}"))
                            .gap(px(2.))
                            .pl(px(4.))
                            .pr(px(8.))
                            .py(px(2.))
                            .rounded(crate::gpui_widgets::corner(9999.))
                            .text_size(px(11.))
                            .bg(gpui::rgba(0xffffff12))
                            .hover(|style| style.bg(gpui::rgba(0xffffff24)))
                            .cursor_pointer()
                            .child(crate::gpui_widgets::icon(&c, "add", 13.))
                            .child(widget.to_string())
                            .on_click(move |_, window, cx| {
                                dispatch(
                                    crate::Message::Settings(Msg::Add(target, widget)),
                                    window,
                                    cx,
                                )
                            }),
                    );
                }
                items.push(group(
                    "Add an Item",
                    vec![
                        self.labeled("Add to", targets.into_any_element()),
                        adds.into_any_element(),
                    ],
                ));
                items.push(group(
                    "Items",
                    vec![
                        number!(
                            WorkspacesMin,
                            "Workspaces always shown",
                            c.workspaces.min,
                            1.,
                            10.,
                            1.
                        ),
                        field!(ClockFormat, "Clock format (strftime, e.g. %a %d %b %H:%M)"),
                        field!(ClockIcon, "Clock icon (leave empty for none)"),
                        number!(
                            TitleChars,
                            "Title length (characters)",
                            c.window_title.max_chars,
                            10.,
                            120.,
                            1.
                        ),
                    ],
                ));
            }
            Page::Theme => {
                items.push(hero(
                    &c,
                    Page::Theme,
                    "Colour Rune and your apps from a preset or your wallpaper.",
                    Some(self.segmented(
                        "theme-source",
                        vec![
                            (
                                "Presets".into(),
                                c.theme.source == ThemeSource::Preset,
                                Msg::Source(ThemeSource::Preset),
                            ),
                            (
                                "Wallpaper".into(),
                                c.theme.source == ThemeSource::Wallpaper,
                                Msg::Source(ThemeSource::Wallpaper),
                            ),
                        ],
                    )),
                ));
                items.push(group(
                    "Appearance",
                    vec![flag!(ThemeDark, "Dark appearance", c.theme.dark)],
                ));
                if c.theme.source == ThemeSource::Preset {
                    let mut grid = row().w_full().flex_wrap().gap(px(8.));
                    for p in PRESETS[usize::from(c.theme.dark)].iter() {
                        let selected = p.name == c.theme.preset;
                        let rgb = |v: rune_services::theme::Rgb| {
                            gpui::rgb((v.0[0] as u32) << 16 | (v.0[1] as u32) << 8 | v.0[2] as u32)
                        };
                        let dispatch = self.dispatch.clone();
                        let msg = Msg::Preset(p.name.clone());
                        grid = grid.child(
                            column()
                                .id(format!("preset:{}", p.name))
                                .w(px(176.))
                                .gap_0()
                                .rounded(crate::gpui_widgets::corner(8.))
                                .overflow_hidden()
                                .border_2()
                                .border_color(if selected {
                                    color(c.colors.accent)
                                } else {
                                    gpui::rgba(0xffffff14)
                                })
                                .cursor_pointer()
                                .active(|style| style.opacity(0.8))
                                .child(
                                    row().gap_0().h(px(34.)).children(
                                        [p.bg_dim, p.primary, p.secondary, p.tertiary, p.fg]
                                            .into_iter()
                                            .map(|v| div().flex_1().h_full().bg(rgb(v))),
                                    ),
                                )
                                .child(
                                    row()
                                        .justify_between()
                                        .px(px(8.))
                                        .py(px(5.))
                                        .bg(gpui::rgba(0xffffff0a))
                                        .text_size(px(12.))
                                        .child(div().truncate().child(p.name.clone()))
                                        .when(selected, |line| {
                                            line.child(
                                                crate::gpui_widgets::icon(&c, "check", 15.)
                                                    .text_color(color(c.colors.accent)),
                                            )
                                        }),
                                )
                                .on_click(move |_, window, cx| {
                                    dispatch(crate::Message::Settings(msg.clone()), window, cx)
                                }),
                        );
                    }
                    items.push(group("Presets", vec![grid.into_any_element()]));
                } else {
                    let mut rows = Vec::new();
                    for v in Variant::ALL {
                        rows.push(self.choice(
                            format!("scheme:{}", v.id()),
                            None,
                            v.label(),
                            None,
                            v.id() == c.theme.scheme,
                            Msg::Scheme(v),
                        ));
                    }
                    items.push(group("Palette style", rows));
                }
                items.push(group(
                    "Apply to apps",
                    ThemeApp::ALL
                        .iter()
                        .map(|&app| {
                            let on = c.theme.apps.iter().any(|a| a == app.id());
                            self.toggle(
                                app.id(),
                                app.label(),
                                on,
                                Msg::Flag(Flag::ThemeApp(app), !on),
                            )
                        })
                        .collect(),
                ));
                let mut files = vec![
                    row()
                        .w_full()
                        .gap(px(8.))
                        .child(div().flex_1().child("Theme files"))
                        .child(self.action("apply-theme", "Apply Again", Msg::ApplyTheme))
                        .child(self.action(
                            "restore-theme",
                            "Restore Originals",
                            Msg::RestoreThemes,
                        ))
                        .into_any_element(),
                ];
                if !s.status.is_empty() {
                    files.push(crate::gpui_widgets::muted(&c, s.status.clone()).into_any_element());
                }
                items.push(group("Files", files));
                items.push(note(
                    &c,
                    "Rune backs up every file it changes. Restore puts the originals back.",
                ));
            }
            Page::Wallpaper => {
                let current = std::path::Path::new(&c.wallpaper.current)
                    .file_name()
                    .map_or_else(
                        || "No wallpaper chosen yet.".into(),
                        |name| format!("Showing {}", name.to_string_lossy()),
                    );
                items.push(hero(&c, Page::Wallpaper, current, None));
                items.push(group(
                    "Source",
                    vec![
                        row()
                            .w_full()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .child(field!(WallpaperDir, "Folder (images and videos)")),
                            )
                            .child(self.glyph(
                                "refresh-wallpapers",
                                "refresh",
                                Msg::RefreshWallpapers,
                                false,
                            ))
                            .into_any_element(),
                    ],
                ));
                let mut grid = row().w_full().flex_wrap().gap(px(10.));
                for (i, w) in s.wallpapers.iter().enumerate() {
                    let selected = c.wallpaper.current == w.path.display().to_string();
                    let name = w.path.file_stem().unwrap_or_default().to_string_lossy();
                    let preview = match &w.thumb {
                        Some(thumb) => img(thumb.clone())
                            .w(px(166.))
                            .h(px(93.))
                            .rounded(crate::gpui_widgets::corner(6.))
                            .into_any_element(),
                        None => div()
                            .w(px(166.))
                            .h(px(93.))
                            .rounded(crate::gpui_widgets::corner(6.))
                            .bg(gpui::rgba(0xffffff0d))
                            .into_any_element(),
                    };
                    let dispatch = self.dispatch.clone();
                    let path = w.path.clone();
                    grid = grid.child(
                        column()
                            .id(format!("wallpaper:{i}"))
                            .w(px(174.))
                            .gap(px(4.))
                            .cursor_pointer()
                            .active(|style| style.opacity(0.8))
                            .child(
                                div()
                                    .relative()
                                    .p(px(1.))
                                    .rounded(crate::gpui_widgets::corner(8.))
                                    .border_2()
                                    .border_color(if selected {
                                        color(c.colors.accent)
                                    } else {
                                        gpui::rgba(0x00000000)
                                    })
                                    .child(preview)
                                    .when(w.video, |tile| {
                                        tile.child(
                                            crate::gpui_widgets::icon(&c, "play_circle", 20.)
                                                .absolute()
                                                .right(px(8.))
                                                .bottom(px(6.))
                                                .text_color(gpui::rgb(0xffffff)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .px(px(2.))
                                    .text_size(px(11.))
                                    .text_color(color(if selected {
                                        c.colors.foreground
                                    } else {
                                        c.colors.muted
                                    }))
                                    .truncate()
                                    .child(truncate(&name, 28)),
                            )
                            .on_click(move |_, window, cx| {
                                dispatch(
                                    crate::Message::Settings(Msg::SetWallpaper(path.clone())),
                                    window,
                                    cx,
                                )
                            }),
                    );
                }
                if s.wallpapers.is_empty() {
                    items.push(group(
                        "Library",
                        vec![
                            crate::gpui_widgets::muted(
                                &c,
                                "No images or videos here yet. Check the folder, then refresh.",
                            )
                            .into_any_element(),
                        ],
                    ));
                } else {
                    items.push(group("Library", vec![grid.into_any_element()]));
                }
                if !s.status.is_empty() {
                    items.push(note(&c, s.status.clone()));
                }
            }
            Page::Weather => {
                let set = !c.weather.location.is_empty();
                items.push(hero(
                    &c,
                    Page::Weather,
                    if set {
                        c.weather.location.clone()
                    } else {
                        "No place set. Weather stays hidden until you pick one.".into()
                    },
                    set.then(|| self.action("clear-weather", "Clear", Msg::ClearWeather)),
                ));
                let mut rows = vec![
                    row()
                        .w_full()
                        .gap(px(8.))
                        .child(div().flex_1().child(self.input(
                            "weather-search".into(),
                            s.search.clone(),
                            "Search a city",
                            (Msg::Search, Some(Msg::SearchSubmit)),
                            window,
                            cx,
                        )))
                        .child(self.action("search-weather", "Search", Msg::SearchSubmit))
                        .into_any_element(),
                ];
                if !s.search_status.is_empty() {
                    rows.push(
                        crate::gpui_widgets::muted(&c, s.search_status.clone()).into_any_element(),
                    );
                }
                for (i, p) in s.places.iter().enumerate() {
                    rows.push(self.choice(
                        format!("place:{i}"),
                        Some("location_on"),
                        p.label(),
                        None,
                        false,
                        Msg::PickPlace(i),
                    ));
                }
                items.push(group("Location", rows));
                items.push(group(
                    "Units",
                    vec![self.toggle(
                        "imperial",
                        "Fahrenheit and mph",
                        c.weather.imperial,
                        Msg::Imperial(!c.weather.imperial),
                    )],
                ));
                items.push(note(
                    &c,
                    "Forecasts from Open-Meteo. Only the selected place is sent, never your position.",
                ));
            }
            Page::Wifi => {
                if let Some(n) = &s.network {
                    let status = if !n.wifi_enabled {
                        "Off".into()
                    } else if n.connected {
                        format!("Connected to {}", n.name)
                    } else {
                        "On, not connected".into()
                    };
                    items.push(hero(
                        &c,
                        Page::Wifi,
                        status,
                        Some(self.toggle_switch(
                            "wifi-power",
                            n.wifi_enabled,
                            Msg::WifiPower(!n.wifi_enabled),
                        )),
                    ));
                    if !s.wifi_status.is_empty() {
                        items.push(note(&c, s.wifi_status.clone()));
                    }
                    if n.wifi_enabled {
                        let mut rows = Vec::new();
                        for (i, w) in s.wifi.iter().enumerate() {
                            let detail = [
                                w.active.then_some("Connected"),
                                w.saved.as_ref().map(|_| "Saved"),
                                Some(if w.secured { "Secured" } else { "Open" }),
                            ]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" · ");
                            let mut entry = row()
                                .w_full()
                                .gap(px(10.))
                                .child(crate::gpui_widgets::status_circle(
                                    &c,
                                    crate::status::wifi_icon(w.strength),
                                    w.active,
                                    26.,
                                ))
                                .child(
                                    column()
                                        .gap_0()
                                        .flex_1()
                                        .min_w_0()
                                        .child(div().truncate().child(w.ssid.clone()))
                                        .child(
                                            crate::gpui_widgets::muted(&c, detail)
                                                .text_size(px(11.)),
                                        ),
                                );
                            if w.saved.is_some() {
                                entry = entry.child(self.destructive(
                                    format!("forget-wifi:{i}"),
                                    "Forget",
                                    Msg::WifiForget(i),
                                ));
                            }
                            if !w.active {
                                entry = entry.child(self.action(
                                    format!("join-wifi:{i}"),
                                    "Join",
                                    Msg::WifiPick(i),
                                ));
                            }
                            rows.push(entry.into_any_element());
                        }
                        if s.wifi.is_empty() {
                            rows.push(
                                crate::gpui_widgets::muted(&c, "Looking for networks…")
                                    .into_any_element(),
                            );
                        }
                        rows.push(
                            row()
                                .w_full()
                                .justify_end()
                                .child(self.action("wifi-scan", "Scan Again", Msg::WifiRescan))
                                .into_any_element(),
                        );
                        items.push(group("Networks", rows));
                        if let Some(pick) = &s.wifi_pick {
                            let password = self.input(
                                "wifi-password".into(),
                                s.wifi_password.clone(),
                                "Password",
                                (Msg::WifiPassword, Some(Msg::WifiJoin)),
                                window,
                                cx,
                            );
                            items.push(group(
                                &format!("Join “{pick}”"),
                                vec![
                                    self.labeled(
                                        "Password",
                                        div().w(px(240.)).child(password).into_any_element(),
                                    ),
                                    row()
                                        .w_full()
                                        .justify_end()
                                        .gap(px(8.))
                                        .child(self.action(
                                            "wifi-cancel",
                                            "Cancel",
                                            Msg::WifiCancel,
                                        ))
                                        .child(self.primary("wifi-submit", "Join", Msg::WifiJoin))
                                        .into_any_element(),
                                ],
                            ));
                        }
                    }
                } else {
                    items.push(hero(
                        &c,
                        Page::Wifi,
                        "Unavailable. NetworkManager isn't running.",
                        None,
                    ));
                }
            }
            Page::Bluetooth => {
                let bt = &s.bt;
                if !bt.available {
                    items.push(hero(
                        &c,
                        Page::Bluetooth,
                        "No Bluetooth adapter found, or BlueZ isn't running.",
                        None,
                    ));
                } else {
                    items.push(hero(
                        &c,
                        Page::Bluetooth,
                        if !bt.powered {
                            "Off"
                        } else if bt.discovering {
                            "On · looking for nearby devices…"
                        } else {
                            "On"
                        },
                        Some(self.toggle_switch(
                            "bt-power",
                            bt.powered,
                            Msg::Bt(BtCommand::Power(!bt.powered)),
                        )),
                    ));
                    if let Some(code) = &bt.pairing_code {
                        items.push(group(
                            "Pairing",
                            vec![
                                crate::gpui_widgets::muted(
                                    &c,
                                    "Type this code on the device, then press Enter.",
                                )
                                .into_any_element(),
                                div()
                                    .text_size(px(28.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(code.clone())
                                    .into_any_element(),
                            ],
                        ));
                    }
                    if !bt.error.is_empty() {
                        items.push(
                            div()
                                .px(px(4.))
                                .text_color(color(c.colors.urgent))
                                .child(bt.error.clone())
                                .into_any_element(),
                        );
                    }
                    for (i, d) in bt.devices.iter().enumerate().filter(|(_, d)| d.paired) {
                        let status = [
                            Some(
                                if d.connected {
                                    "Connected"
                                } else {
                                    "Not Connected"
                                }
                                .to_owned(),
                            ),
                            d.battery.map(|b| format!("{b}% battery")),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ");
                        let (label, cmd) = if d.connected {
                            ("Disconnect", BtCommand::Disconnect(d.path.clone()))
                        } else {
                            ("Connect", BtCommand::Connect(d.path.clone()))
                        };
                        let mut rows = vec![
                            row()
                                .w_full()
                                .gap(px(10.))
                                .child(crate::gpui_widgets::status_circle(
                                    &c,
                                    crate::bt::device_icon(&d.icon),
                                    d.connected,
                                    30.,
                                ))
                                .child(
                                    column()
                                        .gap_0()
                                        .flex_1()
                                        .min_w_0()
                                        .child(div().truncate().child(d.name.clone()))
                                        .child(
                                            crate::gpui_widgets::muted(&c, status)
                                                .text_size(px(11.)),
                                        ),
                                )
                                .child(self.action(format!("bt-action:{i}"), label, Msg::Bt(cmd)))
                                .into_any_element(),
                        ];
                        if let Some(p) = d.profiles.as_ref().filter(|_| d.connected) {
                            let options = [("Audio", &p.audio), ("Call", &p.call)]
                                .into_iter()
                                .filter_map(|(label, profile)| {
                                    profile.as_ref().map(|profile| {
                                        (
                                            label.to_owned(),
                                            &p.active == profile,
                                            Msg::Bt(BtCommand::Profile(
                                                d.address.clone(),
                                                profile.clone(),
                                            )),
                                        )
                                    })
                                })
                                .collect::<Vec<_>>();
                            if !options.is_empty() {
                                rows.push(self.labeled(
                                    "Mode (Audio plays in high quality; Call enables the microphone)",
                                    self.segmented(&format!("profile:{i}"), options),
                                ));
                            }
                        }
                        rows.push(self.toggle(
                            &format!("bt-trust:{}", d.address),
                            "Connect automatically",
                            d.trusted,
                            Msg::Bt(BtCommand::Trust(d.path.clone(), !d.trusted)),
                        ));
                        rows.push(self.destructive(
                            format!("bt-forget:{i}"),
                            "Forget This Device",
                            Msg::Bt(BtCommand::Remove(d.path.clone())),
                        ));
                        items.push(group(&d.name, rows));
                    }
                    let nearby = bt
                        .devices
                        .iter()
                        .enumerate()
                        .filter(|(_, d)| !d.paired)
                        .map(|(i, d)| {
                            row()
                                .w_full()
                                .gap(px(10.))
                                .child(crate::gpui_widgets::status_circle(
                                    &c,
                                    crate::bt::device_icon(&d.icon),
                                    false,
                                    26.,
                                ))
                                .child(div().flex_1().min_w_0().truncate().child(d.name.clone()))
                                .child(self.action(
                                    format!("bt-action:{i}"),
                                    "Pair",
                                    Msg::Bt(BtCommand::Pair(d.path.clone())),
                                ))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>();
                    if bt.powered {
                        items.push(group(
                            "Nearby Devices",
                            if nearby.is_empty() {
                                vec![
                                    crate::gpui_widgets::muted(
                                        &c,
                                        "Put your device in pairing mode to see it here.",
                                    )
                                    .into_any_element(),
                                ]
                            } else {
                                nearby
                            },
                        ));
                    }
                }
            }
            Page::Sound => {
                items.push(hero(
                    &c,
                    Page::Sound,
                    "Choose where sound plays and which microphone apps use.",
                    None,
                ));
                for (label, devices, output) in [
                    ("Output", &s.audio.outputs, true),
                    ("Input", &s.audio.inputs, false),
                ] {
                    let mut rows = devices
                        .iter()
                        .enumerate()
                        .map(|(i, d)| {
                            self.choice(
                                format!("device:{output}:{i}"),
                                Some(if output { "speaker" } else { "mic" }),
                                d.description.clone(),
                                None,
                                d.default,
                                Msg::Audio(DeviceCommand::SetDefault(output, d.name.clone())),
                            )
                        })
                        .collect::<Vec<_>>();
                    if devices.is_empty() {
                        rows.push(
                            crate::gpui_widgets::muted(&c, "No devices found.").into_any_element(),
                        );
                    }
                    if !output && let Some(mic) = s.audio.mic() {
                        rows.push(self.toggle(
                            "mute-mic",
                            "Mute microphone",
                            mic.muted,
                            Msg::Audio(DeviceCommand::ToggleMicMute),
                        ));
                    }
                    items.push(group(label, rows));
                }
            }
            Page::Calendar => {
                items.push(hero(
                    &c,
                    Page::Calendar,
                    "Show events from your calendars when you click the clock.",
                    None,
                ));
                let mut rows = Vec::new();
                for (i, url) in c.calendar.urls.iter().enumerate() {
                    rows.push(
                        row()
                            .w_full()
                            .gap(px(10.))
                            .child(
                                crate::gpui_widgets::icon(&c, "calendar_month", 18.)
                                    .text_color(color(c.colors.muted)),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(truncate(url, 60)))
                            .child(self.glyph(
                                format!("calendar-remove:{i}"),
                                "remove_circle",
                                Msg::RemoveLink(i),
                                true,
                            ))
                            .into_any_element(),
                    );
                }
                rows.push(
                    row()
                        .w_full()
                        .gap(px(8.))
                        .child(div().flex_1().child(self.input(
                            "calendar-link".into(),
                            s.new_link.clone(),
                            "https://…/basic.ics",
                            (Msg::NewLink, Some(Msg::AddLink)),
                            window,
                            cx,
                        )))
                        .child(self.action("calendar-add", "Add", Msg::AddLink))
                        .into_any_element(),
                );
                for error in &s.cal_errors {
                    rows.push(
                        div()
                            .text_size(px(11.))
                            .text_color(color(c.colors.urgent))
                            .child(error.clone())
                            .into_any_element(),
                    );
                }
                items.push(group("Calendars", rows));
                items.push(note(
                    &c,
                    "Use a read-only ICS link. Google: Settings → calendar → Secret address in iCal format. Outlook: Shared calendars → Publish → ICS. Apple, Nextcloud, Proton: share link (webcal:// works). Refreshes every 15 minutes.",
                ));
            }
            Page::Capture => {
                items.push(hero(
                    &c,
                    Page::Capture,
                    "Screenshots, screen recordings and copying text from the screen.",
                    None,
                ));
                items.push(group(
                    "Screenshots",
                    vec![
                        field!(ScreenshotDir, "Save to"),
                        flag!(CaptureCopy, "Copy to clipboard", c.capture.copy),
                    ],
                ));
                items.push(group(
                    "Screen Recording",
                    vec![
                        field!(RecordDir, "Save to"),
                        flag!(RecordAudio, "Record system audio", c.capture.record_audio),
                        number!(Fps, "Frame rate", c.capture.fps, 24., 144., 1.),
                    ],
                ));
                items.push(group(
                    "Text Recognition",
                    vec![field!(
                        OcrLang,
                        "Languages (Tesseract codes such as eng or eng+deu)"
                    )],
                ));
                items.push(group(
                    "Shortcuts",
                    [
                        "hl.bind(\"Print\", hl.dsp.exec_cmd(\"rune-shell screenshot selection\"))",
                        "hl.bind(\"SHIFT + Print\", hl.dsp.exec_cmd(\"rune-shell screenshot window\"))",
                        "hl.bind(\"CTRL + Print\", hl.dsp.exec_cmd(\"rune-shell screenshot screen\"))",
                        "hl.bind(\"SUPER + Print\", hl.dsp.exec_cmd(\"rune-shell record selection\"))",
                        "hl.bind(\"SUPER + SHIFT + T\", hl.dsp.exec_cmd(\"rune-shell ocr\"))",
                    ]
                    .into_iter()
                    .map(code)
                    .collect(),
                ));
                items.push(note(
                    &c,
                    "Add these to your Hyprland config. Run the record command again, or click the timer, to stop.",
                ));
            }
            Page::Launcher => {
                items.push(hero(
                    &c,
                    Page::Launcher,
                    "Search apps and Steam games, type : for emoji, or type a sum like 12*3.",
                    None,
                ));
                items.push(group(
                    "Apps",
                    vec![field!(
                        Terminal,
                        "Terminal (runs terminal apps as <terminal> -e <app>)"
                    )],
                ));
                items.push(group(
                    "Shortcuts",
                    [
                        "hl.bind(\"SUPER + SPACE\", hl.dsp.exec_cmd(\"rune-shell toggle launcher\"))",
                        "hl.bind(\"SUPER + SLASH\", hl.dsp.exec_cmd(\"rune-shell toggle keybinds\"))",
                        "hl.bind(\"SUPER + COMMA\", hl.dsp.exec_cmd(\"rune-shell toggle settings\"))",
                    ]
                    .into_iter()
                    .map(code)
                    .collect(),
                ));
                items.push(note(
                    &c,
                    "Add these to your Hyprland config, using any keys.",
                ));
            }
            Page::Animations => {
                items.push(hero(
                    &c,
                    Page::Animations,
                    "How Rune moves when things open, close and change.",
                    None,
                ));
                items.push(group(
                    "Motion",
                    vec![
                        flag!(AnimEnabled, "Animate changes", c.animations.enabled),
                        number!(
                            AnimDuration,
                            "Speed (milliseconds; lower is snappier)",
                            c.animations.duration_ms,
                            50.,
                            1000.,
                            10.
                        ),
                    ],
                ));
                items.push(group(
                    "Accessibility",
                    vec![flag!(
                        ReduceMotion,
                        "Reduce motion (changes happen instantly)",
                        c.animations.reduce_motion
                    )],
                ));
            }
        }
        let search = self.input(
            "settings-search".into(),
            s.page_search.clone(),
            "Search",
            (Msg::PageSearch, None),
            window,
            cx,
        );
        let groups = super::visible_groups(&s.page_search);
        let foreground = color(c.colors.foreground);
        let mut sidebar = column()
            .id("settings-navigation")
            .overflow_y_scroll()
            .w(px(230.))
            .h_full()
            .flex_shrink_0()
            .px(px(10.))
            .py(px(12.))
            .gap(px(2.))
            .bg(crate::gpui_widgets::material(
                &c,
                crate::gpui_widgets::Material::Sidebar,
            ))
            .rounded_l(crate::gpui_widgets::corner(12.))
            .border_r_1()
            .border_color(gpui::rgba(0x00000066))
            .child(
                row()
                    .gap(px(6.))
                    .mb(px(10.))
                    .pl(px(8.))
                    .rounded(crate::gpui_widgets::corner(7.))
                    .bg(foreground.opacity(0.08))
                    .text_size(px(13.))
                    .child(crate::gpui_widgets::icon(&c, "search", 15.).opacity(0.5))
                    .child(div().flex_1().child(search)),
            );
        if groups.is_empty() {
            sidebar = sidebar.child(
                div()
                    .px(px(8.))
                    .py(px(12.))
                    .text_color(color(c.colors.muted))
                    .child("No Results"),
            );
        }
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                sidebar = sidebar.child(div().h(px(12.)));
            }
            for page in group {
                let selected = page == s.page;
                let dispatch = self.dispatch.clone();
                sidebar = sidebar.child(
                    row()
                        .id(format!("page:{page:?}"))
                        .gap(px(8.))
                        .w_full()
                        .px(px(6.))
                        .py(px(4.))
                        .rounded(crate::gpui_widgets::corner(6.))
                        .when(selected, |item| {
                            item.bg(color(c.colors.accent))
                                .text_color(color(c.colors.on_accent))
                        })
                        .when(!selected, |item| {
                            item.hover(|style| style.bg(foreground.opacity(0.08)))
                        })
                        .active(|style| style.opacity(0.85))
                        .cursor_pointer()
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .size(px(22.))
                                .rounded(crate::gpui_widgets::corner(6.))
                                .bg(gpui::rgba(page.tint()))
                                .text_color(gpui::rgb(0xffffff))
                                .child(crate::gpui_widgets::icon(&c, page.label().0, 15.)),
                        )
                        .child(page.label().1)
                        .on_click(move |_, window, cx| {
                            dispatch(crate::Message::Settings(Msg::Page(page)), window, cx)
                        }),
                );
            }
        }
        let escape = self.dispatch.clone();
        row()
            .id("settings-root")
            .track_focus(&self.focus)
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    escape(crate::Message::Settings(Msg::Close), window, cx);
                }
            })
            .relative()
            .size_full()
            .gap_0()
            .rounded(crate::gpui_widgets::corner(12.))
            .overflow_hidden()
            .items_start()
            .font_family(crate::gpui_widgets::text_font(&c))
            .text_size(px(13.))
            .text_color(foreground)
            .bg(crate::gpui_widgets::material(
                &c,
                crate::gpui_widgets::Material::Content,
            ))
            .child(sidebar)
            .child(
                column()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_0()
                    .child(
                        row()
                            .h(px(52.))
                            .flex_none()
                            .pl(px(22.))
                            .pr(px(12.))
                            .justify_between()
                            .child(
                                crate::gpui_widgets::heading(div())
                                    .text_size(px(15.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(s.page.label().1),
                            )
                            .child(self.glyph("settings-close", "close", Msg::Close, false)),
                    )
                    .child(
                        div()
                            .id(format!("settings-scroll:{:?}", s.page))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(22.))
                            .pb(px(22.))
                            .child(
                                column()
                                    .w_full()
                                    .max_w(px(620.))
                                    .mx_auto()
                                    .gap(px(18.))
                                    .children(items),
                            ),
                    ),
            )
            .child(crate::gpui_widgets::edge(&c, 12.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "opens native settings on Wayland; run one GPUI test per process"]
    fn native_all_pages_and_validated_edits() {
        use gpui::{EntityInputHandler, Focusable, QuitMode};
        gpui_platform::application()
            .with_quit_mode(QuitMode::Explicit)
            .run(|cx| {
                TextInput::bind_keys(cx);
                let (actions, _) = tokio::sync::mpsc::unbounded_channel();
                let (_, events) = tokio::sync::mpsc::unbounded_channel();
                let (app, _) = crate::App::new(Config::default(), events, actions, false);
                let shell = cx.new(|_| app);
                let closed = std::rc::Rc::new(std::cell::Cell::new(false));
                let dispatch: Dispatch = {
                    let shell = shell.clone();
                    let closed = closed.clone();
                    std::rc::Rc::new(move |message, _, cx| {
                        if matches!(message, crate::Message::Settings(Msg::Close)) {
                            closed.set(true);
                        }
                        shell.update(cx, |app, cx| {
                            let _ = app.update(message);
                            cx.notify();
                        });
                    })
                };
                let handle = cx
                    .open_window(
                        crate::gpui_surfaces::options(
                            &crate::Surface::Settings,
                            (860, 580),
                            None,
                            true,
                        ),
                        |window, cx| {
                            cx.new(|cx| SettingsView::new(shell.clone(), dispatch, window, cx))
                        },
                    )
                    .unwrap();
                image::RgbaImage::from_pixel(160, 90, image::Rgba([90, 110, 180, 255]))
                    .save("/tmp/settings-fixture.png")
                    .unwrap();
                cx.spawn(async move |cx| {
                    let delay = std::time::Duration::from_millis(70);
                    cx.background_executor().timer(delay).await;
                    // Native input events must drive the shared validation model, and
                    // accepted edits must retain the same focused editor entity.
                    cx.update(|cx| {
                        handle
                            .update(cx, |view, window, cx| {
                                let input = view.inputs["field:Accent"].1.clone();
                                window.focus(&input.focus_handle(cx), cx);
                                input.update(cx, |input, cx| {
                                    input.replace_text_in_range(Some(0..9), "#12", window, cx)
                                });
                            })
                            .unwrap()
                    });
                    cx.background_executor().timer(delay).await;
                    cx.update(|cx| {
                        assert!(
                            shell
                                .read(cx)
                                .settings_ui
                                .drafts
                                .contains_key(&Field::Accent)
                        );
                        handle
                            .update(cx, |view, window, cx| {
                                let input = view.inputs["field:Accent"].1.clone();
                                assert!(input.focus_handle(cx).is_focused(window));
                                input.update(cx, |input, cx| {
                                    input.replace_text_in_range(Some(0..3), "#123456", window, cx)
                                });
                            })
                            .unwrap();
                    });
                    cx.background_executor().timer(delay).await;
                    cx.update(|cx| {
                        assert_eq!(
                            shell.read(cx).config.colors.accent,
                            Rgba([0x12, 0x34, 0x56, 255])
                        );
                        assert!(
                            !shell
                                .read(cx)
                                .settings_ui
                                .drafts
                                .contains_key(&Field::Accent)
                        );
                        handle
                            .update(cx, |view, window, cx| {
                                assert!(
                                    view.inputs["field:Accent"]
                                        .1
                                        .focus_handle(cx)
                                        .is_focused(window)
                                )
                            })
                            .unwrap();
                    });
                    for populated in [false, true] {
                        if populated {
                            cx.update(|cx| {
                                shell.update(cx, |app, cx| {
                                    let s = &mut app.settings_ui;
                                    s.network = Some(Network {
                                        connected: true,
                                        online: true,
                                        kind: rune_services::system::NetKind::Wifi,
                                        name: "Test network".into(),
                                        strength: Some(90),
                                        wifi_enabled: true,
                                    });
                                    s.wifi = vec![WifiNetwork {
                                        ssid: "Test network".into(),
                                        strength: 90,
                                        secured: true,
                                        active: false,
                                        saved: None,
                                        device: "/device/test".into(),
                                        ap: "/ap/test".into(),
                                    }];
                                    s.wifi_pick = Some("Test network".into());
                                    s.wifi_password = "test-secret".into();
                                    s.bt.available = true;
                                    s.bt.powered = true;
                                    s.bt.pairing_code = Some("123456".into());
                                    s.bt.error = "Test error".into();
                                    s.bt.devices = vec![rune_services::bluetooth::Device {
                                        path: "/test/headset".into(),
                                        name: "Test headset".into(),
                                        address: "00:11:22:33:44:55".into(),
                                        icon: "audio-headset".into(),
                                        paired: true,
                                        trusted: true,
                                        connected: true,
                                        battery: Some(75),
                                        rssi: None,
                                        profiles: Some(rune_services::bluetooth::Profiles {
                                            active: "a2dp".into(),
                                            audio: Some("a2dp".into()),
                                            call: Some("headset".into()),
                                        }),
                                    }];
                                    let device = rune_services::audio::Device {
                                        name: "test".into(),
                                        description: "Test audio device".into(),
                                        muted: false,
                                        default: true,
                                    };
                                    s.audio.outputs = vec![device.clone()];
                                    s.audio.inputs = vec![device];
                                    s.places = vec![Location {
                                        name: "Manila".into(),
                                        latitude: 14.6,
                                        longitude: 121.,
                                        country: "Philippines".into(),
                                        admin1: String::new(),
                                    }];
                                    s.cal_errors = vec!["Example calendar fetch error".into()];
                                    app.config.calendar.urls =
                                        vec!["https://calendar.example/private/basic.ics".into()];
                                    s.wallpapers = vec![Wallpaper {
                                        path: "/tmp/settings-fixture.png".into(),
                                        thumb: Some("/tmp/settings-fixture.png".into()),
                                        video: false,
                                    }];
                                    cx.notify();
                                })
                            });
                        }
                        for page in Page::ALL {
                            cx.update(|cx| {
                                shell.update(cx, |app, cx| {
                                    let _ = app.update(crate::Message::Settings(Msg::Page(page)));
                                    cx.notify();
                                })
                            });
                            cx.background_executor().timer(delay).await;
                            cx.update(|cx| {
                                handle
                                    .update(cx, |view, window, cx| {
                                        assert_eq!(view.page, page);
                                        if page != Page::Appearance {
                                            assert!(view.focus.is_focused(window));
                                        }
                                        let _ = view.render(window, cx);
                                    })
                                    .unwrap()
                            });
                        }
                        // Wallpaper-derived schemes are a second render branch.
                        cx.update(|cx| {
                            shell.update(cx, |app, cx| {
                                app.config.theme.source = ThemeSource::Wallpaper;
                                app.settings_ui.page = Page::Theme;
                                cx.notify();
                            })
                        });
                        cx.background_executor().timer(delay).await;
                    }
                    cx.update(|cx| {
                        cx.update_window(handle.into(), |_, window, cx| {
                            window
                                .dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
                        })
                        .unwrap();
                        assert!(closed.get(), "Escape must close settings from root focus");
                        let _ = std::fs::remove_file("/tmp/settings-fixture.png");
                        cx.quit();
                    });
                })
                .detach();
            });
    }
}
