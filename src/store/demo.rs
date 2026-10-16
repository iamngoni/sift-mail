//! Sample mail for development builds and tests, mirroring the approved design.
//!
//! Compiled only for tests and with `--features demo`. Production builds never
//! contain this module, so there is no fallback path that shows fake mail.

use chrono::{DateTime, Duration, Utc};

use super::{EventKind, NewEvent};
use super::{MailStore, StoreResult, parse_email};
use crate::domain::ical::{Invite, Method, Person};
use crate::domain::{AccountId, Contact, IncomingMessage, Provider, Signals, ThreadRef};

struct Sample {
    remote: &'static str,
    thread: &'static str,
    name: Option<&'static str>,
    email: &'static str,
    subject: &'static str,
    body: &'static str,
    minutes_ago: i64,
    read: bool,
    attachments: bool,
    signals: Signals,
}

const TO_ME: Signals = Signals {
    mailing_list: false,
    calendar_invite: false,
    auto_submitted: false,
    addressed_to_me: true,
};
const INVITE: Signals = Signals {
    calendar_invite: true,
    ..TO_ME
};
const LIST: Signals = Signals {
    mailing_list: true,
    ..TO_ME
};
const ROBOT: Signals = Signals {
    auto_submitted: true,
    ..TO_ME
};
const CC: Signals = Signals {
    addressed_to_me: false,
    ..TO_ME
};

const SAMPLES: &[Sample] = &[
    Sample {
        remote: "m1a",
        thread: "q4",
        name: Some("Priya Raman"),
        email: "priya@company.com",
        subject: "Q4 budget numbers",
        body: "Heads up: the Q4 board pack is due next week. I will need numbers from each team lead.",
        minutes_ago: 60 * 24 * 4,
        read: true,
        attachments: false,
        signals: TO_ME,
    },
    Sample {
        remote: "m1b",
        thread: "q4",
        name: Some("Priya Raman"),
        email: "priya@company.com",
        subject: "Re: Q4 budget numbers",
        body: "Hi, hope your week has been OK.\n\nI am pulling together the board pack and still need two things from your side: the headcount estimate for next quarter and a breakdown of tooling costs.\n\nCould you get it back to me by 17:00 on Monday?\n\nThanks so much,\nPriya",
        minutes_ago: 18,
        read: false,
        attachments: true,
        signals: TO_ME,
    },
    Sample {
        remote: "m2",
        thread: "roadmap",
        name: Some("Daniel Mensah"),
        email: "daniel@company.com",
        subject: "Invitation: Roadmap sync",
        body: "Tue 20 Oct, 14:00–14:30 on Teams. Agenda: Q1 priorities and the top three bets.",
        minutes_ago: 5,
        read: false,
        attachments: false,
        signals: INVITE,
    },
    Sample {
        remote: "m3",
        thread: "vendor",
        name: Some("Tom Okafor"),
        email: "tom@company.com",
        subject: "Vendor contract renewal",
        body: "Morning!\n\nLegal wants to sign the renewal on Monday. The only material change from last year is a 12-month lock-in (section 4).\n\nCan you give me a yes or no before then?\n\nTom",
        minutes_ago: 105,
        read: false,
        attachments: true,
        signals: TO_ME,
    },
    Sample {
        remote: "m4",
        thread: "ticket",
        name: Some("IT Service Desk"),
        email: "no-reply@servicedesk.company.com",
        subject: "Ticket 4821 resolved",
        body: "Your request \"VPN access for contractors\" has been resolved.",
        minutes_ago: 122,
        read: false,
        attachments: false,
        signals: TO_ME,
    },
    Sample {
        remote: "m5",
        thread: "weekly",
        name: Some("Product Weekly"),
        email: "hello@productweekly.com",
        subject: "Issue 112: pricing pages that convert",
        body: "This week: what the best pricing pages have in common, and three to steal from.",
        minutes_ago: 240,
        read: false,
        attachments: false,
        signals: LIST,
    },
    Sample {
        remote: "m6",
        thread: "onboarding",
        name: Some("Lena Fischer"),
        email: "lena@company.com",
        subject: "Who owns the onboarding doc?",
        body: "Hey, quick one: who owns the onboarding doc these days? A couple of links in it are broken.",
        minutes_ago: 60 * 18,
        read: true,
        attachments: false,
        signals: TO_ME,
    },
    Sample {
        remote: "m7",
        thread: "expenses",
        name: Some("Finance Ops"),
        email: "finance@company.com",
        subject: "Expense report approved",
        body: "Your September expense report has been approved and will be paid with the next payroll.",
        minutes_ago: 60 * 26,
        read: true,
        attachments: false,
        signals: ROBOT,
    },
    Sample {
        remote: "m8",
        thread: "offsite",
        name: Some("Kofi Asante"),
        email: "kofi@company.com",
        subject: "Re: Offsite planning",
        body: "Lisbon it is, 6–7 Nov. Can every lead send headcount and dietary needs by Wednesday?",
        minutes_ago: 60 * 48,
        read: false,
        attachments: false,
        signals: CC,
    },
];

