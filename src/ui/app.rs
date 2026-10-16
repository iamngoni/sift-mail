//! The main Sift window's state and behaviour: which view is open, the thread
//! list, the keyboard cursor, the open thread, snooze and undo.
//!
//! Rendering lives in [`super::render`]; this file owns what happens. Every
//! action goes through the store first and then reloads, so the screen always
//! shows what is persisted.

use std::rc::Rc;

use chrono::{Local, Utc};
use gpui_kit::component::input::TextareaState;
use gpui_kit::{Context, Entity, FocusHandle, Focusable as _, KeyDownEvent, Window};

use crate::domain::time::{SnoozeOption, snooze_options};
use crate::domain::{AccountId, MessageId, ThreadRef, TriageState, TriageView};
use crate::store::{
    Account, Attachment, CustomView, CustomViewId, MailStore, MessageDetail, StoreResult,
    ThreadQuery, ThreadSummary, ViewCounts,
};
use crate::sync::service::SyncService;

use super::add_account::AddAccountFlow;
use super::chrome::Overlay;
use super::compose::ComposeForm;
use super::settings::SettingsState;

/// The thread currently open in the reader.
pub(crate) struct OpenThread {
    pub(crate) summary: ThreadSummary,
    pub(crate) messages: Vec<MessageDetail>,
    /// Attachments per message, in the same order.
    pub(crate) files: Vec<Vec<Attachment>>,
}

/// Opens a saved attachment with the system's default app.
pub(crate) type Opener = fn(&std::path::Path) -> std::io::Result<()>;

/// The message at the bottom of the window after an action.
pub(crate) struct Toast {
    pub(crate) text: String,
    /// Previous states to restore if the user presses Undo.
    pub(crate) undo: Option<Vec<(ThreadRef, TriageState)>>,
}

/// Root view of the main window.
pub struct SiftApp {
    pub(crate) store: Rc<MailStore>,
    pub(crate) focus: FocusHandle,
    pub(crate) view: TriageView,
    pub(crate) unread_only: bool,
    pub(crate) threads: Vec<ThreadSummary>,
    pub(crate) counts: ViewCounts,
    pub(crate) cursor: usize,
    pub(crate) open: Option<OpenThread>,
    /// Threads waiting for the user to pick a snooze time.
    pub(crate) snoozing: Option<(Vec<ThreadRef>, Vec<SnoozeOption>)>,
    pub(crate) toast: Option<Toast>,
    pub(crate) error: Option<String>,
    /// Background sync, or `None` when the store has no file (tests, previews).
    pub(crate) sync: Option<SyncService>,
    pub(crate) syncing: bool,
    pub(crate) sync_status: Option<String>,
    /// The Add account dialog, while open.
    pub(crate) form: Option<AddAccountFlow>,
    /// The reply box under an open thread; created on first render of a thread.
    pub(crate) reply_box: Option<Entity<TextareaState>>,
    /// Why the last reply couldn't be sent.
    pub(crate) reply_error: Option<String>,
    /// A message is being sent.
    pub(crate) sending: bool,
    /// The New message dialog, while open.
    pub(crate) compose: Option<ComposeForm>,
    /// The account the inbox is filtered to; `None` is All inboxes.
    pub(crate) account: Option<AccountId>,
    /// Connected accounts, in sidebar order.
    pub(crate) accounts: Vec<Account>,
    /// Saved custom views, shown as extra tabs.
    pub(crate) custom_views: Vec<CustomView>,
    /// The custom view on screen, if one is.
    pub(crate) custom: Option<CustomViewId>,
    /// Threads ticked for a bulk action.
    pub(crate) selected: Vec<ThreadRef>,
    /// The open dialog or popover.
    pub(crate) overlay: Option<Overlay>,
    pub(crate) sidebar: bool,
    pub(crate) summaries: bool,
    pub(crate) hold: bool,
    /// Earlier messages the user expanded in the open thread.
    pub(crate) expanded: Vec<MessageId>,
    pub(crate) opener: Opener,
    /// The Settings screen, while open.
    pub(crate) settings: Option<SettingsState>,
    /// Compact rows.
    pub(crate) compact: bool,
}

