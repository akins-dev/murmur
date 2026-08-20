/*!
 * SOURCE OF TRUTH KEYWORDS: setting_keys, DICTATION_HOTKEY, DICTATION_MODE,
 *   CAPTURE_MODE, CANCEL_COUNTDOWN_MS, TRANSCRIPTION_MODEL, LANGUAGE,
 *   AUTO_PASTE, RESTORE_CLIPBOARD, RETENTION_DAYS, LAUNCH_AT_LOGIN
 * WHAT:  Every setting key in the app, as a constant.
 * WHY:   These strings are database keys. A typo in one does not fail — it
 *        reads as an absent setting and silently falls back to the default,
 *        which looks like the setting "not saving" and is miserable to
 *        diagnose. Constants turn that into a build error, and they make every
 *        reader of a setting greppable.
 * WHERE: Used by registry/mod.rs to declare settings and by every consumer that
 *        reads one. Never write one of these strings inline.
 */

// ── Recording ────────────────────────────────────────────────────────────
pub const DICTATION_HOTKEY: &str = "dictation.hotkey";
pub const DICTATION_MODE: &str = "dictation.mode";
pub const CAPTURE_MODE: &str = "dictation.capture_mode";
pub const CANCEL_COUNTDOWN_MS: &str = "dictation.cancel_countdown_ms";
pub const DISCARD_ON_ESCAPE: &str = "dictation.discard_on_escape";
pub const INPUT_DEVICE: &str = "dictation.input_device";
pub const AUDIO_FEEDBACK: &str = "dictation.audio_feedback";

// ── Transcription ────────────────────────────────────────────────────────
pub const TRANSCRIPTION_MODEL: &str = "transcription.model";
pub const LANGUAGE: &str = "transcription.language";
pub const FINALIZE_TIMEOUT_MS: &str = "transcription.finalize_timeout_ms";

// ── Output ───────────────────────────────────────────────────────────────
pub const AUTO_PASTE: &str = "output.auto_paste";
pub const RESTORE_CLIPBOARD: &str = "output.restore_clipboard";
pub const PASTE_DELAY_MS: &str = "output.paste_delay_ms";
pub const CLIPBOARD_RESTORE_DELAY_MS: &str = "output.clipboard_restore_delay_ms";

// ── Enhancement ──────────────────────────────────────────────────────────
pub const CAPITALISE_SENTENCES: &str = "enhance.capitalise_sentences";
pub const NORMALISE_PUNCTUATION: &str = "enhance.normalise_punctuation";
pub const STRIP_FILLERS: &str = "enhance.strip_fillers";
pub const SPOKEN_COMMANDS: &str = "enhance.spoken_commands";
pub const APPLY_CORRECTIONS: &str = "enhance.apply_corrections";

// ── Privacy ──────────────────────────────────────────────────────────────
pub const RETENTION_DAYS: &str = "privacy.retention_days";
// `privacy.store_audio` was here. Removed rather than wired: nothing in this
// crate ever writes audio to disk, and nothing should. Murmur's promise is that
// your voice never leaves the machine, and the strongest form of that is that
// the audio never becomes a file at all — the transcript is the product.
// Wiring the toggle would have meant BUILDING retention we would then have to
// defend. See registry/reachability.rs for how it was found.

// ── General ──────────────────────────────────────────────────────────────
pub const LAUNCH_AT_LOGIN: &str = "general.launch_at_login";
pub const BASELINE_WPM: &str = "general.baseline_wpm";
pub const CHECK_UPDATES: &str = "general.check_updates";
pub const ONBOARDING_COMPLETE: &str = "general.onboarding_complete";
