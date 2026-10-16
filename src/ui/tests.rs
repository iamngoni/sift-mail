//! Headless UI tests: open the real main window over an in-memory store and
//! drive it with clicks and key presses, the way a user would.

use std::rc::Rc;

use chrono::Utc;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{SiftApp, open_main_window};
use crate::domain::TriageView;
use crate::store::{MailStore, demo};
use crate::theme::{Appearance, SiftTheme};

fn open(cx: &mut TestAppContext, seeded: bool) -> (AnyWindowHandle, Entity<SiftApp>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let store = MailStore::open_in_memory().unwrap();
        if seeded {
            demo::seed(&store, Utc::now()).unwrap();
        }
        let view = open_main_window(Rc::new(store), None, cx).unwrap();
        let handle = *cx.windows().first().unwrap();
        (handle, view)
    })
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, keys: &[&str]) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for key in keys {
            window.press(key, cx);
        }
    })
    .unwrap();
}

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

fn read<R>(cx: &mut TestAppContext, view: &Entity<SiftApp>, f: impl FnOnce(&SiftApp) -> R) -> R {
    view.read_with(cx, |app, _| f(app))
}

#[gpui_kit::test]
fn lists_seeded_threads_newest_first_with_counts(cx: &mut TestAppContext) {
    let (_, view) = open(cx, true);
    let subjects = read(cx, &view, SiftApp::listed_subjects);
    assert_eq!(subjects.len(), 8);
    assert_eq!(subjects[0], "Invitation: Roadmap sync");
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 8);
    assert_eq!(read(cx, &view, |a| a.count(TriageView::Meetings)), 1);
}

#[gpui_kit::test]
fn tabs_and_number_keys_filter_the_list(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, TriageView::Newsletters.id());
    assert_eq!(read(cx, &view, SiftApp::view), TriageView::Newsletters);
    assert_eq!(
        read(cx, &view, SiftApp::listed_subjects),
        ["Issue 112: pricing pages that convert"]
    );
    press(cx, handle, &["3"]);
    assert_eq!(read(cx, &view, SiftApp::view), TriageView::Meetings);
    press(cx, handle, &["9", "ctrl-2"]);
    assert_eq!(
        read(cx, &view, SiftApp::view),
        TriageView::Meetings,
        "unbound keys are ignored"
    );
}

#[gpui_kit::test]
fn unread_filter_hides_read_threads(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, "filter-unread");
    let unread = read(cx, &view, SiftApp::listed_subjects);
    assert!(!unread.contains(&"Who owns the onboarding doc?".to_owned()));
    click(cx, handle, "filter-all");
    assert_eq!(read(cx, &view, SiftApp::listed_subjects).len(), 8);
}

#[gpui_kit::test]
fn keyboard_moves_opens_and_closes_threads(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["j", "j", "k", "down"]);
    assert_eq!(read(cx, &view, SiftApp::cursor), 2);
    press(cx, handle, &["k", "k", "k"]);
    assert_eq!(
        read(cx, &view, SiftApp::cursor),
        0,
        "cursor stops at the top"
    );
    press(cx, handle, &["enter"]);
    assert_eq!(
        read(cx, &view, |a| a.open_subject().map(str::to_owned)),
        Some("Invitation: Roadmap sync".to_owned())
    );
    press(cx, handle, &["j"]);
    assert_eq!(
        read(cx, &view, SiftApp::cursor),
        1,
        "j steps to the next thread while reading"
    );
    assert!(read(cx, &view, |a| a.open_subject().is_some()));
    press(cx, handle, &["escape"]);
    assert_eq!(
        read(cx, &view, |a| a.open_subject().map(str::to_owned)),
        None
    );
}

#[gpui_kit::test]
fn clicking_a_row_opens_it_and_back_returns(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, ("thread-row", 3usize));
    assert_eq!(read(cx, &view, SiftApp::cursor), 3);
    assert!(read(cx, &view, |a| a.open_subject().is_some()));
    click(cx, handle, "back");
    assert!(read(cx, &view, |a| a.open_subject().is_none()));
}

