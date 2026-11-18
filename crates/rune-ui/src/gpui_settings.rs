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
use gpui::{
    AnimationExt, AnyElement, Context, Entity, Focusable, Window, div, img, prelude::*, px,
};

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
    /// Spring selected on the Window Motion page.
    hypr_curve: Option<String>,
    /// Bumped to replay the spring preview.
    hypr_stage: usize,
    hypr_new_curve: bool,
    hypr_new_spring: bool,
    hypr_drag: Option<(String, usize, gpui::Point<gpui::Pixels>, BezierView)>,
    /// The open dropdown on the Hyprland pages.
    hypr_select: Option<String>,
    /// "Not now" on the setup card.
    hypr_later: bool,
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
            hypr_curve: None,
            hypr_stage: 0,
            hypr_new_curve: false,
            hypr_new_spring: false,
            hypr_drag: None,
            hypr_select: None,
            hypr_later: false,
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
    fn audio_volume(
        &mut self,
        c: &Config,
        microphone: bool,
        value: u16,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = if microphone {
            "mic-volume"
        } else {
            "sound-volume"
        };
        let value = f32::from(value.min(100));
        let entity = if let Some(entity) = self.sliders.get(key) {
            entity.clone()
        } else {
            let entity = cx.new(|_| Slider::new((0., 100.), value, color(c.colors.accent)));
            let dispatch = self.dispatch.clone();
            cx.subscribe_in(&entity, window, move |_, _, event, window, cx| {
                if let SliderEvent::Change(value) = event {
                    let message = if microphone {
                        crate::Message::QuickMicVolume(*value)
                    } else {
                        crate::Message::QuickVolume(*value)
                    };
                    dispatch(message, window, cx);
                }
            })
            .detach();
            self.sliders.insert(key.into(), entity.clone());
            entity
        };
        entity.update(cx, |slider, _| {
            slider.sync((0., 100.), value, color(c.colors.accent));
            slider.set_pill(Some(crate::controls::slider::Pill {
                glyph: if microphone { "mic" } else { "volume_up" }.into(),
                font: self.icon_font.clone(),
                fill: color(c.colors.foreground),
                ink: color(c.colors.background),
            }));
        });
        column()
            .w_full()
            .gap(px(6.))
            .child(label_block(
                &c.colors,
                if microphone {
                    "Microphone volume"
                } else {
                    "Sound volume"
                },
            ))
            .child(entity)
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
impl SettingsView {
    fn eq_presets(&self, c: &Config) -> AnyElement {
        use rune_services::equalizer::{PRESETS, preset_of};
        let current = preset_of(&c.equalizer.bands).map(|p| p.id);
        let chip = |id: String, label: &'static str, selected: bool| {
            crate::gpui_widgets::frame(div())
                .id(id)
                .px(px(12.))
                .py(px(4.))
                .rounded(crate::gpui_widgets::corner(9999.))
                .text_size(px(12.))
                .map(|chip| {
                    if selected {
                        chip.bg(color(c.colors.accent))
                            .text_color(color(c.colors.on_accent))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                    } else {
                        chip.bg(gpui::rgba(0xffffff12))
                            .hover(|style| style.bg(gpui::rgba(0xffffff24)))
                    }
                })
                .child(label)
        };
        let mut chips = row().w_full().flex_wrap().gap(px(6.));
        for (index, preset) in PRESETS.iter().enumerate() {
            let dispatch = self.dispatch.clone();
            chips = chips.child(
                chip(
                    format!("eq-preset:{}", preset.id),
                    preset.label,
                    current == Some(preset.id),
                )
                .cursor_pointer()
                .active(|style| style.opacity(0.8))
                .on_click(move |_, window, cx| {
                    dispatch(crate::Message::Settings(Msg::EqPreset(index)), window, cx)
                }),
            );
        }
        if current.is_none() {
            chips = chips.child(chip("eq-preset:custom".into(), "Custom", true));
        }
        chips.into_any_element()
    }
    fn eq_band(
        &mut self,
        c: &Config,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use rune_services::equalizer::{BAND_HZ, MAX_DB};
        let hz = BAND_HZ.get(index).copied().unwrap_or_default();
        let gain = c.equalizer.bands.get(index).copied().unwrap_or_default();
        let range = (-MAX_DB, MAX_DB);
        let key = format!("eq-band:{hz}");
        let accent = color(c.colors.accent);
        let entity = if let Some(entity) = self.sliders.get(&key) {
            entity.update(cx, |s, _| s.sync(range, gain, accent));
            entity.clone()
        } else {
            let entity = cx.new(|_| {
                let mut slider = Slider::new(range, gain, accent);
                slider.set_vertical(0.);
                slider
            });
            let dispatch = self.dispatch.clone();
            cx.subscribe_in(&entity, window, move |_, _, event, window, cx| {
                if let SliderEvent::Change(db) = event {
                    dispatch(
                        crate::Message::Settings(Msg::EqBand(index, *db)),
                        window,
                        cx,
                    )
                }
            })
            .detach();
            self.sliders.insert(key.clone(), entity.clone());
            entity
        };
        let label = if hz >= 1000 {
            format!("{}k", hz / 1000)
        } else {
            hz.to_string()
        };
        column()
            .id(key)
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(if gain == 0. {
                        color(c.colors.muted)
                    } else {
                        color(c.colors.foreground)
                    })
                    .child(format!("{gain:+.1}")),
            )
            .child(div().h(px(160.)).child(entity))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(color(c.colors.muted))
                    .child(label),
            )
            .into_any_element()
    }
}
/// A bar item being dragged: from a section slot, or new from the shelf.
struct DraggedWidget {
    widget: Widget,
    from: Option<(Section, usize)>,
}
struct WidgetGhost {
    widget: Widget,
    font: gpui::SharedString,
    colors: crate::config::Colors,
}
impl Render for WidgetGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::gpui_widgets::frame(row())
            .gap(px(8.))
            .px(px(10.))
            .py(px(6.))
            .rounded(crate::gpui_widgets::corner(7.))
            .bg(color(self.colors.background))
            .text_color(color(self.colors.foreground))
            .shadow_lg()
            .child(
                div()
                    .font_family(self.font.clone())
                    .text_size(px(16.))
                    .child(widget_glyph(self.widget)),
            )
            .child(self.widget.to_string())
    }
}
fn widget_glyph(widget: Widget) -> &'static str {
    match widget {
        Widget::Workspaces => "grid_view",
        Widget::WindowTitle => "web_asset",
        Widget::Clock => "schedule",
        Widget::Media => "music_note",
        Widget::Notifications => "notifications",
        Widget::Tray => "apps",
        Widget::Weather => "partly_cloudy_day",
        Widget::Bluetooth => "bluetooth",
        Widget::Volume => "volume_up",
        Widget::Network => "wifi",
        Widget::Battery => "battery_5_bar",
        Widget::Stats => "memory",
        Widget::QuickSettings => "toggle_on",
        Widget::Power => "power_settings_new",
        Widget::Capture => "photo_camera",
        Widget::Settings => "settings",
    }
}
const SECTIONS: [(&str, Section); 3] = [
    ("Left", Section::Left),
    ("Center", Section::Center),
    ("Right", Section::Right),
];
fn section_items(c: &Config, section: Section) -> &[Widget] {
    match section {
        Section::Left => &c.bar.left,
        Section::Center => &c.bar.center,
        Section::Right => &c.bar.right,
    }
}
/// Mock screen showing the bar's shape, spacing and items as configured.
/// Heights are drawn 1:1; widths are scaled from the settings window's monitor.
fn bar_preview(c: &Config, window: &Window, cx: &gpui::App) -> AnyElement {
    let screen = window
        .display(cx)
        .map(|display| f32::from(display.bounds().size.width))
        .filter(|width| *width > 0.)
        .unwrap_or(2560.);
    let side = (c.bar.margin_x as f32 / screen).min(0.5);
    let fraction = match c.bar.width {
        0 => 1. - 2. * side,
        width => (width as f32 / screen).min(1. - 2. * side),
    };
    let radius = crate::gpui_widgets::corner(c.bar.radius);
    let height = c.bar.height as f32;
    let top = c.bar.margin_top as f32;
    let glyph_size = (height * 0.5).clamp(10., 20.);
    let item = |widget: Widget| -> AnyElement {
        let glyph = |name: &str| crate::gpui_widgets::icon(c, name, glyph_size);
        match widget {
            Widget::Workspaces => row()
                .gap(px(4.))
                .children((0..c.workspaces.min.clamp(1, 10)).map(|index| {
                    div()
                        .h(px(6.))
                        .w(px(if index == 0 { 14. } else { 6. }))
                        .rounded(crate::gpui_widgets::corner(3.))
                        .bg(color(c.colors.foreground).opacity(if index == 0 { 1. } else { 0.4 }))
                }))
                .into_any_element(),
            Widget::Clock if crate::config::valid_clock_format(&c.clock.format) => row()
                .gap(px(4.))
                .when(!c.clock.icon.is_empty(), |line| {
                    line.child(glyph(&c.clock.icon))
                })
                .child(chrono::Local::now().format(&c.clock.format).to_string())
                .into_any_element(),
            Widget::WindowTitle => div()
                .text_color(color(c.colors.muted))
                .child(crate::settings::truncate(
                    "Firefox — Rune Shell",
                    c.window_title.max_chars,
                ))
                .into_any_element(),
            widget => glyph(widget_glyph(widget)).into_any_element(),
        }
    };
    let section = |section: Section| {
        row()
            .gap(px(c.bar.spacing))
            .flex_none()
            .children(section_items(c, section).iter().map(|&widget| item(widget)))
    };
    let mut bar = crate::gpui_widgets::frame(row())
        .absolute()
        .top(px(top))
        .left(gpui::relative((1. - fraction) / 2.))
        .w(gpui::relative(fraction))
        .h(px(height))
        .justify_between()
        .px(px(c.bar.padding))
        .overflow_hidden()
        .bg(color(c.colors.background))
        .text_color(color(c.colors.foreground))
        .text_size(px(c.bar.font_size.clamp(9., 14.)))
        .child(section(Section::Left))
        .child(section(Section::Right))
        .child(
            row()
                .absolute()
                .inset_0()
                .justify_center()
                .child(section(Section::Center)),
        );
    bar = if top == 0. {
        bar.rounded_b(radius)
    } else {
        bar.rounded(radius)
    };
    let windows_top = top + height + c.bar.gap as f32;
    crate::gpui_widgets::frame(div())
        .relative()
        .w_full()
        .h(px(150.))
        .overflow_hidden()
        .rounded(crate::gpui_widgets::corner(10.))
        .bg(gpui::linear_gradient(
            135.,
            gpui::linear_color_stop(color(c.colors.accent).opacity(0.45), 0.),
            gpui::linear_color_stop(color(c.colors.background), 1.),
        ))
        .child(
            div()
                .absolute()
                .top(px(windows_top))
                .left(px(14.))
                .right(px(14.))
                .bottom(px(14.))
                .rounded(crate::gpui_widgets::corner(8.))
                .border_1()
                .border_color(gpui::rgba(0xffffff24))
                .bg(gpui::rgba(0xffffff0d)),
        )
        .child(bar)
        .into_any_element()
}
impl SettingsView {
    /// Left, Center and Right columns of draggable items, then a shelf of every item.
    fn bar_layout(&self, c: &Config, cx: &mut Context<Self>) -> AnyElement {
        let accent = color(c.colors.accent);
        let ghost = {
            let font = self.icon_font.clone();
            let colors = self.colors;
            move |drag: &DraggedWidget,
                  _: gpui::Point<gpui::Pixels>,
                  _: &mut Window,
                  cx: &mut gpui::App| {
                cx.new(|_| WidgetGhost {
                    widget: drag.widget,
                    font: font.clone(),
                    colors,
                })
            }
        };
        let drop_at = |section: Section, at: usize| {
            let dispatch = self.dispatch.clone();
            move |drag: &DraggedWidget, window: &mut Window, cx: &mut gpui::App| {
                let msg = match drag.from {
                    Some(from) => Msg::Place(from, (section, at)),
                    None => Msg::Insert(section, at, drag.widget),
                };
                dispatch(crate::Message::Settings(msg), window, cx)
            }
        };
        let mut lanes = row().w_full().items_start().gap(px(8.));
        for (label, section) in SECTIONS {
            let list = section_items(c, section);
            let targeted = section == self.add_target;
            let mut lane = column()
                .id(format!("lane:{section:?}"))
                .flex_1()
                .min_w_0()
                .min_h(px(120.))
                .gap(px(6.))
                .p(px(6.))
                .rounded(crate::gpui_widgets::corner(8.))
                .bg(gpui::rgba(0x0000002e))
                .border_1()
                .border_dashed()
                .border_color(if targeted {
                    gpui::rgba(0xffffff40)
                } else {
                    gpui::rgba(0x00000000)
                })
                .drag_over::<DraggedWidget>(move |style, _, _, _| {
                    style.border_color(accent).bg(accent.opacity(0.12))
                })
                .on_drop(drop_at(section, usize::MAX))
                .child(
                    row()
                        .id(format!("lane-head:{section:?}"))
                        .w_full()
                        .justify_between()
                        .px(px(2.))
                        .text_size(px(11.))
                        .text_color(color(c.colors.muted))
                        .cursor_pointer()
                        .child(label)
                        .child(list.len().to_string())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.add_target = section;
                            cx.notify();
                        })),
                );
            for (index, &widget) in list.iter().enumerate() {
                lane = lane.child(
                    crate::gpui_widgets::frame(row())
                        .id(format!("chip:{section:?}:{index}:{widget:?}"))
                        .w_full()
                        .gap(px(6.))
                        .pl(px(4.))
                        .pr(px(2.))
                        .py(px(2.))
                        .rounded(crate::gpui_widgets::corner(7.))
                        .bg(gpui::rgba(0xffffff12))
                        .hover(|style| style.bg(gpui::rgba(0xffffff1c)))
                        .active(|style| style.opacity(0.8))
                        .cursor(gpui::CursorStyle::OpenHand)
                        .drag_over::<DraggedWidget>(move |style, _, _, _| {
                            style.border_t_2().border_color(accent)
                        })
                        .on_drag(
                            DraggedWidget {
                                widget,
                                from: Some((section, index)),
                            },
                            ghost.clone(),
                        )
                        .on_drop(drop_at(section, index))
                        .child(crate::gpui_widgets::icon(c, widget_glyph(widget), 16.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(widget.to_string()),
                        )
                        .child(self.glyph(
                            format!("remove:{section:?}:{index}"),
                            "close",
                            Msg::Remove(section, index),
                            true,
                        )),
                );
            }
            if list.is_empty() {
                lane = lane.child(
                    div()
                        .w_full()
                        .py(px(16.))
                        .text_center()
                        .text_size(px(11.))
                        .text_color(color(c.colors.muted))
                        .child("Drop items here"),
                );
            }
            lanes = lanes.child(lane);
        }
        let mut shelf = row().w_full().flex_wrap().gap(px(6.));
        for widget in Widget::ALL {
            let used = SECTIONS
                .iter()
                .any(|&(_, section)| section_items(c, section).contains(&widget));
            let dispatch = self.dispatch.clone();
            let target = self.add_target;
            shelf = shelf.child(
                crate::gpui_widgets::frame(row())
                    .id(format!("shelf:{widget:?}"))
                    .gap(px(2.))
                    .pl(px(4.))
                    .pr(px(8.))
                    .py(px(2.))
                    .rounded(crate::gpui_widgets::corner(9999.))
                    .text_size(px(11.))
                    .bg(gpui::rgba(0xffffff12))
                    .when(used, |chip| chip.opacity(0.5))
                    .hover(|style| style.bg(gpui::rgba(0xffffff24)))
                    .active(|style| style.opacity(0.8))
                    .cursor(gpui::CursorStyle::OpenHand)
                    .on_drag(DraggedWidget { widget, from: None }, ghost.clone())
                    .child(crate::gpui_widgets::icon(c, "add", 13.))
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
        let target = SECTIONS
            .iter()
            .find(|&&(_, section)| section == self.add_target)
            .map_or("Right", |&(label, _)| label);
        column()
            .w_full()
            .gap(px(10.))
            .child(lanes)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(color(c.colors.muted))
                    .child(format!(
                        "Drag items to reorder or move them. Click an item below to add it to {target}, or click a column title to change where it goes."
                    )),
            )
            .child(shelf)
            .into_any_element()
    }
}
/// Turns text typed in a number field into an edit, if it's valid.
type FieldEdit = std::rc::Rc<dyn Fn(&str) -> Option<HyprEdit>>;
/// Unsaved-change dot, from the mockup's `--warn`.
const WARN: u32 = 0xffd60aff;
/// Changed lines in Review Changes.
const ADDED: u32 = 0x7ee787ff;
impl SettingsView {
    fn hypr_slider(
        &mut self,
        c: &Config,
        id: String,
        parameters: (f32, (f32, f32), f32, &str),
        edit: impl Fn(f32) -> HyprEdit + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (value, range, step, unit) = parameters;
        let entity = if let Some(entity) = self.sliders.get(&id) {
            entity.update(cx, |s, _| s.sync(range, value, color(c.colors.accent)));
            entity.clone()
        } else {
            let entity = cx.new(|_| Slider::new(range, value, color(c.colors.accent)));
            let dispatch = self.dispatch.clone();
            cx.subscribe_in(&entity, window, move |_, _, event, window, cx| {
                if let SliderEvent::Change(value) = event {
                    let snapped = (*value / step).round() * step;
                    dispatch(
                        crate::Message::Settings(Msg::Hypr(edit(snapped))),
                        window,
                        cx,
                    )
                }
            })
            .detach();
            self.sliders.insert(id, entity.clone());
            entity
        };
        let shown = if step >= 1. {
            format!("{value:.0}")
        } else {
            format!("{value:.2}")
        };
        row()
            .flex_none()
            .gap(px(10.))
            .child(div().w(px(170.)).child(entity))
            .child(
                div()
                    .min_w(px(46.))
                    .text_right()
                    .text_color(color(c.colors.muted))
                    .child(if unit.is_empty() {
                        shown
                    } else {
                        format!("{shown} {unit}")
                    }),
            )
            .into_any_element()
    }
    /// A small text field holding a number; `edit` turns accepted text into an edit.
    fn hypr_field(
        &mut self,
        id: String,
        value: String,
        edit: FieldEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some((last, entity)) = self.inputs.get_mut(&id) {
            // Leave text being typed alone (e.g. "4." on the way to "4.5").
            if !id.ends_with(":name")
                && last != &value
                && !entity.focus_handle(cx).is_focused(window)
            {
                entity.update(cx, |input, cx| input.sync(&value, "", cx));
                *last = value.clone();
            }
        } else {
            let entity = cx.new(|cx| {
                let mut input = TextInput::new(&value, "", cx);
                input.set_text_size(12.);
                input
            });
            let dispatch = self.dispatch.clone();
            let key = id.clone();
            cx.subscribe_in(&entity, window, move |this, _, event, window, cx| {
                if let InputEvent::Changed(text) = event {
                    if let Some((last, _)) = this.inputs.get_mut(&key) {
                        *last = text.clone();
                    }
                    if let Some(edit) = edit(text.trim()) {
                        dispatch(crate::Message::Settings(Msg::Hypr(edit)), window, cx);
                    }
                }
            })
            .detach();
            self.inputs.insert(id.clone(), (value, entity));
        }
        crate::gpui_widgets::frame(div())
            .w(px(if id.ends_with(":name") { 180. } else { 52. }))
            .flex_none()
            .rounded(crate::gpui_widgets::corner(5.))
            .bg(gpui::rgba(0xffffff14))
            .children(self.inputs.get(&id).map(|(_, entity)| entity.clone()))
            .into_any_element()
    }
    /// A dropdown: the current value, opening a list of `options` below it.
    fn hypr_select(
        &self,
        c: &Config,
        id: String,
        current: &str,
        options: Vec<(String, Msg)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.hypr_select.as_deref() == Some(id.as_str());
        let toggle_id = id.clone();
        let mut select = div()
            .id(format!("{id}:select"))
            .relative()
            .flex_none()
            .child(
                crate::gpui_widgets::frame(row())
                    .gap(px(4.))
                    .min_w(px(96.))
                    .justify_between()
                    .pl(px(8.))
                    .pr(px(4.))
                    .py(px(3.))
                    .rounded(crate::gpui_widgets::corner(5.))
                    .bg(gpui::rgba(0xffffff14))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .hover(|style| style.bg(gpui::rgba(0xffffff24)))
                    .child(current.to_owned())
                    .child(crate::gpui_widgets::icon(c, "expand_more", 16.).opacity(0.6)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.hypr_select = if this.hypr_select.as_deref() == Some(toggle_id.as_str()) {
                    None
                } else {
                    Some(toggle_id.clone())
                };
                cx.notify();
            }));
        if open {
            let mut menu = crate::gpui_widgets::frame(column())
                .id(format!("{id}:menu"))
                .occlude()
                .gap_0()
                .p(px(4.))
                .min_w(px(120.))
                .max_h(px(240.))
                .overflow_y_scroll()
                .rounded(crate::gpui_widgets::corner(8.))
                .bg(crate::gpui_widgets::material(
                    c,
                    crate::gpui_widgets::Material::Popover,
                ))
                .border_1()
                .border_color(gpui::rgba(0xffffff26))
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.hypr_select = None;
                    cx.notify();
                }));
            for (label, msg) in options {
                let selected = label == current;
                let dispatch = self.dispatch.clone();
                menu = menu.child(
                    row()
                        .id(format!("{id}:option:{label}"))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(crate::gpui_widgets::corner(5.))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when(selected, |item| {
                            item.bg(color(c.colors.accent))
                                .text_color(color(c.colors.on_accent))
                        })
                        .when(!selected, |item| {
                            item.hover(|style| style.bg(gpui::rgba(0xffffff1a)))
                        })
                        .child(label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.hypr_select = None;
                            dispatch(crate::Message::Settings(msg.clone()), window, cx);
                        })),
                );
            }
            select = select.child(gpui::deferred(
                gpui::anchored()
                    .snap_to_window_with_margin(px(8.))
                    .child(div().mt(px(28.)).child(menu)),
            ));
        }
        select.into_any_element()
    }
    /// One Hyprland setting, laid out as in the mockup: unsaved dot, label, where the
    /// value comes from, the control and a revert button.
    fn hypr_row(
        &self,
        c: &Config,
        id: &str,
        label: &str,
        state: (Option<&str>, bool, bool),
        control: AnyElement,
        revert: HyprEdit,
    ) -> AnyElement {
        let (source, owned, dirty) = state;
        let (title, detail) = split_label(label);
        let pixel = crate::gpui_widgets::pixel_look();
        let chip = |text: &str| {
            div()
                .flex_none()
                .px(px(7.))
                .py(px(1.))
                .rounded(crate::gpui_widgets::corner(9999.))
                .text_size(px(10.5))
                .whitespace_nowrap()
                .map(|chip| match (owned, pixel) {
                    (true, false) => chip
                        .bg(color(c.colors.accent).opacity(0.2))
                        .text_color(color(c.colors.accent)),
                    (true, true) => chip
                        .border_1()
                        .border_color(color(c.colors.accent))
                        .text_color(color(c.colors.accent)),
                    (false, false) => chip
                        .bg(gpui::rgba(0xffffff12))
                        .text_color(color(c.colors.muted)),
                    (false, true) => chip
                        .border_1()
                        .border_color(gpui::rgba(0xffffff2e))
                        .text_color(color(c.colors.muted)),
                })
                .child(text.to_owned())
        };
        row()
            .w_full()
            .gap(px(10.))
            .child(
                div()
                    .size(px(7.))
                    .flex_none()
                    .rounded(crate::gpui_widgets::corner(9999.))
                    .when(dirty, |dot| dot.bg(gpui::rgba(WARN))),
            )
            .child(
                column()
                    .gap_0()
                    .flex_1()
                    .min_w_0()
                    .child(title.to_owned())
                    .children(detail.map(|detail| {
                        div()
                            .text_size(px(11.5))
                            .text_color(color(c.colors.muted))
                            .child(detail.to_owned())
                    })),
            )
            .children(if owned {
                Some(chip("Rune"))
            } else {
                source.map(chip)
            })
            .child(control)
            .child(if owned {
                self.glyph(format!("{id}:revert"), "undo", Msg::Hypr(revert), false)
            } else {
                div().size(px(26.)).flex_none().into_any_element()
            })
            .into_any_element()
    }
    /// A read-only row with a lock, dimmed as in the mockup.
    fn hypr_locked(&self, c: &Config, label: &str) -> AnyElement {
        let (title, detail) = split_label(label);
        row()
            .w_full()
            .gap(px(10.))
            .opacity(0.6)
            .child(div().size(px(7.)).flex_none())
            .child(
                column()
                    .gap_0()
                    .flex_1()
                    .min_w_0()
                    .child(title.to_owned())
                    .children(detail.map(|detail| {
                        div()
                            .text_size(px(11.5))
                            .text_color(color(c.colors.muted))
                            .child(detail.to_owned())
                    })),
            )
            .child(crate::gpui_widgets::icon(c, "lock", 15.).text_color(color(c.colors.muted)))
            .into_any_element()
    }
    fn hypr_option(
        &mut self,
        c: &Config,
        hypr: &HyprUi,
        spec: &'static hyprland_config::OptionSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use hyprland_config::Kind;
        let key = spec.key;
        let value = hypr.option(key).cloned()?;
        let id = format!("hypr-opt:{key}");
        let source = hypr
            .live
            .as_ref()
            .and_then(|l| l.sources.options.get(key))
            .map_or("Default", String::as_str);
        if let (Kind::Info, Value::Text(text)) = (spec.kind, &value) {
            let file = source.rsplit('/').next().unwrap_or(source);
            return Some(self.hypr_locked(
                c,
                &format!(
                    "{}: {text} (Your Hyprland build adds this option and Rune doesn't know it, so it is left in {file} as is)",
                    spec.label
                ),
            ));
        }
        if key == "decoration.rounding" && c.bar.look == crate::config::Look::Pixel {
            return Some(self.hypr_locked(
                c,
                "Corner radius (The pixel look keeps window corners square)",
            ));
        }
        let owned = hypr.draft.options.contains_key(key);
        let dirty = hypr.draft.options.get(key) != c.hyprland.options.get(key);
        let as_f32 = |v: &Value| match v {
            Value::Int(i) => i32::try_from(*i).map_or(0., |i| i as f32),
            Value::Float(f) => *f as f32,
            _ => 0.,
        };
        let int = |i: i64| i32::try_from(i).map_or(0., |i| i as f32);
        let control = match (spec.kind, &value) {
            (Kind::Bool, Value::Bool(on)) => {
                self.toggle_switch(&id, *on, Msg::Hypr(HyprEdit::Set(key, Value::Bool(!on))))
            }
            (Kind::Int { min, max, step }, v) => self.hypr_slider(
                c,
                id.clone(),
                (
                    as_f32(v),
                    (int(min), int(max)),
                    int(step).max(1.),
                    spec.unit,
                ),
                move |v| HyprEdit::Num(key, v),
                window,
                cx,
            ),
            (Kind::Float { min, max, step }, v) => self.hypr_slider(
                c,
                id.clone(),
                (as_f32(v), (min as f32, max as f32), step as f32, spec.unit),
                move |v| HyprEdit::Num(key, v),
                window,
                cx,
            ),
            (Kind::Choice(options), Value::Text(current)) if options.len() <= 4 => self.segmented(
                &id,
                options
                    .iter()
                    .map(|o| {
                        let mut label = (*o).to_owned();
                        if let Some(first) = label.get_mut(..1) {
                            first.make_ascii_uppercase();
                        }
                        (
                            label,
                            current == o,
                            Msg::Hypr(HyprEdit::Set(key, Value::Text((*o).to_owned()))),
                        )
                    })
                    .collect(),
            ),
            (Kind::Choice(options), Value::Text(current)) => self.hypr_select(
                c,
                id.clone(),
                current,
                options
                    .iter()
                    .map(|o| {
                        (
                            (*o).to_owned(),
                            Msg::Hypr(HyprEdit::Set(key, Value::Text((*o).to_owned()))),
                        )
                    })
                    .collect(),
                cx,
            ),
            (Kind::Sides { .. }, Value::Sides(sides)) => {
                let mut fields = row().gap(px(4.)).flex_none().items_start();
                for (side, (name, v)) in ["top", "right", "bottom", "left"]
                    .into_iter()
                    .zip(sides)
                    .enumerate()
                {
                    let field = self.hypr_field(
                        format!("{id}:{name}"),
                        v.to_string(),
                        std::rc::Rc::new(move |text: &str| {
                            text.parse::<f32>()
                                .ok()
                                .filter(|v| v.is_finite())
                                .map(|v| HyprEdit::Side(key, side, v))
                        }),
                        window,
                        cx,
                    );
                    fields = fields.child(
                        column().gap(px(2.)).items_center().child(field).child(
                            div()
                                .text_size(px(10.))
                                .text_color(color(c.colors.muted))
                                .child(name),
                        ),
                    );
                }
                fields.into_any_element()
            }
            _ => div()
                .text_color(color(c.colors.muted))
                .child(format!("{value:?}"))
                .into_any_element(),
        };
        Some(self.hypr_row(
            c,
            &id,
            spec.label,
            (Some(source), owned, dirty),
            control,
            HyprEdit::Revert(key),
        ))
    }
    /// The bottom bar while there are unsaved changes: live indicator, count and actions.
    fn hypr_pending(&self, c: &Config, hypr: &HyprUi) -> AnyElement {
        use hyprland_config::Setup;
        let count = hypr.changes(&c.hyprland);
        let ready = hypr.live.as_ref().is_some_and(|l| l.setup == Setup::Added);
        let pixel = crate::gpui_widgets::pixel_look();
        let live = row()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(color(c.colors.muted))
            .child(
                div()
                    .size(px(8.))
                    .rounded(crate::gpui_widgets::corner(9999.))
                    .bg(gpui::rgba(0x30d158ff))
                    .with_animation(
                        "hypr-live",
                        gpui::Animation::new(std::time::Duration::from_millis(if pixel {
                            1000
                        } else {
                            1800
                        }))
                        .repeat(),
                        move |dot, t| {
                            let opacity = if pixel {
                                if t < 0.5 { 1. } else { 0. }
                            } else {
                                1. - 0.65 * (t * std::f32::consts::PI).sin()
                            };
                            dot.opacity(opacity)
                        },
                    ),
            )
            .child("Live");
        let save = if hypr.busy {
            div()
                .px(px(14.))
                .py(px(5.))
                .text_color(color(c.colors.muted))
                .child("Working…")
                .into_any_element()
        } else if ready && count > 0 {
            self.primary("hypr-save", "Save", Msg::Hypr(HyprEdit::Save))
        } else {
            div()
                .px(px(12.))
                .py(px(3.))
                .rounded(crate::gpui_widgets::corner(6.))
                .bg(color(c.colors.accent))
                .text_color(color(c.colors.on_accent))
                .opacity(0.4)
                .child("Save")
                .into_any_element()
        };
        let review = {
            let dispatch = self.dispatch.clone();
            button(
                "hypr-review",
                row()
                    .gap(px(5.))
                    .child(crate::gpui_widgets::icon(c, "difference", 15.))
                    .child("Review"),
                crate::Message::Settings(Msg::Page(Page::HyprReview)),
                &dispatch,
            )
            .rounded(crate::gpui_widgets::corner(6.))
            .px(px(10.))
            .bg(gpui::rgba(0xffffff14))
        };
        row()
            .flex_none()
            .w_full()
            .gap(px(12.))
            .px(px(22.))
            .py(px(12.))
            .border_t_1()
            .border_color(gpui::rgba(0xffffff1a))
            .bg(crate::gpui_widgets::material(
                c,
                crate::gpui_widgets::Material::Popover,
            ))
            .child(live)
            .child(format!(
                "{count} unsaved change{}",
                if count == 1 { "" } else { "s" }
            ))
            .child(div().flex_1())
            .child(review)
            .child(self.action("hypr-discard", "Discard", Msg::Hypr(HyprEdit::Discard)))
            .child(save)
            .with_spring(
                "hypr-pending",
                gpui::SpringAnimation::new(gpui::SpringConfig::new(300., 34.6, 1.))
                    .to(true)
                    .from(false),
                |bar, phase| {
                    let t = phase.clamp(0.0..=1.0);
                    bar.relative().top(t.interpolate(px(56.), px(0.)))
                },
            )
            .into_any_element()
    }
    /// The first-run card that adds `require("modules.rune")`, plus errors.
    fn hypr_notices(&self, c: &Config, hypr: &HyprUi, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use hyprland_config::{REQUIRE_COMMENT, REQUIRE_LINE, Setup};
        let mut items = Vec::new();
        if !hypr.status.is_empty() {
            items.push(
                crate::gpui_widgets::frame(div())
                    .p(px(12.))
                    .rounded(crate::gpui_widgets::corner(10.))
                    .bg(color(c.colors.urgent).opacity(0.14))
                    .text_color(color(c.colors.urgent))
                    .child(hypr.status.clone())
                    .into_any_element(),
            );
        }
        let Some(live) = &hypr.live else {
            items.push(note(c, "Reading Hyprland's settings…"));
            return items;
        };
        if live.setup == Setup::Added || (self.hypr_later && live.setup == Setup::NotAdded) {
            return items;
        }
        let accent = color(c.colors.accent);
        let pixel = crate::gpui_widgets::pixel_look();
        let text = if live.setup == Setup::NoLuaConfig {
            "Rune saves to ~/.config/hypr/modules/rune.lua, which needs a Lua config (hyprland.lua, Hyprland 0.55 or newer). Until then, changes still preview live but can't be saved."
        } else {
            "Rune will create modules/rune.lua and add one line to the end of hyprland.lua. It loads last, so it only overrides the values you change here. Your other modules are never rewritten. Until then, changes still preview live but can't be saved."
        };
        let card = column()
            .gap(px(6.))
            .p(px(18.))
            .rounded(crate::gpui_widgets::corner(10.))
            .when(!pixel, |card| {
                card.bg(gpui::linear_gradient(
                    135.,
                    gpui::linear_color_stop(accent.opacity(0.14), 0.),
                    gpui::linear_color_stop(color(c.colors.urgent).opacity(0.08), 1.),
                ))
                .border_1()
                .border_color(gpui::rgba(0xffffff1f))
            })
            .when(pixel, |card| {
                card.bg(accent.opacity(0.14))
                    .border_2()
                    .border_color(accent)
            })
            .child(
                crate::gpui_widgets::heading(div())
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Let Rune manage Hyprland settings"),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(color(c.colors.muted))
                    .child(text),
            );
        if live.setup != Setup::NotAdded {
            items.push(card.into_any_element());
            return items;
        }
        let path = match &live.entry.target {
            Some(target) => format!("{}  (→ {target})", live.entry.path),
            None => live.entry.path.clone(),
        };
        let line =
            |text: String, tone: gpui::Rgba| div().min_h(px(18.)).text_color(tone).child(text);
        let added = gpui::rgba(ADDED);
        let diff = column()
            .gap_0()
            .mt(px(6.))
            .px(px(14.))
            .py(px(10.))
            .rounded(crate::gpui_widgets::corner(7.))
            .bg(gpui::rgba(0x15151880))
            .text_size(px(12.))
            .font_family(crate::gpui_widgets::text_font(c))
            .child(line(path, color(c.colors.muted)))
            .child(line(
                format!("  {}", live.entry.last_line),
                color(c.colors.foreground),
            ))
            .child(line("+".into(), added))
            .child(line(format!("+ {REQUIRE_COMMENT}"), added))
            .child(line(format!("+ {REQUIRE_LINE}"), added));
        let add = {
            let dispatch = self.dispatch.clone();
            button(
                "hypr-setup",
                row()
                    .gap(px(5.))
                    .child(crate::gpui_widgets::icon(c, "add", 16.))
                    .child("Add to hyprland.lua"),
                crate::Message::Settings(Msg::Hypr(HyprEdit::Setup)),
                &dispatch,
            )
            .rounded(crate::gpui_widgets::corner(6.))
            .px(px(12.))
            .bg(accent)
            .text_color(color(c.colors.on_accent))
            .font_weight(gpui::FontWeight::SEMIBOLD)
        };
        let later = div()
            .id("hypr-later")
            .px(px(12.))
            .py(px(3.))
            .rounded(crate::gpui_widgets::corner(6.))
            .bg(gpui::rgba(0xffffff1a))
            .cursor_pointer()
            .active(|style| style.opacity(0.7))
            .child("Not now")
            .on_click(cx.listener(|this, _, _, cx| {
                this.hypr_later = true;
                cx.notify();
            }));
        let actions = row()
            .gap(px(8.))
            .mt(px(12.))
            .when(!hypr.busy, |actions| actions.child(add).child(later));
        items.push(card.child(diff).child(actions).into_any_element());
        items
    }
    fn hypr_page(
        &mut self,
        c: &Config,
        s: &State,
        items: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hypr = &s.hypr;
        let description = match s.page {
            Page::HyprWindows => "Layout, gaps and borders. They apply live while you drag.",
            Page::HyprEffects => "Corners, transparency, blur and shadows.",
            Page::HyprInput => "Keyboard repeat, layout and focus behaviour.",
            Page::HyprMotion => {
                "Hyprland's own animations, with springs and bezier curves. Rune's panel motion is under Rune → Animations."
            }
            _ => {
                "This is exactly what Save writes to modules/rune.lua. Hyprland reloads it on its own."
            }
        };
        items.extend(self.hypr_notices(c, hypr, cx));
        items.push(hero(c, s.page, description, None));
        if hypr.live.is_none() {
            return;
        }
        if let Some(section) = s.page.section() {
            let mut groups: Vec<(&str, Vec<AnyElement>)> = Vec::new();
            for spec in hyprland_config::OPTIONS
                .iter()
                .filter(|o| o.section == section)
            {
                let Some(row) = self.hypr_option(c, hypr, spec, window, cx) else {
                    continue;
                };
                match groups.last_mut() {
                    Some((name, list)) if *name == spec.group => list.push(row),
                    _ => groups.push((spec.group, vec![row])),
                }
            }
            for (name, mut list) in groups {
                if name == "Borders" {
                    list.push(self.hypr_locked(
                        c,
                        "Border colours (Set by your Rune theme (Settings → Theme))",
                    ));
                }
                items.push(group(name, list));
            }
            if section == hyprland_config::Section::Input {
                let field = self.input(
                    "hypr-test-field".into(),
                    hypr.test_text.clone(),
                    "Hold a key here to feel the repeat rate",
                    (|text| Msg::Hypr(HyprEdit::TestText(text)), None),
                    window,
                    cx,
                );
                items.push(group("Test field", vec![field]));
            }
            return;
        }
        match s.page {
            Page::HyprMotion => self.hypr_motion(c, hypr, items, window, cx),
            _ => items.push(self.hypr_review(c, hypr)),
        }
    }
    fn hypr_bezier_editor(
        &mut self,
        c: &Config,
        hypr: &HyprUi,
        items: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut names = hypr.curve_names();
        names.retain(|n| hypr.bezier(n).is_some() && hyprland_config::bezier_preset(n).is_none());
        let selected = self
            .hypr_curve
            .as_ref()
            .filter(|n| names.contains(n))
            .cloned()
            .or_else(|| names.first().cloned());
        let mut children = Vec::new();
        let mut tiles = row().w_full().flex_wrap().gap(px(6.));
        for name in &names {
            let pick = name.clone();
            let label = if hypr.custom_curve(name) {
                format!("{name} ★")
            } else {
                name.clone()
            };
            tiles = tiles.child(
                curve_button(format!("bezier-tile:{name}"), label)
                    .when(selected.as_ref() == Some(name), |tile| {
                        tile.border_1().border_color(color(c.colors.accent))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.hypr_curve = Some(pick.clone());
                        cx.notify();
                    })),
            );
        }
        tiles = tiles.child(
            curve_button("new-curve", "+ New curve").on_click(cx.listener(|this, _, _, cx| {
                this.hypr_new_curve = true;
                cx.notify();
            })),
        );
        children.push(tiles.into_any_element());
        if self.hypr_new_curve {
            let next = (1..=33)
                .map(|i| format!("custom{i}"))
                .find(|n| !hypr.curve_names().contains(n))
                .unwrap_or_default();
            let field = self.hypr_field(
                "hypr-new:name".into(),
                next,
                std::rc::Rc::new(|_| None),
                window,
                cx,
            );
            let spring = self.hypr_new_spring;
            let dispatch = self.dispatch.clone();
            children.push(
                row()
                    .gap(px(8.))
                    .items_center()
                    .child("Name")
                    .child(field)
                    .child(
                        curve_button("new-curve-kind", if spring { "Spring" } else { "Bezier" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.hypr_new_spring = !this.hypr_new_spring;
                                cx.notify();
                            })),
                    )
                    .child(curve_button("create-curve", "Create").on_click(cx.listener(
                        move |this, _, window, cx| {
                            let name = this
                                .inputs
                                .get("hypr-new:name")
                                .map(|(text, _)| text.trim().to_owned())
                                .unwrap_or_default();
                            let curve = if spring {
                                Curve::Spring(Spring {
                                    mass: 1.,
                                    stiffness: 150.,
                                    dampening: 25.,
                                })
                            } else {
                                Curve::Bezier(Bezier {
                                    points: [0.25, 0.1, 0.25, 1.],
                                })
                            };
                            dispatch(
                                crate::Message::Settings(Msg::Hypr(HyprEdit::CurveCreate(
                                    name.clone(),
                                    curve,
                                ))),
                                window,
                                cx,
                            );
                            if this
                                .shell
                                .read(cx)
                                .settings_ui
                                .hypr
                                .draft
                                .curves
                                .contains_key(&name)
                            {
                                this.hypr_curve = Some(name);
                                this.hypr_new_curve = false;
                                this.inputs.remove("hypr-new:name");
                            }
                            cx.notify();
                        },
                    )))
                    .child(curve_button("cancel-curve", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.hypr_new_curve = false;
                            this.inputs.remove("hypr-new:name");
                            cx.notify();
                        },
                    )))
                    .into_any_element(),
            );
        }
        if let Some(name) = selected
            && let Some(bezier) = hypr.bezier(&name)
        {
            let graph = self.bezier_canvas(c, &name, bezier, cx);
            let mut fields = column().gap(px(8.));
            let mut pair = row().gap(px(6.)).items_center();
            for (index, label) in ["X1", "Y1", "X2", "Y2"].iter().enumerate() {
                let target = name.clone();
                let points = bezier.points;
                let field = self.hypr_field(
                    format!("bezier:{name}:{index}"),
                    points.get(index).copied().unwrap_or_default().to_string(),
                    std::rc::Rc::new(move |text| {
                        let v = text.parse::<f64>().ok().filter(|v| v.is_finite())?;
                        Some(HyprEdit::BezierCoordinate(target.clone(), index, v))
                    }),
                    window,
                    cx,
                );
                pair = pair.child((*label).to_owned()).child(field);
                if index % 2 == 1 {
                    fields = fields.child(pair);
                    pair = row().gap(px(6.)).items_center();
                }
            }
            let presets = hyprland_config::BEZIER_PRESETS
                .iter()
                .map(|(preset, b)| {
                    (
                        (*preset).to_owned(),
                        Msg::Hypr(HyprEdit::BezierPoints(name.clone(), b.points)),
                    )
                })
                .collect();
            let preset = self.hypr_select(
                c,
                format!("bezier:{name}:preset"),
                "Load preset",
                presets,
                cx,
            );
            let actions = self.hypr_curve_actions(hypr, &name, window, cx);
            children.push(
                row()
                    .w_full()
                    .gap(px(16.))
                    .items_start()
                    .child(div().flex_1().min_w_0().child(graph))
                    .child(
                        column()
                            .w(px(220.))
                            .flex_none()
                            .gap(px(14.))
                            .child(fields)
                            .child(preset)
                            .child(actions),
                    )
                    .into_any_element(),
            );
            children.push(self.bezier_stage(c, bezier, cx));
        }
        items.push(group_with(
            c,
            "Bezier curves",
            Some("Drag either handle or enter control points".into()),
            children,
        ));
    }

    fn hypr_curve_actions(
        &mut self,
        hypr: &HyprUi,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = name.to_owned();
        if hypr.custom_curve(&name) {
            let field_id = format!("bezier:{name}:name");
            let field = self.hypr_field(
                field_id.clone(),
                name.clone(),
                std::rc::Rc::new(|_| None),
                window,
                cx,
            );
            let old = name.clone();
            let dispatch = self.dispatch.clone();
            row()
                .flex_wrap()
                .gap(px(8.))
                .items_center()
                .child(field)
                .child(
                    curve_button(format!("rename:{name}"), "Rename").on_click(cx.listener(
                        move |this, _, window, cx| {
                            let new = this
                                .inputs
                                .get(&field_id)
                                .map(|(text, _)| text.trim().to_owned())
                                .unwrap_or_default();
                            dispatch(
                                crate::Message::Settings(Msg::Hypr(HyprEdit::CurveRename(
                                    old.clone(),
                                    new.clone(),
                                ))),
                                window,
                                cx,
                            );
                            if this
                                .shell
                                .read(cx)
                                .settings_ui
                                .hypr
                                .draft
                                .curves
                                .contains_key(&new)
                            {
                                this.hypr_curve = Some(new);
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(self.action(
                    format!("delete:{name}"),
                    "Delete",
                    Msg::Hypr(HyprEdit::CurveDelete(name.clone())),
                ))
                .into_any_element()
        } else {
            self.action(
                format!("revert-bezier:{name}"),
                "Revert to dotfile curve",
                Msg::Hypr(HyprEdit::CurveRevert(name)),
            )
        }
    }

    fn bezier_canvas(
        &self,
        c: &Config,
        name: &str,
        bezier: Bezier,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let measured = bounds.clone();
        let hit_bounds = bounds.clone();
        let ink = color(c.colors.accent);
        let pixel = crate::gpui_widgets::pixel_look();
        let target = name.to_owned();
        let moving = name.to_owned();
        let dispatch = self.dispatch.clone();
        let entity = cx.entity();
        let view = match &self.hypr_drag {
            Some((dragged, _, _, view)) if dragged == name => *view,
            _ => BezierView::fit(bezier),
        };
        div()
            .id(format!("bezier-canvas:{name}"))
            .size(px(BEZIER_SIZE))
            .flex_none()
            .relative()
            .rounded(crate::gpui_widgets::corner(7.))
            .bg(gpui::rgba(0x00000038))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    let b = hit_bounds.get();
                    let mut closest = None;
                    for (i, (x, y)) in [
                        (bezier.points[0], bezier.points[1]),
                        (bezier.points[2], bezier.points[3]),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let handle = bezier_screen(b, view, x, y);
                        let dx = f32::from(event.position.x - handle.x);
                        let dy = f32::from(event.position.y - handle.y);
                        let distance = dx * dx + dy * dy;
                        if distance <= 400. && closest.is_none_or(|(_, d, _)| distance < d) {
                            closest = Some((i, distance, event.position - handle));
                        }
                    }
                    if let Some((i, _, offset)) = closest {
                        this.hypr_drag = Some((target.clone(), i, offset, view));
                        cx.notify();
                    }
                }),
            )
            .child(
                gpui::canvas(
                    move |b, _, _| {
                        measured.set(b);
                        b
                    },
                    move |_, b, window, _| {
                        let dot = if pixel { 3. } else { 2. };
                        let origin = bezier_screen(b, view, 0., 0.);
                        let target = bezier_screen(b, view, 1., 1.);
                        for step in 0..=BEZIER_GRID {
                            let t = f64::from(step) / f64::from(BEZIER_GRID);
                            let tone = if step == 0 || step == BEZIER_GRID {
                                ink.opacity(0.3)
                            } else {
                                ink.opacity(0.1)
                            };
                            let y = bezier_screen(b, view, 0., t).y;
                            window.paint_quad(gpui::fill(
                                gpui::Bounds::new(
                                    gpui::point(origin.x, y),
                                    gpui::size(target.x - origin.x, px(1.)),
                                ),
                                tone,
                            ));
                            let x = bezier_screen(b, view, t, 0.).x;
                            window.paint_quad(gpui::fill(
                                gpui::Bounds::new(
                                    gpui::point(x, target.y),
                                    gpui::size(px(1.), origin.y - target.y),
                                ),
                                tone,
                            ));
                        }
                        for p in [origin, target] {
                            window.paint_quad(gpui::fill(
                                gpui::Bounds::new(
                                    p - gpui::point(px(3.), px(3.)),
                                    gpui::size(px(6.), px(6.)),
                                ),
                                ink.opacity(0.5),
                            ));
                        }
                        for i in 0..=160 {
                            let (x, y) = bezier.position(f64::from(i) / 160.);
                            let p = bezier_screen(b, view, x, y);
                            let p = if pixel {
                                gpui::point(
                                    px((f32::from(p.x) / 3.).round() * 3.),
                                    px((f32::from(p.y) / 3.).round() * 3.),
                                )
                            } else {
                                p
                            };
                            window.paint_quad(gpui::fill(
                                gpui::Bounds::new(p, gpui::size(px(dot), px(dot))),
                                ink,
                            ));
                        }
                        for (start, end) in [
                            ((0., 0.), (bezier.points[0], bezier.points[1])),
                            ((1., 1.), (bezier.points[2], bezier.points[3])),
                        ] {
                            for i in 0..=40 {
                                let t = f64::from(i) / 40.;
                                let p = bezier_screen(
                                    b,
                                    view,
                                    start.0 + (end.0 - start.0) * t,
                                    start.1 + (end.1 - start.1) * t,
                                );
                                window.paint_quad(gpui::fill(
                                    gpui::Bounds::new(p, gpui::size(px(1.), px(1.))),
                                    ink.opacity(0.4),
                                ));
                            }
                            let p = bezier_screen(b, view, end.0, end.1);
                            window.paint_quad(gpui::fill(
                                gpui::Bounds::new(
                                    p - gpui::point(px(6.), px(6.)),
                                    gpui::size(px(12.), px(12.)),
                                ),
                                ink,
                            ));
                        }
                        let moved = entity.clone();
                        let target = moving.clone();
                        let dispatch = dispatch.clone();
                        window.on_mouse_event(
                            move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                                if !phase.bubble() {
                                    return;
                                }
                                moved.update(cx, |this, cx| {
                                    if let Some((name, index, offset, drag_view)) =
                                        &mut this.hypr_drag
                                        && *name == target
                                    {
                                        let Some((x, y)) =
                                            bezier_point(b, view, event.position - *offset)
                                        else {
                                            return;
                                        };
                                        let mut points = bezier.points;
                                        let first = index.saturating_mul(2);
                                        if let Some(slot) = points.get_mut(first) {
                                            *slot = x;
                                        }
                                        if let Some(slot) = points.get_mut(first.saturating_add(1))
                                        {
                                            *slot = y;
                                        }
                                        *drag_view =
                                            drag_view.union(BezierView::fit(Bezier { points }));
                                        dispatch(
                                            crate::Message::Settings(Msg::Hypr(
                                                HyprEdit::BezierPoints(name.clone(), points),
                                            )),
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            },
                        );
                        window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, _, cx| {
                            if phase.bubble() && event.button == gpui::MouseButton::Left {
                                entity.update(cx, |this, cx| {
                                    this.hypr_drag = None;
                                    cx.notify();
                                });
                            }
                        });
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    fn bezier_stage(&self, c: &Config, bezier: Bezier, cx: &mut Context<Self>) -> AnyElement {
        let reduced = c.animations.reduce_motion || !c.animations.enabled;
        let pixel = crate::gpui_widgets::pixel_look();
        let window_box = curve_preview_window(c).with_animation(
            format!("bezier-preview:{}:{:?}", self.hypr_stage, bezier.points),
            gpui::Animation::new(std::time::Duration::from_millis(800)),
            move |element, t| {
                let t = if reduced {
                    1.
                } else {
                    bezier.ease(f64::from(t)) as f32
                };
                let t = if pixel { (t * 12.).round() / 12. } else { t };
                element.left(gpui::relative(0.03 + t * 0.74))
            },
        );
        self.curve_preview(c, "bezier-preview", window_box.into_any_element(), cx)
    }

    fn hypr_motion(
        &mut self,
        c: &Config,
        hypr: &HyprUi,
        items: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hypr_bezier_editor(c, hypr, items, window, cx);
        let springs: Vec<_> = hypr
            .springs()
            .into_iter()
            .filter(|name| hypr.curve(name).is_some())
            .collect();
        let curves: Vec<(String, hyprland_config::Spring)> = springs
            .iter()
            .filter_map(|name| Some((name.clone(), hypr.curve(name)?)))
            .collect();
        let sources = hypr.live.as_ref().map(|l| &l.sources);
        let pixel = crate::gpui_widgets::pixel_look();
        if curves.is_empty() {
            items.push(note(
                c,
                "No spring curves found. Use New curve above and choose Spring to create one.",
            ));
        } else {
            let selected = self
                .hypr_curve
                .clone()
                .filter(|name| curves.iter().any(|(n, _)| n == name))
                .or_else(|| curves.first().map(|(n, _)| n.clone()))
                .unwrap_or_default();
            let mut tiles = row().w_full().gap(px(8.)).items_stretch();
            for (name, spring) in &curves {
                let on = *name == selected;
                let pick = name.clone();
                tiles = tiles.child(
                    column()
                        .id(format!("hypr-curve-tile:{name}"))
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .items_center()
                        .p(px(8.))
                        .rounded(crate::gpui_widgets::corner(7.))
                        .bg(gpui::rgba(0xffffff0d))
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .when(on, |tile| {
                            tile.border_2().border_color(color(c.colors.accent))
                        })
                        .when(!on && pixel, |tile| {
                            tile.border_2().border_color(gpui::rgba(0xffffff1a))
                        })
                        .hover(|style| style.bg(gpui::rgba(0xffffff17)))
                        .child(spring_graph(c, *spring, on))
                        .child(div().truncate().child(if hypr.custom_curve(name) {
                            format!("{name} ★")
                        } else {
                            name.clone()
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.hypr_curve = Some(pick.clone());
                            this.hypr_stage = this.hypr_stage.wrapping_add(1);
                            cx.notify();
                        })),
                );
            }
            let mut list = vec![tiles.into_any_element()];
            if let Some(spring) = hypr.curve(&selected) {
                let id = format!("hypr-curve:{selected}");
                let owned = hypr.draft.curves.contains_key(&selected);
                let dirty = hypr.draft.curves.get(&selected) != c.hyprland.curves.get(&selected);
                let source = sources
                    .and_then(|s| s.curves.get(&selected))
                    .map_or("Rune", String::as_str);
                let name = selected.clone();
                let stiffness = self.hypr_slider(
                    c,
                    format!("{id}:stiffness"),
                    (spring.stiffness as f32, (50., 400.), 5., ""),
                    move |v| HyprEdit::Stiffness(name.clone(), v),
                    window,
                    cx,
                );
                list.push(self.hypr_row(
                    c,
                    &format!("{id}:stiffness"),
                    "Stiffness (Higher is faster)",
                    (Some(source), owned, dirty),
                    stiffness,
                    HyprEdit::CurveRevert(selected.clone()),
                ));
                let name = selected.clone();
                let dampening = self.hypr_slider(
                    c,
                    format!("{id}:dampening"),
                    (spring.dampening as f32, (5., 50.), 1., ""),
                    move |v| HyprEdit::Dampening(name.clone(), v),
                    window,
                    cx,
                );
                list.push(self.hypr_row(
                    c,
                    &format!("{id}:dampening"),
                    "Damping (Lower is bouncier. Overshoot is only for gestures.)",
                    (Some(source), owned, dirty),
                    dampening,
                    HyprEdit::CurveRevert(selected.clone()),
                ));
                list.push(self.hypr_stage(c, spring, cx));
                list.push(self.hypr_curve_actions(hypr, &selected, window, cx));
            }
            let from = curves
                .first()
                .and_then(|(n, _)| sources?.curves.get(n))
                .map(|file| format!("from {file}"));
            items.push(group_with(c, "Spring curves", from, list));
        }
        let mut leaves = Vec::new();
        for leaf_name in hyprland_config::LEAVES {
            let Some(leaf) = hypr.leaf(leaf_name).cloned() else {
                continue;
            };
            let name = (*leaf_name).to_owned();
            let id = format!("hypr-leaf:{name}");
            let owned = hypr.draft.animations.contains_key(&name);
            let dirty = hypr.draft.animations.get(&name) != c.hyprland.animations.get(&name);
            let source = sources
                .and_then(|s| s.leaves.get(&name))
                .map_or("Default", String::as_str);
            let picker = self.hypr_select(
                c,
                format!("{id}:curve"),
                &leaf.curve,
                hypr.animation_curves()
                    .into_iter()
                    .map(|curve| {
                        (
                            curve.clone(),
                            Msg::Hypr(HyprEdit::LeafCurve(name.clone(), curve)),
                        )
                    })
                    .collect(),
                cx,
            );
            let speed_name = name.clone();
            let speed = self.hypr_field(
                format!("{id}:speed"),
                format!("{}", leaf.speed),
                std::rc::Rc::new(move |text: &str| {
                    text.parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite() && *v > 0.)
                        .map(|v| HyprEdit::LeafSpeed(speed_name.clone(), v))
                }),
                window,
                cx,
            );
            let toggle = self.toggle_switch(
                &format!("{id}:enabled"),
                leaf.enabled,
                Msg::Hypr(HyprEdit::LeafEnabled(name.clone(), !leaf.enabled)),
            );
            let label = if leaf.style.is_empty() {
                format!("{name} (default style)")
            } else {
                format!("{name} ({})", leaf.style)
            };
            leaves.push(
                self.hypr_row(
                    c,
                    &id,
                    &label,
                    (Some(source), owned, dirty),
                    row()
                        .gap(px(8.))
                        .flex_none()
                        .child(picker)
                        .child(speed)
                        .child(toggle)
                        .into_any_element(),
                    HyprEdit::LeafRevert(name.clone()),
                ),
            );
        }
        if !leaves.is_empty() {
            items.push(group("Animations", leaves));
        }
    }
    /// A window-shaped box that springs across with the selected curve; click to replay.
    fn hypr_stage(
        &self,
        c: &Config,
        spring: hyprland_config::Spring,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pixel = crate::gpui_widgets::pixel_look();
        let reduced = c.animations.reduce_motion || !c.animations.enabled;
        let window_box = curve_preview_window(c).with_spring(
            format!("hypr-stage:{}", self.hypr_stage),
            gpui::SpringAnimation::new(gpui::SpringConfig::new(
                spring.stiffness as f32,
                spring.dampening as f32,
                spring.mass as f32,
            ))
            .to(true)
            .from(false),
            move |b, phase| {
                let t = if reduced {
                    1.
                } else if pixel {
                    (phase.0 * 12.).round() / 12.
                } else {
                    phase.0
                };
                b.left(gpui::relative(0.03 + t * 0.74))
            },
        );
        self.curve_preview(c, "hypr-stage", window_box.into_any_element(), cx)
    }

    fn curve_preview(
        &self,
        c: &Config,
        id: &'static str,
        window_box: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .relative()
            .w_full()
            .h(px(96.))
            .rounded(crate::gpui_widgets::corner(7.))
            .bg(gpui::rgba(0x00000038))
            .overflow_hidden()
            .cursor_pointer()
            .child(window_box)
            .child(
                div()
                    .absolute()
                    .right(px(10.))
                    .bottom(px(8.))
                    .text_size(px(11.))
                    .text_color(color(c.colors.muted))
                    .child("click to replay · Hyprland previews live too"),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.hypr_stage = this.hypr_stage.wrapping_add(1);
                cx.notify();
            }))
            .into_any_element()
    }
    fn hypr_review(&self, c: &Config, hypr: &HyprUi) -> AnyElement {
        let text = hypr.draft.render();
        let dirty = hypr.is_dirty(&c.hyprland);
        let mut code = crate::gpui_widgets::frame(column())
            .gap_0()
            .px(px(14.))
            .py(px(10.))
            .rounded(crate::gpui_widgets::corner(7.))
            .bg(gpui::rgba(0x15151880))
            .font_family(crate::gpui_widgets::text_font(c))
            .text_size(px(12.));
        for line in text.lines() {
            let tone = if line.starts_with("--") {
                color(c.colors.muted)
            } else if dirty {
                gpui::rgba(ADDED)
            } else {
                color(c.colors.foreground)
            };
            code = code.child(div().min_h(px(18.)).text_color(tone).child(line.to_owned()));
        }
        code.into_any_element()
    }
}
/// A settings group whose heading has text on the right, like "from modules/animations.lua".
fn group_with(
    c: &Config,
    title: &str,
    trailing: Option<String>,
    children: Vec<AnyElement>,
) -> AnyElement {
    let list = group(title, children);
    match trailing {
        None => list,
        Some(trailing) => div()
            .relative()
            .child(list)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right(px(4.))
                    .text_size(px(12.))
                    .text_color(color(c.colors.muted))
                    .child(trailing),
            )
            .into_any_element(),
    }
}
fn curve_preview_window(c: &Config) -> gpui::Div {
    let pixel = crate::gpui_widgets::pixel_look();
    div()
        .absolute()
        .top(px(18.))
        .w(px(110.))
        .h(px(60.))
        .rounded(crate::gpui_widgets::corner(10.))
        .bg(gpui::linear_gradient(
            135.,
            gpui::linear_color_stop(color(c.colors.accent), 0.),
            gpui::linear_color_stop(color(c.colors.urgent), 1.),
        ))
        .when(!pixel, |b| b.shadow_lg())
        .when(pixel, |b| b.border_2().border_color(gpui::black()))
}

fn curve_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
) -> gpui::Stateful<gpui::Div> {
    crate::gpui_widgets::frame(div())
        .id(id)
        .px(px(10.))
        .py(px(6.))
        .rounded(crate::gpui_widgets::corner(6.))
        .bg(gpui::rgba(0xffffff14))
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgba(0xffffff24)))
        .active(|s| s.opacity(0.7))
        .child(label.into())
}

