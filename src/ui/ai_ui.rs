//! AI in the window: list summaries, the reader's summary and to-dos, reply
//! drafts and tone rewrites, Ask your inbox in the palette, and Write it in
//! the New message dialog.
//!
//! AI only reads a thread when the user uses a feature on it, only for
//! accounts with AI on, and only for helpers switched on. Every call runs on
//! the background executor; answers are remembered in the store, keyed by the
//! thread's newest message, so nothing is asked twice.

use std::collections::VecDeque;
use std::sync::Arc;

use chrono::Local;
use gpui_kit::{Context, Window};

use crate::ai::prompts::{self, Answer, DraftReply, Insights, MailText, Source, Tone};
use crate::ai::{AiError, AiProvider, Assistant, CliAssistant};
use crate::domain::choices::AiFeature;
use crate::domain::time::long_label;
use crate::domain::{AccountId, ThreadRef};
use crate::store::{Pref, ThreadSummary};

use super::app::SiftApp;
use super::chrome::Overlay;

/// Where an AI answer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiState<T> {
    /// Asked, waiting.
    Loading,
    /// Answered.
    Ready(T),
    /// Couldn't answer; the message says why.
    Failed(String),
}

/// An Ask your inbox answer with the thread it came from.
#[derive(Debug, Clone)]
pub struct InboxAnswer {
    /// The answer.
    pub answer: Answer,
    /// The supporting thread, if any.
    pub source: Option<ThreadSummary>,
}

/// Most list rows summarised at once.
const SUMMARY_BATCH: usize = 25;

fn memo_key(kind: &str, thread: &ThreadSummary) -> String {
    format!(
        "{kind}:{}:{}:{}",
        thread.thread.account.0, thread.thread.key, thread.latest.0
    )
}

impl SiftApp {
    /// Picks the assistant from the saved provider.
    pub(crate) fn load_assistant(&mut self) {
        self.ai_provider = self
            .store
            .preference(Pref::AiProvider)
            .ok()
            .flatten()
            .map_or_else(AiProvider::default, |k| AiProvider::from_key(&k));
        self.ai = CliAssistant::find(self.ai_provider).map(|a| Arc::new(a) as Arc<dyn Assistant>);
    }

    /// Uses another assistant from now on.
    pub fn set_ai_provider(&mut self, provider: AiProvider, cx: &mut Context<Self>) {
        if let Err(error) = self.store.set_preference(Pref::AiProvider, provider.key()) {
            self.error = Some(format!("Sift couldn't save that setting: {error}"));
        }
        self.load_assistant();
        self.insights.clear();
        self.drafts.clear();
        cx.notify();
    }

    /// Replaces the assistant, for tests and previews.
    pub fn use_assistant(&mut self, assistant: Option<Arc<dyn Assistant>>) {
        self.ai = assistant;
    }

    /// Whether a helper may read mail from `account`.
    pub fn ai_allowed(&self, account: AccountId, feature: AiFeature) -> bool {
        self.ai.is_some()
            && self.ai_features.on(feature)
            && self
                .store
                .account_settings(account)
                .map(|s| s.ai)
                .unwrap_or(false)
    }

    fn mail_text(&self, thread: &ThreadRef) -> Vec<MailText> {
        self.store
            .thread_messages(thread)
            .unwrap_or_default()
            .into_iter()
            .map(|m| MailText {
                from: m.from.display().to_owned(),
                when: long_label(m.received_at, &Local),
                body: m.body_text,
            })
            .collect()
    }

    fn me(&self, account: AccountId) -> String {
        self.accounts.iter().find(|a| a.id == account).map_or_else(
            || "the reader".to_owned(),
            |a| format!("{} <{}>", a.name, a.email),
        )
    }

    /// The summary and to-dos for a thread, if asked for.
    pub fn insight(&self, thread: &ThreadSummary) -> Option<&AiState<Insights>> {
        self.insights.get(&memo_key("insights", thread))
    }

    /// Reply drafts for a thread, if asked for.
    pub fn draft_replies(&self, thread: &ThreadSummary) -> Option<&AiState<Vec<DraftReply>>> {
        self.drafts.get(&memo_key("drafts", thread))
    }

