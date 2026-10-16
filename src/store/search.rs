//! Full-text search over every stored message (SQLite FTS5).
//!
//! User text never reaches FTS5 syntax directly: [`fts_query`] turns it into
//! quoted prefix terms, so quotes, `AND`, `NEAR` or `*` typed by the user are
//! searched for, not interpreted.

use rusqlite::params;

use crate::domain::AccountId;

use super::{MailStore, StoreError, StoreResult, ThreadSummary, raw_summary};

/// Most results a search returns.
pub const SEARCH_LIMIT: usize = 50;

/// Turns typed text into a safe FTS5 query: every word must appear, each
/// matched as a prefix (`"tom"*`). `None` if there's nothing to search for.
pub fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '@' && c != '.')
        .map(|w| w.trim_matches('.'))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

impl MailStore {
    /// Threads with a message matching `text`, best matches first, in any
    /// state (done and snoozed mail is searchable too).
    pub fn search(&self, text: &str) -> StoreResult<Vec<ThreadSummary>> {
        self.find(text, false, None)
    }

    /// Inbox threads for a custom view: those matching `terms`, newest first,
    /// for one account or every account shown in All inboxes.
    pub fn custom_view_threads(
        &self,
        terms: &str,
        account: Option<AccountId>,
    ) -> StoreResult<Vec<ThreadSummary>> {
        let mut threads = self.find(terms, true, account)?;
        threads.sort_by_key(|t| std::cmp::Reverse(t.received_at));
        Ok(threads)
    }

    fn find(
        &self,
        text: &str,
        inbox_only: bool,
        account: Option<AccountId>,
    ) -> StoreResult<Vec<ThreadSummary>> {
        let Some(query) = fts_query(text) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare_cached(
            "WITH scored AS MATERIALIZED (
                SELECT rowid AS id, bm25(messages_fts) AS score
                FROM messages_fts WHERE messages_fts MATCH ?1
             ),
             hits AS (
                SELECT m.account_id, m.thread_key, MIN(s.score) AS score
                FROM scored s JOIN messages m ON m.id = s.id
                GROUP BY m.account_id, m.thread_key
             ),
             ranked AS (
                SELECT m.*,
                    ROW_NUMBER() OVER (PARTITION BY m.account_id, m.thread_key
                                       ORDER BY received_at DESC, m.id DESC) AS rn,
                    COUNT(*) OVER (PARTITION BY m.account_id, m.thread_key) AS n,
                    MIN(is_read) OVER (PARTITION BY m.account_id, m.thread_key) AS all_read,
                    h.score
                FROM messages m JOIN hits h
                  ON h.account_id = m.account_id AND h.thread_key = m.thread_key
                WHERE (?3 = 0 OR m.state = 'inbox')
                  AND (m.account_id = ?4 OR (?4 IS NULL AND m.account_id IN (SELECT id FROM accounts WHERE in_all)))
             )
             SELECT id, account_id, thread_key, from_name, from_email, subject, preview,
                    received_at, n, all_read, has_attachments, view
             FROM ranked WHERE rn = 1
             ORDER BY score, received_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![
                query,
                i64::try_from(SEARCH_LIMIT).unwrap_or(i64::MAX),
                inbox_only,
                account.map(|a| a.0)
            ],
            raw_summary,
        )?;
        rows.map(|row| {
            row.map_err(StoreError::from)
                .and_then(super::RawSummary::parse)
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Contact, EmailAddress, IncomingMessage, Provider, Signals, TriageState};

    fn store() -> MailStore {
        let store = MailStore::open_in_memory().unwrap();
        let a = store
            .add_account(
                "Work",
                &EmailAddress::parse("you@company.com").unwrap(),
                Provider::Imap,
            )
            .unwrap();
        let mail = [
            (
                "1",
                "q4",
                "Priya Raman",
                "priya@company.com",
                "Q4 budget numbers",
                "Headcount by Monday",
            ),
            (
                "2",
                "vendor",
                "Tom Okafor",
                "tom@company.com",
                "Vendor contract",
                "Section 4 lock-in",
            ),
            (
                "3",
                "offsite",
                "Kofi",
                "kofi@company.com",
                "Offsite planning",
                "Lisbon it is, café dinner",
            ),
        ];
        for (remote, thread, name, email, subject, body) in mail {
            store
                .upsert(&IncomingMessage {
                    account: a.id,
                    remote_id: remote.into(),
                    thread_key: thread.into(),
                    from: Contact::new(Some(name), EmailAddress::parse(email).unwrap()),
                    subject: subject.into(),
                    body_text: body.into(),
                    received_at: chrono::Utc::now(),
                    is_read: false,
                    has_attachments: false,
                    signals: Signals::default(),
                    message_id: None,
                    references: Vec::new(),
                })
                .unwrap();
        }
        store
    }

    fn subjects(store: &MailStore, text: &str) -> Vec<String> {
        store
            .search(text)
            .unwrap()
            .into_iter()
            .map(|t| t.subject)
            .collect()
    }

    #[test]
    fn finds_by_subject_body_sender_and_prefix() {
        let s = store();
        assert_eq!(subjects(&s, "budget"), ["Q4 budget numbers"]);
        assert_eq!(subjects(&s, "lock"), ["Vendor contract"]);
        assert_eq!(subjects(&s, "tom"), ["Vendor contract"]);
        assert_eq!(subjects(&s, "priya@company.com"), ["Q4 budget numbers"]);
        assert_eq!(
            subjects(&s, "headc mond"),
            ["Q4 budget numbers"],
            "prefixes, all words"
        );
        assert_eq!(
            subjects(&s, "cafe"),
            ["Offsite planning"],
            "diacritics folded"
        );
        assert!(subjects(&s, "nothing-like-this").is_empty());
    }

    #[test]
    fn done_mail_is_searchable_and_syntax_is_inert() {
        let s = store();
        let t = s.search("vendor").unwrap()[0].thread.clone();
        s.set_state(&[t], TriageState::Done).unwrap();
        assert_eq!(subjects(&s, "vendor"), ["Vendor contract"]);
        assert!(subjects(&s, "\"").is_empty());
        assert!(subjects(&s, "NEAR( OR AND *").is_empty());
        assert_eq!(fts_query("  "), None);
        assert!(
            s.custom_view_threads("vendor", None).unwrap().is_empty(),
            "done is not in a view"
        );
        assert_eq!(s.custom_view_threads("offsite", None).unwrap().len(), 1);
        assert_eq!(
            fts_query("tom's \"q4\"").as_deref(),
            Some("\"tom\"* \"s\"* \"q4\"*")
        );
    }
}
