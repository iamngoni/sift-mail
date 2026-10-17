//! Headless tests for Settings: opening it, every account setting, ordering,
//! send-as addresses, the default From account, connection actions, removal,
//! and the other pages.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::SiftApp;
use super::settings::SettingsPage;
use super::tests::compose_ui::add_account;
use super::tests::sync_ui::{open_synced_pub, temp_db};
use super::tests::{click, open, press, read};
use crate::domain::{EmailAddress, Provider};
use crate::store::{NotifyLevel, Pref, SyncRange};

fn typing(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str, text: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
        window.input(text, cx);
    })
    .unwrap();
}

/// Scrolls Settings so the lower rows of an expanded account are on screen.
fn scroll_settings(cx: &mut TestAppContext, handle: AnyWindowHandle, down: bool) {
    let dy = if down { -2000.0 } else { 2000.0 };
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            "settings-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.0), gpui_kit::px(dy))),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn first_account(cx: &mut TestAppContext, view: &Entity<SiftApp>) -> crate::domain::AccountId {
    read(cx, view, |a| a.accounts[0].id)
}

fn open_accounts(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    click(cx, handle, "account-switcher");
    click(cx, handle, "manage-accounts");
}

#[gpui_kit::test]
fn settings_opens_from_the_account_menu_and_closes(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    open_accounts(cx, handle);
    assert_eq!(
        read(cx, &view, SiftApp::settings_page),
        Some(SettingsPage::Accounts)
    );
    let work = first_account(cx, &view);
    assert_eq!(
        read(cx, &view, SiftApp::settings_account),
        Some(work),
        "the first account opens expanded"
    );
    click(cx, handle, ("account-card", 0usize));
    assert_eq!(read(cx, &view, SiftApp::settings_account), None);
    click(cx, handle, ("account-card", 0usize));
    click(cx, handle, "back-to-mail");
    assert!(!read(cx, &view, SiftApp::settings_open));
    press(cx, handle, &["secondary-,"]);
    assert_eq!(
        read(cx, &view, SiftApp::settings_page),
        Some(SettingsPage::General)
    );
    press(cx, handle, &["j", "escape"]);
    assert!(!read(cx, &view, SiftApp::settings_open));
}

#[gpui_kit::test]
fn account_settings_save_as_they_change(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    open_accounts(cx, handle);
    let work = first_account(cx, &view);
    let settings =
        |cx: &mut TestAppContext| read(cx, &view, |a| a.store.account_settings(work).unwrap());

    click(cx, handle, ("swatch", 4usize));
    assert_eq!(settings(cx).colour, 4);
    scroll_settings(cx, handle, true);
    click(cx, handle, ("range", 2usize));
    assert_eq!(settings(cx).range, SyncRange::Year);
    click(cx, handle, ("notify", 2usize));
    assert_eq!(settings(cx).notify, NotifyLevel::Nothing);
    click(cx, handle, "account-ai");
    assert!(!settings(cx).ai);
    click(cx, handle, "in-all");
    assert!(!settings(cx).in_all);
    click(cx, handle, ("sync-toggle", 0usize));
    click(cx, handle, ("sync-toggle", 1usize));
    let s = settings(cx);
    assert!(!s.calendar && !s.contacts);

    scroll_settings(cx, handle, false);
    typing(cx, handle, "account-name", " HQ");
    assert_eq!(read(cx, &view, |a| a.accounts[0].name.clone()), "Work HQ");
    cx.update_window(handle, |_, window, cx| {
        let sig = view.read_with(cx, |a, _| {
            a.settings
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .signature
                .clone()
        });
        sig.update(cx, |s, cx| s.focus(window, cx));
        window.render_frame(cx);
        window.input("Ngoni", cx);
    })
    .unwrap();
    assert_eq!(settings(cx).signature, "Ngoni");
}

#[gpui_kit::test]
fn send_as_addresses_are_checked_added_and_removed(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    open_accounts(cx, handle);
    let work = first_account(cx, &view);
    let error = |cx: &mut TestAppContext| {
        read(cx, &view, |a| {
            a.settings
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .alias_error
                .clone()
        })
    };
    typing(cx, handle, "alias-input", "not-an-address");
    click(cx, handle, "add-alias");
    assert!(error(cx).unwrap().contains("full address"));
    cx.update_window(handle, |_, window, cx| {
        let alias = view.read_with(cx, |a, _| {
            a.settings
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .alias
                .clone()
        });
        alias.update(cx, |s, cx| s.set_value("you@company.com", window, cx));
    })
    .unwrap();
    click(cx, handle, "add-alias");
    assert!(error(cx).unwrap().contains("already"));
    cx.update_window(handle, |_, window, cx| {
        let alias = view.read_with(cx, |a, _| {
            a.settings
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .alias
                .clone()
        });
        alias.update(cx, |s, cx| s.set_value("team@company.com", window, cx));
    })
    .unwrap();
    click(cx, handle, "alias-input");
    press(cx, handle, &["enter"]);
    assert_eq!(error(cx), None);
    assert_eq!(
        read(cx, &view, |a| a.store.aliases(work).unwrap()),
        [EmailAddress::parse("team@company.com").unwrap()]
    );
    click(cx, handle, ("remove-alias", 0usize));
    assert!(read(cx, &view, |a| a
        .store
        .aliases(work)
        .unwrap()
        .is_empty()));
}

