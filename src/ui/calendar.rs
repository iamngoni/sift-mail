//! The Calendar screen and answering invitations, from the Calendar board
//! and the meeting card on the Main board.
//!
//! Answers go back to the organizer as iMIP replies over the account's own
//! SMTP, so Outlook and Google update their side; invites Sift sends are iMIP
//! requests. Find a time searches your own calendar: Sift can't see other
//! people's calendars over IMAP.
//!
//! Rendering lives in [`super::calendar_view`].

use chrono::{
    DateTime, Datelike as _, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone as _,
    Timelike as _, Utc,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, Entity, Focusable as _, Window};

use crate::domain::ical::{self, Person, Rsvp};
use crate::domain::schedule::{self, Busy, DAY_END, DAY_START};
use crate::domain::{Draft, Recipients};
use crate::store::{Event, EventId, EventKind, NewEvent};
use crate::sync::send::SendRequest;

use super::app::{SiftApp, Toast};

/// A slot offered by Find a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// Start, local.
    pub start: NaiveDateTime,
    /// End, local.
    pub end: NaiveDateTime,
    /// Why it's a good time.
    pub note: String,
}

/// An event being added by clicking an empty slot.
pub(crate) struct QuickCreate {
    pub(crate) day: NaiveDate,
    /// Minutes after midnight.
    pub(crate) start: u32,
    /// Length in minutes.
    pub(crate) minutes: u32,
    pub(crate) title: Entity<InputState>,
}

/// The open Calendar screen.
pub(crate) struct CalendarState {
    /// Weeks from this one.
    pub(crate) week: i64,
    /// The event whose details are showing.
    pub(crate) detail: Option<EventId>,
    pub(crate) draft: Option<QuickCreate>,
    pub(crate) ask: Entity<InputState>,
    /// What Find a time understood, and the times it found.
    pub(crate) offers: Option<(String, Vec<Offer>, Vec<Person>)>,
    /// The hold made from an offer, until its invite is sent.
    pub(crate) hold: Option<EventId>,
    /// The last Find a time request.
    pub(crate) search: Option<Search>,
}

/// What Find a time knows about the other people's calendars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Others {
    /// Nobody else, or no way to ask.
    Unknown,
    /// Asking their provider.
    Checking,
    /// Their busy blocks, in local time.
    Busy(Vec<Busy>),
    /// The provider couldn't say; why.
    Failed(String),
}

/// A Find a time request, kept so offers can be redone when calendars arrive.
#[derive(Debug, Clone)]
pub(crate) struct Search {
    pub(crate) days: Vec<NaiveDate>,
    pub(crate) minutes: i64,
    pub(crate) people: Vec<Person>,
    pub(crate) label: String,
    pub(crate) others: Others,
}

/// Local wall-clock time of a UTC instant.
pub fn local(at: DateTime<Utc>) -> NaiveDateTime {
    at.with_timezone(&Local).naive_local()
}

/// UTC instant of a local wall-clock time.
pub fn utc(at: NaiveDateTime) -> DateTime<Utc> {
    Local
        .from_local_datetime(&at)
        .earliest()
        .map_or_else(|| Utc.from_utc_datetime(&at), |t| t.with_timezone(&Utc))
}

/// "Tue 20 Oct · 14:00–14:30".
pub fn when_label(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    let (s, e) = (local(start), local(end));
    format!(
        "{} · {}–{}",
        s.format("%a %-d %b"),
        s.format("%H:%M"),
        e.format("%H:%M")
    )
}

impl SiftApp {
    /// Whether the Calendar screen is showing.
    pub fn calendar_open(&self) -> bool {
        self.calendar.is_some()
    }

