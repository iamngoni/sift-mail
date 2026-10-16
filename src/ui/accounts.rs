//! Adding IMAP accounts and running background sync from the main window.
//!
//! The dialog validates input into an [`AddAccountRequest`] before anything
//! touches the network. The sign-in check and every sync run on the background
//! executor with their own store connection; results come back to the UI
//! thread, which then reloads from the store.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, Focusable as _, FontWeight, TestSupportExt as _,
    Window, div, hsla, px,
};

use super::app::{SiftApp, Toast};
use crate::domain::{EmailAddress, Host, ImapSettings, Provider};
use crate::oauth::OAuthError;
use crate::sync::AddAccountRequest;
use crate::sync::imap::SyncError;
use crate::theme::tokens::{Palette, radius, text};

/// How often accounts sync while the app is open.
pub const SYNC_EVERY: Duration = Duration::from_secs(180);

/// The Add account dialog's inputs and state.
pub(crate) struct AddAccountForm {
    pub(crate) email: Entity<InputState>,
    pub(crate) password: Entity<InputState>,
    pub(crate) server: Entity<InputState>,
    pub(crate) error: Option<String>,
    pub(crate) busy: bool,
    /// Set while waiting for the browser; flip it to cancel.
    pub(crate) waiting: Option<(Provider, Arc<AtomicBool>)>,
    /// The organisation blocked the app; show the message for IT.
    pub(crate) blocked: bool,
}

/// Turns what was typed into a request, or a message to show under the form.
pub fn validate(email: &str, password: &str, server: &str) -> Result<AddAccountRequest, String> {
    let email = EmailAddress::parse(email).map_err(|e| e.to_string())?;
    let provider = Provider::detect(&email);
    let server = server.trim();
    if provider == Provider::Microsoft365 && server.is_empty() {
        return Err(
            "Use Continue with Microsoft above. If this account uses IMAP instead, \
                    enter its server below."
                .to_owned(),
        );
    }
    if password.is_empty() {
        return Err(
            "Enter your password, or an app password if your provider uses them.".to_owned(),
        );
    }
    let mut settings = ImapSettings::for_address(&email, provider);
    if !server.is_empty() {
        settings.host = Host::parse(server).map_err(|e| e.to_string())?;
    }
    let provider = if provider == Provider::Microsoft365 {
        Provider::Imap
    } else {
        provider
    };
    Ok(AddAccountRequest {
        name: default_name(&email, provider),
        email,
        provider,
        settings,
        password: password.to_owned(),
    })
}

/// The message the blocked screen offers to copy for an IT team.
pub fn it_request() -> String {
    "Hi IT team,

I'd like to use Sift Mail as my email app with my Microsoft 365 \n     account. Microsoft says it needs admin approval.

It asks to read and send my mail \n     (IMAP and SMTP) and to stay signed in.

Could you approve it in Microsoft Entra \n     (Enterprise applications > Admin consent requests)?

Thanks!"
        .to_owned()
}

/// "Gmail", "iCloud", or the capitalised first part of the domain.
pub fn default_name(email: &EmailAddress, provider: Provider) -> String {
    match provider {
        Provider::Google => "Gmail".to_owned(),
        Provider::ICloud => "iCloud".to_owned(),
        Provider::Microsoft365 | Provider::Imap => {
            let first = email.domain().split('.').next().unwrap_or_default();
            let mut chars = first.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_else(|| "Mail".to_owned())
        }
    }
}

impl SiftApp {
    /// Whether a sync is running.
    pub fn is_syncing(&self) -> bool {
        self.syncing
    }

    /// The sync status line, if there is one.
    pub fn sync_status(&self) -> Option<&str> {
        self.sync_status.as_deref()
    }

    /// Whether the Add account dialog is open.
    pub fn is_adding_account(&self) -> bool {
        self.form.is_some()
    }

    /// The dialog's error message, if any.
    pub fn form_error(&self) -> Option<&str> {
        self.form.as_ref().and_then(|f| f.error.as_deref())
    }

    /// Syncs now, then every [`SYNC_EVERY`] while the window is open.
    pub(crate) fn schedule_sync(&mut self, cx: &mut Context<Self>) {
        if self.sync.is_none() {
            return;
        }
        self.start_sync(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SYNC_EVERY).await;
                if this.update(cx, |app, cx| app.start_sync(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Runs one sync pass in the background unless one is already running.
    pub fn start_sync(&mut self, cx: &mut Context<Self>) {
        let Some(service) = self.sync.clone() else {
            return;
        };
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.sync_status = Some("Syncing…".to_owned());
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { service.sync_all(Utc::now()) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                app.syncing = false;
                app.sync_status = Some(match result {
                    Ok(summary) => summary.status_line(),
                    Err(error) => format!("Sync failed: {error}"),
                });
                app.reload();
                cx.notify();
            })
        })
        .detach();
    }

