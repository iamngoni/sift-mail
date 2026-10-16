//! The Settings screen: Accounts (name and colour, send-as addresses,
//! signature, what and how much to sync, AI, notifications, connection) plus
//! General, AI, Notifications, Appearance, Keyboard and Privacy pages.
//!
//! Every change is saved as it happens; there is no Save button.
//! Rendering lives in [`super::settings_view`].

use chrono::Utc;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::{AppContext as _, Context, Entity, Window};

use crate::domain::{AccountId, EmailAddress, Provider};
use crate::store::{AccountSettings, NotifyLevel, Pref, SyncRange};

use super::app::SiftApp;

/// A page in the Settings sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    /// Default From address and startup.
    General,
    /// Connected accounts.
    Accounts,
    /// Which assistant Sift uses.
    Ai,
    /// When Sift may interrupt.
    Notifications,
    /// Theme and density.
    Appearance,
    /// Every shortcut.
    Keyboard,
    /// What stays on this computer.
    Privacy,
}

impl SettingsPage {
    /// Pages in sidebar order.
    pub const ALL: [SettingsPage; 7] = [
        Self::General,
        Self::Accounts,
        Self::Ai,
        Self::Notifications,
        Self::Appearance,
        Self::Keyboard,
        Self::Privacy,
    ];

    /// Sidebar label and page title.
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Accounts => "Accounts",
            Self::Ai => "AI",
            Self::Notifications => "Notifications",
            Self::Appearance => "Appearance",
            Self::Keyboard => "Keyboard",
            Self::Privacy => "Privacy",
        }
    }
}

/// Text boxes for the expanded account.
pub(crate) struct AccountEditor {
    pub(crate) account: AccountId,
    pub(crate) name: Entity<InputState>,
    pub(crate) alias: Entity<InputState>,
    pub(crate) signature: Entity<TextareaState>,
    pub(crate) alias_error: Option<String>,
}

/// The open Settings screen.
pub(crate) struct SettingsState {
    pub(crate) page: SettingsPage,
    /// The expanded account card.
    pub(crate) editor: Option<AccountEditor>,
    /// Account waiting for the Remove confirmation.
    pub(crate) confirm_remove: Option<AccountId>,
    /// The Send new mail from menu is open.
    pub(crate) from_menu: bool,
    /// Accounts syncing from this screen.
    pub(crate) busy: Vec<AccountId>,
    /// The last thing that happened, shown as a toast.
    pub(crate) notice: Option<String>,
}

/// Display name of a provider, as the Accounts board writes it.
pub fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Microsoft365 => "Microsoft 365",
        Provider::Google => "Google",
        Provider::ICloud => "iCloud",
        Provider::Imap => "IMAP",
    }
}