#[gpui_kit::test]
fn order_default_from_and_removal(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    view.read_with(cx, |a, _| {
        a.store
            .add_account(
                "Personal",
                &EmailAddress::parse("you@gmail.com").unwrap(),
                Provider::Google,
            )
            .unwrap();
    });
    open_accounts(cx, handle);
    assert_eq!(read(cx, &view, |a| a.accounts.len()), 2);
    click(cx, handle, ("move-down", 0usize));
    assert_eq!(read(cx, &view, |a| a.accounts[0].name.clone()), "Personal");
    click(cx, handle, ("move-up", 1usize));
    assert_eq!(read(cx, &view, |a| a.accounts[0].name.clone()), "Work");

    click(cx, handle, "default-from");
    click(cx, handle, ("from-option", 1usize));
    scroll_settings(cx, handle, true);
    let personal = read(cx, &view, |a| a.accounts[1].id);
    assert_eq!(read(cx, &view, SiftApp::default_from), Some(personal));
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .preference(Pref::DefaultFrom)
            .unwrap()),
        Some(personal.0.to_string())
    );

    click(cx, handle, "remove-account");
    click(cx, handle, "cancel-remove");
    click(cx, handle, "remove-account");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click_at(
            "remove-scrim",
            gpui_kit::point(gpui_kit::px(4.0), gpui_kit::px(4.0)),
            cx,
        );
    })
    .unwrap();
    assert!(read(cx, &view, |a| a
        .settings
        .as_ref()
        .unwrap()
        .confirm_remove
        .is_none()));
    click(cx, handle, "remove-account");
    press(cx, handle, &["escape"]);
    assert!(
        read(cx, &view, SiftApp::settings_open),
        "Escape closes the dialog first"
    );
    click(cx, handle, "remove-account");
    click(cx, handle, "confirm-remove");
    assert_eq!(read(cx, &view, |a| a.accounts.len()), 1);
    assert_eq!(read(cx, &view, SiftApp::settings_account), None);
    assert_eq!(
        read(cx, &view, |a| a.settings.as_ref().unwrap().notice.clone()),
        Some("Work removed from this computer".into())
    );
    assert_eq!(read(cx, &view, SiftApp::default_from), Some(personal));
}

#[gpui_kit::test]
fn connection_actions_without_a_service(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    open_accounts(cx, handle);
    let work = first_account(cx, &view);
    scroll_settings(cx, handle, true);
    click(cx, handle, "sync-account");
    assert_eq!(
        read(cx, &view, |a| a.settings.as_ref().unwrap().notice.clone()),
        Some("Sift can't sync in this session.".into())
    );
    view.read_with(cx, |a, _| {
        a.store
            .record_sync_error(work, true, "the server rejected the sign-in")
            .unwrap();
    });
    click(cx, handle, ("settings-nav", 1usize));
    scroll_settings(cx, handle, false);
    click(cx, handle, "fix-problem");
    assert!(read(cx, &view, |a| a.form.is_some()));
    let email = cx.update(|cx| {
        view.read(cx)
            .form
            .as_ref()
            .unwrap()
            .email
            .read(cx)
            .value()
            .to_string()
    });
    assert_eq!(email, "you@company.com");
    press(cx, handle, &["escape"]);
    scroll_settings(cx, handle, true);
    click(cx, handle, "reauth-account");
    assert!(read(cx, &view, |a| a.form.is_some()));
    press(cx, handle, &["escape"]);
    scroll_settings(cx, handle, false);
    click(cx, handle, "settings-add-account");
    assert!(read(cx, &view, |a| a.form.is_some()));
}

#[gpui_kit::test]
fn sync_now_runs_for_one_account(cx: &mut TestAppContext) {
    let db = temp_db("settings-sync");
    let (handle, view) = open_synced_pub(cx, &db);
    add_account(cx, handle, &view);
    open_accounts(cx, handle);
    scroll_settings(cx, handle, true);
    click(cx, handle, "sync-account");
    assert!(read(cx, &view, |a| !a
        .settings
        .as_ref()
        .unwrap()
        .busy
        .is_empty()));
    click(cx, handle, "sync-account");
    cx.run_until_parked();
    assert!(read(cx, &view, |a| a
        .settings
        .as_ref()
        .unwrap()
        .busy
        .is_empty()));
    let notice = read(cx, &view, |a| a.settings.as_ref().unwrap().notice.clone());
    assert!(notice.unwrap().ends_with("synced"));
    click(cx, handle, "remove-account");
    click(cx, handle, "confirm-remove");
    assert!(read(cx, &view, |a| a.accounts.is_empty()));
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[gpui_kit::test]
fn the_other_pages(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["secondary-,"]);
    for (i, page) in SettingsPage::ALL.into_iter().enumerate() {
        click(cx, handle, ("settings-nav", i));
        assert_eq!(read(cx, &view, SiftApp::settings_page), Some(page));
    }
    click(cx, handle, ("settings-nav", 4usize));
    click(cx, handle, "theme-dark");
    assert_eq!(
        read(cx, &view, |a| a.store.preference(Pref::Appearance).unwrap()),
        Some("dark".into())
    );
    click(cx, handle, "theme-light");
    click(cx, handle, "theme-system");
    click(cx, handle, "density-compact");
    assert!(read(cx, &view, |a| a.compact));
    click(cx, handle, "density-comfortable");
    assert!(!read(cx, &view, |a| a.compact));
    click(cx, handle, ("settings-nav", 3usize));
    click(cx, handle, "settings-hold");
    assert!(read(cx, &view, SiftApp::holding));
    click(cx, handle, "notifications-accounts");
    assert_eq!(
        read(cx, &view, SiftApp::settings_page),
        Some(SettingsPage::Accounts)
    );
    click(cx, handle, ("settings-nav", 0usize));
    click(cx, handle, "default-from");
    click(cx, handle, ("from-option", 0usize));
    assert!(read(cx, &view, |a| !a.settings.as_ref().unwrap().from_menu));
}
