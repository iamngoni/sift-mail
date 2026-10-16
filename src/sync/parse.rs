//! Turns raw RFC 5322 bytes from a server into an [`IncomingMessage`].
//!
//! This is the trust boundary for mail content: everything downstream gets a
//! validated sender, a UTC time, plain text, a thread key and triage signals.
//! A message with no usable sender is rejected here rather than half-stored.

use chrono::{DateTime, Utc};
use mail_parser::{HeaderValue, MessageParser, MimeHeaders};

use crate::domain::{AccountId, Contact, EmailAddress, IncomingMessage, Signals};

/// Why a raw message could not be stored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The bytes had no recognisable headers.
    #[error("not an email message")]
    NotAnEmail,
    /// No From address, or one that isn't a valid address.
    #[error("message has no valid sender")]
    NoSender,
}

/// One attached file, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentData {
    /// File name as sent, or `attachment-<n>` when it had none.
    pub name: String,
    /// MIME type, such as `application/pdf`.
    pub mime: String,
    /// Decoded contents.
    pub bytes: Vec<u8>,
}

/// The attachments in `raw`, decoded. Unparseable mail has none.
pub fn attachments(raw: &[u8]) -> Vec<AttachmentData> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    message
        .attachments()
        .enumerate()
        .map(|(i, part)| AttachmentData {
            name: part
                .attachment_name()
                .map(str::to_owned)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| format!("attachment-{}", i + 1)),
            mime: part.content_type().map_or_else(
                || "application/octet-stream".to_owned(),
                |ct| match ct.subtype() {
                    Some(sub) => format!("{}/{sub}", ct.ctype()),
                    None => ct.ctype().to_owned(),
                },
            ),
            bytes: part.contents().to_vec(),
        })
        .collect()
}

/// Everything known about a message besides its bytes.
#[derive(Debug, Clone)]
pub struct Envelope<'a> {
    /// Account it belongs to.
    pub account: AccountId,
    /// Provider id, such as `"<uidvalidity>:<uid>"` for IMAP.
    pub remote_id: String,
    /// The account's own address, to tell direct mail from CC.
    pub me: &'a EmailAddress,
    /// Server's read flag.
    pub is_read: bool,
    /// Server's arrival time, used when the Date header is missing or bogus.
    pub internal_date: Option<DateTime<Utc>>,
}

/// Parses `raw` into a message ready for the store.
pub fn parse(raw: &[u8], envelope: &Envelope<'_>) -> Result<IncomingMessage, ParseError> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(ParseError::NotAnEmail)?;

    let sender = message
        .from()
        .and_then(|from| from.first())
        .ok_or(ParseError::NoSender)?;
    let address = sender
        .address()
        .and_then(|a| EmailAddress::parse(a).ok())
        .ok_or(ParseError::NoSender)?;
    let from = Contact::new(sender.name(), address);

    let subject = message.subject().unwrap_or_default().trim().to_owned();
    let body_text = message
        .body_text(0)
        .map(|text| text.into_owned())
        .or_else(|| message.body_html(0).map(|html| html_to_text(&html)))
        .unwrap_or_default();

    let received_at = message
        .date()
        .and_then(|d| DateTime::from_timestamp(d.to_timestamp(), 0))
        .or(envelope.internal_date)
        .unwrap_or_else(Utc::now);

    let addressed_to_me = message.to().is_some_and(|to| {
        to.iter().any(|addr| {
            addr.address()
                .is_some_and(|a| a.eq_ignore_ascii_case(envelope.me.as_str()))
        })
    });
    let signals = Signals {
        mailing_list: has_value(message.list_unsubscribe()) || has_value(message.list_id()),
        calendar_invite: message
            .parts
            .iter()
            .any(|part| part.is_content_type("text", "calendar")),
        auto_submitted: is_automatic(
            message.header_raw("Auto-Submitted"),
            message.header_raw("Precedence"),
        ),
        addressed_to_me,
    };

    let thread_key = thread_key(
        first_id(message.references()),
        first_id(message.in_reply_to()),
        message.message_id(),
        &envelope.remote_id,
    );

    Ok(IncomingMessage {
        account: envelope.account,
        remote_id: envelope.remote_id.clone(),
        thread_key,
        from,
        subject,
        body_text,
        received_at,
        is_read: envelope.is_read,
        has_attachments: message.attachment_count() > 0,
        signals,
        message_id: message
            .message_id()
            .map(clean_id)
            .filter(|id| !id.is_empty()),
        references: all_ids(message.references()),
    })
}

/// Picks the conversation key: the root of `References`, else the parent in
/// `In-Reply-To`, else the message's own `Message-ID`, else a key unique to
/// this message so it forms its own thread.
pub fn thread_key(
    references_root: Option<&str>,
    in_reply_to: Option<&str>,
    message_id: Option<&str>,
    remote_id: &str,
) -> String {
    references_root
        .or(in_reply_to)
        .or(message_id)
        .map(|id| id.trim_matches(['<', '>', ' ']).to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| format!("local:{remote_id}"))
}

