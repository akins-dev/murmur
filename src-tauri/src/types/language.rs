/*!
 * SOURCE OF TRUTH KEYWORDS: LanguageOption, WHISPER_LANGUAGES, language_label,
 *   language_options, AUTO_LANGUAGE
 * WHAT:  Every language the app can offer, and the shape the Settings picker
 *        renders.
 * WHY:   One table, so the picker, the engine gating and the stats breakdown
 *        all name a language the same way. `supported` is computed against the
 *        SELECTED engine's declared capabilities rather than assumed — that is
 *        the mechanism that stops the UI offering Hindi on an engine which
 *        cannot speak it, which docs/01 §6 names as the whole reason the engine
 *        seam exists. Without this the `requires_engine` field on a setting is
 *        decorative.
 * WHERE: Resolved by the list_languages command for ChoiceSource::Languages;
 *        labels reused by the Stats language breakdown.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use super::engine::LanguageSupport;
use super::transcript::LanguageCode;

/// The sentinel meaning "detect it". Not a real language code.
pub const AUTO_LANGUAGE: &str = "auto";

/**
 * SOURCE OF TRUTH KEYWORDS: LanguageOption
 * WHAT:  One entry in the language picker.
 * WHY:   `supported` travels with the option instead of the UI filtering the
 *        list, so a picker can SHOW an unavailable language greyed out with a
 *        reason rather than silently omitting it — a user who dictates in Hindi
 *        needs to know the engine is why it vanished.
 * WHERE: Returned by list_languages.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LanguageOption {
    pub code: String,
    /// English name, for a UI in English.
    pub label: String,
    /// Whether the currently selected engine can transcribe it.
    pub supported: bool,
}

/**
 * SOURCE OF TRUTH KEYWORDS: WHISPER_LANGUAGES
 * WHAT:  The 99 languages Whisper is trained on, as (code, English name).
 * WHY:   Held as data rather than derived from the engine, because an engine
 *        that declares `LanguageSupport::All` cannot enumerate itself — and a
 *        picker needs names, not just codes.
 * WHERE: The source for language_options.
 */
pub const WHISPER_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"), ("zh", "Chinese"), ("de", "German"), ("es", "Spanish"),
    ("ru", "Russian"), ("ko", "Korean"), ("fr", "French"), ("ja", "Japanese"),
    ("pt", "Portuguese"), ("tr", "Turkish"), ("pl", "Polish"), ("ca", "Catalan"),
    ("nl", "Dutch"), ("ar", "Arabic"), ("sv", "Swedish"), ("it", "Italian"),
    ("id", "Indonesian"), ("hi", "Hindi"), ("fi", "Finnish"), ("vi", "Vietnamese"),
    ("he", "Hebrew"), ("uk", "Ukrainian"), ("el", "Greek"), ("ms", "Malay"),
    ("cs", "Czech"), ("ro", "Romanian"), ("da", "Danish"), ("hu", "Hungarian"),
    ("ta", "Tamil"), ("no", "Norwegian"), ("th", "Thai"), ("ur", "Urdu"),
    ("hr", "Croatian"), ("bg", "Bulgarian"), ("lt", "Lithuanian"), ("la", "Latin"),
    ("mi", "Maori"), ("ml", "Malayalam"), ("cy", "Welsh"), ("sk", "Slovak"),
    ("te", "Telugu"), ("fa", "Persian"), ("lv", "Latvian"), ("bn", "Bengali"),
    ("sr", "Serbian"), ("az", "Azerbaijani"), ("sl", "Slovenian"), ("kn", "Kannada"),
    ("et", "Estonian"), ("mk", "Macedonian"), ("br", "Breton"), ("eu", "Basque"),
    ("is", "Icelandic"), ("hy", "Armenian"), ("ne", "Nepali"), ("mn", "Mongolian"),
    ("bs", "Bosnian"), ("kk", "Kazakh"), ("sq", "Albanian"), ("sw", "Swahili"),
    ("gl", "Galician"), ("mr", "Marathi"), ("pa", "Punjabi"), ("si", "Sinhala"),
    ("km", "Khmer"), ("sn", "Shona"), ("yo", "Yoruba"), ("so", "Somali"),
    ("af", "Afrikaans"), ("oc", "Occitan"), ("ka", "Georgian"), ("be", "Belarusian"),
    ("tg", "Tajik"), ("sd", "Sindhi"), ("gu", "Gujarati"), ("am", "Amharic"),
    ("yi", "Yiddish"), ("lo", "Lao"), ("uz", "Uzbek"), ("fo", "Faroese"),
    ("ht", "Haitian Creole"), ("ps", "Pashto"), ("tk", "Turkmen"), ("nn", "Nynorsk"),
    ("mt", "Maltese"), ("sa", "Sanskrit"), ("lb", "Luxembourgish"), ("my", "Burmese"),
    ("bo", "Tibetan"), ("tl", "Tagalog"), ("mg", "Malagasy"), ("as", "Assamese"),
    ("tt", "Tatar"), ("haw", "Hawaiian"), ("ln", "Lingala"), ("ha", "Hausa"),
    ("ba", "Bashkir"), ("jw", "Javanese"), ("su", "Sundanese"), ("yue", "Cantonese"),
];

