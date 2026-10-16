//! The main Sift window's state and behaviour: which view is open, the thread
//! list, the keyboard cursor, the open thread, snooze and undo.
//!
//! Rendering lives in [`super::render`]; this file owns what happens. Every
//! action goes through the store first and then reloads, so the screen always
//! shows what is persisted.

use std::rc::Rc;

use chrono::{Local, Utc};
use gpui_kit::{Context, FocusHandle, KeyDownEvent, Window};

use crate::domain::time::{SnoozeOption, snooze_options};
use crate::domain::{ThreadRef, TriageState, TriageView};
use crate::store::{MailStore, MessageDetail, ThreadQuery, ThreadSummary, ViewCounts};
use crate::sync::service::SyncService;

use super::accounts::AddAccountForm;

/// The thread currently open in the reader.
pub(crate) struct OpenThread {
    pub(crate) summary: ThreadSummary,
    pub(crate) messages: Vec<MessageDetail>,
}

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
    pub(crate) form: Option<AddAccountForm>,
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
        };
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
        let query = ThreadQuery {
            view: self.view,
            account: None,
            unread_only: self.unread_only,
        };
        let result = self
            .store
            .wake_snoozed(Utc::now())
            .and_then(|_| Ok((self.store.threads(query)?, self.store.counts(None)?)));
        match result {
            Ok((threads, counts)) => {
                self.threads = threads;
                self.counts = counts;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Sift couldn't read your mail: {error}")),
        }
        self.cursor = self.cursor.min(self.threads.len().saturating_sub(1));
    }

    /// Selects an inbox view and returns to the list.
    pub fn select(&mut self, view: TriageView, cx: &mut Context<Self>) {
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
        let opened = self
            .store
            .set_read(&summary.thread, true)
            .and_then(|()| self.store.thread_messages(&summary.thread));
        match opened {
            Ok(messages) => {
                self.open = Some(OpenThread { summary, messages });
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

    /// Marks the target thread done, with undo.
    pub fn mark_done(&mut self, cx: &mut Context<Self>) {
        if let Some(thread) = self.target() {
            self.apply(vec![thread], TriageState::Done, "Marked done".into(), cx);
        }
    }

    /// Opens the snooze menu for the target thread.
    pub fn start_snooze(&mut self, cx: &mut Context<Self>) {
        if let Some(thread) = self.target() {
            self.snoozing = Some((vec![thread], snooze_options(Utc::now(), &Local)));
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
        if let Err(error) = self.store.set_read(&thread, unread) {
            self.error = Some(format!("Sift couldn't update that thread: {error}"));
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let key = keystroke.key.as_str();
        let digit = key.parse::<u8>().ok();
        if self.form.is_some() {
            // Typing goes to the dialog's inputs; only Escape is ours.
            if key == "escape" {
                self.close_add_account(cx);
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
                _ => return,
            }
        }
        cx.stop_propagation();
    }
}
