# Pillar 1 Reference: Declarative Central Registry & Reachability

/**
 * SOURCE OF TRUTH KEYWORDS: RegistryPattern, CapabilityDefinition, SettingSchema,
 *   ReachabilityScanner, GhostFeatures, DynamicControlGeneration
 * WHAT:  Architecture and implementation guide for declarative capability
 *        registries and automated reachability tests that prevent dead settings.
 * WHY:   Prevents structural decay where features are declared or configured
 *        in UI but ignored by domain logic.
 * WHERE: Referenced by SKILL.md Pillar 1; implemented in registry/ modules.
 */

## 1. The Core Problem: Ghost Features & Configuration Rot

In traditional architectures, adding a configurable feature requires touching multiple disconnected locations:
1. Adding a database column or config key.
2. Writing a UI settings component with hardcoded inputs.
3. Adding permission checks in an endpoint.
4. Reading the value somewhere in business logic.

This distributed approach invariably leads to **Ghost Features**:
- A setting is declared in the settings UI and persists to the database.
- The user flips the toggle, sees "Saved", but **no code in the application actually reads or acts on the value**.
- Two independent codebase audits in production projects revealed that up to 30% of settings were unreferenced ghost controls (e.g. `retention_days` claiming old data is purged, while the purge function was never called).

---

## 2. The Declarative Registry Solution

Instead of scattering feature flags across the codebase, **every capability is declared as a single static schema definition** in `registry/mod.rs` (or `registry/capabilities.ts`).

### Capability Definition Schema (Rust / TypeScript)

```rust
pub struct Capability {
    /// Unique kebab-case identifier (e.g. "dictation.streaming")
    pub id: &'static str,
    /// Human-readable title
    pub title: &'static str,
    /// Operating system or user permissions required before execution
    pub permissions: &'static [PermissionKind],
    /// Configuration settings owned by this capability
    pub settings: &'static [SettingDef],
    /// Telemetry metrics recorded by this capability
    pub metrics: &'static [MetricDef],
    /// UI Navigation item (if this capability has a view)
    pub navigation: Option<NavDef>,
}

pub struct SettingDef {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub control: SettingControlKind, // Toggle, Slider { min, max, step }, Select { options }
    pub default_value: SettingValue,
}
```

### Static Table of Truth

```rust
pub static CAPABILITIES: &[Capability] = &[
    Capability {
        id: "transcription.vad",
        title: "Voice Activity Detection",
        permissions: &[PermissionKind::Microphone],
        settings: &[
            SettingDef {
                key: "vad.silence_threshold_ms",
                title: "Silence Threshold",
                description: "Milliseconds of silence before chunk boundary is cut.",
                control: SettingControlKind::Slider { min: 200, max: 2000, step: 50 },
                default_value: SettingValue::Int(500),
            },
        ],
        metrics: &[
            MetricDef { name: "vad_chunk_cut_total", kind: MetricKind::Counter },
        ],
        navigation: None,
    },
    // ...
];
```

---

## 3. Dynamic UI Control Generation

The frontend does **not** hand-craft individual setting rows or toggles. It queries the registry and renders controls dynamically using a unified control mapper:

```tsx
// SettingControl.tsx
export function SettingControl({ setting, value, onChange }: SettingControlProps) {
  switch (setting.control.type) {
    case 'toggle':
      return <Switch checked={Boolean(value)} onCheckedChange={onChange} />;
    case 'slider':
      return (
        <Slider
          value={[Number(value)]}
          min={setting.control.min}
          max={setting.control.max}
          step={setting.control.step}
          onValueChange={([val]) => onChange(val)}
        />
      );
    case 'select':
      return (
        <Select value={String(value)} onValueChange={onChange}>
          {setting.control.options.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>{opt.label}</SelectItem>
          ))}
        </Select>
      );
  }
}
```

### Benefits:
- **Zero UI Boilerplate**: Adding feature #40 requires adding a single row to `CAPABILITIES`. The settings UI, validation, and serialization are automatically rendered.
- **Visual Consistency**: All controls share identical margins, typography, tooltips, and keyboard accessibility patterns.

---

## 4. The Reachability Scanner: Automated Anti-Ghost Guardrail

How do we prove that every setting in `CAPABILITIES` is actually used?
We write an automated test in `registry/reachability.rs` (or run `scripts/verify-reachability.mjs`).

### How It Works:
1. **Extraction**: The test parses `CAPABILITIES` and extracts every setting key string (e.g. `"vad.silence_threshold_ms"`).
2. **Codebase Scan**: The test reads the entire source tree (`src/`) looking for occurrences of each key.
3. **Exclusions**:
   - **Declaration sites**: The registry table itself, display-ordering lists, and loader views that merely deserialize keys into intermediate structs are excluded.
   - **Test files**: References only found in test harnesses do not count as live consumption.
4. **Assertion**: If any setting key has 0 real consumption sites, **the build fails**:

```rust
#[test]
fn every_setting_is_consumed() {
    let unconsumed: Vec<&str> = CAPABILITIES
        .iter()
        .flat_map(|c| c.settings.iter().map(|s| s.key))
        .filter(|key| !is_consumed_in_source(key))
        .collect();

    assert!(
        unconsumed.is_empty(),
        "These settings are declared in the registry but NEVER consumed by domain logic:
  {}
         Implement the logic that acts on them or remove them from the registry.",
        unconsumed.join("
  ")
    );
}
```

### Self-Cleaning Allowlist
If a setting is legitimately in development or scheduled for release, it can be placed in `KNOWN_UNREACHABLE`:
- Every entry MUST provide an explicit reason.
- When someone implements the feature, the test fails until the entry is removed from `KNOWN_UNREACHABLE`, keeping the list strictly honest.
