//! Which mail provider an address belongs to, and how Sift signs in to it.
//!
//! Detection here is the offline first guess shown while the user types. A
//! company domain defaults to Microsoft 365; the add-account flow will confirm
//! it with autodiscover before connecting, so a wrong guess costs one screen.

use super::email::EmailAddress;

/// A mail provider Sift can connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Microsoft 365 work or school accounts, Outlook.com and Hotmail.
    Microsoft365,
    /// Gmail and Google Workspace.
    Google,
    /// iCloud Mail, which needs an app-specific password.
    ICloud,
    /// Any other provider over IMAP and SMTP.
    Imap,
}

/// How the user proves who they are to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignIn {
    /// OAuth in the system browser. Sift never sees the password.
    Browser,
    /// A provider-issued app-specific password.
    AppPassword,
    /// The account password, stored in the OS credential store.
    Password,
}

impl Provider {
    /// Every provider, in the order the add-account screen lists them.
    pub const ALL: [Provider; 4] = [
        Provider::Microsoft365,
        Provider::Google,
        Provider::ICloud,
        Provider::Imap,
    ];

    /// Best offline guess for an address.
    pub fn detect(email: &EmailAddress) -> Self {
        match email.domain() {
            "gmail.com" | "googlemail.com" => Self::Google,
            "icloud.com" | "me.com" | "mac.com" => Self::ICloud,
            "outlook.com" | "hotmail.com" | "live.com" | "msn.com" => Self::Microsoft365,
            domain if Self::is_known_imap(domain) => Self::Imap,
            // Most work addresses are on Microsoft 365; autodiscover confirms before connecting.
            _ => Self::Microsoft365,
        }
    }

    fn is_known_imap(domain: &str) -> bool {
        const IMAP_DOMAINS: [&str; 6] = [
            "fastmail.com",
            "proton.me",
            "protonmail.com",
            "yahoo.com",
            "zoho.com",
            "gmx.com",
        ];
        IMAP_DOMAINS.contains(&domain)
    }

    /// Name shown in the provider list.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Microsoft365 => "Microsoft 365 or Outlook",
            Self::Google => "Google",
            Self::ICloud => "iCloud Mail",
            Self::Imap => "Other email (IMAP)",
        }
    }

    /// How this provider signs in.
    pub fn sign_in(self) -> SignIn {
        match self {
            Self::Microsoft365 | Self::Google => SignIn::Browser,
            Self::ICloud => SignIn::AppPassword,
            Self::Imap => SignIn::Password,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(address: &str) -> Provider {
        Provider::detect(&EmailAddress::parse(address).unwrap())
    }

    #[test]
    fn detects_consumer_domains() {
        assert_eq!(detect("you@gmail.com"), Provider::Google);
        assert_eq!(detect("you@GoogleMail.com"), Provider::Google);
        assert_eq!(detect("you@icloud.com"), Provider::ICloud);
        assert_eq!(detect("you@me.com"), Provider::ICloud);
        assert_eq!(detect("you@hotmail.com"), Provider::Microsoft365);
        assert_eq!(detect("you@outlook.com"), Provider::Microsoft365);
        assert_eq!(detect("you@fastmail.com"), Provider::Imap);
        assert_eq!(detect("you@proton.me"), Provider::Imap);
    }

    #[test]
    fn company_domains_default_to_microsoft_365() {
        assert_eq!(detect("priya@company.com"), Provider::Microsoft365);
    }

    #[test]
    fn sign_in_method_matches_the_provider() {
        assert_eq!(Provider::Microsoft365.sign_in(), SignIn::Browser);
        assert_eq!(Provider::Google.sign_in(), SignIn::Browser);
        assert_eq!(Provider::ICloud.sign_in(), SignIn::AppPassword);
        assert_eq!(Provider::Imap.sign_in(), SignIn::Password);
    }

    #[test]
    fn every_provider_has_a_name() {
        let names: Vec<_> = Provider::ALL.iter().map(|p| p.display_name()).collect();
        assert_eq!(
            names,
            [
                "Microsoft 365 or Outlook",
                "Google",
                "iCloud Mail",
                "Other email (IMAP)"
            ]
        );
    }
}
