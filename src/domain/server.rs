//! Where an IMAP account lives and how far Sift has synced it.

use std::fmt;

use super::email::EmailAddress;
use super::provider::Provider;

/// Why a server host name was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// Empty, contains spaces or characters outside a DNS name, or has no dot.
    #[error("Enter a server name like imap.example.com.")]
    Invalid,
}

/// A validated server host name such as `imap.mail.me.com`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Host(String);

impl Host {
    /// Parses a host typed by the user or derived from a domain.
    pub fn parse(input: &str) -> Result<Self, HostError> {
        let host = input.trim().trim_end_matches('.').to_ascii_lowercase();
        let valid_chars = host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
        let labels_ok = host.split('.').all(|l| !l.is_empty() && l.len() <= 63);
        if host.is_empty() || host.len() > 253 || !valid_chars || !labels_ok || !host.contains('.')
        {
            return Err(HostError::Invalid);
        }
        Ok(Self(host))
    }

    /// The host name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How an account proves who it is to its mail servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMethod {
    /// A password or app password, sent with LOGIN / AUTH PLAIN over TLS.
    #[default]
    Password,
    /// An OAuth access token, sent with SASL XOAUTH2.
    OAuth,
}

impl AuthMethod {
    /// Stable storage key.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::OAuth => "oauth",
        }
    }

    /// Reads a key written by [`AuthMethod::key`].
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        [Self::Password, Self::OAuth]
            .into_iter()
            .find(|m| m.key() == key)
    }
}

/// How to reach and log in to an IMAP server. Always TLS: port 993 uses
/// implicit TLS, any other port must offer STARTTLS. Plain text is never used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapSettings {
    /// Server name.
    pub host: Host,
    /// Server port, usually 993.
    pub port: u16,
    /// Login name, usually the full address.
    pub username: String,
    /// Password or OAuth.
    pub auth: AuthMethod,
}

impl ImapSettings {
    /// Best-known settings for an address, before the user edits anything.
    pub fn for_address(email: &EmailAddress, provider: Provider) -> Self {
        let host = match (provider, email.domain()) {
            (Provider::ICloud, _) => "imap.mail.me.com".to_owned(),
            (Provider::Google, _) => "imap.gmail.com".to_owned(),
            (Provider::Microsoft365, _) => "outlook.office365.com".to_owned(),
            (_, "yahoo.com") => "imap.mail.yahoo.com".to_owned(),
            (_, "fastmail.com") => "imap.fastmail.com".to_owned(),
            (_, "zoho.com") => "imap.zoho.com".to_owned(),
            (_, "gmx.com") => "imap.gmx.com".to_owned(),
            (_, domain) => format!("imap.{domain}"),
        };
        Self {
            // Every branch above yields a dotted DNS name from a validated domain.
            // `domain()` is already a validated, lower-case DNS name.
            host: Host(host),
            port: 993,
            username: email.as_str().to_owned(),
            auth: AuthMethod::Password,
        }
    }

    /// Whether the connection starts with TLS (port 993) or upgrades with STARTTLS.
    pub fn implicit_tls(&self) -> bool {
        self.port == 993
    }
}

/// How to send mail for an account. Always TLS: 465 is implicit TLS, any
/// other port must offer STARTTLS. The login matches the IMAP one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpSettings {
    /// Server name.
    pub host: Host,
    /// Server port, 465 or 587.
    pub port: u16,
    /// Same as the account's IMAP sign-in.
    pub auth: AuthMethod,
}

impl SmtpSettings {
    /// Best-known outgoing settings for an address, matching
    /// [`ImapSettings::for_address`]. A custom IMAP host `imap.x` maps to `smtp.x`.
    pub fn for_imap(email: &EmailAddress, provider: Provider, imap: &ImapSettings) -> Self {
        let (host, port) = match (provider, email.domain()) {
            (Provider::ICloud, _) => ("smtp.mail.me.com".to_owned(), 587),
            (Provider::Google, _) => ("smtp.gmail.com".to_owned(), 465),
            (Provider::Microsoft365, _) => ("smtp.office365.com".to_owned(), 587),
            (_, "yahoo.com") => ("smtp.mail.yahoo.com".to_owned(), 465),
            (_, "fastmail.com") => ("smtp.fastmail.com".to_owned(), 465),
            _ => {
                let imap_host = imap.host.as_str();
                let base = imap_host.strip_prefix("imap.").unwrap_or(imap_host);
                (format!("smtp.{base}"), 465)
            }
        };
        Self {
            // Built from validated host names above, so this always parses.
            host: Host::parse(&host).unwrap_or_else(|_| imap.host.clone()),
            port,
            auth: imap.auth,
        }
    }

