//! Headless tests for AI in the window, with a fake assistant that answers
//! by prompt type, and for the New message dialog's recipients and drafts.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::SiftApp;
use super::ai_ui::AiState;
use super::chrome::Overlay;
use super::settings::SettingsPage;
use super::tests::{click, open, press, read};
use crate::ai::{AiError, AiProvider, Assistant, FakeAssistant};
use crate::domain::choices::AiFeature;

fn answer(prompt: &str) -> Result<String, AiError> {
    if prompt.contains("\"summary\": string") {
        Ok(r#"{"summary": "Priya needs your Q4 numbers by Monday.", "todos": [{"text": "Send Q4 numbers", "due": "Mon"}]}"#.into())
    } else if prompt.contains("\"drafts\"") {
        Ok(r#"{"drafts": [{"label": "On it", "text": "On it, you'll have them Monday."}, {"label": "Question", "text": "Quick question first."}]}"#.into())
    } else if prompt.contains("Rewrite it to be") {
        Ok(r#"{"text": "Will do."}"#.into())
    } else if prompt.contains("The reader asked") {
        Ok(r#"{"answer": "Tom needs a yes or no on the contract.", "source": 1}"#.into())
    } else if prompt.contains("What they want to say") {
        Ok(r#"{"subject": "Moving our 1:1", "body": "Hi Priya, can we move our 1:1 to Thursday?"}"#.into())
    } else {
        Err(AiError::Unreadable)
    }
}

fn with_fake(cx: &mut TestAppContext, view: &Entity<SiftApp>) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let fake = FakeAssistant(move |p: &str| {
        counter.fetch_add(1, Ordering::SeqCst);
        answer(p)
    });
    view.update(cx, |app, _| {
        app.use_assistant(Some(Arc::new(fake) as Arc<dyn Assistant>))
    });
    calls
}

fn failing(cx: &mut TestAppContext, view: &Entity<SiftApp>) {
    let fake = FakeAssistant(|_: &str| {
        Err(AiError::Failed {
            tool: "Claude Code",
            detail: "not signed in".into(),
        })
    });
    view.update(cx, |app, _| {
        app.use_assistant(Some(Arc::new(fake) as Arc<dyn Assistant>))
    });
}

fn open_thread(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<SiftApp>,
    subject: &str,
) {
    let index = read(cx, view, |a| {
        a.listed_subjects()
            .iter()
            .position(|s| s == subject)
            .unwrap()
    });
    click(cx, handle, ("thread-row", index));
}

fn reply_text(cx: &mut TestAppContext, view: &Entity<SiftApp>) -> String {
    cx.update(|cx| {
        view.read(cx)
            .reply_box
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()
    })
}

fn scroll_reader(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            "reader",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.0),
                gpui_kit::px(-2000.0),
            )),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_reader_summarises_lists_todos_and_drafts_replies(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let calls = with_fake(cx, &view);
    open_thread(cx, handle, &view, "Re: Q4 budget numbers");
    cx.run_until_parked();
    let insight = read(cx, &view, |a| {
        a.open.as_ref().and_then(|o| a.insight(&o.summary).cloned())
    });
    let Some(AiState::Ready(i)) = insight else {
        panic!("no insights")
    };
    assert_eq!(i.summary, "Priya needs your Q4 numbers by Monday.");
    assert_eq!(i.todos.len(), 1);

    click(cx, handle, ("todo", 0usize));
    let key = read(cx, &view, |a| a.todo_key(0).unwrap());
    assert!(read(cx, &view, |a| a.todo_done(&key)));
    click(cx, handle, ("todo", 0usize));
    assert!(!read(cx, &view, |a| a.todo_done(&key)));

    scroll_reader(cx, handle);
    click(cx, handle, ("draft", 0usize));
    assert_eq!(reply_text(cx, &view), "On it, you'll have them Monday.");
    click(cx, handle, ("tone", 0usize));
    cx.run_until_parked();
    assert_eq!(reply_text(cx, &view), "Will do.");

    // Answers are remembered: reopening asks nothing new.
    let before = calls.load(Ordering::SeqCst);
    press(cx, handle, &["escape"]);
    view.update(cx, |app, _| {
        app.insights.clear();
        app.drafts.clear();
    });
    open_thread(cx, handle, &view, "Re: Q4 budget numbers");
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), before);
}

#[gpui_kit::test]
fn summaries_replace_previews_in_the_list(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let calls = with_fake(cx, &view);
    press(cx, handle, &["a"]);
    cx.run_until_parked();
    let ready = read(cx, &view, |a| {
        a.threads
            .iter()
            .filter(|t| matches!(a.insight(t), Some(AiState::Ready(_))))
            .count()
    });
    assert_eq!(ready, 8);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        8,
        "one call per thread, one at a time"
    );
}

