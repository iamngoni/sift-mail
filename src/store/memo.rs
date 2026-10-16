//! Remembered AI answers, and the people you've heard from.
//!
//! AI answers are keyed by what they describe (a thread at its newest
//! message), so a thread is summarised once and a new reply invalidates it.
//! Contacts come from senders already in the store, most frequent first.

use chrono::Utc;
use rusqlite::{OptionalExtension, params};

use super::{MailStore, StoreResult};
use crate::domain::{Contact, EmailAddress};

/// Most contacts a suggestion list shows.
pub const CONTACT_LIMIT: usize = 5;

impl MailStore {
    /// A remembered AI answer.
    pub fn memo(&self, key: &str) -> StoreResult<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM ai_cache WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Remembers an AI answer.
    pub fn remember(&self, key: &str, value: &str) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO ai_cache (key, value, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, created_at = excluded.created_at",
            params![key, value, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// The newest message id, or 0 for an empty store. Taken before a sync to
    /// find what it brought.
    pub fn latest_message_id(&self) -> StoreResult<i64> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(id), 0) FROM messages", [], |r| {
                r.get(0)
            })?)
    }

    /// Unread inbox mail stored after `id` that arrived after `received_after`
    /// (so a first sync of old mail stays quiet), oldest first: account, view,
    /// sender and subject.
    pub fn new_mail_since(
        &self,
        id: i64,
        received_after: chrono::DateTime<Utc>,
    ) -> StoreResult<
        Vec<(
            crate::domain::AccountId,
            crate::domain::TriageView,
            String,
            String,
        )>,
    > {
        let mut stmt = self.conn.prepare_cached(
            "SELECT account_id, view, COALESCE(NULLIF(from_name, ''), from_email), subject
             FROM messages WHERE id > ?1 AND received_at >= ?2 AND state = 'inbox' AND is_read = 0 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id, received_after.timestamp()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (account, view, from, subject) = row?;
            let view = crate::domain::TriageView::from_key(&view)
                .ok_or_else(|| super::StoreError::Corrupt(format!("unknown view {view:?}")))?;
            Ok((crate::domain::AccountId(account), view, from, subject))
        })
        .collect()
    }

    /// People whose name or address starts with `prefix` (any word of the
    /// name), most frequent senders first. An empty prefix matches nobody.
    pub fn contacts(&self, prefix: &str) -> StoreResult<Vec<Contact>> {
        let prefix = prefix.trim().to_lowercase();
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT from_email, MAX(from_name), COUNT(*) AS n FROM messages
             GROUP BY lower(from_email)
             ORDER BY n DESC, from_email",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (email, name) = row?;
            let matches = email.to_lowercase().starts_with(&prefix)
                || name.as_deref().is_some_and(|n| {
                    n.to_lowercase()
                        .split_whitespace()
                        .any(|w| w.starts_with(&prefix))
                });
            if !matches {
                continue;
            }
            if let Ok(address) = EmailAddress::parse(&email) {
                out.push(Contact::new(name.as_deref(), address));
            }
            if out.len() == CONTACT_LIMIT {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::demo;

    #[test]
    fn memos_are_remembered_and_replaced() {
        let s = MailStore::open_in_memory().unwrap();
        assert_eq!(s.memo("k").unwrap(), None);
        s.remember("k", "one").unwrap();
        s.remember("k", "two").unwrap();
        assert_eq!(s.memo("k").unwrap().as_deref(), Some("two"));
    }

    #[test]
    fn contacts_match_names_and_addresses() {
        let s = MailStore::open_in_memory().unwrap();
        demo::seed(&s, Utc::now()).unwrap();
        let names = |p: &str| -> Vec<String> {
            s.contacts(p)
                .unwrap()
                .into_iter()
                .map(|c| c.display().to_owned())
                .collect()
        };
        assert_eq!(names("pri"), ["Priya Raman"], "two messages, one person");
        assert_eq!(names("okafor"), ["Tom Okafor"]);
        assert!(names("tom@").is_empty() || names("tom")[0] == "Tom Okafor");
        assert!(names(" ").is_empty());
        assert!(names("zzz").is_empty());
        assert!(names("a").len() <= CONTACT_LIMIT);
    }
}
