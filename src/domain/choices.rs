//! The choices made during onboarding, as typed values with a stable stored
//! form: how the inbox is sorted, which views show as tabs, which AI helpers
//! are on, and when notifications may interrupt.
//!
//! Stored strings that don't parse fall back to the defaults, so an old or
//! hand-edited preference never stops the app from starting.

use super::view::TriageView;

/// How the inbox is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    /// Tabs for each view (the default).
    #[default]
    Views,
    /// Everything in one list by date.
    Plain,
}

impl SortMode {
    /// Stored form.
    pub fn key(self) -> &'static str {
        match self {
            Self::Views => "views",
            Self::Plain => "plain",
        }
    }

    /// Parses [`SortMode::key`], defaulting to views.
    pub fn from_key(key: &str) -> Self {
        if key == "plain" {
            Self::Plain
        } else {
            Self::Views
        }
    }
}

/// Views that can be turned on or off as tabs. Needs you is always on.
pub const OPTIONAL_VIEWS: [TriageView; 4] = [
    TriageView::Meetings,
    TriageView::Updates,
    TriageView::Newsletters,
    TriageView::Other,
];

/// Which views show as tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewSet {
    meetings: bool,
    updates: bool,
    newsletters: bool,
    other: bool,
}

impl Default for ViewSet {
    /// Everything but Other, as the Welcome board suggests.
    fn default() -> Self {
        Self {
            meetings: true,
            updates: true,
            newsletters: true,
            other: false,
        }
    }
}

impl ViewSet {
    /// Whether `view` shows as a tab. All and Needs you always do.
    pub fn shows(self, view: TriageView) -> bool {
        match view {
            TriageView::All | TriageView::NeedsYou => true,
            TriageView::Meetings => self.meetings,
            TriageView::Updates => self.updates,
            TriageView::Newsletters => self.newsletters,
            TriageView::Other => self.other,
        }
    }

    /// Turns an optional view on or off. All and Needs you can't be hidden.
    pub fn toggle(&mut self, view: TriageView) {
        match view {
            TriageView::All | TriageView::NeedsYou => {}
            TriageView::Meetings => self.meetings = !self.meetings,
            TriageView::Updates => self.updates = !self.updates,
            TriageView::Newsletters => self.newsletters = !self.newsletters,
            TriageView::Other => self.other = !self.other,
        }
    }

    /// Stored form: the view keys that are on.
    pub fn key(self) -> String {
        OPTIONAL_VIEWS
            .into_iter()
            .filter(|v| self.shows(*v))
            .map(TriageView::key)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parses [`ViewSet::key`]; unknown keys are ignored.
    pub fn from_key(key: &str) -> Self {
        let on: Vec<_> = key.split(',').filter_map(TriageView::from_key).collect();
        Self {
            meetings: on.contains(&TriageView::Meetings),
            updates: on.contains(&TriageView::Updates),
            newsletters: on.contains(&TriageView::Newsletters),
            other: on.contains(&TriageView::Other),
        }
    }
}

/// An AI helper that can be switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiFeature {
    /// One-line summaries of long threads.
    Summaries,
    /// Reply drafts and tone rewrites.
    Drafts,
    /// Ask your inbox from the palette.
    Ask,
    /// To-dos and dates found in mail.
    Tasks,
}

impl AiFeature {
    /// Every helper, in the order onboarding lists them.
    pub const ALL: [AiFeature; 4] = [Self::Summaries, Self::Drafts, Self::Ask, Self::Tasks];

    /// Stored form.
    pub fn key(self) -> &'static str {
        match self {
            Self::Summaries => "summaries",
            Self::Drafts => "drafts",
            Self::Ask => "ask",
            Self::Tasks => "tasks",
        }
    }

    /// Title and one-line explanation.
    pub fn copy(self) -> (&'static str, &'static str) {
        match self {
            Self::Summaries => (
                "One-line summaries",
                "The gist of long threads, so you can skip the scroll.",
            ),
            Self::Drafts => (
                "Reply drafts",
                "Two or three replies ready to go. Shorter, friendlier or more formal.",
            ),
            Self::Ask => (
                "Ask your inbox",
                "\u{201c}What does Tom need from me?\u{201d} Answered, with the source.",
            ),
            Self::Tasks => (
                "To-dos and dates",
                "Spots the asks and deadlines hiding in long emails.",
            ),
        }
    }
}

/// Which AI helpers are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiFeatures([bool; 4]);

impl Default for AiFeatures {
    fn default() -> Self {
        Self([true; 4])
    }
}

impl AiFeatures {
    fn index(feature: AiFeature) -> usize {
        AiFeature::ALL
            .iter()
            .position(|f| *f == feature)
            .unwrap_or_default()
    }

    /// Whether a helper is on.
    pub fn on(self, feature: AiFeature) -> bool {
        self.0[Self::index(feature)]
    }

    /// Flips a helper.
    pub fn toggle(&mut self, feature: AiFeature) {
        let i = Self::index(feature);
        self.0[i] = !self.0[i];
    }

