//! Attachments: metadata in SQLite, contents as files in an `attachments`
//! folder next to the database, one subfolder per message.
//!
//! File names come from senders, so they are reduced to a safe single path
//! component before touching the disk.

use std::path::PathBuf;

use rusqlite::params;

use super::{MailStore, StoreError, StoreResult};
use crate::domain::MessageId;
use crate::sync::parse::AttachmentData;

/// A stored attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// Store id.
    pub id: i64,
    /// File name.
    pub name: String,
    /// MIME type.
    pub mime: String,
    /// Size in bytes.
    pub size: u64,
    /// Where the contents are saved, if they are (in-memory stores keep none).
    pub path: Option<PathBuf>,
}

impl Attachment {
    /// The short type badge on a file chip, such as `PDF` or `XLSX`.
    pub fn badge(&self) -> String {
        std::path::Path::new(&self.name)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.len() <= 4)
            .map_or_else(|| "FILE".to_owned(), str::to_uppercase)
    }

    /// Human size, such as `48 KB` or `1.2 MB`.
    pub fn size_label(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        match self.size {
            s if s < KB => format!("{s} B"),
            s if s < MB => format!("{} KB", s.div_ceil(KB)),
            s => format!("{}.{} MB", s / MB, (s % MB) * 10 / MB),
        }
    }
}

/// Turns a sender's file name into one safe path component.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "attachment".to_owned()
    } else {
        trimmed.chars().take(120).collect()
    }
}

impl MailStore {
    /// Where attachment files go, or `None` for an in-memory store.
    fn attachments_dir(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|dir| dir.join("attachments"))
    }

    /// Saves a message's attachments once; later calls for the same message
    /// do nothing, so re-syncing never duplicates files.
    pub fn save_attachments(
        &self,
        message: MessageId,
        files: &[AttachmentData],
    ) -> StoreResult<()> {
        let existing: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM attachments WHERE message_id = ?1",
            [message.0],
            |row| row.get(0),
        )?;
        if existing > 0 || files.is_empty() {
            return Ok(());
        }
        let folder = self
            .attachments_dir()
            .map(|d| d.join(message.0.to_string()));
        if let Some(folder) = &folder {
            std::fs::create_dir_all(folder).map_err(|e| StoreError::CreateDir {
                path: folder.clone(),
                source: e,
            })?;
        }
        for (i, file) in files.iter().enumerate() {
            let path = match &folder {
                Some(folder) => {
                    let path = folder.join(format!("{}-{}", i + 1, safe_name(&file.name)));
                    std::fs::write(&path, &file.bytes).map_err(|e| StoreError::WriteFile {
                        path: path.clone(),
                        source: e,
                    })?;
                    Some(path.to_string_lossy().into_owned())
                }
                None => None,
            };
            self.conn.execute(
                "INSERT INTO attachments (message_id, name, mime, size, path)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    message.0,
                    file.name,
                    file.mime,
                    i64::try_from(file.bytes.len()).unwrap_or(i64::MAX),
                    path
                ],
            )?;
        }
        Ok(())
    }

    /// A message's attachments, in the order they were sent.
    pub fn attachments(&self, message: MessageId) -> StoreResult<Vec<Attachment>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, mime, size, path FROM attachments WHERE message_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([message.0], |row| {
            Ok(Attachment {
                id: row.get(0)?,
                name: row.get(1)?,
                mime: row.get(2)?,
                size: u64::try_from(row.get::<_, i64>(3)?).unwrap_or_default(),
                path: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Contact, EmailAddress, IncomingMessage, Provider, Signals};

    fn message(store: &MailStore) -> MessageId {
        let a = store
            .add_account(
                "Work",
                &EmailAddress::parse("you@company.com").unwrap(),
                Provider::Imap,
            )
            .unwrap();
        store
            .upsert(&IncomingMessage {
                account: a.id,
                remote_id: "1".into(),
                thread_key: "t".into(),
                from: Contact::new(None, EmailAddress::parse("p@company.com").unwrap()),
                subject: "Files".into(),
                body_text: "Attached".into(),
                received_at: chrono::Utc::now(),
                is_read: false,
                has_attachments: true,
                signals: Signals::default(),
                message_id: None,
                references: Vec::new(),
            })
            .unwrap()
    }

    fn file(name: &str, len: usize) -> AttachmentData {
        AttachmentData {
            name: name.into(),
            mime: "application/pdf".into(),
            bytes: vec![b'x'; len],
        }
    }

    #[test]
    fn saves_files_beside_the_database_once() {
        let dir = std::env::temp_dir().join(format!("sift-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = MailStore::open(&dir.join("mail.db")).unwrap();
        let id = message(&store);
        store
            .save_attachments(id, &[file("../../evil.pdf", 10), file("Budget.xlsx", 2048)])
            .unwrap();
        store.save_attachments(id, &[file("again.pdf", 1)]).unwrap();
        let saved = store.attachments(id).unwrap();
        assert_eq!(saved.len(), 2, "a second save is ignored");
        let path = saved[0].path.clone().unwrap();
        assert!(path.starts_with(dir.join("attachments")));
        assert_eq!(path.file_name().unwrap(), "1-_.._evil.pdf");
        assert_eq!(std::fs::read(&path).unwrap().len(), 10);
        assert_eq!(
            (saved[1].badge(), saved[1].size_label()),
            ("XLSX".into(), "2 KB".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn in_memory_stores_keep_metadata_only() {
        let store = MailStore::open_in_memory().unwrap();
        let id = message(&store);
        store.save_attachments(id, &[]).unwrap();
        assert!(store.attachments(id).unwrap().is_empty());
        store.save_attachments(id, &[file("notes", 3)]).unwrap();
        let saved = store.attachments(id).unwrap();
        assert_eq!(saved[0].path, None);
        assert_eq!(
            (saved[0].badge(), saved[0].size_label()),
            ("FILE".into(), "3 B".into())
        );
    }

    #[test]
    fn names_and_sizes_are_tidied() {
        assert_eq!(safe_name(" .. "), "attachment");
        assert_eq!(safe_name("a<b>:c|d?.txt"), "a_b__c_d_.txt");
        assert_eq!(safe_name(&"x".repeat(300)).len(), 120);
        let big = Attachment {
            id: 1,
            name: "deck.pptx".into(),
            mime: String::new(),
            size: 1_572_864,
            path: None,
        };
        assert_eq!(
            (big.badge(), big.size_label()),
            ("PPTX".into(), "1.5 MB".into())
        );
        let long_ext = Attachment {
            name: "a.verylong".into(),
            ..big
        };
        assert_eq!(long_ext.badge(), "FILE");
    }
}
