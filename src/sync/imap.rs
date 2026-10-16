//! IMAP sync: connect over TLS, then copy new INBOX mail into the store.
//!
//! The engine talks to a [`MailSession`], so the sync rules (UIDVALIDITY
//! resets, incremental UID ranges, batching, skipping unparseable mail) are
//! tested without a network. [`ImapSession`] adapts the real `imap` crate.
//! Sync is one-way for now: the server is never modified.

use std::io::{Read, Write};

use chrono::{DateTime, Duration, Utc};

use super::parse::{Envelope, parse};
use crate::domain::{AuthMethod, EmailAddress, ImapSettings, SyncCursor};
use crate::store::{Account, MailStore, StoreError};

/// How far back the first sync of an account reaches.
pub const INITIAL_DAYS: i64 = 90;
/// Messages fetched per round trip.
pub const BATCH: usize = 50;

/// A sync failure, phrased for the user. Never contains a password.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// DNS, TCP or TLS failed.
    #[error("couldn't reach {host}: {detail}")]
    Connect {
        /// Server we tried.
        host: String,
        /// What went wrong.
        detail: String,
    },
    /// The server rejected the user name or password.
    #[error(
        "the server rejected the sign-in. Check the password, or use an app password if your provider requires one"
    )]
    Auth,
    /// The server said something we didn't expect.
    #[error("the server returned an error: {0}")]
    Protocol(String),
    /// No password is saved for the account.
    #[error("no password is saved for this account")]
    NoPassword,
    /// The credential store failed.
    #[error(transparent)]
    Secrets(#[from] super::secrets::SecretError),
    /// Browser sign-in or token refresh failed.
    #[error(transparent)]
    OAuth(#[from] crate::oauth::OAuthError),
    /// Writing to the local store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// What the INBOX looks like on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxInfo {
    /// UIDVALIDITY, if the server reported one.
    pub uid_validity: Option<u32>,
}

/// One fetched message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMessage {
    /// Server UID.
    pub uid: u32,
    /// Has the `\Seen` flag.
    pub seen: bool,
    /// INTERNALDATE.
    pub internal_date: Option<DateTime<Utc>>,
    /// The full RFC 5322 message.
    pub body: Vec<u8>,
}

/// The IMAP operations sync needs.
pub trait MailSession {
    /// SELECTs (read-only EXAMINE) the INBOX.
    fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError>;
    /// `UID SEARCH <query>`.
    fn uid_search(&mut self, query: &str) -> Result<Vec<u32>, SyncError>;
    /// Fetches full messages by UID, without setting `\Seen`.
    fn uid_fetch(&mut self, uids: &[u32]) -> Result<Vec<RawMessage>, SyncError>;
    /// Adds or removes `\Seen` on INBOX messages (SELECTs the INBOX read-write).
    fn set_seen(&mut self, uids: &[u32], seen: bool) -> Result<(), SyncError>;
    /// Appends a sent message to the mailbox marked `\Sent`. Returns `false`
    /// when the server has no Sent mailbox, so nothing was saved.
    fn append_sent(&mut self, raw: &[u8]) -> Result<bool, SyncError>;
}

/// What one sync run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncReport {
    /// Messages written to the store.
    pub stored: usize,
    /// Messages that couldn't be parsed and were skipped.
    pub skipped: usize,
    /// The mailbox was re-fetched because UIDVALIDITY changed.
    pub reset: bool,
}

/// Brings the store up to date with the INBOX of `account`.
pub fn sync_inbox(
    session: &mut impl MailSession,
    store: &MailStore,
    account: &Account,
    cursor: SyncCursor,
    now: DateTime<Utc>,
) -> Result<SyncReport, SyncError> {
    let info = session.examine_inbox()?;
    let mut report = SyncReport::default();
    let mut cursor = cursor;
    if cursor.uid_validity.is_some() && cursor.uid_validity != info.uid_validity {
        report.reset = true;
        cursor.last_uid = 0;
    }
    cursor.uid_validity = info.uid_validity;

    let query = if cursor.last_uid == 0 {
        format!("SINCE {}", imap_date(now - Duration::days(INITIAL_DAYS)))
    } else {
        format!("UID {}:*", cursor.last_uid + 1)
    };
    let mut uids: Vec<u32> = session
        .uid_search(&query)?
        .into_iter()
        // `n:*` always matches the highest UID, even when it is old.
        .filter(|uid| *uid > cursor.last_uid)
        .collect();
    uids.sort_unstable();
    uids.dedup();

    let validity = info.uid_validity.unwrap_or_default();
    for batch in uids.chunks(BATCH) {
        for raw in session.uid_fetch(batch)? {
            let envelope = Envelope {
                account: account.id,
                remote_id: format!("{validity}:{}", raw.uid),
                me: &account.email,
                is_read: raw.seen,
                internal_date: raw.internal_date,
            };
            match parse(&raw.body, &envelope) {
                Ok(message) => {
                    store.upsert(&message)?;
                    report.stored += 1;
                }
                Err(_) => report.skipped += 1,
            }
            cursor.last_uid = cursor.last_uid.max(raw.uid);
        }
        // Save progress per batch so an interrupted first sync resumes.
        store.set_cursor(account.id, cursor)?;
    }
    store.set_cursor(account.id, cursor)?;
    Ok(report)
}

