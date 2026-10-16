//! The local mail store: one SQLite file per installation.
//!
//! Owns persistence of accounts, messages and triage state. It does not talk
//! to providers; sync adapters hand it [`IncomingMessage`]s. Every write that
//! touches more than one row runs in a transaction, and triage actions apply to
//! whole threads so a conversation is never half done.

mod files;
mod prefs;
mod schema;
mod search;

pub use files::{Attachment, safe_name};
pub use prefs::{
    ACCOUNT_COLOURS, AccountHealth, AccountSettings, CustomView, CustomViewId, NotifyLevel, Pref,
    SyncRange, describe_view,
};
pub use search::{SEARCH_LIMIT, fts_query};

#[cfg(any(test, feature = "demo"))]
pub mod demo;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::domain::message::preview;
use crate::domain::{
    AccountId, AuthMethod, Contact, EmailAddress, Host, ImapSettings, IncomingMessage, MessageId,
    Provider, SmtpSettings, SyncCursor, ThreadRef, TriageState, TriageView, triage,
};

/// Most threads returned by one [`MailStore::threads`] call.
pub const PAGE_SIZE: usize = 500;

/// A store failure. Messages are for logs; the UI shows its own copy.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite reported an error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The data directory could not be created.
    #[error("could not create {path}: {source}")]
    CreateDir {
        /// The directory we tried to create.
        path: PathBuf,
        /// Why it failed.
        source: std::io::Error,
    },
    /// An attachment could not be written to disk.
    #[error("could not save {path}: {source}")]
    WriteFile {
        /// The file we tried to write.
        path: PathBuf,
        /// Why it failed.
        source: std::io::Error,
    },
    /// A row held a value this version cannot read, such as an unknown view.
    #[error("stored data is not readable by this version: {0}")]
    Corrupt(String),
}

/// Shorthand for store results.
pub type StoreResult<T> = Result<T, StoreError>;

/// A connected account as the store knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Store id.
    pub id: AccountId,
    /// Name shown in the sidebar, such as "Work".
    pub name: String,
    /// The account's own address.
    pub email: EmailAddress,
    /// Where it lives.
    pub provider: Provider,
    /// Index into the avatar tints.
    pub colour: u8,
}

/// What to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThreadQuery {
    /// The tab. `All` lists every view.
    pub view: TriageView,
    /// One account, or every account when `None`.
    pub account: Option<AccountId>,
    /// Only threads with at least one unread message.
    pub unread_only: bool,
}

/// One row in the inbox list: a thread, shown by its newest message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSummary {
    /// The conversation.
    pub thread: ThreadRef,
    /// Newest message in it.
    pub latest: MessageId,
    /// Sender of the newest message.
    pub from: Contact,
    /// Subject of the newest message.
    pub subject: String,
    /// Preview of the newest message.
    pub preview: String,
    /// When the newest message arrived.
    pub received_at: DateTime<Utc>,
    /// Messages in the thread.
    pub message_count: usize,
    /// At least one message is unread.
    pub unread: bool,
    /// The newest message has attachments.
    pub has_attachments: bool,
    /// Which view the thread is in.
    pub view: TriageView,
}

/// One message inside an open thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDetail {
    /// Store id.
    pub id: MessageId,
    /// Sender.
    pub from: Contact,
    /// Subject line.
    pub subject: String,
    /// Plain-text body.
    pub body_text: String,
    /// Arrival time.
    pub received_at: DateTime<Utc>,
    /// Has attachments.
    pub has_attachments: bool,
    /// Provider id, such as `"7:42"` for IMAP UIDVALIDITY 7, UID 42.
    pub remote_id: String,
    /// `Message-ID` without brackets, if the message had one.
    pub message_id: Option<String>,
    /// `References` without brackets, oldest first.
    pub references: Vec<String>,
}

/// Inbox counts per view, for the tab badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewCounts([usize; 6]);

