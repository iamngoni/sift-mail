//! Small persistent settings: app preferences, custom views, per-account
//! settings and send-as aliases.
//!
//! Preferences are a typed key set ([`Pref`]); unknown stored values fall
//! back to the documented default rather than failing the app.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use super::{MailStore, StoreError, StoreResult};
use crate::domain::{AccountId, EmailAddress};

/// An app-wide preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pref {
    /// `light`, `dark` or `system`.
    Appearance,
    /// `true` shows AI summaries instead of previews in the list.
    Summaries,
    /// `true` shows the sidebar.
    Sidebar,
    /// `true` holds notifications except Needs you.
    HoldNotifications,
    /// `comfortable` or `compact`.
    Density,
    /// `true` once onboarding has been finished or skipped.
    Onboarded,
    /// Store id of the account new mail is sent from.
    DefaultFrom,
    /// `views` (sorted into tabs) or `plain` (one list).
    SortMode,
    /// Comma-separated view keys shown as tabs.
    Views,
    /// Comma-separated AI helpers that are on.
    AiFeatures,
    /// `hold`, `all` or `none`.
    NotifyMode,
    /// Comma-separated check-in times, such as `12:00,17:00`.
    Checkins,
    /// `claude`, `codex` or `off`.
    AiProvider,
}

impl Pref {
    fn key(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Summaries => "summaries",
            Self::Sidebar => "sidebar",
            Self::HoldNotifications => "hold_notifications",
            Self::Density => "density",
            Self::Onboarded => "onboarded",
            Self::DefaultFrom => "default_from",
            Self::SortMode => "sort_mode",
            Self::Views => "views",
            Self::AiFeatures => "ai_features",
            Self::NotifyMode => "notify_mode",
            Self::Checkins => "checkins",
            Self::AiProvider => "ai_provider",
        }
    }
}

/// Identifies a saved custom view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CustomViewId(pub(crate) i64);

/// A view the user described, shown as its own tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomView {
    /// Store id.
    pub id: CustomViewId,
    /// Tab label.
    pub label: String,
    /// Words a thread must contain, matched with full-text search.
    pub terms: String,
}

/// When an account may notify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotifyLevel {
    /// Only mail sorted into Needs you.
    #[default]
    NeedsYou,
    /// Every new message.
    Everything,
    /// Never.
    Nothing,
}

impl NotifyLevel {
    fn key(self) -> &'static str {
        match self {
            Self::NeedsYou => "needs_you",
            Self::Everything => "everything",
            Self::Nothing => "nothing",
        }
    }

    fn from_key(key: &str) -> Self {
        match key {
            "everything" => Self::Everything,
            "nothing" => Self::Nothing,
            _ => Self::NeedsYou,
        }
    }
}

/// How much mail an account keeps locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncRange {
    /// The last 30 days.
    Days30,
    /// The last 90 days.
    #[default]
    Days90,
    /// The last year.
    Year,
    /// Everything on the server.
    Everything,
}

impl SyncRange {
    /// Every choice, in menu order.
    pub const ALL: [SyncRange; 4] = [Self::Days30, Self::Days90, Self::Year, Self::Everything];

    /// Days to fetch on first sync; `None` means everything.
    pub fn days(self) -> Option<i64> {
        match self {
            Self::Days30 => Some(30),
            Self::Days90 => Some(90),
            Self::Year => Some(365),
            Self::Everything => None,
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Days30 => "30 days",
            Self::Days90 => "90 days",
            Self::Year => "1 year",
            Self::Everything => "Everything",
        }
    }

    fn from_days(days: i64) -> Self {
        match days {
            30 => Self::Days30,
            365 => Self::Year,
            0 => Self::Everything,
            _ => Self::Days90,
        }
    }

    fn stored(self) -> i64 {
        self.days().unwrap_or(0)
    }
}

/// Per-account settings from the Accounts screen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountSettings {
    /// Index into the six avatar tints.
    pub colour: u8,
    /// Added below new mail and replies.
    pub signature: String,
    /// Shown in All inboxes.
    pub in_all: bool,
    /// Notification level.
    pub notify: NotifyLevel,
    /// AI features may read this account's mail.
    pub ai: bool,
    /// How much mail to keep.
    pub range: SyncRange,
    /// Sync the account's calendar (Microsoft 365 and Google).
    pub calendar: bool,
    /// Sync the account's contacts (Microsoft 365 and Google).
    pub contacts: bool,
}

/// How an account's last sync went.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountHealth {
    /// When it last synced successfully.
    pub last_sync: Option<DateTime<Utc>>,
    /// The saved password or sign-in stopped working.
    pub needs_sign_in: bool,
    /// Why the last sync failed, if it did.
    pub last_error: Option<String>,
}

