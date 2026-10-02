/*!
 * SOURCE OF TRUTH KEYWORDS: CAPABILITIES, capabilities, capability,
 *   setting_def, default_settings, nav_items, hotkey_defs, all_settings,
 *   validate_registry, RegistrySnapshot
 * WHAT:  The single source of truth for what this app has. One table of
 *        capabilities, plus the lookups every other layer uses to read it.
 * WHY:   A feature is an entry here, never a new pattern. Adding one wires up
 *        its settings UI, its permission preflight, its nav placement, its
 *        hotkey and its metrics at once, because all five read this table
 *        instead of keeping their own copies. The rule that follows from it:
 *        if you are writing a `match` on a feature name anywhere outside this
 *        module, the branch belongs in the table instead.
 *
 *        Built through LazyLock rather than as a `const`, because the entries
 *        hold owned Strings and Vecs. That is deliberate — the same structs
 *        cross to TypeScript through specta unchanged, so the frontend reads
 *        the identical table rather than a hand-maintained mirror of it.
 * WHERE: Read by ipc/factory.rs (preflight), services/settings.rs (defaults and
 *        validation), the Settings view (control generation), the dashboard
 *        shell (nav) and the hotkey adapter (bindings).
 */

pub mod capability;
pub mod keys;

#[cfg(test)]
mod icons;

/// Asserts that every entry in this table is actually READ by something. Our
/// other guardrails check structure; this one checks reachability, which is the
/// gap every High finding in two independent audits fell through.
#[cfg(test)]
mod reachability;

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use specta::Type;

pub use capability::{
    Capability, CapabilityKey, HotkeyDef, MetricDef, NavDef, SettingDef, SettingSection,
};

use crate::ports::permissions::OsPermission;
use crate::types::settings::{ChoiceSource, SettingChoice};
use crate::types::{
    EngineFeature, HotkeyBinding, KeyModifier, LatencyStage, SettingKind, SettingValue,
};

/// The table. Everything else in this module is a view onto it.
pub static CAPABILITIES: LazyLock<Vec<Capability>> = LazyLock::new(build_capabilities);

// ── Small constructors, so the table below reads as data rather than noise ──

fn text(value: &str) -> String {
    value.to_string()
}

fn toggle(
    key: &str,
    label: &str,
    description: &str,
    section: SettingSection,
    default: bool,
) -> SettingDef {
    SettingDef {
        key: text(key),
        label: text(label),
        description: text(description),
        section,
        kind: SettingKind::Toggle,
        default: SettingValue::Bool(default),
        requires_engine: vec![],
        requires_permission: vec![],
        advanced: false,
    }
}

fn number(
    key: &str,
    label: &str,
    description: &str,
    section: SettingSection,
    (min, max, step): (f64, f64, f64),
    unit: Option<&str>,
    default: f64,
) -> SettingDef {
    SettingDef {
        key: text(key),
        label: text(label),
        description: text(description),
        section,
        kind: SettingKind::Number {
            min,
            max,
            step,
            unit: unit.map(text),
        },
        default: SettingValue::Number(default),
        requires_engine: vec![],
        requires_permission: vec![],
        advanced: false,
    }
}

fn choice(
    key: &str,
    label: &str,
    description: &str,
    section: SettingSection,
    options: &[(&str, &str, &str)],
    default: &str,
) -> SettingDef {
    SettingDef {
        key: text(key),
        label: text(label),
        description: text(description),
        section,
        kind: SettingKind::Choice {
            options: options
                .iter()
                .map(|(value, label, description)| SettingChoice {
                    value: text(value),
                    label: text(label),
                    description: if description.is_empty() {
                        None
                    } else {
                        Some(text(description))
                    },
                })
                .collect(),
        },
        default: SettingValue::Choice(text(default)),
        requires_engine: vec![],
        requires_permission: vec![],
        advanced: false,
    }
}

fn dynamic_choice(
    key: &str,
    label: &str,
    description: &str,
    section: SettingSection,
    source: ChoiceSource,
    default: &str,
) -> SettingDef {
    SettingDef {
        key: text(key),
        label: text(label),
        description: text(description),
        section,
        kind: SettingKind::DynamicChoice { source },
        default: SettingValue::Choice(text(default)),
        requires_engine: vec![],
        requires_permission: vec![],
        advanced: false,
    }
}

/// Marks a setting as depending on an OS grant. See SettingDef.
fn needs_permission(mut def: SettingDef, permission: OsPermission) -> SettingDef {
    def.requires_permission.push(permission);
    def
}

