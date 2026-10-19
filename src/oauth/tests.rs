//! OAuth: configuration, PKCE, URL building, token exchange and refresh,
//! error mapping, the loopback listener and the full sign-in with a fake
//! browser that follows the redirect.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;

use super::loopback::parse_target;
use super::*;

fn settings(name: &str) -> Option<String> {
    match name {
        MS_CLIENT_ID => Some("ms-client".into()),
        GOOGLE_CLIENT_ID => Some("g-client".into()),
        GOOGLE_CLIENT_SECRET => Some("g-secret".into()),
        _ => None,
    }
}

fn ms() -> OAuthConfig {
    OAuthConfig::for_provider_with(Provider::Microsoft365, &settings).unwrap()
}

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-16T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn id_token(claims: &str) -> String {
    format!("h.{}.s", URL_SAFE_NO_PAD.encode(claims))
}

/// One recorded request: URL and form fields.
type Request = (String, Vec<(String, String)>);

/// Records requests and replies with canned responses.
struct FakeHttp {
    replies: Mutex<Vec<(u16, String)>>,
    seen: Mutex<Vec<Request>>,
}

impl FakeHttp {
    fn new(replies: Vec<(u16, String)>) -> Self {
        Self {
            replies: Mutex::new(replies),
            seen: Mutex::new(Vec::new()),
        }
    }
    fn form(&self, i: usize) -> Vec<(String, String)> {
        self.seen.lock().unwrap()[i].1.clone()
    }
}

impl TokenHttp for FakeHttp {
    fn post_form(&self, url: &str, form: &[(&str, String)]) -> Result<(u16, String), OAuthError> {
        self.seen.lock().unwrap().push((
            url.to_owned(),
            form.iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        ));
        let mut replies = self.replies.lock().unwrap();
        if replies.is_empty() {
            return Err(OAuthError::Network("no reply".into()));
        }
        Ok(replies.remove(0))
    }
}

#[test]
fn configs_need_a_client_id() {
    let none = |_: &str| None;
    assert_eq!(
        OAuthConfig::for_provider_with(Provider::Microsoft365, &none),
        Err(OAuthError::NotConfigured {
            provider: "Microsoft",
            variable: MS_CLIENT_ID
        })
    );
    let err = OAuthConfig::for_provider_with(Provider::Google, &none).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Google sign-in isn't set up in this build. Set SIFT_GOOGLE_CLIENT_ID and rebuild."
    );
    assert!(OAuthConfig::for_provider_with(Provider::Imap, &settings).is_err());
    assert!(OAuthConfig::for_provider_with(Provider::ICloud, &settings).is_err());
    let g = OAuthConfig::for_provider_with(Provider::Google, &settings).unwrap();
    assert_eq!(g.client_secret.as_deref(), Some("g-secret"));
    assert!(g.scopes.contains("https://mail.google.com/"));
    assert!(ms().scopes.contains("IMAP.AccessAsUser.All"));
    // The process environment has no client IDs in tests.
    let _ = OAuthConfig::for_provider(Provider::Microsoft365);
    assert_eq!(setting("SIFT_DEFINITELY_UNSET", Some("  ")), None);
    assert_eq!(
        setting("SIFT_DEFINITELY_UNSET", Some("x")).as_deref(),
        Some("x")
    );
}

#[test]
fn pkce_matches_the_rfc_example() {
    // RFC 7636 appendix B: this verifier hashes to this challenge.
    let verifier_bytes = [
        116, 24, 223, 180, 151, 153, 224, 37, 79, 250, 96, 125, 216, 173, 187, 186, 22, 212, 37,
        77, 105, 214, 191, 240, 91, 88, 5, 88, 83, 132, 141, 121,
    ];
    let pkce = Pkce::from_bytes(&verifier_bytes, b"s");
    assert_eq!(pkce.verifier, "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    assert_eq!(
        pkce.challenge,
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let random = Pkce::generate().unwrap();
    assert_ne!(random.verifier, Pkce::generate().unwrap().verifier);
    assert_eq!(random.verifier.len(), 64);
}

#[test]
fn authorize_urls_carry_pkce_and_hints() {
    let pkce = Pkce::from_bytes(b"verifier", b"state");
    let hint = EmailAddress::parse("you@company.com").unwrap();
    let url = authorize_url(&ms(), &pkce, "http://127.0.0.1:5000/callback", Some(&hint));
    assert!(url.starts_with(
        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id=ms-client"
    ));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains(&format!("state={}", pkce.state)));
    assert!(url.contains("login_hint=you%40company.com"));
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5000%2Fcallback"));
    assert!(!url.contains("access_type"));
    let g = OAuthConfig::for_provider_with(Provider::Google, &settings).unwrap();
    let url = authorize_url(&g, &pkce, "http://127.0.0.1:1/callback", None);
    assert!(url.contains("access_type=offline") && url.contains("prompt=consent"));
    assert!(!url.contains("login_hint"));
}

