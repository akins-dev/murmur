/*!
 * SOURCE OF TRUTH KEYWORDS: reachability, every_setting_is_consumed,
 *   every_metric_is_recorded, DECLARATION_SITES, KNOWN_UNREACHABLE,
 *   consumption_site
 * WHAT:  Asserts that every setting the registry declares is READ by something,
 *        and every metric it declares is RECORDED by something. Reads the
 *        crate's own source at test time and looks for a consumption site.
 * WHY:   Our other guardrails check STRUCTURE — that a key resolves, that the
 *        table has no duplicates, that a value round-trips through SQLite.
 *        None of them check REACHABILITY, and a registry entry can pass every
 *        one of them while being read by nobody: it generates a control, the
 *        control saves, the value comes back, and the app's behaviour never
 *        changes. The user has told us something and been agreed with.
 *
 *        That is not hypothetical. Two independent audits of this crate landed
 *        on the same shape and it accounted for every High finding between
 *        them: `privacy.retention_days` promising "older transcripts are
 *        deleted automatically" with `purge_older_than` correct and uncalled;
 *        `general.launch_at_login` writing a row nothing acts on; the two paste
 *        delays whose own doc comment claimed they were "built from settings by
 *        pipeline/deliver.rs", a file that does not exist; and four declared
 *        metrics — including TotalFinalize, the number the product promises —
 *        that nothing ever recorded.
 *
 *        A grep-the-source test is crude, and crude is the point: it fails for
 *        a reason anyone can check by hand in ten seconds, and it cannot be
 *        satisfied by a mock. The alternative — trusting a reviewer to notice
 *        an absence — is what produced the list above.
 * WHERE: Compiled into the crate's tests by registry/mod.rs.
 */

use std::collections::HashMap;
use std::path::Path;

use crate::registry::CAPABILITIES;

/// The crate's own `src`, resolved at compile time so the test does not depend
/// on the working directory `cargo test` happens to be run from.
const SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");

/**
 * SOURCE OF TRUTH KEYWORDS: DECLARATION_SITES
 * WHAT:  Paths that mention a key or a stage in order to DECLARE it, not to act
 *        on it.
 * WHY:   This list is what makes the test mean anything. `settings_view.rs`
 *        names every setting it loads, so counting it as a consumer would make
 *        the test pass for a setting that is loaded into a struct field nobody
 *        ever reads — which is precisely the bug being hunted. Same argument
 *        for the registry itself and for the STAGE_ORDER table in
 *        services/metrics.rs, which lists all nine stages purely to fix their
 *        display order.
 *
 *        Loading is still recognised as consumption, but only transitively:
 *        see `settings_view_field`, which requires the FIELD to be read
 *        somewhere real.
 */
const DECLARATION_SITES: &[&str] = &[
    // The table itself.
    "registry/",
    // The loader: turns keys into SessionSettings fields. Not a consumer.
    "session/settings_view.rs",
    // STAGE_ORDER — a display-ordering table naming every stage.
    "services/metrics.rs",
    /*
     * `LatencyStage::as_str` matches on EVERY variant by construction, so
     * without this exclusion the metric half of this file could never fail —
     * every stage is "referenced" by the enum's own serialiser whether or not a
     * single line of code records it.
     *
     * Found by deliberately breaking the recording of TotalFinalize and
     * watching the test stay green. Worth stating plainly: this exclusion is
     * the difference between a guardrail and a decoration, and the same trap
     * waits for any future exhaustive match over a declared type.
     */
    "types/metrics.rs",
];

/// Whole files that exist only for tests. A setting referenced solely by a test
/// is not reachable in the shipped app.
const TEST_ONLY_FILES: &[&str] = &["session/e2e_tests.rs", "testing.rs"];

