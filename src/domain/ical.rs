//! Meeting invitations: reading iCalendar (RFC 5545) from mail and writing
//! iMIP (RFC 6047) requests and replies.
//!
//! Only what invitations use is supported: one VEVENT with start, end,
//! summary, location, organizer and attendees, and the VTIMEZONE rules
//! Outlook and Google send (a standard and a daylight offset with yearly
//! BYMONTH/BYDAY transitions). Anything else is ignored, never trusted.

use chrono::{
    DateTime, Datelike as _, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday,
};

use super::email::EmailAddress;

/// What an invitation asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// A new or updated meeting.
    Request,
    /// The meeting was called off.
    Cancel,
    /// Someone answered an invitation.
    Reply,
    /// Anything else (PUBLISH and friends): shown, never answered.
    Other,
}

/// An answer to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rsvp {
    /// Going.
    Accepted,
    /// Maybe.
    Tentative,
    /// Not going.
    Declined,
}

impl Rsvp {
    /// The iCalendar PARTSTAT value.
    pub fn partstat(self) -> &'static str {
        match self {
            Self::Accepted => "ACCEPTED",
            Self::Tentative => "TENTATIVE",
            Self::Declined => "DECLINED",
        }
    }

    /// Stored form.
    pub fn key(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Tentative => "tentative",
            Self::Declined => "declined",
        }
    }

    /// Parses [`Rsvp::key`].
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "accepted" => Some(Self::Accepted),
            "tentative" => Some(Self::Tentative),
            "declined" => Some(Self::Declined),
            _ => None,
        }
    }

    /// Subject prefix for the reply, as Outlook writes it.
    pub fn subject_prefix(self) -> &'static str {
        match self {
            Self::Accepted => "Accepted",
            Self::Tentative => "Tentative",
            Self::Declined => "Declined",
        }
    }
}

/// One invitation, read from a `text/calendar` part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// What it asks for.
    pub method: Method,
    /// Stable id across updates.
    pub uid: String,
    /// Higher means newer.
    pub sequence: u32,
    /// Title.
    pub summary: String,
    /// Start, UTC.
    pub start: DateTime<Utc>,
    /// End, UTC.
    pub end: DateTime<Utc>,
    /// A whole-day event.
    pub all_day: bool,
    /// Room or link.
    pub location: Option<String>,
    /// Who sent it.
    pub organizer: Option<Person>,
    /// Who's invited.
    pub attendees: Vec<Person>,
}

/// An organizer or attendee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    /// Display name, if given.
    pub name: Option<String>,
    /// Address.
    pub email: EmailAddress,
}

impl Person {
    /// Name if known, else the address.
    pub fn display(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.email.to_string())
    }
}

/// A content line: name, parameters, value.
#[derive(Debug, Clone)]
struct Line {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Line {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// Unfolds and splits content lines (RFC 5545 §3.1).
fn lines(text: &str) -> Vec<Line> {
    let mut unfolded: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = unfolded.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        if !raw.is_empty() {
            unfolded.push(raw.to_owned());
        }
    }
    unfolded
        .into_iter()
        .filter_map(|l| {
            // The value starts at the first colon outside a quoted parameter.
            let mut quoted = false;
            let split = l.char_indices().find(|(_, c)| {
                if *c == '"' {
                    quoted = !quoted;
                }
                *c == ':' && !quoted
            })?;
            let (head, value) = (&l[..split.0], &l[split.0 + 1..]);
            let mut parts = head.split(';');
            let name = parts.next()?.trim().to_ascii_uppercase();
            let params = parts
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| {
                    (
                        k.trim().to_ascii_uppercase(),
                        v.trim_matches('"').to_owned(),
                    )
                })
                .collect();
            Some(Line {
                name,
                params,
                value: value.to_owned(),
            })
        })
        .collect()
}

/// Undoes TEXT escaping (§3.3.11).
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Applies TEXT escaping.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

/// A time zone from a VTIMEZONE: transitions in local time.
#[derive(Debug, Clone, Default)]
struct Zone {
    id: String,
    /// (month, nth weekday or -1 for last, weekday, local time, offset after, in minutes)
    rules: Vec<(u32, i32, Weekday, NaiveTime, i32)>,
    /// Offset when there are no rules.
    fixed: Option<i32>,
}

fn offset_minutes(text: &str) -> Option<i32> {
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => (1, text),
    };
    let hours: i32 = digits.get(0..2)?.parse().ok()?;
    let minutes: i32 = digits.get(2..4).unwrap_or("00").parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

/// The `nth` (1-based, or -1 for last) `day` of `month` in `year`.
fn nth_weekday(year: i32, month: u32, nth: i32, day: Weekday) -> Option<NaiveDate> {
    if nth > 0 {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let shift = (7 + day.num_days_from_monday() - first.weekday().num_days_from_monday()) % 7;
        first.checked_add_signed(Duration::days(i64::from(shift) + 7 * i64::from(nth - 1)))
    } else {
        let next = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)?
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1)?
        };
        let last = next.pred_opt()?;
        let shift = (7 + last.weekday().num_days_from_monday() - day.num_days_from_monday()) % 7;
        last.checked_sub_signed(Duration::days(i64::from(shift)))
    }
}

