//! Headless tests for first-run onboarding: when it shows, connecting an
//! account from it, every choice being saved, keyboard practice, and the
//! summary.

use std::rc::Rc;

use chrono::Utc;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::onboarding::OnboardStep;
use super::tests::compose_ui::{click_low, set_flow_field};
use super::tests::sync_ui::{open_first_run, temp_db};
use super::tests::{click, open, press, read};
use super::{SiftApp, open_main_window};
use crate::domain::TriageView;
use crate::domain::choices::{AiFeature, NotifyMode, SortMode};
use crate::store::{MailStore, Pref, demo};

fn step(cx: &mut TestAppContext, view: &Entity<SiftApp>) -> Option<OnboardStep> {
    read(cx, view, SiftApp::onboarding_step)
}

fn pref(cx: &mut TestAppContext, view: &Entity<SiftApp>, p: Pref) -> Option<String> {
    read(cx, view, move |a| a.store.preference(p).unwrap())
}

/// Scrolls onboarding to the bottom, then clicks.
fn click_footer(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<gpui_kit::ElementId>,
) {
    use gpui_kit::test::TestWindowExt as _;
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            "onboarding",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.0),
                gpui_kit::px(-2000.0),
            )),
            cx,
        );
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_first_run_walks_through_every_step(cx: &mut TestAppContext) {
    let db = temp_db("onboarding");
    let (handle, view) = open_first_run(cx, &db);
    assert_eq!(step(cx, &view), Some(OnboardStep::Welcome));
    press(cx, handle, &["e", "3"]);
    assert_eq!(
        read(cx, &view, SiftApp::view),
        TriageView::All,
        "inbox keys are off"
    );
    click(cx, handle, "lets-sift");
    assert_eq!(step(cx, &view), Some(OnboardStep::Connect));
    press(cx, handle, &["enter"]);
    assert_eq!(
        step(cx, &view),
        Some(OnboardStep::Connect),
        "needs an account first"
    );

    // A provider opens the Add account flow on its sign-in step; it can be closed here.
    click(cx, handle, ("onboard-provider", 3usize));
    assert_eq!(
        read(cx, &view, SiftApp::add_step),
        Some(super::add_account::Step::Imap)
    );
    press(cx, handle, &["escape"]);
    assert!(!read(cx, &view, SiftApp::is_adding_account));

    click(cx, handle, "onboard-type-it");
    set_flow_field(cx, handle, &view, |f| f.email.clone(), "you@fastmail.com");
    click(cx, handle, "continue-email");
    set_flow_field(cx, handle, &view, |f| f.password.clone(), "right");
    click(cx, handle, "connect");
    cx.run_until_parked();
    click_low(cx, handle, "setup-connect");
    cx.run_until_parked();
    click_low(cx, handle, "go-to-inbox");
    assert!(!read(cx, &view, SiftApp::is_adding_account));
    assert_eq!(
        step(cx, &view),
        Some(OnboardStep::Connect),
        "back to onboarding"
    );
    assert_eq!(read(cx, &view, |a| a.accounts.len()), 1);
    click_footer(cx, handle, "onboard-next");

    assert_eq!(step(cx, &view), Some(OnboardStep::Sort));
    click(cx, handle, ("onboard-sort", 1usize));
    assert_eq!(read(cx, &view, |a| a.sort_mode), SortMode::Plain);
    assert_eq!(pref(cx, &view, Pref::SortMode), Some("plain".into()));
    click(cx, handle, ("onboard-sort", 0usize));
    click(cx, handle, ("onboard-view", 4usize));
    click(cx, handle, ("onboard-view", 0usize));
    assert_eq!(
        pref(cx, &view, Pref::Views),
        Some("meetings,updates,newsletters,other".into())
    );
    click_footer(cx, handle, "onboard-back");
    assert_eq!(step(cx, &view), Some(OnboardStep::Connect));
    click_footer(cx, handle, "onboard-next");
    click_footer(cx, handle, "onboard-next");

    assert_eq!(step(cx, &view), Some(OnboardStep::Ai));
    click(cx, handle, ("onboard-ai", 1usize));
    assert!(!read(cx, &view, |a| a.ai_features.on(AiFeature::Drafts)));
    assert_eq!(
        pref(cx, &view, Pref::AiFeatures),
        Some("summaries,ask,tasks".into())
    );
    press(cx, handle, &["enter"]);

    assert_eq!(step(cx, &view), Some(OnboardStep::Focus));
    click(cx, handle, ("onboard-notify", 1usize));
    assert!(!read(cx, &view, SiftApp::holding));
    click(cx, handle, ("onboard-notify", 2usize));
    assert_eq!(read(cx, &view, |a| a.notify_mode), NotifyMode::Never);
    click(cx, handle, ("onboard-notify", 0usize));
    assert!(read(cx, &view, SiftApp::holding));
    click(cx, handle, ("onboard-checkin", 0usize));
    assert_eq!(
        pref(cx, &view, Pref::Checkins),
        Some("09:00,12:00,17:00".into())
    );
    click_footer(cx, handle, "onboard-skip");

    assert_eq!(step(cx, &view), Some(OnboardStep::Look));
    click(cx, handle, ("onboard-theme", 1usize));
    assert_eq!(pref(cx, &view, Pref::Appearance), Some("dark".into()));
    click(cx, handle, ("onboard-theme", 2usize));
    click(cx, handle, ("onboard-theme", 0usize));
    click(cx, handle, "onboard-snug");
    assert!(read(cx, &view, |a| a.compact));
    click(cx, handle, "onboard-roomy");
    press(cx, handle, &["enter"]);

    assert_eq!(step(cx, &view), Some(OnboardStep::Keys));
    press(cx, handle, &["j", "j", "k", "e", "secondary-k"]);
    assert_eq!(
        read(cx, &view, |a| a
            .onboarding
            .as_ref()
            .unwrap()
            .practice_count()),
        4
    );
    assert_eq!(
        read(cx, &view, |a| a.overlay_name()),
        None,
        "Ctrl K is practice here"
    );
    for id in ["practice-j", "practice-k", "practice-palette"] {
        click(cx, handle, id);
    }
    for _ in 0..5 {
        click(cx, handle, "practice-e");
    }
    assert!(read(cx, &view, |a| a
        .onboarding
        .as_ref()
        .unwrap()
        .practice_left
        .is_empty()));
    press(cx, handle, &["e", "x"]);
    click_footer(cx, handle, "onboard-next");

    assert_eq!(step(cx, &view), Some(OnboardStep::Done));
    click_footer(cx, handle, ("onboard-change", 4usize));
    assert_eq!(step(cx, &view), Some(OnboardStep::Look));
    click_footer(cx, handle, "onboard-skip");
    click_footer(cx, handle, "onboard-skip");
    assert_eq!(step(cx, &view), Some(OnboardStep::Done));
    click_footer(cx, handle, "onboard-finish");
    assert_eq!(step(cx, &view), None);
    assert_eq!(pref(cx, &view, Pref::Onboarded), Some("true".into()));
    assert_eq!(
        read(cx, &view, SiftApp::listed_subjects),
        ["Hello from IMAP"]
    );
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[gpui_kit::test]
fn summary_reflects_other_choices_and_enter_finishes(cx: &mut TestAppContext) {
    let db = temp_db("onboarding-summary");
    let (handle, view) = open_first_run(cx, &db);
    press(cx, handle, &["enter"]);
    click(cx, handle, "onboard-type-it");
    press(cx, handle, &["escape"]);
    // Jump straight to the summary with unusual choices.
    view.update(cx, |app, cx| {
        app.set_sort_mode(SortMode::Plain, cx);
        for f in AiFeature::ALL {
            app.toggle_ai_feature(f, cx);
        }
        app.set_notify_mode(NotifyMode::All, cx);
        app.onboard_go(OnboardStep::Done, cx);
    });
    click(cx, handle, ("onboard-change", 3usize));
    view.update(cx, |app, cx| {
        app.set_notify_mode(NotifyMode::Hold, cx);
        for i in [1, 3] {
            app.toggle_checkin(i, cx);
        }
        app.toggle_ai_feature(AiFeature::Ask, cx);
        app.onboard_go(OnboardStep::Done, cx);
    });
    press(cx, handle, &["enter"]);
    assert_eq!(step(cx, &view), None);
    assert!(!read(cx, &view, |a| a.tab_shown(TriageView::Meetings)));
    press(cx, handle, &["3"]);
    assert_eq!(
        read(cx, &view, SiftApp::view),
        TriageView::All,
        "hidden tabs have no key"
    );
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[gpui_kit::test]
fn an_existing_install_skips_onboarding(cx: &mut TestAppContext) {
    let view = cx.update(|cx| {
        gpui_kit::init(cx);
        let store = MailStore::open_in_memory().unwrap();
        demo::seed(&store, Utc::now()).unwrap();
        open_main_window(Rc::new(store), None, cx).unwrap()
    });
    assert_eq!(step(cx, &view), None);
    assert_eq!(pref(cx, &view, Pref::Onboarded), Some("true".into()));
}

#[gpui_kit::test]
fn hidden_views_leave_the_tabs(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["3"]);
    view.update(cx, |app, cx| app.toggle_view_tab(TriageView::Meetings, cx));
    assert_eq!(
        read(cx, &view, SiftApp::view),
        TriageView::All,
        "the open view went away"
    );
    view.update(cx, |app, cx| app.toggle_view_tab(TriageView::Meetings, cx));
    press(cx, handle, &["3"]);
    view.update(cx, |app, cx| app.set_sort_mode(SortMode::Plain, cx));
    assert_eq!(read(cx, &view, SiftApp::view), TriageView::All);
    assert!(!read(cx, &view, |a| a.tab_shown(TriageView::NeedsYou)));
}
