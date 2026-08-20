-- SOURCE OF TRUTH KEYWORDS: error_message, sessions, failure_reason, schema_v2
-- WHAT:  Adds the plain-language failure reason to the sessions table.
-- WHY:   The row already stored `error_code`, but the SENTENCE a user reads was
--        built in Rust at failure time and thrown away. History therefore had a
--        code and no words, so the frontend grew its own code-to-sentence map —
--        a second, drifting set of wording for failures the app already had
--        words for. Two different sentences for the same failure in the pill
--        and in History is exactly what a single source of truth is supposed to
--        make impossible.
--        Storing the message means the words are written once, where the
--        failure happened, and read back verbatim.
-- WHERE: Written by session/actor.rs on a failed session; read by History.

ALTER TABLE sessions ADD COLUMN error_message TEXT;