impl SiftApp {
    /// Whether Settings is showing.
    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }

    /// The Settings page on screen.
    pub fn settings_page(&self) -> Option<SettingsPage> {
        self.settings.as_ref().map(|s| s.page)
    }

    /// The expanded account in Settings.
    pub fn settings_account(&self) -> Option<AccountId> {
        self.settings
            .as_ref()
            .and_then(|s| s.editor.as_ref())
            .map(|e| e.account)
    }

    /// Opens Settings on a page, expanding the first account on Accounts.
    pub fn open_settings(
        &mut self,
        page: SettingsPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = None;
        self.settings = Some(SettingsState {
            page,
            editor: None,
            confirm_remove: None,
            from_menu: false,
            busy: Vec::new(),
            notice: None,
        });
        self.reload();
        if page == SettingsPage::Accounts
            && let Some(first) = self.accounts.first().map(|a| a.id)
        {
            self.toggle_account(first, window, cx);
        }
        cx.notify();
    }

    /// Back to mail.
    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = None;
        self.reload();
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Shows another Settings page.
    pub fn settings_go(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.settings {
            s.page = page;
            s.from_menu = false;
            cx.notify();
        }
    }

    /// Expands an account card, or collapses it if it's open.
    pub fn toggle_account(
        &mut self,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(settings) = &mut self.settings else {
            return;
        };
        if settings
            .editor
            .as_ref()
            .is_some_and(|e| e.account == account)
        {
            settings.editor = None;
            cx.notify();
            return;
        }
        let Some(current) = self.accounts.iter().find(|a| a.id == account) else {
            return;
        };
        let signature_text = self
            .store
            .account_settings(account)
            .map(|s| s.signature)
            .unwrap_or_default();
        let name_text = current.name.clone();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name_text));
        let alias = cx.new(|cx| InputState::new(window, cx).placeholder("team@company.com"));
        let signature = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Your name\nRole · Company")
                .default_value(signature_text)
                .auto_grow(3, 8)
        });
        cx.subscribe_in(
            &name,
            window,
            move |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change | InputEvent::Blur) {
                    let value = input.read(cx).value().to_string();
                    if !value.trim().is_empty() {
                        if let Err(error) = this.store.rename_account(account, &value) {
                            this.error =
                                Some(format!("Sift couldn't rename that account: {error}"));
                        }
                        this.reload();
                        cx.notify();
                    }
                }
            },
        )
        .detach();
        cx.subscribe_in(&alias, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.add_alias(window, cx);
            }
        })
        .detach();
        cx.subscribe_in(
            &signature,
            window,
            move |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change | InputEvent::Blur) {
                    let text = input.read(cx).value().to_string();
                    this.update_account(account, cx, |s| s.signature = text);
                }
            },
        )
        .detach();
        if let Some(settings) = &mut self.settings {
            settings.editor = Some(AccountEditor {
                account,
                name,
                alias,
                signature,
                alias_error: None,
            });
            settings.from_menu = false;
        }
        cx.notify();
    }

    /// Changes one account setting and saves it.
    pub(crate) fn update_account(
        &mut self,
        account: AccountId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut AccountSettings),
    ) {
        let result = self.store.account_settings(account).and_then(|mut s| {
            change(&mut s);
            self.store.save_account_settings(account, &s)
        });
        if let Err(error) = result {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
        self.reload();
        cx.notify();
    }

    /// Picks an account's colour.
    pub fn set_account_colour(&mut self, account: AccountId, colour: u8, cx: &mut Context<Self>) {
        self.update_account(account, cx, |s| s.colour = colour);
    }

    /// Sets how much mail an account keeps.
    pub fn set_account_range(
        &mut self,
        account: AccountId,
        range: SyncRange,
        cx: &mut Context<Self>,
    ) {
        self.update_account(account, cx, |s| s.range = range);
    }

    /// Sets when an account may notify.
    pub fn set_account_notify(
        &mut self,
        account: AccountId,
        notify: NotifyLevel,
        cx: &mut Context<Self>,
    ) {
        self.update_account(account, cx, |s| s.notify = notify);
    }

    /// Flips AI features for an account.
    pub fn toggle_account_ai(&mut self, account: AccountId, cx: &mut Context<Self>) {
        self.update_account(account, cx, |s| s.ai = !s.ai);
    }

    /// Flips whether an account shows in All inboxes.
    pub fn toggle_account_in_all(&mut self, account: AccountId, cx: &mut Context<Self>) {
        self.update_account(account, cx, |s| s.in_all = !s.in_all);
    }

    /// Flips calendar (`"calendar"`) or contacts (`"contacts"`) sync.
    pub fn toggle_account_sync(&mut self, account: AccountId, what: &str, cx: &mut Context<Self>) {
        let calendar = what == "calendar";
        self.update_account(account, cx, |s| {
            if calendar {
                s.calendar = !s.calendar;
            } else {
                s.contacts = !s.contacts;
            }
        });
    }

    /// Moves an account up or down the sidebar order.
    pub fn move_account(&mut self, account: AccountId, delta: i64, cx: &mut Context<Self>) {
        if let Err(error) = self.store.move_account(account, delta) {
            self.error = Some(format!("Sift couldn't reorder accounts: {error}"));
        }
        self.reload();
        cx.notify();
    }

    /// Adds the typed send-as address to the expanded account.
    pub fn add_alias(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.settings.as_mut().and_then(|s| s.editor.as_mut()) else {
            return;
        };
        let text = editor.alias.read(cx).value().to_string();
        let own = self
            .accounts
            .iter()
            .find(|a| a.id == editor.account)
            .map(|a| a.email.clone());
        match EmailAddress::parse(text.trim()) {
            Ok(address) if Some(&address) == own.as_ref() => {
                editor.alias_error = Some("That's already this account's address.".into());
            }
            Ok(address) => {
                if let Err(error) = self.store.add_alias(editor.account, &address) {
                    editor.alias_error = Some(format!("Sift couldn't save that address: {error}"));
                } else {
                    editor.alias_error = None;
                    let input = editor.alias.clone();
                    input.update(cx, |state, cx| state.set_value("", window, cx));
                }
            }
            Err(_) => {
                editor.alias_error = Some("Enter a full address, like team@company.com.".into())
            }
        }
        cx.notify();
    }

    /// Removes a send-as address.
    pub fn remove_alias(
        &mut self,
        account: AccountId,
        address: EmailAddress,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.store.remove_alias(account, &address) {
            self.error = Some(format!("Sift couldn't remove that address: {error}"));
        }
        cx.notify();
    }

    /// Opens or closes the Send new mail from menu.
    pub fn toggle_from_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.settings {
            s.from_menu = !s.from_menu;
            cx.notify();
        }
    }

    /// The account new mail is sent from: the saved choice, else the first.
    pub fn default_from(&self) -> Option<AccountId> {
        let saved = self
            .store
            .preference(Pref::DefaultFrom)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok());
        self.accounts
            .iter()
            .find(|a| Some(a.id.0) == saved)
            .or_else(|| self.accounts.first())
            .map(|a| a.id)
    }

    /// Makes an account the default for new mail.
    pub fn set_default_from(&mut self, account: AccountId, cx: &mut Context<Self>) {
        if let Err(error) = self
            .store
            .set_preference(Pref::DefaultFrom, &account.0.to_string())
        {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
        if let Some(s) = &mut self.settings {
            s.from_menu = false;
        }
        cx.notify();
    }

    /// Syncs one account from its card.
    pub fn sync_account_now(&mut self, account: AccountId, cx: &mut Context<Self>) {
        let Some(service) = self.sync.clone() else {
            self.notice("Sift can't sync in this session.".into(), cx);
            return;
        };
        let Some(settings) = &mut self.settings else {
            return;
        };
        if settings.busy.contains(&account) {
            return;
        }
        settings.busy.push(account);
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { service.sync_account(account, Utc::now()) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                if let Some(s) = &mut app.settings {
                    s.busy.retain(|a| *a != account);
                }
                let name = app
                    .accounts
                    .iter()
                    .find(|a| a.id == account)
                    .map_or_else(String::new, |a| a.name.clone());
                let text = match result {
                    Ok(summary) if summary.failures.is_empty() => format!("{name} synced"),
                    Ok(summary) => summary.status_line(),
                    Err(error) => format!("Sync failed: {error}"),
                };
                app.reload();
                app.notice(text, cx);
            })
        })
        .detach();
    }

    /// Signs an account in again: the Add account flow, prefilled.
    pub fn sign_in_again(
        &mut self,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(email) = self
            .accounts
            .iter()
            .find(|a| a.id == account)
            .map(|a| a.email.to_string())
        else {
            return;
        };
        self.open_add_account(window, cx);
        if let Some(form) = &self.form {
            let input = form.email.clone();
            input.update(cx, |state, cx| state.set_value(email, window, cx));
        }
    }

    /// Asks before removing an account.
    pub fn ask_remove_account(&mut self, account: AccountId, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.settings {
            s.confirm_remove = Some(account);
            cx.notify();
        }
    }

    /// Closes the Remove confirmation.
    pub fn cancel_remove_account(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.settings {
            s.confirm_remove = None;
            cx.notify();
        }
    }

    /// Removes the account waiting for confirmation from this computer.
    pub fn confirm_remove_account(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.settings.as_mut().and_then(|s| s.confirm_remove.take()) else {
            return;
        };
        let name = self
            .accounts
            .iter()
            .find(|a| a.id == account)
            .map_or_else(String::new, |a| a.name.clone());
        let result = match &self.sync {
            Some(service) => service.remove(account).map_err(|e| e.to_string()),
            None => self
                .store
                .remove_account(account)
                .map_err(|e| e.to_string()),
        };
        match result {
            Ok(()) => {
                if let Some(s) = &mut self.settings
                    && s.editor.as_ref().is_some_and(|e| e.account == account)
                {
                    s.editor = None;
                }
                self.reload();
                self.notice(format!("{name} removed from this computer"), cx);
            }
            Err(error) => self.error = Some(format!("Sift couldn't remove {name}: {error}")),
        }
        cx.notify();
    }

    /// Shows a short message at the bottom of Settings.
    pub(crate) fn notice(&mut self, text: String, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.settings {
            s.notice = Some(text);
        }
        cx.notify();
    }

    /// Switches row density and remembers it.
    pub fn set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
        self.compact = compact;
        if let Err(error) = self.store.set_preference(
            Pref::Density,
            if compact { "compact" } else { "comfortable" },
        ) {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_and_providers_have_names() {
        let labels: Vec<_> = SettingsPage::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(
            labels,
            [
                "General",
                "Accounts",
                "AI",
                "Notifications",
                "Appearance",
                "Keyboard",
                "Privacy"
            ]
        );
        assert_eq!(provider_name(Provider::Microsoft365), "Microsoft 365");
        assert_eq!(provider_name(Provider::Google), "Google");
        assert_eq!(provider_name(Provider::ICloud), "iCloud");
        assert_eq!(provider_name(Provider::Imap), "IMAP");
    }
}
