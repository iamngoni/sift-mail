//! Other people's busy times, for Find a time.
//!
//! IMAP can't see anyone's calendar, so for accounts signed in through the
//! browser Sift asks the provider: Microsoft Graph `getSchedule` for
//! Microsoft 365 and the Google Calendar `freeBusy` API for Google. Both only
//! return busy blocks, never titles. Requests and answers are built and read
//! here as plain data; the HTTP call sits behind [`CalendarHttp`].

use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use super::imap::SyncError;
use crate::domain::{EmailAddress, Provider};

/// POSTs JSON with a bearer token: URL, token, body. Returns status and body.
pub type CalendarHttp =
    Arc<dyn Fn(&str, &str, &str) -> Result<(u16, String), SyncError> + Send + Sync>;

/// Microsoft Graph's schedule endpoint.
pub const GRAPH_SCHEDULE: &str = "https://graph.microsoft.com/v1.0/me/calendar/getSchedule";
/// Google Calendar's free/busy endpoint.
pub const GOOGLE_FREEBUSY: &str = "https://www.googleapis.com/calendar/v3/freeBusy";
/// The Graph permission Find a time needs, added to the Microsoft sign-in.
pub const GRAPH_SCOPE: &str = "https://graph.microsoft.com/Calendars.Read.Shared";
/// The Google permission Find a time needs, added to the Google sign-in.
pub const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/calendar.freebusy";

/// One person's busy blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonBusy {
    /// Who.
    pub email: EmailAddress,
    /// Busy from, to (UTC).
    pub busy: Vec<(DateTime<Utc>, DateTime<Utc>)>,
}

