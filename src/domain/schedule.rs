//! Calendar arithmetic: working weeks, free time, and reading a "find a
//! time" request written the way you'd say it to a colleague.
//!
//! All of it works in local wall-clock time on plain values, so it is tested
//! without a clock or a time zone database.

use chrono::{Datelike as _, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

/// First hour shown and offered.
pub const DAY_START: u32 = 8;
/// Last hour shown (exclusive).
pub const DAY_END: u32 = 18;
/// Meetings are only suggested from here…
pub const WORK_START: u32 = 9;
/// …to here.
pub const WORK_END: u32 = 17;

/// The Monday of the week containing `day`.
pub fn monday_of(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

/// Monday to Friday of the week `offset` weeks from the one containing `today`.
pub fn work_week(today: NaiveDate, offset: i64) -> [NaiveDate; 5] {
    let monday = monday_of(today) + Duration::weeks(offset);
    [0, 1, 2, 3, 4].map(|d| monday + Duration::days(d))
}

/// A busy stretch, in local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy {
    /// Start.
    pub start: NaiveDateTime,
    /// End.
    pub end: NaiveDateTime,
}

/// What a "find a time" request asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRequest {
    /// Meeting length.
    pub minutes: i64,
    /// Which week: 0 this week, 1 next week.
    pub week: i64,
    /// Only this day, if the request named one ("tomorrow", "Thursday").
    pub day: Option<NaiveDate>,
    /// Words that might be a person ("Priya", "tom@company.com").
    pub people: Vec<String>,
}

const WEEKDAYS: [(&str, Weekday); 7] = [
    ("monday", Weekday::Mon),
    ("tuesday", Weekday::Tue),
    ("wednesday", Weekday::Wed),
    ("thursday", Weekday::Thu),
    ("friday", Weekday::Fri),
    ("saturday", Weekday::Sat),
    ("sunday", Weekday::Sun),
];

/// Words that are never a person's name in a request.
const NOT_PEOPLE: [&str; 30] = [
    "a", "an", "the", "with", "and", "for", "find", "time", "meet", "meeting", "call", "chat",
    "next", "this", "week", "tomorrow", "today", "min", "mins", "minute", "minutes", "hour",
    "hours", "hr", "half", "quick", "on", "at", "sometime", "me",
];

/// Reads "30 min with Priya next week", "an hour with tom@x.com tomorrow".
pub fn read_request(text: &str, today: NaiveDate) -> TimeRequest {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    let mut minutes = 30;
    for (i, w) in words.iter().enumerate() {
        let number = w
            .trim_end_matches(|c: char| c.is_alphabetic())
            .parse::<i64>()
            .ok();
        let unit = if w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(char::is_alphabetic) {
            Some(w.trim_start_matches(|c: char| c.is_ascii_digit()))
        } else {
            words.get(i + 1).copied()
        };
        match (number, unit) {
            (Some(n), Some(u)) if u.starts_with('h') => minutes = n * 60,
            (Some(n), Some(u)) if u.starts_with('m') => minutes = n,
            _ => {}
        }
        if (*w == "hour" || *w == "hr") && i > 0 && matches!(words[i - 1], "an" | "a" | "one") {
            minutes = 60;
        }
        if *w == "half" && words.get(i + 1) == Some(&"hour") {
            minutes = 30;
        }
    }
    let week = i64::from(lower.contains("next week"));
    let day = if lower.contains("tomorrow") {
        today.succ_opt()
    } else if lower.contains("today") {
        Some(today)
    } else {
        WEEKDAYS
            .iter()
            .find(|(name, _)| words.contains(name))
            .map(|(_, wd)| {
                let monday = monday_of(today) + Duration::weeks(week);
                let candidate = monday + Duration::days(i64::from(wd.num_days_from_monday()));
                if candidate < today {
                    candidate + Duration::weeks(1)
                } else {
                    candidate
                }
            })
    };
    let people = words
        .iter()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.'))
        .filter(|w| {
            w.len() > 1
                && !NOT_PEOPLE.contains(w)
                && !w.chars().all(|c| c.is_ascii_digit())
                && !w.chars().next().is_some_and(|c| c.is_ascii_digit())
                && !WEEKDAYS.iter().any(|(name, _)| name == w)
        })
        .map(str::to_owned)
        .collect();
    TimeRequest {
        minutes: minutes.clamp(15, 240),
        week,
        day,
        people,
    }
}

