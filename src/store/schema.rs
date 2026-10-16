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
    // 4: how each account signs in (password or OAuth).
    "ALTER TABLE imap_accounts ADD COLUMN auth TEXT NOT NULL DEFAULT 'password';",
    // 5: search, custom views, preferences, per-account settings, attachments.
    "CREATE VIRTUAL TABLE messages_fts USING fts5(
        subject, from_name, from_email, body_text,
        content='messages', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
     );
     INSERT INTO messages_fts(rowid, subject, from_name, from_email, body_text)
        SELECT id, subject, coalesce(from_name, ''), from_email, body_text FROM messages;
     CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
        INSERT INTO messages_fts(rowid, subject, from_name, from_email, body_text)
        VALUES (new.id, new.subject, coalesce(new.from_name, ''), new.from_email, new.body_text);
     END;
     CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
        INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_email, body_text)
        VALUES ('delete', old.id, old.subject, coalesce(old.from_name, ''), old.from_email, old.body_text);
     END;
     CREATE TABLE preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE custom_views (
        id INTEGER PRIMARY KEY,
        label TEXT NOT NULL,
        terms TEXT NOT NULL
     );
     CREATE TABLE aliases (
        account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        address TEXT NOT NULL,
        PRIMARY KEY (account_id, address)
     );
     CREATE TABLE attachments (
        id INTEGER PRIMARY KEY,
        message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        mime TEXT NOT NULL,
        size INTEGER NOT NULL,
        path TEXT
     );
     CREATE INDEX attachments_message ON attachments (message_id);
     ALTER TABLE accounts ADD COLUMN colour INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE accounts ADD COLUMN signature TEXT NOT NULL DEFAULT '';
     ALTER TABLE accounts ADD COLUMN in_all INTEGER NOT NULL DEFAULT 1;
     ALTER TABLE accounts ADD COLUMN notify TEXT NOT NULL DEFAULT 'needs_you';
     ALTER TABLE accounts ADD COLUMN ai INTEGER NOT NULL DEFAULT 1;
     ALTER TABLE accounts ADD COLUMN sync_days INTEGER NOT NULL DEFAULT 90;
     ALTER TABLE accounts ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
     UPDATE accounts SET position = id;",
    // 6: what each account syncs, and how its last sync went.
    "ALTER TABLE accounts ADD COLUMN sync_calendar INTEGER NOT NULL DEFAULT 1;
     ALTER TABLE accounts ADD COLUMN sync_contacts INTEGER NOT NULL DEFAULT 1;
     ALTER TABLE accounts ADD COLUMN last_sync TEXT;
     ALTER TABLE accounts ADD COLUMN needs_sign_in INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE accounts ADD COLUMN last_error TEXT;",
    // 7: accounts being set up are not synced until the user finishes.
    "ALTER TABLE accounts ADD COLUMN ready INTEGER NOT NULL DEFAULT 1;",
    // 8: AI answers, so a thread is summarised once.
    "CREATE TABLE ai_cache (key TEXT PRIMARY KEY, value TEXT NOT NULL, created_at TEXT NOT NULL);",
    // 9: the calendar.
    "CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        uid TEXT NOT NULL,
        sequence INTEGER NOT NULL DEFAULT 0,
        title TEXT NOT NULL,
        starts_at TEXT NOT NULL,
        ends_at TEXT NOT NULL,
        all_day INTEGER NOT NULL DEFAULT 0,
        location TEXT,
        organizer TEXT,
        attendees TEXT NOT NULL DEFAULT '',
        kind TEXT NOT NULL,
        rsvp TEXT,
        thread_key TEXT,
        UNIQUE (account_id, uid)
     );
     CREATE INDEX events_time ON events (starts_at);",
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