#[gpui_kit::test]
fn done_hides_the_thread_and_undo_brings_it_back(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let first = read(cx, &view, SiftApp::listed_subjects)[0].clone();
    press(cx, handle, &["e"]);
    assert_eq!(
        read(cx, &view, |a| a.toast_text().map(str::to_owned)),
        Some("Marked done".into())
    );
    assert!(!read(cx, &view, SiftApp::listed_subjects).contains(&first));
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 7);
    click(cx, handle, "undo");
    assert!(read(cx, &view, SiftApp::listed_subjects).contains(&first));
    assert_eq!(read(cx, &view, |a| a.toast_text().map(str::to_owned)), None);
}

#[gpui_kit::test]
fn done_while_reading_moves_to_the_next_thread(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["enter"]);
    let before = read(cx, &view, |a| a.open_subject().map(str::to_owned));
    click(cx, handle, "done");
    let after = read(cx, &view, |a| a.open_subject().map(str::to_owned));
    assert!(after.is_some());
    assert_ne!(before, after);
    press(cx, handle, &["z"]);
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 8);
}

#[gpui_kit::test]
fn snooze_menu_by_key_and_by_click(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["s"]);
    assert!(read(cx, &view, SiftApp::is_snoozing));
    press(cx, handle, &["7"]);
    assert!(
        read(cx, &view, SiftApp::is_snoozing),
        "an option that doesn't exist keeps the menu"
    );
    press(cx, handle, &["2"]);
    assert!(!read(cx, &view, SiftApp::is_snoozing));
    assert!(read(cx, &view, |a| a
        .toast_text()
        .is_some_and(|t| t.starts_with("Snoozed until"))));
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 7);

    // The Snooze button lives in the reader, so open a thread first.
    press(cx, handle, &["enter"]);
    click(cx, handle, "snooze");
    assert!(read(cx, &view, SiftApp::is_snoozing));
    click(cx, handle, ("snooze-option", 0usize));
    assert!(!read(cx, &view, SiftApp::is_snoozing));
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 6);

    press(cx, handle, &["s", "escape"]);
    assert!(!read(cx, &view, SiftApp::is_snoozing));
    press(cx, handle, &["s"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Top-left corner: on the scrim, away from the menu.
        window.click_at(
            "snooze-scrim",
            gpui_kit::point(gpui_kit::px(8.0), gpui_kit::px(8.0)),
            cx,
        );
    })
    .unwrap();
    assert!(
        !read(cx, &view, SiftApp::is_snoozing),
        "clicking outside cancels"
    );
}

#[gpui_kit::test]
fn read_state_toggles_from_keyboard_and_reader(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, "filter-unread");
    let unread_before = read(cx, &view, SiftApp::listed_subjects).len();
    press(cx, handle, &["u"]);
    assert_eq!(
        read(cx, &view, SiftApp::listed_subjects).len(),
        unread_before - 1
    );
    click(cx, handle, "filter-all");
    press(cx, handle, &["enter"]);
    click(cx, handle, "unread");
    assert!(
        read(cx, &view, |a| a.open_subject().is_none()),
        "marking unread closes the reader"
    );
}

#[gpui_kit::test]
fn toast_can_be_dismissed(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["e"]);
    click(cx, handle, "dismiss-toast");
    assert!(read(cx, &view, |a| a.toast_text().is_none()));
    press(cx, handle, &["z"]);
    assert_eq!(
        read(cx, &view, |a| a.count(TriageView::All)),
        7,
        "nothing to undo after dismiss"
    );
}

#[gpui_kit::test]
fn empty_store_shows_the_welcome_empty_state(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, false);
    assert!(read(cx, &view, SiftApp::listed_subjects).is_empty());
    // Actions with nothing selected are harmless no-ops.
    press(cx, handle, &["j", "enter", "e", "s", "u", "z"]);
    assert!(read(cx, &view, |a| a.open_subject().is_none()
        && !a.is_snoozing()));
}

#[gpui_kit::test]
fn theme_button_cycles_appearance(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, false);
    for expected in [Appearance::Light, Appearance::Dark, Appearance::System] {
        click(cx, handle, "toggle-theme");
        cx.update(|cx| assert_eq!(SiftTheme::get(cx).appearance(), expected));
    }
}

#[gpui_kit::test]
fn inbox_zero_after_clearing_a_view(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["3", "e"]);
    assert!(read(cx, &view, SiftApp::listed_subjects).is_empty());
    press(cx, handle, &["1"]);
    click(cx, handle, "filter-unread");
    for _ in 0..8 {
        press(cx, handle, &["e"]);
    }
    assert!(read(cx, &view, SiftApp::listed_subjects).is_empty());
}

