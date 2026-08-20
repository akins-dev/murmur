/*!
 * SOURCE OF TRUTH KEYWORDS: normalise_whitespace, expand_spoken_commands,
 *   strip_fillers, apply_dictionary, dedupe_stutters, normalise_punctuation,
 *   capitalise_sentences, ensure_terminal_punctuation, FILLERS, SPOKEN_COMMANDS
 * WHAT:  The individual enhancement rules. Each is a pure function from string
 *        to string.
 * WHY:   Pure because that makes them table-testable, and the table tests are
 *        the cheapest accuracy guard in the project — perceived quality lives
 *        almost entirely in this file, and a regression here is invisible in
 *        every other kind of test.
 *
 *        Every language-sensitive rule takes the language and does NOTHING for
 *        a language it does not know. That is load-bearing: running the English
 *        filler list over Hindi or Arabic would delete real words, and a
 *        transcription tool that silently eats content is worse than one that
 *        leaves an "um" in.
 * WHERE: Composed in order by adapters/rules/mod.rs.
 */

use crate::types::{DictionaryEntry, LanguageCode, MatchKind};

/// Fillers are listed per language. A language absent from here simply gets no
/// filler removal — see the module WHY.
const ENGLISH_FILLERS: &[&str] = &[
    "um", "uh", "erm", "hmm", "mhm", "uhh", "umm", "er", "ah", "like", "you know", "i mean",
    "sort of", "kind of", "basically", "literally", "actually",
];

/// Only the unambiguous ones. "period" is deliberately absent as a bare word
/// because it is a real noun — it is handled as "full stop" and "new line"
/// style multi-word phrases only.
const ENGLISH_SPOKEN_COMMANDS: &[(&str, &str)] = &[
    ("new paragraph", "\n\n"),
    ("new line", "\n"),
    ("full stop", "."),
    ("question mark", "?"),
    ("exclamation mark", "!"),
    ("open quote", "\""),
    ("close quote", "\""),
    ("open paren", "("),
    ("close paren", ")"),
    ("semicolon", ";"),
    ("colon", ":"),
    ("comma", ","),
];

fn is_english(language: Option<&LanguageCode>) -> bool {
    language.map(|l| l.as_str().starts_with("en")).unwrap_or(false)
}

/**
 * WHAT:  Collapses runs of whitespace and trims the ends, preserving newlines.
 * WHY:   Runs first so every later rule can assume single spaces. Newlines
 *        survive because spoken-command expansion produces them deliberately.
 */
pub fn normalise_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = false;
    let mut newline_run = 0usize;

    for ch in text.chars() {
        match ch {
            '\n' => {
                // Two newlines is a paragraph; more than that is noise.
                if newline_run < 2 {
                    out.push('\n');
                    newline_run += 1;
                }
                last_was_space = true;
            }
            c if c.is_whitespace() => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            c => {
                out.push(c);
                last_was_space = false;
                newline_run = 0;
            }
        }
    }

    out.trim().to_string()
}

/**
 * WHAT:  Turns "new line", "comma" and friends into the characters they name.
 * WHY:   Matched longest-first so "new paragraph" is not consumed by "new
 *        line"'s prefix, and only on whole-word boundaries so "commander" is
 *        never rewritten to ",nder".
 * WHERE: Runs before punctuation normalisation, so what it inserts gets tidied.
 */
pub fn expand_spoken_commands(text: &str, language: Option<&LanguageCode>) -> String {
    if !is_english(language) {
        return text.to_string();
    }

    let mut commands: Vec<&(&str, &str)> = ENGLISH_SPOKEN_COMMANDS.iter().collect();
    commands.sort_by_key(|(phrase, _)| std::cmp::Reverse(phrase.len()));

    let mut out = text.to_string();
    for (phrase, replacement) in commands {
        out = replace_whole_words(&out, phrase, replacement, false);
    }
    out
}

/**
 * WHAT:  Removes filler words.
 * WHY:   Whole-word and case-insensitive. Off by default in settings, because
 *        one person's filler is another person's emphasis — "actually" and
 *        "literally" carry meaning often enough that removing them silently
 *        would be a change to what someone said.
 */
