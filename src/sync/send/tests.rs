//! The send flow and read write-back, with a recording SMTP sender and a
//! recording IMAP session.

use std::sync::{Arc, Mutex};

use super::*;
use crate::domain::{EmailAddress, Provider, Recipients, ReplyContext, TriageView};
use crate::store::{Account, ThreadQuery};
use crate::sync::imap::password_key;
use crate::sync::imap::{MailboxInfo, RawMessage};
use crate::sync::secrets::MemorySecrets;

#[derive(Default)]
struct Log {
    sent: Vec<String>,
    appended: Vec<Vec<u8>>,
    seen: Vec<(Vec<u32>, bool)>,
}

struct Session {
    log: Arc<Mutex<Log>>,
    has_sent_folder: bool,
}

impl MailSession for Session {
    fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError> {
        Ok(MailboxInfo {
            uid_validity: Some(7),
        })
    }
    fn uid_search(&mut self, _: &str) -> Result<Vec<u32>, SyncError> {
        Ok(Vec::new())
    }
    fn uid_fetch(&mut self, _: &[u32]) -> Result<Vec<RawMessage>, SyncError> {
        Ok(Vec::new())
    }
    fn set_seen(&mut self, uids: &[u32], seen: bool) -> Result<(), SyncError> {
        self.log.lock().unwrap().seen.push((uids.to_vec(), seen));
        Ok(())
    }
    fn append_sent(&mut self, raw: &[u8]) -> Result<bool, SyncError> {
        if self.has_sent_folder {
            self.log.lock().unwrap().appended.push(raw.to_vec());
        }
        Ok(self.has_sent_folder)
    }
}

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-16T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

struct World {
    store: MailStore,
    secrets: MemorySecrets,
    account: Account,
    log: Arc<Mutex<Log>>,
}

fn world() -> World {
    let store = MailStore::open_in_memory().unwrap();
    let email = EmailAddress::parse("you@fastmail.com").unwrap();
    let account = store.add_account("Mail", &email, Provider::Imap).unwrap();
    store
        .save_imap(
            account.id,
            &ImapSettings::for_address(&email, Provider::Imap),
        )
        .unwrap();
    let secrets = MemorySecrets::default();
    secrets.set(&password_key(&email), "pw").unwrap();
    World {
        store,
        secrets,
        account,
        log: Arc::default(),
    }
}

impl World {
    fn sender(&self, fail: bool) -> Sender {
        let log = Arc::clone(&self.log);
        Arc::new(move |smtp, user, password, message| {
            assert_eq!(smtp.host.as_str(), "smtp.fastmail.com");
            assert_eq!((user, password), ("you@fastmail.com", "pw"));
            if fail {
                return Err(SyncError::Auth);
            }
            log.lock()
                .unwrap()
                .sent
                .push(String::from_utf8(message.formatted()).unwrap());
            Ok(())
        })
    }

    fn connect(
        &self,
        has_sent_folder: bool,
    ) -> impl FnOnce(&ImapSettings, &str) -> Result<Session, SyncError> {
        let log = Arc::clone(&self.log);
        move |_, _| {
            Ok(Session {
                log,
                has_sent_folder,
            })
        }
    }

    /// Delivers a message from Priya into thread "q4" with UID 42.
    fn deliver(&self) -> ThreadRef {
        self.store
            .upsert(&IncomingMessage {
                account: self.account.id,
                remote_id: "7:42".into(),
                thread_key: "q4".into(),
                from: Contact::new(
                    Some("Priya"),
                    EmailAddress::parse("priya@company.com").unwrap(),
                ),
                subject: "Q4 numbers".into(),
                body_text: "Numbers by Monday?".into(),
                received_at: now(),
                is_read: false,
                has_attachments: false,
                signals: Signals {
                    addressed_to_me: true,
                    ..Signals::default()
                },
                message_id: Some("m1@company.com".into()),
                references: Vec::new(),
            })
            .unwrap();
        self.store
            .set_cursor(
                self.account.id,
                crate::domain::SyncCursor {
                    uid_validity: Some(7),
                    last_uid: 42,
                },
            )
            .unwrap();
        self.store.threads(ThreadQuery::default()).unwrap()[0]
            .thread
            .clone()
    }
}

#[test]
fn replying_sends_saves_and_marks_the_thread_done() {
    let w = world();
    let thread = w.deliver();
    let draft = Draft::new(
        Recipients::parse("priya@company.com").unwrap(),
        "Q4 numbers",
        "On it.",
    )
    .unwrap()
    .replying(ReplyContext::answering("m1@company.com", &[]));
    let request = SendRequest {
        account: w.account.id,
        draft,
        thread: Some(thread.clone()),
    };
    let outcome = send_message(
        &w.store,
        &w.secrets,
        &w.sender(false),
        w.connect(true),
        &request,
        now(),
        9,
    )
    .unwrap();
    assert_eq!(
        outcome,
        SendOutcome {
            saved_to_sent: true
        }
    );

    let log = w.log.lock().unwrap();
    assert!(log.sent[0].contains("In-Reply-To: <m1@company.com>"));
    assert_eq!(log.appended.len(), 1);
    drop(log);

    assert_eq!(w.store.state(&thread).unwrap(), TriageState::Done);
    let messages = w.store.thread_messages(&thread).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].body_text, "On it.");
    assert_eq!(messages[1].from.display(), "Mail");
    assert!(messages[1].remote_id.starts_with("sent:"));
    assert_eq!(messages[1].references, ["m1@company.com"]);
}