impl ViewCounts {
    /// Threads in `view`. `All` is the total.
    pub fn get(&self, view: TriageView) -> usize {
        let index = TriageView::ALL
            .iter()
            .position(|v| *v == view)
            .unwrap_or_default();
        self.0[index]
    }
}

/// The SQLite-backed store. Cheap to share behind an `Rc`; all methods take `&self`.
pub struct MailStore {
    conn: Connection,
    path: Option<PathBuf>,
}

impl MailStore {
    /// Where the store lives by default: the per-user local data directory.
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|dir| dir.join(crate::brand::PRODUCT_NAME).join("mail.db"))
    }

    /// Opens (creating if needed) the store at `path` and migrates it.
    pub fn open(path: &Path) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        Self::from_connection(Connection::open(path)?, Some(path.to_path_buf()))
    }

    /// A throwaway store in memory, for tests and previews.
    pub fn open_in_memory() -> StoreResult<Self> {
        Self::from_connection(Connection::open_in_memory()?, None)
    }

    fn from_connection(conn: Connection, path: Option<PathBuf>) -> StoreResult<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        schema::migrate(&conn)?;
        Ok(Self { conn, path })
    }

    /// Adds an account, or returns the existing one with the same address.
    pub fn add_account(
        &self,
        name: &str,
        email: &EmailAddress,
        provider: Provider,
    ) -> StoreResult<Account> {
        self.conn.execute(
            "INSERT INTO accounts (name, email, provider, position, colour)
             SELECT ?1, ?2, ?3, COALESCE(MAX(position), -1) + 1, COUNT(*) % 6 FROM accounts
             WHERE true
             ON CONFLICT(email) DO NOTHING",
            params![name, email.as_str(), provider_key(provider)],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM accounts WHERE email = ?1",
            [email.as_str()],
            |row| row.get(0),
        )?;
        self.account(AccountId(id))?
            .ok_or_else(|| StoreError::Corrupt(format!("account {id} vanished after insert")))
    }

    /// One account by id.
    pub fn account(&self, id: AccountId) -> StoreResult<Option<Account>> {
        self.conn
            .query_row(
                "SELECT id, name, email, provider, colour FROM accounts WHERE id = ?1",
                [id.0],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?
            .map(
                |(id, name, email, provider, colour): (i64, String, String, String, i64)| {
                    Ok(Account {
                        id: AccountId(id),
                        name,
                        colour: u8::try_from(colour.rem_euclid(6)).unwrap_or_default(),
                        email: parse_email(&email)?,
                        provider: provider_from_key(&provider)?,
                    })
                },
            )
            .transpose()
    }

    /// Every account, oldest first.
    pub fn accounts(&self) -> StoreResult<Vec<Account>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM accounts ORDER BY position, id")?;
        let ids: Vec<i64> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        ids.into_iter()
            .filter_map(|id| self.account(AccountId(id)).transpose())
            .collect()
    }

    /// Stores a message, classifying it. Re-delivering the same remote id
    /// updates read state only. A new message in a done or snoozed thread
    /// brings the whole thread back to the inbox.
    pub fn upsert(&self, message: &IncomingMessage) -> StoreResult<MessageId> {
        let tx = self.conn.unchecked_transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM messages WHERE account_id = ?1 AND remote_id = ?2",
                params![message.account.0, message.remote_id],
                |row| row.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => {
                tx.execute(
                    "UPDATE messages SET is_read = ?2 WHERE id = ?1",
                    params![id, message.is_read],
                )?;
                id
            }
            None => {
                let view = triage::classify(&message.from, message.signals);
                tx.execute(
                    "INSERT INTO messages (account_id, remote_id, thread_key, from_name, from_email,
                        subject, preview, body_text, received_at, is_read, has_attachments, view,
                        message_id, refs)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        message.account.0,
                        message.remote_id,
                        message.thread_key,
                        message.from.name(),
                        message.from.email().as_str(),
                        message.subject,
                        preview(&message.body_text),
                        message.body_text,
                        message.received_at.timestamp(),
                        message.is_read,
                        message.has_attachments,
                        view.key(),
                        message.message_id,
                        message.references.join(" "),
                    ],
                )?;
                // New mail reopens the conversation.
                tx.execute(
                    "UPDATE messages SET state = 'inbox', snoozed_until = NULL
                     WHERE account_id = ?1 AND thread_key = ?2",
                    params![message.account.0, message.thread_key],
                )?;
                tx.last_insert_rowid()
            }
        };
        tx.commit()?;
        Ok(MessageId(id))
    }

    /// Stores a message the user sent. It joins its thread without changing
    /// the thread's view or state, and is always read.
    pub fn record_sent(&self, message: &IncomingMessage) -> StoreResult<MessageId> {
        let tx = self.conn.unchecked_transaction()?;
        let existing: Option<(String, String, Option<i64>)> = tx
            .query_row(
                "SELECT view, state, snoozed_until FROM messages
                 WHERE account_id = ?1 AND thread_key = ?2 ORDER BY received_at DESC, id DESC LIMIT 1",
                params![message.account.0, message.thread_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (view, state, until) = existing
            .unwrap_or_else(|| (TriageView::Other.key().to_owned(), "done".to_owned(), None));
        tx.execute(
            "INSERT INTO messages (account_id, remote_id, thread_key, from_name, from_email,
                subject, preview, body_text, received_at, is_read, has_attachments, view,
                message_id, refs, state, snoozed_until)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, 0, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(account_id, remote_id) DO NOTHING",
            params![
                message.account.0,
                message.remote_id,
                message.thread_key,
                message.from.name(),
                message.from.email().as_str(),
                message.subject,
                preview(&message.body_text),
                message.body_text,
                message.received_at.timestamp(),
                view,
                message.message_id,
                message.references.join(" "),
                state,
                until,
            ],
        )?;
        let id = tx.query_row(
            "SELECT id FROM messages WHERE account_id = ?1 AND remote_id = ?2",
            params![message.account.0, message.remote_id],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(MessageId(id))
    }

    /// Inbox threads matching `query`, newest first, at most [`PAGE_SIZE`].
    pub fn threads(&self, query: ThreadQuery) -> StoreResult<Vec<ThreadSummary>> {
        let view = (query.view != TriageView::All).then(|| query.view.key());
        let mut stmt = self.conn.prepare_cached(
            "WITH ranked AS (
                SELECT m.*,
                    ROW_NUMBER() OVER (PARTITION BY account_id, thread_key
                                       ORDER BY received_at DESC, id DESC) AS rn,
                    COUNT(*) OVER (PARTITION BY account_id, thread_key) AS n,
                    MIN(is_read) OVER (PARTITION BY account_id, thread_key) AS all_read
                FROM messages m
                WHERE state = 'inbox'
             )
             SELECT id, account_id, thread_key, from_name, from_email, subject, preview,
                    received_at, n, all_read, has_attachments, view
             FROM ranked
             WHERE rn = 1
               AND (?1 IS NULL OR view = ?1)
               AND (account_id = ?2 OR (?2 IS NULL AND account_id IN (SELECT id FROM accounts WHERE in_all)))
               AND (?3 = 0 OR all_read = 0)
             ORDER BY received_at DESC, id DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![
                view,
                query.account.map(|a| a.0),
                query.unread_only,
                i64::try_from(PAGE_SIZE).unwrap_or(i64::MAX)
            ],
            raw_summary,
        )?;
        rows.map(|row| row.map_err(StoreError::from).and_then(RawSummary::parse))
            .collect()
    }

    /// Badge counts for every view, optionally for one account.
    pub fn counts(&self, account: Option<AccountId>) -> StoreResult<ViewCounts> {
        let mut stmt = self.conn.prepare_cached(
            "WITH latest AS (
                SELECT view, ROW_NUMBER() OVER (PARTITION BY account_id, thread_key
                                               ORDER BY received_at DESC, id DESC) AS rn
                FROM messages
                WHERE state = 'inbox'
                  AND (account_id = ?1 OR (?1 IS NULL AND account_id IN (SELECT id FROM accounts WHERE in_all)))
             )
             SELECT view, COUNT(*) FROM latest WHERE rn = 1 GROUP BY view",
        )?;
        let mut counts = [0usize; 6];
        let rows = stmt.query_map([account.map(|a| a.0)], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (key, n) = row?;
            let view = TriageView::from_key(&key)
                .ok_or_else(|| StoreError::Corrupt(format!("unknown view {key:?}")))?;
            let n = usize::try_from(n).unwrap_or_default();
            for (slot, v) in counts.iter_mut().zip(TriageView::ALL) {
                if v == view || v == TriageView::All {
                    *slot += n;
                }
            }
        }
        Ok(ViewCounts(counts))
    }

    /// All messages in a thread, oldest first.
    pub fn thread_messages(&self, thread: &ThreadRef) -> StoreResult<Vec<MessageDetail>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, from_name, from_email, subject, body_text, received_at, has_attachments,
                    remote_id, message_id, refs
             FROM messages WHERE account_id = ?1 AND thread_key = ?2
             ORDER BY received_at, id",
        )?;
        let rows = stmt.query_map(params![thread.account.0, thread.key], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?;
        rows.map(|row| {
            let (
                id,
                name,
                email,
                subject,
                body_text,
                at,
                has_attachments,
                remote_id,
                message_id,
                refs,
            ) = row?;
            Ok(MessageDetail {
                id: MessageId(id),
                from: Contact::new(name.as_deref(), parse_email(&email)?),
                subject,
                body_text,
                received_at: timestamp(at)?,
                has_attachments,
                remote_id,
                message_id,
                references: refs.split_whitespace().map(str::to_owned).collect(),
            })
        })
        .collect()
    }

    /// Moves threads to a new triage state. Returns their previous states so
    /// the caller can undo with [`MailStore::restore`].
    pub fn set_state(
        &self,
        threads: &[ThreadRef],
        state: TriageState,
    ) -> StoreResult<Vec<(ThreadRef, TriageState)>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut previous = Vec::with_capacity(threads.len());
        for thread in threads {
            previous.push((thread.clone(), read_state(&tx, thread)?));
            write_state(&tx, thread, state)?;
        }
        tx.commit()?;
        Ok(previous)
    }

    /// Puts threads back into the states [`MailStore::set_state`] returned.
    pub fn restore(&self, previous: &[(ThreadRef, TriageState)]) -> StoreResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (thread, state) in previous {
            write_state(&tx, thread, *state)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Marks every message in a thread read or unread.
    pub fn set_read(&self, thread: &ThreadRef, read: bool) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE messages SET is_read = ?3 WHERE account_id = ?1 AND thread_key = ?2",
            params![thread.account.0, thread.key, read],
        )?;
        Ok(())
    }

    /// Returns snoozed threads whose time has come to the inbox. Returns how many woke.
    pub fn wake_snoozed(&self, now: DateTime<Utc>) -> StoreResult<usize> {
        let changed = self.conn.execute(
            "UPDATE messages SET state = 'inbox', snoozed_until = NULL
             WHERE state = 'snoozed' AND snoozed_until <= ?1",
            [now.timestamp()],
        )?;
        Ok(changed)
    }

    /// The triage state of a thread.
    pub fn state(&self, thread: &ThreadRef) -> StoreResult<TriageState> {
        read_state(&self.conn, thread)
    }

    /// The database file, or `None` for an in-memory store. Background sync
    /// opens its own connection to this path.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Saves (or replaces) the IMAP server settings for an account. The sync
    /// cursor is kept unless the server changed.
    pub fn save_imap(&self, account: AccountId, settings: &ImapSettings) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO imap_accounts (account_id, host, port, username, auth) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(account_id) DO UPDATE SET
                uid_validity = CASE WHEN host = excluded.host THEN uid_validity END,
                last_uid = CASE WHEN host = excluded.host THEN last_uid ELSE 0 END,
                host = excluded.host, port = excluded.port, username = excluded.username,
                auth = excluded.auth",
            params![
                account.0,
                settings.host.as_str(),
                settings.port,
                settings.username,
                settings.auth.key()
            ],
        )?;
        Ok(())
    }

    /// Every IMAP account with its settings and sync cursor.
    pub fn imap_accounts(&self) -> StoreResult<Vec<(Account, ImapSettings, SyncCursor)>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, host, port, username, uid_validity, last_uid, auth
             FROM imap_accounts ORDER BY account_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u16>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<u32>>(4)?,
                row.get::<_, u32>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (id, host, port, username, uid_validity, last_uid, auth) = row?;
            let auth = AuthMethod::from_key(&auth)
                .ok_or_else(|| StoreError::Corrupt(format!("auth method {auth:?}")))?;
            let account = self.account(AccountId(id))?.ok_or_else(|| {
                StoreError::Corrupt(format!("imap settings for missing account {id}"))
            })?;
            let host =
                Host::parse(&host).map_err(|_| StoreError::Corrupt(format!("host {host:?}")))?;
            Ok((
                account,
                ImapSettings {
                    host,
                    port,
                    username,
                    auth,
                },
                SyncCursor {
                    uid_validity,
                    last_uid,
                },
            ))
        })
        .collect()
    }

    /// Records how far an IMAP account has synced.
    pub fn set_cursor(&self, account: AccountId, cursor: SyncCursor) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE imap_accounts SET uid_validity = ?2, last_uid = ?3 WHERE account_id = ?1",
            params![account.0, cursor.uid_validity, cursor.last_uid],
        )?;
        Ok(())
    }

    /// One IMAP account with its settings and cursor, if `account` is IMAP.
    pub fn imap_account(
        &self,
        account: AccountId,
    ) -> StoreResult<Option<(Account, ImapSettings, SyncCursor)>> {
        Ok(self
            .imap_accounts()?
            .into_iter()
            .find(|(a, _, _)| a.id == account))
    }

    /// Outgoing settings: the saved override, else the best guess from the
    /// IMAP settings. `None` if `account` isn't an IMAP account.
    pub fn smtp_settings(&self, account: AccountId) -> StoreResult<Option<SmtpSettings>> {
        let Some((acct, imap, _)) = self.imap_account(account)? else {
            return Ok(None);
        };
        let saved: (Option<String>, Option<u16>) = self.conn.query_row(
            "SELECT smtp_host, smtp_port FROM imap_accounts WHERE account_id = ?1",
            [account.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let default = SmtpSettings::for_imap(&acct.email, acct.provider, &imap);
        Ok(Some(match saved {
            (Some(host), Some(port)) => SmtpSettings {
                host: Host::parse(&host)
                    .map_err(|_| StoreError::Corrupt(format!("smtp host {host:?}")))?,
                port,
                auth: imap.auth,
            },
            _ => default,
        }))
    }

    /// Saves an outgoing-server override for an account.
    pub fn save_smtp(&self, account: AccountId, smtp: &SmtpSettings) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE imap_accounts SET smtp_host = ?2, smtp_port = ?3 WHERE account_id = ?1",
            params![account.0, smtp.host.as_str(), smtp.port],
        )?;
        Ok(())
    }

    /// Removes an account and everything stored for it. Mail on the server is untouched.
    pub fn remove_account(&self, account: AccountId) -> StoreResult<()> {
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?1", [account.0])?;
        Ok(())
    }
}

