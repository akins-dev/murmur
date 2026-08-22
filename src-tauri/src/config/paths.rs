/*!
 * SOURCE OF TRUTH KEYWORDS: AppPaths, data_dir, models_dir, db_path, logs_dir,
 *   audio_dir, ensure_dirs, APP_DIR_NAME
 * WHAT:  Every filesystem location the app uses, resolved once.
 * WHY:   One place decides where things live, so nothing can disagree about
 *        where the database is. Resolved at startup and carried in state rather
 *        than recomputed at each call site, because a path computed twice is a
 *        path that can be computed differently twice — and a model downloaded
 *        to one location and looked for in another is a bug that presents as a
 *        574MB re-download.
 * WHERE: Built once during setup; used by db/connection.rs, telemetry/, and the
 *        model store adapter.
 */

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult, ErrorCode};

/// Matches the bundle identifier so the app's data sits where macOS expects it.
pub const APP_DIR_NAME: &str = "com.webprodigies.murmur";

/**
 * SOURCE OF TRUTH KEYWORDS: AppPaths
 * WHAT:  The resolved set of directories and files, created if absent.
 * WHERE: Constructed by `AppPaths::resolve` during app setup.
 */
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// ~/Library/Application Support/com.murmur.app
    pub data_dir: PathBuf,
    /// Model weights and their Core ML encoders.
    pub models_dir: PathBuf,
    /// Rolling local log files. Never transmitted.
    pub logs_dir: PathBuf,
    /// Retained recordings, only when the user has explicitly opted in.
    pub audio_dir: PathBuf,
    /// The single SQLite file.
    pub db_path: PathBuf,
    /**
     * SOURCE OF TRUTH KEYWORDS: bundled_models_dir, one_download
     * WHAT:  Models shipped INSIDE the app bundle, if any.
     * WHY:   Murmur is distributed as a single download that works the moment
     *        it opens — no second wait, no first-run fetch, nothing to go wrong
     *        on a bad connection. The weights ride along in Contents/Resources
     *        and are used from there, read-only, rather than being copied into
     *        Application Support: a copy would mean 547MB written on first
     *        launch and 1.1GB on disk for one model.
     *
     *        `None` when nothing was bundled, which is the developer build and
     *        the path where a model is still downloaded on demand. Both work;
     *        only the first is what a user receives.
     * WHERE: Set by bootstrap from Tauri's resource directory; consulted by
     *        model_file.
     */
    pub bundled_models_dir: Option<PathBuf>,
}

impl AppPaths {
    /**
     * WHAT:  Resolves every path and creates the directories.
     * WHY:   Creating them here means no later code has to handle "directory
     *        missing" — a failure that otherwise appears halfway through a
     *        download, after the user has waited.
     * WHERE: Called once from lib.rs setup.
     */
    pub fn resolve() -> AppResult<Self> {
        let base = dirs::data_dir().ok_or_else(|| {
            AppError::new(
                ErrorCode::Io,
                "Murmur could not find your Application Support folder.",
            )
        })?;

        let data_dir = base.join(APP_DIR_NAME);
        let paths = Self {
            models_dir: data_dir.join("models"),
            logs_dir: data_dir.join("logs"),
            audio_dir: data_dir.join("audio"),
            db_path: data_dir.join("murmur.db"),
            data_dir,
            bundled_models_dir: None,
        };

        paths.ensure_dirs()?;
        Ok(paths)
    }

    fn ensure_dirs(&self) -> AppResult<()> {
        for dir in [
            &self.data_dir,
            &self.models_dir,
            &self.logs_dir,
            &self.audio_dir,
        ] {
            std::fs::create_dir_all(dir).map_err(|err| {
                AppError::from(err).with_detail(format!("creating {}", dir.display()))
            })?;
        }
        Ok(())
    }

    /**
     * WHAT:  Where a model file with this id belongs.
     * WHY:   A model shipped inside the app wins, and everything downstream
     *        gets it for free — the store sees the file already present so it
     *        never downloads, the engine loads it from there, and the
     *        onboarding step that waits for a download completes immediately.
     *        Putting the choice HERE rather than in the store is what makes
     *        that true: every caller already asks this one function where a
     *        model is.
     *
     *        Falls through to Application Support when nothing is bundled, or
     *        when the user has downloaded a model the bundle does not carry.
     */
    pub fn model_file(&self, model_id: &str) -> PathBuf {
        if let Some(bundled) = &self.bundled_models_dir {
            let shipped = bundled.join(format!("ggml-{model_id}.bin"));
            if shipped.is_file() {
                return shipped;
            }
        }
        self.models_dir.join(format!("ggml-{model_id}.bin"))
    }