    /// Asks for a thread's insights unless they're known or not allowed.
    pub(crate) fn ensure_insights(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let allowed = self.ai_allowed(thread.thread.account, AiFeature::Summaries)
            || self.ai_allowed(thread.thread.account, AiFeature::Tasks);
        let key = memo_key("insights", thread);
        if !allowed || self.insights.contains_key(&key) {
            return;
        }
        if let Some(saved) = self
            .store
            .memo(&key)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Insights>(&s).ok())
        {
            self.insights.insert(key, AiState::Ready(saved));
            return;
        }
        self.summary_queue.push_back(thread.clone());
        self.insights.insert(key, AiState::Loading);
        self.run_summary_queue(cx);
    }

    /// Works through queued summaries one at a time, so the assistant isn't
    /// started twenty times at once.
    fn run_summary_queue(&mut self, cx: &mut Context<Self>) {
        if self.summary_running {
            return;
        }
        let (Some(assistant), Some(thread)) = (self.ai.clone(), self.summary_queue.pop_front())
        else {
            return;
        };
        self.summary_running = true;
        let prompt = prompts::insights_prompt(
            &thread.subject,
            &self.mail_text(&thread.thread),
            &Local::now().format("%a %-d %b %Y").to_string(),
        );
        let key = memo_key("insights", &thread);
        let task = cx.background_executor().spawn(async move {
            assistant
                .complete(&prompt)
                .and_then(|a| prompts::parse_insights(&a))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                app.summary_running = false;
                let state = match result {
                    Ok(insights) => {
                        if let Ok(json) = serde_json::to_string(&insights) {
                            let _ = app.store.remember(&key, &json);
                        }
                        AiState::Ready(insights)
                    }
                    Err(error) => AiState::Failed(error.to_string()),
                };
                app.insights.insert(key, state);
                app.run_summary_queue(cx);
                cx.notify();
            })
        })
        .detach();
    }

    /// Summarises the listed threads when the list shows summaries.
    pub(crate) fn ensure_list_summaries(&mut self, cx: &mut Context<Self>) {
        if !self.summaries || self.ai.is_none() {
            return;
        }
        let threads: Vec<ThreadSummary> =
            self.threads.iter().take(SUMMARY_BATCH).cloned().collect();
        for thread in threads {
            if self.ai_allowed(thread.thread.account, AiFeature::Summaries) {
                self.ensure_insights(&thread, cx);
            }
        }
    }

    /// Tries a failed summary again.
    pub fn retry_insights(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = &self.open {
            let summary = open.summary.clone();
            self.insights.remove(&memo_key("insights", &summary));
            self.ensure_insights(&summary, cx);
            cx.notify();
        }
    }

    /// Asks for reply drafts for a thread unless known or not allowed.
    pub(crate) fn ensure_drafts(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let key = memo_key("drafts", thread);
        if !self.ai_allowed(thread.thread.account, AiFeature::Drafts)
            || self.drafts.contains_key(&key)
        {
            return;
        }
        if let Some(saved) = self
            .store
            .memo(&key)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Vec<DraftReply>>(&s).ok())
        {
            self.drafts.insert(key, AiState::Ready(saved));
            return;
        }
        let Some(assistant) = self.ai.clone() else {
            return;
        };
        let prompt = prompts::drafts_prompt(
            &thread.subject,
            &self.mail_text(&thread.thread),
            &self.me(thread.thread.account),
        );
        self.drafts.insert(key.clone(), AiState::Loading);
        let task = cx.background_executor().spawn(async move {
            assistant
                .complete(&prompt)
                .and_then(|a| prompts::parse_drafts(&a))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                let state = match result {
                    Ok(drafts) => {
                        if let Ok(json) = serde_json::to_string(&drafts) {
                            let _ = app.store.remember(&key, &json);
                        }
                        AiState::Ready(drafts)
                    }
                    Err(error) => AiState::Failed(error.to_string()),
                };
                app.drafts.insert(key, state);
                cx.notify();
            })
        })
        .detach();
    }

    /// AI for a thread that just opened.
    pub(crate) fn thread_opened_ai(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) {
        self.ensure_insights(thread, cx);
        self.ensure_drafts(thread, cx);
    }

    /// Puts a draft into the reply box.
    pub fn use_draft(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        let text = match self.draft_replies(&open.summary) {
            Some(AiState::Ready(drafts)) => drafts.get(index).map(|d| d.text.clone()),
            _ => None,
        };
        if let (Some(text), Some(reply)) = (text, self.reply_box.clone()) {
            reply.update(cx, |s, cx| s.set_value(text, window, cx));
            self.reply_error = None;
            cx.notify();
        }
    }

    /// Rewrites the reply in a tone.
    pub fn rewrite_reply(&mut self, tone: Tone, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(assistant), Some(reply)) = (self.ai.clone(), self.reply_box.clone()) else {
            return;
        };
        let text = reply.read(cx).value().to_string();
        if text.trim().is_empty() || self.rewriting {
            return;
        }
        self.rewriting = true;
        self.reply_error = None;
        cx.notify();
        let prompt = prompts::rewrite_prompt(&text, tone);
        let task = cx.background_executor().spawn(async move {
            assistant
                .complete(&prompt)
                .and_then(|a| prompts::parse_rewrite(&a))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, window, cx| {
                app.rewriting = false;
                match result {
                    Ok(text) => reply.update(cx, |s, cx| s.set_value(text, window, cx)),
                    Err(error) => app.reply_error = Some(format!("Couldn't rewrite that: {error}")),
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Marks a to-do done or not done; remembered per thread.
    pub fn toggle_todo(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        let key = format!("{}:todo:{index}", memo_key("insights", &open.summary));
        let done = self.todo_done(&key);
        if let Err(error) = self.store.remember(&key, if done { "0" } else { "1" }) {
            self.error = Some(format!("Sift couldn't save that: {error}"));
        }
        cx.notify();
    }

    pub(crate) fn todo_done(&self, key: &str) -> bool {
        self.store.memo(key).ok().flatten().as_deref() == Some("1")
    }

    /// The memo key of a to-do in the open thread.
    pub(crate) fn todo_key(&self, index: usize) -> Option<String> {
        self.open
            .as_ref()
            .map(|o| format!("{}:todo:{index}", memo_key("insights", &o.summary)))
    }

    /// Asks the inbox the palette's question.
    pub fn ask_inbox(&mut self, question: String, cx: &mut Context<Self>) {
        let Some(assistant) = self.ai.clone() else {
            return;
        };
        let hits: Vec<ThreadSummary> = self
            .store
            .search_any(&question)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| self.ai_allowed(t.thread.account, AiFeature::Ask))
            .take(5)
            .collect();
        let sources: Vec<Source> = hits
            .iter()
            .map(|t| Source {
                subject: t.subject.clone(),
                from: t.from.display().to_owned(),
                body: self
                    .mail_text(&t.thread)
                    .last()
                    .map(|m| m.body.clone())
                    .unwrap_or_default(),
            })
            .collect();
        let prompt = prompts::ask_prompt(
            &question,
            &sources,
            &Local::now().format("%a %-d %b %Y").to_string(),
        );
        let count = sources.len();
        if let Some(Overlay::Palette(p)) = &mut self.overlay {
            p.answer = Some(AiState::Loading);
        }
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            assistant
                .complete(&prompt)
                .and_then(|a| prompts::parse_answer(&a, count))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                let state = match result {
                    Ok(answer) => {
                        let source = answer.source.and_then(|i| hits.get(i - 1).cloned());
                        AiState::Ready(InboxAnswer { answer, source })
                    }
                    Err(error) => AiState::Failed(error.to_string()),
                };
                if let Some(Overlay::Palette(p)) = &mut app.overlay {
                    p.answer = Some(state);
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Writes the New message from its AI description.
    pub fn write_with_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(assistant) = self.ai.clone() else {
            if let Some(form) = &mut self.compose {
                form.error = Some(AiError::NotInstalled(self.ai_provider.label()).to_string());
            }
            cx.notify();
            return;
        };
        let me = self
            .default_from()
            .map_or_else(|| "the reader".to_owned(), |a| self.me(a));
        let Some(form) = &mut self.compose else {
            return;
        };
        let instruction = form.ai_prompt.read(cx).value().to_string();
        if instruction.trim().is_empty() || form.writing {
            return;
        }
        let to = form.to_text(cx);
        let subject = form.subject.read(cx).value().to_string();
        form.writing = true;
        form.error = None;
        let (subject_box, body_box) = (form.subject.clone(), form.body.clone());
        cx.notify();
        let prompt = prompts::write_prompt(&instruction, &to, &subject, &me);
        let task = cx.background_executor().spawn(async move {
            assistant
                .complete(&prompt)
                .and_then(|a| prompts::parse_written(&a))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |app, window, cx| {
                if let Some(form) = &mut app.compose {
                    form.writing = false;
                    match result {
                        Ok(written) => {
                            if subject.trim().is_empty() && !written.subject.is_empty() {
                                subject_box
                                    .update(cx, |s, cx| s.set_value(written.subject, window, cx));
                            }
                            body_box.update(cx, |s, cx| s.set_value(written.body, window, cx));
                        }
                        Err(error) => form.error = Some(format!("Couldn't write that: {error}")),
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }
}

/// Queue type for list summaries.
pub(crate) type SummaryQueue = VecDeque<ThreadSummary>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_compare() {
        assert_ne!(AiState::<u8>::Loading, AiState::Ready(1));
        assert_eq!(
            AiState::<u8>::Failed("x".into()),
            AiState::Failed("x".into())
        );
    }
}