pub fn strip_fillers(text: &str, language: Option<&LanguageCode>) -> String {
    if !is_english(language) {
        return text.to_string();
    }

    let mut out = text.to_string();
    let mut fillers: Vec<&&str> = ENGLISH_FILLERS.iter().collect();
    // Longest first so "you know" is removed before "know" could be considered.
    fillers.sort_by_key(|f| std::cmp::Reverse(f.len()));

    for filler in fillers {
        out = replace_whole_words(&out, filler, "", false);
    }
    normalise_whitespace(&out)
}

/**
 * WHAT:  Applies the user's replacement table.
 * WHY:   The single highest-leverage rule in the app — a model that writes
 *        "clod code" instead of "Claude Code" feels broken regardless of its
 *        word error rate. Word matching is the default because substring
 *        matching silently corrupts unrelated words.
 */
pub fn apply_dictionary(text: &str, entries: &[DictionaryEntry]) -> String {
    let mut out = text.to_string();
    for entry in entries.iter().filter(|e| e.enabled) {
        out = match entry.match_kind {
            MatchKind::Word => replace_whole_words(&out, &entry.pattern, &entry.replacement, false),
            MatchKind::WordCaseSensitive => {
                replace_whole_words(&out, &entry.pattern, &entry.replacement, true)
            }
            MatchKind::Substring => out.replace(&entry.pattern, &entry.replacement),
        };
    }
    out
}

/**
 * SOURCE OF TRUTH KEYWORDS: dedupe_stutters
 * WHAT:  Collapses an immediately repeated word or short phrase.
 * WHY:   Chunk boundaries overlap by design, so a word spoken across a seam can
 *        be decoded twice. This catches that, and genuine spoken stutters with
 *        it. Limited to repeats of at most three words and only when they are
 *        ADJACENT — beyond that, repetition is usually real speech ("very very
 *        good" is a thing people say, but "the the" is not).
 * WHERE: Runs after the seam join in the assembler.
 */
/**
 * SOURCE OF TRUTH KEYWORDS: CORRECTION_CUES, apply_spoken_corrections,
 *   alignment, restart, MAX_ALIGN_WORDS
 * WHAT:  The interjections that can introduce a spoken self-correction.
 * WHY:   **Measured against 132 of the operator's own transcripts, not
 *        imagined.** The previous version of this rule keyed on "I meant",
 *        "make that" and "scratch that" — and those three phrases appear ZERO
 *        times in the entire corpus. That is why the feature worked "10% of the
 *        time": it was listening for words he never says.
 *
 *        What he actually says, every instance in the corpus:
 *          "I have a meeting on Tuesday, sorry, on Wednesday at 3 p.m."
 *          "...minimalism and no focus. Sorry, no distraction."
 *          "Hey, can you, oh wait, can you hear me?"
 *          "Can you hear me? I mean, can you hear everything I'm saying?"
 *          "So this is a... Actually, sorry, let's start again. Basically, ..."
 *
 *        None of them contain a correction VERB. Every one of them is a bare
 *        interjection followed by a phrase that is structurally PARALLEL to the
 *        one before it. So the cue set here is permissive and the parallel
 *        alignment does the gating — which is the right way round, because a
 *        cue is common and an alignment is not.
 *
 *        "correction" is deliberately absent: he uses it as a noun, about this
 *        very feature, four times in the corpus.
 * WHERE: find_cue_run, used by apply_spoken_corrections.
 */
const CORRECTION_CUES: &[&str] = &[
    "sorry", "no", "wait", "oh", "oops", "actually", "i mean", "i meant", "make that",
    "scratch that", "my bad",
];

/// Phrases that mean "throw away everything I just said".
const RESTART_PHRASES: &[&str] = &[
    "let's start again",
    "lets start again",
    "let me start again",
    "let's start over",
    "let me start over",
    "start again",
    "start over",
];

/**
 * SOURCE OF TRUTH KEYWORDS: MAX_ALIGN_WORDS
 * WHAT:  Longest parallel span the rule will replace.
 * WHY:   A correction fixes a slip — a word, a date, a short phrase. Four words
 *        covers every real case in the corpus ("on Wednesday", "no distraction",
 *        "can you") with room to spare, and caps the damage if an alignment is
 *        ever found where none was meant.
 */
const MAX_ALIGN_WORDS: usize = 4;

