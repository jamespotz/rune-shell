//! The polkit password dialog: one card centered over a dimmed screen.
use crate::{
    App as Shell, Message,
    controls::text_input::{InputEvent, TextInput},
    gpui_notch::Dispatch,
    gpui_panels::action_button,
    gpui_widgets::{self as widgets, color},
};
use gpui::{AnimationExt, Context, Entity, Focusable, Render, Window, div, prelude::*, px};
use rune_services::polkit::{Command, Request, Secret};
use std::time::Duration;

const CARD_W: f32 = 380.;

pub(crate) struct PolkitView {
    shell: Entity<Shell>,
    input: Entity<TextInput>,
    dispatch: Dispatch,
    /// The request this view last synced, and the user picked for it.
    cookie: String,
    attempts: u32,
    user: usize,
    _subscriptions: Vec<gpui::Subscription>,
}

/// PAM's prompt as a placeholder: "Password: " becomes "Password".
fn placeholder(request: &Request) -> &str {
    match request.prompt.trim().trim_end_matches(':').trim_end() {
        "" => "Password",
        prompt => prompt,
    }
}

impl PolkitView {
    pub fn new(
        shell: Entity<Shell>,
        dispatch: Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("", "Password", cx);
            input.set_text_size(15.);
            input.set_password(true, cx);
            input
        });
        input.focus_handle(cx).focus(window, cx);
        let submitted = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Submit) {
                this.submit(window, cx);
            }
        });
        let observed = cx.observe(&shell, |this, _, cx| this.sync(cx));
        let mut view = Self {
            shell,
            input,
            dispatch,
            cookie: String::new(),
            attempts: 0,
            user: 0,
            _subscriptions: vec![submitted, observed],
        };
        view.sync(cx);
        view
    }

    /// Follows the service: a new request or a failed attempt clears the field.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.shell.read(cx).snapshot.polkit.clone() else {
            return;
        };
        let fresh = request.cookie != self.cookie;
        if fresh {
            self.cookie.clone_from(&request.cookie);
            self.user = request.user;
        }
        let failed = request.attempts != self.attempts;
        self.attempts = request.attempts;
        let clear = fresh || failed;
        self.input.update(cx, |input, cx| {
            let text = if clear { "" } else { input.text() }.to_owned();
            input.sync(&text, placeholder(&request), cx);
            input.set_password(!request.echo, cx);
        });
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.shell.read(cx).snapshot.polkit.as_ref() else {
            return;
        };
        if request.busy {
            return;
        }
        let placeholder = placeholder(request).to_owned();
        let cookie = request.cookie.clone();
        let response = Secret::new(self.input.read(cx).text().to_owned());
        self.input
            .update(cx, |input, cx| input.sync("", &placeholder, cx));
        let command = Command::Respond {
            cookie,
            user: self.user,
            response,
        };
        (self.dispatch)(Message::Polkit(command), window, cx);
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cookie = self.cookie.clone();
        (self.dispatch)(Message::Polkit(Command::Cancel { cookie }), window, cx);
    }

    fn cycle_user(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self
            .shell
            .read(cx)
            .snapshot
            .polkit
            .as_ref()
            .map_or(0, |r| r.users.len());
        if let (Ok(count), Ok(user)) = (isize::try_from(count), isize::try_from(self.user))
            && count > 1
        {
            self.user = usize::try_from(user.saturating_add(step).rem_euclid(count)).unwrap_or(0);
            cx.notify();
        }
    }
}

