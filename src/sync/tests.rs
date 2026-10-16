//! OAuth accounts: token lookup and refresh, adding a signed-in account, and
//! the service's browser sign-in with a fake signer.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Duration;

use super::imap::{MailboxInfo, RawMessage};
use super::secrets::{MemorySecrets, Refreshing, TokenRefresher};
use super::service::{BoxedSession, Connector, SyncService};
use super::*;
use crate::oauth::OAuthError;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-16T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn tokens(access: &str, expires_in_min: i64) -> Tokens {
    Tokens {
        access_token: access.into(),
        refresh_token: "rt".into(),
        expires_at: now() + Duration::minutes(expires_in_min),
    }
}

struct Session;

impl MailSession for Session {
    fn examine_inbox(&mut self) -> Result<MailboxInfo, SyncError> {
        Ok(MailboxInfo {
            uid_validity: Some(1),
        })
    }
    fn uid_search(&mut self, _: &str) -> Result<Vec<u32>, SyncError> {
        Ok(Vec::new())
    }
    fn uid_fetch(&mut self, _: &[u32]) -> Result<Vec<RawMessage>, SyncError> {
        Ok(Vec::new())
    }
    fn set_seen(&mut self, _: &[u32], _: bool) -> Result<(), SyncError> {
        Ok(())
    }
    fn append_sent(&mut self, _: &[u8]) -> Result<bool, SyncError> {
        Ok(false)
    }
}

fn signed_in(email: &str) -> SignedIn {
    SignedIn {
        tokens: tokens("at-1", 60),
        email: EmailAddress::parse(email).unwrap(),
        name: Some("You".into()),
    }
}

fn refresher(calls: Arc<AtomicUsize>) -> TokenRefresher {
    Arc::new(move |provider, old| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(provider, Provider::Microsoft365);
        assert_eq!(old.refresh_token, "rt");
        Ok(Tokens {
            access_token: "at-2".into(),
            refresh_token: "rt-rotated".into(),
            expires_at: now() + Duration::hours(1),
        })
    })
}

#[test]
fn oauth_accounts_are_checked_and_saved_with_their_tokens() {
    let store = MailStore::open_in_memory().unwrap();
    let secrets = MemorySecrets::default();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = Arc::clone(&seen);
    let account = add_oauth_account(
        &store,
        &secrets,
        Provider::Microsoft365,
        &signed_in("you@company.com"),
        |s, token| {
            assert_eq!(s.auth, AuthMethod::OAuth);
            assert_eq!(s.host.as_str(), "outlook.office365.com");
            seen2.lock().unwrap().push(token.to_owned());
            Ok(Session)
        },
    )
    .unwrap();
    assert_eq!(account.name, "Work");
    assert_eq!(*seen.lock().unwrap(), ["at-1"]);
    let saved: Tokens =
        serde_json::from_str(&secrets.get("oauth:you@company.com").unwrap().unwrap()).unwrap();
    assert_eq!(saved.access_token, "at-1");
    let (_, settings, _) = store.imap_account(account.id).unwrap().unwrap();
    assert_eq!(settings.auth, AuthMethod::OAuth);
    assert_eq!(
        store.smtp_settings(account.id).unwrap().unwrap().auth,
        AuthMethod::OAuth
    );

    let gmail = add_oauth_account(
        &store,
        &secrets,
        Provider::Google,
        &signed_in("you@gmail.com"),
        |_, _| Ok(Session),
    )
    .unwrap();
    assert_eq!(gmail.name, "Gmail");

    remove_account(&store, &secrets, &account).unwrap();
    assert_eq!(secrets.get("oauth:you@company.com").unwrap(), None);
}

#[test]
fn rejected_tokens_save_nothing() {
    let store = MailStore::open_in_memory().unwrap();
    let secrets = MemorySecrets::default();
    let result = add_oauth_account::<Session>(
        &store,
        &secrets,
        Provider::Google,
        &signed_in("you@gmail.com"),
        |_, _| Err(SyncError::Auth),
    );
    assert!(matches!(result, Err(SyncError::Auth)));
    assert!(store.accounts().unwrap().is_empty());
}