/// A word plus where it sits in the original string, so a replacement can be
/// spliced without rebuilding — and therefore without disturbing anything else.
#[derive(Debug, Clone, Copy)]
struct Word<'a> {
    text: &'a str,
    /// Byte offset of the word in the original string. Only the START is kept:
    /// every splice here takes "everything from this word onwards", so an end
    /// offset would be a field nothing reads.
    start: usize,
}

fn words_with_offsets(text: &str) -> Vec<Word<'_>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                out.push(Word { text: &text[s..i], start: s });
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(Word { text: &text[s..], start: s });
    }
    out
}

/// Lowercased letters and digits only. "Tuesday," and "tuesday" compare equal.
fn bare(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect::<String>()
        .to_lowercase()
}

/// True when this word ends a clause — the next word starts a new one.
fn ends_clause(word: &str) -> bool {
    word.ends_with(',')
        || word.ends_with('.')
        || word.ends_with(';')
        || word.ends_with(':')
        || word.ends_with('?')
        || word.ends_with('!')
        || word.ends_with('\u{2014}')
}

/**
 * SOURCE OF TRUTH KEYWORDS: apply_spoken_corrections
 * WHAT:  Applies spoken self-corrections. Returns the input UNCHANGED unless a
 *        correction is actually found.
 * WHY:   Byte-identical when nothing fires, and that is a correctness
 *        requirement rather than an optimisation. The previous version split the
 *        text into sentences and rejoined them, which silently rewrote "..."
 *        into ". . ." and "3 p.m." into "3 p. m." on every transcript it touched
 *        — measured on 11 of the operator's 132 sessions, none of which
 *        contained a correction at all. A rule that damages text it has no
 *        business changing is worse than no rule.
 *
 *        Two shapes, both requiring STRUCTURAL evidence rather than a keyword:
 *
 *          1. RESTART — "let's start again" and friends. Everything before it
 *             goes. Unambiguous, so no alignment is needed.
 *          2. CUED ALIGNMENT — an interjection with parallel phrases either
 *             side. "on Tuesday, sorry, on Wednesday" aligns on the shared
 *             leading "on"; "can you, oh wait, can you hear me" aligns on a
 *             repeat. Identical spans mean a restart of that phrase (drop the
 *             first); differing spans mean a replacement (keep the second).
 *
 *        The cue alone never fires anything. That is what keeps "Wait, let me
 *        think about that" and "I actually don't like the orange theme" intact
 *        — there is no parallel span, so there is nothing to align.
 * WHERE: Step 3b of RuleEnhancer::enhance, gated on the autocorrect setting.
 */
pub fn apply_spoken_corrections(text: &str, language: Option<&LanguageCode>) -> String {
    if !is_english(language) {
        return text.to_string();
    }

    let mut current = text.to_string();
    for _ in 0..MAX_CORRECTION_PASSES {
        let next = apply_restart(&current)
            .or_else(|| apply_cued_alignment(&current))
            .unwrap_or_else(|| current.clone());
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// How many chained corrections one transcript may contain.
const MAX_CORRECTION_PASSES: usize = 4;

/**
 * WHAT:  "…rubbish… let's start again. The real thing." → "The real thing."
 * WHY:   The one shape that needs no alignment, because the phrase says outright
 *        that what came before is discarded. Requires something AFTER it — a
 *        restart with nothing following is someone announcing an intention, not
 *        performing one.
 */
fn apply_restart(text: &str) -> Option<String> {
    let words = words_with_offsets(text);
    let bares: Vec<String> = words.iter().map(|w| bare(w.text)).collect();

    let mut best: Option<usize> = None;
    for phrase in RESTART_PHRASES {
        let parts: Vec<&str> = phrase.split(' ').collect();
        if parts.len() > bares.len() {
            continue;
        }
        for start in 0..=(bares.len() - parts.len()) {
            if bares[start..start + parts.len()]
                .iter()
                .zip(parts.iter())
                .all(|(w, p)| w == p)
            {
                let after = start + parts.len();
                // Must leave something behind, and the phrase must end its
                // clause — "start over" inside "we can start over the weekend"
                // does not.
                if after < words.len() && ends_clause(words[after - 1].text) {
                    best = Some(match best {
                        Some(previous) if previous > after => previous,
                        _ => after,
                    });
                }
            }
        }
    }

    let after = best?;
    Some(text[words[after].start..].trim().to_string())
}

/**
 * WHAT:  Finds a cue run with parallel phrases either side and resolves it.
 * WHERE: The second and main shape; see the module-level WHY.
 */
fn apply_cued_alignment(text: &str) -> Option<String> {
    let words = words_with_offsets(text);
    let bares: Vec<String> = words.iter().map(|w| bare(w.text)).collect();

    // Walk cue runs from the LAST backwards, so "A, no, B, sorry, C" resolves C
    // first and the earlier pair on the next pass.
    let mut cue_starts: Vec<(usize, usize)> = Vec::new();
    let mut index = 0;
    while index < words.len() {
        if let Some(len) = cue_run_len(&bares, &words, index) {
            cue_starts.push((index, len));
            index += len;
        } else {
            index += 1;
        }
    }

    for (cue_at, cue_len) in cue_starts.into_iter().rev() {
        let after = cue_at + cue_len;
        if cue_at == 0 || after >= words.len() {
            continue;
        }

        // Longest alignment wins: "on Wednesday" beats "Wednesday".
        for span in (1..=MAX_ALIGN_WORDS.min(cue_at).min(words.len() - after)).rev() {
            let before = &bares[cue_at - span..cue_at];
            let candidate = &bares[after..after + span];

            if before.first() != candidate.first() {
                continue;
            }

            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..words[cue_at - span].start]);
            if before == candidate {
                // A restart of the same phrase: drop the first attempt and the
                // cue, keep everything from the repeat onwards.
                out.push_str(text[words[after].start..].trim_end());
            } else {
                // A replacement: the second phrase stands in for the first, and
                // everything after it survives untouched.
                out.push_str(text[words[after].start..].trim_end());
            }
            return Some(out);
        }
    }
    None
}