/// Strips angle brackets and spaces from a message id.
fn clean_id(id: &str) -> String {
    id.trim_matches(['<', '>', ' ']).to_owned()
}

fn all_ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(text) => vec![clean_id(text)],
        HeaderValue::TextList(list) => list.iter().map(|t| clean_id(t)).collect(),
        _ => Vec::new(),
    }
}

fn first_id<'a>(value: &'a HeaderValue<'a>) -> Option<&'a str> {
    match value {
        HeaderValue::Text(text) => Some(text.as_ref()),
        HeaderValue::TextList(list) => list.first().map(|t| t.as_ref()),
        _ => None,
    }
}

fn has_value(value: &HeaderValue<'_>) -> bool {
    !matches!(value, HeaderValue::Empty)
}

/// RFC 3834 `Auto-Submitted` (anything but `no`) or bulk `Precedence`.
fn is_automatic(auto_submitted: Option<&str>, precedence: Option<&str>) -> bool {
    let auto = auto_submitted
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| !v.is_empty() && v != "no");
    let bulk = precedence
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| matches!(v.as_str(), "bulk" | "list" | "junk"));
    auto || bulk
}

/// A deliberately small HTML-to-text pass for HTML-only mail: drops tags,
/// `<style>` and `<script>` content, turns block ends into line breaks and
/// decodes the common entities. Good enough for previews and reading.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let lower: String = after
            .chars()
            .take(8)
            .collect::<String>()
            .to_ascii_lowercase();
        let skip_until = if lower.starts_with("<style") {
            Some("</style>")
        } else if lower.starts_with("<script") {
            Some("</script>")
        } else {
            None
        };
        if let Some(end_tag) = skip_until {
            let lowered = after.to_ascii_lowercase();
            rest = lowered
                .find(end_tag)
                .map_or("", |i| &after[i + end_tag.len()..]);
            continue;
        }
        let Some(end) = after.find('>') else {
            rest = "";
            break;
        };
        let tag = after[1..end].trim_start_matches('/').to_ascii_lowercase();
        let name: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if matches!(
            name.as_str(),
            "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3"
        ) {
            out.push('\n');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> EmailAddress {
        EmailAddress::parse("you@company.com").unwrap()
    }

    fn envelope(me: &EmailAddress) -> Envelope<'_> {
        Envelope {
            account: AccountId(1),
            remote_id: "7:42".into(),
            me,
            is_read: false,
            internal_date: DateTime::from_timestamp(1_700_000_000, 0),
        }
    }

    const PLAIN: &str = "From: Priya Raman <priya@company.com>\r\n\
To: You <YOU@company.com>, Tom <tom@company.com>\r\n\
Subject: Re: Q4 budget numbers\r\n\
Date: Fri, 16 Oct 2026 09:42:00 +0200\r\n\
Message-ID: <reply-2@company.com>\r\n\
In-Reply-To: <reply-1@company.com>\r\n\
References: <root@company.com> <reply-1@company.com>\r\n\
\r\n\
Hi,\r\n\r\nCould you send the numbers by Monday?\r\n";

    #[test]
    fn parses_a_plain_reply() {
        let me = me();
        let m = parse(PLAIN.as_bytes(), &envelope(&me)).unwrap();
        assert_eq!(m.from.display(), "Priya Raman");
        assert_eq!(m.from.email().as_str(), "priya@company.com");
        assert_eq!(m.subject, "Re: Q4 budget numbers");
        assert!(
            m.body_text
                .contains("Could you send the numbers by Monday?")
        );
        assert_eq!(m.received_at.to_rfc3339(), "2026-10-16T07:42:00+00:00");
        assert_eq!(m.thread_key, "root@company.com");
        assert_eq!(m.message_id.as_deref(), Some("reply-2@company.com"));
        assert_eq!(m.references, ["root@company.com", "reply-1@company.com"]);
        assert_eq!(m.remote_id, "7:42");
        assert!(m.signals.addressed_to_me, "To match is case-insensitive");
        assert!(!m.signals.mailing_list && !m.signals.calendar_invite && !m.signals.auto_submitted);
        assert!(!m.has_attachments);
    }

    #[test]
    fn detects_lists_invites_and_robots() {
        let me = me();
        let list = "From: News <news@letter.com>\r\nTo: you@company.com\r\nSubject: Issue 1\r\n\
List-Unsubscribe: <mailto:unsub@letter.com>\r\nMessage-ID: <n1@letter.com>\r\n\r\nHello\r\n";
        let m = parse(list.as_bytes(), &envelope(&me)).unwrap();
        assert!(m.signals.mailing_list);

        let invite = "From: Daniel <daniel@company.com>\r\nTo: you@company.com\r\nSubject: Invitation\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nRoadmap sync\r\n\
--b\r\nContent-Type: text/calendar; method=REQUEST\r\n\r\nBEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n--b--\r\n";
        let m = parse(invite.as_bytes(), &envelope(&me)).unwrap();
        assert!(m.signals.calendar_invite);
        assert_eq!(m.body_text.trim(), "Roadmap sync");

        let robot = "From: Bot <bot@service.com>\r\nTo: you@company.com\r\nAuto-Submitted: auto-generated\r\n\
Subject: Build passed\r\n\r\nok\r\n";
        let m = parse(robot.as_bytes(), &envelope(&me)).unwrap();
        assert!(m.signals.auto_submitted);
        assert!(
            m.thread_key.starts_with("local:"),
            "no ids means its own thread"
        );
    }

    #[test]
    fn html_only_mail_becomes_readable_text() {
        let me = me();
        let html = "From: Shop <shop@store.com>\r\nTo: you@company.com\r\nSubject: Receipt\r\n\
Content-Type: text/html\r\nDate: not a date\r\n\r\n\
<html><head><style>p{color:red}</style><script>x()</script></head>\
<body><p>Thanks &amp; welcome</p><div>Total: &lt;12&gt;</div></body></html>\r\n";
        let m = parse(html.as_bytes(), &envelope(&me)).unwrap();
        assert!(
            m.body_text.contains("Thanks & welcome"),
            "{:?}",
            m.body_text
        );
        assert!(m.body_text.contains("Total: <12>"));
        assert!(!m.body_text.contains("color"));
        assert!(!m.body_text.contains("x()"));
        assert_eq!(
            m.received_at.timestamp(),
            1_700_000_000,
            "falls back to INTERNALDATE"
        );
    }

    #[test]
    fn attachments_are_counted() {
        let me = me();
        let raw = "From: Tom <tom@company.com>\r\nTo: you@company.com\r\nSubject: Contract\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"x\"\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\nSee attached\r\n\
--x\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"c.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n--x--\r\n";
        let m = parse(raw.as_bytes(), &envelope(&me)).unwrap();
        assert!(m.has_attachments);
        let files = attachments(raw.as_bytes());
        assert_eq!(files.len(), 1);
        assert_eq!(
            (files[0].name.as_str(), files[0].mime.as_str()),
            ("c.pdf", "application/pdf")
        );
        assert_eq!(files[0].bytes, b"%PDF-");
        let unnamed = raw.replace("; filename=\"c.pdf\"", "");
        assert_eq!(attachments(unnamed.as_bytes())[0].name, "attachment-1");
        assert!(attachments(b"").is_empty());
    }

    #[test]
    fn rejects_mail_without_a_usable_sender() {
        let me = me();
        assert_eq!(parse(b"", &envelope(&me)), Err(ParseError::NotAnEmail));
        let no_from = "To: you@company.com\r\nSubject: hi\r\n\r\nbody\r\n";
        assert_eq!(
            parse(no_from.as_bytes(), &envelope(&me)),
            Err(ParseError::NoSender)
        );
        let bad_from = "From: not-an-address\r\nSubject: hi\r\n\r\nbody\r\n";
        assert_eq!(
            parse(bad_from.as_bytes(), &envelope(&me)),
            Err(ParseError::NoSender)
        );
        assert_eq!(
            ParseError::NoSender.to_string(),
            "message has no valid sender"
        );
    }

    #[test]
    fn thread_key_prefers_the_root() {
        assert_eq!(
            thread_key(Some("<A@x>"), Some("<b@x>"), Some("<c@x>"), "1"),
            "a@x"
        );
        assert_eq!(thread_key(None, Some("<b@x>"), Some("<c@x>"), "1"), "b@x");
        assert_eq!(thread_key(None, None, Some("<c@x>"), "1"), "c@x");
        assert_eq!(thread_key(None, None, Some("<>"), "1"), "local:1");
        assert_eq!(thread_key(None, None, None, "9"), "local:9");
    }

    #[test]
    fn automatic_headers() {
        assert!(is_automatic(Some("auto-replied"), None));
        assert!(!is_automatic(Some("no"), None));
        assert!(is_automatic(None, Some("Bulk")));
        assert!(!is_automatic(None, Some("first-class")));
        assert!(!is_automatic(Some("  "), None));
    }

    #[test]
    fn html_edge_cases() {
        assert_eq!(html_to_text("a<b"), "a");
        assert_eq!(html_to_text("<style>never closed"), "");
        assert_eq!(html_to_text("one<br>two"), "one\ntwo");
        assert_eq!(html_to_text("plain"), "plain");
    }
}
