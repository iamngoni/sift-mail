//! The one HTTP call OAuth needs: a form POST to a token endpoint.
//!
//! Behind a trait so token handling is tested without a network. The real
//! client uses rustls, explicit timeouts, and never follows redirects.

use std::time::Duration;

use super::OAuthError;

/// POSTs `application/x-www-form-urlencoded` and returns status and body.
pub trait TokenHttp: Send + Sync {
    /// Sends the form. Transport failures are errors; HTTP error statuses are not.
    fn post_form(&self, url: &str, form: &[(&str, String)]) -> Result<(u16, String), OAuthError>;
}

/// The production client.
pub struct ReqwestHttp(reqwest::blocking::Client);

impl ReqwestHttp {
    /// A client with 10 s connect and 30 s total timeouts.
    pub fn new() -> Result<Self, OAuthError> {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("SiftMail/", env!("CARGO_PKG_VERSION")))
            .build()
            .map(Self)
            .map_err(|e| OAuthError::Network(e.to_string()))
    }
}

impl TokenHttp for ReqwestHttp {
    fn post_form(&self, url: &str, form: &[(&str, String)]) -> Result<(u16, String), OAuthError> {
        let response = self
            .0
            .post(url)
            .form(form)
            .send()
            .map_err(|e| OAuthError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|e| OAuthError::Network(e.to_string()))?;
        Ok((status, body))
    }
}
