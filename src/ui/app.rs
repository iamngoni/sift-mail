//! The main Sift window: title bar with view tabs, the inbox area and the
//! floating toolbar.
//!
//! There is no account connection yet, so the inbox shows an honest empty
//! state rather than sample mail. Counts are zero until sync exists.

use gpui_kit::component::{Icon, IconName, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FocusHandle, FontWeight, KeyDownEvent, SharedString,
    TestSupportExt as _, Window, div, px,
};

use crate::brand;
use crate::domain::TriageView;
use crate::theme::tokens::{Palette, radius, size, text};
use crate::theme::{self, Appearance, SiftTheme};

/// Root view of the main window.
pub struct SiftApp {
    view: TriageView,
    focus: FocusHandle,
}

impl SiftApp {
    /// Builds the view and gives it keyboard focus so view shortcuts work at once.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            view: TriageView::default(),
            focus,
        }
    }

    /// The selected inbox view.
    pub fn view(&self) -> TriageView {
        self.view
    }

    /// Selects an inbox view.
    pub fn select(&mut self, view: TriageView, cx: &mut Context<Self>) {
        if self.view != view {
            self.view = view;
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let digit = keystroke.key.parse::<u8>().ok();
        if let Some(view) = digit.and_then(TriageView::from_shortcut) {
            self.select(view, cx);
            cx.stop_propagation();
        }
    }

    fn title_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs: Vec<_> = TriageView::ALL
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
            .text_size(px(text::UI))
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
                    .child("0"),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(view, cx)))
            .test_support()
            .into_any_element()
    }

    fn empty_state(&self, p: &Palette) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.0))
            .max_w(px(420.0))
            .child(
                div()
                    .size(px(56.0))
                    .rounded_full()
                    .bg(p.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::Inbox)
                            .size(px(24.0))
                            .text_color(p.accent_text),
                    ),
            )
            .child(
                div()
                    .text_size(px(text::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(empty_title(self.view)),
            )
            .child(
                div()
                    .text_size(px(text::UI))
                    .text_color(p.ink_3)
                    .text_center()
                    .child(
                        "Welcome to Sift Mail. Connecting accounts is the next piece being built, \
                         so there is nothing to sift yet. Try the views with 1 to 6, or switch \
                         the theme below.",
                    ),
            )
    }

    fn toolbar(
        &self,
        p: &Palette,
        appearance: Appearance,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover = p.bg_hover;
        let label: SharedString = format!("Theme: {}", appearance.label()).into();
        div()
            .absolute()
            .bottom(px(16.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
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
                            .child(label)
                            .on_click(cx.listener(|_, _: &ClickEvent, window, cx| {
                                let next = SiftTheme::get(cx).appearance().next();
                                theme::apply(next, Some(window), cx);
                                cx.notify();
                            }))
                            .test_support(),
                    ),
            )
    }
}

impl Render for SiftApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sift = *SiftTheme::get(cx);
        let p = *sift.palette();
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
            .child(
                div()
                    .relative()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(24.0))
                    .child(self.empty_state(&p))
                    .child(self.toolbar(&p, sift.appearance(), cx)),
            )
    }
}

fn view_icon(view: TriageView) -> IconName {
    match view {
        TriageView::All => IconName::Inbox,
        TriageView::NeedsYou => IconName::CircleCheck,
        TriageView::Meetings => IconName::Calendar,
        TriageView::Updates => IconName::Bell,
        TriageView::Newsletters => IconName::BookOpen,
        TriageView::Other => IconName::Ellipsis,
    }
}

fn appearance_icon(appearance: Appearance) -> IconName {
    match appearance {
        Appearance::Light => IconName::Sun,
        Appearance::Dark => IconName::Moon,
        Appearance::System => IconName::Palette,
    }
}

fn empty_title(view: TriageView) -> String {
    match view {
        TriageView::All => "No mail to sift yet".to_owned(),
        other => format!("Nothing in {} yet", other.label()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::open_main_window;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;

    fn open(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<SiftApp>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let view = open_main_window(cx).unwrap();
            let handle = *cx.windows().first().unwrap();
            (handle, view)
        })
    }

    #[gpui_kit::test]
    fn clicking_a_tab_selects_its_view(cx: &mut TestAppContext) {
        let (handle, view) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(TriageView::Meetings.id(), cx);
        })
        .unwrap();
        assert_eq!(
            view.read_with(cx, |app, _| app.view()),
            TriageView::Meetings
        );
    }

    #[gpui_kit::test]
    fn number_keys_switch_views(cx: &mut TestAppContext) {
        let (handle, view) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press("4", cx);
        })
        .unwrap();
        assert_eq!(view.read_with(cx, |app, _| app.view()), TriageView::Updates);
        cx.update_window(handle, |_, window, cx| {
            window.press("9", cx);
            window.press("ctrl-2", cx);
        })
        .unwrap();
        assert_eq!(
            view.read_with(cx, |app, _| app.view()),
            TriageView::Updates,
            "unbound digits and modified keys leave the view alone"
        );
    }

    #[gpui_kit::test]
    fn theme_button_cycles_appearance(cx: &mut TestAppContext) {
        let (handle, _) = open(cx);
        for expected in [Appearance::Light, Appearance::Dark, Appearance::System] {
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.click("toggle-theme", cx);
            })
            .unwrap();
            cx.update(|cx| assert_eq!(SiftTheme::get(cx).appearance(), expected));
        }
    }

    #[gpui_kit::test]
    fn selecting_the_current_view_is_a_no_op(cx: &mut TestAppContext) {
        let (handle, view) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |app, cx| app.select(TriageView::All, cx));
            window.render_frame(cx);
        })
        .unwrap();
        assert_eq!(view.read_with(cx, |app, _| app.view()), TriageView::All);
    }

    #[test]
    fn empty_titles_name_the_view() {
        assert_eq!(empty_title(TriageView::All), "No mail to sift yet");
        assert_eq!(
            empty_title(TriageView::NeedsYou),
            "Nothing in Needs you yet"
        );
        for view in TriageView::ALL {
            let _ = view_icon(view);
        }
        for appearance in [Appearance::Light, Appearance::Dark, Appearance::System] {
            let _ = appearance_icon(appearance);
        }
    }
}