/// Formats a date the way IMAP SEARCH wants it: `16-Oct-2026`.
pub fn imap_date(at: DateTime<Utc>) -> String {
    at.format("%-d-%b-%Y").to_string()
}

/// Adapts an authenticated `imap` crate session to [`MailSession`].
pub struct ImapSession<T: Read + Write>(pub imap::Session<T>);

impl<T: Read + Write> MailSession for ImapSession<T> {
    fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError> {
        let mailbox = self.0.examine("INBOX").map_err(protocol)?;
        Ok(MailboxInfo {
            uid_validity: mailbox.uid_validity,
        })
    }

    fn uid_search(&mut self, query: &str) -> Result<Vec<u32>, SyncError> {
        Ok(self
            .0
            .uid_search(query)
            .map_err(protocol)?
            .into_iter()
            .collect())
    }

    fn uid_fetch(&mut self, uids: &[u32]) -> Result<Vec<RawMessage>, SyncError> {
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fetches = self
            .0
            .uid_fetch(set, "(UID FLAGS INTERNALDATE BODY.PEEK[])")
            .map_err(protocol)?;
        Ok(fetches
            .iter()
            .filter_map(|f| {
                Some(RawMessage {
                    uid: f.uid?,
                    seen: f
                        .flags()
                        .iter()
                        .any(|flag| matches!(flag, imap::types::Flag::Seen)),
                    internal_date: f.internal_date().map(|d| d.with_timezone(&Utc)),
                    body: f.body()?.to_vec(),
                })
            })
            .collect())
    }

    fn set_seen(&mut self, uids: &[u32], seen: bool) -> Result<(), SyncError> {
        if uids.is_empty() {
            return Ok(());
        }
        self.0.select("INBOX").map_err(protocol)?;
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let op = if seen {
            "+FLAGS.SILENT (\\Seen)"
        } else {
            "-FLAGS.SILENT (\\Seen)"
        };
        self.0.uid_store(set, op).map_err(protocol)?;
        Ok(())
    }

    fn append_sent(&mut self, raw: &[u8]) -> Result<bool, SyncError> {
        let names = self.0.list(Some(""), Some("*")).map_err(protocol)?;
        let sent = names.iter().find(|n| {
            n.attributes()
                .iter()
                .any(|a| matches!(a, imap_proto::NameAttribute::Sent))
        });
        let Some(sent) = sent.map(|n| n.name().to_owned()) else {
            return Ok(false);
        };
        self.0
            .append(&sent, raw)
            .flag(imap::types::Flag::Seen)
            .finish()
            .map_err(protocol)?;
        Ok(true)
    }
}

fn protocol(error: imap::Error) -> SyncError {
    SyncError::Protocol(error.to_string())
}

/// SASL XOAUTH2: the whole response is sent in reply to the first challenge.
struct XOAuth2(String);

impl imap::Authenticator for XOAuth2 {
    type Response = String;

    fn process(&self, _: &[u8]) -> Self::Response {
        self.0.clone()
    }
}

/// Logs in on an already-open stream with a password or an OAuth access
/// token, depending on `auth`. Used by [`connect`] and by tests.
pub fn login<T: Read + Write>(
    client: imap::Client<T>,
    username: &str,
    secret: &str,
    auth: AuthMethod,
) -> Result<ImapSession<T>, SyncError> {
    let session = match auth {
        AuthMethod::Password => client.login(username, secret),
        AuthMethod::OAuth => {
            client.authenticate("XOAUTH2", &XOAuth2(crate::oauth::xoauth2(username, secret)))
        }
    };
    session.map(ImapSession).map_err(|(error, _)| match error {
        imap::Error::No(_) | imap::Error::Bad(_) => SyncError::Auth,
        other => protocol(other),
    })
}

/// Opens a TLS connection (implicit on 993, STARTTLS otherwise) and logs in.
pub fn connect(
    settings: &ImapSettings,
    secret: &str,
) -> Result<ImapSession<imap::Connection>, SyncError> {
    let mode = if settings.implicit_tls() {
        imap::ConnectionMode::Tls
    } else {
        imap::ConnectionMode::StartTls
    };
    let client = imap::ClientBuilder::new(settings.host.as_str(), settings.port)
        .mode(mode)
        .tls_kind(imap::TlsKind::Rust)
        .connect()
        .map_err(|e| SyncError::Connect {
            host: settings.host.to_string(),
            detail: e.to_string(),
        })?;
    login(client, &settings.username, secret, settings.auth)
}

/// The secret key under which an account's password is saved.
pub fn password_key(email: &EmailAddress) -> String {
    format!("imap:{email}")
}

#[cfg(test)]
mod tests;
