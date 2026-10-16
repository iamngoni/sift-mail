//! How the Calendar looks, following the Calendar board: a Monday–Friday
//! grid from 08:00 to 18:00 at 48 px an hour with an all-day row, events
//! coloured by kind, a now line, popovers for details and quick create, a
//! legend, and a side panel with Find a time, Waiting on you, the week at a
//! glance and the keys.

use chrono::{Datelike as _, Local, NaiveTime, Timelike as _, Utc};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, SharedString, Stateful, TestSupportExt as _, div,
    px,
};

use super::app::SiftApp;
use super::calendar::{local, when_label};
use super::render::kbd;
use crate::domain::ical::Rsvp;
use crate::domain::schedule::{DAY_END, DAY_START, hours_label};
use crate::store::{Event, EventKind};
use crate::theme::color::Rgb;
use crate::theme::tokens::{DARK, Palette, radius, text};

/// Height of one hour in the grid.
pub const HOUR: f32 = 48.0;
/// Width of the side panel.
const PANEL_WIDTH: f32 = 320.0;

/// Background, border and text colours for an event kind.
fn colours(kind: EventKind, p: &Palette) -> (Rgb, Rgb, Rgb) {
    let dark = *p == DARK;
    let pick = |light: (u32, u32, u32), dark_set: (u32, u32, u32)| {
        let (b, l, i) = if dark { dark_set } else { light };
        (Rgb::from_u32(b), Rgb::from_u32(l), Rgb::from_u32(i))
    };
    match kind {
        EventKind::Meeting => pick(
            (0xEEEBFC, 0xD9D2F7, 0x3B2A8F),
            (0x24204A, 0x3A3270, 0xD9D2FF),
        ),
        EventKind::Focus => pick(
            (0xE6F4EA, 0xC3E3CD, 0x17532F),
            (0x15301F, 0x25543A, 0xBDEBCD),
        ),
        EventKind::Personal => pick(
            (0xFEF3EA, 0xF2CDB0, 0x6B3410),
            (0x36230F, 0x5F3E1C, 0xFADCC4),
        ),
        EventKind::Pending | EventKind::Declined => (p.bg, p.ink_3, p.ink),
        EventKind::Hold => (p.accent_soft, p.accent, p.accent_text),
        EventKind::Mine => (p.accent, p.accent, p.accent_ink),
    }
}

fn button(
    id: impl Into<gpui_kit::ElementId>,
    label: SharedString,
    dark: bool,
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
        .text_size(px(text::META))
        .font_weight(FontWeight::MEDIUM)
        .when(dark, |b| {
            b.bg(p.primary)
                .border_color(p.primary)
                .text_color(p.primary_ink)
        })
        .when(!dark, |b| {
            b.bg(p.bg)
                .border_color(p.line_strong)
                .hover(move |s| s.bg(hover))
        })
        .child(label)
}

fn card(p: &Palette) -> gpui_kit::Div {
    div()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .rounded(px(radius::CARD))
        .border_1()
        .border_color(p.line_strong)
        .bg(p.bg)
}

