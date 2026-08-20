/*!
 * SOURCE OF TRUTH KEYWORDS: run, murmur_lib, module_tree, ActivationPolicy
 * WHAT:  Crate root. Declares the layer modules and starts the Tauri app.
 * WHY:   Wiring only, no logic — the layering the whole codebase depends on is
 *        only legible if the root stays a table of contents. Module order here
 *        mirrors the dependency direction: infrastructure first, then contracts,
 *        then the implementations and the things that consume them.
 * WHERE: Entered from main.rs.
 */

// ── Infrastructure ───────────────────────────────────────────────────────
pub mod config;
pub mod db;
pub mod error;
pub mod telemetry;

#[cfg(test)]
pub mod testing;

/// Enforces the dependency direction docs/05 §5 describes. Lives at the root
/// because it is a property of the whole module tree, not of any one layer.
#[cfg(test)]
mod layering;
pub mod types;

// ── Contracts ────────────────────────────────────────────────────────────
pub mod ports;

// ── Implementations ──────────────────────────────────────────────────────
pub mod adapters;

// ── Source of truth ──────────────────────────────────────────────────────
pub mod registry;

// ── Domain ───────────────────────────────────────────────────────────────
pub mod pipeline;
pub mod services;
pub mod session;

// ── Boundary ─────────────────────────────────────────────────────────────
pub mod bootstrap;
pub mod ipc;
pub mod tray;

use tauri::Manager;

/**
 * SOURCE OF TRUTH KEYWORDS: run
 * WHAT:  Builds the Tauri app, installs the plugins, and hands off to bootstrap.
 * WHY:   The specta builder is mounted BEFORE setup so `invoke_handler` and the
 *        typed event registry are in place before anything can emit. Activation
 *        policy is set to Accessory here rather than relying on LSUIElement
 *        alone, because the plist only takes effect in a bundled app and this
 *        must behave the same under `tauri dev`.
 * WHERE: Called by main.rs.
 */
/// `Manager` is implemented on AppHandle, not on `&mut App`.
fn handle_for_state(app: &tauri::App) -> tauri::AppHandle {
    app.handle().clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = ipc::bindings::specta_builder();

    tauri::Builder::default()
        /*
         * SOURCE OF TRUTH KEYWORDS: single_instance, second_launch
         * MUST BE FIRST. The plugin's own documentation requires it, and the
         * reason is that a second process has to be turned away BEFORE it runs
         * any of the setup below.
         *
         * What a second launch did until this line existed: it opened the same
         * SQLite file, and `recover_orphans` — which closes out sessions that
         * have no `ended_at` because the app was killed mid-recording — marked
         * the FIRST process's live, in-flight session as Orphaned. Only then
         * did its own hotkey registration fail and setup give up. The damage
         * was already done, to a recording that was still going, in the other
         * process, which had no idea.
         *
         * Reordering recover_orphans (see bootstrap::setup) fixes the
         * single-process race. It cannot fix this one: no ordering inside a
         * process protects it from a second process. The callback shows the
         * dashboard, because someone who launches an already-running menu-bar
         * app is asking to see it.
         */
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tracing::info!("a second launch was refused; showing the dashboard instead");
            tray::show_dashboard(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Required, not optional: UpdaterExt::updater() resolves plugin state
        // through Manager::state, which PANICS when the plugin is absent. The
        // capabilities entry and the config block do not register it — only
        // this line does, and without it check_for_update compiles, exports
        // correct bindings, and panics the first time it is called.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            // Registers the typed event channel the frontend listens on.
            builder.mount_events(app);

            // No Dock icon, no app switcher entry. Murmur is a background
            // utility; the menu bar item is its only permanent surface.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();

            // Logging is started first so everything after it is recorded.
            let paths = config::AppPaths::resolve()?;
            let guard = telemetry::init_tracing(&paths.logs_dir);
            // Held for the process lifetime — dropping the guard closes the log
            // appender and log lines silently stop, which is a confusing thing
            // to debug. `manage` lives on Manager, hence the handle.
            handle_for_state(app).manage(guard);

            if let Err(err) = bootstrap::setup(&handle) {
                tracing::error!(error = %err, "startup failed");
                return Err(Box::new(err) as Box<dyn std::error::Error>);
            }

            if let Err(err) = tray::install_tray(&handle) {
                // The app is still usable by hotkey without a tray icon, so
                // this is logged rather than fatal.
                tracing::error!(error = %err, "could not install the menu bar item");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Murmur");
}
