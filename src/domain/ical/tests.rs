//! Invitations as Outlook and Google send them, and the replies Sift writes.

use super::*;

fn utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

/// An Outlook invite in W. Europe time, with a folded summary.
const OUTLOOK: &str = "BEGIN:VCALENDAR\r\n\
METHOD:REQUEST\r\n\
PRODID:Microsoft Exchange Server 2010\r\n\
VERSION:2.0\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:W. Europe Standard Time\r\n\
BEGIN:STANDARD\r\n\
DTSTART:16010101T030000\r\n\
TZOFFSETFROM:+0200\r\n\
TZOFFSETTO:+0100\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10\r\n\
END:STANDARD\r\n\
BEGIN:DAYLIGHT\r\n\
DTSTART:16010101T020000\r\n\
TZOFFSETFROM:+0100\r\n\
TZOFFSETTO:+0200\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3\r\n\
END:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\n\
ORGANIZER;CN=Daniel Mensah:mailto:daniel@company.com\r\n\
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=You:mailto:you@company.com\r\n\
ATTENDEE;CN=\"Priya Raman\";RSVP=TRUE:mailto:priya@company.com\r\n\
ATTENDEE:mailto:not an address\r\n\
SUMMARY;LANGUAGE=en-US:Roadmap sync\\, Q1 priorities and the\r\n\x20\x20hiring plan\r\n\
DTSTART;TZID=W. Europe Standard Time:20261020T140000\r\n\
DTEND;TZID=W. Europe Standard Time:20261020T143000\r\n\
UID:040000008200E00074C5B7101A82E008\r\n\
SEQUENCE:2\r\n\
LOCATION;LANGUAGE=en-US:Microsoft Teams Meeting\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

#[test]
fn reads_an_outlook_invite_with_its_time_zone() {
    let i = parse(OUTLOOK).unwrap();
    assert_eq!(i.method, Method::Request);
    assert_eq!(i.uid, "040000008200E00074C5B7101A82E008");
    assert_eq!(i.sequence, 2);
    assert_eq!(i.summary, "Roadmap sync, Q1 priorities and the hiring plan");
    // 20 October is daylight time in W. Europe (+02:00).
    assert_eq!(i.start, utc("2026-10-20T12:00:00Z"));
    assert_eq!(i.end, utc("2026-10-20T12:30:00Z"));
    assert!(!i.all_day);
    assert_eq!(i.location.as_deref(), Some("Microsoft Teams Meeting"));
    assert_eq!(i.organizer.as_ref().unwrap().display(), "Daniel Mensah");
    let invited: Vec<_> = i.attendees.iter().map(Person::display).collect();
    assert_eq!(invited, ["You", "Priya Raman"], "bad addresses are skipped");
}

#[test]
fn standard_time_applies_after_the_october_change() {
    let november = OUTLOOK
        .replace("20261020T140000", "20261103T140000")
        .replace("20261020T143000", "20261103T143000");
    let i = parse(&november).unwrap();
    assert_eq!(i.start, utc("2026-11-03T13:00:00Z"), "+01:00 in November");
    let january = OUTLOOK.replace("20261020T140000", "20270105T090000");
    assert_eq!(parse(&january).unwrap().start, utc("2027-01-05T08:00:00Z"));
}

