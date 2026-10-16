//! Store behaviour: classification on insert, thread grouping, triage state
//! transitions, undo, snooze wake-up, and corrupt-data handling.

use super::*;
use crate::domain::Signals;
use chrono::Duration;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-16T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn addr(s: &str) -> EmailAddress {
    EmailAddress::parse(s).unwrap()
}

struct Fixture {
    store: MailStore,
    account: Account,
}

fn fixture() -> Fixture {
    let store = MailStore::open_in_memory().unwrap();
    let account = store
        .add_account("Work", &addr("you@company.com"), Provider::Microsoft365)
        .unwrap();
    Fixture { store, account }
}

impl Fixture {
    fn deliver(&self, remote: &str, thread: &str, minutes_ago: i64, signals: Signals) -> MessageId {
        self.store
            .upsert(&IncomingMessage {
                account: self.account.id,
                remote_id: remote.into(),
                thread_key: thread.into(),
                from: Contact::new(Some("Priya Raman"), addr("priya@company.com")),
                subject: format!("Subject {remote}"),
                body_text: format!("Body of {remote}"),
                received_at: now() - Duration::minutes(minutes_ago),
                is_read: false,
                has_attachments: remote.ends_with('a'),
                signals,
                message_id: Some(format!("{remote}@x.com")),
                references: Vec::new(),
            })
            .unwrap()
    }

    fn thread(&self, key: &str) -> ThreadRef {
        ThreadRef {
            account: self.account.id,
            key: key.into(),
        }
    }

    fn subjects(&self, query: ThreadQuery) -> Vec<String> {
        self.store
            .threads(query)
            .unwrap()
            .into_iter()
            .map(|t| t.subject)
            .collect()
    }
}

const TO_ME: Signals = Signals {
    mailing_list: false,
    calendar_invite: false,
    auto_submitted: false,
    addressed_to_me: true,
};

#[test]
fn accounts_are_unique_by_address() {
    let f = fixture();
    let again = f
        .store
        .add_account("Other name", &addr("you@company.com"), Provider::Google)
        .unwrap();
    assert_eq!(again.id, f.account.id);
    assert_eq!(again.name, "Work");
    assert_eq!(again.provider, Provider::Microsoft365);
    let second = f
        .store
        .add_account("Personal", &addr("you@gmail.com"), Provider::Google)
        .unwrap();
    let names: Vec<_> = f
        .store
        .accounts()
        .unwrap()
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(names, ["Work", "Personal"]);
    assert_eq!(
        f.store.account(second.id).unwrap().unwrap().email.as_str(),
        "you@gmail.com"
    );
    assert_eq!(f.store.account(AccountId(999)).unwrap(), None);
}

#[test]
fn threads_show_the_newest_message_with_counts() {
    let f = fixture();
    f.deliver("1", "q4", 300, TO_ME);
    f.deliver("2a", "q4", 10, TO_ME);
    f.deliver("3", "other", 60, TO_ME);
    let threads = f.store.threads(ThreadQuery::default()).unwrap();
    assert_eq!(threads.len(), 2);
    let q4 = &threads[0];
    assert_eq!(q4.subject, "Subject 2a");
    assert_eq!(q4.message_count, 2);
    assert!(q4.unread);
    assert!(q4.has_attachments);
    assert_eq!(q4.preview, "Body of 2a");
    assert_eq!(q4.view, TriageView::NeedsYou);
    assert_eq!(q4.from.display(), "Priya Raman");
    assert_eq!(q4.thread.account(), f.account.id);

    let messages = f.store.thread_messages(&f.thread("q4")).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].body_text, "Body of 1", "oldest first");
    assert!(messages[1].has_attachments);
    assert_eq!(messages[1].received_at, now() - Duration::minutes(10));
    assert_eq!(messages[1].subject, "Subject 2a");
    assert_eq!(messages[0].from.email().as_str(), "priya@company.com");
    assert_ne!(messages[0].id, messages[1].id);
}

