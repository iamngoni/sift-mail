//! Desktop notifications that respect Focus: with notifications held, only
//! Needs you mail alerts right away and the rest waits for the next check-in;
//! "everything" alerts for all new mail; "never" stays quiet. Each account's
//! own level (Needs you only, Everything, Nothing) applies first.
//!
//! The rules are a pure function; showing a notification sits behind
//! [`Notifier`] so tests never pop anything up.

use std::sync::Arc;

use chrono::{DateTime, Local, NaiveTime, Utc};
use gpui_kit::Context;

use crate::domain::choices::NotifyMode;
use crate::domain::{AccountId, TriageView};
use crate::store::NotifyLevel;

use super::app::SiftApp;

/// Shows one notification: title and body.
pub type Notifier = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// The real notifier: the operating system's notifications.
pub fn system_notifier() -> Notifier {
    Arc::new(|title: &str, body: &str| {
        // A failed notification is not worth interrupting anything for.
        let _ = notify_rust::Notification::new()
            .appname(crate::brand::PRODUCT_NAME)
            .summary(title)
            .body(body)
            .show();
    })
}

/// A newly arrived message, as far as notifications care.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    /// Its account.
    pub account: AccountId,
    /// Where it was sorted.
    pub view: TriageView,
    /// Sender's name.
    pub from: String,
    /// Subject.
    pub subject: String,
}

/// A notification to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    /// Title line.
    pub title: String,
    /// Body.
    pub body: String,
}

/// Most separate alerts at once; more become one summary.
const MAX_SEPARATE: usize = 3;

fn summarise(mail: &[Arrival], title: String) -> Alert {
    let mut senders: Vec<&str> = Vec::new();
    for m in mail {
        if !senders.contains(&m.from.as_str()) {
            senders.push(&m.from);
        }
    }
    let shown: Vec<&str> = senders.iter().take(3).copied().collect();
    let more = senders.len().saturating_sub(shown.len());
    Alert {
        title,
        body: if more > 0 {
            format!("From {} and {more} more", shown.join(", "))
        } else {
            format!("From {}", shown.join(", "))
        },
    }
}

fn alerts(mail: &[Arrival]) -> Vec<Alert> {
    if mail.len() > MAX_SEPARATE {
        return vec![summarise(mail, format!("{} new messages", mail.len()))];
    }
    mail.iter()
        .map(|m| Alert {
            title: m.from.clone(),
            body: m.subject.clone(),
        })
        .collect()
}

/// Decides what to show now and what to hold for the next check-in.
pub fn decide(
    arrivals: &[Arrival],
    mode: NotifyMode,
    level: impl Fn(AccountId) -> NotifyLevel,
) -> (Vec<Alert>, Vec<Arrival>) {
    let eligible: Vec<Arrival> = arrivals
        .iter()
        .filter(|m| match level(m.account) {
            NotifyLevel::Nothing => false,
            NotifyLevel::NeedsYou => m.view == TriageView::NeedsYou,
            NotifyLevel::Everything => true,
        })
        .cloned()
        .collect();
    match mode {
        NotifyMode::Never => (Vec::new(), Vec::new()),
        NotifyMode::All => (alerts(&eligible), Vec::new()),
        NotifyMode::Hold => {
            let (now, later): (Vec<Arrival>, Vec<Arrival>) = eligible
                .into_iter()
                .partition(|m| m.view == TriageView::NeedsYou);
            (alerts(&now), later)
        }
    }
}

/// The summary for a check-in, if anything waited.
pub fn checkin_alert(held: &[Arrival]) -> Option<Alert> {
    (!held.is_empty()).then(|| {
        summarise(
            held,
            match held.len() {
                1 => "1 message waited for your check-in".to_owned(),
                n => format!("{n} messages waited for your check-in"),
            },
        )
    })
}

/// Whether a check-in time fell between `last` and `now` (local times).
pub fn checkin_due(times: &[&str], last: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    let (last, now) = (last.with_timezone(&Local), now.with_timezone(&Local));
    times.iter().any(|t| {
        let Ok(time) = NaiveTime::parse_from_str(t, "%H:%M") else {
            return false;
        };
        let mut day = last.date_naive();
        while day <= now.date_naive() {
            if let Some(at) = day.and_time(time).and_local_timezone(Local).earliest()
                && at > last
                && at <= now
            {
                return true;
            }
            let Some(next) = day.succ_opt() else {
                return false;
            };
            day = next;
        }
        false
    })
}

impl SiftApp {
    /// Shows desktop notifications through `notifier`, or none.
    pub fn use_notifier(&mut self, notifier: Option<Notifier>) {
        self.notifier = notifier;
    }

