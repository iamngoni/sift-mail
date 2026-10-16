//! The CLI runner, exercised with small scripts that stand in for the real
//! tools: one echoes its input, one fails, one is too slow.

use std::path::PathBuf;
use std::process::Command;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sift-ai-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a script that runs `body` and returns its path.
fn script(dir: &Path, name: &str, windows: &str, unix: &str) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join(format!("{name}.cmd"));
        std::fs::write(&path, format!("@echo off\r\n{windows}\r\n")).unwrap();
        path
    } else {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{unix}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
}

#[test]
fn providers_round_trip_and_explain_themselves() {
    for p in AiProvider::ALL {
        assert_eq!(AiProvider::from_key(p.key()), p);
        assert!(!p.label().is_empty() && !p.setup_hint().is_empty());
    }
    assert_eq!(AiProvider::from_key("?"), AiProvider::ClaudeCode);
    assert_eq!(AiProvider::Off.program(), None);
    assert!(CliAssistant::find(AiProvider::Off).is_none());
    assert!(CliAssistant::args(AiProvider::Off, Path::new("x")).is_empty());
    let claude = CliAssistant::args(AiProvider::ClaudeCode, Path::new("x"));
    assert!(claude.contains(&"-p".to_owned()) && claude.contains(&"--disallowedTools".to_owned()));
    let codex = CliAssistant::args(AiProvider::Codex, Path::new("out.txt"));
    assert_eq!(
        codex.last().map(String::as_str),
        Some("-"),
        "prompt comes on stdin"
    );
    assert!(codex.contains(&"read-only".to_owned()));
    let err = AiError::Failed {
        tool: "Codex",
        detail: "not signed in".into(),
    };
    assert_eq!(err.to_string(), "Codex didn't answer: not signed in");
}

#[test]
fn programs_are_found_on_the_path_or_in_usual_folders() {
    let dir = scratch("find");
    let name = if cfg!(windows) {
        "claude.cmd"
    } else {
        "claude"
    };
    std::fs::write(dir.join(name), "").unwrap();
    let path = std::env::join_paths([dir.clone()]).unwrap();
    assert_eq!(find_program("claude", &path, &[]), Some(dir.join(name)));
    assert_eq!(
        find_program(
            "claude",
            std::ffi::OsStr::new(""),
            std::slice::from_ref(&dir)
        ),
        Some(dir.join(name))
    );
    assert_eq!(find_program("codex", &path, &[]), None);
    // Whatever is or isn't installed here, looking must not panic.
    let _ = CliAssistant::find(AiProvider::ClaudeCode).map(|a| a.program().to_path_buf());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_returns_output_and_reports_failures() {
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let echo = if cfg!(windows) { "more" } else { "cat" };
    let out = run(
        Command::new(shell)
            .args([flag, echo])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        "hello there",
        Duration::from_secs(20),
        "Test",
    )
    .unwrap();
    assert!(out.contains("hello there"));

    let failed = run(
        Command::new(shell)
            .args([flag, "echo please sign in 1>&2 && exit 3"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        "",
        Duration::from_secs(20),
        "Test",
    )
    .unwrap_err();
    assert!(
        matches!(failed, AiError::Failed { ref detail, .. } if detail.contains("please sign in"))
    );

    let quiet = run(
        Command::new(shell)
            .args([flag, "exit 2"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        "",
        Duration::from_secs(20),
        "Test",
    )
    .unwrap_err();
    assert!(
        matches!(quiet, AiError::Failed { ref detail, .. } if detail == "it exited with an error")
    );

    let slow = if cfg!(windows) {
        "ping -n 6 127.0.0.1 >nul"
    } else {
        "sleep 5"
    };
    let timed_out = run(
        Command::new(shell)
            .args([flag, slow])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        "",
        Duration::from_millis(300),
        "Test",
    )
    .unwrap_err();
    assert_eq!(timed_out, AiError::TimedOut);

    let missing = run(
        &mut Command::new("sift-no-such-program-xyz"),
        "",
        Duration::from_secs(1),
        "Test",
    );
    assert_eq!(missing, Err(AiError::NotInstalled("Test")));
}

#[test]
fn a_cli_assistant_answers_through_its_program() {
    let dir = scratch("cli");
    let echo = script(&dir, "claude", "more", "cat");
    let claude = CliAssistant::at(AiProvider::ClaudeCode, echo, Duration::from_secs(20));
    assert_eq!(claude.name(), "Claude Code");
    assert_eq!(
        claude.complete("{\"summary\": \"hi\"}").unwrap(),
        "{\"summary\": \"hi\"}"
    );

    let silent = script(&dir, "quiet", "rem nothing", "true");
    let quiet = CliAssistant::at(AiProvider::ClaudeCode, silent, Duration::from_secs(20));
    assert_eq!(quiet.complete("x"), Err(AiError::Unreadable));

    // Codex writes its final message to a file; with no file, stdout is used.
    let codex_echo = script(&dir, "codex", "more", "cat");
    let codex = CliAssistant::at(AiProvider::Codex, codex_echo, Duration::from_secs(20));
    assert_eq!(codex.complete("answer").unwrap(), "answer");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fake_assistant_answers_from_its_closure() {
    let fake = FakeAssistant(|p: &str| Ok(format!("echo: {p}")));
    assert_eq!(fake.name(), "Test assistant");
    assert_eq!(fake.complete("x").unwrap(), "echo: x");
}
