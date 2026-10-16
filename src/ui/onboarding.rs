//! First-run onboarding from the Welcome board: a welcome, then six short
//! steps (connect, sort, helpers, focus, look, keys) and a summary.
//!
//! Every choice is saved the moment it's made, so skipping a step simply
//! keeps the defaults. Rendering lives in [`super::onboarding_view`].

use gpui_kit::{Context, KeyDownEvent, Window};

use crate::domain::choices::{AiFeature, AiFeatures, Checkins, NotifyMode, SortMode, ViewSet};
use crate::domain::{Provider, TriageView};
use crate::store::Pref;
use crate::theme::Appearance;

use super::app::SiftApp;

/// Where onboarding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardStep {
    /// The hero screen.
    Welcome,
    /// 1 · Your mail.
    Connect,
    /// 2 · The pile.
    Sort,
    /// 3 · Helpers.
    Ai,
    /// 4 · Focus.
    Focus,
    /// 5 · Look.
    Look,
    /// 6 · Moves.
    Keys,
    /// All set.
    Done,
}

impl OnboardStep {
    /// Steps in order.
    pub const ALL: [OnboardStep; 8] = [
        Self::Welcome,
        Self::Connect,
        Self::Sort,
        Self::Ai,
        Self::Focus,
        Self::Look,
        Self::Keys,
        Self::Done,
    ];

    /// Position in [`OnboardStep::ALL`].
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|s| *s == self)
            .unwrap_or_default()
    }

    /// The step `delta` away, clamped to the ends.
    pub fn offset(self, delta: isize) -> Self {
        let last = Self::ALL.len() - 1;
        Self::ALL[self.index().saturating_add_signed(delta).min(last)]
    }

    /// Eyebrow, heading and explanation.
    pub fn copy(self, connected: bool) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Welcome => (
                "Welcome to Sift Mail",
                "Less inbox.",
                "More everything else.",
            ),
            Self::Connect if connected => (
                "1 · Your mail",
                "You're in.",
                "Add more accounts whenever you like. Sift keeps each one tidy and separate.",
            ),
            Self::Connect => (
                "1 · Your mail",
                "Where does your mail live?",
                "You'll sign in on your provider's own page. Sift never sees your password; it gets a key you can take back any time.",
            ),
            Self::Sort => (
                "2 · The pile",
                "How tidy do you like it?",
                "Sift files new mail into views as it lands, and learns from what you mark done, snooze or move.",
            ),
            Self::Ai => (
                "3 · Helpers",
                "Pick your helpers",
                "Anything AI does wears a little sparkle, so you always know. Switch any of them off, no hard feelings.",
            ),
            Self::Focus => (
                "4 · Focus",
                "Guard your attention",
                "Most email can wait an hour. Sift lets the urgent stuff through and saves the rest for later.",
            ),
            Self::Look => (
                "5 · Look",
                "Light, dark or both?",
                "Changes here are live. Go on, try them.",
            ),
            Self::Keys => (
                "6 · Moves",
                "Four keys, whole inbox",
                "Click the practice inbox on the right and have a go. Prefer the mouse? Every key has a button too.",
            ),
            Self::Done => (
                "All set",
                "Your inbox, sifted.",
                "Here's what you chose. Everything can be changed later in Settings.",
            ),
        }
    }
}

/// The keyboard practice tasks: label and key.
pub const PRACTICE_TASKS: [(&str, &str); 4] = [
    ("Move down", "J"),
    ("Move up", "K"),
    ("Mark done (bye, email)", "E"),
    ("Ask or command anything", "Ctrl K"),
];

/// Encouragement after 0 to 4 practice tasks.
pub const PRACTICE_HINTS: [&str; 5] = [
    "Your turn. Click the inbox and press J.",
    "Nice, one down.",
    "Getting the hang of it.",
    "One more…",
    "Look at you. Keyboard wizard.",
];