/// Fills an empty store with one demo account and the sample threads.
/// Does nothing if any account already exists.
pub fn seed(store: &MailStore, now: DateTime<Utc>) -> StoreResult<bool> {
    if !store.accounts()?.is_empty() {
        return Ok(false);
    }
    let address = parse_email("you@company.com")?;
    let account = store.add_account("Work", &address, Provider::Microsoft365)?;
    for sample in SAMPLES {
        let id = store.upsert(&IncomingMessage {
            account: account.id,
            remote_id: sample.remote.into(),
            thread_key: sample.thread.into(),
            from: Contact::new(sample.name, parse_email(sample.email)?),
            subject: sample.subject.into(),
            body_text: sample.body.into(),
            received_at: now - Duration::minutes(sample.minutes_ago),
            is_read: sample.read,
            has_attachments: sample.attachments,
            signals: sample.signals,
            message_id: Some(format!("{}@demo.sift", sample.remote)),
            references: Vec::new(),
        })?;
        if sample.attachments {
            let name = if sample.subject.contains("budget") {
                "Q4-headcount-template.xlsx"
            } else {
                "Vendor-agreement-v3.pdf"
            };
            store.save_attachments(
                id,
                &[crate::sync::parse::AttachmentData {
                    name: name.into(),
                    mime: "application/octet-stream".into(),
                    bytes: vec![0; 48 * 1024],
                }],
            )?;
        }
    }
    seed_calendar(store, account.id, now)?;
    Ok(true)
}

/// A week of meetings, focus time and one unanswered invitation, around `now`.
fn seed_calendar(store: &MailStore, account: AccountId, now: DateTime<Utc>) -> StoreResult<()> {
    use chrono::{Datelike as _, Local, NaiveTime, TimeZone as _};
    let today = now.with_timezone(&Local).date_naive();
    let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
    let at = |day: i64, h: u32, m: u32| {
        let naive = (monday + Duration::days(day))
            .and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap_or(NaiveTime::MIN));
        Local
            .from_local_datetime(&naive)
            .earliest()
            .map_or(now, |t| t.with_timezone(&Utc))
    };
    let person = |name: &str, email: &str| -> StoreResult<Person> {
        Ok(Person {
            name: Some(name.to_owned()),
            email: parse_email(email)?,
        })
    };
    let me = person("You", "you@company.com")?;
    let mut n = 0;
    let mut add = |title: &str, start, end, kind, with: Vec<Person>| -> StoreResult<()> {
        n += 1;
        store.add_event(
            &NewEvent {
                account,
                title: title.to_owned(),
                start,
                end,
                kind,
                attendees: with,
                uid: format!("demo-{n}"),
            },
            Some(&me),
        )?;
        Ok(())
    };
    for day in 0..5 {
        add(
            "Standup",
            at(day, 9, 30),
            at(day, 9, 45),
            EventKind::Meeting,
            vec![],
        )?;
    }
    add(
        "Focus: Q4 planning",
        at(0, 13, 0),
        at(0, 15, 0),
        EventKind::Focus,
        vec![],
    )?;
    add(
        "1:1 with Aisha",
        at(2, 10, 0),
        at(2, 10, 30),
        EventKind::Meeting,
        vec![person("Aisha Bello", "aisha@company.com")?],
    )?;
    add(
        "Design review",
        at(3, 11, 0),
        at(3, 12, 0),
        EventKind::Meeting,
        vec![person("Lena Fischer", "lena@company.com")?],
    )?;
    add(
        "Focus: deep work",
        at(3, 15, 0),
        at(3, 17, 0),
        EventKind::Focus,
        vec![],
    )?;
    add(
        "Lunch with Sam",
        at(4, 12, 30),
        at(4, 13, 30),
        EventKind::Personal,
        vec![],
    )?;
    let start = at(8, 14, 0);
    store.apply_invite(
        account,
        &Invite {
            method: Method::Request,
            uid: "demo-roadmap".to_owned(),
            sequence: 0,
            summary: "Roadmap sync".to_owned(),
            start,
            end: start + Duration::minutes(30),
            all_day: false,
            location: Some("Microsoft Teams".to_owned()),
            organizer: Some(person("Daniel Mensah", "daniel@company.com")?),
            attendees: vec![me.clone(), person("Priya Raman", "priya@company.com")?],
        },
        Some(&ThreadRef {
            account,
            key: "roadmap".to_owned(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::EmailAddress;
    use crate::domain::TriageView;
    use crate::store::ThreadQuery;

    #[test]
    fn every_sample_address_is_valid() {
        for sample in SAMPLES {
            assert!(
                EmailAddress::parse(sample.email).is_ok(),
                "{}",
                sample.email
            );
        }
    }

    #[test]
    fn seeds_once_and_sorts_into_every_view() {
        let store = MailStore::open_in_memory().unwrap();
        let now = Utc::now();
        assert!(seed(&store, now).unwrap());
        assert!(!seed(&store, now).unwrap(), "second seed is a no-op");
        let counts = store.counts(None).unwrap();
        assert_eq!(counts.get(TriageView::All), 8);
        for view in [
            TriageView::NeedsYou,
            TriageView::Meetings,
            TriageView::Updates,
            TriageView::Newsletters,
            TriageView::Other,
        ] {
            assert!(counts.get(view) > 0, "{view:?} is empty");
        }
        let all = store.threads(ThreadQuery::default()).unwrap();
        assert_eq!(all[0].subject, "Invitation: Roadmap sync");
    }
}
