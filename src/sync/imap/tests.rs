//! Sync engine tests against a fake session, and the real `imap` adapter
//! against a scripted IMAP server on localhost.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::thread;

use super::*;
use crate::domain::{Provider, TriageView};
use crate::store::ThreadQuery;
use crate::sync::secrets::{MemorySecrets, Secrets};
use crate::sync::{AddAccountRequest, add_imap_account, remove_account, sync_all};

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-16T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn raw(n: u32, from: &str, subject: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: you@company.com\r\nSubject: {subject}\r\n\
Message-ID: <m{n}@x.com>\r\nDate: Fri, 16 Oct 2026 09:0{}:00 +0200\r\n\r\nBody {n}\r\n",
        n % 10
    )
    .into_bytes()
}

/// An in-memory INBOX.
#[derive(Default)]
struct FakeSession {
    validity: Option<u32>,
    messages: BTreeMap<u32, RawMessage>,
    searches: Vec<String>,
    fetches: usize,
    fail_fetch: bool,
}

impl FakeSession {
    fn with(validity: u32, uids: impl IntoIterator<Item = u32>) -> Self {
        let mut s = Self {
            validity: Some(validity),
            ..Self::default()
        };
        for uid in uids {
            s.add(
                uid,
                &format!("Person {uid} <p{uid}@x.com>"),
                &format!("Subject {uid}"),
            );
        }
        s
    }

    fn add(&mut self, uid: u32, from: &str, subject: &str) {
        self.messages.insert(
            uid,
            RawMessage {
                uid,
                seen: uid.is_multiple_of(2),
                internal_date: Some(now()),
                body: raw(uid, from, subject),
            },
        );
    }
}

impl MailSession for FakeSession {
    fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError> {
        Ok(MailboxInfo {
            uid_validity: self.validity,
        })
    }

    fn uid_search(&mut self, query: &str) -> Result<Vec<u32>, SyncError> {
        self.searches.push(query.to_owned());
        if let Some(range) = query.strip_prefix("UID ") {
            let from: u32 = range.trim_end_matches(":*").parse().unwrap();
            let mut hits: Vec<u32> = self
                .messages
                .keys()
                .copied()
                .filter(|u| *u >= from)
                .collect();
            // Real servers always include the highest UID for `n:*`.
            if let Some(max) = self.messages.keys().max() {
                hits.push(*max);
            }
            Ok(hits)
        } else {
            Ok(self.messages.keys().copied().collect())
        }
    }

    fn uid_fetch(&mut self, uids: &[u32]) -> Result<Vec<RawMessage>, SyncError> {
        if self.fail_fetch {
            return Err(SyncError::Protocol("boom".into()));
        }
        self.fetches += 1;
        Ok(uids
            .iter()
            .filter_map(|u| self.messages.get(u).cloned())
            .collect())
    }
    fn set_seen(&mut self, _: &[u32], _: bool) -> Result<(), SyncError> {
        Ok(())
    }
    fn append_sent(&mut self, _: &[u8]) -> Result<bool, SyncError> {
        Ok(false)
    }
}

fn setup() -> (MailStore, Account) {
    let store = MailStore::open_in_memory().unwrap();
    let email = EmailAddress::parse("you@company.com").unwrap();
    let account = store.add_account("Mail", &email, Provider::Imap).unwrap();
    store
        .save_imap(
            account.id,
            &ImapSettings::for_address(&email, Provider::Imap),
        )
        .unwrap();
    (store, account)
}

fn cursor(store: &MailStore) -> SyncCursor {
    store.imap_accounts().unwrap()[0].2
}