/// The length of a cue run starting at `index`, if one starts there.
/// A run must begin a clause and end one — a bare interjection, set off by
/// punctuation, which is what distinguishes "sorry," from "sorry about that".
fn cue_run_len(bares: &[String], words: &[Word<'_>], index: usize) -> Option<usize> {
    if index == 0 || !ends_clause(words[index - 1].text) {
        return None;
    }

    let mut len = 0;
    while index + len < words.len() {
        let remaining = &bares[index + len..];
        let matched = CORRECTION_CUES
            .iter()
            .filter_map(|cue| {
                let parts: Vec<&str> = cue.split(' ').collect();
                (remaining.len() >= parts.len()
                    && remaining[..parts.len()]
                        .iter()
                        .zip(parts.iter())
                        .all(|(w, p)| w == p))
                .then_some(parts.len())
            })
            .max();

        len += matched?;
        // The run ends where the punctuation does.
        if ends_clause(words[index + len - 1].text) {
            return Some(len);
        }
    }
    None
}

pub fn dedupe_stutters(text: &str) -> String {
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    if words.len() < 2 {
        return text.to_string();
    }

    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    let mut index = 0usize;

    while index < words.len() {
        let mut matched = false;

        // Try the longest phrase first, so "how do you how do you" collapses as
        // a phrase rather than leaving fragments behind.
        for span in (1..=3).rev() {
            if index + span * 2 > words.len() {
                continue;
            }
            let first = &words[index..index + span];
            let second = &words[index + span..index + span * 2];

            if phrases_match(first, second) {
                out.extend_from_slice(first);
                index += span * 2;
                matched = true;
                break;
            }
        }

        if !matched {
            out.push(words[index]);
            index += 1;
        }
    }

    out.join(" ")
}

fn phrases_match(a: &[&str], b: &[&str]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            // Compare on letters only, so "the" and "the," count as a repeat.
            let strip = |s: &str| {
                s.chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(|c| c.to_lowercase())
                    .collect::<String>()
            };
            let (x, y) = (strip(x), strip(y));
            !x.is_empty() && x == y
        })
}

/**
 * WHAT:  Tidies spacing around punctuation and normalises quote characters.
 * WHY:   Whisper occasionally emits a space before a comma or doubles a full
 *        stop. Left alone these read as sloppy typing rather than as speech.
 */