const BEZIER_SIZE: f32 = 280.;
const BEZIER_PAD: f32 = 20.;
const BEZIER_GRID: u8 = 4;

/// The vertical range of curve space shown by the editor; x always spans 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BezierView {
    low: f64,
    high: f64,
}

impl BezierView {
    /// The unit square plus both handles. A bezier stays inside its control points' hull,
    /// so the whole curve fits too.
    fn fit(bezier: Bezier) -> Self {
        let [_, y1, _, y2] = bezier.points.map(|v| v.clamp(-2., 3.));
        Self {
            low: 0_f64.min(y1).min(y2),
            high: 1_f64.max(y1).max(y2),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            low: self.low.min(other.low),
            high: self.high.max(other.high),
        }
    }

    /// Pixels per curve unit, or `None` while the editor has no usable size.
    fn scale(self, bounds: gpui::Bounds<gpui::Pixels>) -> Option<f32> {
        let side = f32::from(bounds.size.width.min(bounds.size.height)) - 2. * BEZIER_PAD;
        let span = (self.high - self.low) as f32;
        (side > 0. && span > 0.).then(|| side / span)
    }
}

/// Maps curve space to the editor: (0, 0) bottom-left of the unit grid, (1, 1) top-right.
fn bezier_screen(
    bounds: gpui::Bounds<gpui::Pixels>,
    view: BezierView,
    x: f64,
    y: f64,
) -> gpui::Point<gpui::Pixels> {
    let scale = view.scale(bounds).unwrap_or(0.);
    gpui::point(
        bounds.center().x + px((x as f32 - 0.5) * scale),
        bounds.bottom() - px(BEZIER_PAD + (y - view.low) as f32 * scale),
    )
}

