//! The calendar: meetings from invitations in mail, and events made in Sift.
//!
//! Invitations are applied by UID and SEQUENCE, so an update replaces the
//! meeting and an older copy never overwrites a newer one; a cancellation
//! removes it. Answering keeps the event, coloured by the answer.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

use super::{MailStore, StoreError, StoreResult};
use crate::domain::ical::{Invite, Method, Person, Rsvp};
use crate::domain::{AccountId, EmailAddress, ThreadRef};

/// Identifies a stored event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(pub(crate) i64);

/// What sort of event, which sets its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A meeting you're going to.
    Meeting,
    /// An invitation you haven't answered.
    Pending,
    /// Something you added for yourself.
    Mine,
    /// A time held for a meeting whose invite isn't sent yet.
    Hold,
    /// Focus time; notifications are held.
    Focus,
    /// Personal.
    Personal,
    /// An invitation you declined; hidden from the week.
    Declined,
}

impl EventKind {
    fn key(self) -> &'static str {
        match self {
            Self::Meeting => "meeting",
            Self::Pending => "pending",
            Self::Mine => "mine",
            Self::Hold => "hold",
            Self::Focus => "focus",
            Self::Personal => "personal",
            Self::Declined => "declined",
        }
    }

    fn from_key(key: &str) -> Result<Self, StoreError> {
        Ok(match key {
            "meeting" => Self::Meeting,
            "pending" => Self::Pending,
            "mine" => Self::Mine,
            "hold" => Self::Hold,
            "focus" => Self::Focus,
            "personal" => Self::Personal,
            "declined" => Self::Declined,
            other => return Err(StoreError::Corrupt(format!("event kind {other:?}"))),
        })
    }

    /// Legend label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Meeting => "Meeting",
            Self::Pending => "Not answered",
            Self::Mine => "Yours",
            Self::Hold => "Held, invite not sent",
            Self::Focus => "Focus time (notifications held)",
            Self::Personal => "Personal",
            Self::Declined => "Declined",
        }
    }
}

/// One calendar event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Store id.
    pub id: EventId,
    /// Account it belongs to.
    pub account: AccountId,
    /// iCalendar UID.
    pub uid: String,
    /// iCalendar SEQUENCE.
    pub sequence: u32,
    /// Title.
    pub title: String,
    /// Start, UTC.
    pub start: DateTime<Utc>,
    /// End, UTC.
    pub end: DateTime<Utc>,
    /// A whole-day event.
    pub all_day: bool,
    /// Room or link.
    pub location: Option<String>,
    /// Who organised it.
    pub organizer: Option<Person>,
    /// Who's invited.
    pub attendees: Vec<Person>,
    /// Colour and behaviour.
    pub kind: EventKind,
    /// Your answer, if you gave one.
    pub rsvp: Option<Rsvp>,
    /// The thread the invitation came in, if any.
    pub thread: Option<ThreadRef>,
}

impl Event {
    /// Rebuilds the invitation, for answering it.
    pub fn invite(&self) -> Invite {
        Invite {
            method: Method::Request,
            uid: self.uid.clone(),
            sequence: self.sequence,
            summary: self.title.clone(),
            start: self.start,
            end: self.end,
            all_day: self.all_day,
            location: self.location.clone(),
            organizer: self.organizer.clone(),
            attendees: self.attendees.clone(),
        }
    }
}

/// A new event made in Sift.
#[derive(Debug, Clone)]
pub struct NewEvent {
    /// Account it goes on.
    pub account: AccountId,
    /// Title.
    pub title: String,
    /// Start.
    pub start: DateTime<Utc>,
    /// End.
    pub end: DateTime<Utc>,
    /// Mine, Hold, Focus or Personal.
    pub kind: EventKind,
    /// Who it's with.
    pub attendees: Vec<Person>,
    /// Unique id; also the iCalendar UID.
    pub uid: String,
}

fn people_to_text(people: &[Person]) -> String {
    people
        .iter()
        .map(|p| format!("{}\t{}", p.name.as_deref().unwrap_or_default(), p.email))
        .collect::<Vec<_>>()
        .join("\n")
}