#[test]
fn reads_utc_dates_durations_and_cancellations() {
    let google = "BEGIN:VCALENDAR\nMETHOD:CANCEL\nBEGIN:VEVENT\nDTSTART:20261021T090000Z\nDURATION:PT1H30M\nUID:abc@google.com\nSUMMARY:Standup\nEND:VEVENT\nEND:VCALENDAR\n";
    let i = parse(google).unwrap();
    assert_eq!(i.method, Method::Cancel);
    assert_eq!(i.end - i.start, Duration::minutes(90));
    assert_eq!((i.organizer, i.location), (None, None));

    let day = "BEGIN:VEVENT\nUID:d\nDTSTART;VALUE=DATE:20261023\nEND:VEVENT";
    let d = parse(day).unwrap();
    assert!(d.all_day);
    assert_eq!(d.method, Method::Other);
    assert_eq!(d.summary, "(no title)");
    assert_eq!(d.end - d.start, Duration::days(1));

    let floating =
        "BEGIN:VEVENT\nUID:f\nDTSTART:20261023T100000\nDTEND:20261023T090000\nEND:VEVENT";
    let f = parse(floating).unwrap();
    assert_eq!(f.end, f.start, "an end before the start is clamped");

    let fixed = "BEGIN:VTIMEZONE\nTZID:India\nBEGIN:STANDARD\nDTSTART:16010101T000000\nTZOFFSETTO:+0530\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VEVENT\nUID:x\nDTSTART;TZID=India:20261023T100000\nDURATION:P1D\nMETHOD:REPLY\nEND:VEVENT";
    assert_eq!(parse(fixed).unwrap().start, utc("2026-10-23T04:30:00Z"));
    assert_eq!(parse("BEGIN:VCALENDAR\nMETHOD:REPLY\nBEGIN:VEVENT\nUID:r\nDTSTART:20261023T100000Z\nEND:VEVENT").unwrap().method, Method::Reply);
}

#[test]
fn unusable_calendars_are_rejected() {
    assert_eq!(parse(""), None);
    assert_eq!(
        parse("BEGIN:VEVENT\nDTSTART:20261021T090000Z\nEND:VEVENT"),
        None,
        "no UID"
    );
    assert_eq!(parse("BEGIN:VEVENT\nUID:x\nEND:VEVENT"), None, "no start");
    assert_eq!(parse("BEGIN:VEVENT\nUID:x\nDTSTART:soon\nEND:VEVENT"), None);
    assert_eq!(duration("PT5X"), None);
    assert_eq!(duration("1H"), None);
    assert_eq!(
        duration("P1W2DT3H4M5S"),
        Some(Duration::seconds(((9 * 24 + 3) * 60 + 4) * 60 + 5))
    );
    assert_eq!(offset_minutes("-0330"), Some(-210));
    assert_eq!(offset_minutes("0100"), Some(60));
    assert_eq!(offset_minutes(""), None);
    assert_eq!(weekday("XX"), None);
    assert_eq!(
        nth_weekday(2026, 3, 2, Weekday::Sun),
        NaiveDate::from_ymd_opt(2026, 3, 8)
    );
    assert_eq!(
        nth_weekday(2026, 12, -1, Weekday::Thu),
        NaiveDate::from_ymd_opt(2026, 12, 31)
    );
}

#[test]
fn replies_and_requests_round_trip() {
    let invite = parse(OUTLOOK).unwrap();
    let me = Person {
        name: Some("You".into()),
        email: EmailAddress::parse("you@company.com").unwrap(),
    };
    let now = utc("2026-10-16T10:00:00Z");
    for rsvp in [Rsvp::Accepted, Rsvp::Tentative, Rsvp::Declined] {
        let text = reply(&invite, &me, rsvp, now);
        assert!(text.contains(&format!("PARTSTAT={}", rsvp.partstat())));
        assert!(text.lines().all(|l| l.len() <= 76), "folded");
        let back = parse(&text).unwrap();
        assert_eq!(
            (back.method, back.uid.as_str()),
            (Method::Reply, invite.uid.as_str())
        );
        assert_eq!(back.summary, invite.summary);
        assert_eq!(Rsvp::from_key(rsvp.key()), Some(rsvp));
        assert!(!rsvp.subject_prefix().is_empty());
    }
    assert_eq!(Rsvp::from_key("maybe"), None);

    let mut mine = invite.clone();
    mine.summary = "Q4 numbers; with Priya\nand Tom".into();
    mine.location = Some("Room 4, floor 2".into());
    mine.organizer = Some(me.clone());
    mine.attendees = vec![Person {
        name: Some("Priya \"P\" Raman".into()),
        email: EmailAddress::parse("priya@company.com").unwrap(),
    }];
    let text = request(&mine, now);
    let back = parse(&text).unwrap();
    assert_eq!(back.method, Method::Request);
    assert_eq!(back.summary, mine.summary);
    assert_eq!(back.location, mine.location);
    assert_eq!(back.start, mine.start);
    assert_eq!(back.attendees[0].email.as_str(), "priya@company.com");
    assert_eq!(unescape("a\\Nb\\"), "a\nb");
}
