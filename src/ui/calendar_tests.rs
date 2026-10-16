//! Headless tests for the Calendar and for answering invitations from mail.

use chrono::{Duration, Utc};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::SiftApp;
use super::tests::compose_ui::add_account;
use super::tests::sync_ui::{open_synced_pub, temp_db};
use super::tests::{click, open, press, read};
use crate::domain::ical::{Invite, Method, Person, Rsvp};
use crate::domain::{EmailAddress, ThreadRef};
use crate::store::EventKind;

fn person(name: &str, email: &str) -> Person {
    Person {
        name: Some(name.into()),
        email: EmailAddress::parse(email).unwrap(),
    }
}

fn scroll(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str, dy: f32) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            id,
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.0), gpui_kit::px(dy))),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn roadmap(cx: &mut TestAppContext, view: &Entity<SiftApp>) -> crate::store::Event {
    read(cx, view, |a| {
        a.store
            .pending_invites(Utc::now())
            .unwrap()
            .into_iter()
            .chain(
                a.store
                    .events_between(
                        Utc::now() - Duration::days(30),
                        Utc::now() + Duration::days(30),
                    )
                    .unwrap(),
            )
            .find(|e| e.uid == "demo-roadmap")
            .unwrap()
    })
}

#[gpui_kit::test]
fn the_week_navigates_and_closes(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, "open-calendar");
    assert!(read(cx, &view, SiftApp::calendar_open));
    assert!(
        read(cx, &view, |a| a.week_events().len() >= 5),
        "the demo week has meetings"
    );
    press(cx, handle, &["right"]);
    assert_eq!(read(cx, &view, |a| a.calendar.as_ref().unwrap().week), 1);
    assert!(read(cx, &view, |a| a
        .week_events()
        .iter()
        .any(|e| e.kind == EventKind::Pending)));
    click(cx, handle, "prev-week");
    click(cx, handle, "prev-week");
    assert_eq!(read(cx, &view, |a| a.calendar.as_ref().unwrap().week), -1);
    click(cx, handle, "next-week");
    press(cx, handle, &["right", "right", "t"]);
    assert_eq!(read(cx, &view, |a| a.calendar.as_ref().unwrap().week), 0);
    click(cx, handle, "this-week");
    press(cx, handle, &["left", "x", "ctrl-k"]);
    assert_eq!(read(cx, &view, |a| a.overlay_name()), None);
    click(cx, handle, "tab-mail");
    assert!(!read(cx, &view, SiftApp::calendar_open));
    click(cx, handle, "open-calendar");
    press(cx, handle, &["escape"]);
    assert!(!read(cx, &view, SiftApp::calendar_open));
}

#[gpui_kit::test]
fn events_are_added_shown_and_deleted(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, "open-calendar");
    press(cx, handle, &["right"]);
    click(cx, handle, ("slot", 4 * 100 + 10usize));
    assert!(read(cx, &view, |a| a
        .calendar
        .as_ref()
        .unwrap()
        .draft
        .is_some()));
    press(cx, handle, &["escape"]);
    assert!(read(cx, &view, |a| a
        .calendar
        .as_ref()
        .unwrap()
        .draft
        .is_none()));
    click(cx, handle, ("slot", 4 * 100 + 10usize));
    click(cx, handle, "cancel-event");
    click(cx, handle, ("slot", 4 * 100 + 10usize));
    click(cx, handle, ("length", 2usize));
    assert_eq!(
        read(cx, &view, |a| a
            .calendar
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .minutes),
        90
    );
    cx.update_window(handle, |_, window, cx| {
        let title = view
            .read(cx)
            .calendar
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .title
            .clone();
        title.update(cx, |s, cx| s.set_value("Dentist", window, cx));
    })
    .unwrap();
    click(cx, handle, "save-event");
    assert_eq!(
        read(cx, &view, |a| a.toast_text().map(str::to_owned)),
        Some("Added Dentist".into())
    );
    let dentist = read(cx, &view, |a| {
        a.week_events()
            .into_iter()
            .find(|e| e.title == "Dentist")
            .unwrap()
    });
    assert_eq!((dentist.end - dentist.start).num_minutes(), 90);
    click(
        cx,
        handle,
        ("event", usize::try_from(dentist.id.0).unwrap()),
    );
    assert_eq!(
        read(cx, &view, |a| a.calendar.as_ref().unwrap().detail),
        Some(dentist.id)
    );
    click(cx, handle, "close-detail");
    click(
        cx,
        handle,
        ("event", usize::try_from(dentist.id.0).unwrap()),
    );
    press(cx, handle, &["escape"]);
    assert_eq!(
        read(cx, &view, |a| a.calendar.as_ref().unwrap().detail),
        None
    );
    click(
        cx,
        handle,
        ("event", usize::try_from(dentist.id.0).unwrap()),
    );
    click(cx, handle, "detail-delete");
    assert!(read(cx, &view, |a| a
        .store
        .event(dentist.id)
        .unwrap()
        .is_none()));

    // N starts an event at the next free half hour; an empty title is fine.
    press(cx, handle, &["n"]);
    assert!(read(cx, &view, |a| a
        .calendar
        .as_ref()
        .unwrap()
        .draft
        .is_some()));
    press(cx, handle, &["enter"]);
    assert!(read(cx, &view, |a| a
        .week_events()
        .iter()
        .any(|e| e.title == "New event")));
    click(cx, handle, "new-event");
    assert!(read(cx, &view, |a| a
        .calendar
        .as_ref()
        .unwrap()
        .draft
        .is_some()));
}

