//! The Add account flow from the AddAccount board, in four steps:
//!
//! 1. **Choose**: type an address (Sift guesses the provider) or pick one.
//! 2. **Sign in**: the provider's page in the browser (with a waiting state
//!    and the organisation-blocked screen), or a password form for iCloud and
//!    other IMAP servers with editable server settings.
//! 3. **Set up**: name, colour, what to sync, how far back, AI, notifications.
//! 4. **Connecting**: the first sync, with live progress.
//!
//! Signing in already saves the account, held back from sync (`ready = 0`)
//! until Set up is finished, so the chosen range applies to the first sync.
//! Rendering lives in [`super::add_account_view`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, Entity, Focusable as _, Window};

use crate::domain::{AuthMethod, EmailAddress, Host, ImapSettings, Provider, SmtpSettings};
use crate::oauth::OAuthError;
use crate::store::{Account, AccountSettings, NotifyLevel, SyncRange};
use crate::sync::AddAccountRequest;
use crate::sync::imap::SyncError;
use crate::sync::service::LinkSlot;

use super::accounts::default_name;
use super::app::{SiftApp, Toast};

/// How often the Connecting step re-counts stored messages.
const PROGRESS_EVERY: Duration = Duration::from_millis(400);

/// Where the flow is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Address or provider.
    Choose,
    /// About to open the provider's sign-in page.
    OAuth,
    /// Waiting for the browser.
    Waiting,
    /// The organisation needs to approve the app.
    Blocked,
    /// Password sign-in for iCloud and other IMAP servers.
    Imap,
    /// Name, colour, sync and notification choices.
    Setup,
    /// First sync running.
    Sync,
}

impl Step {
    /// "Step n of 4".
    pub fn number(self) -> usize {
        match self {
            Self::Choose => 1,
            Self::OAuth | Self::Waiting | Self::Blocked | Self::Imap => 2,
            Self::Setup => 3,
            Self::Sync => 4,
        }
    }
}

/// What Sift guessed from a typed address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// The provider to use.
    pub provider: Provider,
    /// The line shown under the address.
    pub text: String,
}

/// Guesses the provider for a typed address, with the sentence shown under it.
pub fn detect(text: &str) -> Option<Detected> {
    let email = EmailAddress::parse(text.trim()).ok()?;
    let provider = Provider::detect(&email);
    let domain = email.domain().to_owned();
    let text = match (provider, domain.as_str()) {
        (Provider::Google, _) => "Gmail address. We will use Google sign-in.".to_owned(),
        (Provider::ICloud, _) => {
            "iCloud address. You will need an app-specific password.".to_owned()
        }
        (Provider::Imap, _) => format!("{domain} supports IMAP. We found its server settings."),
        (Provider::Microsoft365, "outlook.com" | "hotmail.com" | "live.com" | "msn.com") => {
            "Outlook.com address. We will use Microsoft sign-in.".to_owned()
        }
        (Provider::Microsoft365, _) => format!(
            "{domain} is probably on Microsoft 365. We will try Microsoft sign-in; pick Other email if it uses IMAP."
        ),
    };
    Some(Detected { provider, text })
}

/// The message for an IT team when an organisation blocks the app.
pub fn it_request(domain: &str) -> String {
    format!(
        "Hi IT team,\n\nI'd like to use Sift Mail as my email app with my {domain} Microsoft 365 account. Microsoft says it needs admin approval.\n\nIt asks to read and send my mail (IMAP and SMTP) and to stay signed in.\n\nCould you approve it in Microsoft Entra (Enterprise applications > Admin consent requests)?\n\nThanks!"
    )
}

/// What a provider's consent screen will ask for, and why Sift needs it.
pub fn scopes(provider: Provider) -> [(&'static str, &'static str); 3] {
    match provider {
        Provider::Google => [
            (
                "Read, organise and send mail",
                "to show and manage your inbox",
            ),
            ("Stay signed in", "so mail keeps syncing in the background"),
            ("Your name and email address", "to label the account"),
        ],
        _ => [
            ("Read and send mail", "to show and manage your inbox"),
            ("Stay signed in", "so mail keeps syncing in the background"),
            ("Your name and email address", "to label the account"),
        ],
    }
}

