//! Headless tests for the inbox chrome: sidebar, account switcher, palette,
//! shortcuts, custom views, bulk selection, row actions and the reader's
//! navigation.

use chrono::Utc;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::SiftApp;
use super::chrome::{Command, Overlay};
use super::tests::{click, open, press, read};
use crate::domain::{Contact, EmailAddress, IncomingMessage, Provider, Signals, TriageView};
use crate::store::Pref;
use crate::theme::{Appearance, SiftTheme};

fn typing(cx: &mut TestAppContext, handle: AnyWindowHandle, text: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.input(text, cx);
    })
    .unwrap();
}

fn hover_click(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    row: usize,
    id: impl Into<gpui_kit::ElementId>,
) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover(("thread-row", row), cx);
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

fn overlay(cx: &mut TestAppContext, view: &Entity<SiftApp>) -> Option<&'static str> {
    read(cx, view, SiftApp::overlay_name)
}

#[gpui_kit::test]
fn sidebar_summaries_and_hold_toggle_and_persist(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    assert!(!read(cx, &view, SiftApp::sidebar_open));
    press(cx, handle, &["["]);
    assert!(read(cx, &view, SiftApp::sidebar_open));
    click(cx, handle, ("sidebar-view", 2usize));
    assert_eq!(read(cx, &view, SiftApp::view), TriageView::Meetings);
    click(cx, handle, "toggle-sidebar");
    assert!(!read(cx, &view, SiftApp::sidebar_open));
    assert!(!read(cx, &view, |a| a
        .store
        .flag(Pref::Sidebar, true)
        .unwrap()));

    press(cx, handle, &["a"]);
    assert!(read(cx, &view, SiftApp::summaries_on));
    click(cx, handle, "show-preview");
    assert!(!read(cx, &view, SiftApp::summaries_on));
    click(cx, handle, "show-summary");
    click(cx, handle, "toggle-summaries");
    assert!(!read(cx, &view, SiftApp::summaries_on));

    click(cx, handle, "notifications");
    assert_eq!(overlay(cx, &view), Some("notifications"));
    click(cx, handle, "hold-switch");
    assert!(read(cx, &view, SiftApp::holding));
    assert!(read(cx, &view, |a| a
        .store
        .flag(Pref::HoldNotifications, false)
        .unwrap()));
    click(cx, handle, "notifications");
    assert_eq!(overlay(cx, &view), None);

    click(cx, handle, "toggle-theme");
    assert_eq!(
        read(cx, &view, |a| a.store.preference(Pref::Appearance).unwrap()),
        Some("light".into())
    );
}

#[gpui_kit::test]
fn the_account_switcher_filters_the_inbox(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    view.read_with(cx, |a, _| {
        let home = a
            .store
            .add_account(
                "Home",
                &EmailAddress::parse("me@home.org").unwrap(),
                Provider::Imap,
            )
            .unwrap();
        a.store
            .upsert(&IncomingMessage {
                account: home.id,
                remote_id: "h1".into(),
                thread_key: "h1".into(),
                from: Contact::new(Some("Mum"), EmailAddress::parse("mum@home.org").unwrap()),
                subject: "Sunday lunch".into(),
                body_text: "Are you coming?".into(),
                received_at: Utc::now(),
                is_read: false,
                has_attachments: false,
                signals: Signals::default(),
                message_id: None,
                references: Vec::new(),
            })
            .unwrap();
    });
    click(cx, handle, "filter-all");
    assert_eq!(read(cx, &view, SiftApp::listed_subjects).len(), 9);
    click(cx, handle, "account-switcher");
    assert_eq!(overlay(cx, &view), Some("accounts"));
    click(cx, handle, ("switch-account", 1usize));
    assert_eq!(overlay(cx, &view), None);
    assert_eq!(read(cx, &view, SiftApp::listed_subjects), ["Sunday lunch"]);
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 1);
    press(cx, handle, &["["]);
    click(cx, handle, ("sidebar-account", 0usize));
    assert_eq!(read(cx, &view, SiftApp::listed_subjects).len(), 8);
    click(cx, handle, "sidebar-all");
    assert_eq!(read(cx, &view, SiftApp::account_filter), None);
    click(cx, handle, "account-switcher");
    click(cx, handle, "switch-all");
    assert_eq!(read(cx, &view, SiftApp::listed_subjects).len(), 9);
    click(cx, handle, "sidebar-add-account");
    assert!(read(cx, &view, |a| a.form.is_some()));
}