#[gpui_kit::test]
fn invitations_are_answered_from_the_calendar_mail_and_rows(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let invite = roadmap(cx, &view);
    // The reader's meeting card.
    let index = read(cx, &view, |a| {
        a.listed_subjects()
            .iter()
            .position(|s| s == "Invitation: Roadmap sync")
            .unwrap()
    });
    click(cx, handle, ("thread-row", index));
    click(cx, handle, "rsvp-maybe");
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .event(invite.id)
            .unwrap()
            .unwrap()
            .rsvp),
        Some(Rsvp::Tentative)
    );
    click(cx, handle, "rsvp-change");
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .event(invite.id)
            .unwrap()
            .unwrap()
            .kind),
        EventKind::Pending
    );
    click(cx, handle, "rsvp-decline");
    click(cx, handle, "rsvp-change");
    click(cx, handle, "rsvp-accept");
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .event(invite.id)
            .unwrap()
            .unwrap()
            .kind),
        EventKind::Meeting
    );
    press(cx, handle, &["escape"]);

    // Accept and Decline in the list row.
    view.read_with(cx, |a, _| a.store.set_rsvp(invite.id, None).unwrap());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover(("thread-row", index), cx);
        window.render_frame(cx);
        window.click(("row-decline", index), cx);
    })
    .unwrap();
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .event(invite.id)
            .unwrap()
            .unwrap()
            .kind),
        EventKind::Declined
    );
    assert!(
        read(cx, &view, |a| a.open_subject().is_none()),
        "the row didn't open"
    );
    view.read_with(cx, |a, _| a.store.set_rsvp(invite.id, None).unwrap());
    click(cx, handle, "filter-unread");
    click(cx, handle, "filter-all");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("row-accept", index), cx);
    })
    .unwrap();
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .event(invite.id)
            .unwrap()
            .unwrap()
            .kind),
        EventKind::Meeting
    );

    // Waiting on you, in the Calendar.
    view.read_with(cx, |a, _| a.store.set_rsvp(invite.id, None).unwrap());
    click(cx, handle, "open-calendar");
    click(cx, handle, ("waiting-show", 0usize));
    assert_eq!(
        read(cx, &view, |a| a.calendar.as_ref().unwrap().detail),
        Some(invite.id)
    );
    click(cx, handle, "detail-decline");
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .event(invite.id)
            .unwrap()
            .unwrap()
            .kind),
        EventKind::Declined
    );
    view.read_with(cx, |a, _| a.store.set_rsvp(invite.id, None).unwrap());
    click(cx, handle, ("waiting-decline", 0usize));
    view.read_with(cx, |a, _| a.store.set_rsvp(invite.id, None).unwrap());
    click(cx, handle, ("waiting-accept", 0usize));
    assert_eq!(
        read(cx, &view, |a| a.toast_text().map(str::to_owned)),
        Some("Accepted Roadmap sync".into())
    );
    view.read_with(cx, |a, _| a.store.set_rsvp(invite.id, None).unwrap());
    click(cx, handle, ("event", usize::try_from(invite.id.0).unwrap()));
    click(cx, handle, "detail-accept");
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .event(invite.id)
            .unwrap()
            .unwrap()
            .kind),
        EventKind::Meeting
    );
}

#[gpui_kit::test]
fn find_a_time_holds_and_releases(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    click(cx, handle, "open-calendar");
    let offers = read(cx, &view, |a| {
        a.calendar.as_ref().unwrap().offers.clone().unwrap()
    });
    assert_eq!(offers.1.len(), 3);
    assert_eq!(offers.2[0].email.as_str(), "priya@company.com");
    assert!(
        offers
            .0
            .starts_with("Next week, 30 minutes, with Priya Raman.")
    );
    click(cx, handle, ("offer", 1usize));
    let hold = read(cx, &view, |a| a.calendar.as_ref().unwrap().hold.unwrap());
    assert_eq!(read(cx, &view, |a| a.calendar.as_ref().unwrap().week), 1);
    click(cx, handle, ("offer", 0usize));
    let replaced = read(cx, &view, |a| {
        a.calendar
            .as_ref()
            .unwrap()
            .hold
            .and_then(|h| a.store.event(h).unwrap())
    });
    assert_eq!(
        replaced.map(|e| super::calendar::local(e.start)),
        Some(offers.1[0].start),
        "a new pick replaces the hold"
    );
    let _ = hold;
    let hold = read(cx, &view, |a| a.calendar.as_ref().unwrap().hold.unwrap());
    click(cx, handle, "send-invite");
    assert!(
        read(cx, &view, |a| a.error.clone())
            .unwrap()
            .contains("can't send invites")
    );
    click(cx, handle, ("event", usize::try_from(hold.0).unwrap()));
    click(cx, handle, "detail-release");
    assert!(read(cx, &view, |a| a.store.event(hold).unwrap().is_none()));
    assert_eq!(read(cx, &view, |a| a.calendar.as_ref().unwrap().hold), None);

    cx.update_window(handle, |_, window, cx| {
        let ask = view.read(cx).calendar.as_ref().unwrap().ask.clone();
        ask.update(cx, |s, cx| s.set_value("an hour tomorrow", window, cx));
    })
    .unwrap();
    click(cx, handle, "find-time-go");
    let summary = read(cx, &view, |a| {
        a.calendar.as_ref().unwrap().offers.clone().unwrap().0
    });
    assert!(summary.contains("1 hour"));
    click(cx, handle, "find-time");
    press(cx, handle, &["enter", "n", "escape"]);
    assert!(
        read(cx, &view, SiftApp::calendar_open),
        "typing in Find a time isn't a shortcut"
    );
}

