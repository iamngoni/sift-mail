//! The sync work the UI hands to background threads.
//!
//! Each call opens its own connection to the database file, so it can run on
//! any thread while the UI keeps its connection (SQLite WAL allows this).
//! Nothing here touches the UI; results come back as plain values.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use super::imap::{MailSession, MailboxInfo, RawMessage, SyncError, connect};
use super::secrets::Refreshing;
use super::secrets::Secrets;
use super::send::{SendOutcome, SendRequest, send_message, write_read};
use super::smtp::{Sender, smtp_sender};
use super::{AddAccountRequest, add_imap_account, add_oauth_account, remove_account, sync_some};
use crate::domain::{AccountId, EmailAddress, Provider};
use crate::domain::{ImapSettings, ThreadRef};
use crate::oauth::{self, OAuthConfig, OAuthError, ReqwestHttp, SignedIn};
use crate::store::{Account, MailStore};
use std::sync::atomic::{AtomicBool, Ordering};

/// A boxed session, so tests can swap the network for a fake.
pub type BoxedSession = Box<dyn MailSession + Send>;

/// Opens an authenticated session for some settings and password.
pub type Connector =
    Arc<dyn Fn(&ImapSettings, &str) -> Result<BoxedSession, SyncError> + Send + Sync>;

impl MailSession for BoxedSession {
    fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError> {
        (**self).examine_inbox()
    }

    fn uid_search(&mut self, query: &str) -> Result<Vec<u32>, SyncError> {
        (**self).uid_search(query)
    }

    fn uid_fetch(&mut self, uids: &[u32]) -> Result<Vec<RawMessage>, SyncError> {
        (**self).uid_fetch(uids)
    }

    fn set_seen(&mut self, uids: &[u32], seen: bool) -> Result<(), SyncError> {
        (**self).set_seen(uids, seen)
    }

    fn append_sent(&mut self, raw: &[u8]) -> Result<bool, SyncError> {
        (**self).append_sent(raw)
    }
}

/// Result of one background sync pass, for the status line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncSummary {
    /// Accounts that synced.
    pub ok: usize,
    /// New messages stored across accounts.
    pub stored: usize,
    /// One message per failed account, safe to show.
    pub failures: Vec<String>,
}

/// Background sync bound to one database file and credential store.
#[derive(Clone)]
pub struct SyncService {
    db_path: PathBuf,
    secrets: Arc<dyn Secrets>,
    connector: Connector,
    sender: Sender,
    signer: Signer,
}

/// Runs a browser sign-in: provider, optional address hint, a callback that
/// receives the sign-in link (so it can be copied if the browser didn't open),
/// and a cancellation check polled while waiting for the browser.
pub type Signer = Arc<
    dyn Fn(
            Provider,
            Option<&EmailAddress>,
            &dyn Fn(&str),
            &dyn Fn() -> bool,
        ) -> Result<SignedIn, OAuthError>
        + Send
        + Sync,
>;

/// Where the UI reads the sign-in link from while it waits.
pub type LinkSlot = Arc<std::sync::Mutex<Option<String>>>;

/// The production signer: the system browser and real token endpoints.
pub fn browser_signer() -> Signer {
    Arc::new(|provider, hint, show_link, cancelled| {
        let config = OAuthConfig::for_provider(provider)?;
        oauth::sign_in(
            &config,
            &ReqwestHttp::new()?,
            hint,
            &|url: &str| {
                show_link(url);
                oauth::system_browser(url)
            },
            oauth::SIGN_IN_TIMEOUT,
            cancelled,
        )
    })
}

impl SyncService {
    /// A service over `db_path` using `secrets`, a custom IMAP connector and SMTP sender.
    pub fn new(
        db_path: PathBuf,
        secrets: Arc<dyn Secrets>,
        connector: Connector,
        sender: Sender,
    ) -> Self {
        Self {
            db_path,
            secrets,
            connector,
            sender,
            signer: browser_signer(),
        }
    }

    /// Replaces the browser sign-in, for tests and previews.
    pub fn with_signer(mut self, signer: Signer) -> Self {
        self.signer = signer;
        self
    }

    /// The production service: real TLS connections.
    pub fn networked(db_path: PathBuf, secrets: Arc<dyn Secrets>) -> Self {
        let connector: Connector = Arc::new(|settings: &ImapSettings, password: &str| {
            connect(settings, password).map(|s| Box::new(s) as BoxedSession)
        });
        let secrets: Arc<dyn Secrets> = Arc::new(Refreshing::networked(secrets));
        Self::new(db_path, secrets, connector, smtp_sender())
    }

