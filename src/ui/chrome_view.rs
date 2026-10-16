//! How the inbox chrome looks: title-bar controls, the 248 px sidebar, the
//! list's sub-bar (filters or the bulk bar), and the palette, New view,
//! shortcuts, accounts and notifications overlays.

use chrono::Local;
use gpui_kit::component::input::{Input, MoveDown, MoveUp};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, SharedString, Stateful, TestSupportExt as _, div,
    px,
};

use super::app::SiftApp;
use super::chrome::{Overlay, PaletteAction, SHORTCUTS, VIEW_IDEAS};
use super::render::{kbd, scrim, view_icon};
use crate::assets::Lucide;
use crate::domain::TriageView;
use crate::domain::message::initials;
use crate::store::{Account, Attachment, ThreadSummary};
use crate::theme::tokens::{Palette, avatar_tints, radius, size, text};

/// Width of the sidebar, from the Main board.
pub const SIDEBAR_WIDTH: f32 = 248.0;

/// A round avatar in an account's own tint.
pub(super) fn account_avatar(account: &Account, p: &Palette, diameter: f32) -> AnyElement {
    let (bg, ink) = avatar_tints(p)[usize::from(account.colour) % 6];
    div()
        .size(px(diameter))
        .flex_none()
        .rounded_full()
        .bg(bg)
        .text_color(ink)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px((diameter * 0.45).round()))
        .font_weight(FontWeight::BOLD)
        .child(initials(&account.name))
        .into_any_element()
}

