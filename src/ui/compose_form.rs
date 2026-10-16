//! The New message dialog from the Main board: recipients as chips with
//! suggestions from people you've heard from, a subject, a line to tell AI
//! what to write, and the message. Closing keeps the draft for next time;
//! Discard throws it away.

use gpui_kit::component::input::{
    Backspace, Input, InputEvent, InputState, MoveDown, MoveUp, Textarea, TextareaState,
};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, Focusable as _, FontWeight, TestSupportExt as _,
    Window, div, px,
};

use super::app::SiftApp;
use super::render::scrim;
use crate::assets::Lucide;
use crate::domain::{Contact, EmailAddress};
use crate::theme::tokens::{Palette, avatar_tints, radius, text};

/// The New message dialog.
pub(crate) struct ComposeForm {
    /// Where the next recipient is typed.
    pub(crate) to: Entity<InputState>,
    /// Recipients so far.
    pub(crate) chips: Vec<Contact>,
    /// People matching what's typed in To.
    pub(crate) suggestions: Vec<Contact>,
    /// The highlighted suggestion.
    pub(crate) suggestion: usize,
    pub(crate) subject: Entity<InputState>,
    /// What to tell AI to write.
    pub(crate) ai_prompt: Entity<InputState>,
    pub(crate) body: Entity<TextareaState>,
    pub(crate) writing: bool,
    pub(crate) error: Option<String>,
}