#[test]
fn default_store_opens() {
    let store = super::open_default_store().unwrap();
    assert!(store.accounts().is_ok());
}

// ---------- accounts and sync ----------

mod sync_ui {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::sync::imap::{MailSession, MailboxInfo, RawMessage, SyncError};
    use crate::sync::secrets::MemorySecrets;
    use crate::sync::service::{BoxedSession, Connector, SyncService};

    struct Inbox;

    impl MailSession for Inbox {
        fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError> {
            Ok(MailboxInfo {
                uid_validity: Some(1),
            })
        }
        fn uid_search(&mut self, _: &str) -> Result<Vec<u32>, SyncError> {
            Ok(vec![1])
        }
        fn uid_fetch(&mut self, _: &[u32]) -> Result<Vec<RawMessage>, SyncError> {
            Ok(vec![RawMessage {
                uid: 1,
                seen: false,
                internal_date: None,
                body: b"From: Lena <lena@fastmail.com>\r\nTo: you@fastmail.com\r\nSubject: Hello from IMAP\r\nMessage-ID: <h@x>\r\n\r\nHi!\r\n".to_vec(),
            }])
        }
        fn set_seen(&mut self, _: &[u32], _: bool) -> Result<(), SyncError> {
            Ok(())
        }
        fn append_sent(&mut self, _: &[u8]) -> Result<bool, SyncError> {
            Ok(true)
        }
    }

    fn temp_db(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sift-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("mail.db")
    }

    pub(super) fn open_synced_pub(
        cx: &mut TestAppContext,
        db: &std::path::Path,
    ) -> (AnyWindowHandle, Entity<SiftApp>) {
        open_synced(cx, db)
    }

    fn open_synced(
        cx: &mut TestAppContext,
        db: &std::path::Path,
    ) -> (AnyWindowHandle, Entity<SiftApp>) {
        let connector: Connector = Arc::new(|_, password: &str| {
            if password == "right" {
                Ok(Box::new(Inbox) as BoxedSession)
            } else {
                Err(SyncError::Auth)
            }
        });
        let service = SyncService::new(
            db.to_path_buf(),
            Arc::new(MemorySecrets::default()),
            connector,
            Arc::new(|_, _, _, message| {
                let text = String::from_utf8(message.formatted()).unwrap_or_default();
                if text.contains("To: fail@") {
                    Err(SyncError::Protocol("550 rejected".into()))
                } else {
                    Ok(())
                }
            }),
        );
        let db = db.to_path_buf();
        cx.update(|cx| {
            gpui_kit::init(cx);
            let store = MailStore::open(&db).unwrap();
            let view = open_main_window(Rc::new(store), Some(service), cx).unwrap();
            let handle = *cx.windows().first().unwrap();
            (handle, view)
        })
    }