    /// Opens the Calendar on this week.
    pub fn open_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ask =
            cx.new(|cx| InputState::new(window, cx).default_value("30 min with Priya next week"));
        cx.subscribe_in(&ask, window, |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.find_a_time(cx);
            }
        })
        .detach();
        self.overlay = None;
        self.settings = None;
        self.open = None;
        self.calendar = Some(CalendarState {
            week: 0,
            detail: None,
            draft: None,
            ask,
            offers: None,
            hold: None,
            search: None,
        });
        self.focus.focus(window, cx);
        self.find_a_time(cx);
        cx.notify();
    }

    /// Back to mail.
    pub fn close_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.calendar = None;
        self.reload();
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Moves a week back or forward; `None` goes to this week.
    pub fn calendar_week(&mut self, delta: Option<i64>, cx: &mut Context<Self>) {
        if let Some(c) = &mut self.calendar {
            c.week = delta.map_or(0, |d| (c.week + d).clamp(-52, 52));
            c.detail = None;
            c.draft = None;
            cx.notify();
        }
    }

    /// The working days on screen.
    pub fn calendar_days(&self) -> [NaiveDate; 5] {
        let week = self.calendar.as_ref().map_or(0, |c| c.week);
        schedule::work_week(Local::now().date_naive(), week)
    }

    /// Events on screen.
    pub(crate) fn week_events(&self) -> Vec<Event> {
        let days = self.calendar_days();
        let from = utc(days[0].and_time(NaiveTime::MIN));
        let to = utc((days[4] + chrono::Duration::days(1)).and_time(NaiveTime::MIN));
        self.store.events_between(from, to).unwrap_or_default()
    }

    /// Shows or hides an event's details.
    pub fn show_event(&mut self, id: Option<EventId>, cx: &mut Context<Self>) {
        if let Some(c) = &mut self.calendar {
            c.detail = id;
            c.draft = None;
            cx.notify();
        }
    }

    /// Starts a new event at a slot.
    pub fn start_event(
        &mut self,
        day: NaiveDate,
        start: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Add a title"));
        cx.subscribe_in(&title, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.save_event(window, cx);
            }
        })
        .detach();
        title.read(cx).focus_handle(cx).focus(window, cx);
        if let Some(c) = &mut self.calendar {
            c.detail = None;
            c.draft = Some(QuickCreate {
                day,
                start,
                minutes: 30,
                title,
            });
        }
        cx.notify();
    }

    /// New event at the next free half hour (N).
    pub fn new_event_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let days = self.calendar_days();
        let busy = self.busy();
        let now = Local::now().naive_local();
        let slot = schedule::free_slots(&days, &busy, 30, now, 1)
            .into_iter()
            .next();
        let (day, start) = slot.map_or((days[0], DAY_START * 60), |s| {
            (
                s.start.date(),
                s.start.time().num_seconds_from_midnight() / 60,
            )
        });
        self.start_event(day, start, window, cx);
    }

    /// Sets the new event's length.
    pub fn draft_length(&mut self, minutes: u32, cx: &mut Context<Self>) {
        if let Some(draft) = self.calendar.as_mut().and_then(|c| c.draft.as_mut()) {
            draft.minutes = minutes;
            cx.notify();
        }
    }

    /// Drops the new event.
    pub fn cancel_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = &mut self.calendar {
            c.draft = None;
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn organiser(&self) -> Option<(crate::domain::AccountId, Person)> {
        let id = self.default_from()?;
        let account = self.accounts.iter().find(|a| a.id == id)?;
        Some((
            id,
            Person {
                name: Some(account.name.clone()),
                email: account.email.clone(),
            },
        ))
    }

    /// Saves the new event.
    pub fn save_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.calendar.as_mut().and_then(|c| c.draft.take()) else {
            return;
        };
        let Some((account, me)) = self.organiser() else {
            self.error = Some("Add an account before adding events.".into());
            cx.notify();
            return;
        };
        let title = draft.title.read(cx).value().trim().to_owned();
        let start =
            draft.day.and_time(NaiveTime::MIN) + chrono::Duration::minutes(i64::from(draft.start));
        let end = (start + chrono::Duration::minutes(i64::from(draft.minutes))).min(
            draft
                .day
                .and_time(NaiveTime::from_hms_opt(DAY_END, 0, 0).unwrap_or(NaiveTime::MIN)),
        );
        let event = NewEvent {
            account,
            title: if title.is_empty() {
                "New event".into()
            } else {
                title
            },
            start: utc(start),
            end: utc(end),
            kind: EventKind::Mine,
            attendees: Vec::new(),
            uid: format!("{:x}@sift", crate::sync::service::rand_u64()),
        };
        match self.store.add_event(&event, Some(&me)) {
            Ok(saved) => self.say(format!("Added {}", saved.title)),
            Err(error) => self.error = Some(format!("Sift couldn't add that event: {error}")),
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn say(&mut self, text: String) {
        self.toast = Some(Toast { text, undo: None });
    }

    /// Deletes one of your events, or releases a hold.
    pub fn delete_event(&mut self, id: EventId, cx: &mut Context<Self>) {
        let title = self.store.event(id).ok().flatten().map(|e| e.title);
        if let Err(error) = self.store.delete_event(id) {
            self.error = Some(format!("Sift couldn't delete that event: {error}"));
        }
        if let Some(c) = &mut self.calendar {
            c.detail = None;
            if c.hold == Some(id) {
                c.hold = None;
            }
        }
        if let Some(title) = title {
            self.say(format!("Removed {title}"));
        }
        cx.notify();
    }

    /// Answers an invitation and tells the organizer.
    pub fn rsvp(
        &mut self,
        id: EventId,
        answer: Option<Rsvp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(event) = self.store.event(id).ok().flatten() else {
            return;
        };
        if let Err(error) = self.store.set_rsvp(id, answer) {
            self.error = Some(format!("Sift couldn't save that answer: {error}"));
            cx.notify();
            return;
        }
        if let Some(c) = &mut self.calendar {
            c.detail = None;
        }
        let Some(answer) = answer else {
            cx.notify();
            return;
        };
        self.say(format!("{} {}", answer.subject_prefix(), event.title));
        let organizer = event.organizer.clone();
        let account = self
            .accounts
            .iter()
            .find(|a| a.id == event.account)
            .cloned();
        if let (Some(organizer), Some(account), Some(service)) =
            (organizer, account, self.sync.clone())
        {
            let me = Person {
                name: Some(account.name.clone()),
                email: account.email.clone(),
            };
            let ics = ical::reply(&event.invite(), &me, answer, Utc::now());
            let body = format!("{} {}.", answer.subject_prefix(), event.title);
            let draft = Draft::new(
                Recipients::one(organizer.email.clone()),
                &format!("{}: {}", answer.subject_prefix(), event.title),
                &body,
            )
            .map(|d| d.with_calendar("REPLY", ics));
            if let Ok(draft) = draft {
                let request = SendRequest {
                    account: account.id,
                    draft,
                    thread: event.thread.clone(),
                };
                let task = cx
                    .background_executor()
                    .spawn(async move { service.send(request) });
                cx.spawn_in(window, async move |this, cx| {
                    let result = task.await;
                    this.update_in(cx, |app, _, cx| {
                        app.reload();
                        if let Err(error) = result {
                            app.error = Some(format!(
                                "Your answer is saved here, but {} wasn't told: {error}",
                                organizer.display()
                            ));
                        }
                        cx.notify();
                    })
                })
                .detach();
            }
        }
        self.reload();
        cx.notify();
    }

    /// Sends the invite for a hold, turning it into a meeting.
    pub fn send_invite(&mut self, id: EventId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.store.event(id).ok().flatten() else {
            return;
        };
        let Some(service) = self.sync.clone() else {
            self.error = Some("Sift can't send invites in this session.".into());
            cx.notify();
            return;
        };
        let recipients: Vec<_> = event
            .attendees
            .iter()
            .map(|p| p.email.to_string())
            .collect();
        let draft = Recipients::parse(&recipients.join(", ")).and_then(|to| {
            Draft::new(
                to,
                &event.title,
                &format!("{}\n\n{}", event.title, when_label(event.start, event.end)),
            )
        });
        let draft = match draft {
            Ok(d) => d.with_calendar("REQUEST", ical::request(&event.invite(), Utc::now())),
            Err(error) => {
                self.error = Some(format!("Sift couldn't send that invite: {error}"));
                cx.notify();
                return;
            }
        };
        let request = SendRequest {
            account: event.account,
            draft,
            thread: None,
        };
        let task = cx
            .background_executor()
            .spawn(async move { service.send(request) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, _, cx| {
                match result {
                    Ok(_) => {
                        let _ = app.store.set_event_kind(id, EventKind::Meeting);
                        if let Some(c) = &mut app.calendar {
                            c.hold = None;
                            c.detail = None;
                        }
                        app.say(format!("Invite sent for {}", event.title));
                    }
                    Err(error) => {
                        app.error = Some(format!("Sift couldn't send that invite: {error}"))
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Your busy time this week and next, in local time.
    fn busy(&self) -> Vec<Busy> {
        let today = Local::now().date_naive();
        let from = utc(schedule::monday_of(today).and_time(NaiveTime::MIN));
        let to = from + chrono::Duration::weeks(3);
        self.store
            .events_between(from, to)
            .unwrap_or_default()
            .into_iter()
            .map(|e| Busy {
                start: local(e.start),
                end: local(e.end),
            })
            .collect()
    }

    /// Reads the Find a time box, offers three of your free times at once,
    /// and asks the provider about the other people's calendars.
    pub fn find_a_time(&mut self, cx: &mut Context<Self>) {
        let Some(c) = &self.calendar else {
            return;
        };
        let text = c.ask.read(cx).value().to_string();
        let today = Local::now().date_naive();
        let request = schedule::read_request(&text, today);
        let people: Vec<Person> = request
            .people
            .iter()
            .filter_map(|word| self.store.contacts(word).ok()?.into_iter().next())
            .map(|c| Person {
                name: c.name().map(str::to_owned),
                email: c.email().clone(),
            })
            .fold(Vec::new(), |mut all, p| {
                if !all.iter().any(|q: &Person| q.email == p.email) {
                    all.push(p);
                }
                all
            });
        // At the weekend this week's working days are over, so look ahead.
        let weekend = today.weekday().num_days_from_monday() >= 5;
        let week = if weekend {
            request.week.max(1)
        } else {
            request.week
        };
        let days: Vec<NaiveDate> = match request.day {
            Some(day) => vec![day],
            None => schedule::work_week(today, week).to_vec(),
        };
        let when = match (request.day, week) {
            (Some(day), _) => day.format("%A").to_string(),
            (None, 1) => "Next week".into(),
            _ => "This week".into(),
        };
        let length = if request.minutes % 60 == 0 {
            format!(
                "{} hour{}",
                request.minutes / 60,
                if request.minutes == 60 { "" } else { "s" }
            )
        } else {
            format!("{} minutes", request.minutes)
        };
        let with = if people.is_empty() {
            String::new()
        } else {
            format!(
                ", with {}",
                people
                    .iter()
                    .map(Person::display)
                    .collect::<Vec<_>>()
                    .join(" and ")
            )
        };
        let account = self.organiser().map(|(id, _)| id);
        let check = match (&self.sync, account, people.is_empty()) {
            (Some(service), Some(account), false) => Some((service.clone(), account)),
            _ => None,
        };
        if let Some(c) = &mut self.calendar {
            c.search = Some(Search {
                days: days.clone(),
                minutes: request.minutes,
                people: people.clone(),
                label: format!("{when}, {length}{with}."),
                others: if check.is_some() {
                    Others::Checking
                } else {
                    Others::Unknown
                },
            });
        }
        self.show_offers(cx);
        let Some((service, account)) = check else {
            return;
        };
        let emails: Vec<_> = people.iter().map(|p| p.email.clone()).collect();
        let from = utc(days[0].and_time(NaiveTime::MIN));
        let to = utc((days[days.len() - 1] + chrono::Duration::days(1)).and_time(NaiveTime::MIN));
        let task = cx
            .background_executor()
            .spawn(async move { service.free_busy(account, &emails, from, to) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                if let Some(search) = app.calendar.as_mut().and_then(|c| c.search.as_mut()) {
                    search.others = match result {
                        Ok(people) => Others::Busy(
                            people
                                .into_iter()
                                .flat_map(|p| p.busy)
                                .map(|(s, e)| Busy {
                                    start: local(s),
                                    end: local(e),
                                })
                                .collect(),
                        ),
                        // These reasons are written for people; drop the
                        // "server returned an error" framing around them.
                        Err(crate::sync::imap::SyncError::Protocol(why)) => Others::Failed(why),
                        Err(error) => Others::Failed(error.to_string()),
                    };
                }
                app.show_offers(cx);
            })
        })
        .detach();
    }

    /// Recomputes the three offers from your busy time and, once known, theirs.
    pub(crate) fn show_offers(&mut self, cx: &mut Context<Self>) {
        let Some(search) = self.calendar.as_ref().and_then(|c| c.search.clone()) else {
            return;
        };
        let mine = self.busy();
        let theirs: &[Busy] = match &search.others {
            Others::Busy(b) => b,
            _ => &[],
        };
        let all: Vec<Busy> = mine.iter().chain(theirs).copied().collect();
        let both = matches!(search.others, Others::Busy(_));
        let found = schedule::free_slots(
            &search.days,
            &all,
            search.minutes,
            Local::now().naive_local(),
            3,
        );
        let free = if both {
            if search.people.len() == 1 {
                format!("You and {} are both free", search.people[0].display())
            } else {
                "Everyone is free".to_owned()
            }
        } else {
            "You're free".to_owned()
        };
        let offers = found
            .into_iter()
            .map(|slot| {
                let after = mine.iter().any(|b| b.end == slot.start);
                let before = mine.iter().any(|b| b.start == slot.end);
                Offer {
                    start: slot.start,
                    end: slot.end,
                    note: match (after, before) {
                        (true, _) => format!("{free} · right after your last meeting"),
                        (_, true) => format!("{free} · before your next one"),
                        _ => free.clone(),
                    },
                }
            })
            .collect();
        let names = search
            .people
            .iter()
            .map(Person::display)
            .collect::<Vec<_>>()
            .join(" and ");
        let summary = match &search.others {
            Others::Busy(_) => format!(
                "{} These times avoid both calendars and your focus blocks:",
                search.label
            ),
            Others::Checking => format!(
                "{} Checking {names}'s calendar… for now these avoid yours:",
                search.label
            ),
            Others::Failed(why) => format!(
                "{} {names}'s calendar couldn't be checked ({why}), so these only avoid yours:",
                search.label
            ),
            Others::Unknown => format!(
                "{} These times avoid your calendar and focus blocks:",
                search.label
            ),
        };
        if let Some(c) = &mut self.calendar {
            c.offers = Some((summary, offers, search.people));
        }
        cx.notify();
    }

    /// Holds an offered time on your calendar.
    pub fn hold_offer(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((offer, people)) = self.calendar.as_ref().and_then(|c| {
            let (_, offers, people) = c.offers.as_ref()?;
            Some((offers.get(index)?.clone(), people.clone()))
        }) else {
            return;
        };
        let Some((account, me)) = self.organiser() else {
            self.error = Some("Add an account before adding events.".into());
            cx.notify();
            return;
        };
        if let Some(old) = self.calendar.as_ref().and_then(|c| c.hold) {
            let _ = self.store.delete_event(old);
        }
        let title = match people.first() {
            Some(p) => format!("Meeting with {}", p.display()),
            None => "Meeting".to_owned(),
        };
        let event = NewEvent {
            account,
            title,
            start: utc(offer.start),
            end: utc(offer.end),
            kind: EventKind::Hold,
            attendees: people,
            uid: format!("{:x}@sift", crate::sync::service::rand_u64()),
        };
        match self.store.add_event(&event, Some(&me)) {
            Ok(saved) => {
                let today = Local::now().date_naive();
                if let Some(c) = &mut self.calendar {
                    c.hold = Some(saved.id);
                    c.week = (schedule::monday_of(offer.start.date()) - schedule::monday_of(today))
                        .num_weeks();
                }
            }
            Err(error) => self.error = Some(format!("Sift couldn't hold that time: {error}")),
        }
        cx.notify();
    }
}

impl SiftApp {
    /// Keys while the Calendar shows. Returns whether the key was handled.
    pub(crate) fn calendar_key(
        &mut self,
        event: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(c) = &self.calendar else {
            return false;
        };
        let key = event.keystroke.key.as_str();
        let typing = c.draft.is_some() || c.ask.read(cx).focus_handle(cx).is_focused(window);
        if key == "escape" {
            if c.draft.is_some() {
                self.cancel_event(window, cx);
            } else if c.detail.is_some() {
                self.show_event(None, cx);
            } else if typing {
                self.focus.focus(window, cx);
            } else {
                self.close_calendar(window, cx);
            }
            return true;
        }
        if typing || event.keystroke.modifiers.modified() {
            return false;
        }
        match key {
            "left" => self.calendar_week(Some(-1), cx),
            "right" => self.calendar_week(Some(1), cx),
            "t" => self.calendar_week(None, cx),
            "n" => self.new_event_now(window, cx),
            _ => return false,
        }
        true
    }
}
