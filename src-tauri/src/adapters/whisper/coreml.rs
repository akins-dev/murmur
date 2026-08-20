/*!
 * SOURCE OF TRUTH KEYWORDS: coreml_encoder_path, coreml, mlmodelc,
 *   COREML_FORCES_FULL_CONTEXT, WhisperEngine::coreml_available
 * WHAT:  Where whisper.cpp looks for a Core ML encoder, and why having one
 *        changes how every chunk is decoded.
 * WHY:   Two facts about whisper.cpp 1.8.3 live here because both are invisible
 *        in the code that depends on them and both cost real time to rediscover.
 *
 *        1. The encoder path is NOT the model path with a suffix. whisper.cpp
 *           strips a trailing `-qX_X` first, so `ggml-large-v3-turbo-q5_0.bin`
 *           resolves to `ggml-large-v3-turbo-encoder.mlmodelc` — the
 *           unquantised name, which is also how Hugging Face publishes it. An
 *           encoder installed under the quantised name is never opened, and
 *           whisper.cpp says so only in a log line before falling back to Metal.
 *
 *        2. A loaded Core ML encoder is INCOMPATIBLE with a reduced
 *           `audio_ctx`. With one live, whisper.cpp stops building its own
 *           convolution graph and hands the ANE `2 * audio_ctx` mel frames; the
 *           .mlmodelc is compiled for a fixed 3000-frame input, so a reduced
 *           context returns an EMPTY transcript. There is no guard and no error
 *           — measured on this machine as a tail decode "completing" in 5ms
 *           having produced nothing.
 *
 *        Measured cost of turning it on, same fixture, same machine: tail
 *        decode 53ms on Metal alone against 469ms with Core ML live, because
 *        the tail can no longer reduce its context. Core ML is therefore off by
 *        default, and that is a measurement rather than a preference.
 * WHERE: Read by adapters/whisper/engine.rs to decide DecodeProfile; the same
 *        naming rule is mirrored in adapters/http_models/store.rs::delete so a
 *        1.2GB encoder does not outlive the weights it belongs to.
 */

use std::path::{Path, PathBuf};

/**
 * SOURCE OF TRUTH KEYWORDS: coreml_encoder_path
 * WHAT:  The directory whisper.cpp will look in for this model's Core ML
 *        encoder, or None if the path has no usable file name.
 * WHY:   Reimplements `whisper_get_coreml_path_encoder` from whisper.cpp 1.8.3
 *        exactly, including the part that is not guessable: it strips a
 *        trailing quantisation suffix of the form `-qX_X` before appending
 *        `-encoder.mlmodelc`. So `ggml-large-v3-turbo-q5_0.bin` resolves to
 *        `ggml-large-v3-turbo-encoder.mlmodelc` — the UNQUANTISED name, which
 *        is also the name Hugging Face publishes the encoder under.
 *        This was measured, not assumed: an encoder installed under the
 *        quantised name loads nothing and whisper.cpp falls back to Metal with
 *        only a log line to say so.
 * WHERE: Used by coreml_available; the same rule is applied when a model is
 *        deleted so the 1.2GB encoder does not outlive its weights.
 */
pub fn coreml_encoder_path(model_path: &Path) -> Option<PathBuf> {
    let stem = model_path.file_stem().and_then(|s| s.to_str())?;

    // Exactly whisper.cpp's test: a final '-' segment of five characters
    // shaped `-qX_X`.
    let base = match stem.rfind('-') {
        Some(pos) => {
            let suffix = &stem[pos..];
            let bytes = suffix.as_bytes();
            if suffix.len() == 5 && bytes[1] == b'q' && bytes[3] == b'_' {
                &stem[..pos]
            } else {
                stem
            }
        }
        None => stem,
    };

    Some(model_path.with_file_name(format!("{base}-encoder.mlmodelc")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_coreml_encoder_path_matches_whisper_cpp_exactly() {
        // The quantisation suffix is stripped: this is the name Hugging Face
        // publishes and the name whisper.cpp 1.8.3 actually opens.
        assert_eq!(
            coreml_encoder_path(Path::new("/m/ggml-large-v3-turbo-q5_0.bin")),
            Some(PathBuf::from("/m/ggml-large-v3-turbo-encoder.mlmodelc"))
        );
        assert_eq!(
            coreml_encoder_path(Path::new("/m/ggml-small-q5_1.bin")),
            Some(PathBuf::from("/m/ggml-small-encoder.mlmodelc"))
        );
        // An unquantised model keeps its whole name.
        assert_eq!(
            coreml_encoder_path(Path::new("/m/ggml-large-v3-turbo.bin")),
            Some(PathBuf::from("/m/ggml-large-v3-turbo-encoder.mlmodelc"))
        );
        // A trailing segment that only looks like a suffix is left alone.
        assert_eq!(
            coreml_encoder_path(Path::new("/m/ggml-base-english.bin")),
            Some(PathBuf::from("/m/ggml-base-english-encoder.mlmodelc"))
        );
    }
}