/// The practice inbox: sender and subject.
pub const PRACTICE_ROWS: [(&str, &str); 5] = [
    ("Priya Raman", "Q4 budget numbers"),
    ("Product Weekly", "Issue 112: pricing pages"),
    ("Daniel Mensah", "Invitation: Roadmap sync"),
    ("IT Service Desk", "Ticket 4821 resolved"),
    ("Kofi Asante", "Offsite planning"),
];

/// Onboarding's own state; the choices themselves live on [`SiftApp`].
pub(crate) struct Onboarding {
    pub(crate) step: OnboardStep,
    /// Practice rows still in the inbox.
    pub(crate) practice_left: Vec<usize>,
    /// Cursor within `practice_left`.
    pub(crate) practice_cursor: usize,
    /// Which practice tasks are done.
    pub(crate) practice_done: [bool; 4],
}

impl Onboarding {
    fn new() -> Self {
        Self {
            step: OnboardStep::Welcome,
            practice_left: (0..PRACTICE_ROWS.len()).collect(),
            practice_cursor: 0,
            practice_done: [false; 4],
        }
    }

    /// How many practice tasks are done.
    pub(crate) fn practice_count(&self) -> usize {
        self.practice_done.iter().filter(|d| **d).count()
    }
}

impl SiftApp {
    /// Shows onboarding on a true first run; an existing install that never
    /// saw it is marked onboarded instead.
    pub(crate) fn maybe_onboard(&mut self) {
        if self.store.flag(Pref::Onboarded, false).unwrap_or(true) {
            return;
        }
        if self.accounts.is_empty() {
            self.onboarding = Some(Onboarding::new());
        } else if let Err(error) = self.store.set_preference(Pref::Onboarded, "true") {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
    }

    /// The onboarding step on screen, if any.
    pub fn onboarding_step(&self) -> Option<OnboardStep> {
        self.onboarding.as_ref().map(|o| o.step)
    }

    /// Goes to an onboarding step.
    pub fn onboard_go(&mut self, step: OnboardStep, cx: &mut Context<Self>) {
        if let Some(o) = &mut self.onboarding {
            o.step = step;
            cx.notify();
        }
    }

    /// Next step, unless Connect is still waiting for an account.
    pub fn onboard_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(step) = self.onboarding_step() else {
            return;
        };
        match step {
            OnboardStep::Connect if self.accounts.is_empty() => {}
            OnboardStep::Done => self.finish_onboarding(window, cx),
            other => self.onboard_go(other.offset(1), cx),
        }
    }

    /// Previous step.
    pub fn onboard_back(&mut self, cx: &mut Context<Self>) {
        if let Some(step) = self.onboarding_step()
            && step.index() > 1
        {
            self.onboard_go(step.offset(-1), cx);
        }
    }

