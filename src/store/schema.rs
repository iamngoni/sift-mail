//! Schema migrations, tracked with SQLite's `user_version`.
//!
//! Each entry runs once, in order, inside a transaction. Never edit a shipped
//! migration; append a new one.

use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // 1: accounts and messages with per-message triage state.
    "CREATE TABLE accounts (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        email TEXT NOT NULL UNIQUE,
        provider TEXT NOT NULL,
        created_at INTEGER NOT NULL DEFAULT (unixepoch())
     );
     CREATE TABLE messages (
        id INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        remote_id TEXT NOT NULL,
        thread_key TEXT NOT NULL,
        from_name TEXT,
        from_email TEXT NOT NULL,
        subject TEXT NOT NULL,
        preview TEXT NOT NULL,
        body_text TEXT NOT NULL,
        received_at INTEGER NOT NULL,
        is_read INTEGER NOT NULL,
        has_attachments INTEGER NOT NULL,
        view TEXT NOT NULL,
        state TEXT NOT NULL DEFAULT 'inbox' CHECK (state IN ('inbox', 'done', 'snoozed')),
        snoozed_until INTEGER,
        CHECK ((state = 'snoozed') = (snoozed_until IS NOT NULL)),
        UNIQUE (account_id, remote_id)
     );
     CREATE INDEX messages_thread ON messages (account_id, thread_key);
     CREATE INDEX messages_inbox ON messages (state, received_at DESC);",
    // 2: IMAP server settings and sync cursors, one row per IMAP account.
    "CREATE TABLE imap_accounts (
        account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
        host TEXT NOT NULL,
        port INTEGER NOT NULL,
        username TEXT NOT NULL,
        uid_validity INTEGER,
        last_uid INTEGER NOT NULL DEFAULT 0
     );",
    // 3: message ids for reply threading; optional SMTP overrides per account.
    "ALTER TABLE messages ADD COLUMN message_id TEXT;
     ALTER TABLE messages ADD COLUMN refs TEXT NOT NULL DEFAULT '';
     ALTER TABLE imap_accounts ADD COLUMN smtp_host TEXT;
     ALTER TABLE imap_accounts ADD COLUMN smtp_port INTEGER;",
];

/// Brings `conn` up to the latest schema.
pub(super) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current: usize = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|v| usize::try_from(v).unwrap_or_default())?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(
            None,
            "user_version",
            i64::try_from(index + 1).unwrap_or(i64::MAX),
        )?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
/// The schema version this build writes.
pub(super) fn latest_version() -> usize {
    MIGRATIONS.len()
}
