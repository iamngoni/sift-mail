//! AI through the assistant the user already has.
//!
//! Sift has no API keys and no sign-in of its own for AI. It runs the user's
//! installed Claude Code (`claude -p`) or Codex (`codex exec`) as a
//! subprocess, so the official tool handles their subscription. Each call
//! runs in an empty temporary folder with tools restricted, gets its prompt
//! on standard input, and has a time limit.
//!
//! Everything above this module talks to the [`Assistant`] trait, so tests use
//! a fake and the UI never knows which tool answered.

pub mod prompts;

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one AI call may take.
pub const TIMEOUT: Duration = Duration::from_secs(90);

/// Why an AI call didn't produce an answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    /// The chosen assistant isn't installed.
    #[error("{0} isn't installed on this computer")]
    NotInstalled(&'static str),
    /// The assistant ran but failed, often because it isn't signed in.
    #[error("{tool} didn't answer: {detail}")]
    Failed {
        /// Which tool.
        tool: &'static str,
        /// Its error output, trimmed.
        detail: String,
    },
    /// No answer within [`TIMEOUT`].
    #[error("the assistant took too long to answer")]
    TimedOut,
    /// The answer wasn't in the expected shape.
    #[error("the assistant's answer couldn't be read")]
    Unreadable,
}

/// Something that turns a prompt into text.
pub trait Assistant: Send + Sync {
    /// Name shown to the user, such as "Claude Code".
    fn name(&self) -> &'static str;
    /// Answers one prompt. Blocking; run off the UI thread.
    fn complete(&self, prompt: &str) -> Result<String, AiError>;
}

/// Which assistant Sift uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiProvider {
    /// Anthropic's Claude Code CLI.
    #[default]
    ClaudeCode,
    /// OpenAI's Codex CLI.
    Codex,
    /// No AI at all.
    Off,
}

impl AiProvider {
    /// Choices in Settings order.
    pub const ALL: [AiProvider; 3] = [Self::ClaudeCode, Self::Codex, Self::Off];

    /// Stored form.
    pub fn key(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Off => "off",
        }
    }

    /// Parses [`AiProvider::key`], defaulting to Claude Code.
    pub fn from_key(key: &str) -> Self {
        match key {
            "codex" => Self::Codex,
            "off" => Self::Off,
            _ => Self::ClaudeCode,
        }
    }

    /// Name shown in Settings.
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Off => "Off",
        }
    }

    /// The program to look for.
    pub fn program(self) -> Option<&'static str> {
        match self {
            Self::ClaudeCode => Some("claude"),
            Self::Codex => Some("codex"),
            Self::Off => None,
        }
    }

    /// How to get it and sign in, for Settings.
    pub fn setup_hint(self) -> &'static str {
        match self {
            Self::ClaudeCode => {
                "Install Claude Code from claude.com/claude-code, run `claude` once and sign in with your Claude subscription."
            }
            Self::Codex => {
                "Install Codex (npm install -g @openai/codex), run `codex` once and sign in with your ChatGPT subscription."
            }
            Self::Off => "Sift won't send mail to any AI.",
        }
    }
}

/// Finds `program` on `path` (a PATH-style list), trying the Windows
/// extensions npm and installers use.
pub fn find_program(program: &str, path: &std::ffi::OsStr, extra: &[PathBuf]) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat"]
            .iter()
            .map(|ext| format!("{program}.{ext}"))
            .collect()
    } else {
        vec![program.to_owned()]
    };
    std::env::split_paths(path)
        .chain(extra.iter().cloned())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| candidate.is_file())
}

/// Folders installers put these tools in that may not be on PATH yet.
fn usual_folders() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
    }
    if let Some(data) = dirs::data_dir() {
        dirs.push(data.join("npm"));
    }
    dirs
}

/// An installed CLI assistant.
#[derive(Debug, Clone)]
pub struct CliAssistant {
    provider: AiProvider,
    program: PathBuf,
    timeout: Duration,
}

impl CliAssistant {
    /// Finds the provider's CLI, or `None` if it isn't installed.
    pub fn find(provider: AiProvider) -> Option<Self> {
        let program = provider.program()?;
        let path = std::env::var_os("PATH").unwrap_or_default();
        find_program(program, &path, &usual_folders()).map(|program| Self {
            provider,
            program,
            timeout: TIMEOUT,
        })
    }

