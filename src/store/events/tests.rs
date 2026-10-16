//! Invitations applied by UID and SEQUENCE, answers, and events made in Sift.

use chrono::Duration;

use super::*;
use crate::domain::Provider;

fn utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

fn setup() -> (MailStore, AccountId) {
    let store = MailStore::open_in_memory().unwrap();
    let a = store
        .add_account(
            "Work",
            &EmailAddress::parse("you@company.com").unwrap(),
            Provider::Microsoft365,
        )
        .unwrap();
    (store, a.id)
}

fn person(name: &str, email: &str) -> Person {
    Person {
        name: Some(name.into()),
        email: EmailAddress::parse(email).unwrap(),
    }
}

fn invite(sequence: u32, start: &str) -> Invite {
    let start = utc(start);
    Invite {
        method: Method::Request,
        uid: "roadmap-1".into(),
        sequence,
        summary: "Roadmap sync".into(),
        start,
        end: start + Duration::minutes(30),
        all_day: false,
        location: Some("Teams".into()),
        organizer: Some(person("Daniel Mensah", "daniel@company.com")),
        attendees: vec![person("You", "you@company.com")],
    }
}

#[test]
fn invitations_add_update_and_cancel() {
    let (s, account) = setup();
    let thread = ThreadRef {
        account,
        key: "t1".into(),
    };
    let day = (utc("2026-10-19T00:00:00Z"), utc("2026-10-24T00:00:00Z"));
    s.apply_invite(account, &invite(0, "2026-10-20T12:00:00Z"), Some(&thread))
        .unwrap();
    let all = s.events_between(day.0, day.1).unwrap();
    assert_eq!(all.len(), 1);
    let e = &all[0];
    assert_eq!(
        (e.kind, e.title.as_str()),
        (EventKind::Pending, "Roadmap sync")
    );
    assert_eq!(e.organizer.as_ref().unwrap().display(), "Daniel Mensah");
    assert_eq!(e.attendees.len(), 1);
    assert_eq!(e.thread.as_ref(), Some(&thread));
    assert_eq!(s.event_for_thread(&thread).unwrap().unwrap().id, e.id);
    assert_eq!(
        s.pending_invites(utc("2026-10-16T00:00:00Z"))
            .unwrap()
            .len(),
        1
    );
    assert!(
        s.pending_invites(utc("2026-10-21T00:00:00Z"))
            .unwrap()
            .is_empty(),
        "past invites drop off"
    );

    s.set_rsvp(e.id, Some(Rsvp::Accepted)).unwrap();
    let accepted = s.event(e.id).unwrap().unwrap();
    assert_eq!(
        (accepted.kind, accepted.rsvp),
        (EventKind::Meeting, Some(Rsvp::Accepted))
    );

    // An older copy changes nothing; a same-time update keeps the answer.
    s.apply_invite(account, &invite(0, "2026-10-21T12:00:00Z"), None)
        .unwrap();
    assert_eq!(
        s.event(e.id).unwrap().unwrap().start,
        utc("2026-10-20T12:00:00Z")
    );
    let mut renamed = invite(1, "2026-10-20T12:00:00Z");
    renamed.summary = "Roadmap sync (Q1)".into();
    s.apply_invite(account, &renamed, None).unwrap();
    let kept = s.event(e.id).unwrap().unwrap();
    assert_eq!(
        (kept.title.as_str(), kept.kind),
        ("Roadmap sync (Q1)", EventKind::Meeting)
    );
    // A moved meeting needs a new answer.
    s.apply_invite(account, &invite(2, "2026-10-21T12:00:00Z"), None)
        .unwrap();
    let moved = s.event(e.id).unwrap().unwrap();
    assert_eq!((moved.kind, moved.rsvp), (EventKind::Pending, None));

    s.set_rsvp(e.id, Some(Rsvp::Declined)).unwrap();
    assert!(
        s.events_between(day.0, day.1).unwrap().is_empty(),
        "declined is hidden"
    );
    s.set_rsvp(e.id, None).unwrap();
    assert_eq!(s.events_between(day.0, day.1).unwrap().len(), 1);

    let mut cancel = invite(3, "2026-10-21T12:00:00Z");
    cancel.method = Method::Cancel;
    s.apply_invite(account, &cancel, None).unwrap();
    assert!(s.event(e.id).unwrap().is_none());
    s.apply_invite(account, &cancel, None).unwrap();

    let mut published = invite(0, "2026-10-22T09:00:00Z");
    published.method = Method::Other;
    published.uid = "pub".into();
    s.apply_invite(account, &published, None).unwrap();
    assert_eq!(
        s.events_between(day.0, day.1).unwrap()[0].kind,
        EventKind::Meeting
    );
    let mut reply = invite(0, "2026-10-22T09:00:00Z");
    reply.method = Method::Reply;
    reply.uid = "reply".into();
    s.apply_invite(account, &reply, None).unwrap();
    assert_eq!(
        s.events_between(day.0, day.1).unwrap().len(),
        1,
        "replies add nothing"
    );
}

#[test]
fn events_made_in_sift() {
    let (s, account) = setup();
    let me = person("You", "you@company.com");
    let hold = s
        .add_event(
            &NewEvent {
                account,
                title: "Q4 numbers with Priya".into(),
                start: utc("2026-10-19T09:00:00Z"),
                end: utc("2026-10-19T09:30:00Z"),
                kind: EventKind::Hold,
                attendees: vec![person("Priya Raman", "priya@company.com")],
                uid: "sift-1".into(),
            },
            Some(&me),
        )
        .unwrap();
    assert_eq!(hold.kind, EventKind::Hold);
    assert_eq!(hold.organizer, Some(me));
    let request = hold.invite();
    assert_eq!(
        (request.uid.as_str(), request.method),
        ("sift-1", Method::Request)
    );
    s.set_event_kind(hold.id, EventKind::Meeting).unwrap();
    assert_eq!(s.event(hold.id).unwrap().unwrap().kind, EventKind::Meeting);
    s.delete_event(hold.id).unwrap();
    assert!(s.event(hold.id).unwrap().is_none());

    for kind in [
        EventKind::Meeting,
        EventKind::Pending,
        EventKind::Mine,
        EventKind::Hold,
        EventKind::Focus,
        EventKind::Personal,
        EventKind::Declined,
    ] {
        assert_eq!(EventKind::from_key(kind.key()).unwrap(), kind);
        assert!(!kind.label().is_empty());
    }
    assert!(EventKind::from_key("party").is_err());
    assert!(time("soon").is_err());
    assert_eq!(
        people_from_text("bad line\n\tnot-an-address\n\tok@x.com").len(),
        1
    );
}