/// Inverse of [`bezier_screen`].
fn bezier_point(
    bounds: gpui::Bounds<gpui::Pixels>,
    view: BezierView,
    point: gpui::Point<gpui::Pixels>,
) -> Option<(f64, f64)> {
    let scale = view.scale(bounds)?;
    let x = f32::from(point.x - bounds.center().x) / scale + 0.5;
    let y = (f32::from(bounds.bottom() - point.y) - BEZIER_PAD) / scale;
    Some((f64::from(x), f64::from(y) + view.low))
}

/// A spring's step response over 0.8 s. The pixel look draws it on a 3 px grid.
fn spring_graph(c: &Config, spring: hyprland_config::Spring, selected: bool) -> AnyElement {
    let ink = if selected {
        color(c.colors.accent)
    } else {
        color(c.colors.foreground).opacity(0.6)
    };
    let pixel = crate::gpui_widgets::pixel_look();
    div()
        .w_full()
        .h(px(46.))
        .child(
            gpui::canvas(
                |bounds, _, _| bounds,
                move |_, bounds, window, _| {
                    let dot = if pixel { 3. } else { 2. };
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    let mut x = 0.;
                    while x <= width - dot {
                        let t = f64::from(x / width) * 0.8;
                        let y = height - 6. - spring.response(t) as f32 * (height - 16.);
                        let y = if pixel { (y / dot).round() * dot } else { y };
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(
                                gpui::point(bounds.left() + px(x), bounds.top() + px(y)),
                                gpui::size(px(dot), px(dot)),
                            ),
                            ink,
                        ));
                        x += if pixel { dot + 1. } else { 1. };
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.shell.read(cx);
        let c = app.config.clone();
        let s = app.settings_ui.clone();
        let output_volume = app.snapshot.volume.map(|volume| volume.percent);
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
            Page::HyprWindows
            | Page::HyprEffects
            | Page::HyprInput
            | Page::HyprMotion
            | Page::HyprReview => self.hypr_page(&c, &s, &mut items, window, cx),
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
                                "Smooth".into(),
                                c.bar.look == crate::config::Look::Smooth,
                                Msg::Look(crate::config::Look::Smooth),
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
                let shapes = BarShape::ALL
                    .into_iter()
                    .map(|shape| {
                        (
                            shape.label().to_owned(),
                            BarShape::of(&c.bar) == Some(shape),
                            Msg::Shape(shape),
                        )
                    })
                    .collect();
                items.push(hero(
                    &c,
                    Page::Bar,
                    "Drag items to arrange them. Changes apply instantly.",
                    Some(self.segmented("bar-shape", shapes)),
                ));
                items.push(bar_preview(&c, window, cx));
                items.push(group("Layout", vec![self.bar_layout(&c, cx)]));
                let mut shape = vec![
                    number!(Height, "Height", c.bar.height, 24., 56., 1.),
                    number!(
                        Width,
                        "Width (0 fills the screen)",
                        c.bar.width,
                        0.,
                        3840.,
                        10.
                    ),
                ];
                // The pixel look forces square corners, so rounding would do nothing.
                if !crate::gpui_widgets::pixel_look() {
                    shape.push(number!(
                        Radius,
                        "Corner rounding",
                        c.bar.radius,
                        0.,
                        28.,
                        1.
                    ));
                }
                shape.push(number!(
                    MarginTop,
                    "Gap above (0 attaches it to the top edge as a notch)",
                    c.bar.margin_top,
                    0.,
                    32.,
                    1.
                ));
                items.push(group("Shape", shape));
                items.push(group(
                    "Spacing",
                    vec![
                        number!(MarginX, "Gap at the sides", c.bar.margin_x, 0., 400., 1.),
                        number!(Padding, "Side padding", c.bar.padding, 0., 32., 1.),
                        number!(Spacing, "Space between items", c.bar.spacing, 0., 32., 1.),
                        number!(
                            Gap,
                            "Gap below (space between the menu bar and windows)",
                            c.bar.gap,
                            0.,
                            32.,
                            1.
                        ),
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
            Page::Equalizer => {
                let on = c.equalizer.enabled;
                items.push(hero(
                    &c,
                    Page::Equalizer,
                    "Shape the sound of everything you play.",
                    Some(self.toggle_switch("eq-enabled", on, Msg::EqEnabled(!on))),
                ));
                items.push(group("Presets", vec![self.eq_presets(&c)]));
                let mut bands = row()
                    .w_full()
                    .justify_between()
                    .when(!on, |r| r.opacity(0.5));
                for index in 0..c.equalizer.bands.len() {
                    bands = bands.child(self.eq_band(&c, index, window, cx));
                }
                items.push(group("Bands", vec![bands.into_any_element()]));
                items.push(
                    row()
                        .w_full()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.))
                                .text_color(color(c.colors.muted))
                                .child(
                                    "Boosts are balanced automatically so loud parts don't distort. Double-click a band to reset it.",
                                ),
                        )
                        .child(self.action("eq-reset", "Reset", Msg::EqReset))
                        .into_any_element(),
                );
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
                    let volume = if output {
                        output_volume
                    } else {
                        s.audio.mic().and_then(|mic| mic.volume)
                    };
                    if let Some(volume) = volume {
                        rows.insert(0, self.audio_volume(&c, !output, volume, window, cx));
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
                items.push(group(
                    "Reminders",
                    vec![
                        self.labeled(
                            "Notify before timed events",
                            self.segmented(
                                "remind-minutes",
                                [
                                    (0, "Off"),
                                    (5, "5 min"),
                                    (10, "10 min"),
                                    (15, "15 min"),
                                    (30, "30 min"),
                                ]
                                .into_iter()
                                .map(|(minutes, label)| {
                                    (
                                        label.to_owned(),
                                        c.calendar.remind_minutes == minutes,
                                        Msg::RemindMinutes(minutes),
                                    )
                                })
                                .collect(),
                            ),
                        ),
                    ],
                ));
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
                    crate::capture_panel::SHORTCUTS
                        .iter()
                        .map(|(keys, args)| {
                            code(&format!(
                                "hl.bind(\"{keys}\", hl.dsp.exec_cmd(\"rune-shell {args}\"))"
                            ))
                        })
                        .collect(),
                ));
                items.push(note(
                    &c,
                    "Add these to your Hyprland config, or add the Capture widget in Menu Bar to use them with the mouse. Run the record command again, or click the timer, to stop.",
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
        for (index, (title, group)) in groups.into_iter().enumerate() {
            sidebar = sidebar.child(
                crate::gpui_widgets::heading(div())
                    .px(px(6.))
                    .pt(px(if index > 0 { 12. } else { 2. }))
                    .pb(px(4.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(foreground.opacity(0.4))
                    .child(if crate::gpui_widgets::pixel_look() {
                        title.to_uppercase()
                    } else {
                        title.to_owned()
                    }),
            );
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
                        .when(s.hypr.page_dirty(page, &c.hyprland), |item| {
                            item.child(div().flex_1()).child(
                                div()
                                    .size(px(7.))
                                    .rounded(crate::gpui_widgets::corner(9999.))
                                    .bg(gpui::rgba(WARN)),
                            )
                        })
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
                    )
                    .when(
                        s.page.is_hypr() && s.hypr.changes(&c.hyprland) > 0,
                        |content| content.child(self.hypr_pending(&c, &s.hypr)),
                    ),
            )
            .child(crate::gpui_widgets::edge(&c, 12.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_editor_fits_overshooting_handles_inside_the_box() {
        let bounds = gpui::Bounds::new(
            gpui::point(px(10.), px(20.)),
            gpui::size(px(BEZIER_SIZE), px(BEZIER_SIZE)),
        );
        let plain = BezierView::fit(Bezier {
            points: [0.25, 0.1, 0.25, 1.],
        });
        assert_eq!(
            bezier_screen(bounds, plain, 0., 0.),
            gpui::point(px(10. + BEZIER_PAD), px(20. + BEZIER_SIZE - BEZIER_PAD))
        );
        assert_eq!(
            bezier_screen(bounds, plain, 1., 1.),
            gpui::point(px(10. + BEZIER_SIZE - BEZIER_PAD), px(20. + BEZIER_PAD))
        );
        let back = Bezier {
            points: [0.3, 2.5, 0.6, -0.8],
        };
        let view = BezierView::fit(back);
        for i in 0..=64 {
            let (x, y) = back.position(f64::from(i) / 64.);
            let p = bezier_screen(bounds, view, x, y);
            assert!(bounds.contains(&p), "{p:?} escapes {bounds:?}");
        }
        let (x, y) = bezier_point(bounds, view, bezier_screen(bounds, view, 0.25, -0.2)).unwrap();
        assert!((x - 0.25).abs() < 1e-4 && (y + 0.2).abs() < 1e-4);
        assert!(bezier_point(gpui::Bounds::default(), view, bounds.origin).is_none());
    }

    struct DropdownScrollFixture {
        settings: Entity<SettingsView>,
        page_scroll: gpui::ScrollHandle,
        trigger_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    }

    impl Render for DropdownScrollFixture {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let picker = self.settings.update(cx, |view, cx| {
                view.hypr_select(
                    &Config::default(),
                    "test-preset".into(),
                    "Load preset",
                    hyprland_config::BEZIER_PRESETS
                        .iter()
                        .map(|(name, _)| {
                            (
                                (*name).to_owned(),
                                Msg::Hypr(HyprEdit::TestText((*name).to_owned())),
                            )
                        })
                        .collect(),
                    cx,
                )
            });
            let bounds = self.trigger_bounds.clone();
            div()
                .size_full()
                .id("test-page-scroll")
                .overflow_y_scroll()
                .track_scroll(&self.page_scroll)
                .child(
                    column()
                        .w_full()
                        .child(div().h(px(40.)).flex_none())
                        .child(
                            div().relative().child(picker).child(
                                gpui::canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                                    .absolute()
                                    .size_full(),
                            ),
                        )
                        .child(div().h(px(1200.)).flex_none()),
                )
        }
    }

    #[test]
    #[ignore = "opens native settings on Wayland; run one GPUI test per process"]
    fn native_preset_menu_scroll_stays_inside_the_menu() {
        use gpui::{MouseButton, PlatformInput, ScrollDelta, ScrollEvent};
        gpui_platform::application()
            .with_quit_mode(gpui::QuitMode::Explicit)
            .run(|cx| {
                let mut config = Config::default();
                if std::env::var("RUNE_TEST_LOOK").as_deref() == Ok("pixel") {
                    config.bar.look = crate::config::Look::Pixel;
                }
                crate::gpui_widgets::set_look(config.bar.look, config.bar.pixel_font);
                let (actions, _) = tokio::sync::mpsc::unbounded_channel();
                let (_, events) = tokio::sync::mpsc::unbounded_channel();
                let (app, _) = crate::App::new(config, events, actions, false);
                let shell = cx.new(|_| app);
                let dispatch: Dispatch = {
                    let shell = shell.clone();
                    std::rc::Rc::new(move |message, _, cx| {
                        shell.update(cx, |app, cx| {
                            let _ = app.update(message);
                            cx.notify();
                        });
                    })
                };
                let scroll = gpui::ScrollHandle::new();
                let bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
                let handle = cx
                    .open_window(
                        crate::gpui_surfaces::options(
                            &crate::Surface::Settings,
                            (500, 400),
                            None,
                            true,
                            0,
                        ),
                        |window, cx| {
                            let settings = cx.new(|cx| {
                                let mut view =
                                    SettingsView::new(shell.clone(), dispatch, window, cx);
                                view.hypr_select = Some("test-preset".into());
                                view
                            });
                            cx.new(|_| DropdownScrollFixture {
                                settings,
                                page_scroll: scroll.clone(),
                                trigger_bounds: bounds.clone(),
                            })
                        },
                    )
                    .unwrap();
                cx.spawn(async move |cx| {
                    let delay = std::time::Duration::from_millis(100);
                    cx.background_executor().timer(delay).await;
                    let position = cx.update(|cx| {
                        cx.update_window(handle.into(), |_, window, cx| {
                            let position = bounds.get().origin + gpui::point(px(20.), px(48.));
                            window.dispatch_event(
                                PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                    position,
                                    ..Default::default()
                                }),
                                cx,
                            );
                            window.dispatch_event(
                                PlatformInput::Scroll(ScrollEvent {
                                    position,
                                    delta: ScrollDelta::Pixels(gpui::point(px(0.), px(-120.))),
                                    ..Default::default()
                                }),
                                cx,
                            );
                            position
                        })
                        .unwrap()
                    });
                    cx.background_executor().timer(delay).await;
                    cx.update(|_| {
                        assert!(
                            scroll.max_offset().y > px(0.),
                            "fixture page must be scrollable"
                        );
                        assert_eq!(
                            scroll.offset().y,
                            px(0.),
                            "scrolling the preset menu must not move the page"
                        );
                    });
                    for delta in [-10_000., -120., 10_000., 120., -120.] {
                        cx.update(|cx| {
                            cx.update_window(handle.into(), |_, window, cx| {
                                window.dispatch_event(
                                    PlatformInput::Scroll(ScrollEvent {
                                        position,
                                        delta: ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
                                        ..Default::default()
                                    }),
                                    cx,
                                );
                            })
                            .unwrap();
                        });
                        cx.background_executor().timer(delay).await;
                        cx.update(|_| {
                            assert_eq!(
                                scroll.offset().y,
                                px(0.),
                                "scrolling at either end of the menu must not move the page"
                            )
                        });
                    }
                    cx.update(|cx| {
                        cx.update_window(handle.into(), |_, window, cx| {
                            window.dispatch_event(
                                PlatformInput::MouseDown(gpui::MouseDownEvent {
                                    position,
                                    button: MouseButton::Left,
                                    click_count: 1,
                                    ..Default::default()
                                }),
                                cx,
                            );
                            window.dispatch_event(
                                PlatformInput::MouseUp(gpui::MouseUpEvent {
                                    position,
                                    button: MouseButton::Left,
                                    click_count: 1,
                                    ..Default::default()
                                }),
                                cx,
                            );
                        })
                        .unwrap();
                    });
                    cx.background_executor().timer(delay).await;
                    cx.update(|cx| {
                        let selected = &shell.read(cx).settings_ui.hypr.test_text;
                        assert!(
                            !selected.is_empty(),
                            "a preset must remain selectable after scrolling"
                        );
                        assert_ne!(selected, "ease", "the preset list itself must scroll");
                        handle
                            .update(cx, |fixture, window, cx| {
                                assert!(
                                    fixture.settings.read(cx).hypr_select.is_none(),
                                    "selection closes the menu"
                                );
                                let position = gpui::point(px(400.), px(300.));
                                window.dispatch_event(
                                    PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                        position,
                                        ..Default::default()
                                    }),
                                    cx,
                                );
                                window.dispatch_event(
                                    PlatformInput::Scroll(ScrollEvent {
                                        position,
                                        delta: ScrollDelta::Pixels(gpui::point(px(0.), px(-120.))),
                                        ..Default::default()
                                    }),
                                    cx,
                                );
                            })
                            .unwrap();
                    });
                    cx.background_executor().timer(delay).await;
                    cx.update(|cx| {
                        assert!(
                            scroll.offset().y < px(0.),
                            "the page still scrolls outside the menu"
                        );
                        cx.quit();
                    });
                })
                .detach();
            });
    }

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
                            0,
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
                                        volume: Some(50),
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
                                    let mut live = hyprland_config::Snapshot::default();
                                    for spec in hyprland_config::OPTIONS {
                                        let value = match spec.kind {
                                            hyprland_config::Kind::Bool => Value::Bool(true),
                                            hyprland_config::Kind::Choice(list) => {
                                                Value::Text(list[0].into())
                                            }
                                            hyprland_config::Kind::Sides { .. } => {
                                                Value::Sides([0, 16, 16, 16])
                                            }
                                            _ => Value::Int(2),
                                        };
                                        live.options.insert(spec.key.into(), value);
                                    }
                                    live.curves.insert(
                                        "stripGlide".into(),
                                        hyprland_config::Curve::Spring(Spring {
                                            mass: 1.,
                                            stiffness: 150.,
                                            dampening: 25.,
                                        }),
                                    );
                                    live.leaves.insert(
                                        "windowsIn".into(),
                                        Leaf {
                                            enabled: true,
                                            speed: 4.,
                                            curve: "spring:stripGlide".into(),
                                            style: "popin 96%".into(),
                                        },
                                    );
                                    live.setup = hyprland_config::Setup::NotAdded;
                                    s.hypr.received(Ok(Box::new(live)));
                                    s.hypr.draft.curves.insert(
                                        "testBezier".into(),
                                        Curve::Bezier(Bezier {
                                            points: [0.25, 0.1, 0.25, 1.],
                                        }),
                                    );
                                    s.hypr.status = "Example Hyprland error".into();
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
                                        if populated && page == Page::HyprMotion {
                                            for (index, length, text) in
                                                [(0, 4, "0.4"), (1, 3, "-0.3")]
                                            {
                                                let input = view.inputs
                                                    [&format!("bezier:testBezier:{index}")]
                                                    .1
                                                    .clone();
                                                window.focus(&input.focus_handle(cx), cx);
                                                input.update(cx, |input, cx| {
                                                    input.replace_text_in_range(
                                                        Some(0..length),
                                                        text,
                                                        window,
                                                        cx,
                                                    )
                                                });
                                            }
                                            window.focus(&view.focus, cx);
                                        }
                                    })
                                    .unwrap()
                            });
                            if populated && page == Page::HyprMotion {
                                cx.background_executor().timer(delay).await;
                                cx.update(|cx| {
                                    assert_eq!(
                                        shell
                                            .read(cx)
                                            .settings_ui
                                            .hypr
                                            .bezier("testBezier")
                                            .unwrap()
                                            .points,
                                        [0.4, -0.3, 0.25, 1.]
                                    )
                                });
                            }
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
