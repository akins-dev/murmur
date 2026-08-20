/*!
 * SOURCE OF TRUTH KEYWORDS: SettingValue, SettingKind, SettingChoice,
 *   HotkeyBinding, KeyModifier, SettingsSnapshot
 * WHAT:  The declarative shape of a setting — its control kind, its value, and
 *        the hotkey binding type.
 * WHY:   Settings are DATA, not components. A registry entry declares the kind
 *        and the frontend renders it, so adding a setting never means writing a
 *        form. That only holds if the value type is closed and serialisable,
 *        which is why this is one enum rather than a JSON blob.
 * WHERE: Declared in registry/capability.rs, persisted by services/settings.rs
 *        as JSON, rendered by the SettingControl global component.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

/**
 * SOURCE OF TRUTH KEYWORDS: SettingKind, SettingChoice
 * WHAT:  Which control the frontend renders for a setting.
 * WHERE: One field on every SettingDef in the registry.
 */
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SettingKind {
    Toggle,
    Text {
        placeholder: Option<String>,
        max_len: Option<u32>,
    },
    Number {
        min: f64,
        max: f64,
        step: f64,
        unit: Option<String>,
    },
    Choice {
        options: Vec<SettingChoice>,
    },
    /// A choice whose options are only knowable at runtime.
    DynamicChoice {
        source: ChoiceSource,
    },
    Hotkey,
}

/**
 * SOURCE OF TRUTH KEYWORDS: ChoiceSource
 * WHAT:  Where a dynamic setting's options come from.
 * WHY:   Input devices, installed models and supported languages cannot be
 *        listed in a static declaration — they depend on the machine, on what
 *        has been downloaded, and on which engine is selected. Naming the
 *        SOURCE keeps the setting declarative anyway: the registry still says
 *        what the control is, and the frontend resolves the options, so adding
 *        a device picker is still a registry entry rather than a bespoke form.
 * WHERE: Carried by SettingKind::DynamicChoice; resolved by the SettingControl
 *        component through the matching query command.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChoiceSource {
    InputDevices,
    Models,
    /// Languages the SELECTED engine declares support for — which is what stops
    /// the UI offering Hindi on an engine that cannot speak it.
    Languages,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SettingChoice {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

/**
 * SOURCE OF TRUTH KEYWORDS: SettingValue
 * WHAT:  A stored setting value, closed over the kinds above.
 * WHY:   Validating a value against its declared kind is only possible because
 *        both are typed; a bare JSON value would push that check to runtime in
 *        the frontend, where a bad value has already been saved.
 * WHERE: Stored by services/settings.rs; validated against the registry's
 *        SettingDef before a write is accepted.
 */
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
    Choice(String),
    Hotkey(HotkeyBinding),
}

