-- OpenStream SQLite schema v1 (issue #15) — initial release schema.
--
-- Forward-only migration step v0 -> v1, applied inside one transaction by
-- `openstream_persistence::sqlite`. Stores non-secret engine evidence only:
-- typed identifiers, wall/monotonic timestamps, and closed-vocabulary state
-- tokens. Secret values never appear in any column (SECURITY.md hard rules;
-- secret values live only in OS credential storage behind the vault).
--
-- Layout notes:
--   * `seq INTEGER PRIMARY KEY` preserves insertion order for the stable
--     snapshot/unresolved ordering contract of `ExecutionJournal`.
--   * The dedupe key `(source_device_id, message_id)` and the Engine-assigned
--     `execution_id` are durably unique; a duplicate insert fails closed.
--   * `lifecycle` is closed-vocabulary (mirrors `ExecutionState` tokens) and
--     `failure_token` exists exactly when the lifecycle is `failed`.

CREATE TABLE openstream_schema (
    key TEXT PRIMARY KEY CHECK (key = 'schema_version'),
    value INTEGER NOT NULL
);

INSERT INTO openstream_schema (key, value) VALUES ('schema_version', 1);

CREATE TABLE journal_admissions (
    seq INTEGER PRIMARY KEY,
    source_device_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    execution_id TEXT NOT NULL,
    accepted_at_wall_ms INTEGER NOT NULL,
    expires_at_wall_ms INTEGER NOT NULL,
    lifecycle TEXT NOT NULL CHECK (
        lifecycle IN (
            'accepted',
            'running',
            'succeeded',
            'failed',
            'cancelled',
            'expired',
            'outcome_unknown'
        )
    ),
    failure_token TEXT CHECK ((lifecycle = 'failed') = (failure_token IS NOT NULL))
);

CREATE UNIQUE INDEX journal_admissions_dedupe
    ON journal_admissions (source_device_id, message_id);

CREATE UNIQUE INDEX journal_admissions_execution
    ON journal_admissions (execution_id);

CREATE TABLE journal_prepared (
    seq INTEGER PRIMARY KEY,
    execution_id TEXT NOT NULL,
    node_key TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    action_type TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    prepared_at_monotonic_ms INTEGER NOT NULL
);

CREATE INDEX journal_prepared_by_execution ON journal_prepared (execution_id);