/**
 * SOURCE OF TRUTH KEYWORDS: KNOWN_UNREACHABLE
 * WHAT:  Declared settings that are knowingly read by nothing, each with the
 *        reason it has not simply been wired up.
 * WHY:   Empty, and that is the intended resting state. It exists for decisions
 *        that are not an engineer's to make alone — a setting that cannot be
 *        wired without first answering a product question.
 *
 *        Its one entry so far was `privacy.store_audio`, and the resolution is
 *        the pattern to copy: the question ("should a privacy-first app store
 *        audio at all?") was answered NO, so the setting was deleted and this
 *        line with it. An allowlist entry is a question waiting to be answered,
 *        never a place to park a control that does nothing.
 *
 *        `the_allowlist_contains_nothing_that_is_actually_reachable` keeps it
 *        self-cleaning: wiring an allowlisted setting up fails the suite until
 *        its line here is removed.
 */
const KNOWN_UNREACHABLE: &[(&str, &str)] = &[];

/// One source file, with its test module removed.
struct Source {
    path: String,
    body: String,
}

/**
 * WHAT:  Every shipped `.rs` file under src/, minus declaration sites, minus
 *        test-only files, minus each file's `#[cfg(test)] mod tests` block.
 * WHY:   Test code referencing a setting proves the setting is testable, not
 *        that it does anything. Stripping the block is done by finding the
 *        `#[cfg(test)]` that introduces `mod tests` specifically — lib.rs gates
 *        `pub mod testing;` on `#[cfg(test)]` too, and truncating there would
 *        silently discard most of the crate root.
 */
fn shipped_sources() -> Vec<Source> {
    let mut out = Vec::new();
    collect(Path::new(SRC_DIR), Path::new(SRC_DIR), &mut out);
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<Source>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        if DECLARATION_SITES.iter().any(|site| relative.starts_with(site))
            || TEST_ONLY_FILES.contains(&relative.as_str())
        {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };

        out.push(Source {
            path: relative,
            body: strip_test_module(&text),
        });
    }
}

/// Truncates at the `#[cfg(test)]` that introduces a `mod tests` block, and
/// nowhere else. See `shipped_sources`.
fn strip_test_module(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let next = lines[index + 1..]
            .iter()
            .find(|candidate| !candidate.trim().is_empty());
        if next.is_some_and(|n| n.trim_start().starts_with("mod tests")) {
            return lines[..index].join("\n");
        }
    }
    text.to_string()
}

/// Maps `"dictation.hotkey"` to `"DICTATION_HOTKEY"` by reading keys.rs, so a
/// renamed constant is a test failure rather than a silent pass.
fn key_constants() -> HashMap<String, String> {
    let text = std::fs::read_to_string(format!("{SRC_DIR}/registry/keys.rs"))
        .expect("registry/keys.rs is readable");

    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some((ident, value)) = rest.split_once(": &str = ") else {
            continue;
        };
        let value = value.trim().trim_end_matches(';').trim_matches('"');
        map.insert(value.to_string(), ident.to_string());
    }
    map
}

/**
 * SOURCE OF TRUTH KEYWORDS: settings_view_field
 * WHAT:  The SessionSettings field a key is loaded into, if it is loaded at all.
 * WHY:   Most settings are not read by key outside the loader — they travel as
 *        struct fields (`self.settings.auto_paste`). Without this the test would
 *        report seventeen false failures and be turned off within a day.
 */
fn settings_view_field(constant: &str) -> Option<String> {
    let text = std::fs::read_to_string(format!("{SRC_DIR}/session/settings_view.rs")).ok()?;
    let needle = format!("keys::{constant}");

    for line in text.lines() {
        if !line.contains(&needle) {
            continue;
        }
        let trimmed = line.trim();
        // `field: read_bool(stored, keys::X)`
        if let Some((field, _)) = trimmed.split_once(':') {
            let field = field.trim();
            if is_ident(field) {
                return Some(field.to_string());
            }
        }
        // `let field = match read_choice(stored, keys::X)`
        if let Some(rest) = trimmed.strip_prefix("let ") {
            if let Some((field, _)) = rest.split_once('=') {
                let field = field.trim();
                if is_ident(field) {
                    return Some(field.to_string());
                }
            }
        }
    }
    None
}