#[gpui_kit::test]
fn ask_your_inbox_answers_with_a_source(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    with_fake(cx, &view);
    press(cx, handle, &["ctrl-k"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.input("What does Tom need?", cx);
    })
    .unwrap();
    assert_eq!(
        read(cx, &view, SiftApp::palette_labels),
        ["What does Tom need?"]
    );
    press(cx, handle, &["enter"]);
    cx.run_until_parked();
    let answered = read(cx, &view, |a| match &a.overlay {
        Some(Overlay::Palette(p)) => match &p.answer {
            Some(AiState::Ready(found)) => Some((
                found.answer.answer.clone(),
                found.source.as_ref().map(|s| s.subject.clone()),
            )),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(
        answered,
        Some((
            "Tom needs a yes or no on the contract.".into(),
            Some("Vendor contract renewal".into())
        ))
    );
    click(cx, handle, "answer-source");
    assert_eq!(
        read(cx, &view, |a| a.open_subject().map(str::to_owned)),
        Some("Vendor contract renewal".into())
    );
}

#[gpui_kit::test]
fn write_it_drafts_a_new_message(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    with_fake(cx, &view);
    press(cx, handle, &["c"]);
    cx.update_window(handle, |_, window, cx| {
        let ai = view.read(cx).compose.as_ref().unwrap().ai_prompt.clone();
        ai.update(cx, |s, cx| {
            s.set_value("ask Priya to move our 1:1 to Thursday", window, cx)
        });
    })
    .unwrap();
    click(cx, handle, "write-it");
    assert!(read(cx, &view, |a| a.compose.as_ref().unwrap().writing));
    cx.run_until_parked();
    let (subject, body) = cx.update(|cx| {
        let f = view.read(cx).compose.as_ref().unwrap();
        (
            f.subject.read(cx).value().to_string(),
            f.body.read(cx).value().to_string(),
        )
    });
    assert_eq!(subject, "Moving our 1:1");
    assert!(body.starts_with("Hi Priya"));
}

#[gpui_kit::test]
fn failures_are_explained_and_can_be_retried(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    failing(cx, &view);
    open_thread(cx, handle, &view, "Re: Q4 budget numbers");
    cx.run_until_parked();
    assert!(read(cx, &view, |a| matches!(
        a.open.as_ref().and_then(|o| a.insight(&o.summary)),
        Some(AiState::Failed(why)) if why.contains("not signed in")
    )));
    with_fake(cx, &view);
    click(cx, handle, "retry-summary");
    cx.run_until_parked();
    assert!(read(cx, &view, |a| matches!(
        a.open.as_ref().and_then(|o| a.insight(&o.summary)),
        Some(AiState::Ready(_))
    )));

    failing(cx, &view);
    cx.update_window(handle, |_, window, cx| {
        let reply = view.read(cx).reply_box.clone().unwrap();
        reply.update(cx, |s, cx| s.set_value("ok", window, cx));
    })
    .unwrap();
    scroll_reader(cx, handle);
    click(cx, handle, ("tone", 2usize));
    cx.run_until_parked();
    assert!(
        read(cx, &view, |a| a.reply_error.clone())
            .unwrap()
            .contains("Couldn't rewrite")
    );

    press(cx, handle, &["escape", "escape"]);
    press(cx, handle, &["ctrl-k"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.input("Who is Kofi?", cx);
    })
    .unwrap();
    press(cx, handle, &["enter"]);
    cx.run_until_parked();
    assert!(read(cx, &view, |a| matches!(
        &a.overlay,
        Some(Overlay::Palette(p)) if matches!(p.answer, Some(AiState::Failed(_)))
    )));
    press(cx, handle, &["escape", "c"]);
    cx.update_window(handle, |_, window, cx| {
        let ai = view.read(cx).compose.as_ref().unwrap().ai_prompt.clone();
        ai.update(cx, |s, cx| s.set_value("say hi", window, cx));
    })
    .unwrap();
    click(cx, handle, "write-it");
    cx.run_until_parked();
    assert!(
        read(cx, &view, |a| a.compose_error().map(str::to_owned))
            .unwrap()
            .contains("Couldn't write")
    );
}

#[gpui_kit::test]
fn ai_stays_out_where_it_is_switched_off(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    let calls = with_fake(cx, &view);
    let work = read(cx, &view, |a| a.accounts[0].id);
    view.update(cx, |app, cx| {
        app.update_account(work, cx, |s| s.ai = false);
    });
    open_thread(cx, handle, &view, "Re: Q4 budget numbers");
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0, "this account has AI off");
    view.update(cx, |app, cx| {
        app.update_account(work, cx, |s| s.ai = true);
        app.toggle_ai_feature(AiFeature::Drafts, cx);
        app.toggle_ai_feature(AiFeature::Summaries, cx);
        app.toggle_ai_feature(AiFeature::Tasks, cx);
    });
    press(cx, handle, &["escape"]);
    open_thread(cx, handle, &view, "Re: Q4 budget numbers");
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0, "those helpers are off");

    // With no assistant at all, Write it says how to get one.
    view.update(cx, |app, _| app.use_assistant(None));
    press(cx, handle, &["escape", "c"]);
    cx.update_window(handle, |_, window, cx| {
        let ai = view.read(cx).compose.as_ref().unwrap().ai_prompt.clone();
        ai.update(cx, |s, cx| s.set_value("hi", window, cx));
    })
    .unwrap();
    click(cx, handle, "write-it");
    assert!(
        read(cx, &view, |a| a.compose_error().map(str::to_owned))
            .unwrap()
            .contains("isn't installed")
    );
}

#[gpui_kit::test]
fn the_ai_settings_page_picks_the_assistant(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    with_fake(cx, &view);
    press(cx, handle, &["ctrl-,"]);
    click(cx, handle, ("settings-nav", 2usize));
    assert_eq!(
        read(cx, &view, SiftApp::settings_page),
        Some(SettingsPage::Ai)
    );
    click(cx, handle, ("ai-provider", 2usize));
    assert_eq!(read(cx, &view, |a| a.ai_provider), AiProvider::Off);
    assert!(read(cx, &view, |a| a.ai.is_none()));
    assert_eq!(
        read(cx, &view, |a| a
            .store
            .preference(crate::store::Pref::AiProvider)
            .unwrap()),
        Some("off".into())
    );
    click(cx, handle, ("ai-provider", 1usize));
    assert_eq!(read(cx, &view, |a| a.ai_provider), AiProvider::Codex);
    click(cx, handle, ("ai-feature", 0usize));
    assert!(!read(cx, &view, |a| a.ai_features.on(AiFeature::Summaries)));
}

#[gpui_kit::test]
fn recipients_become_chips_with_suggestions(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, true);
    press(cx, handle, &["c"]);
    let typed = |cx: &mut TestAppContext, text: &str| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.input(text, cx);
        })
        .unwrap();
    };
    let chips = |cx: &mut TestAppContext| -> Vec<String> {
        read(cx, &view, |a| {
            a.compose
                .as_ref()
                .unwrap()
                .chips
                .iter()
                .map(|c| c.email().to_string())
                .collect()
        })
    };
    typed(cx, "pri");
    assert_eq!(
        read(cx, &view, |a| a.compose.as_ref().unwrap().suggestions.len()),
        1
    );
    press(cx, handle, &["down", "up", "enter"]);
    assert_eq!(chips(cx), ["priya@company.com"]);
    typed(cx, "x@y.com,");
    assert_eq!(chips(cx), ["priya@company.com", "x@y.com"]);
    typed(cx, "bad,");
    assert!(
        read(cx, &view, |a| a.compose_error().map(str::to_owned))
            .unwrap()
            .contains("bad")
    );
    cx.update_window(handle, |_, window, cx| {
        let to = view.read(cx).compose.as_ref().unwrap().to.clone();
        to.update(cx, |s, cx| s.set_value("", window, cx));
    })
    .unwrap();
    press(cx, handle, &["backspace"]);
    assert_eq!(chips(cx), ["priya@company.com"]);
    click(cx, handle, "compose-to");
    typed(cx, "kofi");
    click(cx, handle, ("suggestion", 0usize));
    assert_eq!(chips(cx), ["priya@company.com", "kofi@company.com"]);
    click(cx, handle, ("remove-recipient", 0usize));
    assert_eq!(chips(cx), ["kofi@company.com"]);
    assert_eq!(
        cx.update(|cx| view.read(cx).compose.as_ref().unwrap().to_text(cx)),
        "kofi@company.com"
    );

    // Closing keeps the draft; Discard throws it away.
    click(cx, handle, "compose-close");
    assert!(!read(cx, &view, SiftApp::is_composing));
    press(cx, handle, &["c"]);
    assert_eq!(chips(cx), ["kofi@company.com"]);
    click(cx, handle, "compose-discard");
    press(cx, handle, &["c"]);
    assert!(chips(cx).is_empty());
}
