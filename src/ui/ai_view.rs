//! How AI shows up: a sparkle on everything it wrote, the summary card in
//! the reader, draft chips above the reply box, tone buttons below it, and
//! summaries in place of previews in the list.

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, TestSupportExt as _, div, px};

use super::ai_ui::AiState;
use super::app::{OpenThread, SiftApp};
use crate::ai::prompts::Tone;
use crate::assets::Lucide;
use crate::domain::choices::AiFeature;
use crate::store::ThreadSummary;
use crate::theme::tokens::{Palette, radius, text};

fn sparkle_label(label: &'static str, p: &Palette) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .text_size(px(text::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.accent_text)
        .child(Icon::new(Lucide::Sparkles).size(px(12.0)))
        .child(label)
        .into_any_element()
}

impl SiftApp {
    /// The list's second line in Summary mode, if there's a summary to show.
    pub(super) fn row_summary(&self, thread: &ThreadSummary, p: &Palette) -> Option<AnyElement> {
        if !self.summaries || !self.ai_allowed(thread.thread.account, AiFeature::Summaries) {
            return None;
        }
        let (text, colour) = match self.insight(thread)? {
            AiState::Ready(i) => (i.summary.clone(), p.accent_text),
            AiState::Loading => ("Summarising…".to_owned(), p.ink_3),
            AiState::Failed(_) => return None,
        };
        Some(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(6.0))
                .text_color(colour)
                .child(Icon::new(Lucide::Sparkles).size(px(12.0)))
                .child(div().truncate().child(text))
                .into_any_element(),
        )
    }

    /// The Summary card at the top of the reader, with to-dos.
    pub(super) fn ai_card(
        &self,
        open: &OpenThread,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let account = open.summary.thread.account;
        let summaries = self.ai_allowed(account, AiFeature::Summaries);
        let tasks = self.ai_allowed(account, AiFeature::Tasks);
        if !summaries && !tasks {
            return None;
        }
        let state = self.insight(&open.summary)?;
        let hover = p.bg_hover;
        let body = match state {
            AiState::Loading => div()
                .text_color(p.ink_3)
                .child("Reading the thread…")
                .into_any_element(),
            AiState::Failed(why) => div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .text_color(p.ink_3)
                        .child(format!("No summary: {why}")),
                )
                .child(
                    div()
                        .id("retry-summary")
                        .h(px(28.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(p.accent_line)
                        .bg(p.bg)
                        .hover(move |s| s.bg(hover))
                        .text_size(px(text::META))
                        .child("Try again")
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.retry_insights(cx)),
                        )
                        .test_support(),
                )
                .into_any_element(),
            AiState::Ready(insights) => {
                let todos: Vec<AnyElement> = if tasks {
                    insights
                        .todos
                        .iter()
                        .enumerate()
                        .map(|(i, todo)| {
                            let done = self.todo_key(i).is_some_and(|k| self.todo_done(&k));
                            div()
                                .id(("todo", i))
                                .min_h(px(32.0))
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .size(px(16.0))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(4.0))
                                        .border_1()
                                        .border_color(if done { p.accent } else { p.line_strong })
                                        .when(done, |c| {
                                            c.bg(p.accent)
                                                .text_color(p.accent_ink)
                                                .child(Icon::new(IconName::Check).size(px(12.0)))
                                        })
                                        .when(!done, |c| c.bg(p.bg)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(if done { p.ink_3 } else { p.ink })
                                        .when(done, |t| t.line_through())
                                        .child(todo.text.clone()),
                                )
                                .when_some(todo.due.clone(), |d, due| {
                                    d.child(
                                        div()
                                            .text_size(px(text::META))
                                            .text_color(p.warn)
                                            .child(format!("Due {due}")),
                                    )
                                })
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.toggle_todo(i, cx)
                                }))
                                .test_support()
                                .into_any_element()
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .when(summaries, |d| {
                        d.child(
                            div()
                                .text_size(px(text::BODY))
                                .line_height(px(22.0))
                                .child(insights.summary.clone()),
                        )
                    })
                    .when(!todos.is_empty(), |d| {
                        d.child(div().pt(px(4.0)).flex().flex_col().children(todos))
                    })
                    .into_any_element()
            }
        };
        Some(
            div()
                .p(px(16.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .rounded(px(radius::CARD))
                .border_1()
                .border_color(p.accent_line)
                .bg(p.accent_soft)
                .child(sparkle_label("SUMMARY", p))
                .child(body)
                .into_any_element(),
        )
    }

    /// Draft chips above the reply box, or `None` when drafts are off.
    pub(super) fn drafts_row(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.open.as_ref()?;
        if !self.ai_allowed(open.summary.thread.account, AiFeature::Drafts) {
            return None;
        }
        let hover = p.bg_hover;
        let content = match self.draft_replies(&open.summary)? {
            AiState::Loading => div()
                .text_color(p.ink_3)
                .child("Drafting replies…")
                .into_any_element(),
            AiState::Failed(why) => div()
                .text_color(p.ink_3)
                .child(format!("No drafts: {why}"))
                .into_any_element(),
            AiState::Ready(drafts) if drafts.is_empty() => div()
                .text_color(p.ink_3)
                .child("No reply needed. Press E to mark done.")
                .into_any_element(),
            AiState::Ready(drafts) => div()
                .flex()
                .flex_wrap()
                .gap(px(6.0))
                .children(drafts.iter().enumerate().map(|(i, d)| {
                    div()
                        .id(("draft", i))
                        .h(px(28.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .border_1()
                        .border_color(p.accent_line)
                        .bg(p.bg)
                        .text_size(px(text::META))
                        .hover(move |s| s.bg(hover))
                        .child(d.label.clone())
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.use_draft(i, window, cx)
                        }))
                        .test_support()
                }))
                .into_any_element(),
        };
        Some(
            div()
                .px(px(14.0))
                .py(px(10.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .border_b_1()
                .border_color(p.line)
                .bg(p.accent_soft)
                .text_size(px(text::META))
                .child(sparkle_label("DRAFTS", p))
                .child(div().flex_1().min_w_0().child(content))
                .into_any_element(),
        )
    }

    /// Shorter / Friendlier / More formal, under the reply box.
    pub(super) fn tone_buttons(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.open.as_ref()?;
        if !self.ai_allowed(open.summary.thread.account, AiFeature::Drafts) {
            return None;
        }
        let empty = self
            .reply_box
            .as_ref()
            .is_none_or(|r| r.read(cx).value().trim().is_empty());
        let hover = p.bg_hover;
        let buttons: Vec<AnyElement> = [Tone::Shorter, Tone::Friendlier, Tone::Formal]
            .into_iter()
            .enumerate()
            .map(|(i, tone)| {
                div()
                    .id(("tone", i))
                    .h(px(28.0))
                    .px(px(10.0))
                    .flex()
                    .flex_none()
                    .items_center()
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(p.line_strong)
                    .text_size(px(text::META))
                    .when(empty || self.rewriting, |b| b.opacity(0.45))
                    .when(!empty && !self.rewriting, |b| b.hover(move |s| s.bg(hover)))
                    .child(tone.label())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.rewrite_reply(tone, window, cx)
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .flex()
                .gap(px(6.0))
                .children(buttons)
                .into_any_element(),
        )
    }
}
