/*!
 * SOURCE OF TRUTH KEYWORDS: layering, nothing_imports_upward, LAYER_ORDER,
 *   KNOWN_UPWARD_IMPORTS
 * WHAT:  Enforces the dependency direction docs/05 §5 describes: infrastructure,
 *        then contracts, then implementations, then domain, then the boundary.
 *        Nothing imports from a layer above its own.
 * WHY:   docs/05 §5 said "CI enforces this — an upward import fails the build
 *        rather than being caught in review." That sentence was not true. There
 *        was no CI at all, nothing checked the direction, and the crate already had one
 *        real violation in it. A guardrail claimed in a document and absent from
 *        the build is worse than an acknowledged gap, because everyone downstream
 *        reasons as though it holds.
 *
 *        So this is the check that makes the sentence true. It runs in
 *        `cargo test`, which CI runs on every push, so an upward import now
 *        fails the build exactly as the document claims.
 *
 *        Crude on purpose — it reads `use crate::x` lines rather than building a
 *        real module graph. That misses fully-qualified paths written inline,
 *        and it will keep missing them. It catches the shape violations
 *        actually take, it fails for a reason anyone can check in ten seconds,
 *        and it cannot be satisfied by a mock.
 * WHERE: Compiled into the crate's tests by lib.rs.
 */

use std::path::Path;

const SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");

/**
 * SOURCE OF TRUTH KEYWORDS: LAYER_ORDER
 * WHAT:  The layers, lowest first. A module may import its own layer and any
 *        layer below it, never one above.
 * WHY:   Mirrors the module order in lib.rs, which is itself the documented
 *        dependency direction. Kept as one list so the two cannot disagree:
 *        `every_module_in_the_crate_root_has_a_layer` asserts the reverse — that
 *        no declared module is missing from this table and therefore silently
 *        exempt from the check.
 */
const LAYER_ORDER: &[&[&str]] = &[
    // Infrastructure: knows about nothing above it.
    &["config", "db", "error", "telemetry", "types"],
    // Contracts.
    &["ports"],
    // Implementations of those contracts.
    &["adapters"],
    // The source of truth, which reads ports to describe what features need.
    &["registry"],
    // Domain.
    &["pipeline", "services", "session"],
    // Boundary: allowed to see everything.
    &["bootstrap", "ipc", "tray"],
];

/**
 * SOURCE OF TRUTH KEYWORDS: KNOWN_UPWARD_IMPORTS
 * WHAT:  Violations that exist today, each with the reason it has not been
 *        fixed yet.
 * WHY:   Recorded rather than hidden. The alternative — weakening the rule
 *        until the crate passes — would make the check green and meaningless,
 *        which is precisely the state docs/05 §5 was in.
 *
 *        Self-cleaning: `the_exception_list_is_still_accurate` fails the moment
 *        an entry stops being a violation, so fixing one REQUIRES deleting its
 *        line here. The list can only shrink by accident and grow on purpose.
 */
const KNOWN_UPWARD_IMPORTS: &[(&str, &str, &str)] = &[
    /*
     * All three are ONE problem wearing three hats: a type that both the domain
     * and the boundary need, which currently lives in the boundary. The fix is
     * a MOVE — the event structs and the context types go down to a layer both
     * sides can import — not three separate repairs, and it is worth doing as
     * one change rather than piecemeal.
     *
     * Not done today because it touches binding generation and the operator is
     * walking the app as an acceptance test on the next launch. Recorded so it
     * is visible and so no FOURTH one can appear unnoticed.
     */
    (
        "adapters/events.rs",
        "ipc",
        "TauriEventSink emits the typed event structs, which live in \
         ipc/events.rs because tauri_specta::Event generates the frontend \
         listeners from them.",
    ),
    (
        "session/actor.rs",
        "ipc",
        "The actor takes AppState and SessionContext, which live in \
         ipc/context.rs. The actor is domain code and should not need to see \
         the boundary to be handed its own dependencies.",
    ),
    (
        "session/delivery.rs",
        "ipc",
        "Same as session/actor.rs: the delivery worker is handed a \
         SessionContext from ipc/context.rs.",
    ),
];

fn layer_of(module: &str) -> Option<usize> {
    LAYER_ORDER
        .iter()
        .position(|layer| layer.contains(&module))
}

/// Every `use crate::<module>` in every shipped source file, as
/// (relative path, imported module).
fn crate_imports() -> Vec<(String, String)> {
    let mut out = Vec::new();
    collect(Path::new(SRC_DIR), Path::new(SRC_DIR), &mut out);
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
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

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Test code is exempt, for the same reason it is exempt from the
        // reachability scan: a test may reach anywhere it likes to build a
        // harness, and counting it would report harnesses as architecture.
        // Deliberately a copy of the stripper in registry/reachability.rs
        // rather than a shared helper — two cfg(test) modules sharing a private
        // utility module is more machinery than fifteen lines is worth.
        let text = strip_test_module(&text);

        for line in text.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("use crate::") else {
                continue;
            };
            let module: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !module.is_empty() && !TEST_ONLY_FILES.contains(&relative.as_str()) {
                out.push((relative.clone(), module));
            }
        }
    }
}