/// The request URL and body for `provider`.
pub fn request(
    provider: Provider,
    people: &[EmailAddress],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<(&'static str, String), SyncError> {
    let stamp = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%S").to_string();
    match provider {
        Provider::Microsoft365 => Ok((
            GRAPH_SCHEDULE,
            json!({
                "schedules": people.iter().map(EmailAddress::as_str).collect::<Vec<_>>(),
                "startTime": { "dateTime": stamp(from), "timeZone": "UTC" },
                "endTime": { "dateTime": stamp(to), "timeZone": "UTC" },
                "availabilityViewInterval": 30
            })
            .to_string(),
        )),
        Provider::Google => Ok((
            GOOGLE_FREEBUSY,
            json!({
                "timeMin": from.to_rfc3339(),
                "timeMax": to.to_rfc3339(),
                "items": people.iter().map(|p| json!({ "id": p.as_str() })).collect::<Vec<_>>()
            })
            .to_string(),
        )),
        Provider::ICloud | Provider::Imap => Err(SyncError::Protocol(
            "this account can't see other people's calendars".into(),
        )),
    }
}

#[derive(Deserialize)]
struct GraphTime {
    #[serde(rename = "dateTime")]
    date_time: String,
}

#[derive(Deserialize)]
struct GraphItem {
    status: String,
    start: GraphTime,
    end: GraphTime,
}

#[derive(Deserialize)]
struct GraphSchedule {
    #[serde(rename = "scheduleId")]
    schedule_id: String,
    #[serde(rename = "scheduleItems", default)]
    items: Vec<GraphItem>,
}

#[derive(Deserialize)]
struct GraphAnswer {
    value: Vec<GraphSchedule>,
}

#[derive(Deserialize)]
struct GoogleBlock {
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct GoogleCalendar {
    #[serde(default)]
    busy: Vec<GoogleBlock>,
}

#[derive(Deserialize)]
struct GoogleAnswer {
    calendars: std::collections::BTreeMap<String, GoogleCalendar>,
}

fn graph_time(text: &str) -> Option<DateTime<Utc>> {
    // Graph writes "2026-10-19T09:00:00.0000000" in the requested zone (UTC).
    let trimmed = text.split('.').next().unwrap_or(text);
    NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|t| t.and_utc())
}

/// Reads a provider's answer. Free and "working elsewhere" time is not busy.
pub fn parse(provider: Provider, status: u16, body: &str) -> Result<Vec<PersonBusy>, SyncError> {
    match status {
        200..=299 => {}
        401 | 403 => {
            return Err(SyncError::Protocol(
                "Sift isn't allowed to see calendars yet; sign in again to grant it".into(),
            ));
        }
        other => {
            return Err(SyncError::Protocol(format!(
                "the calendar service answered {other}"
            )));
        }
    }
    let unreadable = |_| SyncError::Protocol("the calendar answer couldn't be read".into());
    match provider {
        Provider::Microsoft365 => {
            let answer: GraphAnswer = serde_json::from_str(body).map_err(unreadable)?;
            Ok(answer
                .value
                .into_iter()
                .filter_map(|s| {
                    Some(PersonBusy {
                        email: EmailAddress::parse(&s.schedule_id).ok()?,
                        busy: s
                            .items
                            .into_iter()
                            .filter(|i| !matches!(i.status.as_str(), "free" | "workingElsewhere"))
                            .filter_map(|i| {
                                Some((
                                    graph_time(&i.start.date_time)?,
                                    graph_time(&i.end.date_time)?,
                                ))
                            })
                            .collect(),
                    })
                })
                .collect())
        }
        _ => {
            let answer: GoogleAnswer = serde_json::from_str(body).map_err(unreadable)?;
            Ok(answer
                .calendars
                .into_iter()
                .filter_map(|(id, cal)| {
                    Some(PersonBusy {
                        email: EmailAddress::parse(&id).ok()?,
                        busy: cal
                            .busy
                            .into_iter()
                            .filter_map(|b| {
                                Some((
                                    DateTime::parse_from_rfc3339(&b.start)
                                        .ok()?
                                        .with_timezone(&Utc),
                                    DateTime::parse_from_rfc3339(&b.end)
                                        .ok()?
                                        .with_timezone(&Utc),
                                ))
                            })
                            .collect(),
                    })
                })
                .collect())
        }
    }
}

/// The real HTTP call: rustls, timeouts, no redirects, times in UTC.
pub fn networked_http() -> CalendarHttp {
    Arc::new(|url: &str, token: &str, body: &str| {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("SiftMail/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| SyncError::Protocol(e.to_string()))?;
        let response = client
            .post(url)
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .header("Prefer", "outlook.timezone=\"UTC\"")
            .body(body.to_owned())
            .send()
            .map_err(|e| SyncError::Connect {
                host: url.to_owned(),
                detail: e.to_string(),
            })?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|e| SyncError::Protocol(e.to_string()))?;
        Ok((status, text))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn priya() -> EmailAddress {
        EmailAddress::parse("priya@company.com").unwrap()
    }

    #[test]
    fn requests_name_the_people_and_the_window() {
        let (from, to) = (utc("2026-10-19T00:00:00Z"), utc("2026-10-24T00:00:00Z"));
        let (url, body) = request(Provider::Microsoft365, &[priya()], from, to).unwrap();
        assert_eq!(url, GRAPH_SCHEDULE);
        assert!(body.contains("\"schedules\":[\"priya@company.com\"]"));
        assert!(body.contains("2026-10-19T00:00:00"));
        let (url, body) = request(Provider::Google, &[priya()], from, to).unwrap();
        assert_eq!(url, GOOGLE_FREEBUSY);
        assert!(body.contains("\"id\":\"priya@company.com\""));
        assert!(request(Provider::Imap, &[priya()], from, to).is_err());
    }

    #[test]
    fn graph_and_google_answers_are_read() {
        let graph = r#"{"value":[{"scheduleId":"priya@company.com","scheduleItems":[
            {"status":"busy","start":{"dateTime":"2026-10-19T09:00:00.0000000","timeZone":"UTC"},"end":{"dateTime":"2026-10-19T10:00:00.0000000","timeZone":"UTC"}},
            {"status":"free","start":{"dateTime":"2026-10-19T11:00:00.0000000"},"end":{"dateTime":"2026-10-19T12:00:00.0000000"}},
            {"status":"tentative","start":{"dateTime":"bad"},"end":{"dateTime":"2026-10-19T12:00:00"}}]},
            {"scheduleId":"not an address"}]}"#;
        let people = parse(Provider::Microsoft365, 200, graph).unwrap();
        assert_eq!(
            people,
            [PersonBusy {
                email: priya(),
                busy: vec![(utc("2026-10-19T09:00:00Z"), utc("2026-10-19T10:00:00Z"))]
            }]
        );
        let google = r#"{"calendars":{"priya@company.com":{"busy":[
            {"start":"2026-10-19T09:00:00Z","end":"2026-10-19T09:30:00Z"},{"start":"x","end":"y"}]},
            "nobody":{"busy":[]}}}"#;
        let people = parse(Provider::Google, 200, google).unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].busy.len(), 1);
    }

    #[test]
    fn errors_say_what_to_do() {
        assert!(
            parse(Provider::Google, 403, "")
                .unwrap_err()
                .to_string()
                .contains("sign in again")
        );
        assert!(
            parse(Provider::Google, 500, "")
                .unwrap_err()
                .to_string()
                .contains("500")
        );
        assert!(
            parse(Provider::Google, 200, "nope")
                .unwrap_err()
                .to_string()
                .contains("couldn't be read")
        );
        assert!(parse(Provider::Microsoft365, 200, "{}").is_err());
    }
}