#[test]
fn first_sync_fetches_recent_mail_in_batches_then_only_new_mail() {
    let (store, account) = setup();
    let mut session = FakeSession::with(7, 1..=120);
    let report = sync_inbox(&mut session, &store, &account, SyncCursor::default(), now()).unwrap();
    assert_eq!(
        report,
        SyncReport {
            stored: 120,
            skipped: 0,
            reset: false
        }
    );
    assert_eq!(session.fetches, 3, "120 messages in batches of 50");
    assert_eq!(session.searches, ["SINCE 18-Jul-2026"]);
    assert_eq!(
        cursor(&store),
        SyncCursor {
            uid_validity: Some(7),
            last_uid: 120
        }
    );
    assert_eq!(store.counts(None).unwrap().get(TriageView::All), 120);

    session.add(121, "Priya <priya@company.com>", "New one");
    let report = sync_inbox(&mut session, &store, &account, cursor(&store), now()).unwrap();
    assert_eq!(report.stored, 1);
    assert_eq!(session.searches.last().unwrap(), "UID 121:*");
    assert_eq!(cursor(&store).last_uid, 121);

    let report = sync_inbox(&mut session, &store, &account, cursor(&store), now()).unwrap();
    assert_eq!(
        report.stored, 0,
        "the always-returned highest UID is filtered out"
    );
}

#[test]
fn uidvalidity_change_refetches_without_duplicates() {
    let (store, account) = setup();
    let mut session = FakeSession::with(7, 1..=3);
    sync_inbox(&mut session, &store, &account, SyncCursor::default(), now()).unwrap();
    session.validity = Some(8);
    let report = sync_inbox(&mut session, &store, &account, cursor(&store), now()).unwrap();
    assert!(report.reset);
    assert_eq!(report.stored, 3);
    assert_eq!(
        cursor(&store),
        SyncCursor {
            uid_validity: Some(8),
            last_uid: 3
        }
    );
}

#[test]
fn unparseable_mail_is_skipped_and_read_flags_carry_over() {
    let (store, account) = setup();
    let mut session = FakeSession::with(7, [2]);
    session.messages.insert(
        3,
        RawMessage {
            uid: 3,
            seen: false,
            internal_date: None,
            body: b"garbage".to_vec(),
        },
    );
    let report = sync_inbox(&mut session, &store, &account, SyncCursor::default(), now()).unwrap();
    assert_eq!(
        report,
        SyncReport {
            stored: 1,
            skipped: 1,
            reset: false
        }
    );
    assert_eq!(
        cursor(&store).last_uid,
        3,
        "skipped mail isn't retried forever"
    );
    let threads = store.threads(ThreadQuery::default()).unwrap();
    assert!(!threads[0].unread, "uid 2 had \\Seen");
}

#[test]
fn fetch_errors_stop_the_run_and_keep_earlier_progress() {
    let (store, account) = setup();
    let mut session = FakeSession::with(7, 1..=3);
    session.fail_fetch = true;
    let err = sync_inbox(&mut session, &store, &account, SyncCursor::default(), now()).unwrap_err();
    assert_eq!(err.to_string(), "the server returned an error: boom");
    assert_eq!(cursor(&store).last_uid, 0);
}

#[test]
fn adding_an_account_checks_the_sign_in_first() {
    let store = MailStore::open_in_memory().unwrap();
    let secrets = MemorySecrets::default();
    let email = EmailAddress::parse("you@fastmail.com").unwrap();
    let settings = ImapSettings::for_address(&email, Provider::Imap);

    let request = |password: &str| AddAccountRequest {
        name: "Mail".into(),
        email: email.clone(),
        provider: Provider::Imap,
        settings: settings.clone(),
        password: password.into(),
    };
    let rejected = add_imap_account::<FakeSession>(&store, &secrets, &request("wrong"), |_, _| {
        Err(SyncError::Auth)
    });
    assert!(matches!(rejected, Err(SyncError::Auth)));
    assert!(
        store.accounts().unwrap().is_empty(),
        "nothing saved on failure"
    );

    let account = add_imap_account(&store, &secrets, &request("right"), |s, p| {
        assert_eq!(s.host.as_str(), "imap.fastmail.com");
        assert_eq!(p, "right");
        Ok(FakeSession::with(1, []))
    })
    .unwrap();
    assert_eq!(
        secrets.get("imap:you@fastmail.com").unwrap().as_deref(),
        Some("right")
    );
    assert_eq!(store.imap_accounts().unwrap()[0].1, settings);

    remove_account(&store, &secrets, &account).unwrap();
    assert!(store.accounts().unwrap().is_empty());
    assert!(
        store.imap_accounts().unwrap().is_empty(),
        "settings cascade away"
    );
    assert_eq!(secrets.get("imap:you@fastmail.com").unwrap(), None);
}