pub fn normalise_punctuation(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Curly quotes to straight: the output is usually going into a
            // terminal, an editor or a prompt, where curly quotes break things.
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '\u{201C}' | '\u{201D}' => out.push('"'),
            '\u{2013}' | '\u{2014}' => out.push('-'),
            ' ' => {
                // Drop a space that sits before closing punctuation.
                if matches!(chars.peek(), Some(&next) if matches!(next, ',' | '.' | '!' | '?' | ';' | ':')) {
                    continue;
                }
                out.push(' ');
            }
            c if matches!(c, '.' | ',' | '!' | '?') => {
                out.push(c);
                // Collapse a doubled terminal mark, but leave "..." alone —
                // an ellipsis is intentional often enough to preserve.
                if c != '.' {
                    while chars.peek() == Some(&c) {
                        chars.next();
                    }
                }
            }
            c => out.push(c),
        }
    }

    out
}

/**
 * WHAT:  Capitalises the first letter of the text and of each new sentence.
 * WHY:   Whisper is trained on punctuated text and usually gets this right, so
 *        this is a repair pass rather than the primary mechanism. It only ever
 *        upper-cases a leading letter — it never lower-cases anything, because
 *        that would destroy proper nouns and acronyms the model got right.
 */
pub fn capitalise_sentences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut capitalise_next = true;

    for ch in text.chars() {
        if capitalise_next && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            capitalise_next = false;
        } else {
            out.push(ch);
            if matches!(ch, '.' | '!' | '?' | '\n') {
                capitalise_next = true;
            }
        }
    }

    out
}

/**
 * WHAT:  Adds a full stop if the text does not already end in punctuation.
 * WHY:   Skipped when the text ends in a newline from an explicit "new
 *        paragraph" command — the user was structuring, not finishing a
 *        sentence, and appending a stray period there is visibly wrong.
 */
pub fn ensure_terminal_punctuation(text: &str) -> String {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return text.to_string();
    }

    let ends_cleanly = trimmed
        .chars()
        .last()
        .map(|c| matches!(c, '.' | '!' | '?' | ':' | ';' | ',' | '"' | ')'))
        .unwrap_or(false);

    if ends_cleanly || text.ends_with('\n') {
        return text.to_string();
    }

    format!("{trimmed}.")
}

/**
 * SOURCE OF TRUTH KEYWORDS: replace_whole_words
 * WHAT:  Replaces `needle` with `replacement` only at word boundaries.
 * WHY:   Written by hand rather than with a regex crate: the patterns come from
 *        user input, and compiling user text as a regex is both a correctness
 *        hazard and a way to make an unbounded-time replacement. Boundaries are
 *        "not alphanumeric on either side", which is what stops a replacement
 *        of "ai" from rewriting "said".
 * WHERE: The matching primitive under the dictionary, filler and spoken-command
 *        rules.
 */