/// English name for a code, falling back to the code itself.
pub fn language_label(code: &str) -> &str {
    if code == AUTO_LANGUAGE {
        return "Detect automatically";
    }
    WHISPER_LANGUAGES
        .iter()
        .find(|(candidate, _)| *candidate == code)
        .map(|(_, label)| *label)
        .unwrap_or(code)
}

/**
 * SOURCE OF TRUTH KEYWORDS: language_options
 * WHAT:  The picker contents for a given engine, "Detect automatically" first
 *        and the rest alphabetical.
 * WHY:   Support is asked of the engine's declared capabilities, never inferred
 *        from its identity. That is the rule that keeps a second engine from
 *        needing changes here.
 * WHERE: Called by the list_languages command.
 */
pub fn language_options(support: &LanguageSupport, auto_detect: bool) -> Vec<LanguageOption> {
    let mut options = Vec::with_capacity(WHISPER_LANGUAGES.len() + 1);

    options.push(LanguageOption {
        code: AUTO_LANGUAGE.to_string(),
        label: language_label(AUTO_LANGUAGE).to_string(),
        supported: auto_detect,
    });

    let mut languages: Vec<LanguageOption> = WHISPER_LANGUAGES
        .iter()
        .map(|(code, label)| LanguageOption {
            code: (*code).to_string(),
            label: (*label).to_string(),
            supported: support.supports(&LanguageCode((*code).to_string())),
        })
        .collect();

    languages.sort_by(|a, b| a.label.cmp(&b.label));
    options.extend(languages);
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_the_product_promises_is_present() {
        // docs/01 §4.1 M6 names these explicitly.
        for code in ["hi", "ar", "es", "fr", "it", "de", "pt", "ja", "zh", "ko", "ru"] {
            assert!(
                WHISPER_LANGUAGES.iter().any(|(c, _)| *c == code),
                "`{code}` is promised in the ideation doc but missing here"
            );
        }
    }

    #[test]
    fn the_table_has_no_duplicate_codes() {
        let mut seen = std::collections::HashSet::new();
        for (code, _) in WHISPER_LANGUAGES {
            assert!(seen.insert(*code), "`{code}` appears twice");
        }
    }

    #[test]
    fn an_engine_with_gaps_marks_them_unsupported_rather_than_hiding_them() {
        // The Parakeet case: fast, but no Hindi and no Arabic. The picker must
        // be able to say WHY, which means the option is present and flagged.
        let support = LanguageSupport::Set {
            languages: vec![
                LanguageCode("en".into()),
                LanguageCode("fr".into()),
            ],
        };
        let options = language_options(&support, false);

        let hindi = options.iter().find(|o| o.code == "hi").expect("hindi listed");
        assert!(!hindi.supported, "hindi must be shown as unavailable");

        let english = options.iter().find(|o| o.code == "en").expect("english listed");
        assert!(english.supported);
    }

    #[test]
    fn an_all_languages_engine_supports_everything() {
        let options = language_options(&LanguageSupport::All, true);
        assert!(options.iter().all(|o| o.supported));
        assert_eq!(options.len(), WHISPER_LANGUAGES.len() + 1);
    }

    #[test]
    fn auto_is_first_and_reflects_the_engines_ability_to_detect() {
        let options = language_options(&LanguageSupport::All, false);
        assert_eq!(options[0].code, AUTO_LANGUAGE);
        assert!(
            !options[0].supported,
            "an engine that cannot auto-detect must not offer it"
        );
    }

    #[test]
    fn labels_resolve_and_fall_back_to_the_code() {
        assert_eq!(language_label("hi"), "Hindi");
        assert_eq!(language_label("auto"), "Detect automatically");
        assert_eq!(language_label("xx"), "xx");
    }
}