#[gpui_kit::test]
fn the_palette_searches_mail_and_runs_commands(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["ctrl-k"]);
    assert_eq!(overlay(cx, &view), Some("palette"));
    let all = read(cx, &view, SiftApp::palette_labels);
    assert_eq!(all.len(), Command::ALL.len(), "commands only when empty");
    typing(cx, handle, "vendor");
    assert_eq!(
        read(cx, &view, SiftApp::palette_labels),
        ["Vendor contract renewal"]
    );
    press(cx, handle, &["enter"]);
    assert_eq!(overlay(cx, &view), None);
    assert_eq!(
        read(cx, &view, |a| a.open_subject().map(str::to_owned)),
        Some("Vendor contract renewal".into())
    );

    press(cx, handle, &["escape", "/"]);
    assert_eq!(overlay(cx, &view), Some("palette"));
    typing(cx, handle, "go to");
    press(cx, handle, &["down", "down", "up", "down"]);
    assert_eq!(read(cx, &view, SiftApp::palette_selected), Some(1));
    press(cx, handle, &["enter"]);
    assert_eq!(read(cx, &view, SiftApp::view), TriageView::Meetings);

    press(cx, handle, &["ctrl-k"]);
    typing(cx, handle, "zzzz");
    assert!(read(cx, &view, SiftApp::palette_labels).is_empty());
    press(cx, handle, &["enter", "ctrl-k"]);
    assert_eq!(overlay(cx, &view), None, "ctrl-k closes it again");

    click(cx, handle, "open-palette");
    click(cx, handle, ("palette-item", 0usize));
    assert!(
        read(cx, &view, |a| a.compose.is_some()),
        "the first command is New message"
    );
    press(cx, handle, &["escape"]);
    click(cx, handle, "dock-palette");
    click(cx, handle, "overlay-scrim");
    assert_eq!(overlay(cx, &view), None);
}

#[gpui_kit::test]
fn every_command_runs(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    for command in Command::ALL {
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |app, cx| {
                app.compose = None;
                app.form = None;
                app.overlay = None;
                app.run_command(command, window, cx);
            });
        })
        .unwrap();
    }
    assert!(read(cx, &view, SiftApp::sidebar_open));
    assert!(read(cx, &view, SiftApp::summaries_on));
    assert!(
        read(cx, &view, SiftApp::settings_open),
        "the last command opens Settings"
    );
    for expected in [Appearance::Light, Appearance::Dark] {
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |app, cx| {
                app.run_command(Command::ToggleTheme, window, cx);
            });
        })
        .unwrap();
        let _ = expected;
    }
    cx.update(|cx| {
        assert_ne!(SiftTheme::get(cx).appearance(), Appearance::System);
    });
}

#[gpui_kit::test]
fn shortcuts_sheet_opens_and_closes(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["?"]);
    assert_eq!(overlay(cx, &view), Some("shortcuts"));
    press(cx, handle, &["j"]);
    assert_eq!(
        read(cx, &view, SiftApp::cursor),
        0,
        "keys are blocked under a sheet"
    );
    press(cx, handle, &["escape"]);
    assert_eq!(overlay(cx, &view), None);
    click(cx, handle, "open-shortcuts");
    click(cx, handle, "close-shortcuts");
    assert_eq!(overlay(cx, &view), None);
}

#[gpui_kit::test]
fn custom_views_are_created_used_and_removed(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, "new-view");
    assert_eq!(overlay(cx, &view), Some("new-view"));
    typing(cx, handle, "the");
    click(cx, handle, "create-view");
    assert!(read(cx, &view, |a| matches!(
        &a.overlay,
        Some(Overlay::NewView(f)) if f.error.is_some()
    )));
    click(cx, handle, ("view-idea", 0usize));
    click(cx, handle, "create-view");
    assert_eq!(overlay(cx, &view), None);
    assert_eq!(read(cx, &view, SiftApp::custom_view_labels), ["Offsite"]);
    assert_eq!(read(cx, &view, SiftApp::list_title), "Offsite");
    assert_eq!(
        read(cx, &view, SiftApp::listed_subjects),
        ["Re: Offsite planning"]
    );
    press(cx, handle, &["1"]);
    assert_eq!(read(cx, &view, SiftApp::list_title), "All");
    press(cx, handle, &["7"]);
    assert_eq!(read(cx, &view, SiftApp::list_title), "Offsite");
    press(cx, handle, &["8"]);
    assert_eq!(
        read(cx, &view, SiftApp::list_title),
        "Offsite",
        "there is no second custom view"
    );
    click(cx, handle, "filter-unread");
    click(cx, handle, "filter-all");
    click(cx, handle, TriageView::All.id());
    click(cx, handle, ("custom-view", 0usize));
    click(cx, handle, "remove-view");
    assert!(read(cx, &view, SiftApp::custom_view_labels).is_empty());
    assert_eq!(read(cx, &view, SiftApp::view), TriageView::All);

    click(cx, handle, "new-view");
    typing(cx, handle, "travel");
    press(cx, handle, &["enter"]);
    assert_eq!(read(cx, &view, SiftApp::custom_view_labels), ["Travel"]);
    click(cx, handle, "new-view");
    click(cx, handle, "cancel-view");
    assert_eq!(overlay(cx, &view), None);
}