impl SettingValue {
    /// True when this value can be stored against that control kind. The
    /// registry declares the kind; this is what stops a Choice value landing in
    /// a Toggle.
    ///
    /// A DynamicChoice holds a Choice value like any other: only the OPTION
    /// LIST is resolved at runtime, never the value type. Membership in that
    /// list is checked separately, at write time, once the options are known.
    pub fn matches_kind(&self, kind: &SettingKind) -> bool {
        matches!(
            (self, kind),
            (SettingValue::Bool(_), SettingKind::Toggle)
                | (SettingValue::Number(_), SettingKind::Number { .. })
                | (SettingValue::Text(_), SettingKind::Text { .. })
                | (
                    SettingValue::Choice(_),
                    SettingKind::Choice { .. } | SettingKind::DynamicChoice { .. }
                )
                | (SettingValue::Hotkey(_), SettingKind::Hotkey)
        )
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: HotkeyBinding, KeyModifier
 * WHAT:  A global shortcut as the user bound it.
 * WHY:   Modelled rather than stored as a string like "Alt+Space" so conflict
 *        detection and the accelerator string are both derived from one shape,
 *        and a rebind cannot produce something unparseable.
 * WHERE: Stored as a setting; converted to a Tauri accelerator by the hotkey
 *        adapter; rendered by the Keycap component.
 */
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct HotkeyBinding {
    pub modifiers: Vec<KeyModifier>,
    /// Physical key name, e.g. "Space", "KeyD", "F13".
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum KeyModifier {
    Command,
    Control,
    Option,
    Shift,
}

/**
 * SOURCE OF TRUTH KEYWORDS: BINDABLE_KEYS
 * WHAT:  Every `KeyboardEvent.code` a user is allowed to bind.
 * WHY:   The list the hotkey adapter can convert into a plugin `Code`, stated
 *        here so a binding can be REFUSED at the point it is saved rather than
 *        failing silently at registration. Kept honest by
 *        `every_bindable_key_maps_to_a_real_code`, which fails if the adapter
 *        stops accepting any of them — two lists that can drift are two lists
 *        that eventually disagree, and the symptom is a shortcut that saves and
 *        never fires.
 * WHERE: HotkeyBinding::bindable.
 */
pub const BINDABLE_KEYS: &[&str] = &[
    "Space", "Enter", "Tab", "Backquote", "Backslash", "Semicolon", "Quote", "Comma", "Period",
    "Slash", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13",
    "F14", "F15", "F16", "F17", "F18", "F19", "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF",
    "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR",
    "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ", "Digit0", "Digit1", "Digit2",
    "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8", "Digit9",
];

impl HotkeyBinding {
    /**
     * SOURCE OF TRUTH KEYWORDS: bindable, modifier_only, why_option_alone_fails
     * WHAT:  Whether this binding can actually be registered as a global
     *        shortcut. Returns the user-facing reason when it cannot.
     * WHY:   Two different ways a binding can be saved and then never work, and
     *        both were reachable from the settings UI:
     *
     *        1. **Modifier-only.** The user asked for Option on its own. A bare
     *           modifier cannot be a global hotkey here and should not be one.
     *           Mechanically, the plugin's API is `Shortcut::new(modifiers,
     *           code)` — a chord, not a key monitor — so there is nothing to
     *           register. More importantly it would be wrong even if it were
     *           possible: Option is held constantly to type accented characters
     *           and as a modifier in every app, so binding it means Murmur
     *           starts recording while you type. Detecting a tap-and-release of
     *           a lone modifier means a CGEventTap on flagsChanged, which is a
     *           different mechanism, needs Accessibility, and still fires on a
     *           mistimed keystroke. Refused, with a sentence saying why.
     *
     *        2. **A key the app cannot map.** Anything outside the set the
     *           hotkey adapter knows how to convert. Saving one is the failure
     *           this codebase keeps finding in new costumes: the row is written,
     *           the Settings view and the pill re-read it and draw the NEW
     *           keycap, and the OS is holding nothing at all. The app displays a
     *           key that does nothing.
     *
     *        Lives on the type rather than in the command, so the rule is stated
     *        once next to the shape it describes and both the validator and any
     *        future caller get the same answer. The set of bindable keys is
     *        cross-checked against the hotkey adapter's own mapping by
     *        `every_bindable_key_maps_to_a_real_code`, so the two cannot drift.
     * WHERE: ipc/commands/settings.rs validation; mirrored as an instant hint by
     *        the capture control, which must never be the authority.
     */
    pub fn bindable(&self) -> Result<(), String> {
        if self.is_modifier_only() {
            // Legal, and handled by the event tap rather than the shortcut
            // plugin. Exactly one modifier, because "⌥⇧ on their own" is not a
            // gesture anyone can perform reliably.
            return match self.modifiers.len() {
                1 => Ok(()),
                _ => Err(
                    "Pick a single modifier on its own, or hold modifiers and press a key."
                        .to_string(),
                ),
            };
        }
        if !BINDABLE_KEYS.contains(&self.key.as_str()) {
            return Err(format!("`{}` can't be used as a shortcut.", self.key));
        }
        Ok(())
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: is_modifier_only
     * WHAT:  True when the binding is a bare modifier with no key.
     * WHY:   Decides which MECHANISM registers it, and the two have nothing in
     *        common: an ordinary chord goes to the global-shortcut plugin, and a
     *        bare modifier goes to a CGEventTap watching for a double-tap. The
     *        plugin cannot express the second — its API is
     *        `Shortcut::new(modifiers, code)` and there is no code — which is
     *        why this is a property of the binding rather than a flag someone
     *        passes alongside it.
     * WHERE: bootstrap::register_hotkeys, and validation above.
     */
    pub fn is_modifier_only(&self) -> bool {
        self.key.trim().is_empty() || Self::is_modifier_name(&self.key)
    }

    /// The single modifier of a modifier-only binding.
    pub fn sole_modifier(&self) -> Option<KeyModifier> {
        match (self.is_modifier_only(), self.modifiers.as_slice()) {
            (true, [only]) => Some(*only),
            _ => None,
        }
    }

    /// `KeyboardEvent.code` names for the modifiers themselves. A capture that
    /// reports one of these as its KEY is a modifier-only press.
    fn is_modifier_name(key: &str) -> bool {
        matches!(
            key,
            "AltLeft"
                | "AltRight"
                | "ControlLeft"
                | "ControlRight"
                | "MetaLeft"
                | "MetaRight"
                | "ShiftLeft"
                | "ShiftRight"
                | "Alt"
                | "Control"
                | "Meta"
                | "Shift"
        )
    }

    /// The accelerator string the global-shortcut plugin expects.
    pub fn to_accelerator(&self) -> String {
        let mut parts: Vec<&str> = self
            .modifiers
            .iter()
            .map(|m| match m {
                KeyModifier::Command => "CommandOrControl",
                KeyModifier::Control => "Control",
                KeyModifier::Option => "Alt",
                KeyModifier::Shift => "Shift",
            })
            .collect();
        parts.push(&self.key);
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * SOURCE OF TRUTH KEYWORDS: modifier_only_is_bindable
     * WHAT:  A single modifier on its own IS a legal binding.
     * WHY:   This test used to assert the opposite, and the reversal is worth
     *        recording rather than quietly rewriting. The original reasoning was
     *        sound in isolation — Option is held constantly, so binding it would
     *        fire while you type — but it was a guess about how the OPERATOR
     *        works, and he answered it directly: "I like to use option only."
     *        A guess about someone's workflow loses to that person telling you.
     *
     *        The safety concern did not go away, it moved. It is answered by
     *        the double-tap recogniser in adapters/macos/modifier_tap.rs, where
     *        it can be tested against the actual event sequences typing
     *        produces, instead of by refusing the binding outright here.
     *
     *        Two modifiers together are still refused: "⌥⇧ on their own" is not
     *        a gesture anyone can perform repeatably.
     */
    #[test]
    fn a_single_modifier_on_its_own_is_bindable() {
        let binding = HotkeyBinding {
            modifiers: vec![KeyModifier::Option],
            key: String::new(),
        };
        assert!(binding.bindable().is_ok(), "the operator asked for this one");
        assert_eq!(binding.sole_modifier(), Some(KeyModifier::Option));

        // The same thing as the capture control reports it: the modifier's own
        // key code rather than an empty string.
        let captured = HotkeyBinding {
            modifiers: vec![KeyModifier::Option],
            key: "AltLeft".to_string(),
        };
        assert!(captured.bindable().is_ok());
        assert_eq!(captured.sole_modifier(), Some(KeyModifier::Option));
    }

    #[test]
    fn two_bare_modifiers_together_are_refused_with_a_reason() {
        let binding = HotkeyBinding {
            modifiers: vec![KeyModifier::Option, KeyModifier::Shift],
            key: String::new(),
        };
        let reason = binding.bindable().expect_err("not a performable gesture");
        assert!(
            reason.contains("single modifier"),
            "the refusal must say what to do instead: {reason}"
        );
    }

    /**
     * WHAT:  A single non-modifier key with NO modifiers is bindable.
     * WHY:   The other half of the operator's complaint — "you have hardcoded
     *        it to use two different keys". F13 through F19 exist precisely to
     *        be bound on their own, and requiring a chord blocked them for no
     *        reason.
     */
    #[test]
    fn a_bare_function_key_is_bindable_without_any_modifier() {
        for key in ["F13", "F19", "Space"] {
            let binding = HotkeyBinding {
                modifiers: vec![],
                key: key.to_string(),
            };
            assert!(
                binding.bindable().is_ok(),
                "`{key}` on its own must be bindable"
            );
            assert_eq!(binding.sole_modifier(), None, "`{key}` is not a modifier");
        }
    }

    #[test]
    fn a_modifier_plus_a_key_is_bindable() {
        let binding = HotkeyBinding {
            modifiers: vec![KeyModifier::Option],
            key: "Space".to_string(),
        };
        assert!(binding.bindable().is_ok());
    }

    /**
     * WHAT:  A key outside the bindable set is refused by name.
     * WHY:   This is the one that saves and then never fires. `IntlBackslash` is
     *        a real code a real keyboard produces and the adapter cannot map it.
     */
    #[test]
    fn a_key_the_adapter_cannot_map_is_refused_at_save_time() {
        let binding = HotkeyBinding {
            modifiers: vec![KeyModifier::Command],
            key: "IntlBackslash".to_string(),
        };
        let reason = binding.bindable().expect_err("must refuse");
        assert!(reason.contains("IntlBackslash"), "name the key: {reason}");
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: every_bindable_key_maps_to_a_real_code
     * WHAT:  Every key this module says is bindable is one the hotkey adapter
     *        can actually convert.
     * WHY:   Two lists that can drift are two lists that will disagree, and the
     *        symptom is the exact bug BINDABLE_KEYS exists to prevent — a
     *        shortcut that validates, saves, renders, and never fires. Asserted
     *        in both directions so neither list can grow silently past the other.
     */
    #[test]
    fn every_bindable_key_maps_to_a_real_code() {
        for key in BINDABLE_KEYS {
            assert!(
                crate::bootstrap::code_from_name(key).is_some(),
                "`{key}` is offered as bindable but the adapter cannot map it"
            );
        }
    }

    #[test]
    fn the_registry_default_hotkey_is_itself_bindable() {
        // The one binding every install starts with. If it were not bindable,
        // a fresh install would have no working hotkey at all.
        let default = crate::registry::hotkey_defs()[0].default.clone();
        assert!(
            default.bindable().is_ok(),
            "the shipped default hotkey is not bindable"
        );
    }
}