/// Help under the range choice.
pub fn range_help(range: SyncRange) -> &'static str {
    match range {
        SyncRange::Days30 => "Faster to set up. Older mail stays on the server.",
        SyncRange::Days90 => "A good balance. Older mail stays on the server.",
        SyncRange::Year => "Takes a few minutes and uses more disk space.",
        SyncRange::Everything => {
            "Can take a while for large mailboxes. You can use the app while it runs."
        }
    }
}

/// Short provider name for headings: "Microsoft", "Google", "iCloud", "IMAP".
pub fn short_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Microsoft365 => "Microsoft",
        Provider::Google => "Google",
        Provider::ICloud => "iCloud",
        Provider::Imap => "IMAP",
    }
}

/// One line per provider tile: description and sign-in method.
pub fn provider_blurb(provider: Provider) -> (&'static str, &'static str) {
    match provider {
        Provider::Microsoft365 => (
            "Work or school accounts, Outlook.com, Hotmail",
            "Browser sign-in",
        ),
        Provider::Google => ("Gmail and Google Workspace", "Browser sign-in"),
        Provider::ICloud => ("icloud.com, me.com and mac.com addresses", "App password"),
        Provider::Imap => (
            "Fastmail, Proton Bridge, your host, anything with IMAP",
            "Password",
        ),
    }
}

/// The Add account flow's state.
pub(crate) struct AddAccountFlow {
    pub(crate) step: Step,
    pub(crate) provider: Provider,
    /// No accounts yet: a welcome above the dialog, and no way to close it.
    pub(crate) first_run: bool,
    pub(crate) email: Entity<InputState>,
    pub(crate) password: Entity<InputState>,
    pub(crate) imap_host: Entity<InputState>,
    pub(crate) imap_port: Entity<InputState>,
    pub(crate) smtp_host: Entity<InputState>,
    pub(crate) smtp_port: Entity<InputState>,
    pub(crate) show_servers: bool,
    pub(crate) error: Option<String>,
    pub(crate) busy: bool,
    pub(crate) cancel: Option<Arc<AtomicBool>>,
    pub(crate) link: LinkSlot,
    pub(crate) copied_link: bool,
    pub(crate) copied_it: bool,
    /// The account once signed in.
    pub(crate) account: Option<Account>,
    pub(crate) name: Entity<InputState>,
    pub(crate) settings: AccountSettings,
    /// Messages stored so far during the first sync.
    pub(crate) synced: usize,
    /// The first sync finished; `Err` holds why it failed.
    pub(crate) sync_result: Option<Result<(), String>>,
}

impl AddAccountFlow {
    /// The typed address, trimmed.
    pub(crate) fn typed_email(&self, cx: &gpui_kit::App) -> String {
        self.email.read(cx).value().trim().to_owned()
    }
}

impl SiftApp {
    /// Whether the Add account flow is open.
    pub fn is_adding_account(&self) -> bool {
        self.form.is_some()
    }

    /// The flow's step, if it's open.
    pub fn add_step(&self) -> Option<Step> {
        self.form.as_ref().map(|f| f.step)
    }

    /// The flow's error message, if any.
    pub fn form_error(&self) -> Option<&str> {
        self.form.as_ref().and_then(|f| f.error.as_deref())
    }

    /// Whether the flow is waiting for the browser.
    pub fn is_waiting_for_browser(&self) -> bool {
        self.add_step() == Some(Step::Waiting)
    }

    /// Whether the organisation blocked the last sign-in.
    pub fn is_blocked(&self) -> bool {
        self.add_step() == Some(Step::Blocked)
    }

