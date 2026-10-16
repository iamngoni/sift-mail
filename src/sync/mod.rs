//! Getting mail from providers into the store.
//!
//! [`add_imap_account`] proves the credentials work before saving anything,
//! then keeps the password in the OS credential store. [`sync_all`] runs one
//! pass over every IMAP account; the UI calls it on a background thread with
//! its own store connection.

pub mod imap;
pub mod parse;
pub mod secrets;
pub mod send;
pub mod service;
pub mod smtp;

use chrono::{DateTime, Utc};

use self::imap::{MailSession, SyncError, SyncReport, password_key, sync_inbox};
use self::secrets::Secrets;
use crate::domain::{AccountId, AuthMethod, EmailAddress, ImapSettings, Provider, SyncCursor};
use crate::oauth::{SignedIn, Tokens};
use crate::store::{Account, MailStore};

/// What the user typed into the Add account dialog, already parsed.
#[derive(Debug, Clone)]
pub struct AddAccountRequest {
    /// Name in the sidebar.
    pub name: String,
    /// The account address.
    pub email: EmailAddress,
    /// Detected provider.
    pub provider: Provider,
    /// Server settings.
    pub settings: ImapSettings,
    /// The password; dropped as soon as it is saved.
    pub password: String,
}

/// One sync result per account.
pub type AccountResults = Vec<(AccountId, Result<SyncReport, SyncError>)>;

/// Checks the sign-in, then saves the account, its server settings and its
/// password. Nothing is saved if the server rejects the sign-in.
pub fn add_imap_account<S: MailSession>(
    store: &MailStore,
    secrets: &dyn Secrets,
    request: &AddAccountRequest,
    connect: impl FnOnce(&ImapSettings, &str) -> Result<S, SyncError>,
) -> Result<Account, SyncError> {
    let mut session = connect(&request.settings, &request.password)?;
    session.examine_inbox()?;
    let account = store.add_account(&request.name, &request.email, request.provider)?;
    store.save_imap(account.id, &request.settings)?;
    secrets.set(&password_key(&request.email), &request.password)?;
    Ok(account)
}

/// Syncs every IMAP account once. One account failing doesn't stop the others.
pub fn sync_all<S: MailSession>(
    store: &MailStore,
    secrets: &dyn Secrets,
    connect: impl Fn(&ImapSettings, &str) -> Result<S, SyncError>,
    now: DateTime<Utc>,
) -> Result<AccountResults, SyncError> {
    sync_some(store, secrets, connect, now, None)
}

/// Syncs one account, or every account when `only` is `None`.
pub fn sync_some<S: MailSession>(
    store: &MailStore,
    secrets: &dyn Secrets,
    connect: impl Fn(&ImapSettings, &str) -> Result<S, SyncError>,
    now: DateTime<Utc>,
    only: Option<crate::domain::AccountId>,
) -> Result<AccountResults, SyncError> {
    let accounts = store.imap_accounts()?;
    Ok(accounts
        .into_iter()
        .filter(|(account, _, _)| only.is_none_or(|id| id == account.id))
        .map(|(account, settings, cursor)| {
            let result = sync_one(store, secrets, &connect, &account, &settings, cursor, now);
            (account.id, result)
        })
        .collect())
}

fn sync_one<S: MailSession>(
    store: &MailStore,
    secrets: &dyn Secrets,
    connect: &impl Fn(&ImapSettings, &str) -> Result<S, SyncError>,
    account: &Account,
    settings: &ImapSettings,
    cursor: SyncCursor,
    now: DateTime<Utc>,
) -> Result<SyncReport, SyncError> {
    let secret = secret_for(secrets, account, settings, now)?;
    let mut session = connect(settings, &secret)?;
    sync_inbox(&mut session, store, account, cursor, now)
}

/// The key under which an OAuth account's tokens are saved.
pub fn token_key(email: &EmailAddress) -> String {
    format!("oauth:{email}")
}

/// What to present to the mail servers for `account`: its password, or an
/// access token refreshed first if it is about to expire. Refreshed tokens
/// are saved before use so a crash can't lose a rotated refresh token.
pub fn secret_for(
    secrets: &dyn Secrets,
    account: &Account,
    settings: &ImapSettings,
    now: DateTime<Utc>,
) -> Result<String, SyncError> {
    match settings.auth {
        AuthMethod::Password => secrets
            .get(&password_key(&account.email))?
            .ok_or(SyncError::NoPassword),
        AuthMethod::OAuth => {
            let key = token_key(&account.email);
            let saved = secrets.get(&key)?.ok_or(SyncError::NoPassword)?;
            let tokens: Tokens = serde_json::from_str(&saved).map_err(|_| {
                SyncError::Protocol("saved sign-in is unreadable; sign in again".into())
            })?;
            if !tokens.needs_refresh(now) {
                return Ok(tokens.access_token);
            }
            let fresh = secrets.refresh_tokens(account.provider, &tokens)?;
            secrets.set(&key, &save_tokens(&fresh)?)?;
            Ok(fresh.access_token)
        }
    }
}

fn save_tokens(tokens: &Tokens) -> Result<String, SyncError> {
    serde_json::to_string(tokens).map_err(|e| SyncError::Protocol(e.to_string()))
}

/// Saves an account that signed in through the browser: checks the token
/// against the IMAP server, then stores the account and its tokens.
pub fn add_oauth_account<S: MailSession>(
    store: &MailStore,
    secrets: &dyn Secrets,
    provider: Provider,
    signed_in: &SignedIn,
    connect: impl FnOnce(&ImapSettings, &str) -> Result<S, SyncError>,
) -> Result<Account, SyncError> {
    let settings = ImapSettings {
        auth: AuthMethod::OAuth,
        ..ImapSettings::for_address(&signed_in.email, provider)
    };
    let mut session = connect(&settings, &signed_in.tokens.access_token)?;
    session.examine_inbox()?;
    let name = match provider {
        Provider::Google => "Gmail",
        _ => "Work",
    };
    let account = store.add_account(name, &signed_in.email, provider)?;
    store.save_imap(account.id, &settings)?;
    secrets.set(
        &token_key(&signed_in.email),
        &save_tokens(&signed_in.tokens)?,
    )?;
    Ok(account)
}

/// Forgets an account: its local mail, settings and saved password.
pub fn remove_account(
    store: &MailStore,
    secrets: &dyn Secrets,
    account: &Account,
) -> Result<(), SyncError> {
    store.remove_account(account.id)?;
    secrets.delete(&password_key(&account.email))?;
    secrets.delete(&token_key(&account.email))?;
    Ok(())
}

#[cfg(test)]
mod tests;
