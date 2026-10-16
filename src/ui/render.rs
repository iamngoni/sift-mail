//! How the main window looks, following the approved Sift style spec:
//! 44 px title bar with view tabs, 40 px single-line rows under centred day
//! sections, a centred 720 px reader, and a floating toolbar with toasts.

use chrono::{Local, Utc};
use gpui_kit::component::{Icon, IconName, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, Hsla, SharedString, TestSupportExt as _, Window,
    div, hsla, px,
};

use super::app::{OpenThread, SiftApp};
use crate::brand;
use crate::domain::TriageView;
use crate::domain::message::initials;
use crate::domain::time::{DayBucket, bucket, long_label, short_label};
use crate::store::ThreadSummary;
use crate::theme::tokens::{Palette, radius, size, text};
use crate::theme::{self, Appearance, SiftTheme};

/// Width of the sender column, from the style spec.
const SENDER_WIDTH: f32 = 184.0;
/// Widest the reading column gets.
const READER_WIDTH: f32 = 720.0;
/// Widest the list column gets.
const LIST_WIDTH: f32 = 1000.0;

impl Render for SiftApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sift = *SiftTheme::get(cx);
        let p = *sift.palette();
        let body = match &self.open {
            Some(open) => self.reader(open, &p, cx),
            None => self.list(&p, cx),
        };
        div()
            .id("sift-root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.ink)
            .text_size(px(text::UI))
            .child(self.title_bar(&p, cx))
            .when_some(self.error.clone(), |root, message| {
                root.child(
                    div()
                        .px(px(24.0))
                        .py(px(8.0))
                        .bg(p.bg_subtle)
                        .border_b_1()
                        .border_color(p.line)
                        .text_color(p.warn)
                        .child(message),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body)
                    .child(self.dock(&p, sift.appearance(), cx))
                    .when(self.snoozing.is_some(), |area| {
                        area.child(self.snooze_menu(&p, cx))
                    })
                    .when(self.form.is_some(), |area| {
                        area.child(self.add_account_dialog(&p, cx))
                    }),
            )
    }
}

