/*!
 * SOURCE OF TRUTH KEYWORDS: Capability, CapabilityKey, SettingDef, NavDef,
 *   HotkeyDef, MetricDef, SettingSection, capability_key_str
 * WHAT:  The shape of one registry entry — everything the app knows about a
 *        feature, in one struct.
 * WHY:   This is the type that makes "adding a feature is an entry, not a
 *        pattern" true. One entry declares the settings the feature owns, the
 *        OS permissions it needs, the engine abilities it depends on, where it
 *        sits in the dashboard, which hotkey it binds and which metrics it
 *        emits — and the settings UI, the factory preflight, the nav, the
 *        conflict checker and the stats queries all read it rather than each
 *        keeping their own list. Six lists that must agree is how a codebase
 *        develops features that are half-wired.
 * WHERE: Instantiated in registry/mod.rs; consumed by ipc/factory.rs,
 *        services/settings.rs, and mirrored to TypeScript by specta.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::ports::permissions::OsPermission;
use crate::types::{
    settings::SettingKind, EngineFeature, HotkeyBinding, LatencyStage, SettingValue,
};

/**
 * SOURCE OF TRUTH KEYWORDS: CapabilityKey
 * WHAT:  The closed set of features the app has.
 * WHY:   An enum rather than a string, so a typo is a build error and so a new
 *        feature forces every exhaustive match to be revisited deliberately.
 * WHERE: The identity of every registry entry; carried on every IPC command so
 *        the factory knows which entry to preflight against.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityKey {
    /// The core loop: hotkey, record, transcribe, deliver.
    Dictation,
    History,
    Stats,
    Dictionary,
    Models,
    Settings,
    Onboarding,
    Updates,
    /// A page that exists to say there is nothing to pay for.
    Billing,
}

impl CapabilityKey {
    /// Stable string form, used in tracing spans and metric rows.
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityKey::Dictation => "dictation",
            CapabilityKey::History => "history",
            CapabilityKey::Stats => "stats",
            CapabilityKey::Dictionary => "dictionary",
            CapabilityKey::Models => "models",
            CapabilityKey::Billing => "billing",
            CapabilityKey::Settings => "settings",
            CapabilityKey::Onboarding => "onboarding",
            CapabilityKey::Updates => "updates",
        }
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: SettingSection
 * WHAT:  Which group of the Settings page a setting renders in.
 * WHY:   Grouping is presentation, but it belongs to the declaration — a
 *        setting that does not say where it goes forces the Settings view to
 *        keep its own ordering list, which is the exact duplication the
 *        registry exists to remove.
 * WHERE: On every SettingDef; used by the Settings view to build its sections.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SettingSection {
    Recording,
    Transcription,
    Output,
    Vocabulary,
    Privacy,
    General,
}

/**
 * SOURCE OF TRUTH KEYWORDS: SettingDef
 * WHAT:  One setting, fully described: how it renders, what it defaults to,
 *        and what it requires to be usable.
 * WHY:   `default` is typed as SettingValue and validated against `kind` at
 *        startup, so a mismatch is caught the first time the app runs rather
 *        than the first time a user opens that pane. `requires_engine` is what
 *        lets Settings explain that a language is unavailable on the selected
 *        engine instead of offering a control that silently fails at record
 *        time.
 * WHERE: Declared in registry/mod.rs; rendered by the SettingControl component;
 *        enforced on write by services/settings.rs.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SettingDef {
    /// Globally unique, dotted, stable. This is the database key — renaming one
    /// silently resets that setting for every existing user.
    pub key: String,
    pub label: String,
    pub description: String,
    pub section: SettingSection,
    pub kind: SettingKind,
    pub default: SettingValue,
    /// Engine abilities without which this setting cannot do anything.
    pub requires_engine: Vec<EngineFeature>,
    /**
     * OS grants without which this setting cannot do anything, whatever its
     * value. A toggle that is ON while the permission behind it is missing is
     * a control that lies: the user has said yes, the app agrees, and nothing
     * happens. Declaring the dependency lets the UI say WHY instead — which is
     * the same rule as `requires_engine`, applied to permissions.
     */
    pub requires_permission: Vec<OsPermission>,
    /// Hidden behind a disclosure. For settings that exist to unstick a
    /// specific machine, not ones people are expected to browse.
    pub advanced: bool,
}

/**
 * SOURCE OF TRUTH KEYWORDS: NavDef
 * WHAT:  A capability's placement in the dashboard sidebar.
 * WHERE: Present means the capability appears in the nav; absent means it does
 *        not. Read by the dashboard shell.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NavDef {
    pub label: String,
    /// Route segment, e.g. "history". Matched by the dashboard router.
    pub route: String,
    /// Lucide icon name. Resolved by the frontend.
    pub icon: String,
    pub order: u32,
}

/**
 * SOURCE OF TRUTH KEYWORDS: HotkeyDef
 * WHAT:  A global shortcut a capability binds, and its default.
 * WHY:   Declared here so conflict detection has one list to check against
 *        rather than discovering a clash at registration time, when the only
 *        available response is a failure the user cannot interpret.
 * WHERE: Read at startup by the hotkey adapter and by the rebind UI.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HotkeyDef {
    /// Stable id, e.g. "dictation.toggle".
    pub id: String,
    pub label: String,
    pub default: HotkeyBinding,
    /// The setting key holding the user's override, if it is rebindable.
    pub setting_key: Option<String>,
}

/**
 * SOURCE OF TRUTH KEYWORDS: MetricDef
 * WHAT:  A latency stage a capability is expected to emit.
 * WHY:   Declaring metrics closes the loop in both directions: the dashboard
 *        cannot read a metric nothing writes, and nothing can write a metric
 *        that is never surfaced. Both of those are silent failures otherwise.
 * WHERE: Written by telemetry/latency.rs, read by services/stats.rs.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MetricDef {
    pub stage: LatencyStage,
    pub label: String,
    /// Shown on the dashboard latency panel rather than kept for diagnostics.
    pub user_facing: bool,
}

/**
 * SOURCE OF TRUTH KEYWORDS: Capability
 * WHAT:  One feature of the app, completely described.
 * WHERE: The elements of registry::CAPABILITIES.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Capability {
    pub key: CapabilityKey,
    pub name: String,
    pub description: String,
    pub settings: Vec<SettingDef>,
    /// OS grants required before any command on this capability may run. The
    /// command factory enforces this — handlers never check permissions.
    pub requires: Vec<OsPermission>,
    pub engine_needs: Vec<EngineFeature>,
    pub nav: Option<NavDef>,
    pub metrics: Vec<MetricDef>,
    pub hotkey: Option<HotkeyDef>,
}

impl Capability {
    pub fn setting(&self, key: &str) -> Option<&SettingDef> {
        self.settings.iter().find(|s| s.key == key)
    }
}