fn label(text: &'static str, p: &Palette) -> AnyElement {
    div()
        .text_size(px(text::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.ink_3)
        .child(text.to_uppercase())
        .into_any_element()
}

fn y_of(minutes_from_midnight: f32) -> f32 {
    (minutes_from_midnight / 60.0 - DAY_START as f32) * HOUR
}

impl SiftApp {
    /// Mail and Calendar tabs in the title bar while the Calendar shows.
    pub(super) fn section_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let hover = p.bg_hover;
        vec![
            div()
                .id("tab-mail")
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(radius::TAB))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.ink_2)
                .hover(move |s| s.bg(hover))
                .child("Mail")
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.close_calendar(window, cx)),
                )
                .test_support()
                .into_any_element(),
            div()
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(radius::TAB))
                .bg(p.bg_hover)
                .border_1()
                .border_color(p.line_strong)
                .font_weight(FontWeight::MEDIUM)
                .child("Calendar")
                .into_any_element(),
        ]
    }

    /// The New event pill.
    pub(super) fn new_event_pill(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("new-event")
            .mr(px(8.0))
            .h(px(32.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded_full()
            .bg(p.primary)
            .text_color(p.primary_ink)
            .font_weight(FontWeight::SEMIBOLD)
            .child(Icon::new(IconName::Plus).size(px(14.0)))
            .child("New event")
            .child(div().text_size(px(text::SMALL)).opacity(0.8).child("N"))
            .on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.new_event_now(window, cx)),
            )
            .test_support()
            .into_any_element()
    }

    /// The Calendar below the title bar.
    pub(super) fn calendar_screen(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("calendar")
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.week_view(p, cx))
            .child(self.calendar_panel(p, cx))
            .into_any_element()
    }

    fn week_view(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let days = self.calendar_days();
        let events = self.week_events();
        let today = Local::now().date_naive();
        let week = self.calendar.as_ref().map_or(0, |c| c.week);
        let title = match week {
            0 => "This week".to_owned(),
            1 => "Next week".to_owned(),
            -1 => "Last week".to_owned(),
            _ => format!("Week of {}", days[0].format("%-d %b")),
        };
        let range = format!(
            "{} – {}",
            days[0].format("%a %-d %b"),
            days[4].format("%a %-d %b")
        );
        let hover = p.bg_hover;
        let nav = |id: &'static str, icon: IconName, delta: Option<i64>, cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::BUTTON))
                .text_color(p.ink_2)
                .hover(move |s| s.bg(hover))
                .child(Icon::new(icon).size(px(16.0)))
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.calendar_week(delta, cx)),
                )
                .test_support()
        };
        let hours = DAY_END - DAY_START;
        let grid_height = hours as f32 * HOUR;
        let hour_labels: Vec<AnyElement> = (DAY_START..DAY_END)
            .map(|h| {
                div()
                    .absolute()
                    .top(px(((h - DAY_START) as f32 * HOUR - 7.0).max(0.0)))
                    .right(px(8.0))
                    .text_size(px(text::SMALL))
                    .text_color(p.ink_3)
                    .child(format!("{h:02}:00"))
                    .into_any_element()
            })
            .collect();
        let mut headers: Vec<AnyElement> = vec![div().w(px(56.0)).flex_none().into_any_element()];
        let mut all_day: Vec<AnyElement> = vec![
            div()
                .w(px(56.0))
                .flex_none()
                .pr(px(8.0))
                .text_right()
                .text_size(px(text::SMALL))
                .text_color(p.ink_3)
                .child("all-day")
                .into_any_element(),
        ];
        let mut columns: Vec<AnyElement> = Vec::new();
        let mut layer: Vec<AnyElement> = Vec::new();
        for (d, day) in days.iter().enumerate() {
            let todays: Vec<&Event> = events
                .iter()
                .filter(|e| local(e.start).date() <= *day && local(e.end).date() >= *day)
                .collect();
            let meet_minutes: i64 = todays
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        EventKind::Meeting | EventKind::Pending | EventKind::Hold
                    ) && !e.all_day
                })
                .map(|e| (e.end - e.start).num_minutes())
                .sum();
            let is_today = *day == today;
            headers.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(8.0))
                    .pb(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(day.format("%a").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if is_today { p.accent_text } else { p.ink })
                                    .child(day.format("%-d").to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(text::SMALL))
                            .text_color(if meet_minutes >= 90 { p.warn } else { p.ink_3 })
                            .child(match meet_minutes {
                                0 => "No meetings".to_owned(),
                                m if m >= 90 => format!("Busy · {} meetings", hours_label(m)),
                                m => format!("{} meetings", hours_label(m)),
                            }),
                    )
                    .into_any_element(),
            );
            let all_day_titles: Vec<String> = todays
                .iter()
                .filter(|e| e.all_day)
                .map(|e| e.title.clone())
                .collect();
            all_day.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .p(px(2.0))
                    .border_l_1()
                    .border_color(p.line)
                    .children(all_day_titles.into_iter().map(|t| {
                        div()
                            .px(px(6.0))
                            .rounded(px(5.0))
                            .bg(p.accent_soft)
                            .text_color(p.accent_text)
                            .text_size(px(text::SMALL))
                            .truncate()
                            .child(t)
                    }))
                    .into_any_element(),
            );
            let day_copy = *day;
            let (draft_block, draft_popover) = self.draft_block(d, day_copy, p, cx).unzip();
            layer.extend(draft_popover);
            layer.extend(self.detail_popover(d, day_copy, p, cx));
            let slots: Vec<AnyElement> = (0..hours * 2)
                .map(|i| {
                    let start = DAY_START * 60 + i * 30;
                    div()
                        .id(("slot", d * 100 + i as usize))
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(i as f32 * HOUR / 2.0))
                        .h(px(HOUR / 2.0))
                        .when(i % 2 == 0, |s| s.border_t_1().border_color(p.line))
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.start_event(day_copy, start, window, cx);
                        }))
                        .test_support()
                        .into_any_element()
                })
                .collect();
            let blocks: Vec<AnyElement> = todays
                .iter()
                .filter(|e| !e.all_day)
                .map(|e| self.event_block(e, p, cx))
                .collect();
            let now = Local::now().naive_local();
            let now_line =
                (is_today && now.hour() >= DAY_START && now.hour() < DAY_END).then(|| {
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(y_of(
                            now.time().num_seconds_from_midnight() as f32 / 60.0,
                        )))
                        .h(px(2.0))
                        .bg(p.danger)
                        .into_any_element()
                });
            columns.push(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(grid_height))
                    .border_l_1()
                    .border_color(p.line)
                    .when(is_today, |c| c.bg(p.bg_subtle))
                    .children(slots)
                    .children(now_line)
                    .children(blocks)
                    .children(draft_block)
                    .into_any_element(),
            );
        }
        let legend: Vec<AnyElement> = [
            EventKind::Meeting,
            EventKind::Focus,
            EventKind::Personal,
            EventKind::Pending,
            EventKind::Hold,
        ]
        .into_iter()
        .map(|kind| {
            let (bg, line, _) = colours(kind, p);
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(10.0))
                        .rounded(px(3.0))
                        .bg(bg)
                        .border_1()
                        .border_color(line),
                )
                .child(kind.label())
                .into_any_element()
        })
        .collect();
        div()
            .id("week")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .test_support()
            .px(px(24.0))
            .pb(px(24.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(64.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(text::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(div().text_color(p.ink_3).child(range))
                    .child(div().flex_1())
                    .child(nav("prev-week", IconName::ChevronLeft, Some(-1), cx))
                    .child(
                        button("this-week", "Today  T".into(), false, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.calendar_week(None, cx)
                            }))
                            .test_support(),
                    )
                    .child(nav("next-week", IconName::ChevronRight, Some(1), cx)),
            )
            .child(div().flex().children(headers))
            .child(
                div()
                    .min_h(px(28.0))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_b_1()
                    .border_color(p.line)
                    .children(all_day),
            )
            .child(
                div()
                    .flex()
                    .child(
                        div()
                            .relative()
                            .w(px(56.0))
                            .flex_none()
                            .h(px(grid_height))
                            .children(hour_labels),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .children(columns)
                            .children(layer),
                    ),
            )
            .child(
                div()
                    .pt(px(14.0))
                    .flex()
                    .flex_wrap()
                    .gap(px(16.0))
                    .text_size(px(text::META))
                    .text_color(p.ink_3)
                    .children(legend)
                    .child(div().flex_1())
                    .child("Click an empty slot to add an event"),
            )
            .into_any_element()
    }

    fn event_block(&self, e: &Event, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let (bg, line, ink) = colours(e.kind, p);
        let start = local(e.start);
        let end = local(e.end);
        let top = y_of(start.time().num_seconds_from_midnight() as f32 / 60.0).max(0.0);
        let bottom = y_of(end.time().num_seconds_from_midnight() as f32 / 60.0)
            .min((DAY_END - DAY_START) as f32 * HOUR);
        let height = (bottom - top - 3.0).max(18.0);
        let selected = self
            .calendar
            .as_ref()
            .is_some_and(|c| c.detail == Some(e.id));
        let id = e.id;
        let dashed = matches!(e.kind, EventKind::Pending | EventKind::Hold);
        div()
            .id(("event", usize::try_from(e.id.0).unwrap_or_default()))
            .absolute()
            .occlude()
            .left(px(3.0))
            .right(px(3.0))
            .top(px(top + 1.0))
            .h(px(height))
            .px(px(8.0))
            .when(height >= 30.0, |b| b.py(px(3.0)))
            .when(height < 30.0, |b| {
                b.flex().items_center().line_height(px(14.0))
            })
            .overflow_hidden()
            .rounded(px(7.0))
            .bg(bg)
            .border_1()
            .border_color(line)
            .when(dashed, |b| b.border_dashed())
            .when(selected, |b| b.border_2().border_color(p.accent))
            .text_color(ink)
            .text_size(px(text::META))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(e.title.clone()),
            )
            .when(height >= 36.0, |b| {
                b.child(div().opacity(0.8).child(format!(
                    "{}–{}",
                    start.format("%H:%M"),
                    end.format("%H:%M")
                )))
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.show_event(Some(id), cx)))
            .test_support()
            .into_any_element()
    }

    /// A popover over the day columns: Monday to Wednesday open to the right,
    /// Thursday and Friday to the left, so none runs under the side panel.
    fn popover(top: f32, day_index: usize, p: &Palette) -> gpui_kit::Div {
        let column = 1.0 / 5.0;
        let d = day_index as f32;
        div()
            .absolute()
            .occlude()
            .top(px(
                top.clamp(0.0, (DAY_END - DAY_START) as f32 * HOUR - 220.0)
            ))
            .when(day_index <= 2, |pop| {
                pop.left(gpui_kit::relative(d * column)).ml(px(8.0))
            })
            .when(day_index > 2, |pop| {
                pop.right(gpui_kit::relative((4.0 - d) * column))
                    .mr(px(8.0))
            })
            .w(px(280.0))
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .rounded(px(radius::CARD))
            .border_1()
            .border_color(p.line_strong)
            .bg(p.bg)
    }

    fn detail_popover(
        &self,
        day_index: usize,
        day: chrono::NaiveDate,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = self.calendar.as_ref()?.detail?;
        let e = self.store.event(id).ok().flatten()?;
        if local(e.start).date() != day {
            return None;
        }
        let (_, line, _) = colours(e.kind, p);
        let who = if e.attendees.is_empty() {
            "Just you".to_owned()
        } else {
            e.attendees
                .iter()
                .map(|a| a.display())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let joinable = e
            .location
            .as_deref()
            .and_then(|l| l.split_whitespace().find(|w| w.starts_with("https://")))
            .map(str::to_owned);
        let top = y_of(local(e.start).time().num_seconds_from_midnight() as f32 / 60.0) + 24.0;
        Some(
            Self::popover(top, day_index, p)
                .id("event-detail")
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(10.0))
                        .child(
                            div()
                                .mt(px(4.0))
                                .size(px(10.0))
                                .flex_none()
                                .rounded(px(3.0))
                                .bg(line),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(e.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(text::META))
                                        .text_color(p.ink_3)
                                        .child(when_label(e.start, e.end)),
                                ),
                        )
                        .child(
                            div()
                                .id("close-detail")
                                .size(px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.0))
                                .text_color(p.ink_3)
                                .child(Icon::new(IconName::Close).size(px(14.0)))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.show_event(None, cx)
                                }))
                                .test_support(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .text_size(px(text::META))
                        .text_color(p.ink_2)
                        .child(e.location.clone().unwrap_or_else(|| "No location".into()))
                        .child(who),
                )
                .child(match e.kind {
                    EventKind::Pending => div()
                        .flex()
                        .gap(px(6.0))
                        .child(
                            button("detail-accept", "Accept".into(), true, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.rsvp(id, Some(Rsvp::Accepted), window, cx)
                                }))
                                .test_support(),
                        )
                        .child(
                            button("detail-decline", "Decline".into(), false, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.rsvp(id, Some(Rsvp::Declined), window, cx)
                                }))
                                .test_support(),
                        )
                        .into_any_element(),
                    EventKind::Hold => div()
                        .flex()
                        .gap(px(6.0))
                        .child(
                            button("detail-send", "Send invite".into(), true, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.send_invite(id, window, cx)
                                }))
                                .test_support(),
                        )
                        .child(
                            button("detail-release", "Release hold".into(), false, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.delete_event(id, cx)
                                }))
                                .test_support(),
                        )
                        .into_any_element(),
                    EventKind::Mine | EventKind::Focus | EventKind::Personal => div()
                        .child(
                            button("detail-delete", "Delete".into(), false, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.delete_event(id, cx)
                                }))
                                .test_support(),
                        )
                        .into_any_element(),
                    EventKind::Meeting | EventKind::Declined => match joinable {
                        Some(url) => div()
                            .child(
                                button("detail-join", "Join".into(), true, p)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        if let Err(error) =
                                            (this.opener)(std::path::Path::new(&url))
                                        {
                                            this.error = Some(format!(
                                                "Sift couldn't open the link: {error}"
                                            ));
                                        }
                                        cx.notify();
                                    }))
                                    .test_support(),
                            )
                            .into_any_element(),
                        None => div().into_any_element(),
                    },
                })
                .into_any_element(),
        )
    }

    fn draft_block(
        &self,
        day_index: usize,
        day: chrono::NaiveDate,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, AnyElement)> {
        let draft = self.calendar.as_ref()?.draft.as_ref()?;
        if draft.day != day {
            return None;
        }
        let top = y_of(draft.start as f32);
        let height = (draft.minutes as f32 / 60.0 * HOUR - 3.0).max(18.0);
        let start = NaiveTime::from_num_seconds_from_midnight_opt(draft.start * 60, 0)
            .unwrap_or(NaiveTime::MIN);
        let end = NaiveTime::from_num_seconds_from_midnight_opt(
            (draft.start + draft.minutes).min(DAY_END * 60) * 60,
            0,
        )
        .unwrap_or(NaiveTime::MIN);
        let typed = draft.title.read(cx).value().to_string();
        let lengths: Vec<AnyElement> = [(30u32, "30 min"), (60, "1 h"), (90, "1.5 h")]
            .into_iter()
            .enumerate()
            .map(|(i, (minutes, label))| {
                let on = draft.minutes == minutes;
                div()
                    .id(("length", i))
                    .h(px(24.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .text_size(px(text::META))
                    .text_color(if on { p.ink } else { p.ink_3 })
                    .when(on, |s| s.bg(p.bg).border_1().border_color(p.line))
                    .child(label)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.draft_length(minutes, cx)
                    }))
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let block = div()
            .absolute()
            .occlude()
            .left(px(3.0))
            .right(px(3.0))
            .top(px(top + 1.0))
            .h(px(height))
            .px(px(8.0))
            .rounded(px(7.0))
            .bg(p.accent)
            .text_color(p.accent_ink)
            .text_size(px(text::META))
            .font_weight(FontWeight::SEMIBOLD)
            .child(if typed.trim().is_empty() {
                "New event".to_owned()
            } else {
                typed
            })
            .into_any_element();
        Some((
            block,
            div()
                .child(
                    Self::popover(top + height + 6.0, day_index, p)
                        .id("quick-create")
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(format!(
                            "New event · {} {}–{}",
                            day.format("%a %-d"),
                            start.format("%H:%M"),
                            end.format("%H:%M")
                        )))
                        .child(Input::new(&draft.title).id("event-title"))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .text_size(px(text::META))
                                        .text_color(p.ink_3)
                                        .child("Length"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .p(px(2.0))
                                        .gap(px(2.0))
                                        .rounded(px(radius::BUTTON))
                                        .bg(p.bg_hover)
                                        .children(lengths),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(6.0))
                                .child(
                                    button("cancel-event", "Cancel".into(), false, p)
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.cancel_event(window, cx)
                                            },
                                        ))
                                        .test_support(),
                                )
                                .child(
                                    button("save-event", "Save  ↵".into(), true, p)
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.save_event(window, cx)
                                            },
                                        ))
                                        .test_support(),
                                ),
                        ),
                )
                .into_any_element(),
        ))
    }

    fn calendar_panel(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(c) = &self.calendar else {
            return div().into_any_element();
        };
        let hover = p.bg_hover;
        let (summary, offers) = c
            .offers
            .as_ref()
            .map(|(s, o, _)| (s.clone(), o.clone()))
            .unwrap_or_default();
        let hold = c.hold.and_then(|id| self.store.event(id).ok().flatten());
        let offer_rows: Vec<AnyElement> = offers
            .iter()
            .enumerate()
            .map(|(i, offer)| {
                let on = hold.as_ref().is_some_and(|h| local(h.start) == offer.start);
                div()
                    .id(("offer", i))
                    .p(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(radius::BUTTON))
                    .border_1()
                    .border_color(if on { p.accent } else { p.line_strong })
                    .when(on, |o| o.bg(p.accent_soft))
                    .when(!on, |o| o.hover(move |s| s.bg(hover)))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!(
                                "{} · {}–{}",
                                offer.start.format("%a %-d %b"),
                                offer.start.format("%H:%M"),
                                offer.end.format("%H:%M")
                            )))
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.ink_3)
                                    .child(offer.note.clone()),
                            ),
                    )
                    .when(on, |o| {
                        o.child(
                            Icon::new(IconName::Check)
                                .size(px(14.0))
                                .text_color(p.accent_text),
                        )
                    })
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.hold_offer(i, cx)),
                    )
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let pending = self.store.pending_invites(Utc::now()).unwrap_or_default();
        let waiting: Vec<AnyElement> = pending
            .iter()
            .take(3)
            .enumerate()
            .map(|(i, e)| {
                let id = e.id;
                let week = (crate::domain::schedule::monday_of(local(e.start).date())
                    - crate::domain::schedule::monday_of(Local::now().date_naive()))
                .num_weeks();
                card(p)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(e.title.clone()),
                            )
                            .child(div().text_size(px(text::META)).text_color(p.ink_3).child(
                                format!(
                                        "{}{}",
                                        when_label(e.start, e.end),
                                        e.organizer
                                            .as_ref()
                                            .map(|o| format!(" · from {}", o.display()))
                                            .unwrap_or_default()
                                    ),
                            ))
                            .child(
                                div()
                                    .text_size(px(text::META))
                                    .text_color(p.good)
                                    .child(self.fit(e)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(
                                button(("waiting-accept", i), "Accept".into(), true, p)
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.rsvp(id, Some(Rsvp::Accepted), window, cx)
                                        },
                                    ))
                                    .test_support(),
                            )
                            .child(
                                button(("waiting-decline", i), "Decline".into(), false, p)
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.rsvp(id, Some(Rsvp::Declined), window, cx)
                                        },
                                    ))
                                    .test_support(),
                            )
                            .child(
                                button(("waiting-show", i), "Show in week".into(), false, p)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        if let Some(c) = &mut this.calendar {
                                            c.week = week;
                                        }
                                        this.show_event(Some(id), cx);
                                    }))
                                    .test_support(),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        // The week at a glance.
        let events = self.week_events();
        let minutes_of = |kinds: &[EventKind]| -> i64 {
            events
                .iter()
                .filter(|e| kinds.contains(&e.kind) && !e.all_day)
                .map(|e| (e.end - e.start).num_minutes())
                .sum()
        };
        let meetings = minutes_of(&[
            EventKind::Meeting,
            EventKind::Pending,
            EventKind::Hold,
            EventKind::Mine,
        ]);
        let focus = minutes_of(&[EventKind::Focus]);
        let per_day: Vec<(String, i64)> = self
            .calendar_days()
            .iter()
            .map(|d| {
                let m = events
                    .iter()
                    .filter(|e| {
                        local(e.start).date() == *d
                            && !matches!(e.kind, EventKind::Focus | EventKind::Personal)
                            && !e.all_day
                    })
                    .map(|e| (e.end - e.start).num_minutes())
                    .sum();
                (d.format("%A").to_string(), m)
            })
            .collect();
        let busiest = per_day
            .iter()
            .max_by_key(|(_, m)| *m)
            .cloned()
            .unwrap_or_default();
        let lightest = per_day
            .iter()
            .min_by_key(|(_, m)| *m)
            .cloned()
            .unwrap_or_default();
        let keys: Vec<AnyElement> = [
            ("Previous / next week", "← →"),
            ("Today", "T"),
            ("New event", "N"),
            ("Close", "Esc"),
        ]
        .into_iter()
        .map(|(l, k)| {
            div()
                .flex()
                .justify_between()
                .text_size(px(text::META))
                .child(div().text_color(p.ink_2).child(l))
                .child(kbd(k, p))
                .into_any_element()
        })
        .collect();
        div()
            .id("calendar-panel")
            .w(px(PANEL_WIDTH))
            .flex_none()
            .overflow_y_scroll()
            .test_support()
            .p(px(16.0))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .bg(p.bg_subtle)
            .border_l_1()
            .border_color(p.line)
            .child(
                card(p)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(
                                Icon::new(crate::assets::Lucide::Sparkles)
                                    .size(px(14.0))
                                    .text_color(p.accent_text),
                            )
                            .child("Find a time"),
                    )
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child("Say it the way you would to a colleague"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(div().flex_1().child(Input::new(&c.ask).id("find-time")))
                            .child(
                                button("find-time-go", "Find".into(), false, p)
                                    .h(px(32.0))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.find_a_time(cx)
                                    }))
                                    .test_support(),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_2)
                            .child(summary),
                    )
                    .children(offer_rows)
                    .when_some(hold, |d, h| {
                        let id = h.id;
                        d.child(
                            div()
                                .p(px(10.0))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .rounded(px(radius::BUTTON))
                                .bg(p.accent_soft)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(text::META))
                                        .text_color(p.accent_text)
                                        .child(format!(
                                            "Holding {} on your calendar.{}",
                                            local(h.start).format("%a %-d %b"),
                                            if h.attendees.is_empty() {
                                                String::new()
                                            } else {
                                                format!(
                                                    " {} hasn't been invited yet.",
                                                    h.attendees[0].display()
                                                )
                                            }
                                        )),
                                )
                                .when(!h.attendees.is_empty(), |d| {
                                    d.child(
                                        button("send-invite", "Send invite".into(), true, p)
                                            .on_click(cx.listener(
                                                move |this, _: &ClickEvent, window, cx| {
                                                    this.send_invite(id, window, cx)
                                                },
                                            ))
                                            .test_support(),
                                    )
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(label("Waiting on you", p))
                    .children(waiting)
                    .when(pending.is_empty(), |d| {
                        d.child(
                            card(p).child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .text_color(p.ink_2)
                                    .child(
                                        Icon::new(IconName::CircleCheck)
                                            .size(px(16.0))
                                            .text_color(p.good),
                                    )
                                    .child("Nothing waiting on you."),
                            ),
                        )
                    }),
            )
            .child(
                card(p)
                    .child(label(
                        if c.week == 1 {
                            "Next week"
                        } else {
                            "This week"
                        },
                        p,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap(px(24.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(22.0))
                                            .font_weight(FontWeight::BOLD)
                                            .child(hours_label(meetings)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(text::META))
                                            .text_color(p.ink_3)
                                            .child("in meetings"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(22.0))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(p.good)
                                            .child(hours_label(focus)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(text::META))
                                            .text_color(p.ink_3)
                                            .child("protected focus"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(text::META))
                            .text_color(p.ink_2)
                            .child(format!(
                                "{} is your busiest day ({} of meetings). {} is the lightest ({}).",
                                busiest.0,
                                hours_label(busiest.1),
                                lightest.0,
                                hours_label(lightest.1)
                            )),
                    ),
            )
            .child(card(p).child(label("Keyboard", p)).children(keys))
            .into_any_element()
    }

    /// "You're free" or "Clashes with Design review".
    pub(super) fn fit(&self, e: &Event) -> String {
        let clash = self
            .store
            .events_between(e.start, e.end)
            .unwrap_or_default()
            .into_iter()
            .find(|o| o.id != e.id && !matches!(o.kind, EventKind::Declined));
        match clash {
            Some(o) => format!("Clashes with {}", o.title),
            None => "You're free".to_owned(),
        }
    }

    /// The meeting card at the top of an invitation in the reader.
    pub(super) fn meeting_card(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.open.as_ref()?;
        let e = self
            .store
            .event_for_thread(&open.summary.thread)
            .ok()
            .flatten()?;
        let id = e.id;
        let start = local(e.start);
        let fit = self.fit(&e);
        let clash = fit.starts_with("Clashes");
        let answered = e.rsvp.map(|r| match r {
            Rsvp::Accepted => "You're going",
            Rsvp::Tentative => "You said maybe",
            Rsvp::Declined => "You declined",
        });
        Some(
            div()
                .p(px(14.0))
                .flex()
                .items_center()
                .gap(px(14.0))
                .rounded(px(radius::CARD))
                .border_1()
                .border_color(p.line_strong)
                .bg(p.bg)
                .child(
                    div()
                        .w(px(52.0))
                        .flex_none()
                        .rounded(px(8.0))
                        .overflow_hidden()
                        .border_1()
                        .border_color(p.line_strong)
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .w_full()
                                .h(px(18.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(p.accent)
                                .text_color(p.accent_ink)
                                .text_size(px(10.0))
                                .font_weight(FontWeight::BOLD)
                                .child(start.format("%b").to_string().to_uppercase()),
                        )
                        .child(
                            div()
                                .h(px(30.0))
                                .flex()
                                .items_center()
                                .text_size(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(start.day().to_string()),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(text::BODY))
                                .child(e.title.clone()),
                        )
                        .child(div().text_color(p.ink_3).child(format!(
                            "{}{}",
                            when_label(e.start, e.end),
                            e.location.as_deref().map(|l| format!(" · {l}")).unwrap_or_default()
                        )))
                        .child(
                            div()
                                .text_size(px(text::META))
                                .text_color(if clash { p.warn } else { p.good })
                                .child(fit),
                        ),
                )
                .child(match answered {
                    None => div()
                        .flex()
                        .gap(px(6.0))
                        .child(
                            button("rsvp-accept", "Accept".into(), true, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.rsvp(id, Some(Rsvp::Accepted), window, cx)
                                }))
                                .test_support(),
                        )
                        .child(
                            button("rsvp-maybe", "Maybe".into(), false, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.rsvp(id, Some(Rsvp::Tentative), window, cx)
                                }))
                                .test_support(),
                        )
                        .child(
                            button("rsvp-decline", "Decline".into(), false, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.rsvp(id, Some(Rsvp::Declined), window, cx)
                                }))
                                .test_support(),
                        )
                        .into_any_element(),
                    Some(text) => div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_color(p.ink_2)
                                .font_weight(FontWeight::MEDIUM)
                                .child(text),
                        )
                        .child(
                            button("rsvp-change", "Change".into(), false, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.rsvp(id, None, window, cx)
                                }))
                                .test_support(),
                        )
                        .into_any_element(),
                })
                .into_any_element(),
        )
    }

    /// Accept and Decline inside a list row for an unanswered invitation.
    pub(super) fn row_rsvp(
        &self,
        thread: &crate::store::ThreadSummary,
        index: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let e = self.store.event_for_thread(&thread.thread).ok().flatten()?;
        if e.kind != EventKind::Pending {
            return None;
        }
        let id = e.id;
        Some(
            div()
                .flex()
                .gap(px(4.0))
                .child(
                    button(("row-accept", index), "Accept".into(), true, p)
                        .h(px(24.0))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.rsvp(id, Some(Rsvp::Accepted), window, cx);
                        }))
                        .test_support(),
                )
                .child(
                    button(("row-decline", index), "Decline".into(), false, p)
                        .h(px(24.0))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.rsvp(id, Some(Rsvp::Declined), window, cx);
                        }))
                        .test_support(),
                )
                .into_any_element(),
        )
    }

    /// Today's events for the sidebar.
    pub(super) fn today_agenda(&self, p: &Palette) -> Vec<AnyElement> {
        let today = Local::now().date_naive();
        let from = super::calendar::utc(today.and_time(NaiveTime::MIN));
        let to = from + chrono::Duration::days(1);
        self.store
            .events_between(from, to)
            .unwrap_or_default()
            .into_iter()
            .take(4)
            .map(|e| {
                let (_, line, _) = colours(e.kind, p);
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w(px(40.0))
                            .flex_none()
                            .text_size(px(text::META))
                            .text_color(p.ink_3)
                            .child(local(e.start).format("%H:%M").to_string()),
                    )
                    .child(
                        div()
                            .mt(px(3.0))
                            .w(px(3.0))
                            .h(px(14.0))
                            .flex_none()
                            .rounded_full()
                            .bg(line),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(text::META))
                            .child(e.title.clone()),
                    )
                    .into_any_element()
            })
            .collect()
    }
}