    /// Opens the flow at step 1.
    pub fn open_add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let email = input("you@company.com", window, cx);
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password or app password")
                .masked(true)
        });
        let imap_host = input("imap.example.com", window, cx);
        let imap_port = input("993", window, cx);
        let smtp_host = input("smtp.example.com", window, cx);
        let smtp_port = input("465", window, cx);
        let name = input("Work", window, cx);
        cx.subscribe_in(
            &email,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.continue_with_email(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &password,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_password(window, cx);
                }
            },
        )
        .detach();
        email.read(cx).focus_handle(cx).focus(window, cx);
        self.overlay = None;
        self.form = Some(AddAccountFlow {
            step: Step::Choose,
            provider: Provider::Microsoft365,
            first_run: self.accounts.is_empty(),
            email,
            password,
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            show_servers: false,
            error: None,
            busy: false,
            cancel: None,
            link: LinkSlot::default(),
            copied_link: false,
            copied_it: false,
            account: None,
            name,
            settings: AccountSettings {
                in_all: true,
                ai: true,
                calendar: true,
                contacts: true,
                ..AccountSettings::default()
            },
            synced: 0,
            sync_result: None,
        });
        cx.notify();
    }

    /// Whether the flow can be closed now: not on a first run, except once connected.
    pub fn can_close_add_account(&self) -> bool {
        self.form
            .as_ref()
            .is_some_and(|f| !f.first_run || f.step == Step::Sync)
    }

    /// Closes the flow. Cancels a pending browser sign-in, and lets an account
    /// that was signed in but not set up start syncing with its defaults.
    pub fn close_add_account(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.form.take() else {
            return;
        };
        if let Some(cancel) = &flow.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        if let (Some(account), Step::Setup) = (&flow.account, flow.step) {
            if let Err(error) = self.store.set_ready(account.id, true) {
                self.error = Some(format!("Sift couldn't finish adding the account: {error}"));
            }
            self.start_sync(cx);
        }
        self.reload();
        cx.notify();
    }

    /// Continues from the typed address with the detected provider.
    pub fn continue_with_email(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(flow) = &self.form else {
            return;
        };
        if let Some(found) = detect(&flow.typed_email(cx)) {
            self.pick_provider(found.provider, window, cx);
        }
    }

    /// Goes to the sign-in step for a provider.
    pub fn pick_provider(
        &mut self,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(flow) = &mut self.form else {
            return;
        };
        flow.provider = provider;
        flow.error = None;
        let typed = flow.typed_email(cx);
        match provider {
            Provider::Microsoft365 | Provider::Google => flow.step = Step::OAuth,
            Provider::ICloud | Provider::Imap => {
                flow.step = Step::Imap;
                let email = EmailAddress::parse(&typed).ok();
                let imap = email
                    .as_ref()
                    .map(|e| ImapSettings::for_address(e, provider));
                let smtp = match (&email, &imap) {
                    (Some(e), Some(i)) => Some(SmtpSettings::for_imap(e, provider, i)),
                    _ => None,
                };
                let fields = [
                    (
                        flow.imap_host.clone(),
                        imap.as_ref().map(|i| i.host.to_string()),
                    ),
                    (
                        flow.imap_port.clone(),
                        imap.as_ref().map(|i| i.port.to_string()),
                    ),
                    (
                        flow.smtp_host.clone(),
                        smtp.as_ref().map(|s| s.host.to_string()),
                    ),
                    (
                        flow.smtp_port.clone(),
                        smtp.as_ref().map(|s| s.port.to_string()),
                    ),
                ];
                let password = flow.password.clone();
                for (input, value) in fields {
                    if let Some(value) = value {
                        input.update(cx, |s, cx| s.set_value(value, window, cx));
                    }
                }
                password.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
        cx.notify();
    }

    /// One step back.
    pub fn add_account_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(flow) = &mut self.form else {
            return;
        };
        flow.error = None;
        flow.step = match flow.step {
            Step::Setup | Step::Sync => {
                // The account exists by now; going back means starting over with it kept.
                return;
            }
            Step::Waiting => {
                if let Some(cancel) = flow.cancel.take() {
                    cancel.store(true, Ordering::Relaxed);
                }
                Step::OAuth
            }
            _ => Step::Choose,
        };
        if flow.step == Step::Choose {
            let email = flow.email.clone();
            email.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// Shows or hides the server fields.
    pub fn toggle_servers(&mut self, cx: &mut Context<Self>) {
        if let Some(flow) = &mut self.form {
            flow.show_servers = !flow.show_servers;
            cx.notify();
        }
    }

    /// Opens the provider's sign-in page and waits for it in the background.
    pub fn start_oauth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(flow) = self.form.as_mut() else {
            return;
        };
        if flow.cancel.is_some() {
            return;
        }
        let Some(service) = self.sync.clone() else {
            flow.error = Some("Sift can't save accounts in this session.".to_owned());
            cx.notify();
            return;
        };
        let provider = flow.provider;
        let hint = EmailAddress::parse(&flow.typed_email(cx)).ok();
        let cancel = Arc::new(AtomicBool::new(false));
        flow.cancel = Some(Arc::clone(&cancel));
        flow.link = LinkSlot::default();
        flow.copied_link = false;
        flow.step = Step::Waiting;
        flow.error = None;
        let link = Arc::clone(&flow.link);
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { service.sign_in(provider, hint, link, cancel) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, window, cx| {
                let Some(flow) = app.form.as_mut() else {
                    return;
                };
                flow.cancel = None;
                match result {
                    Ok(account) => app.signed_in(account, window, cx),
                    Err(SyncError::OAuth(OAuthError::Cancelled)) => {}
                    Err(SyncError::OAuth(OAuthError::AdminApprovalRequired)) => {
                        flow.step = Step::Blocked;
                    }
                    Err(other) => {
                        flow.step = Step::OAuth;
                        flow.error = Some(other.to_string());
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Stops waiting for the browser.
    pub fn cancel_oauth(&mut self, cx: &mut Context<Self>) {
        if let Some(flow) = self.form.as_mut() {
            if let Some(cancel) = flow.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
            }
            flow.step = Step::OAuth;
            flow.error = None;
        }
        cx.notify();
    }

    /// Copies the sign-in link, for when the browser didn't open.
    pub fn copy_sign_in_link(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.form.as_mut() else {
            return;
        };
        let link = flow.link.lock().ok().and_then(|l| l.clone());
        match link {
            Some(url) => {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(url));
                flow.copied_link = true;
            }
            None => {
                flow.error = Some("The sign-in link isn't ready yet. Try again in a moment.".into())
            }
        }
        cx.notify();
    }

    /// Puts the request for IT on the clipboard.
    pub fn copy_it_request(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.form.as_mut() else {
            return;
        };
        let typed = flow.typed_email(cx);
        let domain = typed.split_once('@').map_or("your", |(_, d)| d).to_owned();
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(it_request(&domain)));
        flow.copied_it = true;
        cx.notify();
    }

    /// Back to step 1 with an empty address.
    pub fn restart_add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(flow) = self.form.as_mut() {
            flow.step = Step::Choose;
            flow.copied_it = false;
            flow.error = None;
            let email = flow.email.clone();
            email.update(cx, |s, cx| s.set_value("", window, cx));
            email.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// Builds the IMAP request from the password form.
    fn imap_request(
        flow: &AddAccountFlow,
        cx: &gpui_kit::App,
    ) -> Result<(AddAccountRequest, SmtpSettings), String> {
        let email = EmailAddress::parse(&flow.typed_email(cx)).map_err(|e| e.to_string())?;
        let password = flow.password.read(cx).value().to_string();
        if password.is_empty() {
            return Err(if flow.provider == Provider::ICloud {
                "Enter an app-specific password from appleid.apple.com.".to_owned()
            } else {
                "Enter your password, or an app password if your provider uses them.".to_owned()
            });
        }
        let port = |input: &Entity<InputState>, what: &str| {
            input
                .read(cx)
                .value()
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| format!("The {what} port must be a number, like 993."))
        };
        let host = |input: &Entity<InputState>| {
            Host::parse(input.read(cx).value().trim()).map_err(|e| e.to_string())
        };
        let settings = ImapSettings {
            host: host(&flow.imap_host)?,
            port: port(&flow.imap_port, "incoming")?,
            username: email.as_str().to_owned(),
            auth: AuthMethod::Password,
        };
        let smtp = SmtpSettings {
            host: host(&flow.smtp_host)?,
            port: port(&flow.smtp_port, "outgoing")?,
            auth: AuthMethod::Password,
        };
        Ok((
            AddAccountRequest {
                name: default_name(&email, flow.provider),
                email,
                provider: flow.provider,
                settings,
                password,
            },
            smtp,
        ))
    }

    /// Checks the password sign-in in the background.
    pub fn submit_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(flow) = self.form.as_mut() else {
            return;
        };
        if flow.busy || flow.step != Step::Imap {
            return;
        }
        let (request, smtp) = match Self::imap_request(flow, cx) {
            Ok(parts) => parts,
            Err(message) => {
                flow.error = Some(message);
                cx.notify();
                return;
            }
        };
        let Some(service) = self.sync.clone() else {
            flow.error = Some("Sift can't save accounts in this session.".to_owned());
            cx.notify();
            return;
        };
        flow.busy = true;
        flow.error = None;
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { service.add_account(request) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, window, cx| {
                if let Some(flow) = app.form.as_mut() {
                    flow.busy = false;
                }
                match result {
                    Ok(account) => {
                        if let Err(error) = app.store.save_smtp(account.id, &smtp) {
                            app.error =
                                Some(format!("Sift couldn't save the outgoing server: {error}"));
                        }
                        app.signed_in(account, window, cx);
                    }
                    Err(error) => {
                        if let Some(flow) = app.form.as_mut() {
                            flow.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Signed in: hold the account back from sync and show Set up.
    fn signed_in(&mut self, account: Account, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.store.set_ready(account.id, false) {
            self.error = Some(format!("Sift couldn't set up the account: {error}"));
        }
        let Some(flow) = self.form.as_mut() else {
            return;
        };
        let name = flow.name.clone();
        let account_name = account.name.clone();
        name.update(cx, |s, cx| s.set_value(account_name, window, cx));
        flow.settings.colour = account.colour;
        if matches!(account.provider, Provider::ICloud | Provider::Imap) {
            flow.settings.calendar = false;
            flow.settings.contacts = false;
        }
        flow.account = Some(account);
        flow.step = Step::Setup;
        flow.error = None;
        self.reload();
        cx.notify();
    }

    /// Changes one Set up choice.
    pub(crate) fn setup_change(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut AccountSettings),
    ) {
        if let Some(flow) = self.form.as_mut() {
            change(&mut flow.settings);
            cx.notify();
        }
    }

    /// Picks the colour in Set up.
    pub fn setup_colour(&mut self, colour: u8, cx: &mut Context<Self>) {
        self.setup_change(cx, |s| s.colour = colour);
    }

    /// Picks the range in Set up.
    pub fn setup_range(&mut self, range: SyncRange, cx: &mut Context<Self>) {
        self.setup_change(cx, |s| s.range = range);
    }

    /// Picks the notification level in Set up.
    pub fn setup_notify(&mut self, notify: NotifyLevel, cx: &mut Context<Self>) {
        self.setup_change(cx, |s| s.notify = notify);
    }

    /// Saves Set up and starts the first sync.
    pub fn finish_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(flow) = self.form.as_mut() else {
            return;
        };
        let Some(account) = flow.account.clone() else {
            return;
        };
        let name = flow.name.read(cx).value().trim().to_owned();
        let settings = flow.settings.clone();
        let saved = (|| {
            if !name.is_empty() {
                self.store.rename_account(account.id, &name)?;
            }
            self.store.save_account_settings(account.id, &settings)?;
            self.store.set_ready(account.id, true)
        })();
        if let Err(error) = saved {
            if let Some(flow) = self.form.as_mut() {
                flow.error = Some(format!("Sift couldn't save those choices: {error}"));
            }
            cx.notify();
            return;
        }
        if let Some(flow) = self.form.as_mut() {
            flow.step = Step::Sync;
            flow.synced = 0;
            flow.sync_result = None;
        }
        self.reload();
        cx.notify();
        let Some(service) = self.sync.clone() else {
            return;
        };
        let id = account.id;
        let task = cx
            .background_executor()
            .spawn(async move { service.sync_account(id, Utc::now()) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                if let Some(flow) = app.form.as_mut() {
                    flow.sync_result = Some(match result {
                        Ok(summary) if summary.failures.is_empty() => Ok(()),
                        Ok(summary) => Err(summary.status_line()),
                        Err(error) => Err(error.to_string()),
                    });
                    flow.synced = app.store.message_count(id).unwrap_or(flow.synced);
                }
                app.reload();
                cx.notify();
            })
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_EVERY).await;
                let running = this.update(cx, |app, cx| {
                    let count = app.store.message_count(id).unwrap_or_default();
                    match app.form.as_mut() {
                        Some(flow) if flow.step == Step::Sync && flow.sync_result.is_none() => {
                            flow.synced = count;
                            cx.notify();
                            true
                        }
                        _ => false,
                    }
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Starts again at step 1 to add another account.
    pub fn add_another_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        self.open_add_account(window, cx);
    }

    /// Closes the flow and shows the inbox, with a note while mail arrives.
    pub fn go_to_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .form
            .as_ref()
            .and_then(|f| f.account.as_ref())
            .map(|a| a.name.clone());
        let still_syncing = self.form.as_ref().is_some_and(|f| f.sync_result.is_none());
        self.form = None;
        self.settings = None;
        if let (Some(name), true) = (name, still_syncing) {
            self.toast = Some(Toast {
                text: format!("Fetching {name} in the background…"),
                undo: None,
            });
        }
        self.reload();
        self.focus.focus(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_explains_itself() {
        let d = |s: &str| detect(s).map(|d| (d.provider, d.text));
        assert_eq!(d("nope"), None);
        assert_eq!(d("you@gmail.com").unwrap().0, Provider::Google);
        assert!(d("you@hotmail.com").unwrap().1.starts_with("Outlook.com"));
        assert!(d("you@icloud.com").unwrap().1.contains("app-specific"));
        assert!(
            d("you@fastmail.com")
                .unwrap()
                .1
                .starts_with("fastmail.com supports IMAP")
        );
        let work = d(" you@company.com ").unwrap();
        assert_eq!(work.0, Provider::Microsoft365);
        assert!(
            work.1
                .starts_with("company.com is probably on Microsoft 365")
        );
    }

    #[test]
    fn copy_for_every_provider_and_range() {
        for p in Provider::ALL {
            let _ = (short_name(p), provider_blurb(p), scopes(p));
        }
        assert_eq!(short_name(Provider::Microsoft365), "Microsoft");
        assert_eq!(
            scopes(Provider::Google)[0].0,
            "Read, organise and send mail"
        );
        for r in SyncRange::ALL {
            assert!(!range_help(r).is_empty());
        }
        assert!(it_request("company.com").contains("company.com Microsoft 365"));
        let steps = [
            Step::Choose,
            Step::OAuth,
            Step::Waiting,
            Step::Blocked,
            Step::Imap,
            Step::Setup,
            Step::Sync,
        ];
        assert_eq!(steps.map(Step::number), [1, 2, 2, 2, 2, 3, 4]);
    }
}
