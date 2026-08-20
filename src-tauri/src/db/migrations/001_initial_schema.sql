-- SOURCE OF TRUTH KEYWORDS: sessions, session_metrics, settings, dictionary,
--   app_profiles, sessions_fts, schema_v1
-- WHAT:  The initial schema — sessions and their metrics, settings, the custom
--        dictionary, per-app profiles, and full-text search over transcripts.
-- WHY:   The session row is written BEFORE transcription starts and updated as
--        it progresses, which is what makes crash recovery possible: a row with
--        no ended_at on launch is an orphan whose audio can still be finished.
--        Cancelled sessions never appear here at all — they are deleted in the
--        same transition that discards the audio, so there is no tombstone to
--        trust and no purge job to forget to run.
-- WHERE: Applied by db/migrations.rs when user_version < 1.

CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    started_at      INTEGER NOT NULL,
    ended_at        INTEGER,
    -- delivered | failed | orphaned. Never 'cancelled' — those rows are gone.
    outcome         TEXT    NOT NULL,
    duration_ms     INTEGER,
    language        TEXT,
    engine_id       TEXT    NOT NULL,
    model_id        TEXT    NOT NULL,
    -- Kept beside final_text so a bad result is attributable to the model or
    -- to our own enhancement rules. Without it that distinction is guesswork.
    raw_text        TEXT,
    final_text      TEXT,
    word_count      INTEGER,
    app_bundle_id   TEXT,
    delivery        TEXT    NOT NULL DEFAULT 'none',
    error_code      TEXT
);

CREATE INDEX idx_sessions_started_at ON sessions (started_at DESC);
CREATE INDEX idx_sessions_outcome    ON sessions (outcome);
CREATE INDEX idx_sessions_language   ON sessions (language);

-- One row per stage per session. Aggregated into p50/p95 by services/stats.rs.
CREATE TABLE session_metrics (
    session_id  TEXT    NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    stage       TEXT    NOT NULL,
    duration_ms REAL    NOT NULL,
    recorded_at INTEGER NOT NULL
);

CREATE INDEX idx_metrics_stage ON session_metrics (stage, recorded_at DESC);

-- Values are JSON-encoded SettingValue, validated against the registry's
-- declared control kind before any write is accepted.
CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      TEXT    NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE dictionary (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern     TEXT    NOT NULL,
    replacement TEXT    NOT NULL,
    -- word | word_cs | substring
    match_kind  TEXT    NOT NULL DEFAULT 'word',
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL,
    -- Bumped on use, so the engine prompt can prioritise recent terms when the
    -- dictionary is larger than the prompt budget.
    used_at     INTEGER
);

CREATE UNIQUE INDEX idx_dictionary_pattern ON dictionary (pattern, match_kind);

CREATE TABLE app_profiles (
    bundle_id     TEXT PRIMARY KEY,
    display_name  TEXT NOT NULL,
    settings_json TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1
);

-- External-content FTS: the index stores no copy of the text, it points at
-- sessions. That makes the triggers below mandatory rather than an
-- optimisation — an external-content table that silently drifts out of sync is
-- worse than having no search at all.
CREATE VIRTUAL TABLE sessions_fts USING fts5 (
    final_text,
    content = 'sessions',
    content_rowid = 'rowid'
);

CREATE TRIGGER sessions_fts_insert AFTER INSERT ON sessions BEGIN
    INSERT INTO sessions_fts (rowid, final_text) VALUES (new.rowid, new.final_text);
END;

CREATE TRIGGER sessions_fts_delete AFTER DELETE ON sessions BEGIN
    INSERT INTO sessions_fts (sessions_fts, rowid, final_text)
    VALUES ('delete', old.rowid, old.final_text);
END;

CREATE TRIGGER sessions_fts_update AFTER UPDATE ON sessions BEGIN
    INSERT INTO sessions_fts (sessions_fts, rowid, final_text)
    VALUES ('delete', old.rowid, old.final_text);
    INSERT INTO sessions_fts (rowid, final_text) VALUES (new.rowid, new.final_text);
END;