#[test]
fn sync_all_reports_each_account_separately() {
    let (store, account) = setup();
    let secrets = MemorySecrets::default();
    let other_email = EmailAddress::parse("me@other.org").unwrap();
    let other = store
        .add_account("Other", &other_email, Provider::Imap)
        .unwrap();
    store
        .save_imap(
            other.id,
            &ImapSettings::for_address(&other_email, Provider::Imap),
        )
        .unwrap();
    secrets.set(&password_key(&account.email), "pw").unwrap();

    let results = sync_all(
        &store,
        &secrets,
        |_, _| Ok(FakeSession::with(1, 1..=2)),
        now(),
    )
    .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].1.as_ref().unwrap().stored, 2);
    assert!(matches!(results[1].1, Err(SyncError::NoPassword)));
    assert_eq!(
        SyncError::NoPassword.to_string(),
        "no password is saved for this account"
    );
}

#[test]
fn saving_new_server_settings_resets_the_cursor() {
    let (store, account) = setup();
    store
        .set_cursor(
            account.id,
            SyncCursor {
                uid_validity: Some(3),
                last_uid: 50,
            },
        )
        .unwrap();
    let same = store.imap_accounts().unwrap()[0].1.clone();
    store
        .save_imap(
            account.id,
            &ImapSettings {
                port: 143,
                ..same.clone()
            },
        )
        .unwrap();
    assert_eq!(cursor(&store).last_uid, 50, "same host keeps progress");
    let moved = ImapSettings {
        host: crate::domain::Host::parse("imap.new.com").unwrap(),
        ..same
    };
    store.save_imap(account.id, &moved).unwrap();
    assert_eq!(cursor(&store), SyncCursor::default());
}

#[test]
fn imap_dates_and_password_keys() {
    assert_eq!(imap_date(now()), "16-Oct-2026");
    let email = EmailAddress::parse("you@company.com").unwrap();
    assert_eq!(password_key(&email), "imap:you@company.com");
    let connect = SyncError::Connect {
        host: "imap.x.com".into(),
        detail: "timed out".into(),
    };
    assert_eq!(connect.to_string(), "couldn't reach imap.x.com: timed out");
    assert!(SyncError::Auth.to_string().contains("app password"));
}

// ---------- the real `imap` adapter against a scripted server ----------

/// Serves one connection: greets, then answers LOGIN, EXAMINE, UID SEARCH,
/// UID FETCH and LOGOUT with canned responses.
fn scripted_server(message: Vec<u8>, accept_password: &'static str) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut out = stream;
        out.write_all(b"* OK IMAP4rev1 ready\r\n").unwrap();
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let mut parts = line.trim_end().splitn(3, ' ');
            let tag = parts.next().unwrap_or("*").to_owned();
            let command = parts.next().unwrap_or("").to_ascii_uppercase();
            let rest = parts.next().unwrap_or("").to_owned();
            line.clear();
            let reply = match command.as_str() {
                "LOGIN" if rest.trim_end_matches('"').ends_with(accept_password) => {
                    format!("{tag} OK LOGIN completed\r\n")
                }
                "LOGIN" => format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n"),
                "AUTHENTICATE" => {
                    out.write_all(b"+ \r\n").unwrap();
                    let mut response = String::new();
                    reader.read_line(&mut response).unwrap();
                    let decoded = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        response.trim_end(),
                    )
                    .unwrap_or_default();
                    let text = String::from_utf8_lossy(&decoded);
                    if text.contains(&format!("auth=Bearer {accept_password}\x01")) {
                        format!("{tag} OK AUTHENTICATE completed\r\n")
                    } else {
                        format!("{tag} NO [AUTHENTICATIONFAILED] Invalid token\r\n")
                    }
                }
                "EXAMINE" => format!(
                    "* 1 EXISTS\r\n* OK [UIDVALIDITY 9] UIDs valid\r\n{tag} OK [READ-ONLY] EXAMINE completed\r\n"
                ),
                "UID" if rest.starts_with("SEARCH") => {
                    format!("* SEARCH 42\r\n{tag} OK SEARCH completed\r\n")
                }
                "UID" if rest.starts_with("FETCH") => {
                    let mut r = format!(
                        "* 1 FETCH (UID 42 FLAGS (\\Seen) INTERNALDATE \"16-Oct-2026 09:42:00 +0200\" BODY[] {{{}}}\r\n",
                        message.len()
                    )
                    .into_bytes();
                    r.extend_from_slice(&message);
                    r.extend_from_slice(format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes());
                    out.write_all(&r).unwrap();
                    continue;
                }
                "LOGOUT" => {
                    let _ =
                        out.write_all(format!("* BYE\r\n{tag} OK LOGOUT completed\r\n").as_bytes());
                    break;
                }
                _ => format!("{tag} BAD unknown\r\n"),
            };
            out.write_all(reply.as_bytes()).unwrap();
        }
    });
    addr
}