#[test]
fn views_filter_and_count() {
    let f = fixture();
    f.deliver("1", "a", 1, TO_ME);
    f.deliver(
        "2",
        "b",
        2,
        Signals {
            calendar_invite: true,
            ..TO_ME
        },
    );
    f.deliver(
        "3",
        "c",
        3,
        Signals {
            mailing_list: true,
            ..TO_ME
        },
    );
    f.deliver(
        "4",
        "d",
        4,
        Signals {
            mailing_list: true,
            ..TO_ME
        },
    );
    let counts = f.store.counts(None).unwrap();
    assert_eq!(counts.get(TriageView::All), 4);
    assert_eq!(counts.get(TriageView::NeedsYou), 1);
    assert_eq!(counts.get(TriageView::Meetings), 1);
    assert_eq!(counts.get(TriageView::Newsletters), 2);
    assert_eq!(counts.get(TriageView::Updates), 0);
    assert_eq!(
        f.subjects(ThreadQuery {
            view: TriageView::Newsletters,
            ..ThreadQuery::default()
        }),
        ["Subject 3", "Subject 4"]
    );
    assert_eq!(
        f.store
            .counts(Some(AccountId(999)))
            .unwrap()
            .get(TriageView::All),
        0
    );
    assert!(
        f.store
            .threads(ThreadQuery {
                account: Some(AccountId(999)),
                ..ThreadQuery::default()
            })
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unread_filter_and_read_toggle() {
    let f = fixture();
    f.deliver("1", "a", 1, TO_ME);
    f.deliver("2", "b", 2, TO_ME);
    f.store.set_read(&f.thread("a"), true).unwrap();
    let unread = ThreadQuery {
        unread_only: true,
        ..ThreadQuery::default()
    };
    assert_eq!(f.subjects(unread), ["Subject 2"]);
    f.store.set_read(&f.thread("a"), false).unwrap();
    assert_eq!(f.subjects(unread).len(), 2);
}

#[test]
fn redelivery_updates_read_state_without_duplicating() {
    let f = fixture();
    let first = f.deliver("1", "a", 1, TO_ME);
    let again = f
        .store
        .upsert(&IncomingMessage {
            account: f.account.id,
            remote_id: "1".into(),
            thread_key: "a".into(),
            from: Contact::new(None, addr("x@y.com")),
            subject: "ignored".into(),
            body_text: "ignored".into(),
            received_at: now(),
            is_read: true,
            has_attachments: false,
            signals: TO_ME,
            message_id: None,
            references: Vec::new(),
        })
        .unwrap();
    assert_eq!(first, again);
    let threads = f.store.threads(ThreadQuery::default()).unwrap();
    assert_eq!(threads.len(), 1);
    assert!(!threads[0].unread);
    assert_eq!(threads[0].subject, "Subject 1");
}

#[test]
fn done_hides_the_thread_and_undo_restores_it() {
    let f = fixture();
    f.deliver("1", "a", 1, TO_ME);
    f.deliver("2", "b", 2, TO_ME);
    let previous = f
        .store
        .set_state(&[f.thread("a")], TriageState::Done)
        .unwrap();
    assert_eq!(previous, [(f.thread("a"), TriageState::Inbox)]);
    assert_eq!(f.store.state(&f.thread("a")).unwrap(), TriageState::Done);
    assert_eq!(f.subjects(ThreadQuery::default()), ["Subject 2"]);
    f.store.restore(&previous).unwrap();
    assert_eq!(f.subjects(ThreadQuery::default()).len(), 2);
}

#[test]
fn new_mail_reopens_a_done_thread() {
    let f = fixture();
    f.deliver("1", "a", 60, TO_ME);
    f.store
        .set_state(&[f.thread("a")], TriageState::Done)
        .unwrap();
    f.deliver("2", "a", 1, TO_ME);
    assert_eq!(f.store.state(&f.thread("a")).unwrap(), TriageState::Inbox);
    assert_eq!(f.subjects(ThreadQuery::default()), ["Subject 2"]);
}

#[test]
fn snoozed_threads_wake_when_due() {
    let f = fixture();
    f.deliver("1", "a", 1, TO_ME);
    let until = now() + Duration::hours(2);
    f.store
        .set_state(&[f.thread("a")], TriageState::Snoozed { until })
        .unwrap();
    assert_eq!(
        f.store.state(&f.thread("a")).unwrap(),
        TriageState::Snoozed { until }
    );
    assert!(f.subjects(ThreadQuery::default()).is_empty());
    assert_eq!(f.store.wake_snoozed(now()).unwrap(), 0);
    assert_eq!(f.store.wake_snoozed(until).unwrap(), 1);
    assert_eq!(f.store.state(&f.thread("a")).unwrap(), TriageState::Inbox);
}

#[test]
fn opens_on_disk_and_migrates_once() {
    let dir = std::env::temp_dir().join(format!("sift-store-{}", std::process::id()));
    let path = dir.join("nested").join("mail.db");
    {
        let store = MailStore::open(&path).unwrap();
        store
            .add_account("Work", &addr("you@company.com"), Provider::Imap)
            .unwrap();
    }
    let reopened = MailStore::open(&path).unwrap();
    assert_eq!(reopened.accounts().unwrap().len(), 1);
    let version: i64 = reopened
        .conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(usize::try_from(version).unwrap(), schema::latest_version());
    drop(reopened);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(MailStore::default_path().is_some_and(|p| p.ends_with("mail.db")));
}

#[test]
fn creating_the_directory_can_fail() {
    // A file where the directory should be makes create_dir_all fail.
    let blocker = std::env::temp_dir().join(format!("sift-blocker-{}", std::process::id()));
    std::fs::write(&blocker, b"x").unwrap();
    let err = match MailStore::open(&blocker.join("sub").join("mail.db")) {
        Err(e) => e,
        Ok(_) => panic!("opening under a file should fail"),
    };
    assert!(matches!(err, StoreError::CreateDir { .. }));
    assert!(err.to_string().starts_with("could not create"));
    let _ = std::fs::remove_file(&blocker);
}

#[test]
fn corrupt_rows_are_reported_not_panicked() {
    let f = fixture();
    f.deliver("1", "a", 1, TO_ME);
    f.store
        .conn
        .execute("UPDATE messages SET view = 'mystery'", [])
        .unwrap();
    assert!(matches!(
        f.store.threads(ThreadQuery::default()),
        Err(StoreError::Corrupt(_))
    ));
    assert!(matches!(f.store.counts(None), Err(StoreError::Corrupt(_))));

    f.store
        .conn
        .execute(
            "UPDATE messages SET view = 'other', from_email = 'nope'",
            [],
        )
        .unwrap();
    assert!(matches!(
        f.store.threads(ThreadQuery::default()),
        Err(StoreError::Corrupt(_))
    ));
    assert!(matches!(
        f.store.thread_messages(&f.thread("a")),
        Err(StoreError::Corrupt(_))
    ));

    f.store
        .conn
        .execute("UPDATE accounts SET provider = 'aol'", [])
        .unwrap();
    let err = f.store.account(f.account.id).unwrap_err();
    assert!(err.to_string().contains("aol"));

    f.store
        .conn
        .execute(
            "UPDATE messages SET from_email = 'a@b.co', received_at = 9223372036854775807",
            [],
        )
        .unwrap();
    assert!(matches!(
        f.store.thread_messages(&f.thread("a")),
        Err(StoreError::Corrupt(_))
    ));

    let sqlite: StoreError = rusqlite::Error::QueryReturnedNoRows.into();
    assert!(sqlite.to_string().starts_with("database error"));
}

#[test]
fn unknown_thread_state_is_corrupt() {
    let f = fixture();
    f.deliver("1", "a", 1, TO_ME);
    // Bypass the CHECK constraint the way an older or newer build might have written it.
    f.store
        .conn
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    f.store
        .conn
        .execute("UPDATE messages SET state = 'archived'", [])
        .unwrap();
    assert!(matches!(
        f.store.state(&f.thread("a")),
        Err(StoreError::Corrupt(_))
    ));
}