    /// Signs in through the browser and saves the account. Blocking; run off
    /// the UI thread. Set `cancel` to stop waiting for the browser.
    pub fn sign_in(
        &self,
        provider: Provider,
        hint: Option<EmailAddress>,
        link: LinkSlot,
        cancel: Arc<AtomicBool>,
    ) -> Result<Account, SyncError> {
        let show_link = move |url: &str| {
            if let Ok(mut slot) = link.lock() {
                *slot = Some(url.to_owned());
            }
        };
        let signed_in = (self.signer)(provider, hint.as_ref(), &show_link, &|| {
            cancel.load(Ordering::Relaxed)
        })?;
        let store = MailStore::open(&self.db_path)?;
        let connector = Arc::clone(&self.connector);
        add_oauth_account(
            &store,
            self.secrets.as_ref(),
            provider,
            &signed_in,
            move |s: &ImapSettings, p: &str| connector(s, p),
        )
    }

    /// Checks the sign-in and saves the account. Blocking; run off the UI thread.
    pub fn add_account(&self, request: AddAccountRequest) -> Result<Account, SyncError> {
        let store = MailStore::open(&self.db_path)?;
        let connector = Arc::clone(&self.connector);
        add_imap_account(
            &store,
            self.secrets.as_ref(),
            &request,
            move |s: &ImapSettings, p: &str| connector(s, p),
        )
    }

    /// Sends a message and records it. Blocking; run off the UI thread.
    pub fn send(&self, request: SendRequest) -> Result<SendOutcome, SyncError> {
        let store = MailStore::open(&self.db_path)?;
        let connector = Arc::clone(&self.connector);
        send_message(
            &store,
            self.secrets.as_ref(),
            &self.sender,
            move |s: &ImapSettings, p: &str| connector(s, p),
            &request,
            Utc::now(),
            rand_u64(),
        )
    }

    /// Writes a thread's read state to the server. Blocking; run off the UI thread.
    pub fn write_read(&self, thread: ThreadRef, read: bool) -> Result<usize, SyncError> {
        let store = MailStore::open(&self.db_path)?;
        let connector = Arc::clone(&self.connector);
        write_read(
            &store,
            self.secrets.as_ref(),
            move |s: &ImapSettings, p: &str| connector(s, p),
            &thread,
            read,
        )
    }

    /// Syncs every account once. Blocking; run off the UI thread.
    pub fn sync_all(&self, now: DateTime<Utc>) -> Result<SyncSummary, SyncError> {
        self.sync_some(None, now)
    }

    /// Syncs one account. Blocking; run off the UI thread.
    pub fn sync_account(
        &self,
        account: AccountId,
        now: DateTime<Utc>,
    ) -> Result<SyncSummary, SyncError> {
        self.sync_some(Some(account), now)
    }

    /// Forgets an account: its local mail, attachments, settings and saved
    /// sign-in. Mail on the server is untouched.
    pub fn remove(&self, account: AccountId) -> Result<(), SyncError> {
        let store = MailStore::open(&self.db_path)?;
        if let Some(account) = store.account(account)? {
            remove_account(&store, self.secrets.as_ref(), &account)?;
        }
        Ok(())
    }

    fn sync_some(
        &self,
        only: Option<AccountId>,
        now: DateTime<Utc>,
    ) -> Result<SyncSummary, SyncError> {
        let store = MailStore::open(&self.db_path)?;
        let connector = Arc::clone(&self.connector);
        let results = sync_some(
            &store,
            self.secrets.as_ref(),
            move |s: &ImapSettings, p: &str| connector(s, p),
            now,
            only,
        )?;
        let mut summary = SyncSummary::default();
        for (account, result) in results {
            match result {
                Ok(report) => {
                    summary.ok += 1;
                    summary.stored += report.stored;
                    store.record_sync_ok(account, now)?;
                }
                Err(error) => {
                    store.record_sync_error(account, error.needs_sign_in(), &error.to_string())?;
                    let name = store
                        .account(account)
                        .ok()
                        .flatten()
                        .map_or_else(|| "An account".to_owned(), |a| a.name);
                    summary.failures.push(format!("{name}: {error}"));
                }
            }
        }
        Ok(summary)
    }
}

/// A random value for Message-IDs, from the OS-seeded hasher (no extra crate).
fn rand_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.finish()
}

