//! Durable storage for authored workspace documents (issue #17).
//!
//! The Studio editor persists every validated [`DeckDocument`] and
//! [`ProfileDocument`] through this store: one row per document, keyed by a
//! closed-vocabulary kind plus the document's durable id. Writes follow the
//! crate's atomic autosave discipline — each mutation is exactly one
//! committed transaction under WAL + `synchronous=FULL`, so a returned `Ok`
//! survives immediate process death or power loss, and a refused write can
//! never tear (the prior committed state stands).
//!
//! The store treats document bytes as opaque: structural truth lives in
//! `openstream-domain`, which validates every document on both write
//! (`Document::to_json_string`) and read (`Document::from_json_str`). This
//! layer never interprets or repairs content; a row that no longer decodes
//! fails closed with [`StorageError::UnrecognizedSchema`]'s sibling
//! [`StorageError::Corrupted`] at load time instead of being skipped
//! silently. Callers surface that honestly (degraded editing) rather than
//! guessing.
//!
//! The database opens through the shared pipeline ([`super::migrations`]):
//! integrity verification, forward-only migrations with verified backup
//! before any upgrade, and typed refusals for foreign or newer schemas. Use
//! [`crate::sqlite::recover`] when an open reported corruption.

use super::migrations::{self, MIGRATIONS};
use crate::sqlite::error::StorageError;
use std::path::{Path, PathBuf};

/// Closed vocabulary of persisted document kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// A versioned deck document (`DeckDocument` JSON).
    Deck,
    /// A versioned profile document (`ProfileDocument` JSON).
    Profile,
}

impl DocumentKind {
    /// The stable SQL/token spelling of this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deck => "deck",
            Self::Profile => "profile",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token {
            "deck" => Some(Self::Deck),
            "profile" => Some(Self::Profile),
            _ => None,
        }
    }
}

/// One document row read back from the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDocument {
    /// Which repository the JSON belongs to.
    pub kind: DocumentKind,
    /// Canonical UUIDv7 string of the enclosed entity.
    pub id: String,
    /// The document JSON exactly as persisted (validated on decode).
    pub json: String,
}

/// SQLite-backed authored-document store. Owns its connection.
#[derive(Debug)]
pub struct WorkspaceStore {
    connection: rusqlite::Connection,
    path: PathBuf,
}

