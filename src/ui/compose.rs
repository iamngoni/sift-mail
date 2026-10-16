//! Writing mail from the main window: the reply box under an open thread and
//! the New message dialog. Also pushes read state back to the server.
//!
//! Input is turned into a [`Draft`] before anything is sent, so the send path
//! only sees valid recipients and a non-empty body. Sending runs in the
//! background; the composer keeps its text until the server accepts the mail.

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Focusable as _, FontWeight, TestSupportExt as _, Window, div,
    px,
};

use super::app::{SiftApp, Toast};
use crate::domain::outgoing::reply_subject;
use crate::domain::{AccountId, Draft, Recipients, ReplyContext, ThreadRef};
use crate::sync::send::SendRequest;
use crate::theme::tokens::{Palette, radius, text};

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
        let sendable: Vec<AccountId> = self
            .store
            .imap_accounts()
            .ok()?
            .into_iter()
            .map(|(account, _, _)| account.id)
            .collect();
        self.default_from()
            .filter(|id| sendable.contains(id))
            .or_else(|| sendable.first().copied())
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

    /// Validates and sends the New message dialog.
    pub fn send_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }
        let account = self.sending_account();
        let Some(form) = self.compose.as_mut() else {
            return;
        };
        let to = form.to_text(cx);
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
            .children(self.drafts_row(p, cx))
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
                    .children(self.tone_buttons(p, cx))
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
}

pub(super) fn send_button(
    id: &'static str,
    sending: bool,
    p: &Palette,
) -> gpui_kit::Stateful<gpui_kit::Div> {
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