#[test]
fn exchanges_codes_and_reads_the_identity() {
    let token = id_token(r#"{"preferred_username":"You@Company.com","name":"Ngoni"}"#);
    let http = FakeHttp::new(vec![(
        200,
        format!(
            r#"{{"access_token":"at","refresh_token":"rt","expires_in":3600,"id_token":"{token}"}}"#
        ),
    )]);
    let pkce = Pkce::from_bytes(b"verifier", b"state");
    let signed = exchange_code(&http, &ms(), &pkce, "the-code", "http://r", now()).unwrap();
    assert_eq!(signed.email.as_str(), "You@company.com");
    assert_eq!(signed.name.as_deref(), Some("Ngoni"));
    assert_eq!(signed.tokens.access_token, "at");
    assert_eq!(signed.tokens.expires_at, now() + chrono::Duration::hours(1));
    let form = http.form(0);
    assert!(form.contains(&("code_verifier".into(), pkce.verifier.clone())));
    assert!(form.contains(&("grant_type".into(), "authorization_code".into())));
    assert!(!form.iter().any(|(k, _)| k == "client_secret"));
}

#[test]
fn exchange_failures_are_explained() {
    let pkce = Pkce::from_bytes(b"v", b"s");
    let no_identity = FakeHttp::new(vec![(
        200,
        r#"{"access_token":"a","refresh_token":"r"}"#.into(),
    )]);
    assert!(matches!(
        exchange_code(&no_identity, &ms(), &pkce, "c", "r", now()),
        Err(OAuthError::Provider(_))
    ));
    let token = id_token(r#"{"email":"a@b.co"}"#);
    let no_refresh = FakeHttp::new(vec![(
        200,
        format!(r#"{{"access_token":"a","id_token":"{token}"}}"#),
    )]);
    let err = exchange_code(&no_refresh, &ms(), &pkce, "c", "r", now()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "The provider refused the sign-in: no refresh token was issued"
    );
    let garbage = FakeHttp::new(vec![(200, "not json".into())]);
    assert!(exchange_code(&garbage, &ms(), &pkce, "c", "r", now()).is_err());
    let admin = FakeHttp::new(vec![(
        400,
        r#"{"error":"invalid_grant","error_description":"AADSTS65001: The user or administrator has not consented"}"#.into(),
    )]);
    assert_eq!(
        exchange_code(&admin, &ms(), &pkce, "c", "r", now()),
        Err(OAuthError::AdminApprovalRequired)
    );
    let html = FakeHttp::new(vec![(502, "<html>".into())]);
    assert_eq!(
        exchange_code(&html, &ms(), &pkce, "c", "r", now()),
        Err(OAuthError::Provider("HTTP 502".into()))
    );
}

#[test]
fn refresh_keeps_the_old_refresh_token_unless_rotated() {
    let g = OAuthConfig::for_provider_with(Provider::Google, &settings).unwrap();
    let tokens = Tokens {
        access_token: "old".into(),
        refresh_token: "rt1".into(),
        expires_at: now(),
    };
    assert!(tokens.needs_refresh(now()));
    let http = FakeHttp::new(vec![
        (200, r#"{"access_token":"new","expires_in":60}"#.into()),
        (
            200,
            r#"{"access_token":"newer","refresh_token":"rt2"}"#.into(),
        ),
    ]);
    let fresh = refresh(&http, &g, &tokens, now()).unwrap();
    assert_eq!(
        (fresh.access_token.as_str(), fresh.refresh_token.as_str()),
        ("new", "rt1")
    );
    assert!(
        http.form(0)
            .contains(&("client_secret".into(), "g-secret".into()))
    );
    let rotated = refresh(&http, &g, &fresh, now()).unwrap();
    assert_eq!(rotated.refresh_token, "rt2");
    assert!(!rotated.needs_refresh(now()));
    assert!(matches!(
        refresh(&http, &g, &fresh, now()),
        Err(OAuthError::Network(_))
    ));
    let bad = FakeHttp::new(vec![(200, "{}".into())]);
    assert!(refresh(&bad, &g, &fresh, now()).is_err());
}

#[test]
fn errors_map_to_user_meaning() {
    assert_eq!(classify_error("access_denied", ""), OAuthError::Cancelled);
    assert_eq!(
        classify_error("consent_required", ""),
        OAuthError::AdminApprovalRequired
    );
    assert_eq!(
        classify_error("admin_policy_enforced", ""),
        OAuthError::AdminApprovalRequired
    );
    assert_eq!(
        classify_error("x", "AADSTS90094: Admin approval required"),
        OAuthError::AdminApprovalRequired
    );
    assert_eq!(
        classify_error("server_error", "Busy\nTrace"),
        OAuthError::Provider("Busy".into())
    );
    assert_eq!(
        classify_error("server_error", ""),
        OAuthError::Provider("server_error".into())
    );
    for e in [
        OAuthError::Cancelled,
        OAuthError::TimedOut,
        OAuthError::BadCallback,
        OAuthError::AdminApprovalRequired,
    ] {
        assert!(e.to_string().ends_with('.'), "{e}");
    }
    assert!(OAuthError::Network("x".into()).to_string().contains("x"));
}

#[test]
fn id_tokens_and_xoauth2() {
    assert_eq!(identity_from_id_token("nope"), None);
    assert_eq!(identity_from_id_token("a.!!!.c"), None);
    assert_eq!(identity_from_id_token(&id_token("[]")), None);
    assert_eq!(
        identity_from_id_token(&id_token(r#"{"email":"bad"}"#)),
        None
    );
    assert_eq!(
        xoauth2("you@x.com", "tok"),
        "user=you@x.com\x01auth=Bearer tok\x01\x01"
    );
}

#[test]
fn callback_targets_parse() {
    assert_eq!(parse_target("/favicon.ico"), None);
    assert_eq!(
        parse_target("/callback?code=abc&state=xyz&session_state=1"),
        Some(Callback::Code {
            code: "abc".into(),
            state: "xyz".into()
        })
    );
    assert_eq!(
        parse_target("/callback?error=access_denied&error_description=User+said+no"),
        Some(Callback::Error {
            code: "access_denied".into(),
            description: "User said no".into()
        })
    );
    assert!(matches!(
        parse_target("/callback"),
        Some(Callback::Error { .. })
    ));
}

fn get(url: &str) -> String {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let mut stream = TcpStream::connect(host).unwrap();
    write!(stream, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn listener_ignores_other_paths_and_returns_the_callback() {
    let listener = Listener::bind().unwrap();
    let base = listener.redirect_uri();
    let favicon = base.replace("/callback", "/favicon.ico");
    let handle = std::thread::spawn(move || {
        assert!(get(&favicon).starts_with("HTTP/1.1 404"));
        let page = get(&format!("{base}?code=c1&state=s1"));
        assert!(page.contains("You're signed in"));
    });
    let callback = listener.wait(Duration::from_secs(10), &|| false).unwrap();
    handle.join().unwrap();
    assert_eq!(
        callback,
        Callback::Code {
            code: "c1".into(),
            state: "s1".into()
        }
    );
}

#[test]
fn listener_times_out_and_cancels() {
    let listener = Listener::bind().unwrap();
    assert_eq!(
        listener.wait(Duration::ZERO, &|| false),
        Err(OAuthError::TimedOut)
    );
    assert_eq!(
        listener.wait(Duration::from_secs(5), &|| true),
        Err(OAuthError::Cancelled)
    );
}

/// A browser that follows the redirect with a canned callback query.
fn browser(query: &'static str) -> impl Fn(&str) -> Result<(), OAuthError> {
    move |url: &str| {
        let parsed = url::Url::parse(url).unwrap();
        let redirect = parsed
            .query_pairs()
            .find(|(k, _)| k == "redirect_uri")
            .unwrap()
            .1
            .into_owned();
        let state = parsed
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let query = query.replace("{state}", &state);
        std::thread::spawn(move || {
            let _ = get(&format!("{redirect}?{query}"));
        });
        Ok(())
    }
}

#[test]
fn full_sign_in_with_a_following_browser() {
    let token = id_token(r#"{"email":"you@company.com"}"#);
    let http = FakeHttp::new(vec![(
        200,
        format!(r#"{{"access_token":"at","refresh_token":"rt","id_token":"{token}"}}"#),
    )]);
    let signed = sign_in(
        &ms(),
        &http,
        None,
        &browser("code=c&state={state}"),
        Duration::from_secs(10),
        &|| false,
    )
    .unwrap();
    assert_eq!(signed.email.as_str(), "you@company.com");

    let wrong_state = sign_in(
        &ms(),
        &http,
        None,
        &browser("code=c&state=forged"),
        Duration::from_secs(10),
        &|| false,
    );
    assert_eq!(wrong_state, Err(OAuthError::BadCallback));

    let blocked = sign_in(
        &ms(),
        &http,
        None,
        &browser("error=access_denied&error_description=AADSTS90094%3A+admin+approval"),
        Duration::from_secs(10),
        &|| false,
    );
    assert_eq!(blocked, Err(OAuthError::AdminApprovalRequired));

    let no_browser = |_: &str| Err(OAuthError::Network("no browser".into()));
    assert!(
        sign_in(
            &ms(),
            &http,
            None,
            &no_browser,
            Duration::from_secs(1),
            &|| false
        )
        .is_err()
    );
}

#[test]
fn real_http_client_reports_network_errors() {
    let http = ReqwestHttp::new().unwrap();
    // Port 9 on localhost is closed.
    assert!(matches!(
        http.post_form("http://127.0.0.1:9/token", &[]),
        Err(OAuthError::Network(_))
    ));
}

#[test]
fn real_http_client_posts_forms_and_returns_status_and_body() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        let body = r#"{"error":"invalid_grant"}"#;
        write!(
            stream,
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });
    let http = ReqwestHttp::new().unwrap();
    let (status, body) = http
        .post_form(
            &format!("http://{addr}/token"),
            &[("grant_type", "refresh_token".to_owned())],
        )
        .unwrap();
    let request = server.join().unwrap();
    assert_eq!(status, 400);
    assert_eq!(body, r#"{"error":"invalid_grant"}"#);
    assert!(request.starts_with("POST /token"));
    assert!(request.contains("application/x-www-form-urlencoded"));
    assert!(request.contains("SiftMail/"));
}

#[test]
fn calendar_permission_is_asked_for_at_consent() {
    let pkce = Pkce::generate().unwrap();
    let url = authorize_url(&ms(), &pkce, "http://127.0.0.1:1/callback", None);
    assert!(url.contains("Calendars.Read.Shared"), "{url}");
    assert!(
        !ms().scopes.contains("graph.microsoft.com"),
        "token requests stay single-service"
    );
    let g = OAuthConfig::for_provider_with(Provider::Google, &settings).unwrap();
    assert!(g.scopes.contains("calendar.freebusy"));
    assert!(g.consent_only.is_empty());
}

#[test]
fn one_organisation_apps_sign_in_through_their_tenant() {
    assert!(
        ms().authorize_url.contains("/common/"),
        "any organisation by default"
    );
    let tenant = |value: &'static str| {
        move |name: &str| match name {
            MS_TENANT => Some(value.to_owned()),
            other => settings(other),
        }
    };
    let own =
        OAuthConfig::for_provider_with(Provider::Microsoft365, &tenant(" contoso.com ")).unwrap();
    assert_eq!(
        own.authorize_url,
        "https://login.microsoftonline.com/contoso.com/oauth2/v2.0/authorize"
    );
    assert_eq!(
        own.token_url,
        "https://login.microsoftonline.com/contoso.com/oauth2/v2.0/token"
    );
    let bad = OAuthConfig::for_provider_with(Provider::Microsoft365, &tenant("evil.com/x?y"));
    assert!(bad.unwrap_err().to_string().contains(MS_TENANT));
}