/// Truncates at the `#[cfg(test)]` that introduces a `mod tests` block, and
/// nowhere else — lib.rs gates real modules on `#[cfg(test)]` too.
fn strip_test_module(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let next = lines[index + 1..]
            .iter()
            .find(|candidate| !candidate.trim().is_empty());
        if next.is_some_and(|n| {
            let t = n.trim_start();
            t.starts_with("mod tests") || t.starts_with("mod ") && t.contains("_tests")
        }) {
            return lines[..index].join("\n");
        }
    }
    text.to_string()
}

/// Whole files that exist only for tests.
const TEST_ONLY_FILES: &[&str] = &["testing.rs", "session/e2e_tests.rs"];

/// The layer a source file belongs to, from its top-level directory or filename.
fn file_layer(relative: &str) -> Option<usize> {
    let head = relative.split('/').next()?;
    let module = head.strip_suffix(".rs").unwrap_or(head);
    layer_of(module)
}

/// Every import that points at a layer above its own file.
fn upward_imports() -> Vec<(String, String)> {
    crate_imports()
        .into_iter()
        .filter(|(file, module)| {
            match (file_layer(file), layer_of(module)) {
                (Some(from), Some(to)) => to > from,
                // lib.rs and main.rs belong to no layer and may see everything.
                _ => false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * SOURCE OF TRUTH KEYWORDS: nothing_imports_upward
     * WHAT:  The claim docs/05 §5 makes, enforced.
     * WHY:   An upward import is not a style preference — it is what turns a
     *        set of layers into a ball of mud, and it happens one reasonable
     *        edit at a time. Catching it in review requires someone to notice;
     *        catching it here requires nothing of anybody.
     */
    #[test]
    fn nothing_imports_upward() {
        let allowed: Vec<(&str, &str)> = KNOWN_UPWARD_IMPORTS
            .iter()
            .map(|(file, module, _)| (*file, *module))
            .collect();

        let violations: Vec<String> = upward_imports()
            .into_iter()
            .filter(|(file, module)| !allowed.contains(&(file.as_str(), module.as_str())))
            .map(|(file, module)| format!("{file} imports crate::{module}"))
            .collect();

        assert!(
            violations.is_empty(),
            "these imports point upward through the layers:\n  {}\n\
             The dependency direction is declared in lib.rs and described in \
             docs/05 §5. Move the shared thing DOWN to a layer both sides can \
             see, rather than reaching up for it.",
            violations.join("\n  ")
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: the_exception_list_is_still_accurate
     * WHAT:  Every recorded exception is still a real violation.
     * WHY:   Makes the list self-cleaning. Fixing a violation fails this until
     *        its line is deleted, so the list cannot outlive the problem — the
     *        same property that keeps the reachability allowlist honest.
     */
    #[test]
    fn the_exception_list_is_still_accurate() {
        let actual = upward_imports();
        for (file, module, reason) in KNOWN_UPWARD_IMPORTS {
            assert!(
                !reason.trim().is_empty(),
                "`{file}` is excepted without a reason"
            );
            assert!(
                actual
                    .iter()
                    .any(|(f, m)| f == file && m == module),
                "`{file}` no longer imports crate::{module} — delete its line from \
                 KNOWN_UPWARD_IMPORTS"
            );
        }
    }

    /**
     * WHAT:  The layer table names the same modules the crate root declares.
     * WHY:   A module missing from LAYER_ORDER is silently exempt from the whole
     *        check — it has no layer, so nothing it imports can be "above" it.
     *        That is the way this guardrail would rot: someone adds a module,
     *        forgets this file, and it quietly becomes the one place upward
     *        imports are allowed.
     */
    #[test]
    fn every_module_in_the_crate_root_has_a_layer() {
        let root = include_str!("lib.rs");
        let declared: Vec<&str> = root
            .lines()
            .filter_map(|line| {
                let t = line.trim();
                t.strip_prefix("pub mod ")
                    .or_else(|| t.strip_prefix("mod "))
                    .and_then(|rest| rest.strip_suffix(';'))
            })
            .collect();

        // `testing` and `layering` are test-support and are not layers.
        const NOT_A_LAYER: &[&str] = &["testing", "layering"];

        let missing: Vec<&str> = declared
            .iter()
            .copied()
            .filter(|module| layer_of(module).is_none() && !NOT_A_LAYER.contains(module))
            .collect();

        assert!(
            missing.is_empty(),
            "these modules are declared in lib.rs but have no layer, so the \
             layering check silently ignores them: {missing:?}"
        );
    }

    /**
     * WHAT:  The check can actually fail.
     * WHY:   Every assertion above is of the form "nothing was found", which a
     *        broken scanner satisfies by finding nothing. The recorded exception
     *        doubles as the positive case: it is a real upward import, so the
     *        scanner must see it.
     */
    #[test]
    fn the_scanner_can_see_an_upward_import() {
        let found = upward_imports();
        assert!(
            !found.is_empty(),
            "the scanner found no upward imports at all, including the one \
             recorded in KNOWN_UPWARD_IMPORTS — it is matching nothing and every \
             assertion here is passing vacuously"
        );
        assert!(
            layer_of("adapters") > layer_of("ports"),
            "the layer table is upside down"
        );
    }
}
