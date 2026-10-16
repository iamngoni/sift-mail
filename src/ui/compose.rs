//! Writing mail from the main window: the reply box under an open thread and
//! the New message dialog. Also pushes read state back to the server.
//!
//! Input is turned into a [`Draft`] before anything is sent, so the send path
//! only sees valid recipients and a non-empty body. Sending runs in the
//! background; the composer keeps its text until the server accepts the mail.

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, Focusable as _, FontWeight, TestSupportExt as _,
    Window, div, hsla, px,
};

use super::app::{SiftApp, Toast};
use crate::domain::outgoing::reply_subject;
use crate::domain::{AccountId, Draft, Recipients, ReplyContext, ThreadRef};
use crate::sync::send::SendRequest;
use crate::theme::tokens::{Palette, radius, text};

/// The New message dialog.
pub(crate) struct ComposeForm {
    pub(crate) to: Entity<InputState>,
    pub(crate) subject: Entity<InputState>,
    pub(crate) body: Entity<TextareaState>,
    pub(crate) error: Option<String>,
}

impl SiftApp {
    /// Whether the New message dialog is open.
    pub fn is_composing(&self) -> bool {
        self.compose.is_some()
    }

    /// The error under the reply box or composer, if any.
    pub fn compose_error(&self) -> Option<&str> {
        self.compose
            .as_ref()
            .and_then(|c| c.error.as_deref())
            .or(self.reply_error.as_deref())
    }

    /// Whether a message is being sent.
    pub fn is_sending(&self) -> bool {
        self.sending
    }

    /// The account new mail is sent from: the first that can send.
    fn sending_account(&self) -> Option<AccountId> {
        self.store
            .imap_accounts()
            .ok()?
            .into_iter()
            .map(|(account, _, _)| account.id)
            .next()
    }