impl Zone {
    /// The UTC offset in minutes at a local time.
    fn offset_at(&self, local: NaiveDateTime) -> i32 {
        let year = local.year();
        let mut best: Option<(NaiveDateTime, i32)> = None;
        for &(month, nth, day, at, offset) in &self.rules {
            for y in [year - 1, year] {
                if let Some(date) = nth_weekday(y, month, nth, day) {
                    let when = date.and_time(at);
                    if when <= local && best.is_none_or(|(b, _)| when > b) {
                        best = Some((when, offset));
                    }
                }
            }
        }
        best.map(|(_, o)| o).or(self.fixed).unwrap_or(0)
    }
}

fn parse_zones(all: &[Line]) -> Vec<Zone> {
    let mut zones = Vec::new();
    let mut zone: Option<Zone> = None;
    let mut part: Option<(Option<NaiveDateTime>, Option<i32>, Option<String>)> = None;
    for l in all {
        match (l.name.as_str(), l.value.as_str()) {
            ("BEGIN", "VTIMEZONE") => zone = Some(Zone::default()),
            ("END", "VTIMEZONE") => zones.extend(zone.take()),
            ("TZID", _) => {
                if let Some(z) = &mut zone {
                    z.id = l.value.clone();
                }
            }
            ("BEGIN", "STANDARD" | "DAYLIGHT") => part = Some((None, None, None)),
            ("DTSTART", _) if part.is_some() => {
                if let Some(p) = &mut part {
                    p.0 = NaiveDateTime::parse_from_str(&l.value, "%Y%m%dT%H%M%S").ok();
                }
            }
            ("TZOFFSETTO", _) => {
                if let Some(p) = &mut part {
                    p.1 = offset_minutes(&l.value);
                }
            }
            ("RRULE", _) => {
                if let Some(p) = &mut part {
                    p.2 = Some(l.value.clone());
                }
            }
            ("END", "STANDARD" | "DAYLIGHT") => {
                let (Some(z), Some((start, Some(offset), rule))) = (&mut zone, part.take()) else {
                    continue;
                };
                let at = start.map_or(NaiveTime::MIN, |s| s.time());
                let fields = |key: &str| {
                    rule.as_deref().and_then(|r| {
                        r.split(';')
                            .filter_map(|kv| kv.split_once('='))
                            .find(|(k, _)| *k == key)
                            .map(|(_, v)| v.to_owned())
                    })
                };
                let month = fields("BYMONTH").and_then(|m| m.parse::<u32>().ok());
                let byday = fields("BYDAY");
                match (month, byday) {
                    (Some(month), Some(byday)) if byday.len() >= 2 => {
                        let (n, code) = byday.split_at(byday.len() - 2);
                        let nth = if n.is_empty() {
                            1
                        } else {
                            n.parse().unwrap_or(1)
                        };
                        if let Some(day) = weekday(code) {
                            z.rules.push((month, nth, day, at, offset));
                        }
                    }
                    _ => z.fixed = Some(offset),
                }
            }
            _ => {}
        }
    }
    zones
}

/// Reads a DTSTART/DTEND line to UTC. Returns the time and whether it's a date.
fn when(line: &Line, zones: &[Zone]) -> Option<(DateTime<Utc>, bool)> {
    let value = line.value.trim();
    if line.param("VALUE") == Some("DATE") || (value.len() == 8 && !value.contains('T')) {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        return Some((Utc.from_utc_datetime(&date.and_time(NaiveTime::MIN)), true));
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&naive), false));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let offset = line
        .param("TZID")
        .and_then(|id| zones.iter().find(|z| z.id == id))
        .map_or_else(
            // Floating times and unknown zones are taken as the computer's local time.
            || {
                chrono::Local
                    .from_local_datetime(&naive)
                    .earliest()
                    .map_or(0, |t| t.offset().local_minus_utc() / 60)
            },
            |z| z.offset_at(naive),
        );
    Some((
        Utc.from_utc_datetime(&(naive - Duration::minutes(i64::from(offset)))),
        false,
    ))
}

fn person(line: &Line) -> Option<Person> {
    let address = line.value.trim();
    let address = address
        .strip_prefix("mailto:")
        .or_else(|| address.strip_prefix("MAILTO:"))
        .unwrap_or(address);
    Some(Person {
        name: line
            .param("CN")
            .map(str::to_owned)
            .filter(|n| !n.trim().is_empty()),
        email: EmailAddress::parse(address).ok()?,
    })
}

