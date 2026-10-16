//! Everything around the list and the reader: the account filter and sidebar,
//! custom views, the command palette, the New view and shortcuts dialogs, the
//! account and notification popovers, bulk selection, and the preferences
//! that survive a restart.
//!
//! Rendering lives in [`super::chrome_view`].

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, Entity, Focusable as _, Window};

use crate::domain::{AccountId, ThreadRef, TriageView};
use crate::store::{CustomViewId, Pref, ThreadSummary};
use crate::theme::{self, Appearance, SiftTheme};

use super::app::SiftApp;

/// Most mail results the palette shows above the commands.
const PALETTE_MAIL: usize = 4;

/// A dialog or popover over the window. Only one is open at a time.
pub(crate) enum Overlay {
    /// The keyboard shortcuts sheet.
    Shortcuts,
    /// Search and commands.
    Palette(PaletteState),
    /// Describing a new view.
    NewView(NewViewForm),
    /// The account switcher under the title bar.
    Accounts,
    /// The notifications popover.
    Notifications,
}

/// The open command palette.
pub(crate) struct PaletteState {
    pub(crate) input: Entity<InputState>,
    pub(crate) items: Vec<PaletteItem>,
    pub(crate) selected: usize,
}

/// The open New view dialog.
pub(crate) struct NewViewForm {
    pub(crate) input: Entity<InputState>,
    pub(crate) error: Option<String>,
}

/// One row in the palette.
#[derive(Debug, Clone)]
pub(crate) struct PaletteItem {
    pub(crate) action: PaletteAction,
    pub(crate) label: String,
    pub(crate) sub: String,
    pub(crate) key: &'static str,
}

/// What a palette row does.
#[derive(Debug, Clone)]
pub(crate) enum PaletteAction {
    /// Opens a thread found by search.
    Open(ThreadSummary),
    /// Runs a command.
    Run(Command),
}

/// A command the palette can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Opens the New message dialog.
    NewMessage,
    /// Switches to a view.
    GoTo(TriageView),
    /// Switches the list's second line between previews and AI summaries.
    ToggleSummaries,
    /// Marks everything listed as read.
    MarkAllRead,
    /// Switches between light and dark.
    ToggleTheme,
    /// Shows or hides the sidebar.
    ToggleSidebar,
    /// Opens the New view dialog.
    NewView,
    /// Opens the shortcuts sheet.
    Shortcuts,
    /// Opens the Add account dialog.
    AddAccount,
    /// Opens Settings.
    Settings,
}

/// Example descriptions offered in the New view dialog.
pub const VIEW_IDEAS: [&str; 3] = [
    "Anything about the offsite",
    "Mail with attachments from Finance",
    "Travel bookings",
];

/// The shortcuts sheet, in the order the design lists them.
pub const SHORTCUTS: [(&str, &str); 14] = [
    ("Next / previous", "J / K"),
    ("Open", "Enter"),
    ("Done", "E"),
    ("Snooze", "S"),
    ("Reply", "R"),
    ("Send", "Ctrl ↵"),
    ("Select", "X"),
    ("Mark read / unread", "U"),
    ("New message", "C"),
    ("Search and commands", "Ctrl K"),
    ("AI summaries", "A"),
    ("Sidebar", "["),
    ("Switch view", "1 – 9"),
    ("Back / close", "Esc"),
];

impl Command {
    /// Every command, in palette order.
    pub const ALL: [Command; 11] = [
        Self::NewMessage,
        Self::GoTo(TriageView::NeedsYou),
        Self::GoTo(TriageView::Meetings),
        Self::ToggleSummaries,
        Self::MarkAllRead,
        Self::ToggleTheme,
        Self::ToggleSidebar,
        Self::NewView,
        Self::Shortcuts,
        Self::AddAccount,
        Self::Settings,
    ];

    /// The palette label, which reads as what will happen given the current state.
    pub fn label(self, summaries: bool, sidebar: bool, dark: bool) -> String {
        match self {
            Self::NewMessage => "New message".into(),
            Self::GoTo(view) => format!("Go to {}", view.label()),
            Self::ToggleSummaries if summaries => "Show previews instead of summaries".into(),
            Self::ToggleSummaries => "Show AI summaries in the list".into(),
            Self::MarkAllRead => "Mark everything in this view read".into(),
            Self::ToggleTheme if dark => "Switch to light theme".into(),
            Self::ToggleTheme => "Switch to dark theme".into(),
            Self::ToggleSidebar if sidebar => "Hide sidebar".into(),
            Self::ToggleSidebar => "Show sidebar".into(),
            Self::NewView => "Create a view…".into(),
            Self::Shortcuts => "Keyboard shortcuts".into(),
            Self::AddAccount => "Add account…".into(),
            Self::Settings => "Settings".into(),
        }
    }

