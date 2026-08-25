-- Schema v1 -> v2 (issue #17): authored workspace documents.
--
-- Stores validated deck and profile documents as canonical JSON produced by
-- the domain documents' deterministic serializer (`DeckDocument` /
-- `ProfileDocument`, schema_version 1.x). One row per document, keyed by
-- closed-vocabulary kind plus the document's durable UUIDv7 id.
--
-- The JSON column is opaque to SQL on purpose: structural truth lives in the
-- `openstream-domain` crate, which validates every byte on both write
-- (`to_json_string`) and read (`from_json_str`). No user content is stored in
-- any other column; `updated_at_wall_ms` is UTC epoch milliseconds supplied
-- by the clock owner.
--
-- The migration chain is shared by every store this crate opens, so existing
-- execution-journal databases also receive this table (unused there) through
-- the standard verified-backup upgrade path.

CREATE TABLE workspace_documents (
    kind TEXT NOT NULL CHECK (kind IN ('deck', 'profile')),
    id TEXT NOT NULL,
    document_json TEXT NOT NULL,
    updated_at_wall_ms INTEGER NOT NULL CHECK (updated_at_wall_ms >= 0),
    PRIMARY KEY (kind, id)
) WITHOUT ROWID;

UPDATE openstream_schema SET value = 2;
