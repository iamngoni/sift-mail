//! Browser sign-in for Microsoft 365 and Google (OAuth 2.0 authorization code
//! with PKCE, RFC 7636, for a public desktop client per RFC 8252).
//!
//! The flow: open the provider's page in the system browser, receive the
//! redirect on a one-shot listener bound to 127.0.0.1, check `state`, then
//! exchange the code for tokens over TLS. Sift never sees the password.
//! Tokens are only ever handed to the credential store and the mail servers.

mod http;
mod loopback;

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use http::{ReqwestHttp, TokenHttp};
pub use loopback::{Callback, Listener};

use crate::domain::{EmailAddress, Provider};

/// How long the user has to finish signing in before Sift stops waiting.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
/// Refresh an access token this long before it expires.
pub const REFRESH_MARGIN_SECS: i64 = 300;

/// A sign-in failure, phrased for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OAuthError {
    /// No client ID was configured for this provider in this build.
    #[error("{provider} sign-in isn't set up in this build. Set {variable} and rebuild.")]
    NotConfigured {
        /// Provider name.
        provider: &'static str,
        /// The variable that holds its client ID.
        variable: &'static str,
    },
    /// The organisation only allows apps an administrator approved.
    #[error("Your organisation needs to approve Sift Mail before you can sign in.")]
    AdminApprovalRequired,
    /// The user closed the page or said no.
    #[error("Sign-in was cancelled.")]
    Cancelled,
    /// Nothing came back in time.
    #[error("Sign-in timed out. Try again.")]
    TimedOut,
    /// The redirect didn't match the request (`state` mismatch or no code).
    #[error("The sign-in response didn't match this request. Try again.")]
    BadCallback,
    /// The provider returned an error.
    #[error("The provider refused the sign-in: {0}")]
    Provider(String),
    /// Network or TLS failure.
    #[error("Couldn't reach the sign-in service: {0}")]
    Network(String),
}

/// A provider's OAuth endpoints and settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    /// Which provider this is.
    pub provider: Provider,
    /// Authorization endpoint.
    pub authorize_url: String,
    /// Token endpoint.
    pub token_url: String,
    /// The public client ID of the Sift Mail app registration.
    pub client_id: String,
    /// Google's installed-app secret. Not confidential for desktop apps; Google requires it anyway.
    pub client_secret: Option<String>,
    /// Space-separated scopes.
    pub scopes: String,
    /// Scopes asked for at consent but not in token requests: permissions for
    /// another service (Microsoft issues one token per service).
    pub consent_only: String,
}

/// Environment variable for the Microsoft client ID.
pub const MS_CLIENT_ID: &str = "SIFT_MS_CLIENT_ID";
/// Environment variable for the Google client ID.
pub const GOOGLE_CLIENT_ID: &str = "SIFT_GOOGLE_CLIENT_ID";
/// Environment variable for the Google installed-app secret.
pub const GOOGLE_CLIENT_SECRET: &str = "SIFT_GOOGLE_CLIENT_SECRET";

/// Reads a value set at build time, overridable at run time.
fn setting(name: &str, built_in: Option<&'static str>) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| built_in.map(str::to_owned))
        .filter(|v| !v.trim().is_empty())
}

impl OAuthConfig {
    /// The configuration for `provider`, if it uses OAuth and is configured.
    pub fn for_provider(provider: Provider) -> Result<Self, OAuthError> {
        Self::for_provider_with(provider, &|name| match name {
            MS_CLIENT_ID => setting(name, option_env!("SIFT_MS_CLIENT_ID")),
            GOOGLE_CLIENT_ID => setting(name, option_env!("SIFT_GOOGLE_CLIENT_ID")),
            GOOGLE_CLIENT_SECRET => setting(name, option_env!("SIFT_GOOGLE_CLIENT_SECRET")),
            _ => None,
        })
    }