    pub(crate) fn ensure_reply_box(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_some() && self.reply_box.is_none() {
            let reply = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Write a reply…")
                    .auto_grow(4, 12)
            });
            // Ctrl+Enter (Cmd+Enter on macOS) arrives as a secondary PressEnter.
            cx.subscribe_in(&reply, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(
                    event,
                    InputEvent::PressEnter {
                        secondary: true,
                        ..
                    }
                ) {
                    this.send_reply(window, cx);
                }
            })
            .detach();
            self.reply_box = Some(reply);
        }
    }

    /// Moves the keyboard into the reply box.
    pub fn focus_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_reply_box(window, cx);
        if let Some(reply) = &self.reply_box {
            reply.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// Sends the reply box's text to the newest message's sender.
    pub fn send_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(open), Some(reply)) = (self.open.as_ref(), self.reply_box.as_ref()) else {
            return;
        };
        if self.sending {
            return;
        }
        let body = reply.read(cx).value();
        let thread = open.summary.thread.clone();
        let Some(last) = open.messages.last() else {
            return;
        };
        let to = Recipients::one(open.summary.from.email().clone());
        let context = last
            .message_id
            .as_deref()
            .map(|id| ReplyContext::answering(id, &last.references));
        // Without a Message-ID there is nothing to thread on; send a plain "Re:".
        let subject = reply_subject(&open.summary.subject);
        let draft = Draft::new(to, &subject, &body).map(|draft| match context {
            Some(context) => draft.replying(context),
            None => draft,
        });
        let draft = match draft {
            Ok(draft) => draft,
            Err(error) => {
                self.reply_error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let first = open
            .summary
            .from
            .display()
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_owned();
        self.dispatch_send(
            SendRequest {
                account: thread.account(),
                draft,
                thread: Some(thread),
            },
            format!("Reply sent to {first} · thread done"),
            window,
            cx,
        );
    }

    /// Opens the New message dialog.
    pub fn open_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let to = cx.new(|cx| InputState::new(window, cx).placeholder("Name@example.com, …"));
        let subject = cx.new(|cx| InputState::new(window, cx).placeholder("Subject"));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Write your message…")
                .auto_grow(6, 14)
        });
        for state in [&to, &subject] {
            cx.subscribe_in(state, window, send_on_secondary_enter)
                .detach();
        }
        cx.subscribe_in(&body, window, send_on_secondary_enter)
            .detach();
        to.read(cx).focus_handle(cx).focus(window, cx);
        self.compose = Some(ComposeForm {
            to,
            subject,
            body,
            error: None,
        });
        cx.notify();
    }

    /// Closes the New message dialog, discarding it.
    pub fn close_compose(&mut self, cx: &mut Context<Self>) {
        self.compose = None;
        cx.notify();
    }

    /// Validates and sends the New message dialog.
    pub fn send_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }
        let account = self.sending_account();
        let Some(form) = self.compose.as_mut() else {
            return;
        };
        let to = form.to.read(cx).value();
        let subject = form.subject.read(cx).value();
        let body = form.body.read(cx).value();
        let draft = Recipients::parse(&to).and_then(|to| Draft::new(to, &subject, &body));
        let (draft, account) = match (draft, account) {
            (Ok(draft), Some(account)) => (draft, account),
            (Err(error), _) => {
                form.error = Some(error.to_string());
                cx.notify();
                return;
            }
            (Ok(_), None) => {
                form.error = Some("Add an account before sending mail.".to_owned());
                cx.notify();
                return;
            }
        };
        let names: Vec<String> = draft
            .to()
            .as_slice()
            .iter()
            .map(|a| a.local_part().to_owned())
            .collect();
        self.dispatch_send(
            SendRequest {
                account,
                draft,
                thread: None,
            },
            format!("Sent to {}", names.join(", ")),
            window,
            cx,
        );
    }

    fn dispatch_send(
        &mut self,
        request: SendRequest,
        done_text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(service) = self.sync.clone() else {
            let message = "Sift can't send mail in this session.".to_owned();
            match self.compose.as_mut() {
                Some(form) => form.error = Some(message),
                None => self.reply_error = Some(message),
            }
            cx.notify();
            return;
        };
        self.sending = true;
        self.reply_error = None;
        if let Some(form) = self.compose.as_mut() {
            form.error = None;
        }
        cx.notify();
        let is_reply = request.thread.is_some();
        let task = cx
            .background_executor()
            .spawn(async move { service.send(request) });
        let focus = self.focus.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, window, cx| {
                app.sending = false;
                match result {
                    Ok(outcome) => {
                        let text = if outcome.saved_to_sent {
                            done_text
                        } else {
                            format!("{done_text} (not saved to the server's Sent folder)")
                        };
                        app.toast = Some(Toast { text, undo: None });
                        if is_reply {
                            app.reply_box = None;
                            app.open = None;
                        } else {
                            app.compose = None;
                        }
                        focus.focus(window, cx);
                        app.reload();
                    }
                    Err(error) => {
                        let message = format!("Couldn't send: {error}");
                        match app.compose.as_mut() {
                            Some(form) if !is_reply => form.error = Some(message),
                            _ => app.reply_error = Some(message),
                        }
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Writes read state to the server in the background. Failures only
    /// update the status line; the local state already changed.
    pub(crate) fn push_read(&mut self, thread: ThreadRef, read: bool, cx: &mut Context<Self>) {
        let Some(service) = self.sync.clone() else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { service.write_read(thread, read) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                this.update(cx, |app, cx| {
                    app.sync_status =
                        Some(format!("Couldn't update read state on the server: {error}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn reply_card(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let (Some(open), Some(reply)) = (self.open.as_ref(), self.reply_box.as_ref()) else {
            return div().into_any_element();
        };
        let to = open.summary.from.display().to_owned();
        div()
            .rounded(px(radius::CARD))
            .border_1()
            .border_color(p.line_strong)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(14.0))
                    .py(px(10.0))
                    .border_b_1()
                    .border_color(p.line)
                    .bg(p.bg_subtle)
                    .text_size(px(text::META))
                    .text_color(p.ink_3)
                    .child(format!("Reply to {to}  ·  R")),
            )
            .child(div().p(px(8.0)).child(Textarea::new(reply)))
            .child(
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_t_1()
                    .border_color(p.line)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when_some(self.reply_error.clone(), |d, e| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(p.danger)
                                .text_size(px(text::META))
                                .child(e),
                        )
                    })
                    .when(self.reply_error.is_none(), |d| {
                        d.child(
                            div()
                                .flex_1()
                                .text_size(px(text::META))
                                .text_color(p.ink_3)
                                .child("Sending marks the thread done"),
                        )
                    })
                    .child(
                        send_button("send-reply", self.sending, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.send_reply(window, cx)
                            }))
                            .test_support(),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn compose_dialog(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.compose.as_ref() else {
            return div().into_any_element();
        };
        let row = |label: &'static str, input: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(div().w(px(64.0)).text_color(p.ink_3).child(label))
                .child(div().flex_1().child(input))
        };
        let hover = p.bg_hover;
        div()
            .id("compose-scrim")
            .absolute()
            .inset_0()
            .bg(hsla(0.0, 0.0, 0.0, 0.28))
            .flex()
            .justify_center()
            .pt(px(80.0))
            .child(
                div()
                    .id("compose")
                    .w(px(560.0))
                    .h_auto()
                    .rounded(px(radius::CARD))
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.line_strong)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(44.0))
                            .px(px(16.0))
                            .flex()
                            .items_center()
                            .bg(p.bg_subtle)
                            .border_b_1()
                            .border_color(p.line)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("New message"),
                    )
                    .child(
                        div()
                            .p(px(14.0))
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(row(
                                "To",
                                Input::new(&form.to).id("compose-to").into_any_element(),
                            ))
                            .child(row(
                                "Subject",
                                Input::new(&form.subject)
                                    .id("compose-subject")
                                    .into_any_element(),
                            ))
                            .child(Textarea::new(&form.body)),
                    )
                    .child(
                        div()
                            .px(px(14.0))
                            .py(px(10.0))
                            .border_t_1()
                            .border_color(p.line)
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .when_some(form.error.clone(), |d, e| {
                                d.child(
                                    div()
                                        .id("compose-error")
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .gap(px(6.0))
                                        .text_color(p.danger)
                                        .child(Icon::new(IconName::CircleAlert).size(px(14.0)))
                                        .child(e),
                                )
                            })
                            .when(form.error.is_none(), |d| d.child(div().flex_1()))
                            .child(
                                div()
                                    .id("compose-cancel")
                                    .flex_none()
                                    .h(px(32.0))
                                    .px(px(12.0))
                                    .flex()
                                    .items_center()
                                    .rounded(px(radius::BUTTON))
                                    .border_1()
                                    .border_color(p.line_strong)
                                    .hover(move |s| s.bg(hover))
                                    .child("Discard")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.close_compose(cx)
                                    }))
                                    .test_support(),
                            )
                            .child(
                                send_button("compose-send", self.sending, p)
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.send_compose(window, cx)
                                    }))
                                    .test_support(),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn send_on_secondary_enter<S: 'static>(
    this: &mut SiftApp,
    _: &Entity<S>,
    event: &InputEvent,
    window: &mut Window,
    cx: &mut Context<SiftApp>,
) {
    if matches!(
        event,
        InputEvent::PressEnter {
            secondary: true,
            ..
        }
    ) {
        this.send_compose(window, cx);
    }
}

fn send_button(id: &'static str, sending: bool, p: &Palette) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(32.0))
        .px(px(14.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded_full()
        .bg(p.primary)
        .text_color(p.primary_ink)
        .font_weight(FontWeight::SEMIBOLD)
        .child(if sending { "Sending…" } else { "Send" })
        .child(
            div()
                .text_size(px(text::SMALL))
                .opacity(0.8)
                .child("Ctrl ↵"),
        )
}
