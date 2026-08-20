/*!
 * SOURCE OF TRUTH KEYWORDS: icon_resolution, every_nav_icon_resolves,
 *   FALLBACK_MARKER, normalise_icon_name, icons.ts
 * WHAT:  Asserts that every icon name the registry declares actually resolves
 *        to a real glyph in the frontend's icon map.
 * WHY:   THIS IS THE TEST THAT WOULD HAVE CAUGHT THE PLACEHOLDER SIDEBAR.
 *        `NavDef.icon` is a STRING that crosses into TypeScript and is looked
 *        up in a hand-maintained map. A name with no entry does not fail to
 *        build, does not warn, and does not throw — it silently renders the
 *        fallback glyph. Billing declared `CreditCard` and Stats declared
 *        `ChartNoAxesColumn`, neither was in the map, and both shipped wearing
 *        a placeholder until the operator noticed and reported it.
 *
 *        That is the same shape as every serious bug in this project: a value
 *        the registry declares, that stores and renders correctly, and is
 *        quietly wrong. The frontend cannot catch it — TypeScript has no idea
 *        what strings Rust will send — so the assertion has to live on the side
 *        that declares the name, reading the file that resolves it. Same
 *        arrangement as `reachability.rs`, which reads shipped source to prove
 *        settings are consumed, and as `tray.rs`, which reads `tokens.css` so
 *        the pill window cannot disagree with the design.
 *
 *        Matching is done by re-implementing `normaliseIconName` rather than by
 *        looking for the literal: the frontend matches case- and
 *        separator-insensitively, so a test that demanded an exact string would
 *        fail on entries that actually work and teach people to weaken it.
 * WHERE: Reads `src/lib/icons.ts`. Runs with `cargo test`.
 */

/// Mirrors `normaliseIconName` in src/lib/icons.ts.
fn normalise_icon_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICONS_TS: &str = include_str!("../../../src/lib/icons.ts");

    /// The keys of the `ICONS` map — everything the frontend can resolve.
    fn mapped_icon_keys() -> Vec<String> {
        let body = ICONS_TS
            .split_once("const ICONS")
            .expect("src/lib/icons.ts must declare an ICONS map")
            .1;
        let body = body
            .split_once('}')
            .expect("the ICONS map must be terminated")
            .0;

        body.lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(key, _)| key.trim().to_string())
            .filter(|key| !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric()))
            .collect()
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: every_nav_icon_resolves
     * WHAT:  Every nav entry's icon exists in the frontend map.
     * WHY:   See the module WHY. Without this, the next capability added with a
     *        mistyped or simply unmapped icon name ships a placeholder glyph and
     *        nobody finds out until someone looks at the sidebar.
     */
    #[test]
    fn every_nav_icon_resolves_to_a_real_glyph() {
        let keys = mapped_icon_keys();
        assert!(
            keys.len() > 5,
            "failed to parse src/lib/icons.ts — got {} keys, so this test is \
             asserting nothing. Fix the parser, do not delete the test.",
            keys.len()
        );

        let missing: Vec<String> = super::super::capabilities()
            .iter()
            .filter_map(|capability| capability.nav.as_ref())
            .filter(|nav| !keys.contains(&normalise_icon_name(&nav.icon)))
            .map(|nav| format!("{} (route \"{}\") declares icon \"{}\"", nav.label, nav.route, nav.icon))
            .collect();

        assert!(
            missing.is_empty(),
            "these nav entries would render the fallback glyph instead of an \
             icon — add each name to ICONS in src/lib/icons.ts:\n  {}",
            missing.join("\n  ")
        );
    }
}
