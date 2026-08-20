/*!
 * SOURCE OF TRUTH KEYWORDS: main, murmur_binary, windows_subsystem, run
 * WHAT:  The executable entry point. Hands straight off to the library.
 * WHY:   Kept to three lines on purpose. Everything the app does lives in the
 *        library crate, which is what lets the whole application — including
 *        the session actor and the full pipeline — be exercised by tests. A
 *        binary with logic in it is a binary whose logic cannot be tested.
 * WHERE: The only caller of murmur_lib::run.
 */

// Prevents an additional console window on Windows in release. DO NOT REMOVE.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    murmur_lib::run()
}