    /**
     * WHAT:  Points the paths at models shipped inside the app bundle.
     * WHY:   Takes a plain PathBuf rather than anything Tauri-shaped, so this
     *        layer stays free of the framework above it. bootstrap knows where
     *        the resources are; this file only needs to know that they exist.
     */
    pub fn with_bundled_models(mut self, dir: PathBuf) -> Self {
        if dir.is_dir() {
            self.bundled_models_dir = Some(dir);
        }
        self
    }

    /**
     * WHAT:  The temp path a download writes to before it is renamed into place.
     * WHY:   Downloads land here and are only renamed after the hash verifies,
     *        so a half-written file can never be mistaken for a usable model.
     *        The rename is atomic within the same directory, which is why this
     *        sits beside the target rather than in the system temp dir.
     * WHERE: Used by the http_models adapter.
     */
    pub fn model_download_file(&self, model_id: &str) -> PathBuf {
        self.models_dir.join(format!("ggml-{model_id}.bin.part"))
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: model_coreml_dir, coreml_encoder_name,
     *   mlmodelc, quantisation_suffix
     * WHAT:  The Core ML encoder directory whisper.cpp looks for beside the
     *        weights.
     * WHY:   whisper.cpp does NOT use the model id verbatim here — it strips a
     *        trailing `-qX_X` quantisation suffix first. So the encoder for
     *        `large-v3-turbo-q5_0` is looked for at
     *        `ggml-large-v3-turbo-encoder.mlmodelc`, the UNQUANTISED name.
     *
     *        This was measured, not assumed. An earlier version of this
     *        function used the id verbatim, which meant an encoder installed at
     *        the name we generated was never opened: whisper.cpp silently fell
     *        back to Metal with only a log line, so a 1.2GB download did
     *        nothing and nothing reported a failure. Every quantised model in
     *        the catalog was affected.
     * WHERE: The single implementation of this rule. adapters/whisper/coreml.rs
     *        resolves paths through here rather than repeating it — two copies
     *        of a rule this silent is how the bug came back.
     */
    pub fn model_coreml_dir(&self, model_id: &str) -> PathBuf {
        self.models_dir
            .join(format!("ggml-{}-encoder.mlmodelc", strip_quantisation(model_id)))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: strip_quantisation, coreml_encoder_name, qX_X
 * WHAT:  Removes a trailing `-qX_X` quantisation suffix from a model id or
 *        file stem, mirroring whisper.cpp's own rule exactly.
 * WHY:   Reproduces the C++ test character for character — a final '-' segment
 *        of five characters shaped `-qX_X`. Anything looser would strip a real
 *        part of a model name; anything stricter would miss a variant and
 *        silently disable Core ML again.
 * WHERE: Used by AppPaths::model_coreml_dir and by the whisper adapter's
 *        encoder lookup and delete paths.
 */
pub fn strip_quantisation(name: &str) -> &str {
    match name.rfind('-') {
        Some(pos) => {
            let suffix = &name[pos..];
            let bytes = suffix.as_bytes();
            if suffix.len() == 5 && bytes[1] == b'q' && bytes[3] == b'_' {
                &name[..pos]
            } else {
                name
            }
        }
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quantisation_suffix_is_stripped_exactly_as_whisper_cpp_does() {
        // The bug this guards: an encoder installed under the quantised name is
        // never opened, and nothing reports the failure.
        assert_eq!(strip_quantisation("large-v3-turbo-q5_0"), "large-v3-turbo");
        assert_eq!(strip_quantisation("small-q5_1"), "small");
        // Unquantised names are untouched.
        assert_eq!(strip_quantisation("large-v3-turbo"), "large-v3-turbo");
        assert_eq!(strip_quantisation("small"), "small");
        // A trailing segment that merely looks similar must survive.
        assert_eq!(strip_quantisation("base-english"), "base-english");
        assert_eq!(strip_quantisation("model-q5"), "model-q5");
    }

    #[test]
    fn the_coreml_directory_uses_the_unquantised_name() {
        let paths = AppPaths {
            data_dir: PathBuf::from("/tmp/murmur"),
            models_dir: PathBuf::from("/tmp/murmur/models"),
            bundled_models_dir: None,
            logs_dir: PathBuf::from("/tmp/murmur/logs"),
            audio_dir: PathBuf::from("/tmp/murmur/audio"),
            db_path: PathBuf::from("/tmp/murmur/murmur.db"),
        };

        assert_eq!(
            paths.model_coreml_dir("large-v3-turbo-q5_0"),
            PathBuf::from("/tmp/murmur/models/ggml-large-v3-turbo-encoder.mlmodelc")
        );
        // The weights themselves DO keep the quantised name.
        assert_eq!(
            paths.model_file("large-v3-turbo-q5_0"),
            PathBuf::from("/tmp/murmur/models/ggml-large-v3-turbo-q5_0.bin")
        );
    }
}
