//! How onboarding looks, following the Welcome board: a split screen with the
//! words on the left and a live picture of each choice on the right ("the
//! stage"), progress dots above, and Back / Skip / Next below.
//!
//! The stage moves like the board: mail falls through the sift into bins
//! that bob, each stage glides in, and Focus pings and the final badges pop in
//! one after another. GPUI holds every animation still when the system asks
//! for reduced motion, which is also what the board does.

use std::time::Duration;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, ClickEvent, Context, ElementId, FontWeight,
    SharedString, Stateful, TestSupportExt as _, div, ease_in_out, ease_out_quint,
    pulsating_between, px,
};

use super::add_account::short_name;
use super::app::SiftApp;
use super::onboarding::{OnboardStep, PRACTICE_HINTS, PRACTICE_ROWS, PRACTICE_TASKS};
use super::render::{chip_label, kbd, view_icon};
use crate::assets::Lucide;
use crate::domain::Provider;
use crate::domain::TriageView;
use crate::domain::choices::{AiFeature, CHECKIN_TIMES, NotifyMode, OPTIONAL_VIEWS, SortMode};
use crate::domain::message::initials;
use crate::theme::tokens::{DARK, LIGHT, Palette, avatar_tints, radius, text};
use crate::theme::{Appearance, SiftTheme};

/// Widest the onboarding content gets.
const CONTENT_WIDTH: f32 = 1120.0;

fn pill(
    id: &'static str,
    label: SharedString,
    enabled: bool,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .h(px(40.0))
        .px(px(18.0))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .rounded_full()
        .bg(p.primary)
        .text_color(p.primary_ink)
        .font_weight(FontWeight::SEMIBOLD)
        .when(!enabled, |b| b.opacity(0.45))
        .child(label)
        .child(
            div()
                .h(px(18.0))
                .px(px(5.0))
                .flex()
                .items_center()
                .rounded(px(5.0))
                .border_1()
                .border_color(p.primary_ink)
                .text_size(px(text::SMALL))
                .opacity(0.7)
                .child("↵"),
        )
}

fn ghost(id: &'static str, label: &'static str, p: &Palette) -> Stateful<gpui_kit::Div> {
    let hover = p.bg_hover;
    div()
        .id(id)
        .h(px(36.0))
        .px(px(12.0))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(radius::BUTTON))
        .text_color(p.ink_2)
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
        .child(label)
}

