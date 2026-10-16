//! How the inbox talks about time: day sections and short timestamps.
//!
//! All functions take "now" and a time zone explicitly so they are pure and
//! testable; the UI passes the local clock.

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};

/// The centred section a row sits under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DayBucket {
    /// Same local calendar day as now.
    Today,
    /// The local day before.
    Yesterday,
    /// Earlier in the same Monday-to-Sunday week.
    EarlierThisWeek,
    /// Anything older, or in the future because of clock skew.
    Older,
}

impl DayBucket {
    /// Section label, upper-cased by the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::EarlierThisWeek => "Earlier this week",
            Self::Older => "Older",
        }
    }
}

/// Which section `at` belongs to, seen from `now` in `tz`.
pub fn bucket<Tz: TimeZone>(at: DateTime<Utc>, now: DateTime<Utc>, tz: &Tz) -> DayBucket {
    let day = at.with_timezone(tz).date_naive();
    let today = now.with_timezone(tz).date_naive();
    if day == today {
        DayBucket::Today
    } else if Some(day) == today.pred_opt() {
        DayBucket::Yesterday
    } else if day < today && same_week(day, today) {
        DayBucket::EarlierThisWeek
    } else {
        DayBucket::Older
    }
}

fn same_week(a: NaiveDate, b: NaiveDate) -> bool {
    a.iso_week() == b.iso_week()
}

/// The short time on the right of a row: `9:42` today, `Wed 14` this week,
/// `12 Oct` this year, `12 Oct 2025` before that.
pub fn short_label<Tz: TimeZone>(at: DateTime<Utc>, now: DateTime<Utc>, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let local = at.with_timezone(tz);
    let today = now.with_timezone(tz).date_naive();
    match bucket(at, now, tz) {
        DayBucket::Today => local.format("%-H:%M").to_string(),
        DayBucket::Yesterday | DayBucket::EarlierThisWeek => local.format("%a %-d").to_string(),
        DayBucket::Older if local.year() == today.year() => local.format("%-d %b").to_string(),
        DayBucket::Older => local.format("%-d %b %Y").to_string(),
    }
}

/// The longer form used in a thread header, such as `Fri 16 Oct, 9:42`.
pub fn long_label<Tz: TimeZone>(at: DateTime<Utc>, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    at.with_timezone(tz).format("%a %-d %b, %-H:%M").to_string()
}

/// A snooze choice offered in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozeOption {
    /// Menu label, such as "Tomorrow".
    pub label: &'static str,
    /// When the thread comes back, shown next to the label.
    pub when: String,
    /// The wake-up time, UTC.
    pub until: DateTime<Utc>,
}

/// The four snooze choices: three hours from now, tomorrow at 08:00, next
/// Monday at 08:00 and the Monday after, all in local time.
pub fn snooze_options<Tz: TimeZone>(now: DateTime<Utc>, tz: &Tz) -> Vec<SnoozeOption>
where
    Tz::Offset: std::fmt::Display,
{
    let local_now = now.with_timezone(tz);
    let today = local_now.date_naive();
    let at_eight = |day: NaiveDate| {
        day.and_hms_opt(8, 0, 0)
            .and_then(|naive| tz.from_local_datetime(&naive).earliest())
            .map(|dt| dt.with_timezone(&Utc))
    };
    let later = now + chrono::Duration::hours(3);
    let tomorrow = today.succ_opt().and_then(at_eight);
    let days_to_monday = 7 - i64::from(today.weekday().num_days_from_monday());
    let monday = today
        .checked_add_signed(chrono::Duration::days(days_to_monday))
        .and_then(at_eight);
    let mut options = vec![SnoozeOption {
        label: "Later today",
        when: later.with_timezone(tz).format("%H:%M").to_string(),
        until: later,
    }];
    if let Some(until) = tomorrow {
        options.push(SnoozeOption {
            label: "Tomorrow",
            when: until.with_timezone(tz).format("%a %H:%M").to_string(),
            until,
        });
    }
    if let Some(until) = monday {
        options.push(SnoozeOption {
            label: "Monday",
            when: until.with_timezone(tz).format("%-d %b, %H:%M").to_string(),
            until,
        });
        let week = until + chrono::Duration::days(7);
        options.push(SnoozeOption {
            label: "Next week",
            when: week
                .with_timezone(tz)
                .format("Mon %-d %b, %H:%M")
                .to_string(),
            until: week,
        });
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// Friday 16 October 2026, 10:00 in UTC+2.
    fn now() -> DateTime<Utc> {
        utc("2026-10-16T08:00:00Z")
    }

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }

    #[test]
    fn buckets_follow_local_calendar_days() {
        assert_eq!(
            bucket(utc("2026-10-16T07:42:00Z"), now(), &tz()),
            DayBucket::Today
        );
        // 23:30 UTC on the 15th is 01:30 on the 16th in UTC+2.
        assert_eq!(
            bucket(utc("2026-10-15T23:30:00Z"), now(), &tz()),
            DayBucket::Today
        );
        assert_eq!(
            bucket(utc("2026-10-15T09:00:00Z"), now(), &tz()),
            DayBucket::Yesterday
        );
        assert_eq!(
            bucket(utc("2026-10-12T09:00:00Z"), now(), &tz()),
            DayBucket::EarlierThisWeek
        );
        assert_eq!(
            bucket(utc("2026-10-11T09:00:00Z"), now(), &tz()),
            DayBucket::Older
        );
        assert_eq!(
            bucket(utc("2026-10-20T09:00:00Z"), now(), &tz()),
            DayBucket::Older
        );
    }

    #[test]
    fn short_labels_shrink_with_distance() {
        assert_eq!(
            short_label(utc("2026-10-16T07:42:00Z"), now(), &tz()),
            "9:42"
        );
        assert_eq!(
            short_label(utc("2026-10-14T09:00:00Z"), now(), &tz()),
            "Wed 14"
        );
        assert_eq!(
            short_label(utc("2026-09-03T09:00:00Z"), now(), &tz()),
            "3 Sep"
        );
        assert_eq!(
            short_label(utc("2025-12-24T09:00:00Z"), now(), &tz()),
            "24 Dec 2025"
        );
        assert_eq!(
            long_label(utc("2026-10-16T07:42:00Z"), &tz()),
            "Fri 16 Oct, 9:42"
        );
    }

    #[test]
    fn labels_are_distinct_and_ordered() {
        let all = [
            DayBucket::Today,
            DayBucket::Yesterday,
            DayBucket::EarlierThisWeek,
            DayBucket::Older,
        ];
        assert!(all.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(
            all.map(DayBucket::label),
            ["Today", "Yesterday", "Earlier this week", "Older"]
        );
    }
    #[test]
    fn snooze_options_land_on_sensible_local_times() {
        let options = snooze_options(now(), &tz());
        let labels: Vec<_> = options.iter().map(|o| o.label).collect();
        assert_eq!(labels, ["Later today", "Tomorrow", "Monday", "Next week"]);
        assert_eq!(options[0].until, now() + chrono::Duration::hours(3));
        assert_eq!(options[0].when, "13:00");
        // Saturday 17 Oct, 08:00 in UTC+2 is 06:00 UTC.
        assert_eq!(options[1].until, utc("2026-10-17T06:00:00Z"));
        assert_eq!(options[1].when, "Sat 08:00");
        assert_eq!(options[2].until, utc("2026-10-19T06:00:00Z"));
        assert_eq!(options[2].when, "19 Oct, 08:00");
        assert_eq!(options[3].until, utc("2026-10-26T06:00:00Z"));
        assert_eq!(options[3].when, "Mon 26 Oct, 08:00");
    }
}
