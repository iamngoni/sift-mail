//! A one-shot HTTP listener on 127.0.0.1 that receives the OAuth redirect
//! (RFC 8252 §7.3). It binds an OS-chosen port, accepts until it gets a
//! request for the callback path, answers with a small page, and closes.
//! It only ever reads the request line; bodies and other paths are ignored.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use super::OAuthError;

/// The path the provider redirects to.
pub const CALLBACK_PATH: &str = "/callback";

/// What the provider sent back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    /// Success: an authorization code and the echoed state.
    Code {
        /// The authorization code.
        code: String,
        /// Must equal the state Sift sent.
        state: String,
    },
    /// The provider reported an error.
    Error {
        /// OAuth error code, such as `access_denied`.
        code: String,
        /// Human description, often with a provider-specific code inside.
        description: String,
    },
}

/// The bound listener.
pub struct Listener {
    inner: TcpListener,
    port: u16,
}

impl Listener {
    /// Binds 127.0.0.1 on a free port.
    pub fn bind() -> Result<Self, OAuthError> {
        let inner =
            TcpListener::bind(("127.0.0.1", 0)).map_err(|e| OAuthError::Network(e.to_string()))?;
        inner
            .set_nonblocking(true)
            .map_err(|e| OAuthError::Network(e.to_string()))?;
        let port = inner
            .local_addr()
            .map_err(|e| OAuthError::Network(e.to_string()))?
            .port();
        Ok(Self { inner, port })
    }

    /// The redirect URI to register the request with.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}{CALLBACK_PATH}", self.port)
    }

    /// Waits for the callback, polling `cancelled` so the UI can abort.
    pub fn wait(
        &self,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Callback, OAuthError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled() {
                return Err(OAuthError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(OAuthError::TimedOut);
            }
            match self.inner.accept() {
                Ok((stream, _)) => {
                    if let Some(callback) = handle(stream)? {
                        return Ok(callback);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(OAuthError::Network(e.to_string())),
            }
        }
    }
}

/// Reads one request. Returns `None` for requests that aren't the callback
/// (browsers also ask for /favicon.ico).
fn handle(stream: TcpStream) -> Result<Option<Callback>, OAuthError> {
    stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(Duration::from_secs(5))))
        .map_err(|e| OAuthError::Network(e.to_string()))?;
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|e| OAuthError::Network(e.to_string()))?;
    let target = line.split_whitespace().nth(1).unwrap_or_default();
    let callback = parse_target(target);
    let (status, page) = match &callback {
        Some(Callback::Code { .. }) => (
            "200 OK",
            page(
                "You're signed in",
                "You can close this tab and go back to Sift.",
            ),
        ),
        Some(Callback::Error { .. }) => (
            "200 OK",
            page(
                "Sign-in didn't finish",
                "Go back to Sift to see what happened.",
            ),
        ),
        None => ("404 Not Found", String::new()),
    };
    let mut out = &stream;
    // The browser may already have gone; a failed write doesn't change the result.
    let _ = write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    Ok(callback)
}

/// Parses the request target, such as `/callback?code=…&state=…`.
pub fn parse_target(target: &str) -> Option<Callback> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return None;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = String::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => description = value.into_owned(),
            _ => {}
        }
    }
    match (error, code, state) {
        (Some(code), _, _) => Some(Callback::Error { code, description }),
        (None, Some(code), Some(state)) => Some(Callback::Code { code, state }),
        _ => Some(Callback::Error {
            code: "invalid_request".into(),
            description: "missing code or state".into(),
        }),
    }
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>{title} · Sift Mail</title>\
         <body style=\"font-family:system-ui,sans-serif;display:grid;place-items:center;height:90vh;color:#18181B\">\
         <div style=\"text-align:center\"><h1 style=\"font-size:22px\">{title}</h1><p style=\"color:#6B6B74\">{body}</p></div>"
    )
}