fn replace_whole_words(
    haystack: &str,
    needle: &str,
    replacement: &str,
    case_sensitive: bool,
) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }

    let subject = if case_sensitive {
        haystack.to_string()
    } else {
        haystack.to_lowercase()
    };
    let pattern = if case_sensitive {
        needle.to_string()
    } else {
        needle.to_lowercase()
    };

    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0usize;

    while let Some(found) = subject[cursor..].find(&pattern) {
        let start = cursor + found;
        let end = start + pattern.len();

        let before_ok = start == 0
            || !subject[..start]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);
        let after_ok = end >= subject.len()
            || !subject[end..]
                .chars()
                .next()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);

        if before_ok && after_ok {
            out.push_str(&haystack[cursor..start]);
            out.push_str(replacement);
        } else {
            out.push_str(&haystack[cursor..end]);
        }
        cursor = end;
    }

    out.push_str(&haystack[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DictionaryId;

    fn english() -> LanguageCode {
        LanguageCode("en".into())
    }

    fn entry(pattern: &str, replacement: &str, kind: MatchKind) -> DictionaryEntry {
        DictionaryEntry {
            id: DictionaryId(1),
            pattern: pattern.into(),
            replacement: replacement.into(),
            match_kind: kind,
            enabled: true,
            used_at: None,
        }
    }

    // ── whitespace ───────────────────────────────────────────────────────

    #[test]
    fn whitespace_collapses_but_paragraphs_survive() {
        assert_eq!(normalise_whitespace("  a   b  "), "a b");
        assert_eq!(normalise_whitespace("a\n\n\n\nb"), "a\n\nb");
        assert_eq!(normalise_whitespace("a\nb"), "a\nb");
    }

    // ── spoken commands ──────────────────────────────────────────────────

    #[test]
    fn spoken_commands_expand_longest_first() {
        let out = expand_spoken_commands("hello new paragraph world", Some(&english()));
        assert_eq!(out.trim(), "hello \n\n world".trim());
    }

    #[test]
    fn spoken_commands_do_not_fire_inside_a_longer_word() {
        // "commander" must survive a rule that replaces "comma".
        let out = expand_spoken_commands("the commander said", Some(&english()));
        assert_eq!(out, "the commander said");
    }

    #[test]
    fn spoken_commands_are_skipped_for_other_languages() {
        let hindi = LanguageCode("hi".into());
        let text = "comma new line";
        assert_eq!(expand_spoken_commands(text, Some(&hindi)), text);
    }

    // ── fillers ──────────────────────────────────────────────────────────

    #[test]
    fn fillers_are_removed_whole_word_only() {
        let out = strip_fillers("um so I was like thinking", Some(&english()));
        assert_eq!(out, "so I was thinking");
    }

    #[test]
    fn filler_removal_does_not_touch_words_that_contain_a_filler() {
        // "like" is a filler; "likely" is not.
        let out = strip_fillers("that is likely umbrella weather", Some(&english()));
        assert_eq!(out, "that is likely umbrella weather");
    }

    #[test]
    fn filler_removal_never_runs_on_an_unknown_language() {
        // The rule that stops us deleting real Hindi or Arabic words.
        let arabic = LanguageCode("ar".into());
        let text = "um like actually";
        assert_eq!(strip_fillers(text, Some(&arabic)), text);
        assert_eq!(strip_fillers(text, None), text);
    }

    // ── dictionary ───────────────────────────────────────────────────────

    #[test]
    fn dictionary_fixes_the_canonical_case() {
        let entries = [entry("clod code", "Claude Code", MatchKind::Word)];
        assert_eq!(
            apply_dictionary("i opened clod code today", &entries),
            "i opened Claude Code today"
        );
    }

    #[test]
    fn a_word_replacement_cannot_corrupt_a_longer_word() {
        // The reason Word is the default and Substring is opt-in.
        let entries = [entry("ai", "AI", MatchKind::Word)];
        assert_eq!(
            apply_dictionary("he said ai is said", &entries),
            "he said AI is said"
        );
    }

    #[test]
    fn substring_matching_is_available_when_explicitly_chosen() {
        let entries = [entry("ai", "AI", MatchKind::Substring)];
        assert_eq!(apply_dictionary("said", &entries), "sAId");
    }

    #[test]
    fn disabled_entries_are_ignored() {
        let mut e = entry("clod", "Claude", MatchKind::Word);
        e.enabled = false;
        assert_eq!(apply_dictionary("clod code", &[e]), "clod code");
    }

    // ── stutters and seams ───────────────────────────────────────────────

    #[test]
    fn an_immediately_repeated_word_is_collapsed() {
        assert_eq!(dedupe_stutters("the the cat sat"), "the cat sat");
    }

    #[test]
    fn a_repeated_phrase_across_a_seam_is_collapsed_as_a_phrase() {
        // Exactly the chunk-overlap case: the last words of one chunk repeated
        // as the first words of the next.
        assert_eq!(
            dedupe_stutters("i want to i want to go home"),
            "i want to go home"
        );
    }

    #[test]
    fn repetition_that_is_real_speech_is_preserved() {
        // Not adjacent duplicates of the same span — this is how people talk.
        assert_eq!(dedupe_stutters("very good and very bad"), "very good and very bad");
    }

    #[test]
    fn punctuation_does_not_hide_a_duplicate() {
        assert_eq!(dedupe_stutters("hello, hello there"), "hello, there");
    }

    // ── punctuation and casing ───────────────────────────────────────────

    #[test]
    fn punctuation_spacing_is_tidied() {
        assert_eq!(normalise_punctuation("hello , world ."), "hello, world.");
    }

    #[test]
    fn curly_quotes_become_straight_ones() {
        assert_eq!(
            normalise_punctuation("\u{201C}hello\u{201D} it\u{2019}s"),
            "\"hello\" it's"
        );
    }

    #[test]
    fn an_ellipsis_survives_but_a_doubled_bang_does_not() {
        assert_eq!(normalise_punctuation("wait..."), "wait...");
        assert_eq!(normalise_punctuation("stop!!"), "stop!");
    }

    #[test]
    fn sentences_are_capitalised_without_lowercasing_anything() {
        assert_eq!(
            capitalise_sentences("hello there. how are you?"),
            "Hello there. How are you?"
        );
        // Acronyms and proper nouns the model got right must survive.
        assert_eq!(capitalise_sentences("the API is fine"), "The API is fine");
    }

    #[test]
    fn terminal_punctuation_is_added_only_when_missing() {
        assert_eq!(ensure_terminal_punctuation("hello"), "hello.");
        assert_eq!(ensure_terminal_punctuation("hello."), "hello.");
        assert_eq!(ensure_terminal_punctuation("really?"), "really?");
    }

    #[test]
    fn a_deliberate_trailing_newline_does_not_get_a_full_stop() {
        assert_eq!(ensure_terminal_punctuation("a list\n\n"), "a list\n\n");
    }

    #[test]
    fn empty_input_stays_empty_through_every_rule() {
        assert_eq!(normalise_whitespace(""), "");
        assert_eq!(strip_fillers("", Some(&english())), "");
        assert_eq!(dedupe_stutters(""), "");
        assert_eq!(capitalise_sentences(""), "");
        assert_eq!(ensure_terminal_punctuation(""), "");
    }
}

#[cfg(test)]
mod correction_tests {
    use super::*;

    fn en() -> LanguageCode {
        LanguageCode("en".into())
    }

    fn correct(text: &str) -> String {
        apply_spoken_corrections(text, Some(&en()))
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: real_corrections_from_the_corpus
     * WHAT:  Every genuine self-correction found in 132 of the operator's own
     *        transcripts.
     * WHY:   These are not invented examples. They are the complete set of real
     *        corrections in the corpus, and they are the reason the rule looks
     *        the way it does: not one of them contains a correction verb like
     *        "I meant". Every one is a bare interjection with structurally
     *        parallel phrases either side.
     *
     *        If a change to CORRECTION_CUES or the alignment breaks one of
     *        these, it has broken the feature on the only data we have.
     */
    #[test]
    fn the_real_corrections_from_the_corpus_all_work() {
        // A replacement: the phrases share their leading "on".
        assert_eq!(
            correct("I have a meeting on Tuesday, sorry, on Wednesday at 3 p.m."),
            "I have a meeting on Wednesday at 3 p.m."
        );

        // A replacement across a sentence boundary, sharing "no".
        assert_eq!(
            correct("This is focused on minimalism and no focus. Sorry, no distraction."),
            "This is focused on minimalism and no distraction."
        );

        // A restart: the repeated "can you" is the signal, so the first attempt
        // goes and the second stands.
        assert_eq!(
            correct("Hey, can you, oh wait, can you hear me?"),
            "Hey, can you hear me?"
        );

        // An explicit restart discards everything before it.
        assert_eq!(
            correct("So this is a... Actually, sorry, let's start again. Basically, welcome to my new YouTube video."),
            "Basically, welcome to my new YouTube video."
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: the_rule_never_damages_text_it_does_not_correct
     * WHAT:  Text with no correction in it comes back byte-identical.
     * WHY:   The failure this replaced. The old rule split sentences and
     *        rejoined them, so every transcript it touched came back with "..."
     *        turned into ". . ." and "3 p.m." into "3 p. m." — on 11 of the
     *        operator's sessions, NONE of which contained a correction. Damage
     *        with no benefit is strictly worse than doing nothing, so
     *        "unchanged" is asserted byte-for-byte rather than approximately.
     */
    #[test]
    fn the_rule_never_damages_text_it_does_not_correct() {
        for text in [
            "It looks like those buttons are... trying to fit inside the sidebar.",
            "I have a meeting at 3 p.m. on Wednesday.",
            "The circle... Progress bar doesn't actually progress.",
            "Say lol This is absolutely free. Enjoy and put a heart icon.",
        ] {
            assert_eq!(correct(text), text, "text with no correction was rewritten");
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: innocent_cue_words_are_left_alone
     * WHAT:  Ordinary sentences containing the cue words are untouched.
     * WHY:   Still the test that matters most, and now drawn from real usage:
     *        "actually" appears 20 times in the corpus and is an adverb in 18 of
     *        them, "instead" appears 8 times and is never a correction, and
     *        "correction" appears 4 times as a noun about this very feature.
     *        The cue set is permissive on purpose — the parallel ALIGNMENT is
     *        what gates — so this is the proof that a cue alone fires nothing.
     */
    #[test]
    fn ordinary_sentences_containing_the_cue_words_are_untouched() {
        for sentence in [
            "This is me testing out to see if the app actually works.",
            "I actually don't like the orange color theme.",
            "Instead of showing a timer, just delete it.",
            "The other feature I want built into this app is auto correction.",
            "Wait, let me think about that.",
            "I'm sorry about the delay.",
            "What I mean by this is they can get the updated version.",
            "We can start over the weekend if that suits you.",
            "No, that's not what the graph shows.",
            "What I meant was clear enough.",
        ] {
            assert_eq!(
                correct(sentence),
                sentence,
                "an ordinary sentence was rewritten"
            );
        }
    }

    /**
     * WHAT:  A cue with no parallel phrase before it does nothing.
     * WHY:   This is the whole precision argument stated as a test. The cue set
     *        is deliberately loose, so if alignment ever stopped being required
     *        the rule would start eating the front of sentences that merely
     *        begin with "Sorry," or "No,".
     */
    #[test]
    fn a_cue_without_an_alignment_never_fires() {
        for sentence in [
            "Sorry, I will send it over tonight.",
            "No, the meeting is still going ahead.",
            "Oh, that reminds me of something else.",
        ] {
            assert_eq!(correct(sentence), sentence);
        }
    }

    #[test]
    fn a_replacement_never_reaches_further_than_the_cap() {
        // The parallel span is capped, so an alignment can never swallow the
        // start of a long sentence.
        let input = "The file lives in the old project folder somewhere, sorry, in the new shared documents folder.";
        let out = correct(input);
        assert!(
            out.starts_with("The file lives"),
            "the cap did not protect the start of the sentence: {out}"
        );
    }

    #[test]
    fn corrections_are_english_only() {
        let fr = LanguageCode("fr".into());
        let input = "on mardi, sorry, on mercredi.";
        assert_eq!(apply_spoken_corrections(input, Some(&fr)), input);
    }
}

#[cfg(test)]
mod corpus_measurement {
    use super::*;

    /**
     * SOURCE OF TRUTH KEYWORDS: measure_against_real_transcripts
     * WHAT:  Runs the correction rule over every transcript in the local
     *        history and prints what it changed. Ignored by default; run with
     *        `cargo test measure_against_real -- --ignored --nocapture`.
     * WHY:   This is how the rule was rebuilt, and it is the only honest way to
     *        change it again. The previous marker set — "I meant", "make that",
     *        "scratch that" — was invented from what a correction sounds like in
     *        the abstract, and appears ZERO times in 132 real sessions. The
     *        rule fired on 11 transcripts, corrected none of them, and quietly
     *        rewrote "..." into ". . ." on all 11.
     *
     *        Not a gate: it needs a populated database, which CI does not have,
     *        and its output is read by a person rather than asserted. The
     *        assertions that came OUT of it live in `correction_tests`.
     * WHERE: Run by hand before changing CORRECTION_CUES or the alignment rule.
     */
    #[test]
    #[ignore]
    fn measure_against_real_transcripts() {
        let Ok(paths) = crate::config::AppPaths::resolve() else {
            eprintln!("skipped: no application support directory");
            return;
        };
        let Ok(db) = crate::db::Database::open(&paths.db_path) else {
            eprintln!("skipped: no local history database");
            return;
        };

        let sessions = crate::services::sessions::list_sessions(&db, 10_000, 0).unwrap_or_default();
        let en = LanguageCode("en".into());
        let (mut changed, mut total) = (0, 0);

        for session in &sessions {
            let Some(raw) = session.raw_text.as_deref() else {
                continue;
            };
            if raw.trim().is_empty() {
                continue;
            }
            total += 1;
            let out = apply_spoken_corrections(raw, Some(&en));
            if out != raw {
                changed += 1;
                println!("\n--- BEFORE: {}", raw.trim());
                println!("--- AFTER : {}", out.trim());
            }
        }
        println!("\n=== fired on {changed} of {total} transcripts ===");
    }
}
