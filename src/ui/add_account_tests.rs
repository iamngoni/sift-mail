//! Headless tests for the Add account flow: detection, password sign-in with
//! editable servers, browser sign-in (waiting, cancel, blocked), Set up and
//! the first sync.

use std::sync::Arc;

use chrono::Utc;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::SiftApp;
use super::add_account::{AddAccountFlow, Step};
use super::tests::compose_ui::{add_account, click_low, set_flow_field};
use super::tests::sync_ui::{open_signing, open_synced_pub, temp_db};
use super::tests::{click, open, press, read};
use crate::domain::TriageView;
use crate::oauth::{OAuthError, SignedIn, Tokens};
use crate::store::{NotifyLevel, SyncRange};
use crate::sync::service::Signer;

fn step(cx: &mut TestAppContext, view: &Entity<SiftApp>) -> Option<Step> {
    read(cx, view, SiftApp::add_step)
}

fn flow<R>(
    cx: &mut TestAppContext,
    view: &Entity<SiftApp>,
    f: impl FnOnce(&AddAccountFlow) -> R,
) -> R {
    read(cx, view, |a| f(a.form.as_ref().unwrap()))
}

fn start(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    click(cx, handle, "account-switcher");
    click(cx, handle, "add-account");
}

fn signed() -> Result<SignedIn, OAuthError> {
    Ok(SignedIn {
        tokens: Tokens {
            // The scripted connector accepts "right".
            access_token: "right".into(),
            refresh_token: "rt".into(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
        },
        email: crate::domain::EmailAddress::parse("you@company.com").unwrap(),
        name: None,
    })
}

#[gpui_kit::test]
fn a_password_account_goes_through_every_step(cx: &mut TestAppContext) {
    let db = temp_db("flow-imap");
    let (handle, view) = open_synced_pub(cx, &db);
    cx.run_until_parked();
    start(cx, handle);
    assert_eq!(step(cx, &view), Some(Step::Choose));
    assert!(flow(cx, &view, |f| f.first_run));
    // Typing in the flow must not trigger inbox shortcuts.
    press(cx, handle, &["e", "3"]);
    assert_eq!(read(cx, &view, SiftApp::view), TriageView::All);
    // A first run can't be closed.
    press(cx, handle, &["escape"]);
    assert!(read(cx, &view, SiftApp::is_adding_account));

    click(cx, handle, "continue-email");
    assert_eq!(step(cx, &view), Some(Step::Choose), "nothing typed yet");
    set_flow_field(cx, handle, &view, |f| f.email.clone(), "you@fastmail.com");
    press(cx, handle, &["enter"]);
    click(cx, handle, "continue-email");
    assert_eq!(step(cx, &view), Some(Step::Imap));
    let host = cx.update(|cx| {
        view.read(cx)
            .form
            .as_ref()
            .unwrap()
            .imap_host
            .read(cx)
            .value()
            .to_string()
    });
    assert_eq!(host, "imap.fastmail.com");

    click(cx, handle, "connect");
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.starts_with("Enter your password"))));
    click(cx, handle, "toggle-servers");
    assert!(flow(cx, &view, |f| f.show_servers));
    set_flow_field(cx, handle, &view, |f| f.imap_port.clone(), "x");
    set_flow_field(cx, handle, &view, |f| f.password.clone(), "wrong");
    click(cx, handle, "connect");
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.contains("incoming port"))));
    set_flow_field(cx, handle, &view, |f| f.imap_port.clone(), "993");
    set_flow_field(cx, handle, &view, |f| f.smtp_host.clone(), "bad host");
    click(cx, handle, "connect");
    assert!(read(cx, &view, |a| a.form_error().is_some()));
    set_flow_field(
        cx,
        handle,
        &view,
        |f| f.smtp_host.clone(),
        "smtp.fastmail.com",
    );
    click(cx, handle, "connect");
    cx.run_until_parked();
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.contains("rejected the sign-in"))));

    set_flow_field(cx, handle, &view, |f| f.password.clone(), "right");
    click(cx, handle, "connect");
    cx.run_until_parked();
    assert_eq!(step(cx, &view), Some(Step::Setup));
    let id = flow(cx, &view, |f| f.account.as_ref().unwrap().id);
    assert!(
        read(cx, &view, |a| a.store.imap_accounts().unwrap().is_empty()),
        "held back from sync until Set up is done"
    );

    click(cx, handle, ("setup-colour", 2usize));
    click_low(cx, handle, ("setup-range", 0usize));
    click_low(cx, handle, ("setup-notify", 1usize));
    click_low(cx, handle, "setup-ai");
    set_flow_field(cx, handle, &view, |f| f.name.clone(), "Fast");
    click_low(cx, handle, "setup-connect");
    assert_eq!(step(cx, &view), Some(Step::Sync));
    cx.run_until_parked();
    assert_eq!(flow(cx, &view, |f| f.sync_result.clone()), Some(Ok(())));
    assert_eq!(flow(cx, &view, |f| f.synced), 1);
    let saved = read(cx, &view, |a| a.store.account_settings(id).unwrap());
    assert_eq!(
        (saved.colour, saved.range, saved.notify, saved.ai),
        (2, SyncRange::Days30, NotifyLevel::Everything, false)
    );
    assert_eq!(read(cx, &view, |a| a.accounts[0].name.clone()), "Fast");
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .smtp_settings(id)
            .unwrap()
            .map(|s| s.host.to_string())),
        Some("smtp.fastmail.com".into())
    );

    click_low(cx, handle, "go-to-inbox");
    assert!(!read(cx, &view, SiftApp::is_adding_account));
    assert_eq!(
        read(cx, &view, SiftApp::listed_subjects),
        ["Hello from IMAP"]
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
fn closing_and_going_back(cx: &mut TestAppContext) {
    let db = temp_db("flow-close");
    let (handle, view) = open_synced_pub(cx, &db);
    add_account(cx, handle, &view);
    assert_eq!(read(cx, &view, |a| a.accounts.len()), 1);

    start(cx, handle);
    assert!(!flow(cx, &view, |f| f.first_run));
    click(cx, handle, "flow-close");
    assert!(!read(cx, &view, SiftApp::is_adding_account));
    start(cx, handle);
    press(cx, handle, &["escape"]);
    assert!(!read(cx, &view, SiftApp::is_adding_account));
    start(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click_at(
            "add-account-scrim",
            gpui_kit::point(gpui_kit::px(4.0), gpui_kit::px(4.0)),
            cx,
        );
    })
    .unwrap();
    assert!(!read(cx, &view, SiftApp::is_adding_account));

    // Back from each sign-in step returns to Choose.
    start(cx, handle);
    click(cx, handle, ("provider", 3usize));
    assert_eq!(step(cx, &view), Some(Step::Imap));
    click(cx, handle, "flow-back");
    assert_eq!(step(cx, &view), Some(Step::Choose));
    click(cx, handle, ("provider", 2usize));
    assert!(flow(cx, &view, |f| f.provider == crate::domain::Provider::ICloud));
    click(cx, handle, "flow-back");
    click(cx, handle, ("provider", 1usize));
    assert_eq!(step(cx, &view), Some(Step::OAuth));
    click(cx, handle, "flow-back");
    assert_eq!(step(cx, &view), Some(Step::Choose));

    // Closing during Set up keeps the account and lets it sync with defaults.
    set_flow_field(cx, handle, &view, |f| f.email.clone(), "me@fastmail.com");
    click(cx, handle, "continue-email");
    set_flow_field(cx, handle, &view, |f| f.password.clone(), "right");
    click(cx, handle, "connect");
    cx.run_until_parked();
    assert_eq!(step(cx, &view), Some(Step::Setup));
    click_low(cx, handle, ("setup-range", 3usize));
    click(cx, handle, "flow-close");
    cx.run_until_parked();
    assert_eq!(read(cx, &view, |a| a.accounts.len()), 2);
    assert_eq!(
        read(cx, &view, |a| a.store.imap_accounts().unwrap().len()),
        2
    );

    // Add another starts over from step 1.
    start(cx, handle);
    set_flow_field(cx, handle, &view, |f| f.email.clone(), "third@fastmail.com");
    click(cx, handle, "continue-email");
    set_flow_field(cx, handle, &view, |f| f.password.clone(), "right");
    click(cx, handle, "connect");
    cx.run_until_parked();
    click_low(cx, handle, "setup-connect");
    click_low(cx, handle, "go-to-inbox");
    assert_eq!(
        read(cx, &view, |a| a.toast_text().map(str::to_owned)),
        Some("Fetching Fastmail in the background…".into())
    );
    cx.run_until_parked();
    start(cx, handle);
    click(cx, handle, ("provider", 3usize));
    click(cx, handle, "flow-back");
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[gpui_kit::test]
fn browser_sign_in_waits_and_finishes(cx: &mut TestAppContext) {
    let db = temp_db("flow-oauth");
    let signer: Signer = Arc::new(|_, _, show_link, cancelled| {
        show_link("https://login.example/authorize?x=1");
        if cancelled() {
            Err(OAuthError::Cancelled)
        } else {
            signed()
        }
    });
    let (handle, view) = open_signing(cx, &db, Some(signer));
    start(cx, handle);
    set_flow_field(cx, handle, &view, |f| f.email.clone(), "you@company.com");
    click(cx, handle, "continue-email");
    assert_eq!(step(cx, &view), Some(Step::OAuth));
    click_low(cx, handle, "open-sign-in");
    assert!(read(cx, &view, SiftApp::is_waiting_for_browser));
    cx.run_until_parked();
    assert_eq!(step(cx, &view), Some(Step::Setup));
    click_low(cx, handle, ("setup-sync", 1usize));
    assert!(!flow(cx, &view, |f| f.settings.contacts));
    click_low(cx, handle, "setup-connect");
    cx.run_until_parked();
    click_low(cx, handle, "add-another");
    assert_eq!(step(cx, &view), Some(Step::Choose));
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[gpui_kit::test]
fn waiting_can_be_cancelled_or_fail(cx: &mut TestAppContext) {
    let db = temp_db("flow-wait");
    let signer: Signer = Arc::new(|_, _, show_link, cancelled| {
        show_link("https://login.example/authorize");
        if cancelled() {
            Err(OAuthError::Cancelled)
        } else {
            Err(OAuthError::TimedOut)
        }
    });
    let (handle, view) = open_signing(cx, &db, Some(signer));
    start(cx, handle);
    click(cx, handle, ("provider", 0usize));
    click_low(cx, handle, "open-sign-in");
    click(cx, handle, "copy-link");
    click(cx, handle, "cancel-oauth");
    assert_eq!(step(cx, &view), Some(Step::OAuth));
    cx.run_until_parked();
    assert!(read(cx, &view, |a| a.form_error().is_none()));

    click_low(cx, handle, "open-sign-in");
    cx.run_until_parked();
    assert_eq!(step(cx, &view), Some(Step::OAuth));
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.contains("timed out"))));

    click_low(cx, handle, "open-sign-in");
    press(cx, handle, &["escape"]);
    assert_eq!(
        step(cx, &view),
        Some(Step::OAuth),
        "Escape stops waiting first"
    );
    click_low(cx, handle, "open-sign-in");
    click(cx, handle, "flow-back");
    assert_eq!(step(cx, &view), Some(Step::OAuth));
    cx.run_until_parked();
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[gpui_kit::test]
fn a_blocked_organisation_offers_the_it_request(cx: &mut TestAppContext) {
    let db = temp_db("flow-blocked");
    let signer: Signer = Arc::new(|_, _, _, _| Err(OAuthError::AdminApprovalRequired));
    let (handle, view) = open_signing(cx, &db, Some(signer));
    start(cx, handle);
    set_flow_field(cx, handle, &view, |f| f.email.clone(), "you@company.com");
    click(cx, handle, "continue-email");
    click_low(cx, handle, "open-sign-in");
    click(cx, handle, "copy-link");
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.contains("isn't ready"))));
    cx.run_until_parked();
    assert!(read(cx, &view, SiftApp::is_blocked));
    click(cx, handle, "copy-it-request");
    let copied = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert!(copied.contains("company.com Microsoft 365"));
    assert!(flow(cx, &view, |f| f.copied_it));
    click(cx, handle, "restart");
    assert_eq!(step(cx, &view), Some(Step::Choose));
    assert!(!flow(cx, &view, |f| f.copied_it));
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[gpui_kit::test]
fn without_a_service_nothing_can_be_saved(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    start(cx, handle);
    click(cx, handle, ("provider", 0usize));
    click_low(cx, handle, "open-sign-in");
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.contains("can't save"))));
    click(cx, handle, "flow-back");
    set_flow_field(cx, handle, &view, |f| f.email.clone(), "you@icloud.com");
    click(cx, handle, "continue-email");
    click(cx, handle, "connect");
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.contains("app-specific"))));
    set_flow_field(cx, handle, &view, |f| f.password.clone(), "pw");
    click(cx, handle, "connect");
    assert!(read(cx, &view, |a| a
        .form_error()
        .is_some_and(|e| e.contains("can't save"))));
}