    /// How many helpers are on.
    pub fn count(self) -> usize {
        self.0.iter().filter(|on| **on).count()
    }

    /// Stored form.
    pub fn key(self) -> String {
        AiFeature::ALL
            .into_iter()
            .filter(|f| self.on(*f))
            .map(AiFeature::key)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parses [`AiFeatures::key`].
    pub fn from_key(key: &str) -> Self {
        let keys: Vec<&str> = key.split(',').collect();
        Self(AiFeature::ALL.map(|f| keys.contains(&f.key())))
    }
}

/// When notifications may interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotifyMode {
    /// Needs you mail pings; everything else waits for check-ins (the default).
    #[default]
    Hold,
    /// Every message pings.
    All,
    /// Nothing pings.
    Never,
}

impl NotifyMode {
    /// Every mode, in the order onboarding lists them.
    pub const ALL: [NotifyMode; 3] = [Self::Hold, Self::All, Self::Never];

    /// Stored form.
    pub fn key(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::All => "all",
            Self::Never => "none",
        }
    }

    /// Parses [`NotifyMode::key`], defaulting to hold.
    pub fn from_key(key: &str) -> Self {
        match key {
            "all" => Self::All,
            "none" => Self::Never,
            _ => Self::Hold,
        }
    }

    /// Title and explanation.
    pub fn copy(self) -> (&'static str, &'static str) {
        match self {
            Self::Hold => (
                "Batch it, let urgent through",
                "Recommended. Needs-you mail pings you; everything else waits for your check-ins.",
            ),
            Self::All => (
                "Ping me for everything",
                "The classic, slightly frantic way. Focus time still holds them.",
            ),
            Self::Never => (
                "Never interrupt me",
                "You open Sift when you choose. Meeting reminders still work.",
            ),
        }
    }
}

/// Times of day held notifications are delivered.
pub const CHECKIN_TIMES: [&str; 4] = ["09:00", "12:00", "15:00", "17:00"];

/// Which check-in times are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkins([bool; 4]);

impl Default for Checkins {
    /// Midday and end of day.
    fn default() -> Self {
        Self([false, true, false, true])
    }
}

impl Checkins {
    /// Whether the check-in at `CHECKIN_TIMES[index]` is on.
    pub fn on(self, index: usize) -> bool {
        self.0.get(index).copied().unwrap_or(false)
    }

    /// Flips a check-in time.
    pub fn toggle(&mut self, index: usize) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = !*slot;
        }
    }

    /// The times that are on, in order.
    pub fn times(self) -> Vec<&'static str> {
        CHECKIN_TIMES
            .into_iter()
            .enumerate()
            .filter(|(i, _)| self.on(*i))
            .map(|(_, t)| t)
            .collect()
    }

    /// Stored form.
    pub fn key(self) -> String {
        self.times().join(",")
    }

    /// Parses [`Checkins::key`].
    pub fn from_key(key: &str) -> Self {
        let times: Vec<&str> = key.split(',').collect();
        Self(CHECKIN_TIMES.map(|t| times.contains(&t)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_and_notify_round_trip_with_defaults() {
        for m in [SortMode::Views, SortMode::Plain] {
            assert_eq!(SortMode::from_key(m.key()), m);
        }
        assert_eq!(SortMode::from_key("?"), SortMode::Views);
        for m in NotifyMode::ALL {
            assert_eq!(NotifyMode::from_key(m.key()), m);
            assert!(!m.copy().0.is_empty());
        }
        assert_eq!(NotifyMode::from_key("?"), NotifyMode::Hold);
    }

    #[test]
    fn views_toggle_but_needs_you_stays() {
        let mut v = ViewSet::default();
        assert_eq!(v.key(), "meetings,updates,newsletters");
        assert!(v.shows(TriageView::All) && v.shows(TriageView::NeedsYou));
        assert!(!v.shows(TriageView::Other));
        for view in TriageView::ALL {
            v.toggle(view);
        }
        assert!(v.shows(TriageView::NeedsYou), "can't be hidden");
        assert_eq!(v.key(), "other");
        assert_eq!(ViewSet::from_key(&v.key()), v);
        assert_eq!(ViewSet::from_key("nonsense,other"), v);
    }

    #[test]
    fn ai_features_and_checkins_round_trip() {
        let mut ai = AiFeatures::default();
        assert_eq!(ai.count(), 4);
        ai.toggle(AiFeature::Drafts);
        assert!(!ai.on(AiFeature::Drafts));
        assert_eq!(ai.key(), "summaries,ask,tasks");
        assert_eq!(AiFeatures::from_key(&ai.key()), ai);
        assert_eq!(AiFeatures::from_key("").count(), 0);
        for f in AiFeature::ALL {
            assert!(!f.copy().1.is_empty());
        }

        let mut c = Checkins::default();
        assert_eq!(c.times(), ["12:00", "17:00"]);
        c.toggle(0);
        c.toggle(9);
        assert_eq!(c.key(), "09:00,12:00,17:00");
        assert_eq!(Checkins::from_key(&c.key()), c);
        assert!(!c.on(9));
    }
}