    /// The key hint shown on the right, if the command has one.
    pub fn key(self) -> &'static str {
        match self {
            Self::NewMessage => "C",
            Self::GoTo(TriageView::NeedsYou) => "2",
            Self::GoTo(TriageView::Meetings) => "3",
            Self::ToggleSummaries => "A",
            Self::ToggleSidebar => "[",
            Self::Shortcuts => "?",
            Self::Settings => "Ctrl ,",
            _ => "",
        }
    }
}

impl SiftApp {
    /// Loads saved preferences and applies the saved appearance.
    pub(crate) fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar = self.store.flag(Pref::Sidebar, false).unwrap_or(false);
        self.summaries = self.store.flag(Pref::Summaries, false).unwrap_or(false);
        self.hold = self
            .store
            .flag(Pref::HoldNotifications, false)
            .unwrap_or(false);
        self.compact = self
            .store
            .preference(Pref::Density)
            .ok()
            .flatten()
            .as_deref()
            == Some("compact");
        let appearance = self
            .store
            .preference(Pref::Appearance)
            .ok()
            .flatten()
            .map_or(Appearance::System, |k| Appearance::from_key(&k));
        if appearance != SiftTheme::get(cx).appearance() {
            theme::apply(appearance, Some(window), cx);
        }
    }

    fn save_flag(&mut self, pref: Pref, value: bool) {
        if let Err(error) = self
            .store
            .set_preference(pref, if value { "true" } else { "false" })
        {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
    }

    /// Whether the sidebar is showing.
    pub fn sidebar_open(&self) -> bool {
        self.sidebar
    }

    /// Whether the list shows AI summaries instead of previews.
    pub fn summaries_on(&self) -> bool {
        self.summaries
    }

    /// Whether notifications are held.
    pub fn holding(&self) -> bool {
        self.hold
    }

    /// The account the inbox is filtered to, if any.
    pub fn account_filter(&self) -> Option<AccountId> {
        self.account
    }

    /// Threads ticked for a bulk action.
    pub fn selection(&self) -> &[ThreadRef] {
        &self.selected
    }

    /// Labels of the custom view tabs.
    pub fn custom_view_labels(&self) -> Vec<String> {
        self.custom_views.iter().map(|v| v.label.clone()).collect()
    }

    /// Whether a dialog or popover is open, and which.
    pub fn overlay_name(&self) -> Option<&'static str> {
        self.overlay.as_ref().map(|o| match o {
            Overlay::Shortcuts => "shortcuts",
            Overlay::Palette(_) => "palette",
            Overlay::NewView(_) => "new-view",
            Overlay::Accounts => "accounts",
            Overlay::Notifications => "notifications",
        })
    }

    /// Labels of the palette rows, in order.
    pub fn palette_labels(&self) -> Vec<String> {
        match &self.overlay {
            Some(Overlay::Palette(p)) => p.items.iter().map(|i| i.label.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// The highlighted palette row.
    pub fn palette_selected(&self) -> Option<usize> {
        match &self.overlay {
            Some(Overlay::Palette(p)) => Some(p.selected),
            _ => None,
        }
    }

    /// Shows or hides the sidebar, and remembers it.
    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar = !self.sidebar;
        self.save_flag(Pref::Sidebar, self.sidebar);
        cx.notify();
    }

    /// Switches the list's second line between previews and summaries.
    pub fn set_summaries(&mut self, on: bool, cx: &mut Context<Self>) {
        self.summaries = on;
        self.save_flag(Pref::Summaries, on);
        cx.notify();
    }

    /// Holds or releases notifications.
    pub fn toggle_hold(&mut self, cx: &mut Context<Self>) {
        self.hold = !self.hold;
        self.save_flag(Pref::HoldNotifications, self.hold);
        cx.notify();
    }

    /// Moves to the next appearance and remembers it.
    pub fn cycle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = SiftTheme::get(cx).appearance().next();
        self.set_appearance(next, window, cx);
    }

    /// Applies and remembers an appearance.
    pub fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        theme::apply(appearance, Some(window), cx);
        if let Err(error) = self
            .store
            .set_preference(Pref::Appearance, appearance.key())
        {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
        cx.notify();
    }

    /// Filters the inbox to one account, or every account in All inboxes.
    pub fn set_account(&mut self, account: Option<AccountId>, cx: &mut Context<Self>) {
        self.account = account;
        self.cursor = 0;
        self.open = None;
        if matches!(self.overlay, Some(Overlay::Accounts)) {
            self.overlay = None;
        }
        self.reload();
        cx.notify();
    }

    /// Shows a custom view's tab.
    pub fn select_custom(&mut self, id: CustomViewId, cx: &mut Context<Self>) {
        self.custom = Some(id);
        self.view = TriageView::All;
        self.cursor = 0;
        self.open = None;
        self.reload();
        cx.notify();
    }

    /// Deletes the open custom view and goes back to All.
    pub fn remove_custom(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.custom.take() {
            if let Err(error) = self.store.remove_custom_view(id) {
                self.error = Some(format!("Sift couldn't remove that view: {error}"));
            }
            self.select(TriageView::All, cx);
        }
    }

    /// Ticks or unticks a thread for a bulk action.
    pub fn toggle_select(&mut self, thread: ThreadRef, cx: &mut Context<Self>) {
        if let Some(i) = self.selected.iter().position(|t| *t == thread) {
            self.selected.remove(i);
        } else {
            self.selected.push(thread);
        }
        cx.notify();
    }

    /// Ticks every listed thread, or clears the ticks if all are ticked.
    pub fn toggle_select_all(&mut self, cx: &mut Context<Self>) {
        if !self.threads.is_empty() && self.selected.len() == self.threads.len() {
            self.selected.clear();
        } else {
            self.selected = self.threads.iter().map(|t| t.thread.clone()).collect();
        }
        cx.notify();
    }

    /// Clears the ticks.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected.clear();
        cx.notify();
    }

    /// Marks the ticked threads read.
    pub fn bulk_read(&mut self, cx: &mut Context<Self>) {
        let threads = std::mem::take(&mut self.selected);
        self.mark_read(threads, cx);
    }

    /// Marks everything listed read.
    pub fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        let threads = self
            .threads
            .iter()
            .filter(|t| t.unread)
            .map(|t| t.thread.clone())
            .collect();
        self.mark_read(threads, cx);
    }

    fn mark_read(&mut self, threads: Vec<ThreadRef>, cx: &mut Context<Self>) {
        for thread in threads {
            match self.store.set_read(&thread, true) {
                Ok(()) => self.push_read(thread, true, cx),
                Err(error) => {
                    self.error = Some(format!("Sift couldn't update that thread: {error}"));
                }
            }
        }
        self.reload();
        cx.notify();
    }

    /// Opens a popover or sheet that has no text box, or closes it if it's open.
    pub fn toggle_overlay(&mut self, which: &'static str, cx: &mut Context<Self>) {
        let wanted = match which {
            "accounts" => Overlay::Accounts,
            "notifications" => Overlay::Notifications,
            _ => Overlay::Shortcuts,
        };
        self.overlay = if self.overlay_name() == Some(which) {
            None
        } else {
            Some(wanted)
        };
        cx.notify();
    }

    /// Closes whatever dialog or popover is open and gives focus back to the list.
    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Opens the palette, or closes it if it's open.
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.overlay, Some(Overlay::Palette(_))) {
            self.close_overlay(window, cx);
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search, run a command, or ask a question…")
        });
        cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let query = input.read(cx).value().to_string();
                    let dark = SiftTheme::get(cx).palette() == &theme::tokens::DARK;
                    let items = this.palette_items(&query, dark);
                    if let Some(Overlay::Palette(p)) = &mut this.overlay {
                        p.items = items;
                        p.selected = 0;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.run_palette_selected(window, cx),
                _ => {}
            },
        )
        .detach();
        input.read(cx).focus_handle(cx).focus(window, cx);
        let dark = SiftTheme::get(cx).palette() == &theme::tokens::DARK;
        let items = self.palette_items("", dark);
        self.overlay = Some(Overlay::Palette(PaletteState {
            input,
            items,
            selected: 0,
        }));
        cx.notify();
    }

    /// Mail matches first, then the commands whose label contains the query.
    pub(crate) fn palette_items(&self, query: &str, dark: bool) -> Vec<PaletteItem> {
        let query = query.trim();
        let needle = query.to_lowercase();
        let mut items = Vec::new();
        if !query.is_empty() {
            let hits = self.store.search(query).unwrap_or_default();
            items.extend(hits.into_iter().take(PALETTE_MAIL).map(|t| PaletteItem {
                label: if t.subject.trim().is_empty() {
                    "(no subject)".into()
                } else {
                    t.subject.clone()
                },
                sub: t.from.display().to_owned(),
                key: "",
                action: PaletteAction::Open(t),
            }));
        }
        items.extend(Command::ALL.into_iter().filter_map(|command| {
            let label = command.label(self.summaries, self.sidebar, dark);
            label.to_lowercase().contains(&needle).then(|| PaletteItem {
                label,
                sub: String::new(),
                key: command.key(),
                action: PaletteAction::Run(command),
            })
        }));
        items
    }

    /// Moves the palette highlight.
    pub fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(Overlay::Palette(p)) = &mut self.overlay {
            let last = p.items.len().saturating_sub(1);
            p.selected = p.selected.saturating_add_signed(delta).min(last);
            cx.notify();
        }
    }

    /// Runs the highlighted palette row.
    pub fn run_palette_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.palette_selected() {
            self.run_palette(index, window, cx);
        }
    }

    /// Runs the palette row at `index`.
    pub fn run_palette(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Palette(p)) = &self.overlay else {
            return;
        };
        let Some(item) = p.items.get(index).cloned() else {
            return;
        };
        self.close_overlay(window, cx);
        match item.action {
            PaletteAction::Open(summary) => self.open_summary(summary, cx),
            PaletteAction::Run(command) => self.run_command(command, window, cx),
        }
    }

    /// Runs a command.
    pub fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::NewMessage => self.open_compose(window, cx),
            Command::GoTo(view) => self.select(view, cx),
            Command::ToggleSummaries => self.set_summaries(!self.summaries, cx),
            Command::MarkAllRead => self.mark_all_read(cx),
            Command::ToggleTheme => {
                let dark = SiftTheme::get(cx).palette() == &theme::tokens::DARK;
                let next = if dark {
                    Appearance::Light
                } else {
                    Appearance::Dark
                };
                self.set_appearance(next, window, cx);
            }
            Command::ToggleSidebar => self.toggle_sidebar(cx),
            Command::NewView => self.open_new_view(window, cx),
            Command::Shortcuts => self.toggle_overlay("shortcuts", cx),
            Command::AddAccount => self.open_add_account(window, cx),
            Command::Settings => {
                self.open_settings(super::settings::SettingsPage::General, window, cx)
            }
        }
    }

    /// Opens a saved attachment with the system's default app.
    pub fn open_attachment(&mut self, path: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        let result = match &path {
            Some(path) => (self.opener)(path).map_err(|e| e.to_string()),
            None => Err("this file wasn't downloaded".to_owned()),
        };
        if let Err(why) = result {
            self.error = Some(format!("Sift couldn't open that file: {why}"));
        }
        cx.notify();
    }

    /// Expands or collapses an earlier message in the open thread.
    pub fn toggle_message(&mut self, id: crate::domain::MessageId, cx: &mut Context<Self>) {
        if let Some(i) = self.expanded.iter().position(|m| *m == id) {
            self.expanded.remove(i);
        } else {
            self.expanded.push(id);
        }
        cx.notify();
    }

    /// Opens the New view dialog.
    pub fn open_new_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. anything about the offsite"));
        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.create_view(window, cx);
            }
        })
        .detach();
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.overlay = Some(Overlay::NewView(NewViewForm { input, error: None }));
        cx.notify();
    }

    /// Fills the New view box with one of the examples.
    pub fn use_view_idea(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(Overlay::NewView(form)), Some(idea)) = (&self.overlay, VIEW_IDEAS.get(index)) {
            let input = form.input.clone();
            input.update(cx, |state, cx| state.set_value(*idea, window, cx));
        }
    }

    /// Saves the described view and shows its tab.
    pub fn create_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::NewView(form)) = &self.overlay else {
            return;
        };
        let text = form.input.read(cx).value().to_string();
        match self.store.add_custom_view(&text) {
            Ok(Some(view)) => {
                self.close_overlay(window, cx);
                self.select_custom(view.id, cx);
            }
            Ok(None) => {
                if let Some(Overlay::NewView(form)) = &mut self.overlay {
                    form.error =
                        Some("Describe it with a word or two, like a person or a topic.".into());
                }
                cx.notify();
            }
            Err(error) => {
                if let Some(Overlay::NewView(form)) = &mut self.overlay {
                    form.error = Some(format!("Sift couldn't save that view: {error}"));
                }
                cx.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_labels_follow_state() {
        assert_eq!(
            Command::ToggleSummaries.label(true, false, false),
            "Show previews instead of summaries"
        );
        assert_eq!(
            Command::ToggleSummaries.label(false, false, false),
            "Show AI summaries in the list"
        );
        assert_eq!(
            Command::ToggleSidebar.label(false, true, false),
            "Hide sidebar"
        );
        assert_eq!(
            Command::ToggleSidebar.label(false, false, false),
            "Show sidebar"
        );
        assert_eq!(
            Command::ToggleTheme.label(false, false, true),
            "Switch to light theme"
        );
        assert_eq!(
            Command::ToggleTheme.label(false, false, false),
            "Switch to dark theme"
        );
        assert_eq!(
            Command::GoTo(TriageView::NeedsYou).label(false, false, false),
            "Go to Needs you"
        );
        let keys: Vec<_> = Command::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(
            keys,
            ["C", "2", "3", "A", "", "", "[", "", "?", "", "Ctrl ,"]
        );
        for c in Command::ALL {
            assert!(!c.label(false, false, false).is_empty());
        }
        assert_eq!(SHORTCUTS.len(), 14);
    }
}
