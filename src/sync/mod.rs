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
use crate::domain::{AccountId, EmailAddress, ImapSettings, Provider, SyncCursor};
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
    let accounts = store.imap_accounts()?;
    Ok(accounts
        .into_iter()
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
    let password = secrets
        .get(&password_key(&account.email))?
        .ok_or(SyncError::NoPassword)?;
    let mut session = connect(settings, &password)?;
    sync_inbox(&mut session, store, account, cursor, now)
}

/// Forgets an account: its local mail, settings and saved password.
pub fn remove_account(
    store: &MailStore,
    secrets: &dyn Secrets,
    account: &Account,
) -> Result<(), SyncError> {
    store.remove_account(account.id)?;
    secrets.delete(&password_key(&account.email))?;
    Ok(())
}
