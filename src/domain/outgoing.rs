//! Mail the user writes: recipients, drafts and reply context.
//!
//! A [`Draft`] can only exist with at least one valid recipient and a
//! non-blank body, so the send path never has to re-check either.

use super::email::{EmailAddress, EmailError};

/// Why a draft can't be sent yet. Messages are shown under the composer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DraftError {
    /// The To field is empty.
    #[error("Add at least one recipient.")]
    NoRecipients,
    /// One of the recipients isn't an address.
    #[error("{address:?} isn't a valid address. {reason}")]
    BadRecipient {
        /// What was typed.
        address: String,
        /// Why it was rejected.
        reason: EmailError,
    },
    /// Nothing was written.
    #[error("Write something before sending.")]
    EmptyBody,
}

/// One or more recipients, de-duplicated, in the order typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipients(Vec<EmailAddress>);

impl Recipients {
    /// Parses a comma- or semicolon-separated list like `a@x.com, b@y.com`.
    pub fn parse(input: &str) -> Result<Self, DraftError> {
        let mut list: Vec<EmailAddress> = Vec::new();
        for part in input
            .split([',', ';'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let address = EmailAddress::parse(part).map_err(|reason| DraftError::BadRecipient {
                address: part.to_owned(),
                reason,
            })?;
            if !list.contains(&address) {
                list.push(address);
            }
        }
        if list.is_empty() {
            return Err(DraftError::NoRecipients);
        }
        Ok(Self(list))
    }

    /// A single recipient.
    pub fn one(address: EmailAddress) -> Self {
        Self(vec![address])
    }

    /// The addresses.
    pub fn as_slice(&self) -> &[EmailAddress] {
        &self.0
    }
}

/// What a reply needs to thread correctly in every client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyContext {
    /// `Message-ID` of the message being answered, without angle brackets.
    pub in_reply_to: String,
    /// The full `References` chain to send, oldest first, without brackets.
    pub references: Vec<String>,
}

impl ReplyContext {
    /// Builds the context for answering a message with `message_id` whose own
    /// `References` were `previous`.
    pub fn answering(message_id: &str, previous: &[String]) -> Self {
        let mut references = previous.to_vec();
        if !references.iter().any(|r| r == message_id) {
            references.push(message_id.to_owned());
        }
        Self {
            in_reply_to: message_id.to_owned(),
            references,
        }
    }
}

/// A message ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    to: Recipients,
    subject: String,
    body: String,
    reply: Option<ReplyContext>,
    calendar: Option<CalendarPart>,
}

/// A `text/calendar` part sent alongside the text (iMIP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarPart {
    /// `REQUEST`, `REPLY` or `CANCEL`.
    pub method: &'static str,
    /// The iCalendar text.
    pub ics: String,
}

impl Draft {
    /// A new message. The body must not be blank.
    pub fn new(to: Recipients, subject: &str, body: &str) -> Result<Self, DraftError> {
        if body.trim().is_empty() {
            return Err(DraftError::EmptyBody);
        }
        Ok(Self {
            to,
            subject: subject.trim().to_owned(),
            body: body.to_owned(),
            reply: None,
            calendar: None,
        })
    }

    /// Turns this draft into a reply: threads it and prefixes "Re:" once.
    pub fn replying(mut self, context: ReplyContext) -> Self {
        self.subject = reply_subject(&self.subject);
        self.reply = Some(context);
        self
    }

    /// Recipients.
    pub fn to(&self) -> &Recipients {
        &self.to
    }

    /// Subject line, possibly empty.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Body text.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Reply threading, if this is a reply.
    pub fn reply(&self) -> Option<&ReplyContext> {
        self.reply.as_ref()
    }

    /// Attaches an invitation or an answer to one.
    pub fn with_calendar(mut self, method: &'static str, ics: String) -> Self {
        self.calendar = Some(CalendarPart { method, ics });
        self
    }

    /// The calendar part, if any.
    pub fn calendar(&self) -> Option<&CalendarPart> {
        self.calendar.as_ref()
    }
}

/// `Re: subject`, without stacking `Re: Re:`.
pub fn reply_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("re:") || lower.starts_with("aw:") || lower.starts_with("sv:") {
        trimmed.to_owned()
    } else if trimmed.is_empty() {
        "Re:".to_owned()
    } else {
        format!("Re: {trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipients_parse_split_and_dedupe() {
        let r = Recipients::parse(" a@x.com; b@y.com , A@X.com ").unwrap();
        let list: Vec<_> = r.as_slice().iter().map(EmailAddress::as_str).collect();
        assert_eq!(list, ["a@x.com", "b@y.com", "A@x.com"]);
        assert_eq!(Recipients::parse(" , ;"), Err(DraftError::NoRecipients));
        let bad = Recipients::parse("a@x.com, nope").unwrap_err();
        assert!(matches!(bad, DraftError::BadRecipient { ref address, .. } if address == "nope"));
        assert_eq!(
            bad.to_string(),
            "\"nope\" isn't a valid address. An email address has exactly one @."
        );
        let one = Recipients::one(EmailAddress::parse("z@z.co").unwrap());
        assert_eq!(one.as_slice().len(), 1);
    }

    #[test]
    fn drafts_need_a_body_and_replies_thread() {
        let to = Recipients::parse("a@x.com").unwrap();
        assert_eq!(
            Draft::new(to.clone(), "Hi", "   "),
            Err(DraftError::EmptyBody)
        );
        let draft = Draft::new(to, " Q4 numbers ", "On it.").unwrap();
        assert_eq!(draft.subject(), "Q4 numbers");
        assert_eq!(draft.body(), "On it.");
        assert_eq!(draft.to().as_slice()[0].as_str(), "a@x.com");
        assert!(draft.reply().is_none());

        let ctx = ReplyContext::answering("m2@x", &["m1@x".to_owned()]);
        assert_eq!(ctx.references, ["m1@x", "m2@x"]);
        let again = ReplyContext::answering("m2@x", &ctx.references);
        assert_eq!(again.references.len(), 2, "no duplicate ids");
        let reply = draft.replying(ctx.clone());
        assert_eq!(reply.subject(), "Re: Q4 numbers");
        assert_eq!(reply.reply(), Some(&ctx));
        assert_eq!(
            DraftError::NoRecipients.to_string(),
            "Add at least one recipient."
        );
    }

    #[test]
    fn reply_subjects_dont_stack() {
        assert_eq!(reply_subject("Hello"), "Re: Hello");
        assert_eq!(reply_subject("RE: Hello"), "RE: Hello");
        assert_eq!(reply_subject("Aw: Hallo"), "Aw: Hallo");
        assert_eq!(reply_subject("  "), "Re:");
    }
}