#[test]
fn a_new_message_is_stored_out_of_the_inbox_even_without_a_sent_folder() {
    let w = world();
    let draft = Draft::new(
        Recipients::parse("lena@company.com").unwrap(),
        "Hello",
        "Hi Lena",
    )
    .unwrap();
    let request = SendRequest {
        account: w.account.id,
        draft,
        thread: None,
    };
    let outcome = send_message(
        &w.store,
        &w.secrets,
        &w.sender(false),
        w.connect(false),
        &request,
        now(),
        1,
    )
    .unwrap();
    assert!(!outcome.saved_to_sent);
    assert_eq!(
        w.store.counts(None).unwrap().get(TriageView::All),
        0,
        "sent mail isn't inbox mail"
    );
}

#[test]
fn a_rejected_send_saves_nothing() {
    let w = world();
    let thread = w.deliver();
    let draft = Draft::new(Recipients::parse("priya@company.com").unwrap(), "Re", "Hi").unwrap();
    let request = SendRequest {
        account: w.account.id,
        draft,
        thread: Some(thread.clone()),
    };
    let err = send_message(
        &w.store,
        &w.secrets,
        &w.sender(true),
        w.connect(true),
        &request,
        now(),
        1,
    )
    .unwrap_err();
    assert!(matches!(err, SyncError::Auth));
    assert_eq!(w.store.thread_messages(&thread).unwrap().len(), 1);
    assert_eq!(w.store.state(&thread).unwrap(), TriageState::Inbox);
    assert!(w.log.lock().unwrap().appended.is_empty());
}

#[test]
fn a_failed_sent_copy_does_not_fail_the_send() {
    let w = world();
    let draft = Draft::new(Recipients::parse("a@x.com").unwrap(), "Hi", "Hello").unwrap();
    let request = SendRequest {
        account: w.account.id,
        draft,
        thread: None,
    };
    let broken = |_: &ImapSettings, _: &str| -> Result<Session, SyncError> { Err(SyncError::Auth) };
    let outcome = send_message(
        &w.store,
        &w.secrets,
        &w.sender(false),
        broken,
        &request,
        now(),
        1,
    )
    .unwrap();
    assert!(!outcome.saved_to_sent);
    assert_eq!(w.log.lock().unwrap().sent.len(), 1);
}

#[test]
fn accounts_that_cannot_send_say_so() {
    let w = world();
    let other = w
        .store
        .add_account(
            "Other",
            &EmailAddress::parse("o@x.com").unwrap(),
            Provider::Microsoft365,
        )
        .unwrap();
    let draft = Draft::new(Recipients::parse("a@x.com").unwrap(), "Hi", "Hello").unwrap();
    let request = SendRequest {
        account: other.id,
        draft: draft.clone(),
        thread: None,
    };
    let err = send_message(
        &w.store,
        &w.secrets,
        &w.sender(false),
        w.connect(true),
        &request,
        now(),
        1,
    )
    .unwrap_err();
    assert!(err.to_string().contains("can't send mail yet"));

    w.secrets.delete(&password_key(&w.account.email)).unwrap();
    let request = SendRequest {
        account: w.account.id,
        draft,
        thread: None,
    };
    let err = send_message(
        &w.store,
        &w.secrets,
        &w.sender(false),
        w.connect(true),
        &request,
        now(),
        1,
    )
    .unwrap_err();
    assert!(matches!(err, SyncError::NoPassword));
}

#[test]
fn read_state_is_written_back_for_current_uids_only() {
    let w = world();
    let thread = w.deliver();
    // A stale UID from an older UIDVALIDITY and a local sent copy are skipped.
    w.store
        .upsert(&IncomingMessage {
            account: w.account.id,
            remote_id: "6:5".into(),
            thread_key: "q4".into(),
            from: Contact::new(None, EmailAddress::parse("priya@company.com").unwrap()),
            subject: "old".into(),
            body_text: "old".into(),
            received_at: now(),
            is_read: false,
            has_attachments: false,
            signals: Signals::default(),
            message_id: None,
            references: Vec::new(),
        })
        .unwrap();
    let written = write_read(&w.store, &w.secrets, w.connect(true), &thread, true).unwrap();
    assert_eq!(written, 1);
    assert_eq!(w.log.lock().unwrap().seen, [(vec![42], true)]);

    let other = ThreadRef {
        account: AccountId(999),
        key: "x".into(),
    };
    assert_eq!(
        write_read(&w.store, &w.secrets, w.connect(true), &other, true).unwrap(),
        0
    );
}

#[test]
fn write_back_needs_a_cursor_and_messages() {
    let w = world();
    let thread = ThreadRef {
        account: w.account.id,
        key: "empty".into(),
    };
    assert_eq!(
        write_read(&w.store, &w.secrets, w.connect(true), &thread, true).unwrap(),
        0,
        "no cursor yet"
    );
    w.store
        .set_cursor(
            w.account.id,
            crate::domain::SyncCursor {
                uid_validity: Some(7),
                last_uid: 0,
            },
        )
        .unwrap();
    assert_eq!(
        write_read(&w.store, &w.secrets, w.connect(true), &thread, true).unwrap(),
        0,
        "no messages"
    );
}

#[test]
fn remote_ids_map_to_uids() {
    assert_eq!(uid_for("7:42", 7), Some(42));
    assert_eq!(uid_for("6:42", 7), None);
    assert_eq!(uid_for("sent:abc", 7), None);
    assert_eq!(uid_for("7:x", 7), None);
}