    /// Opens the Add account dialog with empty fields.
    pub fn open_add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("you@example.com"));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password or app password")
                .masked(true)
        });
        let server = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Server (optional), e.g. imap.example.com")
        });
        email.read(cx).focus_handle(cx).focus(window, cx);
        self.form = Some(AddAccountForm {
            email,
            password,
            server,
            error: None,
            busy: false,
            waiting: None,
            blocked: false,
        });
        cx.notify();
    }

    /// Closes the dialog and returns focus to the inbox. Cancels a pending
    /// browser sign-in so the listener stops waiting.
    pub fn close_add_account(&mut self, cx: &mut Context<Self>) {
        if let Some((_, cancel)) = self.form.as_ref().and_then(|f| f.waiting.as_ref()) {
            cancel.store(true, Ordering::Relaxed);
        }
        self.form = None;
        cx.notify();
    }

    /// Whether the dialog is waiting for the browser.
    pub fn is_waiting_for_browser(&self) -> bool {
        self.form.as_ref().is_some_and(|f| f.waiting.is_some())
    }

    /// Whether the organisation blocked the last sign-in.
    pub fn is_blocked(&self) -> bool {
        self.form.as_ref().is_some_and(|f| f.blocked)
    }

    /// Opens the provider's sign-in page and waits for it in the background.
    pub fn start_oauth(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if form.waiting.is_some() {
            return;
        }
        let Some(service) = self.sync.clone() else {
            form.error = Some("Sift can't save accounts in this session.".to_owned());
            cx.notify();
            return;
        };
        let hint = EmailAddress::parse(&form.email.read(cx).value()).ok();
        let cancel = Arc::new(AtomicBool::new(false));
        form.waiting = Some((provider, Arc::clone(&cancel)));
        form.blocked = false;
        form.error = None;
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { service.sign_in(provider, hint, cancel) });
        let focus = self.focus.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, window, cx| {
                match result {
                    Ok(account) => {
                        app.form = None;
                        app.toast = Some(Toast {
                            text: format!("Added {}. Fetching your mail…", account.name),
                            undo: None,
                        });
                        focus.focus(window, cx);
                        app.reload();
                        app.start_sync(cx);
                    }
                    Err(error) => {
                        if let Some(form) = app.form.as_mut() {
                            form.waiting = None;
                            match error {
                                SyncError::OAuth(OAuthError::Cancelled) => {}
                                SyncError::OAuth(OAuthError::AdminApprovalRequired) => {
                                    form.blocked = true;
                                    form.error =
                                        Some(OAuthError::AdminApprovalRequired.to_string());
                                }
                                other => form.error = Some(other.to_string()),
                            }
                        }
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Stops waiting for the browser.
    pub fn cancel_oauth(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.form.as_mut()
            && let Some((_, cancel)) = form.waiting.take()
        {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Puts the request for IT on the clipboard.
    pub fn copy_it_request(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(it_request()));
        self.toast = Some(Toast {
            text: "Copied the request for your IT team".to_owned(),
            undo: None,
        });
        cx.notify();
    }

    fn oauth_section(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.form.as_ref() else {
            return div().into_any_element();
        };
        if let Some((provider, _)) = &form.waiting {
            let name = if *provider == Provider::Google {
                "Google"
            } else {
                "Microsoft"
            };
            return div()
                .p(px(14.0))
                .rounded(px(radius::BUTTON))
                .bg(p.accent_soft)
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    Icon::new(IconName::LoaderCircle)
                        .size(px(18.0))
                        .text_color(p.accent_text),
                )
                .child(
                    div()
                        .flex_1()
                        .child(format!("Finish signing in with {name} in your browser…")),
                )
                .child(
                    div()
                        .id("cancel-oauth")
                        .h(px(28.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(p.line_strong)
                        .child("Cancel")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_oauth(cx)))
                        .test_support(),
                )
                .into_any_element();
        }
        let hover = p.bg_hover;
        let button =
            |id: &'static str, label: &'static str, provider: Provider, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .flex_1()
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .rounded(px(radius::BUTTON))
                    .border_1()
                    .border_color(p.line_strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .hover(move |s| s.bg(hover))
                    .child(
                        Icon::new(if provider == Provider::Google {
                            IconName::Globe
                        } else {
                            IconName::Building2
                        })
                        .size(px(16.0)),
                    )
                    .child(label)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.start_oauth(provider, window, cx);
                    }))
                    .test_support()
            };
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(button(
                        "signin-microsoft",
                        "Continue with Microsoft",
                        Provider::Microsoft365,
                        cx,
                    ))
                    .child(button(
                        "signin-google",
                        "Continue with Google",
                        Provider::Google,
                        cx,
                    )),
            )
            .when(form.blocked, |d| {
                d.child(
                    div()
                        .p(px(12.0))
                        .rounded(px(radius::BUTTON))
                        .bg(p.bg_subtle)
                        .border_1()
                        .border_color(p.line_strong)
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(div().text_size(px(text::META)).text_color(p.ink_2).child(
                            "Microsoft 365 only lets approved apps connect. Nothing was \
                                        connected and no mail left your account. Send your IT team \
                                        this request:",
                        ))
                        .child(
                            div()
                                .id("copy-it-request")
                                .h(px(28.0))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(p.line_strong)
                                .child("Copy request for IT")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.copy_it_request(cx)
                                }))
                                .test_support(),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .text_size(px(text::META))
                    .text_color(p.ink_3)
                    .child(div().flex_1().h(px(1.0)).bg(p.line))
                    .child("or use a password (IMAP)")
                    .child(div().flex_1().h(px(1.0)).bg(p.line)),
            )
            .into_any_element()
    }

    /// Validates the dialog, then checks the sign-in in the background.
    pub fn submit_add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if form.busy {
            return;
        }
        let email = form.email.read(cx).value();
        let password = form.password.read(cx).value();
        let server = form.server.read(cx).value();
        let request = match validate(&email, &password, &server) {
            Ok(request) => request,
            Err(message) => {
                form.error = Some(message);
                cx.notify();
                return;
            }
        };
        let Some(service) = self.sync.clone() else {
            form.error = Some("Sift can't save accounts in this session.".to_owned());
            cx.notify();
            return;
        };
        form.busy = true;
        form.error = None;
        cx.notify();
        let name = request.name.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.add_account(request) });
        let focus = self.focus.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, window, cx| {
                match result {
                    Ok(_) => {
                        app.form = None;
                        app.toast = Some(Toast {
                            text: format!("Added {name}. Fetching your mail…"),
                            undo: None,
                        });
                        focus.focus(window, cx);
                        app.reload();
                        app.start_sync(cx);
                    }
                    Err(error) => {
                        if let Some(form) = app.form.as_mut() {
                            form.busy = false;
                            form.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    pub(crate) fn add_account_dialog(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.form.as_ref() else {
            return div().into_any_element();
        };
        let field = |label: &'static str, state: &Entity<InputState>, id: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(text::META))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.ink_2)
                        .child(label),
                )
                .child(Input::new(state).id(id))
        };
        let hover = p.bg_hover;
        div()
            .id("add-account-scrim")
            .absolute()
            .inset_0()
            .bg(hsla(0.0, 0.0, 0.0, 0.28))
            .flex()
            .justify_center()
            .pt(px(96.0))
            .child(
                div()
                    .id("add-account")
                    .w(px(440.0))
                    .h_auto()
                    .p(px(20.0))
                    .rounded(px(radius::CARD))
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.line_strong)
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(px(text::TITLE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Add an account"),
                            )
                            .child(div().text_color(p.ink_3).child(
                                "Sign in with your provider, or use a password for any IMAP \
                                 account.",
                            )),
                    )
                    .child(self.oauth_section(p, cx))
                    .child(field("Email", &form.email, "field-email"))
                    .child(field("Password", &form.password, "field-password"))
                    .child(field("Server", &form.server, "field-server"))
                    .when_some(form.error.clone(), |d, error| {
                        d.child(
                            div()
                                .id("form-error")
                                .flex()
                                .gap(px(8.0))
                                .text_color(p.danger)
                                .child(Icon::new(IconName::CircleAlert).size(px(16.0)))
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .id("cancel-add")
                                    .h(px(32.0))
                                    .px(px(12.0))
                                    .flex()
                                    .items_center()
                                    .rounded(px(radius::BUTTON))
                                    .border_1()
                                    .border_color(p.line_strong)
                                    .hover(move |s| s.bg(hover))
                                    .child("Cancel")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.close_add_account(cx);
                                    }))
                                    .test_support(),
                            )
                            .child(
                                div()
                                    .id("connect")
                                    .h(px(32.0))
                                    .px(px(14.0))
                                    .flex()
                                    .items_center()
                                    .rounded_full()
                                    .bg(p.primary)
                                    .text_color(p.primary_ink)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if form.busy { "Checking…" } else { "Connect" })
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.submit_add_account(window, cx);
                                    }))
                                    .test_support(),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_explains_each_problem() {
        assert_eq!(
            validate("nope", "pw", "").unwrap_err(),
            "An email address has exactly one @."
        );
        assert!(
            validate("you@company.com", "pw", "")
                .unwrap_err()
                .contains("Continue with Microsoft")
        );
        assert!(
            validate("you@fastmail.com", "", "")
                .unwrap_err()
                .starts_with("Enter your password")
        );
        assert!(
            validate("you@fastmail.com", "pw", "bad host")
                .unwrap_err()
                .contains("imap.example.com")
        );
    }

    #[test]
    fn valid_input_becomes_a_request() {
        let r = validate(" you@fastmail.com ", "pw", "").unwrap();
        assert_eq!(r.settings.host.as_str(), "imap.fastmail.com");
        assert_eq!(r.provider, Provider::Imap);
        assert_eq!(r.name, "Fastmail");

        let r = validate("you@company.com", "pw", "mail.company.com").unwrap();
        assert_eq!(r.settings.host.as_str(), "mail.company.com");
        assert_eq!(r.provider, Provider::Imap, "a company server is plain IMAP");

        let r = validate("you@gmail.com", "app-pw", "").unwrap();
        assert_eq!((r.name.as_str(), r.provider), ("Gmail", Provider::Google));
        assert_eq!(validate("you@icloud.com", "x", "").unwrap().name, "iCloud");
    }
}