    /// After a sync: notify about mail stored after `since`, as Focus allows.
    pub(crate) fn notify_new_mail(&mut self, since: i64) {
        let arrivals: Vec<Arrival> = self
            .store
            .new_mail_since(since, Utc::now() - chrono::Duration::hours(12))
            .unwrap_or_default()
            .into_iter()
            .map(|(account, view, from, subject)| Arrival {
                account,
                view,
                from,
                subject,
            })
            .collect();
        if arrivals.is_empty() {
            return;
        }
        let store = &self.store;
        let (now, later) = decide(&arrivals, self.notify_mode, |account| {
            store
                .account_settings(account)
                .map(|s| s.notify)
                .unwrap_or_default()
        });
        self.held.extend(later);
        if let Some(notifier) = &self.notifier {
            for alert in now {
                notifier(&alert.title, &alert.body);
            }
        }
    }

    /// Delivers held mail if a check-in time has passed since the last look.
    pub(crate) fn check_in(&mut self, now: DateTime<Utc>, cx: &mut Context<Self>) {
        let due = checkin_due(&self.checkins.times(), self.last_checkin, now);
        self.last_checkin = now;
        if !due || self.notify_mode != NotifyMode::Hold {
            return;
        }
        let held = std::mem::take(&mut self.held);
        if let (Some(alert), Some(notifier)) = (checkin_alert(&held), &self.notifier) {
            notifier(&alert.title, &alert.body);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(account: i64, view: TriageView, from: &str) -> Arrival {
        Arrival {
            account: AccountId(account),
            view,
            from: from.into(),
            subject: format!("From {from}"),
        }
    }

    #[test]
    fn hold_lets_needs_you_through_and_keeps_the_rest() {
        let new = [
            mail(1, TriageView::NeedsYou, "Priya"),
            mail(1, TriageView::Newsletters, "Product Weekly"),
            mail(2, TriageView::NeedsYou, "Mum"),
        ];
        let everything = |_| NotifyLevel::Everything;
        let (now, later) = decide(&new, NotifyMode::Hold, everything);
        assert_eq!(
            now.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(),
            ["Priya", "Mum"]
        );
        assert_eq!(later, [new[1].clone()]);
        let (now, later) = decide(&new, NotifyMode::All, everything);
        assert_eq!((now.len(), later.len()), (3, 0));
        assert_eq!(
            decide(&new, NotifyMode::Never, everything),
            (vec![], vec![])
        );
    }

    #[test]
    fn account_levels_apply_first() {
        let new = [
            mail(1, TriageView::Updates, "IT"),
            mail(2, TriageView::NeedsYou, "Mum"),
            mail(3, TriageView::Updates, "Shop"),
        ];
        let levels = |a: AccountId| match a.0 {
            1 => NotifyLevel::NeedsYou,
            2 => NotifyLevel::Nothing,
            _ => NotifyLevel::Everything,
        };
        let (now, later) = decide(&new, NotifyMode::All, levels);
        assert_eq!(
            now.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(),
            ["Shop"]
        );
        assert!(later.is_empty());
    }

    #[test]
    fn many_messages_become_one_summary() {
        let new: Vec<_> = ["A", "B", "C", "D", "A"]
            .into_iter()
            .map(|f| mail(1, TriageView::NeedsYou, f))
            .collect();
        let (now, _) = decide(&new, NotifyMode::All, |_| NotifyLevel::Everything);
        assert_eq!(
            now,
            [Alert {
                title: "5 new messages".into(),
                body: "From A, B, C and 1 more".into()
            }]
        );
        assert_eq!(checkin_alert(&[]), None);
        assert_eq!(
            checkin_alert(&new[..1]).unwrap().title,
            "1 message waited for your check-in"
        );
        assert_eq!(checkin_alert(&new[..2]).unwrap().body, "From A, B");
    }

    #[test]
    fn check_ins_fall_due_once() {
        let at = |h: u32, m: u32| {
            Local::now()
                .date_naive()
                .and_hms_opt(h, m, 0)
                .and_then(|t| t.and_local_timezone(Local).earliest())
                .unwrap()
                .with_timezone(&Utc)
        };
        assert!(checkin_due(&["12:00"], at(11, 59), at(12, 1)));
        assert!(!checkin_due(&["12:00"], at(12, 1), at(12, 5)));
        assert!(!checkin_due(&["nonsense"], at(0, 0), at(23, 0)));
        assert!(
            checkin_due(&["09:00"], at(8, 0) - chrono::Duration::days(1), at(8, 0)),
            "yesterday's counts"
        );
    }
}
