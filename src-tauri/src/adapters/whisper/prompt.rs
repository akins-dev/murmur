/*!
 * SOURCE OF TRUTH KEYWORDS: fit_prompt, sanitise_prompt, PROMPT_TOKEN_BUDGET,
 *   PROMPT_TERM_SEPARATORS, TokenCounter
 * WHAT:  Fits the user's dictionary prompt into whisper's initial-prompt budget
 *        and strips the one byte that would abort the process.
 * WHY:   Whisper accepts roughly 224 tokens of prompt and silently discards the
 *        overflow, so a large dictionary loses whichever terms happen to fall
 *        off the end — a dictionary that stops working as it grows, with no
 *        error anywhere. Truncating here makes the loss deliberate and ordered.
 *        The null-byte strip is not cosmetic: whisper-rs builds a CString and
 *        PANICS on an interior null, and this string originates in user-typed
 *        dictionary entries, so an unsanitised prompt is a user-triggerable
 *        crash.
 * WHERE: Called by adapters/whisper/engine.rs with a token counter backed by
 *        WhisperContext::tokenize; the prompt itself is assembled by
 *        pipeline/worker.rs from the dictionary service.
 */

/// Whisper's initial-prompt capacity, in tokens. Overflow is dropped silently
/// by whisper.cpp, which is why we truncate before handing it over.
pub const PROMPT_TOKEN_BUDGET: usize = 224;

/// Characters a prompt builder may use to separate terms. Splitting on these
/// is what lets truncation land between terms rather than mid-word, where it
/// would bias recognition toward a fragment that is not a real word.
pub const PROMPT_TERM_SEPARATORS: [char; 3] = [',', ';', '\n'];

/**
 * SOURCE OF TRUTH KEYWORDS: TOKENIZE_CAPACITY
 * WHAT:  The largest string, in bytes, that may be handed to
 *        WhisperContext::tokenize — and the buffer size to ask it for.
 * WHY:   **This is a memory-safety bound, not a tuning knob.** whisper.cpp's
 *        `whisper_tokenize` returns `-n_tokens` when the text needs more room
 *        than the caller offered, but whisper-rs 0.16 only tests for `== -1`
 *        and otherwise does `tokens.set_len(ret as usize)`. Feed it a prompt
 *        that overflows and `-1883` becomes a length of 18 quintillion: an
 *        immediate non-unwinding abort that no `catch_unwind` and no `?` can
 *        contain. A user with a large dictionary would have crashed the app on
 *        every keypress.
 *        Two rules keep us out of it, and both are needed: ask for a buffer
 *        this large, and never pass text longer than it in bytes. Whisper's BPE
 *        never emits more tokens than the byte length of its input, so a string
 *        of at most this many bytes cannot overflow a buffer of this many
 *        tokens, and the return value is always a true count.
 *        8192 is well above any string that could tokenise to 224 tokens (the
 *        longest entries in the vocabulary are a couple of dozen bytes), so
 *        rejecting anything larger outright loses nothing real.
 * WHERE: Enforced by adapters/whisper/engine.rs::fit_prompt_for, the only
 *        place in the app that calls tokenize.
 */
pub const TOKENIZE_CAPACITY: usize = 8192;

/**
 * SOURCE OF TRUTH KEYWORDS: sanitise_prompt
 * WHAT:  Removes interior null bytes and collapses surrounding whitespace.
 * WHY:   See the module WHY — whisper-rs panics on a null byte, and this string
 *        comes from user input.
 * WHERE: Applied by fit_prompt before anything else touches the text.
 */
pub fn sanitise_prompt(prompt: &str) -> String {
    prompt.replace('\0', " ").trim().to_string()
}

/**
 * SOURCE OF TRUTH KEYWORDS: fit_prompt, TokenCounter
 * WHAT:  Returns the longest prefix of the prompt's terms that fits the token
 *        budget, or None when nothing usable is left.
 * WHY:   **The leading terms win.** The caller must therefore order the prompt
 *        most-important-first — pipeline/worker.rs sorts the dictionary by most
 *        recently used, so the terms the user actually says survive and the
 *        long tail is what gets dropped. Truncation walks whole terms, never
 *        characters: half a term in the prompt biases the decoder toward a
 *        non-word, which is worse than omitting it.
 *        `count` is injected rather than called directly so the budget logic is
 *        testable without a 574MB model on disk.
 * WHERE: adapters/whisper/engine.rs, once per transcribe call.
 */
pub fn fit_prompt<F>(prompt: &str, budget: usize, count: F) -> Option<String>
where
    F: Fn(&str) -> Option<usize>,
{
    let cleaned = sanitise_prompt(prompt);
    if cleaned.is_empty() || budget == 0 {
        return None;
    }

    if count(&cleaned).is_some_and(|tokens| tokens <= budget) {
        return Some(cleaned);
    }

    let terms: Vec<&str> = cleaned
        .split(PROMPT_TERM_SEPARATORS)
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .collect();

    // A single oversized term has no separator to cut at. Dropping it whole is
    // correct: a partial term is an active bias toward a word that is not real.
    if terms.len() <= 1 {
        return None;
    }

    // Binary search for the longest fitting prefix. Tokenising is not free and
    // a large dictionary would otherwise be tokenised once per term dropped.
    let mut low = 0usize;
    let mut high = terms.len();
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        let candidate = terms[..mid].join(", ");
        if count(&candidate).is_some_and(|tokens| tokens <= budget) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    if low == 0 {
        return None;
    }
    Some(terms[..low].join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for whisper's tokenizer: one token per whitespace-separated
    /// word, which is close enough to exercise every branch.
    fn word_counter(text: &str) -> Option<usize> {
        Some(text.split_whitespace().count())
    }

    #[test]
    fn a_prompt_inside_the_budget_is_returned_untouched() {
        let fitted = fit_prompt("Claude Code, Tauri, specta", 32, word_counter);
        assert_eq!(fitted.as_deref(), Some("Claude Code, Tauri, specta"));
    }

    #[test]
    fn overflow_drops_the_trailing_terms_and_keeps_the_leading_ones() {
        let prompt = "alpha, bravo, charlie, delta, echo";
        let fitted = fit_prompt(prompt, 2, word_counter).expect("two terms fit");
        assert_eq!(fitted, "alpha, bravo");
    }

    #[test]
    fn a_single_term_that_cannot_fit_is_dropped_rather_than_cut_in_half() {
        assert_eq!(fit_prompt("one two three four", 2, word_counter), None);
    }

    #[test]
    fn null_bytes_never_reach_the_ffi_boundary() {
        let fitted = fit_prompt("Claude\0Code", 32, word_counter).expect("still usable");
        assert!(!fitted.contains('\0'));
    }

    #[test]
    fn an_empty_or_whitespace_prompt_is_no_prompt() {
        assert_eq!(fit_prompt("", 32, word_counter), None);
        assert_eq!(fit_prompt("   \n  ", 32, word_counter), None);
    }

    #[test]
    fn a_zero_budget_yields_nothing() {
        assert_eq!(fit_prompt("alpha, bravo", 0, word_counter), None);
    }

    #[test]
    fn a_tokenizer_that_fails_is_treated_as_a_prompt_that_does_not_fit() {
        assert_eq!(fit_prompt("alpha, bravo", 99, |_| None), None);
    }
}