impl SiftApp {
    /// Builds the view over `store`, gives it keyboard focus, and starts
    /// background sync when `sync` is given.
    pub fn new(
        store: Rc<MailStore>,
        sync: Option<SyncService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut app = Self {
            store,
            focus,
            view: TriageView::default(),
            unread_only: false,
            threads: Vec::new(),
            counts: ViewCounts::default(),
            cursor: 0,
            open: None,
            snoozing: None,
            toast: None,
            error: None,
            sync,
            syncing: false,
            sync_status: None,
            form: None,
            reply_box: None,
            reply_error: None,
            sending: false,
            compose: None,
            account: None,
            accounts: Vec::new(),
            custom_views: Vec::new(),
            custom: None,
            selected: Vec::new(),
            overlay: None,
            sidebar: false,
            summaries: false,
            hold: false,
            expanded: Vec::new(),
            opener: |path| open::that_detached(path),
            settings: None,
            compact: false,
        };
        app.load_preferences(window, cx);
        app.reload();
        app.schedule_sync(cx);
        app
    }

    /// The selected inbox view.
    pub fn view(&self) -> TriageView {
        self.view
    }

    /// Subjects of the listed threads, in order.
    pub fn listed_subjects(&self) -> Vec<String> {
        self.threads.iter().map(|t| t.subject.clone()).collect()
    }

    /// Index of the keyboard cursor in the list.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Subject of the open thread, if one is open.
    pub fn open_subject(&self) -> Option<&str> {
        self.open.as_ref().map(|o| o.summary.subject.as_str())
    }

    /// Text of the current toast, if any.
    pub fn toast_text(&self) -> Option<&str> {
        self.toast.as_ref().map(|t| t.text.as_str())
    }

    /// Thread count shown on a view's tab.
    pub fn count(&self, view: TriageView) -> usize {
        self.counts.get(view)
    }

    /// Whether the snooze menu is showing.
    pub fn is_snoozing(&self) -> bool {
        self.snoozing.is_some()
    }

    /// Re-reads the list and counts from the store, waking due snoozes first.
    pub(crate) fn reload(&mut self) {
        match self.load() {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(format!("Sift couldn't read your mail: {error}")),
        }
        self.cursor = self.cursor.min(self.threads.len().saturating_sub(1));
        let threads = &self.threads;
        self.selected
            .retain(|s| threads.iter().any(|t| t.thread == *s));
    }

    fn load(&mut self) -> StoreResult<()> {
        self.store.wake_snoozed(Utc::now())?;
        self.accounts = self.store.accounts()?;
        if self
            .account
            .is_some_and(|id| !self.accounts.iter().any(|a| a.id == id))
        {
            self.account = None;
        }
        self.custom_views = self.store.custom_views()?;
        let custom = self
            .custom
            .and_then(|id| self.custom_views.iter().find(|v| v.id == id));
        self.threads = match custom {
            Some(view) => {
                let mut threads = self.store.custom_view_threads(&view.terms, self.account)?;
                if self.unread_only {
                    threads.retain(|t| t.unread);
                }
                threads
            }
            None => {
                self.custom = None;
                self.store.threads(ThreadQuery {
                    view: self.view,
                    account: self.account,
                    unread_only: self.unread_only,
                })?
            }
        };
        self.counts = self.store.counts(self.account)?;
        Ok(())
    }

    /// The custom view on screen, if one is.
    pub(crate) fn current_custom(&self) -> Option<&CustomView> {
        self.custom
            .and_then(|id| self.custom_views.iter().find(|v| v.id == id))
    }

    /// The title of what's listed: a view or a custom view.
    pub fn list_title(&self) -> String {
        self.current_custom()
            .map_or_else(|| self.view.label().to_owned(), |v| v.label.clone())
    }

    /// Selects an inbox view and returns to the list.
    pub fn select(&mut self, view: TriageView, cx: &mut Context<Self>) {
        self.custom = None;
        self.view = view;
        self.cursor = 0;
        self.open = None;
        self.reload();
        cx.notify();
    }

    /// Switches between all threads and unread threads only.
    pub fn set_unread_only(&mut self, unread_only: bool, cx: &mut Context<Self>) {
        self.unread_only = unread_only;
        self.cursor = 0;
        self.reload();
        cx.notify();
    }

    /// Moves the cursor, or steps to the next or previous thread when reading.
    pub fn move_cursor(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.threads.is_empty() {
            return;
        }
        let last = self.threads.len() - 1;
        let next = self.cursor.saturating_add_signed(delta).min(last);
        self.cursor = next;
        if self.open.is_some() {
            self.open_at(next, cx);
        }
        cx.notify();
    }