fn people_from_text(text: &str) -> Vec<Person> {
    text.lines()
        .filter_map(|l| {
            let (name, email) = l.split_once('\t')?;
            Some(Person {
                name: (!name.is_empty()).then(|| name.to_owned()),
                email: EmailAddress::parse(email).ok()?,
            })
        })
        .collect()
}

const COLUMNS: &str = "id, account_id, uid, sequence, title, starts_at, ends_at, all_day, location, \
     organizer, attendees, kind, rsvp, thread_key";

fn event_row(row: &Row<'_>) -> rusqlite::Result<RawEvent> {
    Ok(RawEvent {
        id: row.get(0)?,
        account: row.get(1)?,
        uid: row.get(2)?,
        sequence: row.get(3)?,
        title: row.get(4)?,
        start: row.get(5)?,
        end: row.get(6)?,
        all_day: row.get(7)?,
        location: row.get(8)?,
        organizer: row.get(9)?,
        attendees: row.get(10)?,
        kind: row.get(11)?,
        rsvp: row.get(12)?,
        thread_key: row.get(13)?,
    })
}

struct RawEvent {
    id: i64,
    account: i64,
    uid: String,
    sequence: u32,
    title: String,
    start: String,
    end: String,
    all_day: bool,
    location: Option<String>,
    organizer: Option<String>,
    attendees: String,
    kind: String,
    rsvp: Option<String>,
    thread_key: Option<String>,
}

fn time(text: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| StoreError::Corrupt(format!("event time {text:?}")))
}

impl RawEvent {
    fn parse(self) -> Result<Event, StoreError> {
        let account = AccountId(self.account);
        Ok(Event {
            id: EventId(self.id),
            account,
            uid: self.uid,
            sequence: self.sequence,
            title: self.title,
            start: time(&self.start)?,
            end: time(&self.end)?,
            all_day: self.all_day,
            location: self.location,
            organizer: self
                .organizer
                .as_deref()
                .and_then(|o| people_from_text(o).pop()),
            attendees: people_from_text(&self.attendees),
            kind: EventKind::from_key(&self.kind)?,
            rsvp: self.rsvp.as_deref().and_then(Rsvp::from_key),
            thread: self.thread_key.map(|key| ThreadRef { account, key }),
        })
    }
}

