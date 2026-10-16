//! How Settings looks, following the Accounts board: a 220 px page list, a
//! 760 px column of account cards that expand into labelled rows, and a
//! confirmation before removing an account.

use chrono::{Local, Utc};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, SharedString, TestSupportExt as _, div, px,
};

use super::app::SiftApp;
use super::chrome::SHORTCUTS;
use super::chrome_view::{account_avatar, checkbox, small_button};
use super::render::{kbd, scrim};
use super::settings::{SettingsPage, provider_name};
use crate::domain::time::{DayBucket, bucket};
use crate::domain::{AccountId, Provider};
use crate::store::{Account, NotifyLevel, SyncRange};
use crate::theme::tokens::{AVATAR_NAMES, Palette, avatar_tints, radius, text};
use crate::theme::{Appearance, SiftTheme};

/// Width of the Settings page list.
const NAV_WIDTH: f32 = 220.0;
/// Widest the Settings column gets.
const COLUMN_WIDTH: f32 = 760.0;

/// "2 min ago", "3 days ago": how long since a sync.
pub fn ago(then: chrono::DateTime<Utc>, now: chrono::DateTime<Utc>) -> String {
    let minutes = (now - then).num_minutes().max(0);
    match minutes {
        0 => "just now".to_owned(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => match minutes / 1440 {
            1 => "1 day ago".to_owned(),
            days => format!("{days} days ago"),
        },
    }
}

fn segment(
    id: impl Into<gpui_kit::ElementId>,
    label: &'static str,
    on: bool,
    p: &Palette,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id.into())
        .h(px(26.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .rounded(px(6.0))
        .text_size(px(text::META))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if on { p.ink } else { p.ink_3 })
        .when(on, |s| s.bg(p.bg).border_1().border_color(p.line))
        .child(label)
}

fn segments(children: Vec<AnyElement>, p: &Palette) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .p(px(2.0))
        .gap(px(2.0))
        .rounded(px(radius::BUTTON))
        .bg(p.bg_hover)
        .children(children)
        .into_any_element()
}

fn row(
    title: &'static str,
    help: Option<SharedString>,
    value: AnyElement,
    p: &Palette,
) -> AnyElement {
    div()
        .py(px(16.0))
        .flex()
        .items_start()
        .gap(px(24.0))
        .border_t_1()
        .border_color(p.line)
        .child(
            div()
                .w(px(180.0))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .when_some(help, |d, h| {
                    d.child(div().text_size(px(text::META)).text_color(p.ink_3).child(h))
                }),
        )
        .child(div().flex_1().min_w_0().child(value))
        .into_any_element()
}

fn switch(
    id: impl Into<gpui_kit::ElementId>,
    on: bool,
    p: &Palette,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id.into())
        .w(px(32.0))
        .h(px(18.0))
        .flex_none()
        .p(px(2.0))
        .flex()
        .when(on, |s| s.justify_end())
        .rounded_full()
        .bg(if on { p.accent } else { p.line_strong })
        .child(div().size(px(14.0)).rounded_full().bg(p.bg))
}