    /// Like [`OAuthConfig::for_provider`] with an explicit settings source.
    pub fn for_provider_with(
        provider: Provider,
        settings: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, OAuthError> {
        match provider {
            Provider::Microsoft365 => Ok(Self {
                provider,
                authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
                    .into(),
                token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
                client_id: settings(MS_CLIENT_ID).ok_or(OAuthError::NotConfigured {
                    provider: "Microsoft",
                    variable: MS_CLIENT_ID,
                })?,
                client_secret: None,
                scopes: "offline_access openid email profile \
                         https://outlook.office.com/IMAP.AccessAsUser.All \
                         https://outlook.office.com/SMTP.Send"
                    .into(),
                consent_only: crate::sync::freebusy::GRAPH_SCOPE.into(),
            }),
            Provider::Google => Ok(Self {
                provider,
                authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
                client_id: settings(GOOGLE_CLIENT_ID).ok_or(OAuthError::NotConfigured {
                    provider: "Google",
                    variable: GOOGLE_CLIENT_ID,
                })?,
                client_secret: settings(GOOGLE_CLIENT_SECRET),
                scopes: format!(
                    "openid email profile https://mail.google.com/ {}",
                    crate::sync::freebusy::GOOGLE_SCOPE
                ),
                consent_only: String::new(),
            }),
            Provider::ICloud | Provider::Imap => Err(OAuthError::Provider(
                "this provider signs in with a password".into(),
            )),
        }
    }
}

/// The secrets for one sign-in attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    /// Sent with the code exchange; never leaves this process before then.
    pub verifier: String,
    /// SHA-256 of the verifier, sent with the authorization request.
    pub challenge: String,
    /// Random value echoed back by the provider; must match.
    pub state: String,
}

impl Pkce {
    /// Fresh random values from the OS RNG.
    pub fn generate() -> Result<Self, OAuthError> {
        let mut verifier = [0u8; 48];
        let mut state = [0u8; 24];
        getrandom::fill(&mut verifier).map_err(|e| OAuthError::Network(e.to_string()))?;
        getrandom::fill(&mut state).map_err(|e| OAuthError::Network(e.to_string()))?;
        Ok(Self::from_bytes(&verifier, &state))
    }

    /// Deterministic construction, for tests and for [`Pkce::generate`].
    pub fn from_bytes(verifier: &[u8], state: &[u8]) -> Self {
        let verifier = URL_SAFE_NO_PAD.encode(verifier);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
            state: URL_SAFE_NO_PAD.encode(state),
        }
    }
}

/// The page to open in the browser.
pub fn authorize_url(
    config: &OAuthConfig,
    pkce: &Pkce,
    redirect_uri: &str,
    hint: Option<&EmailAddress>,
) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.append_pair("client_id", &config.client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair(
            "scope",
            &[config.scopes.as_str(), config.consent_only.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        )
        .append_pair("state", &pkce.state)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256");
    if config.provider == Provider::Google {
        // Without these Google returns no refresh token on repeat sign-ins.
        q.append_pair("access_type", "offline")
            .append_pair("prompt", "consent");
    }
    if let Some(hint) = hint {
        q.append_pair("login_hint", hint.as_str());
    }
    format!("{}?{}", config.authorize_url, q.finish())
}

/// Access and refresh tokens for one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    /// Short-lived bearer token for IMAP and SMTP.
    pub access_token: String,
    /// Long-lived token used to get new access tokens.
    pub refresh_token: String,
    /// When `access_token` stops working, UTC.
    pub expires_at: DateTime<Utc>,
}

impl Tokens {
    /// Whether the access token should be refreshed before use.
    pub fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        self.expires_at - chrono::Duration::seconds(REFRESH_MARGIN_SECS) <= now
    }
}

/// What a successful sign-in produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    /// The tokens.
    pub tokens: Tokens,
    /// The account's address, from the ID token.
    pub email: EmailAddress,
    /// The person's name, if the ID token had one.
    pub name: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    error_description: Option<String>,
}

/// Exchanges an authorization code for tokens.
pub fn exchange_code(
    http: &dyn TokenHttp,
    config: &OAuthConfig,
    pkce: &Pkce,
    code: &str,
    redirect_uri: &str,
    now: DateTime<Utc>,
) -> Result<SignedIn, OAuthError> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("client_id", config.client_id.clone()),
        ("code", code.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("code_verifier", pkce.verifier.clone()),
        ("scope", config.scopes.clone()),
    ];
    if let Some(secret) = &config.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    let body = post(http, &config.token_url, &form)?;
    let response: TokenResponse =
        serde_json::from_str(&body).map_err(|e| OAuthError::Provider(e.to_string()))?;
    let (email, name) = response
        .id_token
        .as_deref()
        .and_then(identity_from_id_token)
        .ok_or_else(|| {
            OAuthError::Provider("the provider didn't say which account signed in".into())
        })?;
    let refresh_token = response
        .refresh_token
        .ok_or_else(|| OAuthError::Provider("no refresh token was issued".into()))?;
    Ok(SignedIn {
        tokens: Tokens {
            access_token: response.access_token,
            refresh_token,
            expires_at: now + chrono::Duration::seconds(response.expires_in.unwrap_or(3600)),
        },
        email,
        name,
    })
}