fn read_state(conn: &Connection, thread: &ThreadRef) -> StoreResult<TriageState> {
    let (state, until): (String, Option<i64>) = conn.query_row(
        "SELECT state, snoozed_until FROM messages
         WHERE account_id = ?1 AND thread_key = ?2 ORDER BY id DESC LIMIT 1",
        params![thread.account.0, thread.key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    match (state.as_str(), until) {
        ("inbox", _) => Ok(TriageState::Inbox),
        ("done", _) => Ok(TriageState::Done),
        ("snoozed", Some(at)) => Ok(TriageState::Snoozed {
            until: timestamp(at)?,
        }),
        (other, _) => Err(StoreError::Corrupt(format!("unknown state {other:?}"))),
    }
}

fn write_state(conn: &Connection, thread: &ThreadRef, state: TriageState) -> StoreResult<()> {
    let (key, until) = match state {
        TriageState::Inbox => ("inbox", None),
        TriageState::Done => ("done", None),
        TriageState::Snoozed { until } => ("snoozed", Some(until.timestamp())),
    };
    conn.execute(
        "UPDATE messages SET state = ?3, snoozed_until = ?4
         WHERE account_id = ?1 AND thread_key = ?2",
        params![thread.account.0, thread.key, key, until],
    )?;
    Ok(())
}

struct RawSummary {
    id: i64,
    account: i64,
    key: String,
    from_name: Option<String>,
    from_email: String,
    subject: String,
    preview: String,
    received_at: i64,
    count: i64,
    all_read: bool,
    has_attachments: bool,
    view: String,
}

fn raw_summary(row: &Row<'_>) -> rusqlite::Result<RawSummary> {
    Ok(RawSummary {
        id: row.get(0)?,
        account: row.get(1)?,
        key: row.get(2)?,
        from_name: row.get(3)?,
        from_email: row.get(4)?,
        subject: row.get(5)?,
        preview: row.get(6)?,
        received_at: row.get(7)?,
        count: row.get(8)?,
        all_read: row.get(9)?,
        has_attachments: row.get(10)?,
        view: row.get(11)?,
    })
}

impl RawSummary {
    fn parse(self) -> StoreResult<ThreadSummary> {
        Ok(ThreadSummary {
            thread: ThreadRef {
                account: AccountId(self.account),
                key: self.key,
            },
            latest: MessageId(self.id),
            from: Contact::new(self.from_name.as_deref(), parse_email(&self.from_email)?),
            subject: self.subject,
            preview: self.preview,
            received_at: timestamp(self.received_at)?,
            message_count: usize::try_from(self.count).unwrap_or(1),
            unread: !self.all_read,
            has_attachments: self.has_attachments,
            view: TriageView::from_key(&self.view)
                .ok_or_else(|| StoreError::Corrupt(format!("unknown view {:?}", self.view)))?,
        })
    }
}

fn parse_email(text: &str) -> StoreResult<EmailAddress> {
    EmailAddress::parse(text).map_err(|e| StoreError::Corrupt(format!("address {text:?}: {e}")))
}

fn timestamp(seconds: i64) -> StoreResult<DateTime<Utc>> {
    DateTime::from_timestamp(seconds, 0)
        .ok_or_else(|| StoreError::Corrupt(format!("timestamp {seconds} out of range")))
}

fn provider_key(provider: Provider) -> &'static str {
    match provider {
        Provider::Microsoft365 => "microsoft365",
        Provider::Google => "google",
        Provider::ICloud => "icloud",
        Provider::Imap => "imap",
    }
}

fn provider_from_key(key: &str) -> StoreResult<Provider> {
    Provider::ALL
        .into_iter()
        .find(|p| provider_key(*p) == key)
        .ok_or_else(|| StoreError::Corrupt(format!("unknown provider {key:?}")))
}

#[cfg(test)]
mod tests;
