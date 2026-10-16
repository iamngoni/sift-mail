//! How the Add account flow looks, following the AddAccount board: a 560 px
//! card with a header, "Step n of 4" and a thin progress bar, over the dimmed
//! inbox (or under a welcome line on a first run).

use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, SharedString, Stateful, TestSupportExt as _, div,
    px,
};

use super::add_account::{
    AddAccountFlow, Step, detect, it_request, provider_blurb, range_help, scopes, short_name,
};
use super::app::SiftApp;
use super::chrome_view::checkbox;
use super::render::scrim;
use crate::assets::Lucide;
use crate::domain::Provider;
use crate::domain::message::initials;
use crate::store::{NotifyLevel, SyncRange};
use crate::theme::tokens::{AVATAR_NAMES, Palette, avatar_tints, radius, text};

/// Widest the card gets.
const CARD_WIDTH: f32 = 560.0;

fn pill(
    id: &'static str,
    label: SharedString,
    enabled: bool,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .h(px(40.0))
        .px(px(16.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(p.primary)
        .text_color(p.primary_ink)
        .font_weight(FontWeight::SEMIBOLD)
        .when(!enabled, |b| b.opacity(0.4))
        .child(label)
}

fn button(id: &'static str, label: SharedString, p: &Palette) -> Stateful<gpui_kit::Div> {
    let hover = p.bg_hover;
    div()
        .id(id)
        .h(px(36.0))
        .px(px(12.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(radius::BUTTON))
        .border_1()
        .border_color(p.line_strong)
        .bg(p.bg)
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
        .child(label)
}

fn label(text: &'static str, p: &Palette) -> impl IntoElement {
    div()
        .text_size(px(text::META))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.ink_2)
        .child(text)
}

fn help(text: impl Into<SharedString>, p: &Palette) -> impl IntoElement {
    div()
        .text_size(px(text::META))
        .text_color(p.ink_3)
        .child(text.into())
}

fn tile(provider: Provider, p: &Palette, size: f32) -> AnyElement {
    let icon: gpui_kit::component::Icon = match provider {
        Provider::Microsoft365 => IconName::Building2.into(),
        Provider::Google => IconName::Globe.into(),
        Provider::ICloud => Lucide::Cloud.into(),
        Provider::Imap => Lucide::Mail.into(),
    };
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(10.0))
        .bg(p.bg_hover)
        .text_color(p.ink)
        .child(icon.size(px(size * 0.5)))
        .into_any_element()
}

fn segment(
    id: (&'static str, usize),
    label: &'static str,
    on: bool,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    div()
        .id(id)
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

impl SiftApp {
    /// The Add account flow over the window.
    pub(crate) fn add_account_dialog(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(flow) = self.form.as_ref() else {
            return div().into_any_element();
        };
        let title: SharedString = match flow.step {
            Step::Choose if flow.first_run => "Connect your first account".into(),
            Step::Choose => "Add an account".into(),
            Step::OAuth | Step::Waiting => "Sign in".into(),
            Step::Blocked => "Couldn't connect".into(),
            Step::Imap => flow.provider.display_name().into(),
            Step::Setup => "Set up your account".into(),
            Step::Sync => "Connecting".into(),
        };
        let can_back = matches!(
            flow.step,
            Step::OAuth | Step::Imap | Step::Blocked | Step::Waiting
        );
        let can_close = self.can_close_add_account();
        let hover = p.bg_hover;
        let icon_button = |id: &'static str, icon: IconName| {
            div()
                .id(id)
                .size(px(32.0))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(radius::BUTTON))
                .text_color(p.ink_2)
                .hover(move |s| s.bg(hover))
                .child(Icon::new(icon).size(px(16.0)))
        };
        let body = match flow.step {
            Step::Choose => self.choose_step(flow, p, cx),
            Step::OAuth => self.oauth_step(flow, p, cx),
            Step::Waiting => self.waiting_step(flow, p, cx),
            Step::Blocked => self.blocked_step(flow, p, cx),
            Step::Imap => self.imap_step(flow, p, cx),
            Step::Setup => self.setup_step(flow, p, cx),
            Step::Sync => self.sync_step(flow, p, cx),
        };
        let progress = flow.step.number() as f32 / 4.0;
        let card = div()
            .id("add-account-card")
            .w(px(CARD_WIDTH))
            .flex_none()
            .max_w_full()
            .rounded(px(16.0))
            .border_1()
            .border_color(p.line_strong)
            .bg(p.bg)
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(56.0))
                    .pl(px(16.0))
                    .pr(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b_1()
                    .border_color(p.line)
                    .when(can_back, |h| {
                        h.child(
                            icon_button("flow-back", IconName::ArrowLeft)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.add_account_back(window, cx);
                                }))
                                .test_support(),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child(format!("Step {} of 4", flow.step.number())),
                    )
                    .when(can_close, |h| {
                        h.child(
                            icon_button("flow-close", IconName::Close)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.close_add_account(cx)
                                }))
                                .test_support(),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(2.0))
                    .w_full()
                    .child(div().h_full().w(gpui_kit::relative(progress)).bg(p.accent)),
            )
            .child(div().p(px(24.0)).child(body));
        let privacy = matches!(flow.step, Step::Choose | Step::OAuth);
        div()
            .id("add-account-scrim")
            .absolute()
            .inset_0()
            .when(!flow.first_run, |s| s.bg(scrim()))
            .when(flow.first_run, |s| s.bg(p.bg_subtle))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(if flow.first_run { 48.0 } else { 72.0 }))
            .px(px(16.0))
            .pb(px(32.0))
            .gap(px(16.0))
            .when(can_close, |s| {
                s.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_add_account(cx)))
            })
            .test_support()
            .when(flow.first_run, |s| {
                s.child(
                    div()
                        .max_w(px(CARD_WIDTH))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(text::SMALL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.ink_3)
                                .child("WELCOME"),
                        )
                        .child(
                            div()
                                .text_size(px(28.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("All your mail, calmly in one place"),
                        )
                        .child(div().text_color(p.ink_2).text_center().child(
                            "Connect an account to begin. You can add more at any time, and each stays separate unless you choose All inboxes.",
                        )),
                )
            })
            .child(card)
            .when(privacy, |s| {
                s.child(
                    div()
                        .max_w(px(CARD_WIDTH))
                        .text_size(px(text::META))
                        .text_color(if flow.first_run { p.ink_3 } else { p.bg })
                        .text_center()
                        .child(
                            "Mail is stored on this computer and synced straight from your provider. Sift uses each provider's own sign-in, so you can disconnect from your account settings at any time.",
                        ),
                )
            })
            .into_any_element()
    }

    fn error_line(flow: &AddAccountFlow, p: &Palette) -> Option<AnyElement> {
        flow.error.clone().map(|e| {
            div()
                .id("flow-error")
                .text_size(px(text::META))
                .text_color(p.danger)
                .child(e)
                .into_any_element()
        })
    }

    fn choose_step(
        &self,
        flow: &AddAccountFlow,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let found = detect(&flow.typed_email(cx));
        let hover = p.bg_hover;
        let tiles: Vec<AnyElement> = Provider::ALL
            .into_iter()
            .enumerate()
            .map(|(i, provider)| {
                let (desc, method) = provider_blurb(provider);
                let on = found.as_ref().is_some_and(|f| f.provider == provider);
                div()
                    .id(("provider", i))
                    .min_h(px(60.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .rounded(px(radius::CARD))
                    .border_1()
                    .border_color(if on { p.accent } else { p.line_strong })
                    .when(on, |t| t.bg(p.accent_soft))
                    .when(!on, |t| t.hover(move |s| s.bg(hover)))
                    .child(tile(provider, p, 36.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(provider.display_name()),
                            )
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(desc),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child(method),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.pick_provider(provider, window, cx);
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(label("Your email address", p))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(div().flex_1().child(Input::new(&flow.email).id("flow-email")))
                            .child(
                                pill("continue-email", "Continue".into(), found.is_some(), p)
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.continue_with_email(window, cx);
                                    }))
                                    .test_support(),
                            ),
                    )
                    .child(match &found {
                        Some(f) => div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(px(text::META))
                            .text_color(p.accent_text)
                            .child(Icon::new(IconName::Check).size(px(14.0)))
                            .child(f.text.clone())
                            .into_any_element(),
                        None => help(
                            "We'll work out where your mail lives. Your password never goes through us.",
                            p,
                        )
                        .into_any_element(),
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .text_size(px(text::META))
                    .text_color(p.ink_3)
                    .child(div().flex_1().h(px(1.0)).bg(p.line))
                    .child("or pick a provider")
                    .child(div().flex_1().h(px(1.0)).bg(p.line)),
            )
            .child(div().flex().flex_col().gap(px(8.0)).children(tiles))
            .into_any_element()
    }

    fn email_chip(flow: &AddAccountFlow, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let typed = flow.typed_email(cx);
        let shown = if typed.is_empty() {
            "your account".to_owned()
        } else {
            typed
        };
        div()
            .h(px(36.0))
            .px(px(6.0))
            .pr(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded_full()
            .border_1()
            .border_color(p.line_strong)
            .child(
                div()
                    .size(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(p.accent_soft)
                    .text_color(p.accent_text)
                    .text_size(px(text::SMALL))
                    .font_weight(FontWeight::BOLD)
                    .child(initials(&shown)),
            )
            .child(shown)
            .into_any_element()
    }

    fn oauth_step(&self, flow: &AddAccountFlow, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let short = short_name(flow.provider);
        let rows: Vec<AnyElement> = scopes(flow.provider)
            .into_iter()
            .map(|(title, why)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        Icon::new(Lucide::ShieldCheck)
                            .size(px(16.0))
                            .text_color(p.good),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .child(div().font_weight(FontWeight::MEDIUM).child(title))
                            .child(div().text_color(p.ink_3).child(format!("· {why}"))),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(18.0))
            .child(tile(flow.provider, p, 48.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(div().text_size(px(17.0)).font_weight(FontWeight::SEMIBOLD).child(format!("Sign in with {short}")))
                    .child(div().text_color(p.ink_2).child(format!(
                        "{short}'s sign-in page opens in your browser. Sign in there, approve access, and you'll come straight back here. Sift never sees your password."
                    ))),
            )
            .child(Self::email_chip(flow, p, cx))
            .child(
                div()
                    .w_full()
                    .p(px(14.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .rounded(px(radius::CARD))
                    .bg(p.bg_subtle)
                    .child(
                        div()
                            .text_size(px(text::SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.ink_3)
                            .child(format!("{} WILL ASK YOU TO ALLOW", short.to_uppercase())),
                    )
                    .children(rows),
            )
            .children(Self::error_line(flow, p))
            .child(
                pill("open-sign-in", "Open sign-in page".into(), true, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.start_oauth(window, cx)))
                    .test_support(),
            )
            .into_any_element()
    }

    fn waiting_step(
        &self,
        flow: &AddAccountFlow,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .py(px(12.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.0))
            .child(
                Icon::new(IconName::LoaderCircle)
                    .size(px(28.0))
                    .text_color(p.accent),
            )
            .child(
                div()
                    .text_size(px(17.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Waiting for you to finish in your browser…"),
            )
            .child(div().text_color(p.ink_2).text_center().child(format!(
                "Signing in as {}. This window updates by itself when you're done.",
                flow.typed_email(cx)
            )))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        button(
                            "copy-link",
                            if flow.copied_link {
                                "Link copied".into()
                            } else {
                                "Copy sign-in link".into()
                            },
                            p,
                        )
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.copy_sign_in_link(cx)),
                        )
                        .test_support(),
                    )
                    .child(
                        button("cancel-oauth", "Cancel".into(), p)
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_oauth(cx)),
                            )
                            .test_support(),
                    ),
            )
            .children(Self::error_line(flow, p))
            .child(help(
                "Browser didn't open? Copy the link and paste it into any browser.",
                p,
            ))
            .into_any_element()
    }

    fn blocked_step(
        &self,
        flow: &AddAccountFlow,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let typed = flow.typed_email(cx);
        let domain = typed
            .split_once('@')
            .map_or("your organisation", |(_, d)| d)
            .to_owned();
        div()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .child(
                div()
                    .flex()
                    .gap(px(14.0))
                    .child(
                        div()
                            .size(px(40.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(10.0))
                            .bg(p.warn_soft)
                            .text_color(p.warn)
                            .child(Icon::new(Lucide::Lock).size(px(18.0))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(div().text_size(px(17.0)).font_weight(FontWeight::SEMIBOLD).child("Your organisation needs to approve Sift Mail"))
                            .child(div().text_color(p.ink_2).child(format!(
                                "Microsoft 365 accounts at {domain} can only connect to apps your IT team has approved. Nothing was connected and no mail left your account."
                            ))),
                    ),
            )
            .child(
                div()
                    .rounded(px(radius::CARD))
                    .border_1()
                    .border_color(p.line_strong)
                    .overflow_hidden()
                    .child(
                        div()
                            .h(px(40.0))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .bg(p.bg_subtle)
                            .border_b_1()
                            .border_color(p.line)
                            .child(
                                div()
                                    .text_size(px(text::SMALL))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.ink_3)
                                    .child("MESSAGE FOR YOUR IT TEAM"),
                            )
                            .child(
                                button("copy-it-request", if flow.copied_it { "Copied".into() } else { "Copy".into() }, p)
                                    .h(px(28.0))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_it_request(cx)))
                                    .test_support(),
                            ),
                    )
                    .child(div().p(px(12.0)).text_color(p.ink_2).child(it_request(&domain))),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        button("restart", "Use a different account".into(), p)
                            .h(px(40.0))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.restart_add_account(window, cx)))
                            .test_support(),
                    )
                    .when(!flow.first_run, |d| {
                        d.child(
                            pill("blocked-done", "Done".into(), true, p)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_add_account(cx)))
                                .test_support(),
                        )
                    }),
            )
            .into_any_element()
    }

    fn imap_step(&self, flow: &AddAccountFlow, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let icloud = flow.provider == Provider::ICloud;
        let host = flow.imap_host.read(cx).value().to_string();
        let smtp_port = flow.smtp_port.read(cx).value().to_string();
        let server_fields = |kind: &'static str,
                             host: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
                             port: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
                             security: &'static str,
                             ids: (&'static str, &'static str)| {
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(help(format!("{kind} server"), p))
                        .child(Input::new(host).id(ids.0)),
                )
                .child(
                    div()
                        .w(px(84.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(help("Port", p))
                        .child(Input::new(port).id(ids.1)),
                )
                .child(
                    div()
                        .w(px(96.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(help("Security", p))
                        .child(
                            div()
                                .h(px(32.0))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .rounded(px(radius::BUTTON))
                                .bg(p.bg_subtle)
                                .text_color(p.ink_2)
                                .child(security),
                        ),
                )
        };
        let password_ok =
            !flow.password.read(cx).value().is_empty() && !flow.typed_email(cx).is_empty();
        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .when(icloud, |d| {
                d.child(
                    div()
                        .p(px(12.0))
                        .flex()
                        .gap(px(10.0))
                        .rounded(px(radius::CARD))
                        .bg(p.accent_soft)
                        .text_color(p.accent_text)
                        .child(Icon::new(Lucide::KeyRound).size(px(16.0)))
                        .child(div().flex_1().child(
                            "iCloud needs an app-specific password, not your Apple Account password. Create one at appleid.apple.com under Sign-In and Security.",
                        )),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(label("Email address", p))
                    .child(Input::new(&flow.email).id("imap-email")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(label(if icloud { "App-specific password" } else { "Password or app password" }, p))
                    .child(Input::new(&flow.password).id("imap-password"))
                    .child(help("Stored in the system credential store on this computer, never on our servers.", p)),
            )
            .child(
                div()
                    .rounded(px(radius::CARD))
                    .border_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .id("toggle-servers")
                            .h(px(44.0))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(Icon::new(Lucide::Server).size(px(16.0)).text_color(p.ink_3))
                            .child(div().flex_1().text_color(p.ink_2).child(format!("Server settings found · {host}")))
                            .child(div().text_color(p.accent_text).font_weight(FontWeight::MEDIUM).child(if flow.show_servers { "Hide" } else { "Edit" }))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_servers(cx)))
                            .test_support(),
                    )
                    .when(flow.show_servers, |d| {
                        d.child(
                            div()
                                .p(px(12.0))
                                .pt(px(0.0))
                                .flex()
                                .flex_col()
                                .gap(px(10.0))
                                .child(server_fields("Incoming (IMAP)", &flow.imap_host, &flow.imap_port, "SSL/TLS", ("imap-host", "imap-port")))
                                .child(server_fields(
                                    "Outgoing (SMTP)",
                                    &flow.smtp_host,
                                    &flow.smtp_port,
                                    if smtp_port.trim() == "587" { "STARTTLS" } else { "SSL/TLS" },
                                    ("smtp-host", "smtp-port"),
                                )),
                        )
                    }),
            )
            .children(Self::error_line(flow, p))
            .child(
                div().flex().justify_end().child(
                    pill("connect", if flow.busy { "Checking…".into() } else { "Sign in".into() }, password_ok && !flow.busy, p)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.submit_password(window, cx)))
                        .test_support(),
                ),
            )
            .into_any_element()
    }

    fn setup_step(&self, flow: &AddAccountFlow, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let s = &flow.settings;
        let name = {
            let typed = flow.name.read(cx).value().trim().to_owned();
            if typed.is_empty() {
                flow.account
                    .as_ref()
                    .map_or_else(|| "account".to_owned(), |a| a.name.clone())
            } else {
                typed
            }
        };
        let tints = avatar_tints(p);
        let letter = initials(&name)
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default();
        let colours: Vec<AnyElement> = (0u8..6)
            .map(|c| {
                let (bg, ink) = tints[usize::from(c)];
                let on = s.colour == c;
                div()
                    .id(("setup-colour", usize::from(c)))
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(bg)
                    .text_color(ink)
                    .font_weight(FontWeight::BOLD)
                    .border_2()
                    .border_color(if on { p.ink } else { bg })
                    .child(letter.clone())
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.setup_colour(c, cx)),
                    )
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let password_based = flow
            .account
            .as_ref()
            .is_some_and(|a| matches!(a.provider, Provider::ICloud | Provider::Imap));
        let mut syncs: Vec<AnyElement> = vec![
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(checkbox("setup-mail", true, p))
                .child("Mail")
                .into_any_element(),
        ];
        if !password_based {
            for (i, (label, on)) in [("Calendar", s.calendar), ("Contacts", s.contacts)]
                .into_iter()
                .enumerate()
            {
                syncs.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            checkbox(("setup-sync", i), on, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.setup_change(cx, |s| {
                                        if i == 0 {
                                            s.calendar = !s.calendar;
                                        } else {
                                            s.contacts = !s.contacts;
                                        }
                                    });
                                }))
                                .test_support(),
                        )
                        .child(label)
                        .into_any_element(),
                );
            }
        }
        let ranges: Vec<AnyElement> = SyncRange::ALL
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                segment(("setup-range", i), r.label(), s.range == r, p)
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.setup_range(r, cx)),
                    )
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
            segment(("setup-notify", i), label, s.notify == level, p)
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.setup_notify(level, cx)),
                )
                .test_support()
                .into_any_element()
        })
        .collect();
        let (email, provider) = flow
            .account
            .as_ref()
            .map(|a| (a.email.to_string(), short_name(a.provider)))
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(
                div()
                    .h(px(40.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(radius::BUTTON))
                    .bg(p.good_soft)
                    .text_color(p.good)
                    .child(Icon::new(IconName::CircleCheck).size(px(16.0)))
                    .child(div().flex_1().child(format!("Signed in as {email}")))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(provider)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(label("Name in the sidebar", p))
                            .child(Input::new(&flow.name).id("setup-name")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(label("Colour", p))
                            .child(div().flex().gap(px(6.0)).children(colours))
                            .child(help(AVATAR_NAMES[usize::from(s.colour % 6)], p)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(label("Keep in sync", p))
                    .child(div().flex().gap(px(16.0)).children(syncs)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(label("Download mail from the last", p))
                    .child(segments(ranges, p))
                    .child(help(range_help(s.range), p)),
            )
            .child(
                div()
                    .p(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .rounded(px(radius::CARD))
                    .border_1()
                    .border_color(p.accent_line)
                    .bg(p.accent_soft)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("AI features for this account"))
                            .child(help(
                                "Summaries, reply drafts and Ask your inbox. Turn this off if your employer doesn't allow AI on work mail. You can change it later in Settings.",
                                p,
                            )),
                    )
                    .child(
                        div()
                            .id("setup-ai")
                            .w(px(32.0))
                            .h(px(18.0))
                            .flex_none()
                            .p(px(2.0))
                            .flex()
                            .when(s.ai, |w| w.justify_end())
                            .rounded_full()
                            .bg(if s.ai { p.accent } else { p.line_strong })
                            .child(div().size(px(14.0)).rounded_full().bg(p.bg))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.setup_change(cx, |s| s.ai = !s.ai)))
                            .test_support(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(label("Notify me about", p))
                    .child(segments(notifies, p)),
            )
            .children(Self::error_line(flow, p))
            .child(
                div().flex().justify_end().child(
                    pill("setup-connect", format!("Connect {name}").into(), true, p)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.finish_setup(window, cx)))
                        .test_support(),
                ),
            )
            .into_any_element()
    }

    fn sync_step(&self, flow: &AddAccountFlow, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(account) = &flow.account else {
            return div().into_any_element();
        };
        let done = flow.sync_result.is_some();
        let failed = matches!(flow.sync_result, Some(Err(_)));
        let (bg, ink) = avatar_tints(p)[usize::from(flow.settings.colour % 6)];
        let job = |title: String, status: String, fraction: f32, done: bool| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .when(done, |d| {
                            d.child(Icon::new(IconName::Check).size(px(14.0)).text_color(p.good))
                        })
                        .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(title))
                        .child(
                            div()
                                .text_size(px(text::META))
                                .text_color(p.ink_3)
                                .child(status),
                        ),
                )
                .child(
                    div()
                        .h(px(4.0))
                        .w_full()
                        .rounded_full()
                        .bg(p.bg_hover)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .w(gpui_kit::relative(fraction))
                                .bg(if done { p.good } else { p.accent }),
                        ),
                )
                .into_any_element()
        };
        let count = flow.synced;
        let mail_status = match (&flow.sync_result, count) {
            (Some(Ok(())), 1) => "1 message".to_owned(),
            (Some(Ok(())), n) => format!("{n} messages"),
            (Some(Err(_)), _) => "Didn't finish".to_owned(),
            (None, 0) => "Working…".to_owned(),
            (None, n) => format!("{n} so far"),
        };
        let mail_fraction = if done {
            1.0
        } else {
            // Unknown total: ease towards 90% as messages arrive.
            0.9 * (1.0 - (-(count as f32) / 400.0).exp()).max(0.05)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .size(px(44.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(bg)
                            .text_color(ink)
                            .font_weight(FontWeight::BOLD)
                            .child(initials(&account.name)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if done && !failed {
                                        format!("{} is ready", account.name)
                                    } else {
                                        format!("Connecting {}", account.name)
                                    }),
                            )
                            .child(help(
                                format!("{} · {}", account.email, short_name(account.provider)),
                                p,
                            )),
                    ),
            )
            .child(job("Folders and labels".into(), "Done".into(), 1.0, true))
            .child(job(
                format!(
                    "Mail from the last {}",
                    flow.settings.range.label().to_lowercase()
                ),
                mail_status,
                mail_fraction,
                done && !failed,
            ))
            .child(help(
                match &flow.sync_result {
                    Some(Err(why)) => format!("{why}. Sift will try again in the background."),
                    Some(Ok(())) => {
                        "Everything is here. New mail arrives every few minutes.".to_owned()
                    }
                    None => {
                        "You can start using your inbox now. Mail keeps arriving in the background."
                            .to_owned()
                    }
                },
                p,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        button("add-another", "Add another account".into(), p)
                            .h(px(40.0))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.add_another_account(window, cx)
                            }))
                            .test_support(),
                    )
                    .child(
                        pill("go-to-inbox", "Go to inbox".into(), true, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.go_to_inbox(window, cx)
                            }))
                            .test_support(),
                    ),
            )
            .into_any_element()
    }
}