/// Up to `count` free slots of `minutes` on `days`, during working hours,
/// after `now`, on the half hour. Prefers one per day so the choices differ.
pub fn free_slots(
    days: &[NaiveDate],
    busy: &[Busy],
    minutes: i64,
    now: NaiveDateTime,
    count: usize,
) -> Vec<Busy> {
    let fits = |start: NaiveDateTime| {
        let end = start + Duration::minutes(minutes);
        start >= now && !busy.iter().any(|b| b.start < end && b.end > start)
    };
    let mut per_day: Vec<Vec<Busy>> = Vec::new();
    for day in days {
        let mut slots = Vec::new();
        let mut t =
            day.and_time(NaiveTime::from_hms_opt(WORK_START, 0, 0).unwrap_or(NaiveTime::MIN));
        let last = day.and_time(NaiveTime::from_hms_opt(WORK_END, 0, 0).unwrap_or(NaiveTime::MIN));
        while t + Duration::minutes(minutes) <= last {
            if fits(t) {
                slots.push(Busy {
                    start: t,
                    end: t + Duration::minutes(minutes),
                });
            }
            t += Duration::minutes(30);
        }
        per_day.push(slots);
    }
    // First the earliest free slot of each day, then the rest in order.
    let mut out: Vec<Busy> = per_day.iter().filter_map(|s| s.first().copied()).collect();
    out.truncate(count);
    for slot in per_day.iter().flatten() {
        if out.len() >= count {
            break;
        }
        if !out.contains(slot) {
            out.push(*slot);
        }
    }
    out.sort_by_key(|s| s.start);
    out
}

/// Hours as the design writes them: "6h", "4.5h", "0h".
pub fn hours_label(minutes: i64) -> String {
    let tenths = (minutes * 10 + 30) / 60;
    if tenths % 10 == 0 {
        format!("{}h", tenths / 10)
    } else {
        format!("{}.{}h", tenths / 10, tenths % 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    /// Friday 16 October 2026.
    fn today() -> NaiveDate {
        date("2026-10-16")
    }

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(monday_of(today()), date("2026-10-12"));
        assert_eq!(
            monday_of(date("2026-10-18")),
            date("2026-10-12"),
            "Sunday belongs to the week before"
        );
        let next = work_week(today(), 1);
        assert_eq!((next[0], next[4]), (date("2026-10-19"), date("2026-10-23")));
    }

    #[test]
    fn requests_read_the_way_people_write_them() {
        let r = read_request("30 min with Priya next week", today());
        assert_eq!((r.minutes, r.week, r.day), (30, 1, None));
        assert_eq!(r.people, ["priya"]);
        let r = read_request("An hour with tom@company.com tomorrow", today());
        assert_eq!((r.minutes, r.day), (60, Some(date("2026-10-17"))));
        assert_eq!(r.people, ["tom@company.com"]);
        assert_eq!(read_request("45mins Thursday", today()).minutes, 45);
        assert_eq!(
            read_request("Thursday", today()).day,
            Some(date("2026-10-22")),
            "a past day means next week"
        );
        assert_eq!(read_request("friday", today()).day, Some(today()));
        assert_eq!(read_request("2 hours", today()).minutes, 120);
        assert_eq!(read_request("half hour today", today()).day, Some(today()));
        assert_eq!(read_request("1hr", today()).minutes, 60);
        assert_eq!(read_request("8 hours", today()).minutes, 240, "clamped");
        assert!(read_request("", today()).people.is_empty());
    }

    #[test]
    fn free_slots_avoid_busy_time_and_spread_out() {
        let days = work_week(today(), 1);
        let busy = [
            Busy {
                start: at("2026-10-19 09:00"),
                end: at("2026-10-19 10:30"),
            },
            Busy {
                start: at("2026-10-20 09:00"),
                end: at("2026-10-20 17:00"),
            },
        ];
        let slots = free_slots(&days, &busy, 30, at("2026-10-16 12:00"), 3);
        let starts: Vec<_> = slots.iter().map(|s| s.start).collect();
        assert_eq!(
            starts,
            [
                at("2026-10-19 10:30"),
                at("2026-10-21 09:00"),
                at("2026-10-22 09:00")
            ]
        );
        // Past times are never offered.
        let today_only = free_slots(&[today()], &[], 60, at("2026-10-16 15:10"), 3);
        assert_eq!(
            today_only.iter().map(|s| s.start).collect::<Vec<_>>(),
            [at("2026-10-16 15:30"), at("2026-10-16 16:00")]
        );
        // Fewer days than slots: the rest come from the same day.
        assert_eq!(
            free_slots(&[date("2026-10-19")], &[], 30, at("2026-10-16 00:00"), 3).len(),
            3
        );
        assert!(free_slots(&[], &[], 30, at("2026-10-16 00:00"), 3).is_empty());
    }

    #[test]
    fn hours_read_naturally() {
        assert_eq!(hours_label(360), "6h");
        assert_eq!(hours_label(270), "4.5h");
        assert_eq!(hours_label(0), "0h");
        assert_eq!(hours_label(15), "0.3h");
    }
}