impl Render for PolkitView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = self.shell.read(cx);
        let config = &shell.config;
        let Some(request) = shell.snapshot.polkit.as_ref() else {
            return div().into_any_element();
        };
        let pixel = widgets::pixel_look();
        let reduce = cx.reduce_motion();
        let fg = color(config.colors.foreground);
        let muted = color(config.colors.muted);
        let accent = color(config.colors.accent);

        let users = (request.users.len() > 1).then(|| {
            widgets::row()
                .flex_wrap()
                .gap(px(6.))
                .children(request.users.iter().enumerate().map(|(i, name)| {
                    let selected = i == self.user;
                    widgets::frame(widgets::row())
                        .id(gpui::ElementId::Name(format!("polkit-user-{name}").into()))
                        .gap(px(5.))
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(widgets::corner(9999.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .map(|chip| {
                            if selected {
                                chip.bg(accent).text_color(color(config.colors.on_accent))
                            } else {
                                chip.bg(fg.opacity(0.08))
                                    .hover(|style| style.bg(fg.opacity(0.14)))
                            }
                        })
                        .active(|style| style.opacity(0.75))
                        .child(widgets::icon(config, "person", 14.))
                        .child(name.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.user = i;
                            cx.notify();
                        }))
                }))
        });

        // The input draws its own focus box; a second frame here would double it.
        let field = widgets::row()
            .gap(px(10.))
            .h(px(42.))
            .when(request.busy, |field| field.opacity(0.6))
            .child(
                widgets::icon(config, if request.echo { "person" } else { "key" }, 18.)
                    .text_color(muted),
            )
            .child(div().flex_1().min_w_0().child(self.input.clone()));

        let (status, status_color) = match (&request.error, &request.info, request.busy) {
            (_, _, true) => ("Checking…".to_owned(), muted),
            (Some(error), _, _) => (error.clone(), color(config.colors.urgent)),
            (None, Some(info), _) => (info.clone(), muted),
            (None, None, false) => (String::new(), muted),
        };

        let buttons = widgets::row()
            .gap(px(8.))
            .pt(px(4.))
            .child(
                action_button(config, "polkit-cancel", ("close", "Cancel"), false, true)
                    .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
            )
            .child(
                action_button(
                    config,
                    "polkit-submit",
                    ("lock_open", "Authenticate"),
                    true,
                    !request.busy,
                )
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            );

        let card = widgets::frame(widgets::surface(config, widgets::Material::Popover, 22.))
            .w(px(CARD_W))
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(22.))
            .child(
                widgets::row()
                    .gap(px(12.))
                    .child(widgets::status_circle(config, "shield_lock", true, 40.))
                    .child(
                        widgets::heading(div())
                            .text_size(px(17.))
                            .font_weight(widgets::font_weight(gpui::FontWeight::SEMIBOLD))
                            .child("Authentication required"),
                    ),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .child(request.message.clone()),
            )
            .children(users)
            .child(field)
            .child(
                div()
                    .min_h(px(16.))
                    .text_size(px(12.))
                    .text_color(status_color)
                    .child(status),
            )
            .child(buttons)
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(muted.opacity(0.8))
                    .truncate()
                    .child(request.action.clone()),
            );

        // Wrong password: a short sideways shake, stepped in the pixel look.
        let card = if request.attempts > 0 && !reduce {
            let id = (
                "polkit-shake",
                usize::try_from(request.attempts).unwrap_or(0),
            );
            card.with_animation(
                id,
                gpui::Animation::new(Duration::from_millis(360)),
                move |card, t| {
                    let offset = (t * std::f32::consts::TAU * 3.).sin() * (1. - t) * 10.;
                    let offset = if pixel {
                        (offset / 3.).round() * 3.
                    } else {
                        offset
                    };
                    card.relative().left(px(offset))
                },
            )
            .into_any_element()
        } else {
            card.into_any_element()
        };

        let card = div().child(card).with_animation(
            "polkit-open",
            gpui::Animation::new(Duration::from_millis(if reduce { 1 } else { 200 }))
                .with_easing(gpui::ease_out_quint()),
            move |card, t| {
                let t = if pixel { (t * 4.).round() / 4. } else { t };
                card.opacity(t).mt(px((1. - t) * 16.))
            },
        );

        let backdrop = gpui::black().opacity(if pixel { 0.6 } else { 0.42 });
        div()
            .bg(backdrop)
            .id("polkit")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .font_family(widgets::text_font(config))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.cancel(window, cx),
                    "tab" if event.keystroke.modifiers.shift => this.cycle_user(-1, cx),
                    "tab" => this.cycle_user(1, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(card)
            .into_any_element()
    }
}