impl SyncSummary {
    /// One line for the title bar, such as "Synced · 3 new".
    pub fn status_line(&self) -> String {
        match (self.failures.first(), self.stored) {
            (Some(first), _) => format!("Sync failed. {first}"),
            (None, 0) => "Up to date".to_owned(),
            (None, 1) => "Synced · 1 new".to_owned(),
            (None, n) => format!("Synced · {n} new"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EmailAddress, Provider};
    use crate::sync::secrets::MemorySecrets;

    struct Inbox(Vec<RawMessage>);

    impl MailSession for Inbox {
        fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError> {
            Ok(MailboxInfo {
                uid_validity: Some(1),
            })
        }
        fn uid_search(&mut self, _: &str) -> Result<Vec<u32>, SyncError> {
            Ok(self.0.iter().map(|m| m.uid).collect())
        }
        fn uid_fetch(&mut self, uids: &[u32]) -> Result<Vec<RawMessage>, SyncError> {
            Ok(self
                .0
                .iter()
                .filter(|m| uids.contains(&m.uid))
                .cloned()
                .collect())
        }
        fn set_seen(&mut self, _: &[u32], _: bool) -> Result<(), SyncError> {
            Ok(())
        }
        fn append_sent(&mut self, _: &[u8]) -> Result<bool, SyncError> {
            Ok(false)
        }
    }

    fn message(uid: u32) -> RawMessage {
        RawMessage {
            uid,
            seen: false,
            internal_date: None,
            body: format!("From: A <a@x.com>\r\nTo: you@x.com\r\nSubject: S{uid}\r\nMessage-ID: <{uid}@x>\r\n\r\nhi\r\n").into_bytes(),
        }
    }

    fn service(dir: &std::path::Path, accept: &'static str) -> SyncService {
        let connector: Connector = Arc::new(move |_: &ImapSettings, password: &str| {
            if password == accept {
                Ok(Box::new(Inbox(vec![message(1), message(2)])) as BoxedSession)
            } else {
                Err(SyncError::Auth)
            }
        });
        SyncService::new(
            dir.join("mail.db"),
            Arc::new(MemorySecrets::default()),
            connector,
            Arc::new(|_, _, _, _| Ok(())),
        )
    }

    fn request(password: &str) -> AddAccountRequest {
        let email = EmailAddress::parse("you@x.com").unwrap();
        AddAccountRequest {
            name: "Mail".into(),
            settings: ImapSettings::for_address(&email, Provider::Imap),
            email,
            provider: Provider::Imap,
            password: password.into(),
        }
    }

    #[test]
    fn adds_then_syncs_through_its_own_connection() {
        let dir = std::env::temp_dir().join(format!("sift-svc-{}", std::process::id()));
        let svc = service(&dir, "pw");
        assert!(matches!(
            svc.add_account(request("bad")),
            Err(SyncError::Auth)
        ));
        let account = svc.add_account(request("pw")).unwrap();
        assert_eq!(account.name, "Mail");
        let summary = svc.sync_all(Utc::now()).unwrap();
        assert_eq!(
            summary,
            SyncSummary {
                ok: 1,
                stored: 2,
                failures: vec![]
            }
        );
        assert_eq!(summary.status_line(), "Synced · 2 new");
        assert_eq!(
            svc.sync_all(Utc::now()).unwrap().status_line(),
            "Up to date"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failures_are_named_per_account() {
        let dir = std::env::temp_dir().join(format!("sift-svc-fail-{}", std::process::id()));
        let svc = service(&dir, "pw");
        svc.add_account(request("pw")).unwrap();
        let first = svc.sync_all(Utc::now()).unwrap();
        assert!(first.failures.is_empty());
        let store = MailStore::open(&dir.join("mail.db")).unwrap();
        let id = store.accounts().unwrap()[0].id;
        assert!(store.account_health(id).unwrap().last_sync.is_some());
        // Lose the saved password: the next sync reports it by account name.
        svc.secrets.delete("imap:you@x.com").unwrap();
        let summary = svc.sync_all(Utc::now()).unwrap();
        assert_eq!(
            summary.failures,
            ["Mail: no password is saved for this account"]
        );
        assert!(summary.status_line().starts_with("Sync failed. Mail:"));
        let health = store.account_health(id).unwrap();
        assert!(health.needs_sign_in, "a missing password needs a sign-in");
        assert!(health.last_error.is_some());
        let one = svc.sync_account(id, Utc::now()).unwrap();
        assert_eq!(one.failures.len(), 1, "only that account is tried");
        svc.remove(id).unwrap();
        svc.remove(id).unwrap();
        assert!(store.accounts().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_lines() {
        let one = SyncSummary {
            ok: 1,
            stored: 1,
            failures: vec![],
        };
        assert_eq!(one.status_line(), "Synced · 1 new");
    }

    #[test]
    fn networked_service_reports_connect_errors() {
        let dir = std::env::temp_dir().join(format!("sift-svc-net-{}", std::process::id()));
        let svc = SyncService::networked(dir.join("mail.db"), Arc::new(MemorySecrets::default()));
        let mut req = request("pw");
        req.settings.host = crate::domain::Host::parse("localhost.invalid").unwrap();
        req.settings.port = 9;
        assert!(matches!(
            svc.add_account(req),
            Err(SyncError::Connect { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