    fn fill(
        cx: &mut TestAppContext,
        handle: AnyWindowHandle,
        view: &Entity<SiftApp>,
        email: &str,
        password: &str,
        server: &str,
    ) {
        let view = view.clone();
        cx.update_window(handle, |_, window, cx| {
            let (e, p, s) = view.read_with(cx, |app, _| {
                let f = app.form.as_ref().unwrap();
                (f.email.clone(), f.password.clone(), f.server.clone())
            });
            e.update(cx, |state, cx| {
                state.set_value(email.to_owned(), window, cx)
            });
            p.update(cx, |state, cx| {
                state.set_value(password.to_owned(), window, cx)
            });
            s.update(cx, |state, cx| {
                state.set_value(server.to_owned(), window, cx)
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn adding_an_imap_account_fetches_its_mail(cx: &mut TestAppContext) {
        let db = temp_db("add");
        let (handle, view) = open_synced(cx, &db);
        cx.run_until_parked();
        assert_eq!(
            read(cx, &view, |a| a.sync_status().map(str::to_owned)),
            Some("Up to date".into())
        );

        click(cx, handle, "add-account");
        assert!(read(cx, &view, SiftApp::is_adding_account));
        // Typing in the dialog must not trigger inbox shortcuts.
        press(cx, handle, &["e", "3"]);
        assert_eq!(read(cx, &view, SiftApp::view), TriageView::All);

        fill(cx, handle, &view, "you@company.com", "right", "");
        click(cx, handle, "connect");
        assert!(read(cx, &view, |a| a
            .form_error()
            .is_some_and(|e| e.contains("browser sign-in"))));

        fill(cx, handle, &view, "you@fastmail.com", "wrong", "");
        click(cx, handle, "connect");
        cx.run_until_parked();
        assert!(read(cx, &view, |a| a
            .form_error()
            .is_some_and(|e| e.contains("rejected the sign-in"))));

        fill(cx, handle, &view, "you@fastmail.com", "right", "");
        click(cx, handle, "connect");
        cx.run_until_parked();
        assert!(!read(cx, &view, SiftApp::is_adding_account));
        assert_eq!(
            read(cx, &view, SiftApp::listed_subjects),
            ["Hello from IMAP"]
        );
        assert_eq!(
            read(cx, &view, |a| a.sync_status().map(str::to_owned)),
            Some("Synced · 1 new".into())
        );

        click(cx, handle, "sync-now");
        cx.run_until_parked();
        assert_eq!(
            read(cx, &view, |a| a.sync_status().map(str::to_owned)),
            Some("Up to date".into())
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[gpui_kit::test]
    fn the_dialog_closes_with_cancel_or_escape(cx: &mut TestAppContext) {
        let db = temp_db("cancel");
        let (handle, view) = open_synced(cx, &db);
        click(cx, handle, "add-account");
        click(cx, handle, "cancel-add");
        assert!(!read(cx, &view, SiftApp::is_adding_account));
        click(cx, handle, "add-account");
        press(cx, handle, &["escape"]);
        assert!(!read(cx, &view, SiftApp::is_adding_account));
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[gpui_kit::test]
    fn without_a_sync_service_accounts_cannot_be_saved(cx: &mut TestAppContext) {
        let (handle, view) = open(cx, false);
        assert!(read(cx, &view, |a| a.sync_status().is_none() && !a.is_syncing()));
        click(cx, handle, "add-account");
        fill(cx, handle, &view, "you@fastmail.com", "right", "");
        click(cx, handle, "connect");
        assert!(read(cx, &view, |a| a
            .form_error()
            .is_some_and(|e| e.contains("can't save"))));
    }
}

mod compose_ui {
    use super::*;

    fn add_account(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<SiftApp>) {
        click(cx, handle, "add-account");
        sync_ui_fill(cx, handle, view, "you@fastmail.com", "right", "");
        click(cx, handle, "connect");
        cx.run_until_parked();
    }

    fn sync_ui_fill(
        cx: &mut TestAppContext,
        handle: AnyWindowHandle,
        view: &Entity<SiftApp>,
        email: &str,
        password: &str,
        server: &str,
    ) {
        let view = view.clone();
        cx.update_window(handle, |_, window, cx| {
            let (e, p, s) = view.read_with(cx, |app, _| {
                let f = app.form.as_ref().unwrap();
                (f.email.clone(), f.password.clone(), f.server.clone())
            });
            e.update(cx, |state, cx| {
                state.set_value(email.to_owned(), window, cx)
            });
            p.update(cx, |state, cx| {
                state.set_value(password.to_owned(), window, cx)
            });
            s.update(cx, |state, cx| {
                state.set_value(server.to_owned(), window, cx)
            });
        })
        .unwrap();
    }

    fn type_reply(
        cx: &mut TestAppContext,
        handle: AnyWindowHandle,
        view: &Entity<SiftApp>,
        text: &str,
    ) {
        let view = view.clone();
        cx.update_window(handle, |_, window, cx| {
            let reply = view.read_with(cx, |app, _| app.reply_box.clone().unwrap());
            reply.update(cx, |state, cx| state.set_value(text.to_owned(), window, cx));
        })
        .unwrap();
    }

    fn compose(
        cx: &mut TestAppContext,
        handle: AnyWindowHandle,
        view: &Entity<SiftApp>,
        to: &str,
        subject: &str,
        body: &str,
    ) {
        let view = view.clone();
        cx.update_window(handle, |_, window, cx| {
            let (t, s, b) = view.read_with(cx, |app, _| {
                let c = app.compose.as_ref().unwrap();
                (c.to.clone(), c.subject.clone(), c.body.clone())
            });
            t.update(cx, |state, cx| state.set_value(to.to_owned(), window, cx));
            s.update(cx, |state, cx| {
                state.set_value(subject.to_owned(), window, cx)
            });
            b.update(cx, |state, cx| state.set_value(body.to_owned(), window, cx));
        })
        .unwrap();
    }

    fn temp_db(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sift-compose-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("mail.db")
    }

    #[gpui_kit::test]
    fn replying_sends_and_marks_the_thread_done(cx: &mut TestAppContext) {
        let db = temp_db("reply");
        let (handle, view) = super::sync_ui::open_synced_pub(cx, &db);
        add_account(cx, handle, &view);
        assert_eq!(
            read(cx, &view, SiftApp::listed_subjects),
            ["Hello from IMAP"]
        );

        press(cx, handle, &["enter", "r"]);
        cx.run_until_parked();
        // Shortcut keys typed into the reply box stay in the reply box.
        press(cx, handle, &["e", "s", "2"]);
        assert!(read(cx, &view, |a| a.open_subject().is_some()
            && !a.is_snoozing()));

        type_reply(cx, handle, &view, "   ");
        press(cx, handle, &["ctrl-enter"]);
        assert_eq!(
            read(cx, &view, |a| a.compose_error().map(str::to_owned)),
            Some("Write something before sending.".into())
        );

        type_reply(cx, handle, &view, "Thanks Lena!");
        click(cx, handle, "send-reply");
        cx.run_until_parked();
        assert_eq!(
            read(cx, &view, |a| a.toast_text().map(str::to_owned)),
            Some("Reply sent to Lena · thread done".into())
        );
        assert!(read(cx, &view, |a| a.open_subject().is_none()));
        assert!(
            read(cx, &view, SiftApp::listed_subjects).is_empty(),
            "replied thread is done"
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[gpui_kit::test]
    fn escape_leaves_the_reply_box(cx: &mut TestAppContext) {
        let db = temp_db("escape");
        let (handle, view) = super::sync_ui::open_synced_pub(cx, &db);
        add_account(cx, handle, &view);
        press(cx, handle, &["enter", "r", "escape", "e"]);
        assert!(
            read(cx, &view, SiftApp::listed_subjects).is_empty(),
            "E works again after Esc"
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[gpui_kit::test]
    fn composing_a_new_message(cx: &mut TestAppContext) {
        let db = temp_db("new");
        let (handle, view) = super::sync_ui::open_synced_pub(cx, &db);

        press(cx, handle, &["c"]);
        assert!(read(cx, &view, SiftApp::is_composing));
        compose(cx, handle, &view, "lena@company.com", "Hi", "Hello");
        click(cx, handle, "compose-send");
        assert_eq!(
            read(cx, &view, |a| a.compose_error().map(str::to_owned)),
            Some("Add an account before sending mail.".into())
        );
        click(cx, handle, "compose-cancel");
        assert!(!read(cx, &view, SiftApp::is_composing));

        add_account(cx, handle, &view);
        click(cx, handle, "new-message");
        compose(cx, handle, &view, "", "Hi", "Hello");
        press(cx, handle, &["ctrl-enter"]);
        assert_eq!(
            read(cx, &view, |a| a.compose_error().map(str::to_owned)),
            Some("Add at least one recipient.".into())
        );

        compose(cx, handle, &view, "fail@company.com", "Hi", "Hello");
        click(cx, handle, "compose-send");
        cx.run_until_parked();
        assert!(read(cx, &view, |a| a
            .compose_error()
            .is_some_and(|e| e.contains("550 rejected"))));
        assert!(
            read(cx, &view, SiftApp::is_composing),
            "the draft is kept after a failure"
        );

        compose(
            cx,
            handle,
            &view,
            "lena@company.com, tom@company.com",
            "Hi",
            "Hello",
        );
        click(cx, handle, "compose-send");
        assert!(read(cx, &view, SiftApp::is_sending));
        cx.run_until_parked();
        assert!(!read(cx, &view, SiftApp::is_composing));
        assert_eq!(
            read(cx, &view, |a| a.toast_text().map(str::to_owned)),
            Some("Sent to lena, tom".into())
        );
        press(cx, handle, &["c", "escape"]);
        assert!(!read(cx, &view, SiftApp::is_composing));
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[gpui_kit::test]
    fn without_a_service_mail_cannot_be_sent(cx: &mut TestAppContext) {
        let (handle, view) = open(cx, true);
        press(cx, handle, &["enter", "r"]);
        type_reply(cx, handle, &view, "Hi");
        click(cx, handle, "send-reply");
        assert!(read(cx, &view, |a| a
            .compose_error()
            .is_some_and(|e| e.contains("can't send"))));
    }
}
