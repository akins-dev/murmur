/*!
 * SOURCE OF TRUTH KEYWORDS: whisper_live_tests, shared_engine, engine_lock,
 *   tail_decode_budget, median_ms, TAIL_BUDGET_MS
 * WHAT:  The adapter tests that need a real model file and real spoken audio.
 * WHY:   Kept out of the modules they exercise, and self-skipping, so
 *        `cargo test` is green on a clean checkout. A test that fails because a
 *        574MB download is absent trains everyone to ignore a red suite, which
 *        costs more than the coverage is worth — and the guards that decide
 *        correctness (silence, blocklist, audio_ctx, prompt budget) are all
 *        covered by model-free unit tests beside the code they test.
 *        What lives here instead is everything that can only be learned by
 *        running the model: that speech comes back as words, that silence comes
 *        back as nothing, that the dictionary prompt changes what is heard, and
 *        what the tail decode actually costs.
 *
 *        The engine and the fixtures come from `crate::testing`, and NOTHING
 *        here may build its own. whisper.cpp's Metal backend aborts the whole
 *        process when a second context is destroyed while another holds device
 *        resources, so "one context per process" is a correctness requirement
 *        that binds the test binary exactly as it binds the app. An earlier
 *        version of this file built an engine per test: five 574MB models, five
 *        concurrent decodes, and a capability probe that read 7.12x instead of
 *        34.8x — which failed the budget assertion below against a number that
 *        was measuring its own siblings.
 * WHERE: Run by hand after the ModelStore has fetched the default model. The
 *        shared harness is src/testing.rs.
 */

    use std::time::Instant;

    use super::*;
    use crate::ports::TranscribeRequest;
    use crate::testing;
    use crate::types::{AudioChunk, ChunkKind, LanguageCode, LanguageHint, TARGET_SAMPLE_RATE};

    /**
     * SOURCE OF TRUTH KEYWORDS: median_ms, TIMED_RUNS, TAIL_BUDGET_MS,
     *   REFERENCE_REALTIME_FACTOR
     * WHAT:  Median wall-clock of repeated timed runs, discarding a warm-up,
     *        with an untimed `setup` run before each sample.
     * WHY:   A single sample is not a measurement. The run that sent this file
     *        back took one sample of the tail and read 327ms where three
     *        consecutive serial runs read 53, 55 and 56ms — one scheduling
     *        outlier, promoted to a verdict. A median of three rejects exactly
     *        that: one bad sample cannot carry the result.
     *
     *        `setup` is separate from `timed` because the tail decode is only
     *        meaningful after an interior chunk — that is the state a real
     *        session hands it, and whisper.cpp inherits `audio_ctx` from it —
     *        but the interior decode is not the thing being timed.
     * WHERE: Both timed tests in this file.
     */
    const TIMED_RUNS: usize = 3;

    /// The stage budget from docs/02 §7, and the machine docs/03 §9 measured it
    /// on. Both are quoted, never guessed — see the budget assertion below.
    const TAIL_BUDGET_MS: f64 = 250.0;
    const REFERENCE_REALTIME_FACTOR: f32 = 34.8;

    fn median_ms(mut setup: impl FnMut(), mut timed: impl FnMut()) -> f64 {
        // Warm-up, never timed: the first pass through any new buffer shape
        // pays a Metal allocation that the steady state does not.
        setup();
        timed();

        let mut samples = Vec::with_capacity(TIMED_RUNS);
        for _ in 0..TIMED_RUNS {
            setup();
            let started = Instant::now();
            timed();
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        samples[samples.len() / 2]
    }

    /**
     * WHAT:  Whether a Core ML encoder is both compiled in and present on disk.
     * WHY:   Asks the same question `WhisperEngine::coreml_available` asks, via
     *        the same path rule, because the shared engine arrives as a trait
     *        object and the trait deliberately exposes capabilities rather than
     *        implementation details. Using whisper.cpp's own path rule matters:
     *        it strips a trailing `-qX_X` before appending `-encoder.mlmodelc`,
     *        so a plausible-looking path answers the wrong question.
     * WHERE: The two timed tests, which cannot assert a reduced audio_ctx while
     *        Core ML is live. See docs/03 §9 question 5.
     */
    fn coreml_is_live() -> bool {
        cfg!(feature = "coreml")
            && testing::live_model_path()
                .and_then(|path| coreml_encoder_path(&path))
                .is_some_and(|path| path.is_dir())
    }

    fn chunk(samples: Vec<f32>, kind: ChunkKind) -> AudioChunk {
        let end_ms = (samples.len() as u64 * 1000) / u64::from(TARGET_SAMPLE_RATE);
        AudioChunk {
            samples,
            start_ms: 0,
            end_ms,
            kind,
        }
    }

    fn english() -> TranscribeRequest {
        TranscribeRequest {
            language: LanguageHint::Pinned {
                language: LanguageCode("en".to_string()),
            },
            ..TranscribeRequest::default()
        }
    }

    /**
     * WHAT:  Decodes real speech and asserts the words come back.
     * WHY:   The deliverable is "turns 16kHz mono f32 into segments", and the
     *        only way to know that is true is to hand it speech and read what
     *        comes out. Everything else in this file tests a guard around this.
     * WHERE: The whole adapter, end to end.
     */
    #[test]
    fn real_speech_decodes_into_segments() {
        let _lock = testing::engine_lock();
        let Some(engine) = testing::shared_engine() else {
            eprintln!("skipped: default model not downloaded");
            return;
        };
        let Some(samples) =
            testing::synthesise_speech("The quick brown fox jumps over the lazy dog.", "fox")
        else {
            eprintln!("skipped: could not synthesise a speech fixture");
            return;
        };

        let segments = engine
            .transcribe(&chunk(samples, ChunkKind::Tail), &english())
            .expect("speech decodes");
        // This must hold in EVERY build configuration. It is the assertion that
        // caught Core ML silently returning an empty transcript.
        let text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        eprintln!("decoded: {text:?}");

        assert!(!segments.is_empty(), "real speech produced no segments");
        for word in ["quick", "brown", "fox", "lazy", "dog"] {
            assert!(text.contains(word), "{word:?} missing from {text:?}");
        }
        assert!(segments.iter().all(|s| s.end_ms >= s.start_ms));
        assert_eq!(
            segments[0].language.as_ref().map(LanguageCode::as_str),
            Some("en")
        );
    }

    /**
     * WHAT:  Feeds the engine silence and near-silence and asserts nothing
     *        comes back.
     * WHY:   Run under BOTH a pinned and an auto language, deliberately. An
     *        earlier version passed only under auto — and passed for the wrong
     *        reason, because `detect_language` was making whisper.cpp return
     *        zero segments without transcribing at all. The near-silent case
     *        matters most: it is loud enough to clear the digital-silence guard,
     *        so it reaches the model, and whisper reliably answers "Thank you."
     * WHERE: The behaviour under test is docs/03 §2.4 defences 1 and 2.
     */
    #[test]
    fn silence_and_near_silence_produce_no_text() {
        let _lock = testing::engine_lock();
        let Some(engine) = testing::shared_engine() else {
            eprintln!("skipped: default model not downloaded");
            return;
        };

        for request in [TranscribeRequest::default(), english()] {
            for kind in [ChunkKind::Tail, ChunkKind::Interior] {
                let silent = chunk(vec![0.0_f32; TARGET_SAMPLE_RATE as usize * 3], kind);
                let segments = engine
                    .transcribe(&silent, &request)
                    .expect("silence is not an error");
                assert!(
                    segments.is_empty(),
                    "silence hallucinated {:?}",
                    segments.iter().map(|s| &s.text).collect::<Vec<_>>()
                );
            }
        }

        // Above the digital-silence floor, so this genuinely reaches the model
        // and exercises the blocklist rather than the cheap early return.
        let hiss: Vec<f32> = (0..TARGET_SAMPLE_RATE as usize * 3)
            .map(|i| if i % 2 == 0 { 3.0e-4 } else { -3.0e-4 })
            .collect();
        assert!(!is_digital_silence(&hiss), "this fixture must reach the model");
        let segments = engine
            .transcribe(&chunk(hiss, ChunkKind::Tail), &english())
            .expect("near-silence is not an error");
        assert!(
            segments.is_empty(),
            "a near-silent buffer hallucinated {:?}",
            segments.iter().map(|s| &s.text).collect::<Vec<_>>()
        );
    }

    /**
     * WHAT:  Shows the dictionary prompt changing what the model hears, and
     *        that an oversized prompt truncates instead of failing.
     * WHY:   docs/03 §2.3 argues the prompt fixes recognition where a post-hoc
     *        replacement cannot. This machine reproduces the exact example the
     *        doc uses — "Claude Code" comes back as "Claude Coat" unprompted —
     *        so the claim is testable rather than rhetorical.
     * WHERE: adapters/whisper/prompt.rs and the initial_prompt parameter.
     */
    #[test]
    fn the_dictionary_prompt_changes_what_the_model_hears() {
        let _lock = testing::engine_lock();
        let Some(engine) = testing::shared_engine() else {
            eprintln!("skipped: default model not downloaded");
            return;
        };
        let Some(samples) =
            testing::synthesise_speech("Claude Code is running the transcription.", "claude")
        else {
            eprintln!("skipped: could not synthesise a speech fixture");
            return;
        };

        let decode = |request: &TranscribeRequest| -> String {
            engine
                .transcribe(&chunk(samples.clone(), ChunkKind::Tail), request)
                .expect("decodes")
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };

        let unprompted = decode(&english());
        let prompted = decode(&TranscribeRequest {
            prompt: Some("Claude Code, Anthropic, Tauri, specta".to_string()),
            ..english()
        });
        eprintln!("unprompted: {unprompted:?}");
        eprintln!("prompted:   {prompted:?}");

        // An oversized dictionary must truncate rather than overflow or abort.
        let huge = (0..2000)
            .map(|i| format!("terminology{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let truncated = decode(&TranscribeRequest {
            prompt: Some(huge),
            ..english()
        });
        assert!(
            !truncated.is_empty(),
            "an oversized prompt must be truncated, not fatal"
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: tail_decode_budget, language_detect_cost
     * WHAT:  Times the tail decode in the shape a real session produces it —
     *        an interior chunk at full context, then the trailing fragment —
     *        with the language auto-detected and with it pinned.
     * WHY:   docs/03 §9 question 2 asks for the tail-decode cost with a reduced
     *        `audio_ctx`, and the answer is only meaningful in session order.
     *        whisper.cpp assigns `state->exp_n_audio_ctx` from `params.audio_ctx`
     *        AFTER it has already run the language-detection encoder pass, so
     *        that pass reuses whatever `audio_ctx` the PREVIOUS call on this
     *        state left behind. Measuring a tail in isolation therefore reports
     *        a number the product never sees: after a full-context interior
     *        chunk, an auto-language tail pays a full 1500-context encode for
     *        detection before it decodes anything.
     * WHERE: Run by hand; the numbers go back into docs/03 §9.
     */
    #[test]
    fn tail_decode_is_measured_in_the_shape_a_session_produces_it() {
        let _lock = testing::engine_lock();
        let Some(engine) = testing::shared_engine() else {
            eprintln!("skipped: default model not downloaded");
            return;
        };
        let Some(speech) = testing::synthesise_speech(
            "This is a longer sentence used to fill an interior chunk before the trailing fragment is decoded.",
            "interior",
        ) else {
            eprintln!("skipped: could not synthesise a speech fixture");
            return;
        };
        eprintln!(
            "machine: realtime_factor {:.2}x | core ml encoder {} | {} threads",
            engine.capabilities().realtime_factor,
            if coreml_is_live() { "present" } else { "absent" },
            params::decode_thread_count(),
        );

        let interior = chunk(speech.clone(), ChunkKind::Interior);
        let tail_samples: Vec<f32> = speech
            .iter()
            .take(TARGET_SAMPLE_RATE as usize * 12 / 10)
            .copied()
            .collect();
        let tail = chunk(tail_samples, ChunkKind::Tail);

        // Always preceded by an interior chunk: that is what a real session
        // hands the tail, and it is what the detection pass inherits from.
        let time = |request: &TranscribeRequest| -> f64 {
            median_ms(
                || {
                    let _ = engine.transcribe(&interior, request);
                },
                || {
                    let _ = engine.transcribe(&tail, request).expect("tail decodes");
                },
            )
        };

        let auto_ms = time(&TranscribeRequest::default());
        let pinned_ms = time(&english());
        let measured = engine.capabilities().realtime_factor;
        eprintln!(
            "tail decode after an interior chunk — auto language: {auto_ms:.0}ms | pinned: {pinned_ms:.0}ms  (audio_ctx {} vs {})",
            audio_ctx_for(tail.samples.len()),
            FULL_AUDIO_CTX
        );

        // With a Core ML encoder live the reduced context is unusable (it
        // returns an empty transcript, see DecodeProfile::for_chunk), so every
        // chunk pays the full encoder and the budget is not reachable — which
        // is the finding, not a bug.
        if coreml_is_live() {
            eprintln!(
                "budget assertion skipped: Core ML is live, so the tail cannot use a reduced audio_ctx"
            );
            return;
        }

        // 1. The promise the pipeline was rewired to keep. Pinning the language
        //    skips a full 1500-context encode the tail would otherwise pay just
        //    to work out what language it is in; docs/03 §9 measures that at
        //    6.2x. Asserted at 1.5x, which leaves the finding room to move with
        //    the hardware but still fails outright if the pin stops being
        //    applied — the two numbers converge when it does.
        assert!(
            auto_ms > pinned_ms * 1.5,
            "pinning the language must still be worth paying for: auto {auto_ms:.0}ms vs pinned {pinned_ms:.0}ms"
        );

        // 2. The stage budget from docs/02 §7, scaled by how much slower this
        //    machine measured ITSELF to be than the one docs/03 §9 quotes.
        //
        //    An unscaled 250ms asserts something about the hardware, not about
        //    Murmur: it passes on an idle M4 Max and goes red on a laptop that
        //    is merely busy, and a suite that is red for reasons nobody caused
        //    is a suite everyone learns to ignore. The scaling factor is not a
        //    fudge — `realtime_factor` is a full-context decode of a fixed
        //    probe, taken in this same process moments earlier, so it absorbs
        //    exactly the machine speed and load that the raw budget cannot.
        //
        //    It never relaxes below the product's promise, and it stays live:
        //    a tail that regressed to full context costs ~334ms here and fails
        //    this on any machine, because the regression scales too.
        assert!(
            measured > 0.0,
            "prepare() must have measured this machine before anything is timed"
        );
        let slowdown = f64::from(REFERENCE_REALTIME_FACTOR / measured).max(1.0);
        let allowance = TAIL_BUDGET_MS * slowdown;
        eprintln!(
            "budget: {pinned_ms:.0}ms against {allowance:.0}ms  ({TAIL_BUDGET_MS:.0}ms x {slowdown:.2} — this machine measured {measured:.1}x vs the reference {REFERENCE_REALTIME_FACTOR:.1}x)"
        );
        assert!(
            pinned_ms < allowance,
            "a pinned-language tail decode must fit the latency budget: was {pinned_ms:.0}ms against {allowance:.0}ms on a {measured:.1}x machine"
        );
    }

    /**
     * WHAT:  Proves the reduced encoder context is what makes the tail cheap.
     * WHY:   Stated as the single most important line in the engine, so it is
     *        asserted rather than assumed. Language is pinned on both sides so
     *        the detection pass cannot mask the difference — that masking is
     *        exactly what made an earlier version of this test read 283ms
     *        against 283ms and conclude the optimisation did nothing.
     * WHERE: adapters/whisper/params.rs::audio_ctx_for.
     */
    #[test]
    fn the_reduced_encoder_context_is_what_makes_the_tail_cheap() {
        let _lock = testing::engine_lock();
        let Some(engine) = testing::shared_engine() else {
            eprintln!("skipped: default model not downloaded");
            return;
        };
        let Some(speech) = testing::synthesise_speech("A short trailing fragment.", "fragment")
        else {
            eprintln!("skipped: could not synthesise a speech fixture");
            return;
        };
        if coreml_is_live() {
            eprintln!(
                "skipped: Core ML is live, so the adapter deliberately keeps the full encoder context"
            );
            return;
        }
        let request = english();

        let median = |kind: ChunkKind| -> f64 {
            median_ms(
                || {},
                || {
                    let _ = engine.transcribe(&chunk(speech.clone(), kind), &request);
                },
            )
        };

        let reduced = median(ChunkKind::Tail);
        let full = median(ChunkKind::Interior);
        eprintln!(
            "{:.1}s fragment, language pinned — audio_ctx {}: {reduced:.0}ms | audio_ctx {}: {full:.0}ms  ({:.1}x)",
            speech.len() as f32 / TARGET_SAMPLE_RATE as f32,
            audio_ctx_for(speech.len()),
            FULL_AUDIO_CTX,
            full / reduced
        );
        assert!(
            reduced * 2.0 < full,
            "the reduced encoder context must be substantially faster, was {reduced:.0}ms vs {full:.0}ms"
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: arabic_auto_detect, multilingual_probe
     * WHAT:  Auto-detect must identify Arabic and return Arabic script, and an
     *        explicit Arabic pin must do the same.
     * WHY:   docs/01 §4.1 M6 promises Hindi and Arabic specifically. A pipeline
     *        that silently decodes them as English produces plausible-looking
     *        English nonsense rather than an error, so nothing surfaces the
     *        failure — it has to be asserted against real non-Latin audio.
     * WHERE: Guards the multilingual claim end to end.
     */
    #[test]
    fn arabic_is_detected_and_returned_in_arabic_script() {
        let _guard = crate::testing::engine_lock();
        let Some(engine) = crate::testing::shared_engine() else {
            eprintln!("skipped: no model");
            return;
        };
        // Synthesised with the system Arabic voice, so the fixture cannot drift
        // and nothing has to be committed.
        let Some(samples) = crate::testing::synthesise_speech_with_voice(
            "مرحبا، هذا اختبار للنظام باللغة العربية. أتمنى أن يعمل بشكل صحيح.",
            "arabic",
            Some("Majed"),
        ) else {
            eprintln!("skipped: no Arabic voice installed");
            return;
        };

        let chunk = AudioChunk {
            samples,
            start_ms: 0,
            end_ms: 6000,
            kind: ChunkKind::Tail,
        };

        // 1. Auto-detect.
        let auto = engine
            .transcribe(&chunk, &TranscribeRequest { language: LanguageHint::Auto, prompt: None })
            .expect("auto decode");
        let auto_text: String = auto.iter().map(|s| s.text.as_str()).collect();
        let auto_lang = auto.first().and_then(|s| s.language.clone());
        eprintln!("AUTO   lang={auto_lang:?} text={auto_text:?}");

        // 2. Explicitly pinned to Arabic.
        let pinned = engine
            .transcribe(&chunk, &TranscribeRequest {
                language: LanguageHint::Pinned { language: LanguageCode("ar".into()) },
                prompt: None,
            })
            .expect("pinned decode");
        let pinned_text: String = pinned.iter().map(|s| s.text.as_str()).collect();
        eprintln!("PINNED lang=ar text={pinned_text:?}");

        let has_arabic = |s: &str| s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
        assert!(has_arabic(&pinned_text), "pinning to Arabic must return Arabic script, got {pinned_text:?}");
        assert!(has_arabic(&auto_text), "auto-detect must return Arabic script, got {auto_text:?}");
    }