/// Gets a new access token. Providers may rotate the refresh token; if they
/// don't send one, the old one stays valid.
pub fn refresh(
    http: &dyn TokenHttp,
    config: &OAuthConfig,
    tokens: &Tokens,
    now: DateTime<Utc>,
) -> Result<Tokens, OAuthError> {
    let mut form = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("client_id", config.client_id.clone()),
        ("refresh_token", tokens.refresh_token.clone()),
        ("scope", config.scopes.clone()),
    ];
    if let Some(secret) = &config.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    let body = post(http, &config.token_url, &form)?;
    let response: TokenResponse =
        serde_json::from_str(&body).map_err(|e| OAuthError::Provider(e.to_string()))?;
    Ok(Tokens {
        access_token: response.access_token,
        refresh_token: response
            .refresh_token
            .unwrap_or_else(|| tokens.refresh_token.clone()),
        expires_at: now + chrono::Duration::seconds(response.expires_in.unwrap_or(3600)),
    })
}

fn post(http: &dyn TokenHttp, url: &str, form: &[(&str, String)]) -> Result<String, OAuthError> {
    let (status, body) = http.post_form(url, form)?;
    if (200..300).contains(&status) {
        return Ok(body);
    }
    Err(match serde_json::from_str::<ErrorResponse>(&body) {
        Ok(e) => classify_error(&e.error, e.error_description.as_deref().unwrap_or_default()),
        Err(_) => OAuthError::Provider(format!("HTTP {status}")),
    })
}

/// Maps a provider error code to what the user should see.
pub fn classify_error(code: &str, description: &str) -> OAuthError {
    // Microsoft: AADSTS65001 (consent missing), AADSTS90094 / AADSTS90095
    // (admin approval required or requested); Google: admin_policy_enforced.
    let admin = [
        "AADSTS65001",
        "AADSTS90094",
        "AADSTS90095",
        "admin_policy_enforced",
    ];
    if admin.iter().any(|c| description.contains(c) || code == *c) || code == "consent_required" {
        OAuthError::AdminApprovalRequired
    } else if code == "access_denied" {
        OAuthError::Cancelled
    } else {
        let detail = description.lines().next().unwrap_or(code);
        OAuthError::Provider(if detail.is_empty() {
            code.to_owned()
        } else {
            detail.to_owned()
        })
    }
}

/// Reads `email` and `name` from an ID token's payload. The token came
/// straight from the provider's token endpoint over TLS, so its signature
/// isn't re-verified here (OpenID Connect Core §3.1.3.7, item 6).
pub fn identity_from_id_token(id_token: &str) -> Option<(EmailAddress, Option<String>)> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let email = claims
        .get("email")
        .or_else(|| claims.get("preferred_username"))
        .and_then(|v| v.as_str())
        .and_then(|e| EmailAddress::parse(e).ok())?;
    let name = claims
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    Some((email, name))
}

/// The SASL XOAUTH2 initial response for IMAP and SMTP.
pub fn xoauth2(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

/// Runs a complete sign-in: listen, open the browser, wait, exchange.
/// Blocking; run off the UI thread.
pub fn sign_in(
    config: &OAuthConfig,
    http: &dyn TokenHttp,
    hint: Option<&EmailAddress>,
    open_browser: &dyn Fn(&str) -> Result<(), OAuthError>,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<SignedIn, OAuthError> {
    let pkce = Pkce::generate()?;
    let listener = Listener::bind()?;
    let redirect = listener.redirect_uri();
    open_browser(&authorize_url(config, &pkce, &redirect, hint))?;
    let callback = listener.wait(timeout, cancelled)?;
    match callback {
        Callback::Code { code, state } if state == pkce.state => {
            exchange_code(http, config, &pkce, &code, &redirect, Utc::now())
        }
        Callback::Code { .. } => Err(OAuthError::BadCallback),
        Callback::Error { code, description } => Err(classify_error(&code, &description)),
    }
}

/// Opens a URL in the user's default browser.
pub fn system_browser(url: &str) -> Result<(), OAuthError> {
    open::that_detached(url)
        .map_err(|e| OAuthError::Network(format!("couldn't open the browser: {e}")))
}

#[cfg(test)]
mod tests;