    /// Opens the thread at `index` and marks it read.
    pub fn open_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(summary) = self.threads.get(index).cloned() else {
            return;
        };
        self.cursor = index;
        self.open_summary(summary, cx);
    }

    /// Opens a thread, listed or not (search finds done mail too), and marks it read.
    pub(crate) fn open_summary(&mut self, summary: ThreadSummary, cx: &mut Context<Self>) {
        if let Some(index) = self.threads.iter().position(|t| t.thread == summary.thread) {
            self.cursor = index;
        }
        let opened = self
            .store
            .set_read(&summary.thread, true)
            .and_then(|()| self.store.thread_messages(&summary.thread));
        match opened {
            Ok(messages) => {
                if summary.unread {
                    self.push_read(summary.thread.clone(), true, cx);
                }
                if self
                    .open
                    .as_ref()
                    .is_none_or(|o| o.summary.thread != summary.thread)
                {
                    self.reply_box = None;
                    self.reply_error = None;
                    self.expanded.clear();
                }
                let files = messages
                    .iter()
                    .map(|m| self.store.attachments(m.id).unwrap_or_default())
                    .collect();
                self.open = Some(OpenThread {
                    summary,
                    messages,
                    files,
                });
                self.reload();
            }
            Err(error) => self.error = Some(format!("Sift couldn't open that thread: {error}")),
        }
        cx.notify();
    }

    /// Closes the reader and returns to the list.
    pub fn close_thread(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        cx.notify();
    }

    /// The thread an action applies to: the open one, else the cursor row.
    fn target(&self) -> Option<ThreadRef> {
        self.open
            .as_ref()
            .map(|o| o.summary.thread.clone())
            .or_else(|| self.threads.get(self.cursor).map(|t| t.thread.clone()))
    }

    /// What a bulk-capable action applies to: the ticked threads when the list
    /// is showing, else the target thread.
    fn targets(&self) -> Vec<ThreadRef> {
        if self.open.is_none() && !self.selected.is_empty() {
            self.selected.clone()
        } else {
            self.target().into_iter().collect()
        }
    }

    /// Marks the target threads done, with undo.
    pub fn mark_done(&mut self, cx: &mut Context<Self>) {
        let threads = self.targets();
        if !threads.is_empty() {
            let text = match threads.len() {
                1 => "Marked done".to_owned(),
                n => format!("{n} conversations marked done"),
            };
            self.apply(threads, TriageState::Done, text, cx);
        }
    }

    /// Marks one listed thread done, from its row.
    pub fn mark_row_done(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.threads.get(index) {
            let thread = t.thread.clone();
            self.apply(vec![thread], TriageState::Done, "Marked done".into(), cx);
        }
    }

    /// Opens the snooze menu for one listed thread, from its row.
    pub fn snooze_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.threads.get(index) {
            self.snoozing = Some((vec![t.thread.clone()], snooze_options(Utc::now(), &Local)));
            cx.notify();
        }
    }

    /// Flips read state of one listed thread, from its row.
    pub fn toggle_row_read(&mut self, index: usize, cx: &mut Context<Self>) {
        self.cursor = index.min(self.threads.len().saturating_sub(1));
        self.toggle_read(cx);
    }

    /// Opens the snooze menu for the target thread.
    pub fn start_snooze(&mut self, cx: &mut Context<Self>) {
        let threads = self.targets();
        if !threads.is_empty() {
            self.snoozing = Some((threads, snooze_options(Utc::now(), &Local)));
            cx.notify();
        }
    }

    /// Snoozes with the `index`th menu option.
    pub fn choose_snooze(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((threads, options)) = self.snoozing.take() else {
            return;
        };
        match options.get(index) {
            Some(option) => {
                let text = format!("Snoozed until {}", option.when);
                self.apply(
                    threads,
                    TriageState::Snoozed {
                        until: option.until,
                    },
                    text,
                    cx,
                );
            }
            None => self.snoozing = Some((threads, options)),
        }
        cx.notify();
    }

    /// Closes the snooze menu without snoozing.
    pub fn cancel_snooze(&mut self, cx: &mut Context<Self>) {
        self.snoozing = None;
        cx.notify();
    }

    /// Flips read state of the target thread.
    pub fn toggle_read(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.target() else {
            return;
        };
        let unread = self
            .threads
            .iter()
            .find(|t| t.thread == thread)
            .is_some_and(|t| t.unread);
        match self.store.set_read(&thread, unread) {
            Ok(()) => self.push_read(thread, unread, cx),
            Err(error) => self.error = Some(format!("Sift couldn't update that thread: {error}")),
        }
        self.open = None;
        self.reload();
        cx.notify();
    }

    /// Undoes the last done or snooze.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(previous) = self.toast.take().and_then(|t| t.undo) else {
            return;
        };
        if let Err(error) = self.store.restore(&previous) {
            self.error = Some(format!("Sift couldn't undo that: {error}"));
        }
        self.reload();
        cx.notify();
    }

    /// Hides the toast.
    pub fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        self.toast = None;
        cx.notify();
    }

    fn apply(
        &mut self,
        threads: Vec<ThreadRef>,
        state: TriageState,
        text: String,
        cx: &mut Context<Self>,
    ) {
        match self.store.set_state(&threads, state) {
            Ok(previous) => {
                self.toast = Some(Toast {
                    text,
                    undo: Some(previous),
                });
                let was_open = self.open.take().is_some();
                self.reload();
                if was_open && !self.threads.is_empty() {
                    // Keep reading: open whatever now sits under the cursor.
                    self.open_at(self.cursor, cx);
                }
            }
            Err(error) => self.error = Some(format!("Sift couldn't update that thread: {error}")),
        }
        cx.notify();
    }

    pub(crate) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        let replying = self
            .reply_box
            .as_ref()
            .is_some_and(|b| b.read(cx).focus_handle(cx).is_focused(window));
        if keystroke.modifiers.secondary()
            && key == "k"
            && self.form.is_none()
            && self.compose.is_none()
        {
            self.toggle_palette(window, cx);
            cx.stop_propagation();
            return;
        }
        if keystroke.modifiers.secondary() && key == "," && self.form.is_none() {
            self.open_settings(super::settings::SettingsPage::General, window, cx);
            cx.stop_propagation();
            return;
        }
        if self.settings.is_some() && self.form.is_none() {
            // Settings has its own text boxes; Escape backs out.
            if key == "escape" {
                if self
                    .settings
                    .as_ref()
                    .is_some_and(|s| s.confirm_remove.is_some())
                {
                    self.cancel_remove_account(cx);
                } else {
                    self.close_settings(window, cx);
                }
                cx.stop_propagation();
            }
            return;
        }
        if self.overlay.is_some() {
            // The palette and New view boxes take typing; Escape closes.
            if key == "escape" {
                self.close_overlay(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        let typed = keystroke.key_char.as_deref().unwrap_or(key);
        if typed == "?" && self.form.is_none() && self.compose.is_none() && !replying {
            self.toggle_overlay("shortcuts", cx);
            cx.stop_propagation();
            return;
        }
        if keystroke.modifiers.modified() {
            return;
        }
        let digit = key.parse::<u8>().ok();
        if self.form.is_some() || self.compose.is_some() || replying {
            // Typing goes to the text boxes; only Escape is ours.
            if key == "escape" {
                if self.is_waiting_for_browser() {
                    self.cancel_oauth(cx);
                } else if self.form.is_some() {
                    if self.can_close_add_account() {
                        self.close_add_account(cx);
                    }
                } else if self.compose.is_some() {
                    self.close_compose(cx);
                } else {
                    self.focus.focus(window, cx);
                }
                cx.stop_propagation();
            }
            return;
        }
        if self.snoozing.is_some() {
            match (key, digit) {
                ("escape", _) => self.cancel_snooze(cx),
                (_, Some(d)) if d >= 1 => self.choose_snooze(usize::from(d - 1), cx),
                _ => return,
            }
        } else if let Some(view) = digit.and_then(TriageView::from_shortcut) {
            self.select(view, cx);
        } else if let Some(d) = digit.filter(|d| *d >= 7) {
            let Some(id) = self.custom_views.get(usize::from(d - 7)).map(|v| v.id) else {
                return;
            };
            self.select_custom(id, cx);
        } else {
            match key {
                "j" | "down" => self.move_cursor(1, cx),
                "k" | "up" => self.move_cursor(-1, cx),
                "enter" | "o" if self.open.is_none() => self.open_at(self.cursor, cx),
                "escape" if self.open.is_some() => self.close_thread(cx),
                "e" => self.mark_done(cx),
                "s" => self.start_snooze(cx),
                "u" => self.toggle_read(cx),
                "z" => self.undo(cx),
                "r" if self.open.is_some() => self.focus_reply(window, cx),
                "c" => self.open_compose(window, cx),
                "x" if self.open.is_none() => {
                    if let Some(t) = self.threads.get(self.cursor) {
                        self.toggle_select(t.thread.clone(), cx);
                    }
                }
                "/" => self.toggle_palette(window, cx),
                "a" => self.set_summaries(!self.summaries, cx),
                "[" => self.toggle_sidebar(cx),
                "escape" if !self.selected.is_empty() => self.clear_selection(cx),
                _ => return,
            }
        }
        cx.stop_propagation();
    }
}