#[gpui_kit::test]
fn meetings_with_links_can_be_joined(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let account = read(cx, &view, |a| a.accounts[0].id);
    let start = Utc::now() + Duration::days(7);
    view.read_with(cx, |a, _| {
        a.store
            .apply_invite(
                account,
                &Invite {
                    method: Method::Other,
                    uid: "linked".into(),
                    sequence: 0,
                    summary: "Board review".into(),
                    start,
                    end: start + Duration::hours(1),
                    all_day: false,
                    location: Some("Teams https://teams.example/join/1".into()),
                    organizer: None,
                    attendees: vec![],
                },
                None,
            )
            .unwrap();
    });
    view.update(cx, |app, _| {
        app.opener = |url| {
            assert!(url.to_string_lossy().starts_with("https://teams.example"));
            Ok(())
        };
    });
    click(cx, handle, "open-calendar");
    let e = read(cx, &view, |a| {
        a.store
            .events_between(start - Duration::minutes(1), start + Duration::minutes(1))
            .unwrap()[0]
            .clone()
    });
    view.update(cx, |app, cx| {
        let week = (crate::domain::schedule::monday_of(super::calendar::local(e.start).date())
            - crate::domain::schedule::monday_of(chrono::Local::now().date_naive()))
        .num_weeks();
        app.calendar.as_mut().unwrap().week = week;
        app.show_event(Some(e.id), cx);
    });
    scroll(cx, handle, "week", -2000.0);
    click(cx, handle, "detail-join");
    assert_eq!(read(cx, &view, |a| a.error.clone()), None);
}

#[gpui_kit::test]
fn answers_and_invites_go_out_by_mail(cx: &mut TestAppContext) {
    let db = temp_db("calendar-send");
    let (handle, view) = open_synced_pub(cx, &db);
    add_account(cx, handle, &view);
    let account = read(cx, &view, |a| a.accounts[0].id);
    let start = Utc::now() + Duration::days(3);
    for (uid, organizer) in [("ok", "daniel@company.com"), ("bad", "fail@company.com")] {
        view.read_with(cx, |a, _| {
            a.store
                .apply_invite(
                    account,
                    &Invite {
                        method: Method::Request,
                        uid: uid.into(),
                        sequence: 0,
                        summary: format!("Meeting {uid}"),
                        start,
                        end: start + Duration::minutes(30),
                        all_day: false,
                        location: None,
                        organizer: Some(person("Organizer", organizer)),
                        attendees: vec![],
                    },
                    Some(&ThreadRef {
                        account,
                        key: uid.into(),
                    }),
                )
                .unwrap();
        });
    }
    let events = read(cx, &view, |a| a.store.pending_invites(Utc::now()).unwrap());
    for e in &events {
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |app, cx| {
                app.rsvp(e.id, Some(Rsvp::Accepted), window, cx)
            });
        })
        .unwrap();
    }
    cx.run_until_parked();
    let error = read(cx, &view, |a| a.error.clone()).unwrap();
    assert!(error.contains("wasn't told"), "{error}");

    // A hold with someone becomes a meeting once its invite is sent.
    view.update(cx, |app, _| app.error = None);
    click(cx, handle, "open-calendar");
    click(cx, handle, ("offer", 0usize));
    let hold = read(cx, &view, |a| a.calendar.as_ref().unwrap().hold);
    if let Some(hold) = hold {
        let has_people = read(cx, &view, |a| {
            !a.store.event(hold).unwrap().unwrap().attendees.is_empty()
        });
        if has_people {
            click(cx, handle, "send-invite");
            cx.run_until_parked();
            assert_eq!(
                read(cx, &view, |a| a.store.event(hold).unwrap().unwrap().kind),
                EventKind::Meeting
            );
        } else {
            cx.update_window(handle, |_, window, cx| {
                view.update(cx, |app, cx| app.send_invite(hold, window, cx));
            })
            .unwrap();
            assert!(
                read(cx, &view, |a| a.error.clone())
                    .unwrap()
                    .contains("couldn't send that invite")
            );
        }
    }
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}