    /// Whether the connection starts with TLS (465) or upgrades with STARTTLS.
    pub fn implicit_tls(&self) -> bool {
        self.port == 465
    }
}

/// Sync progress for one mailbox. When the server's UIDVALIDITY changes, every
/// UID is reassigned, so the cursor resets and the mailbox is fetched again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncCursor {
    /// The server's UIDVALIDITY when `last_uid` was recorded.
    pub uid_validity: Option<u32>,
    /// Highest UID stored so far; 0 before the first sync.
    pub last_uid: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(s: &str) -> EmailAddress {
        EmailAddress::parse(s).unwrap()
    }

    #[test]
    fn smtp_settings_follow_the_provider_or_the_imap_host() {
        let cases = [
            ("you@icloud.com", Provider::ICloud, "smtp.mail.me.com", 587),
            ("you@gmail.com", Provider::Google, "smtp.gmail.com", 465),
            (
                "you@company.com",
                Provider::Microsoft365,
                "smtp.office365.com",
                587,
            ),
            ("you@yahoo.com", Provider::Imap, "smtp.mail.yahoo.com", 465),
            ("you@fastmail.com", Provider::Imap, "smtp.fastmail.com", 465),
            ("you@example.org", Provider::Imap, "smtp.example.org", 465),
        ];
        for (address, provider, host, port) in cases {
            let e = email(address);
            let imap = ImapSettings::for_address(&e, provider);
            let smtp = SmtpSettings::for_imap(&e, provider, &imap);
            assert_eq!((smtp.host.as_str(), smtp.port), (host, port), "{address}");
        }
        let e = email("you@company.com");
        let custom = ImapSettings {
            host: Host::parse("mail.company.com").unwrap(),
            ..ImapSettings::for_address(&e, Provider::Imap)
        };
        let smtp = SmtpSettings::for_imap(&e, Provider::Imap, &custom);
        assert_eq!(smtp.host.as_str(), "smtp.mail.company.com");
        assert!(smtp.implicit_tls());
        assert!(!SmtpSettings { port: 587, ..smtp }.implicit_tls());
    }

    #[test]
    fn hosts_are_normalised_and_validated() {
        assert_eq!(
            Host::parse(" IMAP.Example.com. ").unwrap().as_str(),
            "imap.example.com"
        );
        assert_eq!(
            Host::parse("imap.example.com").unwrap().to_string(),
            "imap.example.com"
        );
        for bad in [
            "",
            "localhost",
            "imap example.com",
            "imap..com",
            "im@p.com",
            &"a.".repeat(130),
        ] {
            assert_eq!(Host::parse(bad), Err(HostError::Invalid), "{bad:?}");
        }
        assert!(HostError::Invalid.to_string().contains("imap.example.com"));
    }

    #[test]
    fn known_providers_get_their_servers() {
        let cases = [
            ("you@icloud.com", Provider::ICloud, "imap.mail.me.com"),
            ("you@gmail.com", Provider::Google, "imap.gmail.com"),
            (
                "you@company.com",
                Provider::Microsoft365,
                "outlook.office365.com",
            ),
            ("you@yahoo.com", Provider::Imap, "imap.mail.yahoo.com"),
            ("you@fastmail.com", Provider::Imap, "imap.fastmail.com"),
            ("you@zoho.com", Provider::Imap, "imap.zoho.com"),
            ("you@gmx.com", Provider::Imap, "imap.gmx.com"),
            ("you@example.org", Provider::Imap, "imap.example.org"),
        ];
        for (address, provider, host) in cases {
            let s = ImapSettings::for_address(&email(address), provider);
            assert_eq!(s.host.as_str(), host, "{address}");
            assert_eq!(s.port, 993);
            assert_eq!(s.username, address);
            assert!(s.implicit_tls());
        }
        let starttls = ImapSettings {
            port: 143,
            ..ImapSettings::for_address(&email("a@b.co"), Provider::Imap)
        };
        assert!(!starttls.implicit_tls());
        assert_eq!(
            SyncCursor::default(),
            SyncCursor {
                uid_validity: None,
                last_uid: 0
            }
        );
    }
}
