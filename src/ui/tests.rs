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
        let view = open_main_window(Rc::new(store), cx).unwrap();
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
