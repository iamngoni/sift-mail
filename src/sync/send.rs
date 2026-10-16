//! Sending and the small writes Sift makes to a server.
//!
//! Order matters when sending: SMTP first, because once that succeeds the
//! mail is out. Saving to the server's Sent mailbox is best effort and never
//! turns a sent message into an error. The local copy keeps the thread whole,
//! and replying marks the thread done, as the design specifies.

use chrono::{DateTime, Utc};

use super::imap::{MailSession, SyncError, password_key};
use super::secrets::Secrets;
use super::smtp::{From, Sender, build};
use crate::domain::{
    AccountId, Contact, Draft, ImapSettings, IncomingMessage, Signals, ThreadRef, TriageState,
};
use crate::store::MailStore;

/// What to send, and from which account.
#[derive(Debug, Clone)]
pub struct SendRequest {
    /// The account to send from.
    pub account: AccountId,
    /// The message.
    pub draft: Draft,
    /// The thread being answered, so the copy joins it and it is marked done.
    pub thread: Option<ThreadRef>,
}

/// What happened after a successful send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendOutcome {
    /// A copy was saved to the server's Sent mailbox.
    pub saved_to_sent: bool,
}

/// Sends `request`, saves copies, and marks an answered thread done.
pub fn send_message<S: MailSession>(
    store: &MailStore,
    secrets: &dyn Secrets,
    sender: &Sender,
    connect: impl FnOnce(&ImapSettings, &str) -> Result<S, SyncError>,
    request: &SendRequest,
    now: DateTime<Utc>,
    unique: u64,
) -> Result<SendOutcome, SyncError> {
    let (account, imap, _) = store
        .imap_account(request.account)?
        .ok_or_else(|| SyncError::Protocol("this account can't send mail yet".to_owned()))?;
    let smtp = store
        .smtp_settings(request.account)?
        .ok_or_else(|| SyncError::Protocol("this account can't send mail yet".to_owned()))?;
    let password = secrets
        .get(&password_key(&account.email))?
        .ok_or(SyncError::NoPassword)?;

    let built = build(
        &From {
            name: &account.name,
            email: &account.email,
        },
        &request.draft,
        now,
        unique,
    )?;
    sender(&smtp, &imap.username, &password, &built.message)?;

    // The mail is out. Everything below is bookkeeping and must not fail the send.
    let saved_to_sent = connect(&imap, &password)
        .and_then(|mut session| session.append_sent(&built.message.formatted()))
        .unwrap_or(false);

    let thread_key = request
        .thread
        .as_ref()
        .map_or_else(|| built.message_id.clone(), |t| t.key.clone());
    store.record_sent(&IncomingMessage {
        account: account.id,
        remote_id: format!("sent:{}", built.message_id),
        thread_key: thread_key.clone(),
        from: Contact::new(Some(&account.name), account.email.clone()),
        subject: request.draft.subject().to_owned(),
        body_text: request.draft.body().to_owned(),
        received_at: now,
        is_read: true,
        has_attachments: false,
        signals: Signals::default(),
        message_id: Some(built.message_id),
        references: request
            .draft
            .reply()
            .map(|r| r.references.clone())
            .unwrap_or_default(),
    })?;
    if let Some(thread) = &request.thread {
        store.set_state(std::slice::from_ref(thread), TriageState::Done)?;
    }
    Ok(SendOutcome { saved_to_sent })
}

/// Writes a thread's read state back to the server. Only messages fetched
/// under the server's current UIDVALIDITY are touched; local sent copies and
/// stale UIDs are skipped rather than guessed at.
pub fn write_read<S: MailSession>(
    store: &MailStore,
    secrets: &dyn Secrets,
    connect: impl FnOnce(&ImapSettings, &str) -> Result<S, SyncError>,
    thread: &ThreadRef,
    read: bool,
) -> Result<usize, SyncError> {
    let Some((account, imap, cursor)) = store.imap_account(thread.account())? else {
        return Ok(0);
    };
    let Some(validity) = cursor.uid_validity else {
        return Ok(0);
    };
    let uids: Vec<u32> = store
        .thread_messages(thread)?
        .iter()
        .filter_map(|m| uid_for(&m.remote_id, validity))
        .collect();
    if uids.is_empty() {
        return Ok(0);
    }
    let password = secrets
        .get(&password_key(&account.email))?
        .ok_or(SyncError::NoPassword)?;
    connect(&imap, &password)?.set_seen(&uids, read)?;
    Ok(uids.len())
}

/// The UID in a remote id like `"7:42"`, if it was fetched under `validity`.
pub fn uid_for(remote_id: &str, validity: u32) -> Option<u32> {
    let (v, uid) = remote_id.split_once(':')?;
    (v.parse::<u32>().ok()? == validity).then(|| uid.parse().ok())?
}

#[cfg(test)]
mod tests;