impl SiftApp {
    fn title_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs: Vec<AnyElement> = TriageView::ALL
            .into_iter()
            .map(|view| self.tab(view, p, cx))
            .collect();
        TitleBar::new()
            .h(px(size::TITLE_BAR))
            .bg(p.bg)
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .pl(px(12.0))
                    .child(
                        Icon::new(IconName::Inbox)
                            .size(px(18.0))
                            .text_color(p.accent),
                    )
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::BOLD)
                            .child(brand::SHORT_NAME),
                    )
                    .child(div().w(px(8.0)))
                    .child(div().flex().items_center().gap(px(2.0)).children(tabs)),
            )
            .child(div().flex_1())
            .child(self.title_actions(p, cx))
    }

    fn title_actions(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let hover = p.bg_hover;
        let status = self.sync_status.clone();
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pr(px(8.0))
            .when_some(status, |d, status| {
                d.child(
                    div()
                        .max_w(px(320.0))
                        .truncate()
                        .text_size(px(text::META))
                        .text_color(p.ink_3)
                        .child(status),
                )
            })
            .when(self.sync.is_some(), |d| {
                d.child(
                    div()
                        .id("sync-now")
                        .size(px(size::ICON_BUTTON))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(radius::BUTTON))
                        .text_color(p.ink_2)
                        .hover(move |s| s.bg(hover))
                        .child(Icon::new(IconName::RefreshCw).size(px(16.0)))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start_sync(cx)))
                        .test_support(),
                )
            })
            .child(
                div()
                    .id("add-account")
                    .h(px(size::ICON_BUTTON))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded_full()
                    .bg(p.primary)
                    .text_color(p.primary_ink)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Icon::new(IconName::Plus).size(px(14.0)))
                    .child("Add account")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_add_account(window, cx);
                    }))
                    .test_support(),
            )
    }

    fn tab(&self, view: TriageView, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let active = self.view == view;
        let hover = p.bg_hover;
        div()
            .id(view.id())
            .h(px(size::TAB))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .rounded(px(radius::TAB))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if active { p.ink } else { p.ink_2 })
            .when(active, |tab| {
                tab.bg(p.bg_hover).border_1().border_color(p.line_strong)
            })
            .when(!active, |tab| tab.hover(move |style| style.bg(hover)))
            .child(
                Icon::new(view_icon(view))
                    .size(px(14.0))
                    .text_color(p.ink_3),
            )
            .child(view.label())
            .child(
                div()
                    .text_size(px(text::META))
                    .text_color(p.ink_3)
                    .child(self.counts.get(view).to_string()),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(view, cx)))
            .test_support()
            .into_any_element()
    }

    fn list(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let now = Utc::now();
        let mut rows: Vec<AnyElement> = Vec::with_capacity(self.threads.len() + 4);
        let mut last: Option<DayBucket> = None;
        for (index, thread) in self.threads.iter().enumerate() {
            let section = bucket(thread.received_at, now, &Local);
            if last != Some(section) {
                rows.push(section_header(section, p));
                last = Some(section);
            }
            rows.push(self.row(index, thread, p, now, cx));
        }
        let column = div()
            .w_full()
            .max_w(px(LIST_WIDTH))
            .px(px(24.0))
            .flex()
            .flex_col()
            .child(self.list_header(p, cx))
            .child(div().border_t_1().border_color(p.line).children(rows))
            .when(self.threads.is_empty(), |c| c.child(self.empty_state(p)))
            .child(div().h(px(96.0)));
        div()
            .id("thread-list")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .child(column)
            .into_any_element()
    }

    fn list_header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let segment = |id: &'static str,
                       label: &'static str,
                       on: bool,
                       value: bool,
                       cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(24.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(6.0))
                .text_size(px(text::META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if on { p.ink } else { p.ink_3 })
                .when(on, |s| s.bg(p.bg).border_1().border_color(p.line))
                .child(label)
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.set_unread_only(value, cx)),
                )
                .test_support()
        };
        let count = self.counts.get(self.view);
        div()
            .h(px(56.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .text_size(px(text::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.view.label()),
            )
            .child(div().text_color(p.ink_3).child(format!(
                "{count} {}",
                if count == 1 {
                    "conversation"
                } else {
                    "conversations"
                }
            )))
            .child(
                div()
                    .flex()
                    .p(px(2.0))
                    .gap(px(2.0))
                    .rounded(px(radius::BUTTON))
                    .bg(p.bg_hover)
                    .child(segment("filter-all", "All", !self.unread_only, false, cx))
                    .child(segment(
                        "filter-unread",
                        "Unread",
                        self.unread_only,
                        true,
                        cx,
                    )),
            )
    }

    fn row(
        &self,
        index: usize,
        thread: &ThreadSummary,
        p: &Palette,
        now: chrono::DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = index == self.cursor;
        let hover = p.bg_hover;
        let weight = if thread.unread {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        let strong = if thread.unread { p.ink } else { p.ink_2 };
        let count = (thread.message_count > 1).then(|| thread.message_count.to_string());
        div()
            .id(("thread-row", index))
            .group("row")
            .h(px(size::ROW))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_b_1()
            .border_color(p.line)
            .cursor_pointer()
            .when(current, |row| row.bg(p.bg_active))
            .when(!current, |row| row.hover(move |s| s.bg(hover)))
            .child(avatar(&initials(thread.from.display()), p, 20.0))
            .child(
                div()
                    .w(px(SENDER_WIDTH))
                    .flex_none()
                    .flex()
                    .gap(px(6.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .truncate()
                            .font_weight(weight)
                            .text_color(strong)
                            .child(thread.from.display().to_owned()),
                    )
                    .when_some(count, |s, n| {
                        s.child(div().text_size(px(text::META)).text_color(p.ink_3).child(n))
                    }),
            )
            .child(
                div()
                    .size(px(6.0))
                    .flex_none()
                    .rounded_full()
                    .when(thread.unread, |d| d.bg(p.accent)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(p.ink_3)
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(weight)
                                    .text_color(strong)
                                    .child(subject_or_placeholder(&thread.subject)),
                            )
                            .child(div().truncate().child(format!("— {}", thread.preview))),
                    ),
            )
            .when(thread.has_attachments, |row| {
                row.child(Icon::new(IconName::File).size(px(14.0)).text_color(p.ink_3))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .group_hover("row", |s| s.invisible())
                    .child(chip(thread.view, p))
                    .child(
                        div()
                            .w(px(52.0))
                            .text_right()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child(short_label(thread.received_at, now, &Local)),
                    ),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_at(index, cx)))
            .test_support()
            .into_any_element()
    }

    fn empty_state(&self, p: &Palette) -> impl IntoElement {
        let (title, body) = if self.store.accounts().map(|a| a.is_empty()).unwrap_or(false) {
            (
                "No mail to sift yet".to_owned(),
                "Welcome to Sift Mail. Add an account to start sifting.",
            )
        } else if self.unread_only {
            (
                "No unread mail here".to_owned(),
                "Switch to All to see everything in this view.",
            )
        } else {
            (
                match self.view {
                    TriageView::All => "Inbox zero".to_owned(),
                    other => format!("Nothing in {}", other.label()),
                },
                "Everything here is done or snoozed. Nice.",
            )
        };
        div()
            .py(px(96.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0))
            .child(
                Icon::new(IconName::CircleCheck)
                    .size(px(32.0))
                    .text_color(p.good),
            )
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(div().text_color(p.ink_3).child(body))
    }

    fn reader(&self, open: &OpenThread, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let messages: Vec<AnyElement> = open
            .messages
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let paragraphs: Vec<AnyElement> = m
                    .body_text
                    .split('\n')
                    .map(|line| {
                        div()
                            .min_h(px(22.0))
                            .child(line.to_owned())
                            .into_any_element()
                    })
                    .collect();
                div()
                    .p(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .when(i > 0, |d| d.border_t_1().border_color(p.line))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(avatar(&m.from.initials(), p, 32.0))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(text::BODY))
                                            .child(m.from.display().to_owned()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(text::META))
                                            .text_color(p.ink_3)
                                            .child(m.from.email().to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(long_label(m.received_at, &Local)),
                            ),
                    )
                    .child(
                        div()
                            .pl(px(44.0))
                            .text_size(px(text::BODY))
                            .text_color(p.ink_2)
                            .children(paragraphs),
                    )
                    .into_any_element()
            })
            .collect();
        let count = open.messages.len();
        let back_label: SharedString = self.view.label().into();
        div()
            .id("reader")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(READER_WIDTH))
                    .px(px(24.0))
                    .flex()
                    .flex_col()
                    .gap(px(20.0))
                    .child(
                        div()
                            .h(px(52.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(button(
                                "back",
                                IconName::ChevronLeft,
                                back_label,
                                p,
                                cx.listener(|this, _: &ClickEvent, _, cx| this.close_thread(cx)),
                            ))
                            .child(div().flex_1())
                            .child(button(
                                "done",
                                IconName::Check,
                                "Done  E".into(),
                                p,
                                cx.listener(|this, _: &ClickEvent, _, cx| this.mark_done(cx)),
                            ))
                            .child(button(
                                "snooze",
                                IconName::Bell,
                                "Snooze  S".into(),
                                p,
                                cx.listener(|this, _: &ClickEvent, _, cx| this.start_snooze(cx)),
                            ))
                            .child(button(
                                "unread",
                                IconName::Inbox,
                                "Mark unread  U".into(),
                                p,
                                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_read(cx)),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .text_size(px(text::SUBJECT))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(subject_or_placeholder(&open.summary.subject)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(chip(open.summary.view, p))
                                    .child(if count == 1 {
                                        "1 message".to_owned()
                                    } else {
                                        format!("{count} messages")
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .rounded(px(radius::CARD))
                            .border_1()
                            .border_color(p.line_strong)
                            .overflow_hidden()
                            .children(messages),
                    )
                    .child(div().h(px(96.0))),
            )
            .into_any_element()
    }

    fn dock(
        &self,
        p: &Palette,
        appearance: Appearance,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover = p.bg_hover;
        let theme_label: SharedString = format!("Theme: {}", appearance.label()).into();
        let toast = self
            .toast
            .as_ref()
            .map(|t| (t.text.clone(), t.undo.is_some()));
        div()
            .absolute()
            .bottom(px(16.0))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
            .when_some(toast, |dock, (text, can_undo)| {
                dock.child(
                    div()
                        .h(px(40.0))
                        .pl(px(14.0))
                        .pr(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .rounded(px(radius::TOAST))
                        .bg(p.primary)
                        .text_color(p.primary_ink)
                        .child(text)
                        .when(can_undo, |t| {
                            t.child(
                                div()
                                    .id("undo")
                                    .h(px(28.0))
                                    .px(px(10.0))
                                    .flex()
                                    .items_center()
                                    .rounded(px(7.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Undo  Z")
                                    .on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| this.undo(cx)),
                                    )
                                    .test_support(),
                            )
                        })
                        .child(
                            div()
                                .id("dismiss-toast")
                                .size(px(28.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(IconName::Close).size(px(14.0)))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.dismiss_toast(cx)
                                    }),
                                )
                                .test_support(),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .p(px(4.0))
                    .rounded(px(radius::CARD))
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.line_strong)
                    .child(
                        div()
                            .id("toggle-theme")
                            .h(px(size::ICON_BUTTON))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(px(radius::BUTTON))
                            .text_size(px(text::META))
                            .text_color(p.ink_2)
                            .hover(move |style| style.bg(hover))
                            .child(Icon::new(appearance_icon(appearance)).size(px(16.0)))
                            .child(theme_label)
                            .on_click(cx.listener(|_, _: &ClickEvent, window, cx| {
                                let next = SiftTheme::get(cx).appearance().next();
                                theme::apply(next, Some(window), cx);
                                cx.notify();
                            }))
                            .test_support(),
                    ),
            )
    }

    fn snooze_menu(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let options: Vec<AnyElement> = self
            .snoozing
            .as_ref()
            .map(|(_, options)| options.clone())
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, option)| {
                let hover = p.bg_hover;
                div()
                    .id(("snooze-option", i))
                    .h(px(40.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(7.0))
                    .hover(move |s| s.bg(hover))
                    .child(div().flex_1().child(option.label))
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child(option.when),
                    )
                    .child(kbd(&(i + 1).to_string(), p))
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.choose_snooze(i, cx)),
                    )
                    .test_support()
                    .into_any_element()
            })
            .collect();
        div()
            .id("snooze-scrim")
            .absolute()
            .inset_0()
            .bg(scrim())
            .flex()
            .justify_center()
            .pt(px(120.0))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_snooze(cx)))
            .test_support()
            .child(
                div()
                    .id("snooze-menu")
                    .w(px(360.0))
                    .h_auto()
                    .p(px(6.0))
                    .rounded(px(radius::CARD))
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.line_strong)
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(10.0))
                            .pt(px(8.0))
                            .pb(px(6.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(text::BODY))
                            .child("Snooze until…"),
                    )
                    .children(options),
            )
            .into_any_element()
    }
}

