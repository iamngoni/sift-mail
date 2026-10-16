//! Rule-based sorting of new mail into views.
//!
//! This is the deterministic baseline that always runs. A later AI pass may
//! refine it, but mail is never left unsorted because AI is off or offline.
//! Rules are checked in a fixed order; the first match wins.

use super::message::{Contact, Signals};
use super::view::TriageView;

/// Local parts that almost always mean a machine sent the message.
const AUTOMATED_LOCAL_PARTS: [&str; 10] = [
    "noreply",
    "no-reply",
    "donotreply",
    "do-not-reply",
    "notifications",
    "notification",
    "alerts",
    "mailer-daemon",
    "postmaster",
    "bounce",
];

/// Picks the view for a message.
///
/// Order: calendar invites → Meetings; mailing lists → Newsletters; anything
/// automated → Updates; mail sent to you directly by a person → Needs you;
/// everything else (CC'd threads, unknown) → Other.
pub fn classify(from: &Contact, signals: Signals) -> TriageView {
    if signals.calendar_invite {
        TriageView::Meetings
    } else if signals.mailing_list {
        TriageView::Newsletters
    } else if signals.auto_submitted || looks_automated(from) {
        TriageView::Updates
    } else if signals.addressed_to_me {
        TriageView::NeedsYou
    } else {
        TriageView::Other
    }
}

/// True when the sender address looks like a robot, such as `no-reply@…`.
pub fn looks_automated(from: &Contact) -> bool {
    let local = from.email().local_part().to_ascii_lowercase();
    AUTOMATED_LOCAL_PARTS
        .iter()
        .any(|robot| local == *robot || local.starts_with(&format!("{robot}+")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::EmailAddress;

    fn person() -> Contact {
        Contact::new(
            Some("Priya Raman"),
            EmailAddress::parse("priya@company.com").unwrap(),
        )
    }

    fn robot(local: &str) -> Contact {
        Contact::new(
            None,
            EmailAddress::parse(&format!("{local}@service.com")).unwrap(),
        )
    }

    #[test]
    fn invites_win_over_everything() {
        let s = Signals {
            calendar_invite: true,
            mailing_list: true,
            auto_submitted: true,
            addressed_to_me: true,
        };
        assert_eq!(classify(&person(), s), TriageView::Meetings);
    }

    #[test]
    fn mailing_lists_are_newsletters() {
        let s = Signals {
            mailing_list: true,
            addressed_to_me: true,
            ..Signals::default()
        };
        assert_eq!(classify(&person(), s), TriageView::Newsletters);
    }

    #[test]
    fn automated_mail_is_an_update() {
        let header = Signals {
            auto_submitted: true,
            addressed_to_me: true,
            ..Signals::default()
        };
        assert_eq!(classify(&person(), header), TriageView::Updates);
        let direct = Signals {
            addressed_to_me: true,
            ..Signals::default()
        };
        assert_eq!(classify(&robot("no-reply"), direct), TriageView::Updates);
        assert_eq!(
            classify(&robot("Notifications+gh"), direct),
            TriageView::Updates
        );
    }

    #[test]
    fn people_writing_to_you_need_you_and_cc_is_other() {
        let to_me = Signals {
            addressed_to_me: true,
            ..Signals::default()
        };
        assert_eq!(classify(&person(), to_me), TriageView::NeedsYou);
        assert_eq!(classify(&person(), Signals::default()), TriageView::Other);
    }

    #[test]
    fn similar_looking_people_are_not_robots() {
        assert!(!looks_automated(&robot("noreplyjoe")));
        assert!(!looks_automated(&person()));
        assert!(looks_automated(&robot("mailer-daemon")));
    }
}