impl SiftApp {
    /// The whole Settings screen below the title bar.
    pub(super) fn settings_screen(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = &self.settings else {
            return div().into_any_element();
        };
        let hover = p.bg_hover;
        let nav: Vec<AnyElement> = SettingsPage::ALL
            .into_iter()
            .enumerate()
            .map(|(i, page)| {
                let on = state.page == page;
                div()
                    .id(("settings-nav", i))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(7.0))
                    .text_color(if on { p.ink } else { p.ink_2 })
                    .when(on, |n| {
                        n.bg(p.bg)
                            .font_weight(FontWeight::SEMIBOLD)
                            .border_1()
                            .border_color(p.line_strong)
                    })
                    .when(!on, |n| n.hover(move |s| s.bg(hover)))
                    .child(page.label())
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.settings_go(page, cx)),
                    )
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let body = match state.page {
            SettingsPage::Accounts => self.accounts_page(p, cx),
            SettingsPage::General => self.general_page(p, cx),
            SettingsPage::Ai => self.simple_page(
                "AI",
                "Summaries, reply drafts and Ask your inbox run through the assistant you already use: Claude Code or Codex, signed in with your own subscription. Sift never asks for an API key.",
                p,
            ),
            SettingsPage::Notifications => self.notifications_page(p, cx),
            SettingsPage::Appearance => self.appearance_page(p, cx),
            SettingsPage::Keyboard => self.keyboard_page(p),
            SettingsPage::Privacy => self.simple_page(
                "Privacy",
                "Your mail, attachments and settings live in a database on this computer. Passwords and sign-in tokens are kept in the system credential store. Nothing is sent anywhere except your own mail servers, and the AI assistant for accounts where AI is on.",
                p,
            ),
        };
        div()
            .id("settings")
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .w(px(NAV_WIDTH))
                    .flex_none()
                    .p(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .bg(p.bg_subtle)
                    .border_r_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .px(px(10.0))
                            .pb(px(8.0))
                            .text_size(px(text::SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.ink_3)
                            .child("SETTINGS"),
                    )
                    .children(nav),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .test_support()
                    .flex()
                    .justify_center()
                    .items_start()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(COLUMN_WIDTH))
                            .px(px(32.0))
                            .py(px(32.0))
                            .flex()
                            .flex_col()
                            .gap(px(20.0))
                            .child(body)
                            .child(div().h(px(64.0))),
                    ),
            )
            .when_some(state.notice.clone(), |s, notice| {
                s.child(
                    div()
                        .absolute()
                        .bottom(px(16.0))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("settings-notice")
                                .h(px(40.0))
                                .px(px(14.0))
                                .flex()
                                .items_center()
                                .rounded(px(radius::TOAST))
                                .bg(p.primary)
                                .text_color(p.primary_ink)
                                .child(notice),
                        ),
                )
            })
            .when(state.confirm_remove.is_some(), |s| {
                s.child(self.remove_dialog(p, cx))
            })
            .into_any_element()
    }

    fn page_title(title: &'static str, sub: Option<String>, p: &Palette) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_size(px(text::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .when_some(sub, |d, s| d.child(div().text_color(p.ink_3).child(s)))
            .into_any_element()
    }

    fn simple_page(&self, title: &'static str, body: &'static str, p: &Palette) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(Self::page_title(title, None, p))
            .child(
                div()
                    .text_color(p.ink_2)
                    .text_size(px(text::BODY))
                    .child(body),
            )
            .into_any_element()
    }

    fn keyboard_page(&self, p: &Palette) -> AnyElement {
        let rows: Vec<AnyElement> = SHORTCUTS
            .iter()
            .map(|(label, key)| {
                div()
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(p.line)
                    .child(div().text_color(p.ink_2).child(*label))
                    .child(kbd(key, p))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(Self::page_title(
                "Keyboard",
                Some("Press ? anywhere to see these.".into()),
                p,
            ))
            .child(div().flex().flex_col().children(rows))
            .into_any_element()
    }

    fn appearance_page(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let current = SiftTheme::get(cx).appearance();
        let themes: Vec<AnyElement> = [
            ("theme-light", "Light", Appearance::Light),
            ("theme-dark", "Dark", Appearance::Dark),
            ("theme-system", "Match system", Appearance::System),
        ]
        .into_iter()
        .map(|(id, label, appearance)| {
            segment(id, label, current == appearance, p)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.set_appearance(appearance, window, cx);
                }))
                .test_support()
                .into_any_element()
        })
        .collect();
        let density = vec![
            segment("density-comfortable", "Comfortable", !self.compact, p)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_compact(false, cx)))
                .test_support()
                .into_any_element(),
            segment("density-compact", "Compact", self.compact, p)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_compact(true, cx)))
                .test_support()
                .into_any_element(),
        ];
        div()
            .flex()
            .flex_col()
            .child(Self::page_title("Appearance", None, p))
            .child(div().h(px(16.0)))
            .child(row("Theme", None, segments(themes, p), p))
            .child(row(
                "Density",
                Some("Compact fits more rows on screen.".into()),
                segments(density, p),
                p,
            ))
            .into_any_element()
    }

    fn notifications_page(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .child(Self::page_title("Notifications", None, p))
            .child(div().h(px(16.0)))
            .child(row(
                "Hold notifications",
                Some("Only Needs you can alert you. Everything else waits for your next check-in at 12:00 and 17:00.".into()),
                switch("settings-hold", self.hold, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_hold(cx)))
                    .test_support()
                    .into_any_element(),
                p,
            ))
            .child(row(
                "Per account",
                Some("Choose what each account may notify you about under Accounts.".into()),
                small_button("notifications-accounts", "Open Accounts".into(), p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.settings_go(SettingsPage::Accounts, cx);
                    }))
                    .test_support()
                    .into_any_element(),
                p,
            ))
            .into_any_element()
    }

    fn general_page(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .child(Self::page_title("General", None, p))
            .child(div().h(px(16.0)))
            .child(self.default_from_row(p, cx))
            .into_any_element()
    }

    fn default_from_row(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.settings.as_ref().is_some_and(|s| s.from_menu);
        let current = self.default_from();
        let label = |a: &Account| format!("{} · {}", a.name, a.email);
        let selected = self
            .accounts
            .iter()
            .find(|a| Some(a.id) == current)
            .map_or_else(|| "No accounts yet".to_owned(), label);
        let hover = p.bg_hover;
        let options: Vec<AnyElement> =
            self.accounts
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let id = a.id;
                    div()
                        .id(("from-option", i))
                        .h(px(36.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .rounded(px(7.0))
                        .hover(move |s| s.bg(hover))
                        .child(account_avatar(a, p, 20.0))
                        .child(div().flex_1().child(label(a)))
                        .when(Some(id) == current, |d| {
                            d.child(
                                Icon::new(IconName::Check)
                                    .size(px(14.0))
                                    .text_color(p.accent_text),
                            )
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_default_from(id, cx)
                        }))
                        .test_support()
                        .into_any_element()
                })
                .collect();
        row(
            "Send new mail from",
            Some("Replies always go from the address the mail was sent to.".into()),
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .id("default-from")
                        .h(px(36.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded(px(radius::BUTTON))
                        .border_1()
                        .border_color(p.line_strong)
                        .bg(p.bg)
                        .child(selected)
                        .child(
                            Icon::new(IconName::ChevronDown)
                                .size(px(14.0))
                                .text_color(p.ink_3),
                        )
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_from_menu(cx)),
                        )
                        .test_support(),
                )
                .when(open, |d| {
                    d.child(
                        div()
                            .p(px(4.0))
                            .rounded(px(radius::BUTTON))
                            .border_1()
                            .border_color(p.line_strong)
                            .bg(p.bg)
                            .children(options),
                    )
                })
                .into_any_element(),
            p,
        )
    }

    fn accounts_page(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let count = self.accounts.len();
        let now = Utc::now();
        let problem = self.accounts.iter().find_map(|a| {
            let health = self.store.account_health(a.id).ok()?;
            health.needs_sign_in.then(|| {
                let when = health
                    .last_sync
                    .map_or_else(|| "a while ago".to_owned(), |t| ago(t, now));
                (a.id, format!(
                    "{} stopped syncing {when}. Its password was changed or access was revoked.",
                    a.name
                ))
            })
        });
        let cards: Vec<AnyElement> = self
            .accounts
            .iter()
            .enumerate()
            .map(|(i, a)| self.account_card(i, a, count, p, cx))
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Self::page_title(
                        "Accounts",
                        Some(format!(
                            "{count} {} connected · order here sets the order in the sidebar",
                            if count == 1 { "account" } else { "accounts" }
                        )),
                        p,
                    ))
                    .child(
                        div()
                            .id("settings-add-account")
                            .h(px(32.0))
                            .px(px(14.0))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .bg(p.primary)
                            .text_color(p.primary_ink)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Add account")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.open_add_account(window, cx);
                            }))
                            .test_support(),
                    ),
            )
            .when_some(problem, |d, (id, text)| {
                d.child(
                    div()
                        .px(px(14.0))
                        .py(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .rounded(px(radius::CARD))
                        .border_1()
                        .border_color(p.warn_line)
                        .bg(p.warn_soft)
                        .child(div().flex_1().text_color(p.warn).child(text))
                        .child(
                            small_button("fix-problem", "Sign in again".into(), p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.sign_in_again(id, window, cx);
                                }))
                                .test_support(),
                        ),
                )
            })
            .child(self.default_from_row(p, cx))
            .children(cards)
            .when(count == 0, |d| {
                d.child(
                    div()
                        .text_color(p.ink_3)
                        .child("No accounts yet. Add one to start."),
                )
            })
            .into_any_element()
    }

    fn status_pill(&self, account: &Account, busy: bool, p: &Palette) -> AnyElement {
        let health = self.store.account_health(account.id).unwrap_or_default();
        let now = Utc::now();
        let synced = health.last_sync.map(|t| match bucket(t, now, &Local) {
            DayBucket::Today => format!("synced {}", ago(t, now)),
            _ => format!("last synced {}", ago(t, now)),
        });
        let (text, bg, ink) = if busy {
            ("Connecting…".to_owned(), p.accent_soft, p.accent_text)
        } else if health.needs_sign_in {
            (
                match synced {
                    Some(s) => format!("Needs sign-in · {s}"),
                    None => "Needs sign-in".to_owned(),
                },
                p.warn_soft,
                p.warn,
            )
        } else if let Some(error) = health.last_error {
            (format!("Sync failed · {error}"), p.warn_soft, p.warn)
        } else {
            (
                match synced {
                    Some(s) => format!("Connected · {s}"),
                    None => "Connected".to_owned(),
                },
                p.good_soft,
                p.good,
            )
        };
        div()
            .h(px(22.0))
            .max_w(px(260.0))
            .px(px(8.0))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .rounded_full()
            .bg(bg)
            .text_color(ink)
            .text_size(px(text::SMALL))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().size(px(6.0)).flex_none().rounded_full().bg(ink))
            .child(div().truncate().child(text))
            .into_any_element()
    }

    fn account_card(
        &self,
        index: usize,
        account: &Account,
        count: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = &self.settings else {
            return div().into_any_element();
        };
        let id = account.id;
        let open = state.editor.as_ref().is_some_and(|e| e.account == id);
        let busy = state.busy.contains(&id);
        let hover = p.bg_hover;
        let is_default = self.default_from() == Some(id);
        let mover = |name: &'static str,
                     icon: IconName,
                     delta: i64,
                     enabled: bool,
                     cx: &mut Context<Self>| {
            div()
                .id((name, index))
                .size(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.0))
                .text_color(if enabled { p.ink_2 } else { p.line_strong })
                .when(enabled, |b| {
                    b.hover(move |s| s.bg(hover)).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.move_account(id, delta, cx);
                        },
                    ))
                })
                .child(Icon::new(icon).size(px(14.0)))
                .test_support()
        };
        let header = div()
            .flex()
            .items_center()
            .child(
                div()
                    .pl(px(6.0))
                    .flex()
                    .flex_col()
                    .child(mover("move-up", IconName::ChevronUp, -1, index > 0, cx))
                    .child(mover(
                        "move-down",
                        IconName::ChevronDown,
                        1,
                        index + 1 < count,
                        cx,
                    )),
            )
            .child(
                div()
                    .id(("account-card", index))
                    .flex_1()
                    .min_w_0()
                    .min_h(px(64.0))
                    .pl(px(8.0))
                    .pr(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .hover(move |s| s.bg(hover))
                    .child(account_avatar(account, p, 36.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(account.name.clone()),
                                    )
                                    .when(is_default, |d| {
                                        d.child(
                                            div()
                                                .h(px(20.0))
                                                .px(px(8.0))
                                                .flex()
                                                .items_center()
                                                .rounded_full()
                                                .bg(p.bg_hover)
                                                .text_color(p.ink_2)
                                                .text_size(px(text::SMALL))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Default"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(format!(
                                        "{} · {}",
                                        account.email,
                                        provider_name(account.provider)
                                    )),
                            ),
                    )
                    .child(self.status_pill(account, busy, p))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.toggle_account(id, window, cx);
                    }))
                    .test_support(),
            );
        div()
            .rounded(px(radius::CARD))
            .border_1()
            .border_color(if open { p.accent_line } else { p.line_strong })
            .bg(p.bg)
            .overflow_hidden()
            .child(header)
            .when(open, |card| {
                card.child(self.account_rows(account, busy, p, cx))
            })
            .into_any_element()
    }

    fn account_rows(
        &self,
        account: &Account,
        busy: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(editor) = self.settings.as_ref().and_then(|s| s.editor.as_ref()) else {
            return div().into_any_element();
        };
        let id = account.id;
        let settings = self.store.account_settings(id).unwrap_or_default();
        let tints = avatar_tints(p);
        let swatches: Vec<AnyElement> = (0u8..6)
            .map(|c| {
                let (bg, ink) = tints[usize::from(c)];
                let on = settings.colour == c;
                div()
                    .id(("swatch", usize::from(c)))
                    .size(px(24.0))
                    .rounded_full()
                    .bg(bg)
                    .border_2()
                    .border_color(if on { ink } else { bg })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.set_account_colour(id, c, cx)
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let mut aliases: Vec<AnyElement> = vec![
            div()
                .h(px(36.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(radius::BUTTON))
                .border_1()
                .border_color(p.line)
                .child(div().flex_1().child(account.email.to_string()))
                .child(
                    div()
                        .text_size(px(text::META))
                        .text_color(p.ink_3)
                        .child("Primary"),
                )
                .into_any_element(),
        ];
        for (i, alias) in self
            .store
            .aliases(id)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let address = alias.clone();
            aliases.push(
                div()
                    .h(px(36.0))
                    .pl(px(10.0))
                    .pr(px(4.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(radius::BUTTON))
                    .border_1()
                    .border_color(p.line)
                    .child(div().flex_1().child(alias.to_string()))
                    .child(
                        div()
                            .id(("remove-alias", i))
                            .size(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.0))
                            .text_color(p.ink_3)
                            .child(Icon::new(IconName::Close).size(px(14.0)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove_alias(id, address.clone(), cx);
                            }))
                            .test_support(),
                    )
                    .into_any_element(),
            );
        }
        let syncs = {
            let mut checks: Vec<AnyElement> = vec![
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(checkbox("sync-mail", true, p))
                    .child("Mail")
                    .into_any_element(),
            ];
            if account.provider != Provider::Imap && account.provider != Provider::ICloud {
                for (key, label, on) in [
                    ("calendar", "Calendar", settings.calendar),
                    ("contacts", "Contacts", settings.contacts),
                ] {
                    checks.push(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                checkbox(("sync-toggle", usize::from(key == "contacts")), on, p)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.toggle_account_sync(id, key, cx);
                                    }))
                                    .test_support(),
                            )
                            .child(label)
                            .into_any_element(),
                    );
                }
            }
            checks
        };
        let ranges: Vec<AnyElement> = SyncRange::ALL
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                segment(("range", i), r.label(), settings.range == r, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.set_account_range(id, r, cx)
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let notifies: Vec<AnyElement> = [
            (NotifyLevel::NeedsYou, "Needs you only"),
            (NotifyLevel::Everything, "Everything"),
            (NotifyLevel::Nothing, "Nothing"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (level, label))| {
            segment(("notify", i), label, settings.notify == level, p)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_account_notify(id, level, cx)
                }))
                .test_support()
                .into_any_element()
        })
        .collect();
        let provider = provider_name(account.provider);
        let password_based = matches!(account.provider, Provider::Imap | Provider::ICloud);
        div()
            .px(px(20.0))
            .pb(px(4.0))
            .flex()
            .flex_col()
            .child(row(
                "Name and colour",
                Some("Shown in the sidebar and account switcher.".into()),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(Input::new(&editor.name).id("account-name"))
                    .child(div().flex().gap(px(10.0)).children(swatches))
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child(AVATAR_NAMES[usize::from(settings.colour % 6)]),
                    )
                    .into_any_element(),
                p,
            ))
            .child(row(
                "Send as",
                Some("Addresses you can choose in the From field.".into()),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(aliases)
                    .child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(div().flex_1().child(Input::new(&editor.alias).id("alias-input")))
                            .child(
                                small_button("add-alias", "Add".into(), p)
                                    .h(px(36.0))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.add_alias(window, cx)))
                                    .test_support(),
                            ),
                    )
                    .when_some(editor.alias_error.clone(), |d, e| d.child(div().text_color(p.warn).child(e)))
                    .when(account.provider == Provider::Microsoft365, |d| {
                        d.child(
                            div()
                                .text_size(px(text::META))
                                .text_color(p.ink_3)
                                .child("Microsoft 365 only sends from addresses your admin has given you \"Send as\" rights to."),
                        )
                    })
                    .into_any_element(),
                p,
            ))
            .child(row(
                "Signature",
                Some("Added to new mail and replies from this account.".into()),
                Textarea::new(&editor.signature).into_any_element(),
                p,
            ))
            .child(row(
                "Sync",
                Some(if password_based {
                    "IMAP accounts sync mail only.".into()
                } else {
                    "Turning something off removes its local copy.".into()
                }),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(div().flex().gap(px(16.0)).children(syncs))
                    .child(segments(ranges, p))
                    .into_any_element(),
                p,
            ))
            .child(row(
                "AI features",
                Some("Summaries, drafts and Ask your inbox.".into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        switch("account-ai", settings.ai, p)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_account_ai(id, cx)))
                            .test_support(),
                    )
                    .child(div().text_color(p.ink_2).child(if settings.ai {
                        "On. Mail from this account can be summarised and drafted."
                    } else {
                        "Off. Nothing from this account is sent to AI."
                    }))
                    .into_any_element(),
                p,
            ))
            .child(row(
                "Notifications",
                None,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(segments(notifies, p))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                checkbox("in-all", settings.in_all, p)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_account_in_all(id, cx)))
                                    .test_support(),
                            )
                            .child("Include in All inboxes"),
                    )
                    .into_any_element(),
                p,
            ))
            .child(row(
                "Connection",
                Some(if password_based {
                    "Password stored in the system credential store.".into()
                } else {
                    format!("Signed in through {provider}. You can also revoke access from your {provider} account page.").into()
                }),
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(
                        small_button("sync-account", if busy { "Syncing…".into() } else { "Sync now".into() }, p)
                            .h(px(32.0))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.sync_account_now(id, cx)))
                            .test_support(),
                    )
                    .child(
                        small_button("reauth-account", "Sign in again".into(), p)
                            .h(px(32.0))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.sign_in_again(id, window, cx);
                            }))
                            .test_support(),
                    )
                    .child(div().flex_1())
                    .child(
                        small_button("remove-account", "Remove account…".into(), p)
                            .h(px(32.0))
                            .text_color(p.danger)
                            .border_color(p.danger_line)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.ask_remove_account(id, cx)))
                            .test_support(),
                    )
                    .into_any_element(),
                p,
            ))
            .into_any_element()
    }

    fn remove_dialog(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let account: Option<AccountId> = self.settings.as_ref().and_then(|s| s.confirm_remove);
        let Some(account) = account.and_then(|id| self.accounts.iter().find(|a| a.id == id)) else {
            return div().into_any_element();
        };
        let provider = match account.provider {
            Provider::Imap => "your mail provider",
            other => provider_name(other),
        };
        div()
            .id("remove-scrim")
            .absolute()
            .inset_0()
            .bg(scrim())
            .flex()
            .justify_center()
            .pt(px(140.0))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_remove_account(cx)))
            .test_support()
            .child(
                div()
                    .id("remove-dialog")
                    .w(px(440.0))
                    .h_auto()
                    .p(px(20.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .rounded(px(radius::CARD))
                    .border_1()
                    .border_color(p.line_strong)
                    .bg(p.bg)
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Remove {} from this computer?", account.name)),
                    )
                    .child(div().text_color(p.ink_2).child(format!(
                        "Your mail stays safe in {provider}. This only removes what is stored here:"
                    )))
                    .child(
                        div()
                            .pl(px(8.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .text_color(p.ink_2)
                            .child("• Downloaded mail and attachments")
                            .child("• This account's settings, signature and send-as addresses")
                            .child("• Its saved password or sign-in"),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(
                                small_button("cancel-remove", "Cancel".into(), p)
                                    .h(px(32.0))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.cancel_remove_account(cx)
                                    }))
                                    .test_support(),
                            )
                            .child(
                                small_button(
                                    "confirm-remove",
                                    format!("Remove {}", account.name).into(),
                                    p,
                                )
                                .h(px(32.0))
                                .bg(p.danger)
                                .border_color(p.danger)
                                .text_color(p.bg)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.confirm_remove_account(cx)
                                }))
                                .test_support(),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_reads_naturally() {
        let now = Utc::now();
        let mins = |m| now - chrono::Duration::minutes(m);
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(mins(2), now), "2 min ago");
        assert_eq!(ago(mins(125), now), "2 h ago");
        assert_eq!(ago(mins(1500), now), "1 day ago");
        assert_eq!(ago(mins(3 * 1440), now), "3 days ago");
        assert_eq!(ago(now + chrono::Duration::minutes(5), now), "just now");
    }
}