fn section_header(section: DayBucket, p: &Palette) -> AnyElement {
    div()
        .h(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .border_b_1()
        .border_color(p.line)
        .text_size(px(text::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.ink_3)
        .child(section.label().to_uppercase())
        .into_any_element()
}

fn avatar(initials: &str, p: &Palette, diameter: f32) -> impl IntoElement {
    div()
        .size(px(diameter))
        .flex_none()
        .rounded_full()
        .bg(p.accent_soft)
        .text_color(p.accent_text)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px((diameter * 0.42).round()))
        .font_weight(FontWeight::BOLD)
        .child(initials.to_owned())
}

fn chip(view: TriageView, p: &Palette) -> impl IntoElement {
    div()
        .h(px(20.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .rounded(px(radius::CHIP))
        .border_1()
        .border_color(p.line_strong)
        .text_size(px(text::SMALL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.ink_2)
        .child(
            Icon::new(view_icon(view))
                .size(px(12.0))
                .text_color(view_colour(view, p)),
        )
        .child(chip_label(view))
}

fn kbd(key: &str, p: &Palette) -> impl IntoElement {
    div()
        .h(px(18.0))
        .min_w(px(18.0))
        .px(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .border_1()
        .border_color(p.line_strong)
        .text_size(px(text::SMALL))
        .text_color(p.ink_3)
        .child(key.to_owned())
}

fn button(
    id: &'static str,
    icon: IconName,
    label: SharedString,
    p: &Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> impl IntoElement {
    let hover = p.bg_hover;
    div()
        .id(id)
        .h(px(28.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(p.line_strong)
        .text_size(px(text::META))
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
        .child(Icon::new(icon).size(px(14.0)))
        .child(label)
        .on_click(on_click)
        .test_support()
}

fn scrim() -> Hsla {
    hsla(0.0, 0.0, 0.0, 0.28)
}

fn subject_or_placeholder(subject: &str) -> String {
    if subject.trim().is_empty() {
        "(no subject)".to_owned()
    } else {
        subject.to_owned()
    }
}

pub(super) fn chip_label(view: TriageView) -> &'static str {
    match view {
        TriageView::All => "Mail",
        TriageView::NeedsYou => "To-do",
        TriageView::Meetings => "Invite",
        TriageView::Updates => "Update",
        TriageView::Newsletters => "Newsletter",
        TriageView::Other => "Other",
    }
}

fn view_colour(view: TriageView, p: &Palette) -> crate::theme::color::Rgb {
    match view {
        TriageView::NeedsYou => p.warn,
        TriageView::Meetings => p.accent,
        TriageView::Updates => p.ink_3,
        TriageView::Newsletters => p.good,
        TriageView::All | TriageView::Other => p.ink_3,
    }
}

pub(super) fn view_icon(view: TriageView) -> IconName {
    match view {
        TriageView::All => IconName::Inbox,
        TriageView::NeedsYou => IconName::CircleCheck,
        TriageView::Meetings => IconName::Calendar,
        TriageView::Updates => IconName::Bell,
        TriageView::Newsletters => IconName::BookOpen,
        TriageView::Other => IconName::Ellipsis,
    }
}

pub(super) fn appearance_icon(appearance: Appearance) -> IconName {
    match appearance {
        Appearance::Light => IconName::Sun,
        Appearance::Dark => IconName::Moon,
        Appearance::System => IconName::Palette,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_subjects_get_a_placeholder() {
        assert_eq!(subject_or_placeholder("  "), "(no subject)");
        assert_eq!(subject_or_placeholder("Hi"), "Hi");
    }

    #[test]
    fn every_view_has_a_chip_icon_and_colour() {
        let p = crate::theme::tokens::LIGHT;
        let labels: Vec<_> = TriageView::ALL.iter().map(|v| chip_label(*v)).collect();
        assert_eq!(
            labels,
            ["Mail", "To-do", "Invite", "Update", "Newsletter", "Other"]
        );
        for view in TriageView::ALL {
            let _ = (view_icon(view), view_colour(view, &p));
        }
        for appearance in [Appearance::Light, Appearance::Dark, Appearance::System] {
            let _ = appearance_icon(appearance);
        }
        assert!(scrim().a > 0.0);
    }
}