    /// Leaves onboarding for the inbox, for good.
    pub fn finish_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.store.set_preference(Pref::Onboarded, "true") {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
        self.onboarding = None;
        self.reload();
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Opens the Add account flow, already on a provider's sign-in step.
    pub fn onboard_connect(
        &mut self,
        provider: Option<Provider>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_add_account(window, cx);
        if let Some(provider) = provider {
            self.pick_provider(provider, window, cx);
        }
    }

    fn save(&mut self, pref: Pref, value: &str) {
        if let Err(error) = self.store.set_preference(pref, value) {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
    }

    /// Loads the onboarding choices.
    pub(crate) fn load_choices(&mut self) {
        let get = |pref| self.store.preference(pref).ok().flatten();
        self.sort_mode =
            get(Pref::SortMode).map_or_else(SortMode::default, |k| SortMode::from_key(&k));
        self.views = get(Pref::Views).map_or_else(ViewSet::default, |k| ViewSet::from_key(&k));
        self.ai_features =
            get(Pref::AiFeatures).map_or_else(AiFeatures::default, |k| AiFeatures::from_key(&k));
        self.notify_mode =
            get(Pref::NotifyMode).map_or_else(NotifyMode::default, |k| NotifyMode::from_key(&k));
        self.checkins =
            get(Pref::Checkins).map_or_else(Checkins::default, |k| Checkins::from_key(&k));
    }

    /// Picks views or one big list.
    pub fn set_sort_mode(&mut self, mode: SortMode, cx: &mut Context<Self>) {
        self.sort_mode = mode;
        self.save(Pref::SortMode, mode.key());
        if mode == SortMode::Plain {
            self.select(TriageView::All, cx);
        }
        cx.notify();
    }

    /// Shows or hides a view's tab.
    pub fn toggle_view_tab(&mut self, view: TriageView, cx: &mut Context<Self>) {
        self.views.toggle(view);
        let key = self.views.key();
        self.save(Pref::Views, &key);
        if !self.views.shows(self.view) {
            self.select(TriageView::All, cx);
        }
        cx.notify();
    }

    /// Switches an AI helper.
    pub fn toggle_ai_feature(&mut self, feature: AiFeature, cx: &mut Context<Self>) {
        self.ai_features.toggle(feature);
        let key = self.ai_features.key();
        self.save(Pref::AiFeatures, &key);
        cx.notify();
    }

    /// Picks when notifications may interrupt.
    pub fn set_notify_mode(&mut self, mode: NotifyMode, cx: &mut Context<Self>) {
        self.notify_mode = mode;
        self.save(Pref::NotifyMode, mode.key());
        self.hold = mode == NotifyMode::Hold;
        self.save(
            Pref::HoldNotifications,
            if self.hold { "true" } else { "false" },
        );
        cx.notify();
    }

    /// Flips a check-in time.
    pub fn toggle_checkin(&mut self, index: usize, cx: &mut Context<Self>) {
        self.checkins.toggle(index);
        let key = self.checkins.key();
        self.save(Pref::Checkins, &key);
        cx.notify();
    }

    /// Applies a theme from the Look step.
    pub fn onboard_theme(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_appearance(appearance, window, cx);
    }

    /// One practice key: `j`, `k`, `e` or `palette`.
    pub fn practice(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(o) = &mut self.onboarding else {
            return;
        };
        let last = o.practice_left.len().saturating_sub(1);
        match key {
            "j" => {
                o.practice_cursor = (o.practice_cursor + 1).min(last);
                o.practice_done[0] = true;
            }
            "k" => {
                o.practice_cursor = o.practice_cursor.saturating_sub(1);
                o.practice_done[1] = true;
            }
            "e" if !o.practice_left.is_empty() => {
                o.practice_left.remove(o.practice_cursor.min(last));
                o.practice_cursor = o
                    .practice_cursor
                    .min(o.practice_left.len().saturating_sub(1));
                o.practice_done[2] = true;
            }
            "palette" => o.practice_done[3] = true,
            _ => return,
        }
        cx.notify();
    }

    /// Keys while onboarding shows. Returns whether the key was handled.
    pub(crate) fn onboarding_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(step) = self.onboarding_step() else {
            return false;
        };
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        if step == OnboardStep::Keys {
            if keystroke.modifiers.secondary() && key == "k" {
                self.practice("palette", cx);
                return true;
            }
            if !keystroke.modifiers.modified() && matches!(key, "j" | "k" | "e") {
                self.practice(key, cx);
                return true;
            }
        }
        if key == "enter" && !keystroke.modifiers.modified() {
            self.onboard_next(window, cx);
            return true;
        }
        // Inbox shortcuts don't apply behind onboarding.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_move_and_clamp() {
        assert_eq!(OnboardStep::Welcome.offset(-1), OnboardStep::Welcome);
        assert_eq!(OnboardStep::Welcome.offset(1), OnboardStep::Connect);
        assert_eq!(OnboardStep::Done.offset(1), OnboardStep::Done);
        for step in OnboardStep::ALL {
            let (eyebrow, heading, sub) = step.copy(false);
            assert!(!eyebrow.is_empty() && !heading.is_empty() && !sub.is_empty());
        }
        assert_eq!(OnboardStep::Connect.copy(true).1, "You're in.");
        assert_eq!(PRACTICE_HINTS.len(), PRACTICE_TASKS.len() + 1);
    }
}