fn advanced(mut def: SettingDef) -> SettingDef {
    def.advanced = true;
    def
}

fn metric(stage: LatencyStage, label: &str, user_facing: bool) -> MetricDef {
    MetricDef {
        stage,
        label: text(label),
        user_facing,
    }
}

/// ⌥Space — the default dictation hotkey.
fn default_hotkey() -> HotkeyBinding {
    HotkeyBinding {
        modifiers: vec![KeyModifier::Option],
        key: text("Space"),
    }
}

// ── The table ────────────────────────────────────────────────────────────

fn build_capabilities() -> Vec<Capability> {
    vec![
        Capability {
            key: CapabilityKey::Dictation,
            name: text("Dictation"),
            description: text("Press a hotkey, talk, press it again — your words are pasted where you were typing."),
            requires: vec![OsPermission::Microphone],
            engine_needs: vec![EngineFeature::Streaming],
            nav: None,
            hotkey: Some(HotkeyDef {
                id: text("dictation.toggle"),
                label: text("Start / stop dictation"),
                default: default_hotkey(),
                setting_key: Some(text(keys::DICTATION_HOTKEY)),
            }),
            metrics: vec![
                metric(LatencyStage::HotkeyDispatch, "Hotkey", false),
                metric(LatencyStage::DeviceOpen, "Microphone open", false),
                metric(LatencyStage::ChunkDecode, "Background decode", false),
                metric(LatencyStage::TailDecode, "Final decode", true),
                metric(LatencyStage::Assemble, "Assemble", false),
                metric(LatencyStage::Enhance, "Enhance", false),
                metric(LatencyStage::TotalFinalize, "Stop to pasted", true),
            ],
            settings: vec![
                SettingDef {
                    key: text(keys::DICTATION_HOTKEY),
                    label: text("Dictation hotkey"),
                    description: text("The key you press anywhere to start and stop."),
                    section: SettingSection::Recording,
                    kind: SettingKind::Hotkey,
                    default: SettingValue::Hotkey(default_hotkey()),
                    requires_engine: vec![],
                    requires_permission: vec![],
                    advanced: false,
                },
                choice(
                    keys::DICTATION_MODE,
                    "Hotkey behaviour",
                    "Toggle presses once to start and once to stop. Push-to-talk records only while held.",
                    SettingSection::Recording,
                    &[
                        ("toggle", "Toggle", "Press to start, press again to stop"),
                        ("push_to_talk", "Push to talk", "Hold to record, release to send"),
                    ],
                    "toggle",
                ),
                choice(
                    keys::CAPTURE_MODE,
                    "Microphone",
                    "Instant keeps the microphone open so the first syllable is never clipped — macOS will show the orange microphone indicator at all times.",
                    SettingSection::Recording,
                    &[
                        ("on_demand", "Open when I press the key", "Indicator only shows while recording"),
                        ("instant", "Keep it warm (instant)", "Never clips the first word; indicator always lit"),
                    ],
                    "on_demand",
                ),
                toggle(
                    keys::DISCARD_ON_ESCAPE,
                    "Escape discards immediately",
                    "Press Escape and the recording is gone at once, with no countdown to change your mind.",
                    SettingSection::Recording,
                    false,
                ),
                number(
                    keys::CANCEL_COUNTDOWN_MS,
                    "Cancel countdown",
                    "How long Escape waits before discarding a recording. Press Escape again during the countdown to keep it.",
                    SettingSection::Recording,
                    (1000.0, 10_000.0, 500.0),
                    Some("ms"),
                    3000.0,
                ),
                dynamic_choice(
                    keys::INPUT_DEVICE,
                    "Input device",
                    "Which microphone to record from.",
                    SettingSection::Recording,
                    ChoiceSource::InputDevices,
                    "default",
                ),
                toggle(
                    keys::AUDIO_FEEDBACK,
                    "Sounds",
                    "A short sound when recording starts and when text is delivered.",
                    SettingSection::Recording,
                    true,
                ),
                advanced(number(
                    keys::FINALIZE_TIMEOUT_MS,
                    "Finalize timeout",
                    "If the last fragment has not decoded by this point, deliver what is ready rather than waiting.",
                    SettingSection::Recording,
                    (2000.0, 30_000.0, 500.0),
                    Some("ms"),
                    8000.0,
                )),
            ],
        },
        Capability {
            key: CapabilityKey::Models,
            name: text("Transcription"),
            description: text("The speech recognition model and the language it listens for."),
            requires: vec![],
            engine_needs: vec![],
            nav: None,
            hotkey: None,
            metrics: vec![],
            settings: vec![
                dynamic_choice(
                    keys::TRANSCRIPTION_MODEL,
                    "Model",
                    "Larger models are more accurate; smaller ones use less memory.",
                    SettingSection::Transcription,
                    ChoiceSource::Models,
                    "small-q5_1",
                ),
                SettingDef {
                    key: text(keys::LANGUAGE),
                    label: text("Language"),
                    description: text("Pin your language for a little more speed and accuracy, or let Murmur detect it."),
                    section: SettingSection::Transcription,
                    kind: SettingKind::DynamicChoice {
                        source: ChoiceSource::Languages,
                    },
                    default: SettingValue::Choice(text("auto")),
                    requires_engine: vec![EngineFeature::LanguageAutoDetect],
                    requires_permission: vec![],
                    advanced: false,
                },
            ],
        },
        Capability {
            key: CapabilityKey::Settings,
            name: text("Output"),
            description: text("What happens to your words once they are transcribed."),
            // Accessibility is NOT required: without it delivery degrades to
            // clipboard-only, which is a supported outcome rather than a failure.
            requires: vec![],
            engine_needs: vec![],
            nav: Some(NavDef {
                label: text("Settings"),
                route: text("settings"),
                icon: text("Settings"),
                order: 30,
            }),
            hotkey: None,
            metrics: vec![
                metric(LatencyStage::ClipboardWrite, "Clipboard", false),
                metric(LatencyStage::Inject, "Paste", false),
            ],
            settings: vec![
                needs_permission(
                    toggle(
                        keys::AUTO_PASTE,
                        "Paste automatically",
                        "Paste into whatever had focus. Turn this off to only copy to the clipboard.",
                        SettingSection::Output,
                        true,
                    ),
                    OsPermission::Accessibility,
                ),
                needs_permission(
                    toggle(
                        keys::RESTORE_CLIPBOARD,
                        "Restore my clipboard",
                        "Put back what was on the clipboard after pasting. Turn this off if pastes arrive empty in a particular app.",
                        SettingSection::Output,
                        true,
                    ),
                    OsPermission::Accessibility,
                ),
                advanced(number(
                    keys::PASTE_DELAY_MS,
                    "Paste delay",
                    "How long to wait after writing the clipboard before pasting. Raise it if an app pastes stale text.",
                    SettingSection::Output,
                    (0.0, 500.0, 5.0),
                    Some("ms"),
                    40.0,
                )),
                advanced(number(
                    keys::CLIPBOARD_RESTORE_DELAY_MS,
                    "Clipboard restore delay",
                    "How long to wait after pasting before restoring the clipboard.",
                    SettingSection::Output,
                    (0.0, 1000.0, 10.0),
                    Some("ms"),
                    150.0,
                )),
                toggle(
                    keys::CAPITALISE_SENTENCES,
                    "Capitalise sentences",
                    "Start each sentence with a capital letter.",
                    SettingSection::Output,
                    true,
                ),
                toggle(
                    keys::NORMALISE_PUNCTUATION,
                    "Tidy punctuation",
                    "Normalise spacing, quotes and terminal punctuation.",
                    SettingSection::Output,
                    true,
                ),
                toggle(
                    keys::STRIP_FILLERS,
                    "Remove filler words",
                    "Drop “um”, “uh”, “you know” and similar so speech reads like writing.",
                    SettingSection::Output,
                    false,
                ),
                toggle(
                    keys::SPOKEN_COMMANDS,
                    "Spoken formatting",
                    "Turn “new line”, “new paragraph”, “comma” and “period” into the thing you said.",
                    SettingSection::Output,
                    true,
                ),
                toggle(
                    keys::APPLY_CORRECTIONS,
                    "Apply spoken corrections",
                    "When you correct yourself out loud — \u{201c}Tuesday, sorry, I meant Wednesday\u{201d} — keep only the correction. It matches spoken phrases like \u{201c}I meant\u{201d} and \u{201c}make that\u{201d}; it does not rewrite your wording.",
                    SettingSection::Output,
                    false,
                ),
                toggle(
                    keys::LAUNCH_AT_LOGIN,
                    "Launch at login",
                    "Start Murmur automatically when you log in.",
                    SettingSection::General,
                    false,
                ),
                number(
                    keys::BASELINE_WPM,
                    "Your typing speed",
                    "Used to work out how much time dictation has saved you.",
                    SettingSection::General,
                    (10.0, 200.0, 1.0),
                    Some("wpm"),
                    40.0,
                ),
                toggle(
                    keys::CHECK_UPDATES,
                    "Check for updates",
                    "Check for a new version on launch and once a day. This is the only network request Murmur makes after setup.",
                    SettingSection::General,
                    true,
                ),
            ],
        },
        /*
         * SOURCE OF TRUTH KEYWORDS: Billing, free_forever
         * WHAT:  A Billing page that exists to say there is nothing to pay for.
         * WHY:   The operator asked for it as a joke, and it is a good one
         *        precisely because a billing entry is the last thing a user of a
         *        local-first tool expects to find. It also answers a real
         *        question — people DO look for the pricing page — so the joke
         *        and the honest answer are the same page.
         *
         *        Declares no settings and no metrics, which is the point: it is
         *        a nav entry and nothing else. That is legal here because the
         *        reachability test walks settings and metrics, and a capability
         *        with neither has nothing to be unreachable. Do NOT invent a
         *        setting to make it look more like its neighbours.
         * WHERE: Rendered from the registry by the dashboard shell; the page
         *        itself is a frontend component on the `billing` route.
         */
        Capability {
            key: CapabilityKey::Billing,
            name: text("Billing"),
            description: text("What Murmur costs, which is nothing."),
            requires: vec![],
            engine_needs: vec![],
            nav: Some(NavDef {
                label: text("Billing"),
                // Last in the sidebar. It is a punchline, not a feature.
                route: text("billing"),
                icon: text("CreditCard"),
                order: 40,
            }),
            hotkey: None,
            metrics: vec![],
            settings: vec![],
        },
        Capability {
            key: CapabilityKey::History,
            name: text("History"),
            description: text("Everything you have dictated, searchable, on this machine only."),
            requires: vec![],
            engine_needs: vec![],
            nav: Some(NavDef {
                label: text("History"),
                route: text("history"),
                icon: text("Clock"),
                order: 20,
            }),
            hotkey: None,
            metrics: vec![],
            settings: vec![
                number(
                    keys::RETENTION_DAYS,
                    "Keep history for",
                    "Older transcripts are deleted automatically. Set to 0 to keep everything forever.",
                    SettingSection::Privacy,
                    (0.0, 365.0, 1.0),
                    Some("days"),
                    0.0,
                ),
            ],
        },
        Capability {
            key: CapabilityKey::Stats,
            name: text("Stats"),
            description: text("How much you have dictated, and how fast Murmur actually is."),
            requires: vec![],
            engine_needs: vec![],
            nav: Some(NavDef {
                label: text("Stats"),
                route: text("stats"),
                icon: text("ChartNoAxesColumn"),
                order: 10,
            }),
            hotkey: None,
            metrics: vec![],
            settings: vec![],
        },
        Capability {
            key: CapabilityKey::Dictionary,
            name: text("Dictionary"),
            description: text("Names and jargon the model keeps getting wrong, and what they should be."),
            requires: vec![],
            // Prompting the engine with these terms is what fixes recognition
            // rather than patching the output, so the feature declares it.
            engine_needs: vec![EngineFeature::InitialPrompt],
            nav: None,
            hotkey: None,
            metrics: vec![],
            settings: vec![],
        },
        Capability {
            key: CapabilityKey::Onboarding,
            name: text("Setup"),
            description: text("First-run permissions, model download, and a hotkey test."),
            requires: vec![],
            engine_needs: vec![],
            nav: None,
            hotkey: None,
            metrics: vec![],
            settings: vec![toggle(
                keys::ONBOARDING_COMPLETE,
                "Setup complete",
                "Whether first-run setup has been finished.",
                SettingSection::General,
                false,
            )],
        },
        Capability {
            key: CapabilityKey::Updates,
            name: text("Updates"),
            description: text("Signed updates from GitHub Releases."),
            requires: vec![],
            engine_needs: vec![],
            nav: None,
            hotkey: None,
            metrics: vec![],
            settings: vec![],
        },
    ]
}