fn client(addr: std::net::SocketAddr) -> imap::Client<TcpStream> {
    let mut client = imap::Client::new(TcpStream::connect(addr).unwrap());
    client.read_greeting().unwrap();
    client
}

#[test]
fn real_adapter_syncs_from_a_server() {
    let addr = scripted_server(
        raw(42, "Tom <tom@company.com>", "Vendor contract"),
        "secret",
    );
    let mut session = login(
        client(addr),
        "you@company.com",
        "secret",
        AuthMethod::Password,
    )
    .unwrap();
    let (store, account) = setup();
    let report = sync_inbox(&mut session, &store, &account, SyncCursor::default(), now()).unwrap();
    assert_eq!(report.stored, 1);
    assert_eq!(
        cursor(&store),
        SyncCursor {
            uid_validity: Some(9),
            last_uid: 42
        }
    );
    let thread = &store.threads(ThreadQuery::default()).unwrap()[0];
    assert_eq!(thread.subject, "Vendor contract");
    assert!(!thread.unread);
}

#[test]
fn real_adapter_maps_a_rejected_login_to_auth() {
    let addr = scripted_server(Vec::new(), "secret");
    assert!(matches!(
        login(
            client(addr),
            "you@company.com",
            "nope",
            AuthMethod::Password
        ),
        Err(SyncError::Auth)
    ));
}

#[test]
fn connecting_to_nowhere_is_a_connect_error() {
    // Port 9 on localhost is reserved and closed; the TLS connector fails fast.
    let settings = ImapSettings {
        host: crate::domain::Host::parse("localhost.invalid").unwrap(),
        port: 9,
        username: "x".into(),
        auth: AuthMethod::Password,
    };
    match connect(&settings, "pw") {
        Err(SyncError::Connect { host, .. }) => assert_eq!(host, "localhost.invalid"),
        Err(other) => panic!("expected a connect error, got {other}"),
        Ok(_) => panic!("connected to nowhere"),
    }
}

#[test]
fn real_adapter_signs_in_with_an_oauth_token() {
    let addr = scripted_server(raw(42, "Tom <tom@company.com>", "Hi"), "token-123");
    let mut session = login(
        client(addr),
        "you@company.com",
        "token-123",
        AuthMethod::OAuth,
    )
    .unwrap();
    assert!(session.examine_inbox().is_ok());
    let addr = scripted_server(Vec::new(), "token-123");
    assert!(matches!(
        login(
            client(addr),
            "you@company.com",
            "expired",
            AuthMethod::OAuth
        ),
        Err(SyncError::Auth)
    ));
}

#[test]
fn sign_in_problems_are_told_apart() {
    use crate::oauth::OAuthError;
    assert!(SyncError::Auth.needs_sign_in());
    assert!(SyncError::NoPassword.needs_sign_in());
    assert!(SyncError::OAuth(OAuthError::Provider("invalid_grant".into())).needs_sign_in());
    assert!(!SyncError::OAuth(OAuthError::Network("offline".into())).needs_sign_in());
    assert!(!SyncError::Protocol("BAD".into()).needs_sign_in());
}