#[test]
fn access_tokens_are_refreshed_and_rotations_saved() {
    let store = MailStore::open_in_memory().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let secrets = Refreshing::new(
        Arc::new(MemorySecrets::default()),
        refresher(Arc::clone(&calls)),
    );
    let account = add_oauth_account(
        &store,
        &secrets,
        Provider::Microsoft365,
        &signed_in("you@company.com"),
        |_, _| Ok(Session),
    )
    .unwrap();
    let (_, settings, _) = store.imap_account(account.id).unwrap().unwrap();

    // Fresh: used as is.
    assert_eq!(
        secret_for(&secrets, &account, &settings, now()).unwrap(),
        "at-1"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // Inside the refresh margin: refreshed, and the rotated token saved first.
    let later = now() + Duration::minutes(58);
    assert_eq!(
        secret_for(&secrets, &account, &settings, later).unwrap(),
        "at-2"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let saved: Tokens =
        serde_json::from_str(&secrets.get("oauth:you@company.com").unwrap().unwrap()).unwrap();
    assert_eq!(saved.refresh_token, "rt-rotated");
}

#[test]
fn broken_or_missing_tokens_ask_to_sign_in_again() {
    let store = MailStore::open_in_memory().unwrap();
    let secrets = MemorySecrets::default();
    let email = EmailAddress::parse("you@company.com").unwrap();
    let account = store
        .add_account("Work", &email, Provider::Microsoft365)
        .unwrap();
    let settings = ImapSettings {
        auth: AuthMethod::OAuth,
        ..ImapSettings::for_address(&email, Provider::Microsoft365)
    };
    assert!(matches!(
        secret_for(&secrets, &account, &settings, now()),
        Err(SyncError::NoPassword)
    ));
    secrets.set(&token_key(&email), "garbage").unwrap();
    let err = secret_for(&secrets, &account, &settings, now()).unwrap_err();
    assert!(err.to_string().contains("sign in again"));
    // A plain store can't refresh an expired token.
    secrets
        .set(
            &token_key(&email),
            &serde_json::to_string(&tokens("old", -1)).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        secret_for(&secrets, &account, &settings, now()),
        Err(SyncError::OAuth(_))
    ));
}

#[test]
fn refreshing_wrapper_delegates_storage() {
    let secrets = Refreshing::new(
        Arc::new(MemorySecrets::default()),
        refresher(Arc::new(AtomicUsize::new(0))),
    );
    secrets.set("k", "v").unwrap();
    assert_eq!(secrets.get("k").unwrap().as_deref(), Some("v"));
    secrets.delete("k").unwrap();
    assert_eq!(secrets.get("k").unwrap(), None);
    let _ = Refreshing::networked(Arc::new(MemorySecrets::default()));
}

fn service(dir: &std::path::Path) -> SyncService {
    let connector: Connector = Arc::new(|_, token: &str| {
        if token == "at-1" {
            Ok(Box::new(Session) as BoxedSession)
        } else {
            Err(SyncError::Auth)
        }
    });
    SyncService::new(
        dir.join("mail.db"),
        Arc::new(MemorySecrets::default()),
        connector,
        Arc::new(|_, _, _, _| Ok(())),
    )
}

#[test]
fn the_service_signs_in_through_the_signer() {
    let dir = std::env::temp_dir().join(format!("sift-signin-{}", std::process::id()));
    let ok = service(&dir).with_signer(Arc::new(|provider, hint, cancelled| {
        assert_eq!(provider, Provider::Microsoft365);
        assert_eq!(hint.map(EmailAddress::as_str), Some("you@company.com"));
        assert!(!cancelled());
        Ok(signed_in("you@company.com"))
    }));
    let hint = EmailAddress::parse("you@company.com").ok();
    let account = ok
        .sign_in(
            Provider::Microsoft365,
            hint,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    assert_eq!(account.email.as_str(), "you@company.com");

    let blocked =
        service(&dir).with_signer(Arc::new(|_, _, _| Err(OAuthError::AdminApprovalRequired)));
    let err = blocked
        .sign_in(
            Provider::Microsoft365,
            None,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        SyncError::OAuth(OAuthError::AdminApprovalRequired)
    ));

    // The real signer needs a client ID, which tests don't have.
    let real = service(&dir);
    let err = real
        .sign_in(Provider::Google, None, Arc::new(AtomicBool::new(true)))
        .unwrap_err();
    assert!(matches!(
        err,
        SyncError::OAuth(OAuthError::NotConfigured { .. })
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plain_stores_cannot_refresh() {
    let err = MemorySecrets::default()
        .refresh_tokens(Provider::Google, &tokens("x", 0))
        .unwrap_err();
    assert!(err.to_string().contains("can't be refreshed"));
}