// ── Views onto the table ─────────────────────────────────────────────────

pub fn capabilities() -> &'static [Capability] {
    &CAPABILITIES
}

pub fn capability(key: CapabilityKey) -> Option<&'static Capability> {
    CAPABILITIES.iter().find(|c| c.key == key)
}

/// Every setting in the app, flattened across capabilities.
pub fn all_settings() -> impl Iterator<Item = &'static SettingDef> {
    CAPABILITIES.iter().flat_map(|c| c.settings.iter())
}

pub fn setting_def(key: &str) -> Option<&'static SettingDef> {
    all_settings().find(|s| s.key == key)
}

/// The defaults every fresh install starts from, and the fallback for any
/// setting that has never been written.
pub fn default_settings() -> HashMap<String, SettingValue> {
    all_settings()
        .map(|def| (def.key.clone(), def.default.clone()))
        .collect()
}

/// Sidebar entries, in declared order.
pub fn nav_items() -> Vec<&'static NavDef> {
    let mut items: Vec<&NavDef> = CAPABILITIES.iter().filter_map(|c| c.nav.as_ref()).collect();
    items.sort_by_key(|n| n.order);
    items
}

pub fn hotkey_defs() -> Vec<&'static HotkeyDef> {
    CAPABILITIES.iter().filter_map(|c| c.hotkey.as_ref()).collect()
}

