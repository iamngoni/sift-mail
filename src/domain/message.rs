//! Messages, threads and their triage state.
//!
//! [`IncomingMessage`] is what a sync adapter hands to the store: already
//! parsed, with a validated sender. Everything the inbox shows is derived from
//! it by the store, so adapters never decide presentation.

use chrono::{DateTime, Utc};

use super::email::EmailAddress;

/// Store-assigned id of a connected account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(pub(crate) i64);

/// Store-assigned id of a single message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(pub(crate) i64);

/// Identifies a conversation: the provider's thread key within one account.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThreadRef {
    pub(crate) account: AccountId,
    pub(crate) key: String,
}

impl ThreadRef {
    /// The account the thread belongs to.
    pub fn account(&self) -> AccountId {
        self.account
    }
}

/// Someone who sent a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    name: Option<String>,
    email: EmailAddress,
}

impl Contact {
    /// A contact with an optional display name. Blank names count as no name.
    pub fn new(name: Option<&str>, email: EmailAddress) -> Self {
        let name = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned);
        Self { name, email }
    }

    /// The display name if there is one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The address.
    pub fn email(&self) -> &EmailAddress {
        &self.email
    }

    /// What the inbox shows: the name, or the address when there is none.
    pub fn display(&self) -> &str {
        self.name.as_deref().unwrap_or(self.email.as_str())
    }

    /// Up to two initials for the avatar, upper-case.
    pub fn initials(&self) -> String {
        initials(self.display())
    }
}

/// Up to two upper-case initials from a display string such as "Priya Raman".
pub fn initials(display: &str) -> String {
    let words: Vec<&str> = display
        .split(|c: char| c.is_whitespace() || c == '@' || c == '.')
        .filter(|w| !w.is_empty())
        .collect();
    let picked: Vec<&str> = match words.as_slice() {
        [] => Vec::new(),
        [only] => vec![only],
        [first, .., last] => vec![first, last],
    };
    picked
        .iter()
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// Facts about a message that decide which view it lands in. Sync adapters
/// fill these from headers; see [`super::triage::classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals {
    /// Has a `List-Unsubscribe` or `List-Id` header.
    pub mailing_list: bool,
    /// Carries a `text/calendar` part or an invite method.
    pub calendar_invite: bool,
    /// `Auto-Submitted` other than `no`, or a `Precedence: bulk` style header.
    pub auto_submitted: bool,
    /// The user is in `To`, not only `Cc` or `Bcc`.
    pub addressed_to_me: bool,
}

/// A message as a sync adapter delivers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessage {
    /// Which account it arrived in.
    pub account: AccountId,
    /// The provider's id, unique within the account. Re-delivering it updates in place.
    pub remote_id: String,
    /// The provider's conversation key.
    pub thread_key: String,
    /// Sender.
    pub from: Contact,
    /// Subject line, may be empty.
    pub subject: String,
    /// Plain-text body.
    pub body_text: String,
    /// When the provider received it, UTC.
    pub received_at: DateTime<Utc>,
    /// Already read on the server.
    pub is_read: bool,
    /// Has at least one attachment.
    pub has_attachments: bool,
    /// Classification inputs.
    pub signals: Signals,
    /// `Message-ID` without angle brackets, used to thread replies.
    pub message_id: Option<String>,
    /// `References` ids without angle brackets, oldest first.
    pub references: Vec<String>,
}

/// Where a thread is in the user's workflow. Each state carries exactly the
/// data it needs: only a snoozed thread has a wake-up time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageState {
    /// Visible in the inbox.
    Inbox,
    /// Marked done; hidden until a new message arrives in the thread.
    Done,
    /// Hidden until `until`, then back in the inbox.
    Snoozed {
        /// When it comes back, UTC.
        until: DateTime<Utc>,
    },
}

/// Maximum preview length in characters.
pub const PREVIEW_CHARS: usize = 160;

/// The one-line preview shown in the list: whitespace collapsed, quoted reply
/// lines (`>`) skipped, cut at [`PREVIEW_CHARS`] with an ellipsis.
pub fn preview(body: &str) -> String {
    let text = body
        .lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() <= PREVIEW_CHARS {
        return text;
    }
    let cut: String = text.chars().take(PREVIEW_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(s: &str) -> EmailAddress {
        EmailAddress::parse(s).unwrap()
    }

    #[test]
    fn contact_prefers_the_name_and_ignores_blank_names() {
        let named = Contact::new(Some("  Priya Raman "), email("priya@company.com"));
        assert_eq!(named.name(), Some("Priya Raman"));
        assert_eq!(named.display(), "Priya Raman");
        assert_eq!(named.initials(), "PR");
        assert_eq!(named.email().as_str(), "priya@company.com");

        let blank = Contact::new(Some("   "), email("tom@company.com"));
        assert_eq!(blank.name(), None);
        assert_eq!(blank.display(), "tom@company.com");
        assert_eq!(blank.initials(), "TC");
    }

    #[test]
    fn initials_handle_one_many_and_no_words() {
        assert_eq!(initials("Mum"), "M");
        assert_eq!(initials("Kofi Ama Asante"), "KA");
        assert_eq!(initials(""), "");
        assert_eq!(initials("élodie dupont"), "ÉD");
    }

    #[test]
    fn preview_collapses_whitespace_and_skips_quotes() {
        let body =
            "Hi Priya,\n\n  Thanks   for this.\n> On Monday you wrote:\n> old text\nSee you.";
        assert_eq!(preview(body), "Hi Priya, Thanks for this. See you.");
    }

    #[test]
    fn preview_truncates_long_bodies_on_a_character_boundary() {
        let body = "é".repeat(400);
        let p = preview(&body);
        assert_eq!(p.chars().count(), PREVIEW_CHARS);
        assert!(p.ends_with('…'));
        assert_eq!(preview("short"), "short");
    }

    #[test]
    fn thread_ref_exposes_its_account() {
        let t = ThreadRef {
            account: AccountId(7),
            key: "t".into(),
        };
        assert_eq!(t.account(), AccountId(7));
    }
}