#[gpui_kit::test]
fn bulk_selection_acts_on_every_ticked_thread(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["x", "j", "x"]);
    assert_eq!(read(cx, &view, |a| a.selection().len()), 2);
    press(cx, handle, &["x"]);
    assert_eq!(read(cx, &view, |a| a.selection().len()), 1);
    hover_click(cx, handle, 3, ("select-row", 3usize));
    click(cx, handle, "bulk-done");
    assert_eq!(
        read(cx, &view, |a| a.toast_text().map(str::to_owned)),
        Some("2 conversations marked done".into())
    );
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 6);
    assert!(read(cx, &view, |a| a.selection().is_empty()));

    click(cx, handle, "select-all");
    assert_eq!(read(cx, &view, |a| a.selection().len()), 6);
    click(cx, handle, "bulk-snooze");
    assert!(read(cx, &view, SiftApp::is_snoozing));
    press(cx, handle, &["escape"]);
    click(cx, handle, "bulk-read");
    click(cx, handle, "filter-unread");
    assert!(read(cx, &view, SiftApp::listed_subjects).is_empty());
    click(cx, handle, "filter-all");

    click(cx, handle, "select-all");
    click(cx, handle, "select-all");
    assert!(
        read(cx, &view, |a| a.selection().is_empty()),
        "a second click clears"
    );
    press(cx, handle, &["x", "escape"]);
    assert!(read(cx, &view, |a| a.selection().is_empty()));
    press(cx, handle, &["x"]);
    click(cx, handle, "clear-selection");
    assert!(read(cx, &view, |a| a.selection().is_empty()));
}

#[gpui_kit::test]
fn row_actions_and_mark_all_read(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let first = read(cx, &view, SiftApp::listed_subjects)[0].clone();
    hover_click(cx, handle, 0, ("row-done", 0usize));
    assert!(!read(cx, &view, SiftApp::listed_subjects).contains(&first));
    assert!(
        read(cx, &view, |a| a.open_subject().is_none()),
        "row buttons don't open the thread"
    );
    hover_click(cx, handle, 0, ("row-snooze", 0usize));
    assert!(read(cx, &view, SiftApp::is_snoozing));
    press(cx, handle, &["4"]);
    assert_eq!(read(cx, &view, |a| a.count(TriageView::All)), 6);
    hover_click(cx, handle, 1, ("row-read", 1usize));
    click(cx, handle, "mark-all-read");
    click(cx, handle, "filter-unread");
    assert!(read(cx, &view, SiftApp::listed_subjects).is_empty());
}

#[gpui_kit::test]
fn the_reader_steps_and_expands_earlier_messages(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let index = read(cx, &view, |a| {
        a.listed_subjects()
            .iter()
            .position(|s| s == "Re: Q4 budget numbers")
            .unwrap()
    });
    click(cx, handle, ("thread-row", index));
    click(cx, handle, ("expand-message", 0usize));
    assert_eq!(read(cx, &view, |a| a.expanded.len()), 1);
    click(cx, handle, ("message-header", 0usize));
    assert!(read(cx, &view, |a| a.expanded.is_empty()));
    click(cx, handle, "next-thread");
    assert_eq!(read(cx, &view, SiftApp::cursor), index + 1);
    click(cx, handle, "prev-thread");
    assert_eq!(read(cx, &view, SiftApp::cursor), index);
}

#[gpui_kit::test]
fn file_chips_open_saved_attachments(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let index = read(cx, &view, |a| {
        a.listed_subjects()
            .iter()
            .position(|s| s == "Re: Q4 budget numbers")
            .unwrap()
    });
    click(cx, handle, ("thread-row", index));
    click(cx, handle, ("attachment", 1usize));
    assert!(
        read(cx, &view, |a| a.error.clone())
            .unwrap()
            .contains("wasn't downloaded")
    );
    view.update(cx, |app, _| {
        app.error = None;
        app.opener = |path| {
            assert!(path.ends_with("budget.xlsx"));
            Ok(())
        };
        if let Some(open) = &mut app.open {
            for files in &mut open.files {
                for file in files {
                    file.path = Some("budget.xlsx".into());
                }
            }
        }
    });
    click(cx, handle, ("attachment", 1usize));
    assert_eq!(read(cx, &view, |a| a.error.clone()), None);
    view.update(cx, |app, _| {
        app.opener = |_| Err(std::io::Error::other("no app for .xlsx"));
    });
    click(cx, handle, ("attachment", 1usize));
    assert!(
        read(cx, &view, |a| a.error.clone())
            .unwrap()
            .contains("no app for .xlsx")
    );
}

#[gpui_kit::test]
fn a_start_screen_can_be_named(cx: &mut TestAppContext) {
    let (_, view) = open(cx, true);
    cx.update(|cx| super::open_start_screen(&view, "nothing", cx));
    assert!(!read(cx, &view, SiftApp::calendar_open));
    cx.update(|cx| super::open_start_screen(&view, "calendar", cx));
    assert!(read(cx, &view, SiftApp::calendar_open));
    cx.update(|cx| super::open_start_screen(&view, "settings", cx));
    assert!(read(cx, &view, SiftApp::settings_open));
}