impl MailStore {
    /// Applies an invitation from mail: adds or updates the meeting, or
    /// removes it when cancelled. Older copies and replies change nothing.
    pub fn apply_invite(
        &self,
        account: AccountId,
        invite: &Invite,
        thread: Option<&ThreadRef>,
    ) -> StoreResult<()> {
        let existing: Option<(i64, u32)> = self
            .conn
            .query_row(
                "SELECT id, sequence FROM events WHERE account_id = ?1 AND uid = ?2",
                params![account.0, invite.uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match (invite.method, existing) {
            (Method::Cancel, Some((id, _))) => {
                self.conn
                    .execute("DELETE FROM events WHERE id = ?1", [id])?;
            }
            (Method::Request | Method::Other, None) => {
                self.conn.execute(
                    &format!(
                        "INSERT INTO events ({}) VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12)",
                        COLUMNS
                    ),
                    params![
                        account.0,
                        invite.uid,
                        invite.sequence,
                        invite.summary,
                        invite.start.to_rfc3339(),
                        invite.end.to_rfc3339(),
                        invite.all_day,
                        invite.location,
                        invite.organizer.as_ref().map(|o| people_to_text(std::slice::from_ref(o))),
                        people_to_text(&invite.attendees),
                        if invite.method == Method::Request { "pending" } else { "meeting" },
                        thread.map(|t| t.key.clone()),
                    ],
                )?;
            }
            (Method::Request | Method::Other, Some((id, sequence)))
                if invite.sequence > sequence =>
            {
                // A changed meeting needs a fresh answer.
                let moved: bool = self.conn.query_row(
                    "SELECT starts_at != ?2 OR ends_at != ?3 FROM events WHERE id = ?1",
                    params![id, invite.start.to_rfc3339(), invite.end.to_rfc3339()],
                    |r| r.get(0),
                )?;
                self.conn.execute(
                    "UPDATE events SET sequence = ?2, title = ?3, starts_at = ?4, ends_at = ?5,
                        all_day = ?6, location = ?7, attendees = ?8,
                        kind = CASE WHEN ?9 THEN 'pending' ELSE kind END,
                        rsvp = CASE WHEN ?9 THEN NULL ELSE rsvp END,
                        thread_key = COALESCE(?10, thread_key)
                     WHERE id = ?1",
                    params![
                        id,
                        invite.sequence,
                        invite.summary,
                        invite.start.to_rfc3339(),
                        invite.end.to_rfc3339(),
                        invite.all_day,
                        invite.location,
                        people_to_text(&invite.attendees),
                        moved && invite.method == Method::Request,
                        thread.map(|t| t.key.clone()),
                    ],
                )?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Adds an event made in Sift.
    pub fn add_event(&self, event: &NewEvent, organizer: Option<&Person>) -> StoreResult<Event> {
        self.conn.execute(
            &format!(
                "INSERT INTO events ({}) VALUES (NULL, ?1, ?2, 0, ?3, ?4, ?5, 0, NULL, ?6, ?7, ?8, NULL, NULL)",
                COLUMNS
            ),
            params![
                event.account.0,
                event.uid,
                event.title,
                event.start.to_rfc3339(),
                event.end.to_rfc3339(),
                organizer.map(|o| people_to_text(std::slice::from_ref(o))),
                people_to_text(&event.attendees),
                event.kind.key(),
            ],
        )?;
        self.event(EventId(self.conn.last_insert_rowid()))?
            .ok_or_else(|| StoreError::Corrupt("event vanished after insert".into()))
    }

    /// One event.
    pub fn event(&self, id: EventId) -> StoreResult<Option<Event>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM events WHERE id = ?1"),
                [id.0],
                event_row,
            )
            .optional()?
            .map(RawEvent::parse)
            .transpose()
    }

    /// Events overlapping `[from, to)`, earliest first, declined ones left out.
    pub fn events_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> StoreResult<Vec<Event>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM events
             WHERE starts_at < ?2 AND ends_at > ?1 AND kind != 'declined'
             ORDER BY starts_at, id"
        ))?;
        let rows = stmt.query_map(params![from.to_rfc3339(), to.to_rfc3339()], event_row)?;
        rows.map(|r| r.map_err(StoreError::from).and_then(RawEvent::parse))
            .collect()
    }

    /// Unanswered invitations that haven't happened yet, soonest first.
    pub fn pending_invites(&self, now: DateTime<Utc>) -> StoreResult<Vec<Event>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM events WHERE kind = 'pending' AND ends_at > ?1 ORDER BY starts_at"
        ))?;
        let rows = stmt.query_map([now.to_rfc3339()], event_row)?;
        rows.map(|r| r.map_err(StoreError::from).and_then(RawEvent::parse))
            .collect()
    }

    /// The meeting an invitation thread is about, if any.
    pub fn event_for_thread(&self, thread: &ThreadRef) -> StoreResult<Option<Event>> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM events WHERE account_id = ?1 AND thread_key = ?2
                     ORDER BY sequence DESC LIMIT 1"
                ),
                params![thread.account.0, thread.key],
                event_row,
            )
            .optional()?
            .map(RawEvent::parse)
            .transpose()
    }

    /// Records an answer: accepted and maybe show as meetings, declined hides it.
    pub fn set_rsvp(&self, id: EventId, rsvp: Option<Rsvp>) -> StoreResult<()> {
        let kind = match rsvp {
            None => EventKind::Pending,
            Some(Rsvp::Declined) => EventKind::Declined,
            Some(_) => EventKind::Meeting,
        };
        self.conn.execute(
            "UPDATE events SET rsvp = ?2, kind = ?3 WHERE id = ?1",
            params![id.0, rsvp.map(Rsvp::key), kind.key()],
        )?;
        Ok(())
    }

    /// Changes an event's kind, such as a hold becoming a meeting once invited.
    pub fn set_event_kind(&self, id: EventId, kind: EventKind) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE events SET kind = ?2 WHERE id = ?1",
            params![id.0, kind.key()],
        )?;
        Ok(())
    }

    /// Deletes an event.
    pub fn delete_event(&self, id: EventId) -> StoreResult<()> {
        self.conn
            .execute("DELETE FROM events WHERE id = ?1", [id.0])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