    /// Uses a specific program, for tests and unusual installs.
    pub fn at(provider: AiProvider, program: PathBuf, timeout: Duration) -> Self {
        Self {
            provider,
            program,
            timeout,
        }
    }

    /// Where the CLI lives.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The fixed arguments: non-interactive, no tools, prompt on stdin.
    pub fn args(provider: AiProvider, output: &Path) -> Vec<String> {
        match provider {
            AiProvider::ClaudeCode => vec![
                "-p".into(),
                "--output-format".into(),
                "text".into(),
                "--max-turns".into(),
                "1".into(),
                "--disallowedTools".into(),
                "Bash,Edit,Write,NotebookEdit,WebFetch,WebSearch,Read,Glob,Grep".into(),
                "Follow the instructions on standard input.".into(),
            ],
            AiProvider::Codex => vec![
                "exec".into(),
                "--skip-git-repo-check".into(),
                "--sandbox".into(),
                "read-only".into(),
                "--color".into(),
                "never".into(),
                "--output-last-message".into(),
                output.to_string_lossy().into_owned(),
                "-".into(),
            ],
            AiProvider::Off => Vec::new(),
        }
    }
}

impl Assistant for CliAssistant {
    fn name(&self) -> &'static str {
        self.provider.label()
    }

    fn complete(&self, prompt: &str) -> Result<String, AiError> {
        let tool = self.provider.label();
        let work = tempdir()?;
        let output = work.join("answer.txt");
        let args = Self::args(self.provider, &output);
        let is_script = self
            .program
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
        let mut command = if is_script {
            // npm installs .cmd shims, which only cmd.exe can run. The
            // arguments are fixed above; the prompt never reaches the shell.
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&self.program).args(&args);
            c
        } else {
            let mut c = Command::new(&self.program);
            c.args(&args);
            c
        };
        let result = run(
            command
                .current_dir(&work)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
            prompt,
            self.timeout,
            tool,
        );
        let answer = result.and_then(|stdout| {
            if self.provider == AiProvider::Codex {
                std::fs::read_to_string(&output).or(Ok(stdout))
            } else {
                Ok(stdout)
            }
        });
        let _ = std::fs::remove_dir_all(&work);
        let answer = answer?.trim().to_owned();
        if answer.is_empty() {
            Err(AiError::Unreadable)
        } else {
            Ok(answer)
        }
    }
}

/// A fresh empty folder for one call, so the assistant has nothing to read.
fn tempdir() -> Result<PathBuf, AiError> {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let unique = format!("sift-ai-{}-{n}-{:x}", std::process::id(), rand_suffix());
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).map_err(|e| AiError::Failed {
        tool: "Sift",
        detail: format!("couldn't make a working folder: {e}"),
    })?;
    Ok(dir)
}

fn rand_suffix() -> u64 {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(0);
    h.finish()
}

/// Runs a prepared command: writes `input` to stdin, waits up to `timeout`,
/// and returns stdout. Kills the process on timeout.
pub fn run(
    command: &mut Command,
    input: &str,
    timeout: Duration,
    tool: &'static str,
) -> Result<String, AiError> {
    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AiError::NotInstalled(tool),
        _ => AiError::Failed {
            tool,
            detail: e.to_string(),
        },
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_owned();
        std::thread::spawn(move || {
            // A tool that exits early closes its stdin; that's not our error.
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.read_to_string(&mut s);
        }
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(err) = stderr.as_mut() {
            let _ = err.read_to_string(&mut s);
        }
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AiError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                return Err(AiError::Failed {
                    tool,
                    detail: e.to_string(),
                });
            }
        }
    };
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        let detail = err
            .lines()
            .chain(out.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("it exited with an error")
            .chars()
            .take(200)
            .collect();
        Err(AiError::Failed { tool, detail })
    }
}

/// An assistant that answers from a closure, for tests and previews.
pub struct FakeAssistant<F>(pub F);

impl<F> Assistant for FakeAssistant<F>
where
    F: Fn(&str) -> Result<String, AiError> + Send + Sync,
{
    fn name(&self) -> &'static str {
        "Test assistant"
    }

    fn complete(&self, prompt: &str) -> Result<String, AiError> {
        (self.0)(prompt)
    }
}

#[cfg(test)]
mod tests;