/// A 16 px tick box.
pub(super) fn checkbox(
    id: impl Into<gpui_kit::ElementId>,
    checked: bool,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    div()
        .id(id.into())
        .size(px(16.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .border_1()
        .cursor_pointer()
        .when(checked, |c| {
            c.bg(p.accent)
                .border_color(p.accent)
                .text_color(p.accent_ink)
                .child(Icon::new(IconName::Check).size(px(12.0)))
        })
        .when(!checked, |c| c.border_color(p.line_strong).bg(p.bg))
}

fn icon_button(
    id: &'static str,
    icon: impl Into<Icon>,
    on: bool,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    let hover = p.bg_hover;
    div()
        .id(id)
        .size(px(size::ICON_BUTTON))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(radius::BUTTON))
        .text_color(if on { p.accent_text } else { p.ink_2 })
        .when(on, |b| b.bg(p.accent_soft))
        .hover(move |s| s.bg(hover))
        .child(Icon::new(icon).size(px(16.0)))
}

fn icon_button_at(
    id: impl Into<gpui_kit::ElementId>,
    icon: impl Into<Icon>,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    let hover = p.bg_active;
    div()
        .id(id.into())
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .bg(p.bg_hover)
        .text_color(p.ink_2)
        .hover(move |s| s.bg(hover))
        .child(Icon::new(icon).size(px(14.0)))
}

fn small_button(
    id: impl Into<gpui_kit::ElementId>,
    label: SharedString,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    let hover = p.bg_hover;
    div()
        .id(id.into())
        .h(px(28.0))
        .px(px(10.0))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(p.line_strong)
        .bg(p.bg)
        .text_size(px(text::META))
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
        .child(label)
}

fn panel(id: &'static str, width: f32, p: &Palette) -> Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .w(px(width))
        .max_w_full()
        .rounded(px(radius::CARD))
        .bg(p.bg)
        .border_1()
        .border_color(p.line_strong)
        .flex()
        .flex_col()
        .on_click(|_, _, cx| cx.stop_propagation())
}

impl SiftApp {
    fn account_label(&self) -> String {
        self.account
            .and_then(|id| self.accounts.iter().find(|a| a.id == id))
            .map_or_else(|| "All inboxes".to_owned(), |a| a.name.clone())
    }

    /// Search, notifications, the account switcher, sync and New.
    pub(super) fn title_actions(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let hover = p.bg_hover;
        let avatars: Vec<AnyElement> = self
            .accounts
            .iter()
            .take(2)
            .enumerate()
            .map(|(i, a)| {
                div()
                    .when(i > 0, |d| d.ml(px(-6.0)))
                    .rounded_full()
                    .border_2()
                    .border_color(p.bg)
                    .child(account_avatar(a, p, 20.0))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pr(px(8.0))
            .when_some(self.sync_status.clone(), |d, status| {
                d.child(
                    div()
                        .max_w(px(260.0))
                        .truncate()
                        .text_size(px(text::META))
                        .text_color(p.ink_3)
                        .child(status),
                )
            })
            .when(self.sync.is_some(), |d| {
                d.child(
                    icon_button("sync-now", IconName::RefreshCw, false, p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start_sync(cx)))
                        .test_support(),
                )
            })
            .child(
                icon_button("open-palette", IconName::Search, false, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.toggle_palette(window, cx);
                    }))
                    .test_support(),
            )
            .child(
                icon_button("notifications", IconName::Bell, self.hold, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.toggle_overlay("notifications", cx);
                    }))
                    .test_support(),
            )
            .child(
                div()
                    .id("account-switcher")
                    .h(px(size::ICON_BUTTON))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(radius::BUTTON))
                    .hover(move |s| s.bg(hover))
                    .child(div().flex().items_center().children(avatars))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.ink_2)
                            .child(self.account_label()),
                    )
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .size(px(12.0))
                            .text_color(p.ink_3),
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.toggle_overlay("accounts", cx);
                    }))
                    .test_support(),
            )
            .child(
                div()
                    .id("new-message")
                    .h(px(size::ICON_BUTTON))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded_full()
                    .bg(p.primary)
                    .text_color(p.primary_ink)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Icon::new(Lucide::Pencil).size(px(14.0)))
                    .child("New")
                    .child(div().text_size(px(text::SMALL)).opacity(0.8).child("C"))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_compose(window, cx);
                    }))
                    .test_support(),
            )
            .into_any_element()
    }

    /// Custom view tabs and the New view button, after the built-in tabs.
    pub(super) fn custom_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let hover = p.bg_hover;
        let mut tabs: Vec<AnyElement> = self
            .custom_views
            .iter()
            .enumerate()
            .map(|(i, view)| {
                let active = self.custom == Some(view.id);
                let id = view.id;
                div()
                    .id(("custom-view", i))
                    .h(px(size::TAB))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(px(radius::TAB))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if active { p.ink } else { p.ink_2 })
                    .when(active, |t| {
                        t.bg(p.bg_hover).border_1().border_color(p.line_strong)
                    })
                    .when(!active, |t| t.hover(move |s| s.bg(hover)))
                    .child(
                        Icon::new(Lucide::Sparkles)
                            .size(px(14.0))
                            .text_color(p.ink_3),
                    )
                    .child(view.label.clone())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_custom(id, cx);
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        tabs.push(
            div()
                .id("new-view")
                .size(px(size::TAB))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::TAB))
                .text_color(p.ink_3)
                .hover(move |s| s.bg(hover))
                .child(Icon::new(IconName::Plus).size(px(14.0)))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_new_view(window, cx);
                }))
                .test_support()
                .into_any_element(),
        );
        tabs
    }

    /// Accounts, views with counts, and today's agenda.
    pub(super) fn sidebar(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let hover = p.bg_hover;
        let item = |id: gpui_kit::ElementId, active: bool| {
            div()
                .id(id)
                .h(px(32.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(radius::BUTTON))
                .text_color(if active { p.ink } else { p.ink_2 })
                .when(active, |d| d.bg(p.bg_active))
                .when(!active, |d| d.hover(move |s| s.bg(hover)))
        };
        let label = |text: &'static str| {
            div()
                .px(px(8.0))
                .pt(px(12.0))
                .pb(px(4.0))
                .text_size(px(text::SMALL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.ink_3)
                .child(text.to_uppercase())
        };
        let mut accounts: Vec<AnyElement> = vec![
            item("sidebar-all".into(), self.account.is_none())
                .child(
                    div()
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(p.bg_hover)
                        .child(Icon::new(IconName::Inbox).size(px(14.0))),
                )
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::MEDIUM)
                        .child("All inboxes"),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_account(None, cx)))
                .test_support()
                .into_any_element(),
        ];
        for (i, account) in self.accounts.iter().enumerate() {
            let id = account.id;
            accounts.push(
                item(("sidebar-account", i).into(), self.account == Some(id))
                    .h(px(44.0))
                    .child(account_avatar(account, p, 24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(account.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(account.email.to_string()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.set_account(Some(id), cx);
                    }))
                    .test_support()
                    .into_any_element(),
            );
        }
        accounts.push(
            item("sidebar-add-account".into(), false)
                .text_color(p.ink_3)
                .child(
                    div()
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(p.line_strong)
                        .child(Icon::new(IconName::Plus).size(px(12.0))),
                )
                .child("Add account")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_add_account(window, cx);
                }))
                .test_support()
                .into_any_element(),
        );
        let views: Vec<AnyElement> = TriageView::ALL
            .into_iter()
            .enumerate()
            .map(|(i, view)| {
                item(
                    ("sidebar-view", i).into(),
                    self.custom.is_none() && self.view == view,
                )
                .child(
                    Icon::new(view_icon(view))
                        .size(px(14.0))
                        .text_color(p.ink_3),
                )
                .child(div().flex_1().child(view.label()))
                .child(
                    div()
                        .text_size(px(text::META))
                        .text_color(p.ink_3)
                        .child(self.counts.get(view).to_string()),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(view, cx)))
                .test_support()
                .into_any_element()
            })
            .collect();
        div()
            .id("sidebar")
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .p(px(8.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .bg(p.bg_subtle)
            .border_r_1()
            .border_color(p.line)
            .children(accounts)
            .child(label("Views"))
            .children(views)
            .child(
                div()
                    .mt(px(12.0))
                    .mx(px(4.0))
                    .p(px(12.0))
                    .rounded(px(radius::CARD))
                    .border_1()
                    .border_color(p.line)
                    .bg(p.bg)
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Today"))
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(Local::now().format("%a %-d %b").to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child("Nothing on your calendar today."),
                    ),
            )
            .into_any_element()
    }

    /// The bar above the list: title and filters, or the bulk bar while
    /// threads are ticked.
    pub(super) fn subbar(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let all_ticked = !self.threads.is_empty() && self.selected.len() == self.threads.len();
        let select_all = checkbox("select-all", all_ticked, p)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_select_all(cx)))
            .test_support();
        let bar = div()
            .h(px(56.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .pl(px(12.0))
            .child(select_all);
        if !self.selected.is_empty() {
            let n = self.selected.len();
            return bar
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{n} selected")),
                )
                .child(div().flex_1())
                .child(
                    small_button("bulk-done", "Done  E".into(), p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.mark_done(cx)))
                        .test_support(),
                )
                .child(
                    small_button("bulk-snooze", "Snooze  S".into(), p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start_snooze(cx)))
                        .test_support(),
                )
                .child(
                    small_button("bulk-read", "Mark read".into(), p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.bulk_read(cx)))
                        .test_support(),
                )
                .child(
                    small_button("clear-selection", "Clear".into(), p)
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.clear_selection(cx)),
                        )
                        .test_support(),
                )
                .into_any_element();
        }
        let segment = |id: &'static str, label: &'static str, on: bool| {
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
        };
        let group = |a: AnyElement, b: AnyElement| {
            div()
                .flex()
                .flex_none()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(radius::BUTTON))
                .bg(p.bg_hover)
                .child(a)
                .child(b)
        };
        let count = self.threads.len();
        bar.child(
            div()
                .flex()
                .items_baseline()
                .gap(px(10.0))
                .min_w_0()
                .child(
                    div()
                        .text_size(px(text::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(self.list_title()),
                )
                .child(div().flex_none().text_color(p.ink_3).child(format!(
                    "{count} {}",
                    if count == 1 {
                        "conversation"
                    } else {
                        "conversations"
                    }
                ))),
        )
        .child(group(
            segment("filter-all", "All", !self.unread_only)
                .on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.set_unread_only(false, cx)),
                )
                .test_support()
                .into_any_element(),
            segment("filter-unread", "Unread", self.unread_only)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_unread_only(true, cx)))
                .test_support()
                .into_any_element(),
        ))
        .child(div().flex_1())
        .when(self.custom.is_some(), |b| {
            b.child(
                small_button("remove-view", "Remove view".into(), p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.remove_custom(cx)))
                    .test_support(),
            )
        })
        .child(group(
            segment("show-preview", "Preview", !self.summaries)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_summaries(false, cx)))
                .test_support()
                .into_any_element(),
            segment("show-summary", "Summary", self.summaries)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_summaries(true, cx)))
                .test_support()
                .into_any_element(),
        ))
        .child(
            small_button("mark-all-read", "Mark all read".into(), p)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.mark_all_read(cx)))
                .test_support(),
        )
        .into_any_element()
    }

    /// A row's tick box: always shown while anything is ticked, else on hover.
    pub(super) fn row_checkbox(
        &self,
        index: usize,
        thread: &ThreadSummary,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ticked = self.selected.contains(&thread.thread);
        let showing = ticked || !self.selected.is_empty();
        let target = thread.thread.clone();
        checkbox(("select-row", index), ticked, p)
            .when(!showing, |c| {
                c.invisible().group_hover("row", |s| s.visible())
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.toggle_select(target.clone(), cx);
            }))
            .test_support()
            .into_any_element()
    }

    /// Done, Snooze and read toggle, shown over the chip and time on hover.
    pub(super) fn row_actions(
        &self,
        index: usize,
        unread: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let read_icon = if unread {
            Lucide::MailOpen
        } else {
            Lucide::Mail
        };
        div()
            .absolute()
            .right(px(8.0))
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .gap(px(2.0))
            .invisible()
            .group_hover("row", |s| s.visible())
            .child(
                icon_button_at(("row-done", index), IconName::Check, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.mark_row_done(index, cx);
                    }))
                    .test_support(),
            )
            .child(
                icon_button_at(("row-snooze", index), Lucide::Clock, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.snooze_row(index, cx);
                    }))
                    .test_support(),
            )
            .child(
                icon_button_at(("row-read", index), read_icon, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_row_read(index, cx);
                    }))
                    .test_support(),
            )
            .into_any_element()
    }

    /// File chips under a message: type badge, name and size; click to open.
    pub(super) fn file_chips(
        &self,
        files: &[Attachment],
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover = p.bg_hover;
        let chips: Vec<AnyElement> = files
            .iter()
            .map(|file| {
                let path = file.path.clone();
                div()
                    .id(("attachment", usize::try_from(file.id).unwrap_or_default()))
                    .h(px(44.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(radius::BUTTON))
                    .border_1()
                    .border_color(p.line_strong)
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .h(px(24.0))
                            .min_w(px(32.0))
                            .px(px(4.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.0))
                            .bg(p.accent_soft)
                            .text_color(p.accent_text)
                            .text_size(px(text::SMALL))
                            .font_weight(FontWeight::BOLD)
                            .child(file.badge()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(file.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(file.size_label()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_attachment(path.clone(), cx);
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        div()
            .pl(px(44.0))
            .flex()
            .flex_wrap()
            .gap(px(8.0))
            .children(chips)
            .into_any_element()
    }

    /// The open dialog or popover, if any.
    pub(super) fn overlay_layer(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let overlay = self.overlay.as_ref()?;
        let close = cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx));
        let popover = matches!(overlay, Overlay::Accounts | Overlay::Notifications);
        let layer = div()
            .id("overlay-scrim")
            .absolute()
            .inset_0()
            .flex()
            .when(!popover, |l| l.bg(scrim()).justify_center().pt(px(96.0)))
            .when(popover, |l| {
                l.justify_end().items_start().pt(px(4.0)).pr(px(
                    if matches!(overlay, Overlay::Accounts) {
                        96.0
                    } else {
                        300.0
                    },
                ))
            })
            .on_click(close)
            .test_support();
        let content = match overlay {
            Overlay::Shortcuts => self.shortcuts_sheet(p, cx),
            Overlay::Palette(_) => self.palette_panel(p, cx),
            Overlay::NewView(_) => self.new_view_panel(p, cx),
            Overlay::Accounts => self.accounts_menu(p, cx),
            Overlay::Notifications => self.notifications_popover(p, cx),
        };
        Some(layer.child(content).into_any_element())
    }

    fn shortcuts_sheet(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = SHORTCUTS
            .iter()
            .map(|(label, key)| {
                div()
                    .w(px(220.0))
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(div().text_color(p.ink_2).child(*label))
                    .child(kbd(key, p))
                    .into_any_element()
            })
            .collect();
        panel("shortcuts", 520.0, p)
            .p(px(20.0))
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Keyboard shortcuts"),
                    )
                    .child(
                        icon_button("close-shortcuts", IconName::Close, false, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.close_overlay(window, cx)
                            }))
                            .test_support(),
                    ),
            )
            .child(div().flex().flex_wrap().gap_x(px(32.0)).children(rows))
            .into_any_element()
    }

    fn palette_panel(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(Overlay::Palette(state)) = &self.overlay else {
            return div().into_any_element();
        };
        let hover = p.bg_hover;
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_kind = None;
        for (i, item) in state.items.iter().enumerate() {
            let kind = matches!(item.action, PaletteAction::Open(_));
            if last_kind != Some(kind) {
                rows.push(
                    div()
                        .px(px(10.0))
                        .pt(px(10.0))
                        .pb(px(4.0))
                        .text_size(px(text::SMALL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.ink_3)
                        .child(if kind { "MAIL" } else { "COMMANDS" })
                        .into_any_element(),
                );
                last_kind = Some(kind);
            }
            let on = i == state.selected;
            rows.push(
                div()
                    .id(("palette-item", i))
                    .h(px(36.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(7.0))
                    .when(on, |r| r.bg(p.bg_active))
                    .when(!on, |r| r.hover(move |s| s.bg(hover)))
                    .child(
                        Icon::new(if kind {
                            IconName::Inbox
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(14.0))
                        .text_color(p.ink_3),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .gap(px(8.0))
                            .child(div().truncate().child(item.label.clone()))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(p.ink_3)
                                    .child(item.sub.clone()),
                            ),
                    )
                    .when(!item.key.is_empty(), |r| r.child(kbd(item.key, p)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.run_palette(i, window, cx);
                    }))
                    .test_support()
                    .into_any_element(),
            );
        }
        let empty = state.items.is_empty();
        panel("palette", 600.0, p)
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.palette_move(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.palette_move(1, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .h(px(52.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        Icon::new(IconName::Search)
                            .size(px(16.0))
                            .text_color(p.ink_3),
                    )
                    .child(
                        div().flex_1().child(
                            Input::new(&state.input)
                                .appearance(false)
                                .id("palette-input"),
                        ),
                    )
                    .child(kbd("Esc", p)),
            )
            .child(
                div()
                    .id("palette-results")
                    .max_h(px(380.0))
                    .overflow_y_scroll()
                    .p(px(6.0))
                    .children(rows)
                    .when(empty, |d| {
                        d.child(
                            div()
                                .p(px(16.0))
                                .text_color(p.ink_3)
                                .child("No matches. Try a person, a subject or a command."),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(36.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .border_t_1()
                    .border_color(p.line)
                    .bg(p.bg_subtle)
                    .text_size(px(text::META))
                    .text_color(p.ink_3)
                    .child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .child(kbd("↑", p))
                            .child(kbd("↓", p))
                            .child("move"),
                    )
                    .child(div().flex().gap(px(4.0)).child(kbd("↵", p)).child("run")),
            )
            .into_any_element()
    }

    fn new_view_panel(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(Overlay::NewView(form)) = &self.overlay else {
            return div().into_any_element();
        };
        let hover = p.bg_hover;
        let ideas: Vec<AnyElement> = VIEW_IDEAS
            .iter()
            .enumerate()
            .map(|(i, idea)| {
                div()
                    .id(("view-idea", i))
                    .h(px(28.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(p.accent_line)
                    .bg(p.accent_soft)
                    .text_color(p.accent_text)
                    .text_size(px(text::META))
                    .hover(move |s| s.bg(hover))
                    .child(*idea)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.use_view_idea(i, window, cx);
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        panel("new-view-dialog", 440.0, p)
            .p(px(20.0))
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("New view"),
                    )
                    .child(
                        div()
                            .text_color(p.ink_3)
                            .child("Describe what belongs in it. It gets its own tab."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(text::META))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Show me…"),
                    )
                    .child(Input::new(&form.input).id("new-view-input")),
            )
            .child(div().flex().flex_wrap().gap(px(6.0)).children(ideas))
            .when_some(form.error.clone(), |d, e| {
                d.child(div().text_color(p.warn).child(e))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        small_button("cancel-view", "Cancel".into(), p)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.close_overlay(window, cx)
                            }))
                            .test_support(),
                    )
                    .child(
                        small_button("create-view", "Create view".into(), p)
                            .bg(p.primary)
                            .border_color(p.primary)
                            .text_color(p.primary_ink)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.create_view(window, cx)
                            }))
                            .test_support(),
                    ),
            )
            .into_any_element()
    }

    fn accounts_menu(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let hover = p.bg_hover;
        let entry = |id: gpui_kit::ElementId| {
            div()
                .id(id)
                .min_h(px(40.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(7.0))
                .hover(move |s| s.bg(hover))
        };
        let tick = |on: bool| {
            div().w(px(16.0)).when(on, |d| {
                d.child(
                    Icon::new(IconName::Check)
                        .size(px(14.0))
                        .text_color(p.accent_text),
                )
            })
        };
        let mut items: Vec<AnyElement> = vec![
            entry("switch-all".into())
                .child(
                    div()
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(p.bg_hover)
                        .child(Icon::new(IconName::Inbox).size(px(14.0))),
                )
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::MEDIUM)
                        .child("All inboxes"),
                )
                .child(tick(self.account.is_none()))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_account(None, cx)))
                .test_support()
                .into_any_element(),
        ];
        for (i, account) in self.accounts.iter().enumerate() {
            let id = account.id;
            items.push(
                entry(("switch-account", i).into())
                    .child(account_avatar(account, p, 24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(account.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(account.email.to_string()),
                            ),
                    )
                    .child(tick(self.account == Some(id)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.set_account(Some(id), cx);
                    }))
                    .test_support()
                    .into_any_element(),
            );
        }
        panel("accounts-menu", 300.0, p)
            .p(px(6.0))
            .children(items)
            .child(div().my(px(4.0)).h(px(1.0)).bg(p.line))
            .child(
                entry("add-account".into())
                    .child(Icon::new(IconName::Plus).size(px(16.0)).text_color(p.ink_3))
                    .child("Add account…")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.overlay = None;
                        this.open_add_account(window, cx);
                    }))
                    .test_support(),
            )
            .into_any_element()
    }

    fn notifications_popover(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let on = self.hold;
        panel("notifications-popover", 320.0, p)
            .p(px(14.0))
            .child(
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Hold notifications"))
                            .child(div().text_size(px(text::META)).text_color(p.ink_3).child(
                                "Only Needs you can alert you. Everything else waits for your next check-in at 12:00 and 17:00.",
                            )),
                    )
                    .child(
                        div()
                            .id("hold-switch")
                            .w(px(36.0))
                            .h(px(20.0))
                            .flex_none()
                            .p(px(2.0))
                            .flex()
                            .when(on, |s| s.justify_end())
                            .rounded_full()
                            .bg(if on { p.accent } else { p.line_strong })
                            .child(div().size(px(16.0)).rounded_full().bg(p.bg))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_hold(cx)))
                            .test_support(),
                    ),
            )
            .into_any_element()
    }
}
