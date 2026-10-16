//! A validated, normalised email address.
//!
//! Only [`EmailAddress::parse`] can build one, so any `EmailAddress` in the app
//! has exactly one `@`, a non-empty local part, and a dotted, lower-case domain.
//! This is a practical check for user-typed addresses, not full RFC 5322.

use std::fmt;

/// Longest address SMTP allows (RFC 5321 §4.5.3.1.3).
const MAX_LEN: usize = 254;
/// Longest local part SMTP allows (RFC 5321 §4.5.3.1.1).
const MAX_LOCAL_LEN: usize = 64;

/// Why text is not a usable email address. Messages are safe to show users.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmailError {
    /// Nothing was typed.
    #[error("Enter an email address.")]
    Empty,
    /// Over 254 characters, or a local part over 64.
    #[error("That address is too long.")]
    TooLong,
    /// No `@`, or more than one.
    #[error("An email address has exactly one @.")]
    WrongAtCount,
    /// Nothing before the `@`.
    #[error("Add the part before the @.")]
    MissingLocalPart,
    /// The domain is missing, has no dot, or has an empty label like `a..com`.
    #[error("Check the part after the @, like company.com.")]
    BadDomain,
    /// Contains whitespace or control characters.
    #[error("Email addresses can't contain spaces.")]
    Whitespace,
}

/// A normalised email address: trimmed, with the domain lower-cased.
/// The local part keeps its case because some servers treat it as significant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmailAddress {
    value: String,
    at: usize,
}

impl EmailAddress {
    /// Parses user-typed text.
    pub fn parse(input: &str) -> Result<Self, EmailError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(EmailError::Empty);
        }
        if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(EmailError::Whitespace);
        }
        if trimmed.len() > MAX_LEN {
            return Err(EmailError::TooLong);
        }
        let mut parts = trimmed.split('@');
        let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(EmailError::WrongAtCount);
        };
        if local.is_empty() {
            return Err(EmailError::MissingLocalPart);
        }
        if local.len() > MAX_LOCAL_LEN {
            return Err(EmailError::TooLong);
        }
        let domain = domain.to_ascii_lowercase();
        let labels: Vec<&str> = domain.split('.').collect();
        if labels.len() < 2 || labels.iter().any(|label| label.is_empty()) {
            return Err(EmailError::BadDomain);
        }
        Ok(Self {
            at: local.len(),
            value: format!("{local}@{domain}"),
        })
    }

    /// The whole address.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// The part before `@`, as typed.
    pub fn local_part(&self) -> &str {
        &self.value[..self.at]
    }

    /// The lower-case domain after `@`.
    pub fn domain(&self) -> &str {
        &self.value[self.at + 1..]
    }
}

impl fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

impl TryFrom<&str> for EmailAddress {
    type Error = EmailError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_valid_addresses() {
        let email = EmailAddress::parse("  Priya.Raman@Company.COM ").unwrap();
        assert_eq!(email.as_str(), "Priya.Raman@company.com");
        assert_eq!(email.local_part(), "Priya.Raman");
        assert_eq!(email.domain(), "company.com");
        assert_eq!(email.to_string(), "Priya.Raman@company.com");
        assert_eq!(EmailAddress::try_from("a@b.co").unwrap().domain(), "b.co");
    }

    #[test]
    fn rejects_each_kind_of_bad_input_with_a_specific_reason() {
        let cases = [
            ("", EmailError::Empty),
            ("   ", EmailError::Empty),
            ("no-at.example.com", EmailError::WrongAtCount),
            ("a@b@c.com", EmailError::WrongAtCount),
            ("@company.com", EmailError::MissingLocalPart),
            ("you@", EmailError::BadDomain),
            ("you@localhost", EmailError::BadDomain),
            ("you@company..com", EmailError::BadDomain),
            ("you@.com", EmailError::BadDomain),
            ("you @company.com", EmailError::Whitespace),
        ];
        for (input, expected) in cases {
            assert_eq!(EmailAddress::parse(input), Err(expected), "{input:?}");
        }
        // Surrounding whitespace, including a pasted newline, is trimmed.
        assert!(EmailAddress::parse("you@company.com\n").is_ok());
    }

    #[test]
    fn enforces_smtp_length_limits() {
        let long_local = format!("{}@company.com", "a".repeat(65));
        assert_eq!(EmailAddress::parse(&long_local), Err(EmailError::TooLong));
        let long_total = format!("a@{}.com", "b".repeat(260));
        assert_eq!(EmailAddress::parse(&long_total), Err(EmailError::TooLong));
        let max_local = format!("{}@company.com", "a".repeat(64));
        assert!(EmailAddress::parse(&max_local).is_ok());
    }

    #[test]
    fn errors_have_user_facing_messages() {
        for err in [
            EmailError::Empty,
            EmailError::TooLong,
            EmailError::WrongAtCount,
            EmailError::MissingLocalPart,
            EmailError::BadDomain,
            EmailError::Whitespace,
        ] {
            let text = err.to_string();
            assert!(text.ends_with('.'), "{text}");
        }
    }
}