impl WorkspaceStore {
    /// Opens (or creates) the workspace database at `path` through the full
    /// open pipeline. Existing v1 databases upgrade through the standard
    /// verified-backup path; use [`crate::sqlite::recover`] when a prior
    /// open reported corruption.
    ///
    /// # Errors
    /// [`StorageError`] for every fail-closed case of the open pipeline.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let connection = migrations::open_with(path, MIGRATIONS)?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
        })
    }

    /// The database file this store owns.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads every stored document. Ordering is deterministic (kind, then
    /// durable id) so callers observe one stable sequence.
    ///
    /// # Errors
    /// [`StorageError`] when rows cannot be read, or when any row carries a
    /// kind outside the closed vocabulary (structural damage, never guessed).
    pub fn load_all(&self) -> Result<Vec<StoredDocument>, StorageError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT kind, id, document_json FROM workspace_documents
                 ORDER BY kind ASC, id ASC",
            )
            .map_err(|_| StorageError::Corrupted {
                stage: super::error::CorruptionStage::Probe,
            })?;
        let mut rows = statement.query([]).map_err(|_| StorageError::Unavailable)?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next().map_err(|_| StorageError::Unavailable)? {
            let kind_token: String = row.get(0).map_err(|_| StorageError::Corrupted {
                stage: super::error::CorruptionStage::Content,
            })?;
            let Some(kind) = DocumentKind::from_token(&kind_token) else {
                return Err(StorageError::Corrupted {
                    stage: super::error::CorruptionStage::Content,
                });
            };
            let id: String = row.get(1).map_err(|_| StorageError::Unavailable)?;
            let json: String = row.get(2).map_err(|_| StorageError::Unavailable)?;
            documents.push(StoredDocument { kind, id, json });
        }
        Ok(documents)
    }

    /// Replaces the entire stored contents with `documents` inside ONE
    /// transaction. The editor commits whole-workspace snapshots after every
    /// accepted edit, undo, or redo, so all-or-nothing rewrites keep every
    /// crash window trivially consistent: the store shows either the exact
    /// previous snapshot or the exact new one, never a hybrid.
    ///
    /// # Errors
    /// [`StorageError::Unavailable`] when the transaction could not commit;
    /// the prior committed state stands untouched.
    pub fn rewrite_all(
        &mut self,
        documents: &[StoredDocument],
        updated_at_wall_ms: i64,
    ) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|_| StorageError::Unavailable)?;
        transaction
            .execute("DELETE FROM workspace_documents", [])
            .map_err(|_| StorageError::Unavailable)?;
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO workspace_documents
                     (kind, id, document_json, updated_at_wall_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|_| StorageError::Unavailable)?;
            for document in documents {
                insert
                    .execute(rusqlite::params![
                        document.kind.as_str(),
                        document.id,
                        document.json,
                        updated_at_wall_ms
                    ])
                    .map_err(|_| StorageError::Unavailable)?;
            }
        }
        transaction.commit().map_err(|_| StorageError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentKind, StoredDocument, WorkspaceStore};
    use crate::sqlite::migrations::SCHEMA_VERSION;
    use std::path::PathBuf;

    fn scratch(label: &str) -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join(format!("{label}.sqlite3"));
        (temp, path)
    }

    fn deck_doc(id: &str) -> StoredDocument {
        StoredDocument {
            kind: DocumentKind::Deck,
            id: id.to_owned(),
            json: format!(r#"{{"deck":"{id}"}}"#),
        }
    }

    #[test]
    fn fresh_store_opens_at_current_schema_version() {
        let (_dir, path) = scratch("workspace-fresh");
        let store = WorkspaceStore::open(&path).expect("open");
        assert_eq!(
            rusqlite::Connection::open(&path)
                .and_then(|c| c.query_row(
                    "SELECT value FROM openstream_schema WHERE key = 'schema_version'",
                    [],
                    |row| row.get::<_, u32>(0)
                ))
                .expect("anchor readable"),
            SCHEMA_VERSION
        );
        assert!(store.load_all().expect("empty load").is_empty());
    }

    #[test]
    fn rewrite_and_load_round_trip_is_deterministic() {
        let (_dir, path) = scratch("workspace-roundtrip");
        let mut store = WorkspaceStore::open(&path).expect("open");
        let docs = vec![deck_doc("b"), deck_doc("a")];
        store.rewrite_all(&docs, 1_755_945_600_000).expect("write");
        // Deterministic order: sorted by (kind, id), not insertion order.
        let loaded = WorkspaceStore::open(&path)
            .expect("reopen")
            .load_all()
            .expect("load");
        assert_eq!(
            loaded,
            vec![deck_doc("a"), deck_doc("b")],
            "load order is (kind, id), independent of write order"
        );
    }

    #[test]
    fn rewrite_replaces_the_whole_snapshot_atomically() {
        let (_dir, path) = scratch("workspace-replace");
        let mut store = WorkspaceStore::open(&path).expect("open");
        store
            .rewrite_all(&[deck_doc("keep-1"), deck_doc("drop-2")], 10)
            .expect("first write");
        store
            .rewrite_all(&[deck_doc("keep-1"), deck_doc("new-3")], 20)
            .expect("rewrite");
        let loaded = store.load_all().expect("load");
        let ids: Vec<&str> = loaded.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            ids,
            ["keep-1", "new-3"],
            "the rewrite removed the dropped row"
        );
    }

    #[test]
    fn profiles_are_a_distinct_kind_namespace() {
        let (_dir, path) = scratch("workspace-kinds");
        let mut store = WorkspaceStore::open(&path).expect("open");
        store
            .rewrite_all(
                &[
                    StoredDocument {
                        kind: DocumentKind::Deck,
                        id: "same-id".into(),
                        json: "{}".into(),
                    },
                    StoredDocument {
                        kind: DocumentKind::Profile,
                        id: "same-id".into(),
                        json: "{}".into(),
                    },
                ],
                5,
            )
            .expect("write");
        let loaded = store.load_all().expect("load");
        assert_eq!(loaded.len(), 2, "kind is part of the primary key");
        assert_eq!(loaded[0].kind, DocumentKind::Deck);
        assert_eq!(loaded[1].kind, DocumentKind::Profile);
    }
}
