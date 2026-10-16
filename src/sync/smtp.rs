//! Sending mail: build a standards-compliant message from a [`Draft`] and
//! hand it to an SMTP server over TLS.
//!
//! Building is pure and tested byte-for-byte; the network send sits behind
//! [`Sender`] so the rest of the send flow is tested without a server.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use lettre::message::header::ContentType;
use lettre::message::{Mailbox, MessageBuilder};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{CertificateStore, Tls, TlsParameters};
use lettre::{Message, SmtpTransport, Transport};

use super::imap::SyncError;
use crate::domain::{AuthMethod, Draft, EmailAddress, SmtpSettings};

/// Sends a built message. Arguments: server, login, password, message.
pub type Sender =
    Arc<dyn Fn(&SmtpSettings, &str, &str, &Message) -> Result<(), SyncError> + Send + Sync>;

/// The account a message is sent from.
#[derive(Debug, Clone)]
pub struct From<'a> {
    /// Display name, such as "Priya Raman". May be empty.
    pub name: &'a str,
    /// Address.
    pub email: &'a EmailAddress,
}

/// A built message plus the id it was given, so the local copy threads.
pub struct Built {
    /// The MIME message.
    pub message: Message,
    /// Its `Message-ID`, without brackets.
    pub message_id: String,
}

/// Builds the MIME message for `draft`. `unique` makes the Message-ID unique;
/// callers pass a random value.
pub fn build(
    from: &From<'_>,
    draft: &Draft,
    now: DateTime<Utc>,
    unique: u64,
) -> Result<Built, SyncError> {
    let mailbox = |name: Option<&str>, email: &EmailAddress| -> Result<Mailbox, SyncError> {
        let address = email
            .as_str()
            .parse()
            .map_err(|e: lettre::address::AddressError| SyncError::Protocol(e.to_string()))?;
        Ok(Mailbox::new(
            name.filter(|n| !n.is_empty()).map(str::to_owned),
            address,
        ))
    };
    let message_id = format!(
        "{:x}.{:x}@{}",
        now.timestamp_millis(),
        unique,
        from.email.domain()
    );
    let mut builder: MessageBuilder = Message::builder()
        .from(mailbox(Some(from.name), from.email)?)
        .subject(draft.subject())
        .date(now.into())
        .message_id(Some(format!("<{message_id}>")));
    for to in draft.to().as_slice() {
        builder = builder.to(mailbox(None, to)?);
    }
    if let Some(reply) = draft.reply() {
        builder = builder
            .in_reply_to(format!("<{}>", reply.in_reply_to))
            .references(
                reply
                    .references
                    .iter()
                    .map(|r| format!("<{r}>"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
    }
    let message = builder
        .header(ContentType::TEXT_PLAIN)
        .body(draft.body().to_owned())
        .map_err(|e| SyncError::Protocol(e.to_string()))?;
    Ok(Built {
        message,
        message_id,
    })
}

/// The real SMTP sender: TLS on 465, STARTTLS otherwise, Mozilla's root store.
pub fn smtp_sender() -> Sender {
    Arc::new(
        |settings: &SmtpSettings, username: &str, password: &str, message: &Message| {
            let host = settings.host.as_str();
            let tls = TlsParameters::builder(host.to_owned())
                .certificate_store(CertificateStore::WebpkiRoots)
                .build()
                .map_err(|e| connect_error(host, e))?;
            let tls = if settings.implicit_tls() {
                Tls::Wrapper(tls)
            } else {
                Tls::Required(tls)
            };
            let transport = SmtpTransport::builder_dangerous(host)
                .port(settings.port)
                .tls(tls)
                .credentials(Credentials::new(username.to_owned(), password.to_owned()))
                .authentication(match settings.auth {
                    AuthMethod::Password => vec![Mechanism::Plain, Mechanism::Login],
                    AuthMethod::OAuth => vec![Mechanism::Xoauth2],
                })
                .build();
            transport.send(message).map(|_| ()).map_err(|e| {
                if e.is_permanent() && e.to_string().contains("535") {
                    SyncError::Auth
                } else {
                    connect_error(host, e)
                }
            })
        },
    )
}

fn connect_error(host: &str, error: impl std::fmt::Display) -> SyncError {
    SyncError::Connect {
        host: host.to_owned(),
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Recipients, ReplyContext};

    fn me() -> EmailAddress {
        EmailAddress::parse("you@fastmail.com").unwrap()
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-16T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn builds_a_threaded_plain_text_reply() {
        let me = me();
        let draft = Draft::new(
            Recipients::parse("priya@company.com").unwrap(),
            "Q4 numbers",
            "On it.\nMonday.",
        )
        .unwrap()
        .replying(ReplyContext::answering(
            "m2@company.com",
            &["m1@company.com".to_owned()],
        ));
        let built = build(
            &From {
                name: "You",
                email: &me,
            },
            &draft,
            now(),
            42,
        )
        .unwrap();
        let text = String::from_utf8(built.message.formatted()).unwrap();
        assert!(text.contains("From: You <you@fastmail.com>"), "{text}");
        assert!(text.contains("To: priya@company.com"));
        assert!(text.contains("Subject: Re: Q4 numbers"));
        assert!(text.contains("In-Reply-To: <m2@company.com>"));
        assert!(text.contains("References: <m1@company.com> <m2@company.com>"));
        assert!(text.contains("Content-Type: text/plain"));
        assert!(text.contains("On it."));
        assert!(built.message_id.ends_with("@fastmail.com"));
        assert!(text.contains(&format!("Message-ID: <{}>", built.message_id)));
    }

    #[test]
    fn builds_a_new_message_to_several_people_without_a_name() {
        let me = me();
        let draft = Draft::new(
            Recipients::parse("a@x.com, b@y.com").unwrap(),
            "Hi",
            "Hello",
        )
        .unwrap();
        let built = build(
            &From {
                name: "",
                email: &me,
            },
            &draft,
            now(),
            1,
        )
        .unwrap();
        let text = String::from_utf8(built.message.formatted()).unwrap();
        assert!(text.contains("From: you@fastmail.com"));
        assert!(text.contains("a@x.com") && text.contains("b@y.com"));
        assert!(!text.contains("In-Reply-To"));
    }

    #[test]
    fn real_sender_reports_unreachable_servers() {
        let settings = SmtpSettings {
            host: crate::domain::Host::parse("localhost.invalid").unwrap(),
            port: 9,
            auth: AuthMethod::OAuth,
        };
        let me = me();
        let draft = Draft::new(Recipients::parse("a@x.com").unwrap(), "Hi", "Hello").unwrap();
        let built = build(
            &From {
                name: "",
                email: &me,
            },
            &draft,
            now(),
            1,
        )
        .unwrap();
        let err = smtp_sender()(&settings, "you", "pw", &built.message).unwrap_err();
        assert!(matches!(err, SyncError::Connect { ref host, .. } if host == "localhost.invalid"));
        let starttls = SmtpSettings {
            port: 587,
            ..settings
        };
        assert!(smtp_sender()(&starttls, "you", "pw", &built.message).is_err());
    }
}