/// Number of avatar tints an account colour can pick from.
pub const ACCOUNT_COLOURS: u8 = 6;

impl MailStore {
    /// A preference, or `None` if never set.
    pub fn preference(&self, pref: Pref) -> StoreResult<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM preferences WHERE key = ?1",
                [pref.key()],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// A boolean preference, with a default for unset or unreadable values.
    pub fn flag(&self, pref: Pref, default: bool) -> StoreResult<bool> {
        Ok(match self.preference(pref)?.as_deref() {
            Some("true") => true,
            Some("false") => false,
            _ => default,
        })
    }

    /// Saves a preference.
    pub fn set_preference(&self, pref: Pref, value: &str) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO preferences (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![pref.key(), value],
        )?;
        Ok(())
    }

    /// Saved custom views, oldest first.
    pub fn custom_views(&self) -> StoreResult<Vec<CustomView>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, label, terms FROM custom_views ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok(CustomView {
                id: CustomViewId(row.get(0)?),
                label: row.get(1)?,
                terms: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Saves a custom view from a description such as "anything about the offsite".
    /// Returns `None` if the description has no searchable words.
    pub fn add_custom_view(&self, description: &str) -> StoreResult<Option<CustomView>> {
        let Some((label, terms)) = describe_view(description) else {
            return Ok(None);
        };
        self.conn.execute(
            "INSERT INTO custom_views (label, terms) VALUES (?1, ?2)",
            params![label, terms],
        )?;
        let id = CustomViewId(self.conn.last_insert_rowid());
        Ok(Some(CustomView { id, label, terms }))
    }

    /// Deletes a custom view.
    pub fn remove_custom_view(&self, id: CustomViewId) -> StoreResult<()> {
        self.conn
            .execute("DELETE FROM custom_views WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// Settings for an account.
    pub fn account_settings(&self, account: AccountId) -> StoreResult<AccountSettings> {
        Ok(self.conn.query_row(
            "SELECT colour, signature, in_all, notify, ai, sync_days, sync_calendar, sync_contacts
             FROM accounts WHERE id = ?1",
            [account.0],
            |r| {
                Ok(AccountSettings {
                    colour: u8::try_from(
                        r.get::<_, i64>(0)?.rem_euclid(i64::from(ACCOUNT_COLOURS)),
                    )
                    .unwrap_or_default(),
                    signature: r.get(1)?,
                    in_all: r.get(2)?,
                    notify: NotifyLevel::from_key(&r.get::<_, String>(3)?),
                    ai: r.get(4)?,
                    range: SyncRange::from_days(r.get(5)?),
                    calendar: r.get(6)?,
                    contacts: r.get(7)?,
                })
            },
        )?)
    }

    /// Saves settings for an account.
    pub fn save_account_settings(
        &self,
        account: AccountId,
        s: &AccountSettings,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE accounts SET colour = ?2, signature = ?3, in_all = ?4, notify = ?5, ai = ?6,
                sync_days = ?7, sync_calendar = ?8, sync_contacts = ?9 WHERE id = ?1",
            params![
                account.0,
                i64::from(s.colour % ACCOUNT_COLOURS),
                s.signature,
                s.in_all,
                s.notify.key(),
                s.ai,
                s.range.stored(),
                s.calendar,
                s.contacts
            ],
        )?;
        Ok(())
    }

    /// How the account's last sync went.
    pub fn account_health(&self, account: AccountId) -> StoreResult<AccountHealth> {
        Ok(self.conn.query_row(
            "SELECT last_sync, needs_sign_in, last_error FROM accounts WHERE id = ?1",
            [account.0],
            |r| {
                Ok(AccountHealth {
                    last_sync: r
                        .get::<_, Option<String>>(0)?
                        .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                        .map(|t| t.with_timezone(&Utc)),
                    needs_sign_in: r.get(1)?,
                    last_error: r.get(2)?,
                })
            },
        )?)
    }

    /// Holds an account back from sync while it is being set up (`false`), or
    /// lets it sync (`true`).
    pub fn set_ready(&self, account: AccountId, ready: bool) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE accounts SET ready = ?2 WHERE id = ?1",
            params![account.0, ready],
        )?;
        Ok(())
    }

    /// Messages stored for an account, in any state.
    pub fn message_count(&self, account: AccountId) -> StoreResult<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id = ?1",
            [account.0],
            |r| r.get(0),
        )?;
        Ok(usize::try_from(n).unwrap_or_default())
    }

    /// Records a successful sync.
    pub fn record_sync_ok(&self, account: AccountId, at: DateTime<Utc>) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE accounts SET last_sync = ?2, needs_sign_in = 0, last_error = NULL WHERE id = ?1",
            params![account.0, at.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Records a failed sync; `needs_sign_in` when the credentials stopped working.
    pub fn record_sync_error(
        &self,
        account: AccountId,
        needs_sign_in: bool,
        message: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE accounts SET needs_sign_in = ?2, last_error = ?3 WHERE id = ?1",
            params![account.0, needs_sign_in, message],
        )?;
        Ok(())
    }

    /// Renames an account.
    pub fn rename_account(&self, account: AccountId, name: &str) -> StoreResult<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Corrupt("an account name can't be empty".into()));
        }
        self.conn.execute(
            "UPDATE accounts SET name = ?2 WHERE id = ?1",
            params![account.0, name],
        )?;
        Ok(())
    }

    /// Moves an account one place up (`-1`) or down (`1`) in the sidebar.
    pub fn move_account(&self, account: AccountId, delta: i64) -> StoreResult<()> {
        let ids: Vec<i64> = self.accounts()?.iter().map(|a| a.id.0).collect();
        let Some(index) = ids.iter().position(|id| *id == account.0) else {
            return Ok(());
        };
        let target = i64::try_from(index).unwrap_or_default() + delta;
        let Ok(target) = usize::try_from(target) else {
            return Ok(());
        };
        if target >= ids.len() {
            return Ok(());
        }
        let mut order = ids;
        order.swap(index, target);
        let tx = self.conn.unchecked_transaction()?;
        for (position, id) in order.iter().enumerate() {
            tx.execute(
                "UPDATE accounts SET position = ?2 WHERE id = ?1",
                params![id, i64::try_from(position).unwrap_or_default()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Extra addresses an account can send as, besides its own.
    pub fn aliases(&self, account: AccountId) -> StoreResult<Vec<EmailAddress>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT address FROM aliases WHERE account_id = ?1 ORDER BY address")?;
        let rows = stmt.query_map([account.0], |row| row.get::<_, String>(0))?;
        rows.map(|r| super::parse_email(&r?)).collect()
    }

    /// Adds a send-as address.
    pub fn add_alias(&self, account: AccountId, address: &EmailAddress) -> StoreResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO aliases (account_id, address) VALUES (?1, ?2)",
            params![account.0, address.as_str()],
        )?;
        Ok(())
    }

    /// Removes a send-as address.
    pub fn remove_alias(&self, account: AccountId, address: &EmailAddress) -> StoreResult<()> {
        self.conn.execute(
            "DELETE FROM aliases WHERE account_id = ?1 AND address = ?2",
            params![account.0, address.as_str()],
        )?;
        Ok(())
    }
}