impl ComposeForm {
    /// Every recipient, chips first, then whatever is still typed.
    pub(crate) fn to_text(&self, cx: &gpui_kit::App) -> String {
        let typed = self.to.read(cx).value().to_string();
        self.chips
            .iter()
            .map(|c| c.email().to_string())
            .chain(std::iter::once(typed).filter(|t| !t.trim().is_empty()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl SiftApp {
    /// Opens the New message dialog, bringing back a closed draft if there is one.
    pub fn open_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = self.compose_draft.take() {
            draft.to.read(cx).focus_handle(cx).focus(window, cx);
            self.compose = Some(draft);
            cx.notify();
            return;
        }
        let to = cx.new(|cx| InputState::new(window, cx).placeholder("Name or address"));
        let subject = cx.new(|cx| InputState::new(window, cx).placeholder("Subject"));
        let ai_prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Tell AI what to say, e.g. ask Priya to move our 1:1 to Thursday")
        });
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Write your message…")
                .auto_grow(6, 14)
        });
        cx.subscribe_in(
            &to,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let typed = input.read(cx).value().to_string();
                    if typed.contains([',', ';']) {
                        this.commit_recipient(window, cx);
                    } else {
                        let found = this.store.contacts(&typed).unwrap_or_default();
                        if let Some(form) = &mut this.compose {
                            form.suggestions = found;
                            form.suggestion = 0;
                        }
                        cx.notify();
                    }
                }
                InputEvent::PressEnter {
                    secondary: true, ..
                } => this.send_compose(window, cx),
                InputEvent::PressEnter { .. } => this.commit_recipient(window, cx),
                _ => {}
            },
        )
        .detach();
        cx.subscribe_in(&subject, window, send_on_secondary_enter)
            .detach();
        cx.subscribe_in(&body, window, send_on_secondary_enter)
            .detach();
        cx.subscribe_in(
            &ai_prompt,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.write_with_ai(window, cx);
                }
            },
        )
        .detach();
        to.read(cx).focus_handle(cx).focus(window, cx);
        self.compose = Some(ComposeForm {
            to,
            chips: Vec::new(),
            suggestions: Vec::new(),
            suggestion: 0,
            subject,
            ai_prompt,
            body,
            writing: false,
            error: None,
        });
        cx.notify();
    }

    /// Closes the dialog and keeps the draft for next time.
    pub fn close_compose(&mut self, cx: &mut Context<Self>) {
        self.compose_draft = self.compose.take();
        cx.notify();
    }

    /// Throws the draft away.
    pub fn discard_compose(&mut self, cx: &mut Context<Self>) {
        self.compose = None;
        self.compose_draft = None;
        cx.notify();
    }

    /// Turns what's typed in To (or the highlighted suggestion) into a chip.
    pub fn commit_recipient(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.compose else {
            return;
        };
        let typed = form.to.read(cx).value().to_string();
        let mut added = Vec::new();
        if let Some(pick) = form.suggestions.get(form.suggestion).cloned()
            && !typed.contains([',', ';'])
        {
            added.push(pick);
        } else {
            for part in typed
                .split([',', ';'])
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                match EmailAddress::parse(part) {
                    Ok(address) => added.push(Contact::new(None, address)),
                    Err(error) => {
                        form.error = Some(format!("\"{part}\": {error}"));
                        cx.notify();
                        return;
                    }
                }
            }
        }
        for contact in added {
            if !form.chips.iter().any(|c| c.email() == contact.email()) {
                form.chips.push(contact);
            }
        }
        form.suggestions.clear();
        form.suggestion = 0;
        form.error = None;
        let input = form.to.clone();
        input.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    /// Adds a suggested person.
    pub fn pick_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.compose {
            form.suggestion = index;
        }
        self.commit_recipient(window, cx);
    }

    /// Removes a recipient chip.
    pub fn remove_recipient(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.compose
            && index < form.chips.len()
        {
            form.chips.remove(index);
            cx.notify();
        }
    }

    fn move_suggestion(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        let Some(form) = &mut self.compose else {
            return false;
        };
        if form.suggestions.is_empty() {
            return false;
        }
        let last = form.suggestions.len() - 1;
        form.suggestion = form.suggestion.saturating_add_signed(delta).min(last);
        cx.notify();
        true
    }

    pub(crate) fn compose_dialog(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.compose.as_ref() else {
            return div().into_any_element();
        };
        let hover = p.bg_hover;
        let tints = avatar_tints(p);
        let field = |label: &'static str, content: AnyElement| {
            div()
                .min_h(px(44.0))
                .px(px(16.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .border_b_1()
                .border_color(p.line)
                .child(
                    div()
                        .w(px(56.0))
                        .flex_none()
                        .text_color(p.ink_3)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(content))
        };
        let chips: Vec<AnyElement> = form
            .chips
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (bg, ink) = tints[i % 6];
                div()
                    .h(px(26.0))
                    .pl(px(3.0))
                    .pr(px(4.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded_full()
                    .bg(p.bg_hover)
                    .child(
                        div()
                            .size(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(bg)
                            .text_color(ink)
                            .text_size(px(9.0))
                            .font_weight(FontWeight::BOLD)
                            .child(c.initials()),
                    )
                    .child(c.display().to_owned())
                    .child(
                        div()
                            .id(("remove-recipient", i))
                            .size(px(18.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .text_color(p.ink_3)
                            .hover(move |s| s.bg(hover))
                            .child(Icon::new(IconName::Close).size(px(10.0)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove_recipient(i, cx)
                            }))
                            .test_support(),
                    )
                    .into_any_element()
            })
            .collect();
        let suggestions: Vec<AnyElement> = form
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let on = i == form.suggestion;
                div()
                    .id(("suggestion", i))
                    .h(px(40.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(7.0))
                    .when(on, |s| s.bg(p.bg_active))
                    .when(!on, |s| s.hover(move |st| st.bg(hover)))
                    .child(
                        div()
                            .size(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(p.accent_soft)
                            .text_color(p.accent_text)
                            .text_size(px(10.0))
                            .font_weight(FontWeight::BOLD)
                            .child(c.initials()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(c.display().to_owned()),
                            )
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(c.email().to_string()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.pick_suggestion(i, window, cx)
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let has_suggestions = !suggestions.is_empty();
        let typed_subject = form.subject.read(cx).value().to_string();
        let title = if typed_subject.trim().is_empty() {
            "New message".to_owned()
        } else {
            typed_subject
        };
        let ai_empty = form.ai_prompt.read(cx).value().trim().is_empty();
        div()
            .id("compose-scrim")
            .occlude()
            .absolute()
            .inset_0()
            .bg(scrim())
            .flex()
            .justify_center()
            .items_start()
            .pt(px(72.0))
            .child(
                div()
                    .id("compose")
                    .w(px(600.0))
                    .max_w_full()
                    .flex_none()
                    .rounded(px(radius::CARD))
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.line_strong)
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(48.0))
                            .pl(px(16.0))
                            .pr(px(8.0))
                            .flex()
                            .items_center()
                            .border_b_1()
                            .border_color(p.line)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .id("compose-close")
                                    .size(px(32.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(radius::BUTTON))
                                    .text_color(p.ink_2)
                                    .hover(move |s| s.bg(hover))
                                    .child(Icon::new(IconName::Close).size(px(16.0)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.close_compose(cx);
                                        this.focus.focus(window, cx);
                                    }))
                                    .test_support(),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .child(field(
                                "To",
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(6.0))
                                    .py(px(6.0))
                                    .children(chips)
                                    .child(
                                        div()
                                            .min_w(px(160.0))
                                            .flex_1()
                                            .capture_action(cx.listener(
                                                |this, _: &MoveUp, _, cx| {
                                                    if this.move_suggestion(-1, cx) {
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            ))
                                            .capture_action(cx.listener(
                                                |this, _: &MoveDown, _, cx| {
                                                    if this.move_suggestion(1, cx) {
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            ))
                                            .capture_action(cx.listener(
                                                |this, _: &Backspace, _, cx| {
                                                    let empty =
                                                        this.compose.as_ref().is_some_and(|f| {
                                                            f.to.read(cx).value().is_empty()
                                                                && !f.chips.is_empty()
                                                        });
                                                    if empty {
                                                        let last = this
                                                            .compose
                                                            .as_ref()
                                                            .map_or(0, |f| f.chips.len() - 1);
                                                        this.remove_recipient(last, cx);
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            ))
                                            .child(
                                                Input::new(&form.to)
                                                    .appearance(false)
                                                    .id("compose-to"),
                                            ),
                                    )
                                    .into_any_element(),
                            ))
                            .when(has_suggestions, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top(px(46.0))
                                        .left(px(80.0))
                                        .w(px(320.0))
                                        .p(px(4.0))
                                        .rounded(px(radius::CARD))
                                        .border_1()
                                        .border_color(p.line_strong)
                                        .bg(p.bg)
                                        .children(suggestions),
                                )
                            }),
                    )
                    .child(field(
                        "Subject",
                        Input::new(&form.subject)
                            .appearance(false)
                            .id("compose-subject")
                            .into_any_element(),
                    ))
                    .child(
                        div()
                            .min_h(px(48.0))
                            .px(px(16.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .bg(p.accent_soft)
                            .border_b_1()
                            .border_color(p.accent_line)
                            .child(
                                Icon::new(Lucide::Sparkles)
                                    .size(px(16.0))
                                    .text_color(p.accent_text),
                            )
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&form.ai_prompt)
                                        .appearance(false)
                                        .id("compose-ai"),
                                ),
                            )
                            .child(
                                div()
                                    .id("write-it")
                                    .h(px(28.0))
                                    .px(px(10.0))
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .rounded(px(7.0))
                                    .border_1()
                                    .border_color(p.accent_line)
                                    .bg(p.bg)
                                    .text_size(px(text::META))
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(ai_empty || form.writing, |b| b.opacity(0.5))
                                    .child(if form.writing {
                                        "Writing…"
                                    } else {
                                        "Write it"
                                    })
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.write_with_ai(window, cx)
                                    }))
                                    .test_support(),
                            ),
                    )
                    .child(div().p(px(12.0)).child(Textarea::new(&form.body)))
                    .child(
                        div()
                            .px(px(16.0))
                            .py(px(12.0))
                            .border_t_1()
                            .border_color(p.line)
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                super::compose::send_button("compose-send", self.sending, p)
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.send_compose(window, cx)
                                    }))
                                    .test_support(),
                            )
                            .child(match form.error.clone() {
                                Some(e) => div()
                                    .id("compose-error")
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .gap(px(6.0))
                                    .text_color(p.danger)
                                    .child(Icon::new(IconName::CircleAlert).size(px(14.0)))
                                    .child(e)
                                    .into_any_element(),
                                None => div()
                                    .flex_1()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child("Closing keeps your draft")
                                    .into_any_element(),
                            })
                            .child(
                                div()
                                    .id("compose-discard")
                                    .size(px(32.0))
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(radius::BUTTON))
                                    .text_color(p.ink_3)
                                    .hover(move |s| s.bg(hover))
                                    .child(Icon::new(Lucide::Trash).size(px(16.0)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.discard_compose(cx);
                                        this.focus.focus(window, cx);
                                    }))
                                    .test_support(),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn send_on_secondary_enter<S: 'static>(
    this: &mut SiftApp,
    _: &Entity<S>,
    event: &InputEvent,
    window: &mut Window,
    cx: &mut Context<SiftApp>,
) {
    if matches!(
        event,
        InputEvent::PressEnter {
            secondary: true,
            ..
        }
    ) {
        this.send_compose(window, cx);
    }
}