/// Reads the first VEVENT of an iCalendar text. `None` if there isn't a
/// usable one (no UID or no start).
pub fn parse(text: &str) -> Option<Invite> {
    let all = lines(text);
    let zones = parse_zones(&all);
    let method = match all
        .iter()
        .find(|l| l.name == "METHOD")
        .map(|l| l.value.trim().to_ascii_uppercase())
        .as_deref()
    {
        Some("REQUEST") => Method::Request,
        Some("CANCEL") => Method::Cancel,
        Some("REPLY") => Method::Reply,
        _ => Method::Other,
    };
    let start_at = all
        .iter()
        .position(|l| l.name == "BEGIN" && l.value.trim() == "VEVENT")?;
    let event: Vec<&Line> = all[start_at..]
        .iter()
        .take_while(|l| !(l.name == "END" && l.value.trim() == "VEVENT"))
        .collect();
    let get = |name: &str| event.iter().find(|l| l.name == name).copied();
    let uid = get("UID")?.value.trim().to_owned();
    let (start, all_day) = when(get("DTSTART")?, &zones)?;
    let end = get("DTEND")
        .and_then(|l| when(l, &zones))
        .map(|(e, _)| e)
        .or_else(|| {
            get("DURATION")
                .and_then(|d| duration(&d.value))
                .map(|d| start + d)
        })
        .unwrap_or(
            start
                + if all_day {
                    Duration::days(1)
                } else {
                    Duration::minutes(30)
                },
        );
    Some(Invite {
        method,
        uid,
        sequence: get("SEQUENCE")
            .and_then(|l| l.value.trim().parse().ok())
            .unwrap_or(0),
        summary: get("SUMMARY").map_or_else(|| "(no title)".to_owned(), |l| unescape(&l.value)),
        start,
        end: end.max(start),
        all_day,
        location: get("LOCATION")
            .map(|l| unescape(&l.value))
            .filter(|s| !s.trim().is_empty()),
        organizer: get("ORGANIZER").and_then(person),
        attendees: event
            .iter()
            .filter(|l| l.name == "ATTENDEE")
            .filter_map(|l| person(l))
            .collect(),
    })
}

/// Reads an ISO 8601 duration such as `PT30M` or `P1D`.
fn duration(text: &str) -> Option<Duration> {
    let text = text.trim().strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut number = String::new();
    let mut time = false;
    for c in text.chars() {
        match c {
            'T' => time = true,
            d if d.is_ascii_digit() => number.push(d),
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += match (unit, time) {
                    ('W', _) => Duration::weeks(n),
                    ('D', _) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(total)
}

fn stamp(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

fn person_line(kind: &str, person: &Person, extra: &str) -> String {
    let name = person
        .name
        .as_deref()
        .map(|n| format!(";CN=\"{}\"", n.replace('"', "'")))
        .unwrap_or_default();
    format!("{kind}{name}{extra}:mailto:{}", person.email)
}

/// Folds long lines at 75 octets (§3.1) and joins with CRLF.
fn fold(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        let mut current = String::new();
        for c in line.chars() {
            if current.len() + c.len_utf8() > 75 {
                out.push_str(&current);
                out.push_str("\r\n ");
                current.clear();
            }
            current.push(c);
        }
        out.push_str(&current);
        out.push_str("\r\n");
    }
    out
}

/// An iMIP REPLY answering `invite` as `me`.
pub fn reply(invite: &Invite, me: &Person, rsvp: Rsvp, now: DateTime<Utc>) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "PRODID:-//Sift Mail//EN".to_owned(),
        "VERSION:2.0".to_owned(),
        "METHOD:REPLY".to_owned(),
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}", invite.uid),
        format!("SEQUENCE:{}", invite.sequence),
        format!("DTSTAMP:{}", stamp(now)),
        format!("DTSTART:{}", stamp(invite.start)),
        format!("DTEND:{}", stamp(invite.end)),
        format!("SUMMARY:{}", escape(&invite.summary)),
    ];
    if let Some(organizer) = &invite.organizer {
        lines.push(person_line("ORGANIZER", organizer, ""));
    }
    lines.push(person_line(
        "ATTENDEE",
        me,
        &format!(";PARTSTAT={}", rsvp.partstat()),
    ));
    lines.push("END:VEVENT".to_owned());
    lines.push("END:VCALENDAR".to_owned());
    fold(&lines)
}

/// An iMIP REQUEST inviting `attendees` to a new meeting organised by `me`.
pub fn request(invite: &Invite, now: DateTime<Utc>) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "PRODID:-//Sift Mail//EN".to_owned(),
        "VERSION:2.0".to_owned(),
        "METHOD:REQUEST".to_owned(),
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}", invite.uid),
        format!("SEQUENCE:{}", invite.sequence),
        format!("DTSTAMP:{}", stamp(now)),
        format!("DTSTART:{}", stamp(invite.start)),
        format!("DTEND:{}", stamp(invite.end)),
        format!("SUMMARY:{}", escape(&invite.summary)),
    ];
    if let Some(location) = &invite.location {
        lines.push(format!("LOCATION:{}", escape(location)));
    }
    if let Some(organizer) = &invite.organizer {
        lines.push(person_line("ORGANIZER", organizer, ""));
    }
    for attendee in &invite.attendees {
        lines.push(person_line(
            "ATTENDEE",
            attendee,
            ";ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE",
        ));
    }
    lines.push("END:VEVENT".to_owned());
    lines.push("END:VCALENDAR".to_owned());
    fold(&lines)
}

#[cfg(test)]
mod tests;