/**
 * SOURCE OF TRUTH KEYWORDS: RegistrySnapshot
 * WHAT:  The whole table as one serialisable value for the frontend.
 * WHY:   The frontend reads the SAME declarations the backend enforces, rather
 *        than a mirror someone has to remember to update. That is what makes a
 *        generated settings page trustworthy.
 * WHERE: Returned by the registry IPC command; consumed by the Settings view
 *        and the dashboard shell.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RegistrySnapshot {
    pub capabilities: Vec<Capability>,
}

pub fn snapshot() -> RegistrySnapshot {
    RegistrySnapshot {
        capabilities: CAPABILITIES.clone(),
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: validate_registry
 * WHAT:  Self-check over the table: unique setting keys, unique capability
 *        keys, and every default matching its declared control kind.
 * WHY:   These are the mistakes that are invisible at compile time and silent
 *        at runtime — a duplicated key means one setting quietly shadows
 *        another, and a default of the wrong type means a control renders
 *        empty. Running this at startup turns both into a loud failure during
 *        development instead of a support ticket.
 * WHERE: Called once from lib.rs during setup, and asserted in tests.
 */
pub fn validate_registry() -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let mut seen_settings: HashMap<&str, usize> = HashMap::new();
    let mut seen_capabilities: HashMap<CapabilityKey, usize> = HashMap::new();

    for capability in CAPABILITIES.iter() {
        *seen_capabilities.entry(capability.key).or_insert(0) += 1;

        for setting in &capability.settings {
            *seen_settings.entry(setting.key.as_str()).or_insert(0) += 1;

            if !setting.default.matches_kind(&setting.kind) {
                problems.push(format!(
                    "setting `{}` has a default that does not match its control kind",
                    setting.key
                ));
            }
        }
    }

    for (key, count) in seen_settings {
        if count > 1 {
            problems.push(format!("setting key `{key}` is declared {count} times"));
        }
    }
    for (key, count) in seen_capabilities {
        if count > 1 {
            problems.push(format!(
                "capability `{}` is declared {count} times",
                key.as_str()
            ));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_internally_consistent() {
        if let Err(problems) = validate_registry() {
            panic!("registry is invalid:\n  {}", problems.join("\n  "));
        }
    }

    #[test]
    fn every_declared_key_constant_resolves_to_a_setting() {
        // Guards against a constant being renamed without its declaration.
        for key in [
            keys::DICTATION_HOTKEY,
            keys::DICTATION_MODE,
            keys::CAPTURE_MODE,
            keys::CANCEL_COUNTDOWN_MS,
            keys::DISCARD_ON_ESCAPE,
            keys::INPUT_DEVICE,
            keys::AUDIO_FEEDBACK,
            keys::TRANSCRIPTION_MODEL,
            keys::LANGUAGE,
            keys::FINALIZE_TIMEOUT_MS,
            keys::AUTO_PASTE,
            keys::RESTORE_CLIPBOARD,
            keys::PASTE_DELAY_MS,
            keys::CLIPBOARD_RESTORE_DELAY_MS,
            keys::CAPITALISE_SENTENCES,
            keys::NORMALISE_PUNCTUATION,
            keys::STRIP_FILLERS,
            keys::SPOKEN_COMMANDS,
            keys::APPLY_CORRECTIONS,
            keys::RETENTION_DAYS,
            keys::LAUNCH_AT_LOGIN,
            keys::BASELINE_WPM,
            keys::CHECK_UPDATES,
            keys::ONBOARDING_COMPLETE,
        ] {
            assert!(
                setting_def(key).is_some(),
                "setting constant `{key}` has no declaration in the registry"
            );
        }
    }

    #[test]
    fn nav_items_are_ordered() {
        let orders: Vec<u32> = nav_items().iter().map(|n| n.order).collect();
        let mut sorted = orders.clone();
        sorted.sort_unstable();
        assert_eq!(orders, sorted);
    }
}