/// A choice tile: icon square, title and explanation; accent when chosen.
fn option(
    id: (&'static str, usize),
    icon: gpui_kit::component::Icon,
    title: &'static str,
    body: &'static str,
    on: bool,
    p: &Palette,
) -> Stateful<gpui_kit::Div> {
    let hover = p.bg_hover;
    div()
        .id(id)
        .min_h(px(64.0))
        .p(px(12.0))
        .flex()
        .items_center()
        .gap(px(14.0))
        .rounded(px(radius::CARD))
        .border_1()
        .border_color(if on { p.accent } else { p.line_strong })
        .when(on, |o| o.bg(p.accent_soft))
        .when(!on, |o| o.bg(p.bg).hover(move |s| s.bg(hover)))
        .child(
            div()
                .size(px(36.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(10.0))
                .bg(if on { p.accent } else { p.bg_hover })
                .text_color(if on { p.accent_ink } else { p.ink })
                .child(icon.size(px(18.0))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(
                    div()
                        .text_size(px(text::META))
                        .text_color(p.ink_3)
                        .child(body),
                ),
        )
}

fn switch(id: (&'static str, usize), on: bool, p: &Palette) -> Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .w(px(36.0))
        .h(px(20.0))
        .flex_none()
        .p(px(2.0))
        .flex()
        .when(on, |s| s.justify_end())
        .rounded_full()
        .bg(if on { p.accent } else { p.line_strong })
        .child(div().size(px(16.0)).rounded_full().bg(p.bg))
}

fn stage(p: &Palette) -> gpui_kit::Div {
    div()
        .flex_1()
        .min_w_0()
        .min_h(px(460.0))
        .p(px(32.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(14.0))
        .rounded(px(20.0))
        .border_1()
        .border_color(p.line)
        .bg(p.bg_subtle)
}

/// Where a repeating animation is `offset` of a cycle later, so a group of
/// them runs out of step.
fn shifted(t: f32, offset: f32) -> f32 {
    (t + offset).fract()
}

/// A falling envelope fades in at the top and out as it reaches the sift.
fn fall_opacity(s: f32) -> f32 {
    (s / 0.15).min((1.0 - s) / 0.25).clamp(0.0, 1.0)
}

/// The delay before the `index`th of a staggered group arrives.
fn stagger(index: usize) -> Duration {
    Duration::from_millis(120 + 70 * u64::try_from(index).unwrap_or(0))
}

/// Fades `element` in once after `delay`, rising `lift` pixels as it does.
fn arrive(
    id: impl Into<ElementId>,
    element: gpui_kit::Div,
    delay: Duration,
    lift: f32,
) -> AnyElement {
    element
        .with_animations(
            id,
            vec![
                Animation::new(delay.max(Duration::from_millis(1))),
                Animation::new(Duration::from_millis(420)).with_easing(ease_out_quint()),
            ],
            move |el, step, t| {
                let t = if step == 0 { 0.0 } else { t };
                let el = el.opacity(t);
                if lift == 0.0 {
                    el
                } else {
                    el.relative().top(px((1.0 - t) * lift))
                }
            },
        )
        .into_any_element()
}

fn card(p: &Palette) -> gpui_kit::Div {
    div()
        .rounded(px(radius::CARD))
        .border_1()
        .border_color(p.line_strong)
        .bg(p.bg)
}

impl SiftApp {
    /// The whole onboarding screen below the title bar.
    pub(super) fn onboarding_screen(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(o) = &self.onboarding else {
            return div().into_any_element();
        };
        let body = if o.step == OnboardStep::Welcome {
            self.welcome(p, cx)
        } else {
            self.onboarding_step_screen(o.step, p, cx)
        };
        div()
            .id("onboarding")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .test_support()
            .flex()
            .justify_center()
            .items_start()
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_WIDTH))
                    .px(px(40.0))
                    .py(px(40.0))
                    .child(body),
            )
            .into_any_element()
    }

    /// "Sifting 1,204" in the title bar while the first account syncs.
    pub(super) fn onboarding_sync_chip(&self, p: &Palette) -> Option<AnyElement> {
        let first = self.accounts.first()?;
        let count = self.store.message_count(first.id).unwrap_or_default();
        let busy = self.syncing
            || self
                .form
                .as_ref()
                .is_some_and(|f| f.sync_result.is_none() && f.account.is_some());
        let text = if busy {
            format!("Sifting {count}")
        } else {
            format!("All {count} messages sifted")
        };
        Some(
            div()
                .h(px(26.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded_full()
                .bg(if busy { p.accent_soft } else { p.good_soft })
                .text_color(if busy { p.accent_text } else { p.good })
                .text_size(px(text::META))
                .font_weight(FontWeight::MEDIUM)
                .child(
                    Icon::new(if busy {
                        IconName::LoaderCircle
                    } else {
                        IconName::Check
                    })
                    .size(px(12.0)),
                )
                .child(text)
                .into_any_element(),
        )
    }

    fn welcome(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let chip = |label: &'static str| {
            div()
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded_full()
                .border_1()
                .border_color(p.line_strong)
                .text_size(px(text::META))
                .text_color(p.ink_2)
                .child(label)
        };
        let tints = avatar_tints(p);
        let drops: Vec<AnyElement> = [
            ("Priya", 0usize, 6.0f32, 0.0f32),
            ("Weekly digest", 2, 30.0, 40.0),
            ("Invite: Roadmap", 3, 56.0, 10.0),
            ("Your receipt", 4, 18.0, 80.0),
            ("Tom", 1, 70.0, 64.0),
            ("Flash sale!", 5, 42.0, 110.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, tint, left, top))| {
            let (bg, ink) = tints[tint];
            // Each envelope falls the height of the scene, starting where the
            // still picture puts it, so reduced motion shows the board's layout.
            let offset = (top + 34.0) / 194.0;
            div()
                .absolute()
                .left(gpui_kit::relative(left / 100.0))
                .top(px(top))
                .h(px(30.0))
                .pl(px(4.0))
                .pr(px(10.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded_full()
                .border_1()
                .border_color(p.line_strong)
                .bg(p.bg)
                .text_size(px(text::META))
                .child(
                    div()
                        .size(px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(bg)
                        .text_color(ink)
                        .text_size(px(10.0))
                        .font_weight(FontWeight::BOLD)
                        .child(initials(label)),
                )
                .child(label)
                .with_animation(
                    ("drop", i),
                    Animation::new(Duration::from_millis(4800)).repeat(),
                    move |el, t| {
                        let s = shifted(t, offset);
                        el.top(px(-34.0 + 194.0 * s)).opacity(fall_opacity(s))
                    },
                )
                .into_any_element()
        })
        .collect();
        let bins: Vec<AnyElement> = [
            ("Needs you", 5, TriageView::NeedsYou),
            ("Meetings", 2, TriageView::Meetings),
            ("Read later", 77, TriageView::Newsletters),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, n, view))| {
            let offset = i as f32 / 3.0;
            card(p)
                .flex_1()
                .relative()
                .p(px(14.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    Icon::new(view_icon(view))
                        .size(px(16.0))
                        .text_color(p.accent),
                )
                .child(
                    div()
                        .text_size(px(24.0))
                        .font_weight(FontWeight::BOLD)
                        .child(n.to_string()),
                )
                .child(
                    div()
                        .text_size(px(text::META))
                        .text_color(p.ink_3)
                        .child(label),
                )
                .with_animation(
                    ("bin", i),
                    Animation::new(Duration::from_millis(2400))
                        .repeat()
                        .with_easing(ease_in_out),
                    move |el, t| {
                        let s = shifted(t, offset);
                        el.top(px(-4.0 * (s * std::f32::consts::TAU).sin().max(0.0)))
                    },
                )
                .into_any_element()
        })
        .collect();
        div()
            .flex()
            .gap(px(40.0))
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(20.0))
                    .child(
                        div()
                            .text_size(px(text::SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.accent_text)
                            .child("WELCOME TO SIFT MAIL"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .text_size(px(48.0))
                            .line_height(px(54.0))
                            .font_weight(FontWeight::BOLD)
                            .child("Less inbox.")
                            .child(div().text_color(p.accent).child("More everything else.")),
                    )
                    .child(div().text_size(px(16.0)).line_height(px(26.0)).text_color(p.ink_2).child(
                        "Sift sorts the pile, sums up the long ones and gets you to the bottom fast. Setup takes about two minutes, and Sift gets to work on your mail while you're still choosing.",
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.0))
                            .child(
                                pill("lets-sift", "Let's sift".into(), true, p)
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.onboard_next(window, cx)))
                                    .test_support(),
                            )
                            .child(div().text_size(px(text::META)).text_color(p.ink_3).child("No credit card. No new address. Just calmer email.")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.0))
                            .child(chip("Sorted for you"))
                            .child(chip("AI that stays in its lane"))
                            .child(chip("A key for everything")),
                    ),
            )
            .child(
                stage(p)
                    .justify_end()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(160.0))
                            .overflow_hidden()
                            .children(drops),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(p.accent_line)
                            .bg(p.accent_soft)
                            .text_color(p.accent_text)
                            .font_weight(FontWeight::BOLD)
                            .child("sift")
                            .with_animation(
                                "sift-bar",
                                Animation::new(Duration::from_millis(2400))
                                    .repeat()
                                    .with_easing(pulsating_between(0.7, 1.0)),
                                |el, t| el.opacity(t),
                            ),
                    )
                    .child(div().w_full().flex().gap(px(12.0)).children(bins)),
            )
            .into_any_element()
    }

    fn onboarding_step_screen(
        &self,
        step: OnboardStep,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let connected = !self.accounts.is_empty();
        let (eyebrow, heading, sub) = step.copy(connected);
        let dots: Vec<AnyElement> = OnboardStep::ALL[1..]
            .iter()
            .map(|s| {
                let current = *s == step;
                let done = s.index() < step.index();
                div()
                    .h(px(6.0))
                    .w(px(if current { 24.0 } else { 6.0 }))
                    .rounded_full()
                    .bg(if current || done {
                        p.accent
                    } else {
                        p.line_strong
                    })
                    .into_any_element()
            })
            .collect();
        let content = match step {
            OnboardStep::Connect => self.connect_content(p, cx),
            OnboardStep::Sort => self.sort_content(p, cx),
            OnboardStep::Ai => self.ai_content(p, cx),
            OnboardStep::Focus => self.focus_content(p, cx),
            OnboardStep::Look => self.look_content(p, cx),
            OnboardStep::Keys => self.keys_content(p),
            OnboardStep::Done | OnboardStep::Welcome => self.done_content(p, cx),
        };
        let stage_view = match step {
            OnboardStep::Connect => self.connect_stage(p),
            OnboardStep::Sort => self.sort_stage(p),
            OnboardStep::Ai => self.ai_stage(p),
            OnboardStep::Focus => self.focus_stage(p),
            OnboardStep::Look => self.look_stage(p, cx),
            OnboardStep::Keys => self.keys_stage(p, cx),
            OnboardStep::Done | OnboardStep::Welcome => self.done_stage(p),
        };
        let practice_count = self.onboarding.as_ref().map_or(0, |o| o.practice_count());
        let next_label: SharedString = match step {
            OnboardStep::Connect => "Continue".into(),
            OnboardStep::Sort => "Looks good".into(),
            OnboardStep::Keys if practice_count == PRACTICE_TASKS.len() => "Nailed it".into(),
            _ => "Next".into(),
        };
        let can_skip = !matches!(step, OnboardStep::Connect | OnboardStep::Done);
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(div().flex().gap(px(6.0)).items_center().children(dots))
            .child(
                div()
                    .flex()
                    .gap(px(40.0))
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h(px(460.0))
                            .flex()
                            .flex_col()
                            .gap(px(24.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.0))
                                    .child(
                                        div()
                                            .text_size(px(text::SMALL))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.accent_text)
                                            .child(eyebrow.to_uppercase()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(32.0))
                                            .line_height(px(38.0))
                                            .font_weight(FontWeight::BOLD)
                                            .child(heading),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(15.0))
                                            .line_height(px(24.0))
                                            .text_color(p.ink_2)
                                            .child(sub),
                                    ),
                            )
                            .child(content)
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .when(step.index() > 1, |f| {
                                        f.child(
                                            ghost("onboard-back", "Back", p)
                                                .on_click(cx.listener(
                                                    |this, _: &ClickEvent, _, cx| {
                                                        this.onboard_back(cx)
                                                    },
                                                ))
                                                .test_support(),
                                        )
                                    })
                                    .child(div().flex_1())
                                    .when(can_skip, |f| {
                                        f.child(
                                            ghost(
                                                "onboard-skip",
                                                if step == OnboardStep::Keys {
                                                    "I'll learn later"
                                                } else {
                                                    "Use the defaults"
                                                },
                                                p,
                                            )
                                            .on_click(cx.listener(
                                                move |this, _: &ClickEvent, _, cx| {
                                                    this.onboard_go(step.offset(1), cx);
                                                },
                                            ))
                                            .test_support(),
                                        )
                                    })
                                    .child(if step == OnboardStep::Done {
                                        pill(
                                            "onboard-finish",
                                            "Take me to my inbox".into(),
                                            true,
                                            p,
                                        )
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.finish_onboarding(window, cx)
                                            },
                                        ))
                                        .test_support()
                                    } else {
                                        pill(
                                            "onboard-next",
                                            next_label,
                                            step != OnboardStep::Connect || connected,
                                            p,
                                        )
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.onboard_next(window, cx)
                                            },
                                        ))
                                        .test_support()
                                    }),
                            ),
                    )
                    // Keyed by step, so each new stage glides in.
                    .child(arrive(
                        ("stage-in", step.index()),
                        div().flex_1().min_w_0().flex().child(stage_view),
                        Duration::ZERO,
                        16.0,
                    )),
            )
            .into_any_element()
    }

    fn connect_content(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        if let Some(first) = self.accounts.first() {
            return div()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(
                    div()
                        .h(px(48.0))
                        .px(px(14.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .rounded(px(radius::CARD))
                        .bg(p.good_soft)
                        .text_color(p.good)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(Icon::new(IconName::CircleCheck).size(px(18.0)))
                        .child(format!("{} is in. Sift is already sorting.", first.name)),
                )
                .child(div().text_color(p.ink_3).child(
                    "While that hums along, a few quick choices. Nothing here is permanent.",
                ))
                .child(
                    ghost("onboard-add-more", "Add another account", p)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.onboard_connect(None, window, cx)
                        }))
                        .test_support(),
                )
                .into_any_element();
        }
        let options: Vec<AnyElement> = Provider::ALL
            .into_iter()
            .enumerate()
            .map(|(i, provider)| {
                let (title, body): (&'static str, &'static str) = match provider {
                    Provider::Microsoft365 => (
                        "Microsoft 365 or Outlook",
                        "Work or school, Outlook.com, Hotmail",
                    ),
                    Provider::Google => ("Google", "Gmail and Google Workspace"),
                    Provider::ICloud => ("iCloud Mail", "Uses an app-specific password"),
                    Provider::Imap => (
                        "Something else (IMAP)",
                        "Fastmail, your host, anything with IMAP",
                    ),
                };
                let icon: gpui_kit::component::Icon = match provider {
                    Provider::Microsoft365 => IconName::Building2.into(),
                    Provider::Google => IconName::Globe.into(),
                    Provider::ICloud => Lucide::Cloud.into(),
                    Provider::Imap => Lucide::Server.into(),
                };
                option(("onboard-provider", i), icon, title, body, false, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.onboard_connect(Some(provider), window, cx);
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .children(options)
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .text_size(px(text::META))
                    .text_color(p.ink_3)
                    .child("Know your address but not your provider?")
                    .child(
                        div()
                            .id("onboard-type-it")
                            .text_color(p.accent_text)
                            .font_weight(FontWeight::MEDIUM)
                            .child("Type it in")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.onboard_connect(None, window, cx)
                            }))
                            .test_support(),
                    )
                    .child("and Sift will work it out."),
            )
            .into_any_element()
    }

    fn connect_stage(&self, p: &Palette) -> AnyElement {
        let first = self.accounts.first();
        let provider = first.map_or("Your provider", |a| short_name(a.provider));
        let row = |icon: gpui_kit::component::Icon, title: String, sub: &'static str| {
            card(p)
                .w(px(300.0))
                .p(px(14.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .size(px(36.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(10.0))
                        .bg(p.bg_hover)
                        .child(icon.size(px(18.0))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(
                            div()
                                .text_size(px(text::META))
                                .text_color(p.ink_3)
                                .child(sub),
                        ),
                )
        };
        stage(p)
            .child(row(
                IconName::Globe.into(),
                provider.to_owned(),
                "Your password lives here",
            ))
            .child(
                div()
                    .h(px(30.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded_full()
                    .bg(if first.is_some() {
                        p.good_soft
                    } else {
                        p.accent_soft
                    })
                    .text_color(if first.is_some() {
                        p.good
                    } else {
                        p.accent_text
                    })
                    .text_size(px(text::META))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Icon::new(Lucide::KeyRound).size(px(14.0)))
                    .child(if first.is_some() {
                        "Connected securely"
                    } else {
                        "A key, not a password"
                    }),
            )
            .child(row(
                Lucide::Mail.into(),
                "Sift on this computer".to_owned(),
                if first.is_some() {
                    "Downloading the newest mail first"
                } else {
                    "Mail is stored on your computer"
                },
            ))
            .into_any_element()
    }

    fn sort_content(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let modes = [
            (
                SortMode::Views,
                Icon::from(IconName::Inbox),
                "Sift it into views",
                "Recommended. Tabs for what matters, and the noise grouped so you can clear it in one go.",
            ),
            (
                SortMode::Plain,
                Icon::from(IconName::Menu),
                "Just one big list",
                "Everything by date, like you're used to. You can turn views on later.",
            ),
        ];
        let hover = p.bg_hover;
        let chips: Vec<AnyElement> = std::iter::once(TriageView::NeedsYou)
            .chain(OPTIONAL_VIEWS)
            .enumerate()
            .map(|(i, view)| {
                let on = self.views.shows(view);
                let locked = view == TriageView::NeedsYou;
                div()
                    .id(("onboard-view", i))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded_full()
                    .border_1()
                    .border_color(if on { p.accent } else { p.line_strong })
                    .bg(if on { p.accent_soft } else { p.bg })
                    .text_color(if on { p.accent_text } else { p.ink_2 })
                    .when(!locked, |c| c.hover(move |s| s.bg(hover)))
                    .when(locked, |c| c.opacity(0.8))
                    .child(Icon::new(view_icon(view)).size(px(14.0)))
                    .child(view.label())
                    .when(!locked, |c| {
                        c.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.toggle_view_tab(view, cx)
                        }))
                    })
                    .test_support()
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .children(
                modes
                    .into_iter()
                    .enumerate()
                    .map(|(i, (mode, icon, title, body))| {
                        option(
                            ("onboard-sort", i),
                            icon,
                            title,
                            body,
                            self.sort_mode == mode,
                            p,
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_sort_mode(mode, cx)
                        }))
                        .test_support()
                        .into_any_element()
                    }),
            )
            .when(self.sort_mode == SortMode::Views, |d| {
                d.child(
                    div()
                        .pt(px(8.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(text::SMALL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.ink_3)
                                .child("TABS ALONG THE TOP"),
                        )
                        .child(div().flex().flex_wrap().gap(px(8.0)).children(chips)),
                )
            })
            .into_any_element()
    }

    fn sort_stage(&self, p: &Palette) -> AnyElement {
        let sample = [
            (
                "Priya Raman",
                "Q4 budget numbers",
                TriageView::NeedsYou,
                0usize,
            ),
            (
                "Daniel Mensah",
                "Invitation: Roadmap sync",
                TriageView::Meetings,
                3,
            ),
            (
                "IT Service Desk",
                "Ticket 4821 resolved",
                TriageView::Updates,
                4,
            ),
            (
                "Product Weekly",
                "Issue 112: pricing pages",
                TriageView::Newsletters,
                2,
            ),
            ("Kofi Asante", "Re: Offsite planning", TriageView::Other, 1),
        ];
        let tints = avatar_tints(p);
        let tabs: Vec<AnyElement> = TriageView::ALL
            .into_iter()
            .filter(|v| self.tab_shown(*v))
            .map(|v| {
                let active = v == TriageView::All;
                div()
                    .h(px(26.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .rounded(px(radius::TAB))
                    .when(active, |t| t.bg(p.bg_hover))
                    .text_size(px(text::META))
                    .font_weight(FontWeight::MEDIUM)
                    .child(Icon::new(view_icon(v)).size(px(12.0)).text_color(p.ink_3))
                    .child(v.label())
                    .into_any_element()
            })
            .collect();
        let rows: Vec<AnyElement> = sample
            .into_iter()
            .map(|(from, subject, view, tint)| {
                let (bg, ink) = tints[tint];
                let shown = self.sort_mode == SortMode::Views && self.views.shows(view);
                div()
                    .h(px(40.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .border_t_1()
                    .border_color(p.line)
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
                            .child(initials(from)),
                    )
                    .child(
                        div()
                            .w(px(110.0))
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(from),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(p.ink_2)
                            .child(subject),
                    )
                    .when(shown, |r| {
                        r.child(
                            div()
                                .h(px(20.0))
                                .px(px(6.0))
                                .flex()
                                .items_center()
                                .rounded(px(radius::CHIP))
                                .border_1()
                                .border_color(p.line_strong)
                                .text_size(px(text::SMALL))
                                .child(chip_label(view)),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        stage(p)
            .child(
                card(p)
                    .w_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .p(px(8.0))
                            .flex()
                            .flex_wrap()
                            .gap(px(2.0))
                            .children(tabs),
                    )
                    .children(rows),
            )
            .into_any_element()
    }

    fn ai_content(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = AiFeature::ALL
            .into_iter()
            .enumerate()
            .map(|(i, feature)| {
                let (title, body) = feature.copy();
                let on = self.ai_features.on(feature);
                div()
                    .min_h(px(60.0))
                    .p(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .rounded(px(radius::CARD))
                    .border_1()
                    .border_color(p.line_strong)
                    .child(
                        div()
                            .size(px(36.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(10.0))
                            .bg(p.accent_soft)
                            .text_color(p.accent_text)
                            .child(Icon::new(Lucide::Sparkles).size(px(16.0))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(body),
                            ),
                    )
                    .child(
                        switch(("onboard-ai", i), on, p)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.toggle_ai_feature(feature, cx)
                            }))
                            .test_support(),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .children(rows)
            .child(div().pt(px(4.0)).text_size(px(text::META)).text_color(p.ink_3).child(
                "AI only reads a message when you use a feature on it, and nothing is used for training. Work account off-limits? Give it its own switch in Settings.",
            ))
            .into_any_element()
    }

    fn ai_stage(&self, p: &Palette) -> AnyElement {
        let on = |f| if self.ai_features.on(f) { 1.0 } else { 0.18 };
        let sparkle = |label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(text::SMALL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.accent_text)
                .child(Icon::new(Lucide::Sparkles).size(px(12.0)))
                .child(label)
        };
        stage(p)
            .child(
                card(p)
                    .w_full()
                    .p(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Q4 budget numbers"),
                    )
                    .child(
                        div()
                            .opacity(on(AiFeature::Summaries))
                            .p(px(12.0))
                            .rounded(px(radius::BUTTON))
                            .border_1()
                            .border_color(p.accent_line)
                            .bg(p.accent_soft)
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(sparkle("SUMMARY"))
                            .child(
                                "Priya needs your Q4 headcount and tooling costs by Monday 17:00.",
                            ),
                    )
                    .child(
                        div()
                            .opacity(on(AiFeature::Tasks))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .size(px(14.0))
                                    .rounded(px(4.0))
                                    .border_1()
                                    .border_color(p.line_strong),
                            )
                            .child("Send headcount and tooling costs")
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.warn)
                                    .child("Due Mon"),
                            ),
                    )
                    .child(
                        div()
                            .opacity(on(AiFeature::Drafts))
                            .flex()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(sparkle("DRAFTS"))
                            .children(
                                [
                                    "On it, by Monday",
                                    "Quick question first",
                                    "Can we push to Tuesday?",
                                ]
                                .map(|d| {
                                    div()
                                        .h(px(26.0))
                                        .px(px(10.0))
                                        .flex()
                                        .items_center()
                                        .rounded_full()
                                        .border_1()
                                        .border_color(p.accent_line)
                                        .text_size(px(text::META))
                                        .child(d)
                                }),
                            ),
                    ),
            )
            .child(
                card(p)
                    .opacity(on(AiFeature::Ask))
                    .w_full()
                    .h(px(44.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(p.ink_3)
                    .child(Icon::new(IconName::Search).size(px(14.0)))
                    .child("What does Tom need from me?"),
            )
            .into_any_element()
    }

    fn focus_content(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let hover = p.bg_hover;
        let icons: [Icon; 3] = [
            Lucide::Clock.into(),
            IconName::Bell.into(),
            IconName::EyeOff.into(),
        ];
        let modes: Vec<AnyElement> = NotifyMode::ALL
            .into_iter()
            .zip(icons)
            .enumerate()
            .map(|(i, (mode, icon))| {
                let (title, body) = mode.copy();
                option(
                    ("onboard-notify", i),
                    icon,
                    title,
                    body,
                    self.notify_mode == mode,
                    p,
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.set_notify_mode(mode, cx)),
                )
                .test_support()
                .into_any_element()
            })
            .collect();
        let times: Vec<AnyElement> = CHECKIN_TIMES
            .into_iter()
            .enumerate()
            .map(|(i, time)| {
                let on = self.checkins.on(i);
                div()
                    .id(("onboard-checkin", i))
                    .h(px(30.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if on { p.accent } else { p.line_strong })
                    .bg(if on { p.accent_soft } else { p.bg })
                    .text_color(if on { p.accent_text } else { p.ink_2 })
                    .when(!on, |c| c.hover(move |s| s.bg(hover)))
                    .text_size(px(text::META))
                    .font_weight(FontWeight::MEDIUM)
                    .child(time)
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_checkin(i, cx)),
                    )
                    .test_support()
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .children(modes)
            .when(self.notify_mode == NotifyMode::Hold, |d| {
                d.child(
                    div()
                        .pt(px(6.0))
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(text::META))
                                .text_color(p.ink_3)
                                .child("Check in at"),
                        )
                        .children(times),
                )
            })
            .into_any_element()
    }

    fn focus_stage(&self, p: &Palette) -> AnyElement {
        // A working day from 08:00 to 18:00, with pings where they would land.
        let at = |hour: f32| ((hour - 8.0) / 10.0).clamp(0.0, 1.0);
        let urgent = [9.4f32, 13.2, 16.1];
        let ping =
            |left: f32, size: f32, colour: crate::theme::color::Rgb, label: Option<String>| {
                div()
                    .absolute()
                    .left(gpui_kit::relative(left * 0.92 + 0.02))
                    .size(px(size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(colour)
                    .text_color(p.accent_ink)
                    .text_size(px(text::SMALL))
                    .font_weight(FontWeight::BOLD)
                    .children(label)
            };
        let mut pings: Vec<gpui_kit::Div> = Vec::new();
        let mode = match self.notify_mode {
            NotifyMode::All => 0,
            NotifyMode::Hold => 1,
            NotifyMode::Never => 2,
        };
        match self.notify_mode {
            NotifyMode::All => {
                for i in 0..24 {
                    pings.push(ping(i as f32 / 24.0, 8.0, p.accent, None));
                }
                pings.extend(urgent.map(|u| ping(at(u), 12.0, p.danger, None)));
            }
            NotifyMode::Hold => {
                pings.extend(urgent.map(|u| ping(at(u), 12.0, p.danger, None)));
                let times = self.checkins.times();
                let per = if times.is_empty() {
                    0
                } else {
                    40 / times.len()
                };
                for t in times {
                    let hour = t
                        .split(':')
                        .next()
                        .and_then(|h| h.parse::<f32>().ok())
                        .unwrap_or(12.0);
                    pings.push(ping(at(hour), 30.0, p.accent, Some(per.to_string())));
                }
            }
            NotifyMode::Never => {
                pings.extend(urgent.map(|u| ping(at(u), 8.0, p.line_strong, None)));
            }
        }
        stage(p)
            .child(
                card(p)
                    .w_full()
                    .p(px(18.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Your day"))
                    .child(
                        div()
                            .relative()
                            .h(px(40.0))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .h(px(2.0))
                                    .bg(p.line_strong),
                            )
                            .children(pings.into_iter().enumerate().map(|(i, ping)| {
                                // Keyed by mode too, so changing it pops them in again.
                                let id = ("ping", mode * 100 + i);
                                ping.with_animations(
                                    id,
                                    vec![
                                        Animation::new(Duration::from_millis(1 + 25 * i as u64)),
                                        Animation::new(Duration::from_millis(360))
                                            .with_easing(ease_out_quint()),
                                    ],
                                    |el, step, t| el.opacity(if step == 0 { 0.0 } else { t }),
                                )
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(text::SMALL))
                            .text_color(p.ink_3)
                            .children(["08:00", "10:00", "12:00", "14:00", "16:00", "18:00"]),
                    ),
            )
            .child(div().text_size(px(text::META)).text_color(p.ink_3).child(
                match self.notify_mode {
                    NotifyMode::Hold => {
                        "Red is urgent and gets through. Everything else arrives in a batch."
                    }
                    NotifyMode::All => "Every message taps you on the shoulder.",
                    NotifyMode::Never => "Quiet. Mail waits until you open Sift.",
                },
            ))
            .into_any_element()
    }

    fn look_content(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let current = SiftTheme::get(cx).appearance();
        let preview = |palette: Palette, rows: usize| {
            div()
                .w_full()
                .h(px(84.0))
                .p(px(8.0))
                .flex()
                .flex_col()
                .gap(px(if self.compact { 4.0 } else { 7.0 }))
                .rounded(px(radius::BUTTON))
                .bg(palette.bg)
                .border_1()
                .border_color(palette.line_strong)
                .children((0..rows).map(move |i| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(div().size(px(6.0)).rounded_full().bg(if i < 2 {
                            palette.accent
                        } else {
                            palette.line_strong
                        }))
                        .child(div().w(px(28.0)).h(px(4.0)).rounded_full().bg(palette.ink))
                        .child(
                            div()
                                .flex_1()
                                .h(px(4.0))
                                .rounded_full()
                                .bg(palette.line_strong),
                        )
                }))
        };
        let themes: Vec<AnyElement> = [
            ("Light", Appearance::Light),
            ("Dark", Appearance::Dark),
            ("Match system", Appearance::System),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, appearance))| {
            let on = current == appearance;
            let palette = match appearance {
                Appearance::Dark => DARK,
                _ => LIGHT,
            };
            div()
                .id(("onboard-theme", i))
                .flex_1()
                .p(px(8.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .rounded(px(radius::CARD))
                .border_2()
                .border_color(if on { p.accent } else { p.line })
                .child(preview(palette, 5))
                .child(
                    div()
                        .text_center()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.onboard_theme(appearance, window, cx)
                }))
                .test_support()
                .into_any_element()
        })
        .collect();
        let density = |id: &'static str, label: &'static str, compact: bool| {
            let on = self.compact == compact;
            div()
                .id(id)
                .h(px(28.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .rounded(px(6.0))
                .text_size(px(text::META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if on { p.ink } else { p.ink_3 })
                .when(on, |s| s.bg(p.bg).border_1().border_color(p.line))
                .child(label)
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.set_compact(compact, cx)),
                )
                .test_support()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .child(div().flex().gap(px(10.0)).children(themes))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Breathing room"),
                    )
                    .child(
                        div()
                            .flex()
                            .p(px(2.0))
                            .gap(px(2.0))
                            .rounded(px(radius::BUTTON))
                            .bg(p.bg_hover)
                            .child(density("onboard-roomy", "Roomy", false))
                            .child(density("onboard-snug", "Snug", true)),
                    ),
            )
            .into_any_element()
    }

    fn look_stage(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let current = SiftTheme::get(cx).appearance();
        let (icon, line, sub): (Icon, &str, &str) = match current {
            Appearance::Light => (
                IconName::Sun.into(),
                "Bright and clear.",
                "Crisp whites for daytime focus.",
            ),
            Appearance::Dark => (
                IconName::Moon.into(),
                "Easy on the eyes.",
                "Deep greys that stay calm after dark.",
            ),
            Appearance::System => (
                IconName::Palette.into(),
                "Follows your computer.",
                "Light by day, dark by night, automatically.",
            ),
        };
        let row_h = if self.compact { 32.0 } else { 40.0 };
        let rows = [
            "Q4 budget numbers",
            "Invitation: Roadmap sync",
            "Vendor contract renewal",
            "Ticket 4821 resolved",
            "Issue 112: pricing pages",
        ];
        stage(p)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(icon.size(px(20.0)).text_color(p.accent))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(line))
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(sub),
                            ),
                    ),
            )
            .child(
                card(p)
                    .w_full()
                    .overflow_hidden()
                    .children(rows.iter().enumerate().map(|(i, subject)| {
                        div()
                            .h(px(row_h))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .when(i > 0, |r| r.border_t_1().border_color(p.line))
                            .child(
                                div()
                                    .size(px(6.0))
                                    .rounded_full()
                                    .when(i < 2, |d| d.bg(p.accent)),
                            )
                            .child(
                                div()
                                    .font_weight(if i < 2 {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .child(*subject),
                            )
                    })),
            )
            .into_any_element()
    }

    fn keys_content(&self, p: &Palette) -> AnyElement {
        let Some(o) = &self.onboarding else {
            return div().into_any_element();
        };
        let tasks: Vec<AnyElement> = PRACTICE_TASKS
            .iter()
            .enumerate()
            .map(|(i, (label, key))| {
                let done = o.practice_done[i];
                div()
                    .h(px(44.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .rounded(px(radius::BUTTON))
                    .border_1()
                    .border_color(if done { p.good } else { p.line_strong })
                    .when(done, |t| t.bg(p.good_soft))
                    .child(
                        div()
                            .size(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(if done { p.good } else { p.line_strong })
                            .when(done, |c| {
                                c.bg(p.good)
                                    .text_color(p.bg)
                                    .child(Icon::new(IconName::Check).size(px(12.0)))
                            }),
                    )
                    .child(div().flex_1().child(*label))
                    .child(kbd(key, p))
                    .into_any_element()
            })
            .collect();
        let count = o.practice_count();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .children(tasks)
            .child(
                div()
                    .pt(px(4.0))
                    .text_color(if count == PRACTICE_TASKS.len() {
                        p.good
                    } else {
                        p.ink_3
                    })
                    .child(PRACTICE_HINTS[count.min(PRACTICE_HINTS.len() - 1)]),
            )
            .into_any_element()
    }

    fn keys_stage(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(o) = &self.onboarding else {
            return div().into_any_element();
        };
        let left = o.practice_left.len();
        let rows: Vec<AnyElement> = o
            .practice_left
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let (from, subject) = PRACTICE_ROWS[*row];
                let current = i == o.practice_cursor;
                div()
                    .h(px(40.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .border_t_1()
                    .border_color(p.line)
                    .when(current, |r| r.bg(p.bg_active))
                    .child(
                        div()
                            .w(px(120.0))
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(from),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(p.ink_2)
                            .child(subject),
                    )
                    .into_any_element()
            })
            .collect();
        let practice_buttons = |id: &'static str,
                                label: &'static str,
                                key: &'static str,
                                cx: &mut Context<Self>| {
            ghost(id, label, p)
                .border_1()
                .border_color(p.line_strong)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.practice(key, cx)))
                .test_support()
        };
        stage(p)
            .child(
                card(p)
                    .w_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .h(px(40.0))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if left > 0 {
                                format!("Practice inbox · {left} to go")
                            } else {
                                "Practice inbox".to_owned()
                            }),
                    )
                    .children(rows)
                    .when(left == 0, |c| {
                        c.child(
                            div()
                                .h(px(120.0))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(6.0))
                                .border_t_1()
                                .border_color(p.line)
                                .child(
                                    Icon::new(IconName::CircleCheck)
                                        .size(px(24.0))
                                        .text_color(p.good),
                                )
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Inbox zero. Feels good."),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(practice_buttons("practice-j", "Down  J", "j", cx))
                    .child(practice_buttons("practice-k", "Up  K", "k", cx))
                    .child(practice_buttons("practice-e", "Done  E", "e", cx))
                    .child(practice_buttons(
                        "practice-palette",
                        "Command  Ctrl K",
                        "palette",
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn done_content(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let account = self.accounts.first().map_or_else(
            || "None yet".to_owned(),
            |a| format!("{} via {}", a.name, short_name(a.provider)),
        );
        let inbox = match self.sort_mode {
            SortMode::Plain => "One big list".to_owned(),
            SortMode::Views => std::iter::once(TriageView::NeedsYou)
                .chain(OPTIONAL_VIEWS)
                .filter(|v| self.views.shows(*v))
                .map(TriageView::label)
                .collect::<Vec<_>>()
                .join(", "),
        };
        let helpers = match self.ai_features.count() {
            4 => "All four on".to_owned(),
            0 => "None, thanks".to_owned(),
            n => format!("{n} of 4 on"),
        };
        let focus = match self.notify_mode {
            NotifyMode::Hold => {
                let times = self.checkins.times();
                if times.is_empty() {
                    "Batched at no set times".to_owned()
                } else {
                    format!("Batched at {}", times.join(" and "))
                }
            }
            NotifyMode::All => "Everything pings".to_owned(),
            NotifyMode::Never => "Never interrupt".to_owned(),
        };
        let look = format!(
            "{}, {}",
            match SiftTheme::get(cx).appearance() {
                Appearance::System => "Match system",
                Appearance::Dark => "Dark",
                Appearance::Light => "Light",
            },
            if self.compact { "snug" } else { "roomy" }
        );
        let rows: Vec<AnyElement> =
            [
                ("Account", account, OnboardStep::Connect),
                ("Inbox", inbox, OnboardStep::Sort),
                ("Helpers", helpers, OnboardStep::Ai),
                ("Focus", focus, OnboardStep::Focus),
                ("Look", look, OnboardStep::Look),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (k, v, step))| {
                div()
                    .min_h(px(48.0))
                    .py(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .border_b_1()
                    .border_color(p.line)
                    .child(div().w(px(90.0)).text_color(p.ink_3).child(k))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::MEDIUM)
                            .child(v),
                    )
                    .child(
                        ghost("onboard-change", "Change", p)
                            .id(("onboard-change", i))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.onboard_go(step, cx)
                            }))
                            .test_support(),
                    )
                    .into_any_element()
            })
            .collect();
        div().flex().flex_col().children(rows).into_any_element()
    }

    fn done_stage(&self, p: &Palette) -> AnyElement {
        let mut chips: Vec<(&'static str, Icon)> = Vec::new();
        if self.sort_mode == SortMode::Views {
            for view in std::iter::once(TriageView::NeedsYou).chain(OPTIONAL_VIEWS) {
                if self.views.shows(view) {
                    chips.push((view.label(), Icon::new(view_icon(view))));
                }
            }
        }
        if self.ai_features.on(AiFeature::Summaries) {
            chips.push(("Summaries", Lucide::Sparkles.into()));
        }
        if self.notify_mode == NotifyMode::Hold {
            chips.push(("Focus", Lucide::Clock.into()));
        }
        stage(p)
            .child(arrive(
                "done-check",
                div().child(
                    Icon::new(IconName::CircleCheck)
                        .size(px(40.0))
                        .text_color(p.good),
                ),
                stagger(0),
                12.0,
            ))
            .child(
                div()
                    .text_size(px(20.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Your inbox is ready"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(8.0))
                    .children(chips.into_iter().enumerate().map(|(i, (label, icon))| {
                        let chip = div()
                            .h(px(30.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .rounded_full()
                            .border_1()
                            .border_color(p.line_strong)
                            .bg(p.bg)
                            .child(icon.size(px(14.0)).text_color(p.accent))
                            .child(label);
                        arrive(("done-chip", i), chip, stagger(i + 2), 8.0)
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_runs_out_of_step_and_fades_at_the_ends() {
        assert!((shifted(0.75, 0.5) - 0.25).abs() < 1e-6);
        assert_eq!(fall_opacity(0.0), 0.0);
        assert_eq!(fall_opacity(0.5), 1.0);
        assert_eq!(fall_opacity(1.0), 0.0);
        assert!(stagger(2) > stagger(1));
    }
}