fn is_ident(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/**
 * WHAT:  Where a SessionSettings field is actually read.
 * WHY:   Anchored on the receiver (`settings.`, `context.`, `request.`) rather
 *        than a bare field name, so a doc comment mentioning
 *        `finalize_timeout_ms` in prose does not count as a consumer. That
 *        exact false pass exists in pipeline/worker.rs.
 */
fn field_consumption_site(sources: &[Source], field: &str) -> Option<String> {
    let patterns = [
        format!("settings.{field}"),
        format!("context.{field}"),
        format!("request.{field}"),
    ];

    sources.iter().find_map(|source| {
        patterns
            .iter()
            .any(|p| source.body.contains(p))
            .then(|| source.path.clone())
    })
}

fn key_consumption_site(sources: &[Source], constant: &str) -> Option<String> {
    let needle = format!("keys::{constant}");
    sources
        .iter()
        .find(|source| source.body.contains(&needle))
        .map(|source| source.path.clone())
}

/// Resolves a setting to the file that acts on it, by either route.
fn consumption_site(sources: &[Source], key: &str, constants: &HashMap<String, String>) -> Option<String> {
    let constant = constants.get(key)?;

    if let Some(site) = key_consumption_site(sources, constant) {
        return Some(site);
    }

    let field = settings_view_field(constant)?;
    field_consumption_site(sources, &field)
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * SOURCE OF TRUTH KEYWORDS: every_setting_is_consumed
     * WHAT:  Every declared setting is read by shipped code.
     * WHY:   See the module WHY. This is the test that would have caught all
     *        four inert settings on the day each was added.
     */
    #[test]
    fn every_declared_setting_is_read_by_something() {
        let sources = shipped_sources();
        let constants = key_constants();
        assert!(
            !sources.is_empty(),
            "found no source files to scan — the test is not testing anything"
        );

        let allowed: Vec<&str> = KNOWN_UNREACHABLE.iter().map(|(key, _)| *key).collect();
        let mut inert = Vec::new();

        for capability in CAPABILITIES.iter() {
            for setting in &capability.settings {
                if allowed.contains(&setting.key.as_str()) {
                    continue;
                }
                assert!(
                    constants.contains_key(&setting.key),
                    "setting `{}` has no constant in registry/keys.rs",
                    setting.key
                );
                if consumption_site(&sources, &setting.key, &constants).is_none() {
                    inert.push(setting.key.clone());
                }
            }
        }

        assert!(
            inert.is_empty(),
            "these settings are declared, rendered and stored, and read by nothing:\n  {}\n\
             Each one is a control that agrees with the user and changes no behaviour. \
             Wire it to a consumer, delete it, or add it to KNOWN_UNREACHABLE with a reason.",
            inert.join("\n  ")
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: every_metric_is_recorded
     * WHAT:  Every declared metric stage is recorded by shipped code.
     * WHY:   A declared-but-unrecorded stage renders an empty row on the
     *        latency panel forever. TotalFinalize — "Stop to pasted", the
     *        headline number this product promises — was one of four.
     */
    #[test]
    fn every_declared_metric_is_recorded_by_something() {
        let sources = shipped_sources();
        let mut unrecorded = Vec::new();

        for capability in CAPABILITIES.iter() {
            for metric in &capability.metrics {
                let needle = format!("LatencyStage::{:?}", metric.stage);
                if !sources.iter().any(|source| source.body.contains(&needle)) {
                    unrecorded.push(format!("{:?} ({})", metric.stage, metric.label));
                }
            }
        }

        assert!(
            unrecorded.is_empty(),
            "these metrics are declared and never recorded:\n  {}\n\
             The dashboard has a row for each and nothing ever writes one.",
            unrecorded.join("\n  ")
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: the_allowlist_is_self_cleaning
     * WHAT:  Every KNOWN_UNREACHABLE entry is still genuinely unreachable.
     * WHY:   This is what stops the allowlist becoming a place to put things.
     *        Wiring a setting up makes this fail until its line is deleted, so
     *        the list cannot outlive the problem it documents.
     */
    #[test]
    fn the_allowlist_contains_nothing_that_is_actually_reachable() {
        let sources = shipped_sources();
        let constants = key_constants();

        for (key, reason) in KNOWN_UNREACHABLE {
            assert!(
                !reason.trim().is_empty(),
                "`{key}` is allowlisted without a reason"
            );
            assert!(
                super::super::setting_def(key).is_some(),
                "`{key}` is allowlisted but is not a declared setting — delete the entry"
            );
            if let Some(site) = consumption_site(&sources, key, &constants) {
                panic!(
                    "`{key}` is on KNOWN_UNREACHABLE but is now read by {site}. \
                     Delete its entry from the allowlist."
                );
            }
        }
    }

    /**
     * WHAT:  The test module stripper does not eat real code.
     * WHY:   lib.rs gates `pub mod testing;` behind `#[cfg(test)]` two thirds of
     *        the way up the file. A naive truncate-at-first-`#[cfg(test)]` would
     *        drop the entire module tree and make every lookup below it silently
     *        pass or fail for the wrong reason.
     */
    #[test]
    fn stripping_tests_keeps_code_that_follows_a_cfg_test_attribute() {
        let sample = "\
#[cfg(test)]
pub mod testing;
pub mod ports;

#[cfg(test)]
mod tests {
    fn helper() {}
}";
        let stripped = strip_test_module(sample);
        assert!(stripped.contains("pub mod ports;"), "real code was discarded");
        assert!(!stripped.contains("fn helper"), "the test module survived");
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: the_scan_can_fail
     * WHAT:  The scanner reports None for something genuinely absent and Some
     *        for something genuinely present.
     * WHY:   The assertions above are all of the form "nothing was found", and
     *        a broken scanner satisfies every one of them by finding nothing
     *        ever. This crate has no history to diff against, so the only way to
     *        show the guardrail is falsifiable is to hand it a key that cannot
     *        be consumed and require it to say so.
     *
     *        Paired deliberately with a positive case. Either half alone can be
     *        passed by a stub: always-None passes the negative, always-Some
     *        passes the positive, and nothing passes both.
     */
    #[test]
    fn the_scan_reports_absence_and_presence_differently() {
        let sources = shipped_sources();

        assert!(
            key_consumption_site(&sources, "A_SETTING_NOBODY_HAS_EVER_DECLARED").is_none(),
            "the scanner found a constant that does not exist — it is matching everything"
        );
        assert!(
            key_consumption_site(&sources, "CHECK_UPDATES").is_some(),
            "the scanner missed CHECK_UPDATES, which ipc/commands/updates.rs reads by key — \
             it is matching nothing, and every assertion in this file is passing vacuously"
        );

        assert!(
            field_consumption_site(&sources, "a_field_that_does_not_exist").is_none(),
            "the field scanner is matching everything"
        );
        assert!(
            field_consumption_site(&sources, "auto_paste").is_some(),
            "the field scanner missed settings.auto_paste"
        );

        // And the metric half, which has no field route to fall back on.
        assert!(
            !sources
                .iter()
                .any(|s| s.body.contains("LatencyStage::NotARealStage")),
            "the metric scanner is matching everything"
        );
        assert!(
            sources
                .iter()
                .any(|s| s.body.contains("LatencyStage::TailDecode")),
            "the metric scanner cannot see the one stage that was always recorded"
        );
    }

    /**
     * WHAT:  The scan actually excludes the declaration sites.
     * WHY:   If the exclusion silently stopped matching — a moved file, a
     *        renamed directory — every assertion above would pass vacuously and
     *        report a clean bill of health for a registry full of dead entries.
     *        A guardrail that cannot fail is worse than no guardrail.
     */
    #[test]
    fn the_scan_excludes_the_declaration_sites() {
        let sources = shipped_sources();

        for site in DECLARATION_SITES {
            assert!(
                !sources.iter().any(|s| s.path.starts_with(site)),
                "declaration site `{site}` is being counted as a consumer"
            );
        }
        assert!(
            sources.iter().any(|s| s.path == "session/actor.rs"),
            "the scan is not reaching session/actor.rs, where most settings are consumed"
        );
    }
}