/// Filler words dropped from view descriptions.
const STOP_WORDS: [&str; 16] = [
    "anything",
    "everything",
    "all",
    "mail",
    "mails",
    "email",
    "emails",
    "about",
    "from",
    "with",
    "the",
    "and",
    "for",
    "my",
    "to",
    "show",
];

/// Turns "Anything about the offsite" into ("Offsite", "offsite").
pub fn describe_view(description: &str) -> Option<(String, String)> {
    let words: Vec<String> = description
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() > 1 && !STOP_WORDS.contains(&w.as_str()))
        .collect();
    if words.is_empty() {
        return None;
    }
    let label = words.join(" ");
    let mut chars = label.chars();
    let label: String = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    Some((label.chars().take(22).collect(), words.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Provider;

    fn store() -> (MailStore, AccountId) {
        let store = MailStore::open_in_memory().unwrap();
        let a = store
            .add_account(
                "Work",
                &EmailAddress::parse("you@company.com").unwrap(),
                Provider::Imap,
            )
            .unwrap();
        (store, a.id)
    }

    #[test]
    fn preferences_round_trip_with_defaults() {
        let (s, _) = store();
        assert_eq!(s.preference(Pref::Appearance).unwrap(), None);
        assert!(s.flag(Pref::Sidebar, true).unwrap());
        s.set_preference(Pref::Sidebar, "false").unwrap();
        assert!(!s.flag(Pref::Sidebar, true).unwrap());
        s.set_preference(Pref::Sidebar, "true").unwrap();
        assert!(s.flag(Pref::Sidebar, false).unwrap());
        s.set_preference(Pref::Sidebar, "maybe").unwrap();
        assert!(
            s.flag(Pref::Sidebar, true).unwrap(),
            "unreadable falls back"
        );
        for p in [
            Pref::SortMode,
            Pref::Views,
            Pref::AiFeatures,
            Pref::NotifyMode,
            Pref::Checkins,
            Pref::AiProvider,
            Pref::Appearance,
            Pref::Summaries,
            Pref::HoldNotifications,
            Pref::Density,
            Pref::Onboarded,
        ] {
            s.set_preference(p, "x").unwrap();
            assert_eq!(s.preference(p).unwrap().as_deref(), Some("x"));
        }
    }

    #[test]
    fn custom_views_from_descriptions() {
        let (s, _) = store();
        assert_eq!(s.add_custom_view("anything about the").unwrap(), None);
        let v = s
            .add_custom_view("Anything about the offsite")
            .unwrap()
            .unwrap();
        assert_eq!((v.label.as_str(), v.terms.as_str()), ("Offsite", "offsite"));
        let w = s
            .add_custom_view("Mail with attachments from Finance")
            .unwrap()
            .unwrap();
        assert_eq!(w.terms, "attachments finance");
        assert_eq!(s.custom_views().unwrap().len(), 2);
        s.remove_custom_view(v.id).unwrap();
        assert_eq!(s.custom_views().unwrap(), [w]);
        assert_eq!(
            describe_view("a really really long description of things")
                .unwrap()
                .0
                .chars()
                .count(),
            22
        );
    }

    #[test]
    fn account_settings_round_trip() {
        let (s, id) = store();
        let defaults = s.account_settings(id).unwrap();
        assert_eq!(defaults.range, SyncRange::Days90);
        assert!(defaults.in_all && defaults.ai);
        assert_eq!(defaults.notify, NotifyLevel::NeedsYou);
        for (range, notify) in [
            (SyncRange::Days30, NotifyLevel::Everything),
            (SyncRange::Year, NotifyLevel::Nothing),
            (SyncRange::Everything, NotifyLevel::NeedsYou),
        ] {
            let settings = AccountSettings {
                colour: 9,
                signature: "— Ngoni".into(),
                in_all: false,
                notify,
                ai: false,
                range,
                calendar: false,
                contacts: true,
            };
            s.save_account_settings(id, &settings).unwrap();
            let back = s.account_settings(id).unwrap();
            assert_eq!(back.colour, 3, "colours wrap");
            assert_eq!((back.range, back.notify), (range, notify));
            assert!(!back.in_all && !back.ai && !back.calendar && back.contacts);
        }
        assert_eq!(
            SyncRange::ALL.map(SyncRange::label),
            ["30 days", "90 days", "1 year", "Everything"]
        );
    }

    #[test]
    fn sync_health_is_recorded() {
        let (s, id) = store();
        assert_eq!(s.account_health(id).unwrap(), AccountHealth::default());
        s.record_sync_error(id, true, "the server rejected the sign-in")
            .unwrap();
        let bad = s.account_health(id).unwrap();
        assert!(bad.needs_sign_in);
        assert_eq!(
            bad.last_error.as_deref(),
            Some("the server rejected the sign-in")
        );
        let now = chrono::Utc::now();
        s.record_sync_ok(id, now).unwrap();
        let good = s.account_health(id).unwrap();
        assert!(!good.needs_sign_in && good.last_error.is_none());
        assert_eq!(good.last_sync.map(|t| t.timestamp()), Some(now.timestamp()));
        assert_eq!(s.message_count(id).unwrap(), 0);
        s.set_ready(id, false).unwrap();
        assert!(s.imap_accounts().unwrap().is_empty());
        s.set_ready(id, true).unwrap();
        s.set_preference(Pref::DefaultFrom, "1").unwrap();
        assert_eq!(
            s.preference(Pref::DefaultFrom).unwrap().as_deref(),
            Some("1")
        );
    }

    #[test]
    fn rename_reorder_and_aliases() {
        let (s, work) = store();
        let personal = s
            .add_account(
                "Personal",
                &EmailAddress::parse("you@gmail.com").unwrap(),
                Provider::Google,
            )
            .unwrap()
            .id;
        assert!(s.rename_account(work, "  ").is_err());
        s.rename_account(work, " Office ").unwrap();
        s.move_account(personal, -1).unwrap();
        let names: Vec<_> = s.accounts().unwrap().into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["Personal", "Office"]);
        s.move_account(personal, -1).unwrap();
        s.move_account(work, 5).unwrap();
        s.move_account(AccountId(99), 1).unwrap();
        assert_eq!(
            s.accounts().unwrap()[0].name,
            "Personal",
            "out-of-range moves are ignored"
        );

        let team = EmailAddress::parse("team@company.com").unwrap();
        s.add_alias(work, &team).unwrap();
        s.add_alias(work, &team).unwrap();
        assert_eq!(s.aliases(work).unwrap(), std::slice::from_ref(&team));
        s.remove_alias(work, &team).unwrap();
        assert!(s.aliases(work).unwrap().is_empty());
    }
}
