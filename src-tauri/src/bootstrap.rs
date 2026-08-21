/*!
 * SOURCE OF TRUTH KEYWORDS: bootstrap, setup, build_ports, spawn_actor,
 *   register_hotkeys, prepare_engine, recover_orphans, show_first_run
 * WHAT:  Everything that has to happen once at launch: resolve paths, open the
 *        database, construct the adapters, start the session actor, register
 *        the global hotkey, and warm the model.
 * WHY:   This is the ONLY file that names a concrete adapter. Everything else
 *        in the app is written against ports, so swapping an engine or moving
 *        to Windows changes this file and one directory under adapters/, and
 *        nothing else.
 *
 *        Ordering here is not arbitrary. The model is warmed on a background
 *        thread AFTER the windows exist, because loading it costs ~1.5s and
 *        doing that before the UI appears makes a fast app look slow. But it
 *        must complete before the first hotkey press, which is why it starts
 *        immediately rather than lazily — a cold load on the hotkey path is
 *        the single worst first impression this app could give.
 * WHERE: Called from lib.rs `run`.
 */

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::adapters;
use crate::adapters::cpal::CpalAudioSource;
use crate::adapters::macos::{MacosInjector, MacosPermissions};
use crate::adapters::rules::RuleEnhancer;
use crate::config::AppPaths;
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::ipc::context::{AppState, Ports, SessionHandle};
use crate::ports::permissions::OsPermission;
use crate::pipeline::worker::AsrWorker;
use crate::registry::{self, keys};
use crate::services;
use crate::session::{SessionActor, SessionEvent, SessionSettings};
use crate::telemetry::now_ms;
use crate::types::{HotkeyBinding, KeyModifier, RecordingMode, SettingValue};

/**
 * SOURCE OF TRUTH KEYWORDS: report_permissions, startup_permission_state
 * WHAT:  Logs the state of every OS grant once at launch.
 * WHY:   A missing grant does not announce itself — it degrades a feature
 *        quietly, and from the outside that is indistinguishable from the
 *        feature being broken. Diagnosing it otherwise means reproducing a
 *        recording and reading a downstream message. One line at startup makes
 *        the app say what it can and cannot do before anyone has to ask.
 *
 *        Worth knowing when reading this: macOS attributes a grant to the
 *        RESPONSIBLE process, which for a binary launched from a terminal is
 *        the terminal, not the app. So a build launched by hand from a shell
 *        can report Denied while the identical bundle launched normally
 *        reports Granted, and the grant list looks correct in both cases.
 * WHERE: Called once from setup, after the ports exist.
 */
fn report_permissions(ports: &Ports) {
    for permission in [OsPermission::Microphone, OsPermission::Accessibility] {
        let state = ports.permissions.check(permission);
        tracing::info!(?permission, ?state, "permission at startup");
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: keep_windows_alive, CloseRequested, prevent_close,
 *   hide_on_close
 * WHAT:  Makes closing a window HIDE it rather than destroy it.
 * WHY:   Tauri destroys a window when it is closed, and a destroyed window
 *        cannot be shown again — `get_webview_window` returns None from then
 *        on. So closing the dashboard did not merely put it away: it removed
 *        the app's ONLY window permanently, and the tray item that exists to
 *        reopen it silently did nothing for the rest of the session. Dictation
 *        kept working, which made it look like the window had broken rather
 *        than gone.
 *
 *        Hiding is also the correct macOS behaviour independently of the bug.
 *        Closing a window is not quitting an app; for a menu-bar utility with
 *        no Dock icon it is simply putting the window away, and Quit in the
 *        tray remains the one way to actually stop Murmur.
 *
 *        The pill is deliberately NOT included: it has no close control, it is
 *        not clickable at all, and its visibility is owned by the session FSM.
 * WHERE: Installed once during setup, for every window a user can close.
 */
fn keep_windows_alive(app: &AppHandle) {
    for label in [DASHBOARD_WINDOW, ONBOARDING_WINDOW] {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        let handle = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(err) = handle.hide() {
                    tracing::warn!(error = %err, "could not hide the window on close");
                }
                tracing::debug!(window = handle.label(), "window hidden rather than destroyed");
            }
        });
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: apply_window_vibrancy, NSVisualEffectMaterial,
 *   glass, vibrancy, frosted
 * WHAT:  Installs the native NSVisualEffectView behind each window.
 * WHY:   THIS IS WHAT MAKES THE GLASS. Every window is declared
 *        `transparent: true` and the web layer paints a transparent body, on
 *        the understanding that a native vibrancy view sits behind it. Without
 *        this call there is nothing back there at all: the windows are fully
 *        see-through, which reads as "the window did not open" rather than as a
 *        rendering fault, and the entire frosted look the design depends on is
 *        simply absent.
 *
 *        Materials are assigned per docs/04 §3 — HudWindow for the pill,
 *        Sidebar for the dashboard, and Popover for onboarding, which is a
 *        sheet rather than a workspace. A failure to apply is logged and not
 *        fatal: an opaque-ish window is worse-looking but still usable, and
 *        refusing to launch over a visual effect would be the wrong trade.
 * WHERE: Called once per window during setup.
 */
fn apply_window_vibrancy(app: &AppHandle) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

    // The radius is what makes the glass the PILL's shape rather than the
    // window's. Without it the effect view is a rectangle filling the whole
    // window — which is exactly the "huge square" behind the pill. The two full
    // windows take None because for them the window IS the surface.
    // Every window gets a radius, and the two full windows get one for the same
    // reason the pill does. They are `transparent` with `titleBarStyle: Overlay`,
    // so macOS paints no background of its own — the NSVisualEffectView IS the
    // visible window. Leaving it square meant the app had system-radius corners
    // (about 10pt) no matter what the design asked for, and rounding the WEB
    // layer instead would have put square glass corners outside a rounded page:
    // the pill's "huge square" bug relocated.
    //
    // Native traffic lights are unaffected. `decorations: false` would have got
    // the same corners at the cost of drawing our own window buttons, and
    // hand-made controls that feel subtly un-Mac are exactly what this app
    // cannot afford.
    let window_radius = crate::tray::design_token("--radius-window");
    for (label, material, radius) in [
        (PILL_WINDOW, NSVisualEffectMaterial::HudWindow, Some(crate::tray::pill_radius())),
        // The rail is its own floating object, so it carries the window radius
        // rather than the pill's. Sidebar material to match the dashboard it
        // hangs beside.
        (SIDEBAR_WINDOW, NSVisualEffectMaterial::Sidebar, Some(window_radius)),
        (DASHBOARD_WINDOW, NSVisualEffectMaterial::Sidebar, Some(window_radius)),
        (ONBOARDING_WINDOW, NSVisualEffectMaterial::Popover, Some(window_radius)),
    ] {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        // Active state: the material keeps its blur even when the app is not
        // frontmost. An accessory app is rarely frontmost, and a pill that goes
        // flat grey the moment you click elsewhere would look broken.
        match apply_vibrancy(&window, material, Some(NSVisualEffectState::Active), radius) {
            Ok(()) => tracing::debug!(window = label, "vibrancy applied"),
            Err(err) => tracing::warn!(window = label, error = %err, "could not apply vibrancy"),
        }
    }
}

/// Window labels, matching tauri.conf.json. Typos here fail at runtime.
pub const DASHBOARD_WINDOW: &str = "dashboard";
pub const PILL_WINDOW: &str = "pill";
pub const SIDEBAR_WINDOW: &str = "sidebar";
pub const ONBOARDING_WINDOW: &str = "onboarding";

/**
 * SOURCE OF TRUTH KEYWORDS: setup
 * WHAT:  The whole launch sequence.
 * WHERE: Called from the Tauri builder's setup hook.
 */
pub fn setup(app: &AppHandle) -> AppResult<()> {
    let paths = AppPaths::resolve()?;
    let db = Database::open(&paths.db_path)?;

    // A registry that contradicts itself is a wiring bug, and it is far better
    // to know at launch than to find a control that will not render.
    if let Err(problems) = registry::validate_registry() {
        return Err(AppError::internal(problems.join("; ")));
    }

    // BEFORE the state is managed and before the actor exists, and the ordering
    // is the whole point. `manage` makes every IPC command reachable and the
    // three windows are created by tauri.conf before setup runs, so a webview
    // that invokes start_recording during the rest of this function gets a
    // session row written — and a sweep that ran afterwards would mark that
    // LIVE session orphaned for having no ended_at. Recovering crashed sessions
    // is only meaningful while no session can yet exist.
    recover_orphans(&db);

    let ports = build_ports(app, &paths)?;
    let settings = SessionSettings::load(&db);

    // The channel is created before AppState so the handle can go into it, and
    // the actor receives a clone of the finished state — no cycle.
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(crate::session::actor::EVENT_QUEUE_DEPTH);
    let state = AppState::new(
        paths.clone(),
        db.clone(),
        ports.clone(),
        SessionHandle::new(event_tx),
        app.clone(),
    );
    app.manage(state.clone());

    let (decode_tx, decode_rx) = tokio::sync::mpsc::channel(16);
    let worker = AsrWorker::spawn(Arc::clone(&ports.engine), decode_tx)
        .map_err(|err| AppError::internal(err).with_detail("spawning the ASR worker"))?;

    let actor = SessionActor::new(state.session_context(), worker, decode_rx, settings.clone());
    tauri::async_runtime::spawn(actor.run(event_rx));

    apply_window_vibrancy(app);
    crate::tray::adopt_pill_tokens(app);
    watch_permissions(app);
    attach_sidebar(app);
    keep_rail_centred(app);
    keep_windows_alive(app);
    report_permissions(&ports);
    start_retention_sweep(state.clone());
    start_update_checks(state.clone());
    /*
     * SOURCE OF TRUTH KEYWORDS: hotkey_failure_is_not_fatal
     * Logged, never propagated. A hotkey that will not register is a degraded
     * app — the tray and the dashboard still work, and the binding can be
     * changed in Settings — whereas propagating it here aborts `setup` and the
     * app does not launch at all. Refusing to start because a shortcut is taken
     * by another app is a wildly disproportionate response.
     *
     * This matters more since a modifier-only binding became possible: that
     * path needs Accessibility, so a revoked grant would otherwise turn a
     * missing hotkey into an app that cannot be opened to fix it.
     */
    if let Err(err) = register_hotkeys(app, &db) {
        tracing::error!(error = %err, "could not register the dictation hotkey; the app will start without it");
    }
    watch_settings_for_rebinds(app);
    prepare_engine(
        app,
        Arc::clone(&ports.engine),
        Arc::clone(&ports.models),
        default_model_id(),
    );
    show_first_window(app, &db)?;
    show_dashboard_on_launch(app);

    Ok(())
}

/**
 * SOURCE OF TRUTH KEYWORDS: build_ports
 * WHAT:  Constructs one implementation per port.
 * WHY:   The engine is built around whatever model file is currently on disk,
 *        WITHOUT downloading — a first run with no model still has to reach the
 *        onboarding window, and a bootstrap that blocks on a 574MB download
 *        would show nothing at all until it finished.
 * WHERE: Called once by setup.
 */
fn build_ports(app: &AppHandle, paths: &AppPaths) -> AppResult<Ports> {
    let events: Arc<dyn crate::ports::EventSink> =
        Arc::new(adapters::TauriEventSink::new(app.clone()));

    let models = adapters::build_model_store(paths.clone(), Arc::clone(&events))?;

    let model_id = default_model_id();
    let engine = adapters::build_engine(
        &adapters::default_engine_id(),
        paths.model_file(model_id.as_str()),
    )?;

    Ok(Ports {
        engine,
        audio: Arc::new(CpalAudioSource::new()),
        enhancer: Arc::new(RuleEnhancer::new()),
        injector: Arc::new(MacosInjector::new(MacosPermissions::new())),
        models,
        permissions: Arc::new(MacosPermissions::new()),
        events,
    })
}

fn default_model_id() -> crate::types::ModelId {
    let id = registry::setting_def(keys::TRANSCRIPTION_MODEL)
        .and_then(|def| match &def.default {
            SettingValue::Choice(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "large-v3-turbo-q5_0".to_string());
    crate::types::ModelId(id)
}

/**
 * SOURCE OF TRUTH KEYWORDS: worth_warming
 * WHAT:  Whether a model's state means there is something on disk worth
 *        verifying and loading at startup.
 * WHY:   Extracted so the one decision that matters here can be tested. The
 *        warm-up runs on a background thread holding an AppHandle, which no
 *        unit test can build, so leaving this inline would have meant the
 *        behaviour was only checkable by launching the app — and the bug it
 *        replaces was invisible precisely because nobody launches a fresh
 *        install twice.
 *
 *        `Failed` counts as worth trying: the recorded failure may have been a
 *        bad hash on a file that has since been replaced by hand, and `ensure`
 *        re-hashes rather than trusting the verdict. `NotDownloaded` is the one
 *        state that must return false, because acting on it means DOWNLOADING,
 *        and choosing what to download is onboarding's job.
 * WHERE: prepare_engine's warm-up thread.
 */
fn worth_warming(state: &crate::types::ModelState) -> bool {
    !matches!(state, crate::types::ModelState::NotDownloaded)
}

/**
 * SOURCE OF TRUTH KEYWORDS: prepare_engine
 * WHAT:  Loads the model and warms its Metal context, off the main thread.
 * WHY:   `prepare()` blocks for well over a second, and it must NEVER be on the
 *        hotkey path — a cold load there turns "press and talk" into a
 *        multi-second hang the first time someone uses the app. Running it here
 *        means the model is warm before the user can reach for the key.
 *        A missing model file is not an error at this point: onboarding has not
 *        run yet, and the download is its job.
 * WHERE: Called once by setup, after the windows exist.
 */
fn prepare_engine(
    app: &AppHandle,
    engine: Arc<dyn crate::ports::TranscriptionEngine>,
    models: Arc<dyn crate::ports::ModelStore>,
    model_id: crate::types::ModelId,
) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("murmur-engine-warmup".into())
        .spawn(move || {
            /*
             * SOURCE OF TRUTH KEYWORDS: warm_up_never_downloads
             * ASK FIRST, and do not download. `status` reports what is on disk
             * without fetching anything; `ensure` fetches. Calling `ensure`
             * here meant a fresh install began pulling 574MB of the DEFAULT
             * model the moment the app launched — before onboarding had asked
             * the user which model they wanted. Pick a different one and you
             * paid for two.
             *
             * The comment that used to sit here said "onboarding will fetch
             * it", describing behaviour this code did not have. Downloading is
             * onboarding's job because onboarding is where the choice is made,
             * and this thread has no business pre-empting it.
             *
             * Once the file IS present, `ensure` still runs, and that part is
             * deliberate: it hashes before the engine is handed a path, which
             * is what "verify by hash, never by presence" means. For an
             * installed model it downloads nothing, and this thread is already
             * waiting on a ~1.5s load, so the hash costs nothing anyone is
             * watching.
             */
            let installed = tauri::async_runtime::block_on(models.status(&model_id))
                .map(|status| worth_warming(&status.state))
                .unwrap_or(false);

            if !installed {
                tracing::info!(
                    model = model_id.as_str(),
                    "no model on disk yet; leaving the download to onboarding"
                );
                return;
            }

            if let Err(err) = tauri::async_runtime::block_on(models.ensure(&model_id)) {
                tracing::info!(error = %err, "model could not be verified; onboarding will fetch it");
                return;
            }

            let started = std::time::Instant::now();
            match engine.prepare() {
                Ok(()) => tracing::info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    realtime_factor = engine.capabilities().realtime_factor,
                    "engine warm"
                ),
                Err(err) => {
                    // Expected on a first run — the model has not been
                    // downloaded yet. Onboarding will call prepare again.
                    tracing::info!(error = %err, "engine not ready yet");
                    let _ = &app;
                }
            }
        })
        .ok();
}

/**
 * SOURCE OF TRUTH KEYWORDS: recover_orphans
 * WHAT:  Closes out sessions that were in flight when the process last died.
 * WHY:   A row with no `ended_at` means the app was killed mid-recording. The
 *        audio is gone with the process, so the row cannot be completed — but
 *        it MUST be closed, or it stays in "find_orphans" forever and History
 *        shows a recording that never ends. Marking it is what makes the crash
 *        visible in History rather than silent.
 * WHERE: Called once at launch, before any new session can start.
 */
fn recover_orphans(db: &Database) {
    match services::sessions::find_orphans(db) {
        Ok(orphans) if orphans.is_empty() => {}
        Ok(orphans) => {
            tracing::warn!(count = orphans.len(), "recovering interrupted sessions");
            for orphan in orphans {
                if let Err(err) = services::sessions::mark_orphaned(db, &orphan.id, now_ms()) {
                    tracing::error!(error = %err, "could not close an interrupted session");
                }
            }
        }
        Err(err) => tracing::error!(error = %err, "could not check for interrupted sessions"),
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: UPDATE_CHECK_INTERVAL, start_update_checks
 * WHAT:  How often the app looks for a new version after the check at launch.
 * WHY:   Twenty-four hours, because that is precisely what the
 *        `general.check_updates` setting promises: "Check for a new version on
 *        launch and once a day." The number is not a judgement call here — it is
 *        a sentence the user has already read, and the code exists to make it
 *        true.
 */
const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/**
 * SOURCE OF TRUTH KEYWORDS: start_update_checks, lying_control
 * WHAT:  Checks for a new version at launch and once a day, and tells the UI
 *        when one exists.
 * WHY:   `check_for_update` and `install_update` were registered, exported and
 *        called by NOTHING — no launch check, no timer, no button. Meanwhile the
 *        setting described a schedule in the present tense. That is the same
 *        shape as the four inert settings this codebase already found: a
 *        control the user reads, agrees with, and which changes no behaviour.
 *
 *        The setting is re-read on every tick rather than captured, so turning
 *        updates off stops the next request rather than the one after a restart
 *        — it is described as "the only network request Murmur makes after
 *        setup", and a promise about network traffic has to be honoured
 *        immediately to mean anything.
 *
 *        Takes the Updates reentrancy guard, exactly as the IPC path does. A
 *        scheduled check racing a manual one is the race the guard exists for,
 *        and being a timer does not exempt it. A skipped tick costs nothing.
 *
 *        Emits rather than acting. Downloading or installing without being asked
 *        would replace the running app under someone mid-sentence; the event
 *        offers it and `install_update` — which already refuses while a session
 *        is live — does the work when they say yes.
 * WHERE: Started once by setup.
 */
fn start_update_checks(state: AppState) {
    use crate::ipc::commands::updates::{look_for_update, UpdateCheck};
    use crate::ipc::events::UpdateAvailable;
    use tauri_specta::Event;

    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(UPDATE_CHECK_INTERVAL);
        loop {
            // The first tick completes immediately: that is the launch check.
            ticker.tick().await;

            let Some(_guard) = state.begin_exclusive(registry::CapabilityKey::Updates) else {
                tracing::debug!("an update check is already running; skipping this one");
                continue;
            };

            match look_for_update(&state).await {
                Ok(UpdateCheck::Available {
                    version,
                    current_version,
                    notes,
                    ..
                }) => {
                    tracing::info!(version, current_version, "an update is available");
                    let event = UpdateAvailable {
                        version,
                        current_version,
                        notes,
                    };
                    if let Err(err) = event.emit(&state.app) {
                        tracing::warn!(error = %err, "could not announce the update");
                    }
                }
                Ok(UpdateCheck::UpToDate { current_version }) => {
                    tracing::debug!(current_version, "already on the newest version")
                }
                Ok(UpdateCheck::Disabled) => {
                    tracing::debug!("update checks are turned off")
                }
                // Offline, or the endpoint is unreachable. Not worth telling the
                // user about — they did not ask for this check.
                Err(err) => tracing::info!(error = %err, "could not check for updates"),
            }
        }
    });
}

/// How often the retention sweep runs after the one at launch. Six hours, not
/// twenty-four: a machine that sleeps overnight would otherwise skip its daily
/// tick entirely and a "delete after 7 days" setting would quietly mean eight
/// or nine. Not hourly either — this deletes rows, and a promise measured in
/// days does not need to be kept to the hour.
const RETENTION_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/**
 * SOURCE OF TRUTH KEYWORDS: start_retention_sweep, retention_days, purge
 * WHAT:  Applies `privacy.retention_days` — at launch, and every six hours
 *        after that for as long as the app is running.
 * WHY:   The setting says "Older transcripts are deleted automatically". Until
 *        this existed nothing anywhere called `purge_older_than` except an IPC
 *        command the frontend never invoked, so the sentence was false: the
 *        control rendered, the value saved, the value came back, and every
 *        transcript was kept forever. In an app whose entire pitch is that your
 *        speech never leaves the machine, a privacy control that does nothing
 *        is the worst kind of bug — the user has been told they are protected.
 *
 *        Both halves are needed. Launch-only would never fire for someone who
 *        leaves Murmur running for weeks, which is the intended way to use a
 *        menu-bar utility. Timer-only would never fire for someone who quits
 *        every evening.
 *
 *        The setting is re-read on every sweep rather than captured once, for
 *        the same reason `updates_enabled` re-reads: a cached retention window
 *        is a deletion the user has already cancelled, or one they have asked
 *        for and will not get until they restart.
 *
 *        Takes the History reentrancy guard, exactly as the IPC path does. A
 *        background job that deletes rows while an export is walking them is
 *        the race the guard exists to prevent, and being a timer rather than a
 *        command does not exempt it. A skipped tick is free — the next one is
 *        six hours away and the cutoff will have moved by seconds.
 * WHERE: Started once by setup, after the database is open.
 */
fn start_retention_sweep(state: AppState) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(RETENTION_SWEEP_INTERVAL);
        // The first tick completes immediately, which is the launch sweep.
        loop {
            ticker.tick().await;
            sweep_retention(&state);
        }
    });
}

/// One sweep. Separated from the timer so it is testable without waiting.
fn sweep_retention(state: &AppState) {
    let days = retention_days(&state.db);

    let Some(cutoff) = retention_cutoff(days, now_ms()) else {
        return;
    };

    let Some(_guard) = state.begin_exclusive(registry::CapabilityKey::History) else {
        tracing::debug!("history is busy; retention sweep will run again later");
        return;
    };
    match services::sessions::purge_older_than(&state.db, cutoff) {
        Ok(0) => tracing::debug!(days, "retention sweep found nothing to delete"),
        Ok(removed) => tracing::info!(days, removed, "retention sweep deleted old transcripts"),
        Err(err) => tracing::error!(error = %err, "retention sweep failed"),
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: retention_cutoff
 * WHAT:  The timestamp before which transcripts should be deleted, or None when
 *        nothing should be.
 * WHY:   Split out so the policy is testable without a database, a clock or an
 *        AppState. The two answers that matter are both edge cases: 0 means
 *        KEEP EVERYTHING — it is the registry default and the value the
 *        description promises ("Set to 0 to keep everything forever") — and a
 *        negative value, which no UI can produce but a hand-edited database
 *        can, must also delete nothing rather than computing a cutoff in the
 *        future and erasing the user's entire history.
 * WHERE: sweep_retention.
 */
fn retention_cutoff(days: i64, now: i64) -> Option<i64> {
    if days <= 0 {
        return None;
    }
    Some(now - days * 24 * 60 * 60 * 1000)
}

/// The user's retention window in days, falling back to the registry default.
fn retention_days(db: &Database) -> i64 {
    let stored = services::settings::get_setting(db, keys::RETENTION_DAYS)
        .ok()
        .flatten();

    let value = stored.or_else(|| {
        registry::setting_def(keys::RETENTION_DAYS).map(|def| def.default.clone())
    });

    match value {
        Some(SettingValue::Number(days)) => days as i64,
        _ => 0,
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: register_hotkeys, dictation_binding
 * WHAT:  Registers the global dictation shortcut.
 * WHY:   One binding, registered for the app's lifetime. Escape is deliberately
 *        NOT registered here — it is bound only while recording and released
 *        immediately after, so Murmur never steals Escape from another app.
 *        That registration lives with the session, not with the bootstrap.
 * WHERE: Called once by setup.
 */
/**
 * SOURCE OF TRUTH KEYWORDS: DICTATION_SHORTCUT, registered_binding
 * WHAT:  The dictation shortcut currently registered with the OS.
 * WHY:   Rebinding has to unregister the OLD accelerator, and the only record
 *        of what that was is here. Reading the database instead would return
 *        the NEW value — the write lands before the event fires — so the old
 *        binding would stay registered forever and both keys would work.
 *
 *        Deliberately not `unregister_all()`, which would also drop Escape.
 *        Escape is registered and released per session by set_escape_registered
 *        and is none of this function's business.
 * WHERE: Written by register_hotkeys and rebind_dictation_hotkey.
 */
static DICTATION_SHORTCUT: parking_lot::Mutex<Option<Shortcut>> = parking_lot::Mutex::new(None);

/**
 * SOURCE OF TRUTH KEYWORDS: MODIFIER_TAP, modifier_only_binding
 * WHAT:  The running event tap, when the binding is a bare modifier.
 * WHY:   Exactly one of this and DICTATION_SHORTCUT is ever live, because a
 *        binding is either a chord or a lone modifier and the two are
 *        registered by completely different mechanisms. Held statically for the
 *        same reason as the shortcut: rebinding has to tear down whichever one
 *        is currently active, and after the settings write the database can no
 *        longer say which that was.
 */
static MODIFIER_TAP: parking_lot::Mutex<Option<crate::adapters::macos::ModifierTap>> =
    parking_lot::Mutex::new(None);

/// Stops whichever mechanism is currently registered. Idempotent.
fn release_dictation_binding(app: &AppHandle) {
    // Dropping the tap stops its thread.
    *MODIFIER_TAP.lock() = None;

    if let Some(previous) = DICTATION_SHORTCUT.lock().take() {
        let manager = app.global_shortcut();
        if manager.is_registered(previous) {
            if let Err(err) = manager.unregister(previous) {
                tracing::warn!(error = %err, "could not release the old hotkey");
            }
        }
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: bind_dictation, two_mechanisms
 * WHAT:  Registers `binding` by whichever mechanism it needs.
 * WHY:   One function so the CHOICE is made in one place. A bare modifier needs
 *        a CGEventTap watching for a double-tap; anything else is an ordinary
 *        global shortcut. Callers should never have to know which.
 * WHERE: register_hotkeys at launch, and rebind_dictation_hotkey after a write.
 */
fn bind_dictation(app: &AppHandle, binding: &HotkeyBinding) -> AppResult<()> {
    if let Some(modifier) = binding.sole_modifier() {
        let handler_app = app.clone();
        let tap = crate::adapters::macos::watch_modifier_tap(modifier, move || {
            // The tap has no notion of press and release — a double-tap is a
            // single gesture — so it always reports a Pressed edge and the
            // toggle logic in on_dictation_hotkey does the rest. Push-to-talk
            // is meaningless for a modifier and is documented as such in the
            // recording-mode setting.
            on_dictation_hotkey(&handler_app, ShortcutState::Pressed);
        });

        return match tap {
            Some(tap) => {
                *MODIFIER_TAP.lock() = Some(tap);
                tracing::info!(?modifier, taps = crate::adapters::macos::TAPS_REQUIRED, "dictation bound to a modifier tap");
                Ok(())
            }
            None => Err(AppError::new(
                ErrorCode::HotkeyRegistrationFailed,
                "Murmur could not watch for that key. Allow Murmur under Accessibility and try again.",
            )
            .recoverable()),
        };
    }

    let shortcut = to_shortcut(binding)?;
    let handler_app = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            on_dictation_hotkey(&handler_app, event.state());
        })
        .map_err(|err| {
            AppError::new(
                ErrorCode::HotkeyRegistrationFailed,
                "Murmur could not register its hotkey. Another app may already be using it.",
            )
            .recoverable()
            .with_detail(err)
        })?;

    *DICTATION_SHORTCUT.lock() = Some(shortcut);
    Ok(())
}

fn register_hotkeys(app: &AppHandle, db: &Database) -> AppResult<()> {
    let binding = dictation_binding(db);
    bind_dictation(app, &binding)?;
    // Only for real shortcuts. A modifier-only binding is not an accelerator
    // and renders as nonsense here ("Alt+AltLeft"), which reads in a log like a
    // chord that could never be pressed — `bind_dictation` logs the tap it
    // actually installed instead. A diagnostic that describes the wrong
    // mechanism costs more than no diagnostic.
    if !binding.is_modifier_only() {
        tracing::info!(accelerator = binding.to_accelerator(), "hotkey registered");
    }
    Ok(())
}

/**
 * SOURCE OF TRUTH KEYWORDS: watch_settings_for_rebinds, settings_reach_the_OS
 * WHAT:  Applies the two settings whose effect lives outside this process — the
 *        dictation hotkey and launch-at-login — whenever one is written.
 * WHY:   `set_setting` writes a row and emits SettingsChanged. Every other
 *        consumer of a setting is inside this process and re-reads it when it
 *        next needs it, so for them the write IS the change. These two are not:
 *        their effect lives in the OS, and until this existed nothing carried
 *        the value there.
 *
 *        The hotkey case was worse than inert. `register_hotkeys` ran once at
 *        launch, so rebinding wrote the row, the Settings view and the pill
 *        re-read it and drew the NEW keycap, and the OS went on listening for
 *        the OLD one. The app displayed a key that did nothing and hid the key
 *        that did — a user pressing what the screen told them to press got
 *        silence, which reads as the whole feature being broken.
 *
 *        Launch-at-login was simply never applied: the plugin was installed in
 *        lib.rs and driven by nothing.
 * WHERE: Installed once by setup, immediately after register_hotkeys.
 */
fn watch_settings_for_rebinds(app: &AppHandle) {
    use crate::ipc::events::SettingsChanged;
    use tauri_specta::Event;

    // Reconcile once at launch: the OS is the durable side, and it can disagree
    // with the database if a LaunchAgent was removed by hand or a previous run
    // failed to apply a change.
    apply_launch_at_login(app);

    let handler_app = app.clone();
    SettingsChanged::listen(app, move |event| {
        // `None` means several settings changed at once — a reset, or an
        // import. Both of the settings below have to be re-applied then, so an
        // unnamed change is treated as naming all of them.
        let key = event.payload.key.clone();
        let touched = |candidate: &str| key.as_deref().is_none_or(|k| k == candidate);

        if touched(keys::DICTATION_HOTKEY) {
            rebind_dictation_hotkey(&handler_app);
        }
        if touched(keys::LAUNCH_AT_LOGIN) {
            apply_launch_at_login(&handler_app);
        }
    });
}

/**
 * WHAT:  Swaps the registered dictation shortcut for the one now in the
 *        database.
 * WHY:   Unregisters BEFORE registering. The two bindings can share a key with
 *        different modifiers, and registering first would leave the OS holding
 *        two handlers for overlapping accelerators — the failure mode being
 *        that one press starts and immediately stops a recording.
 *
 *        A failure to register the new binding is reported and the OLD one is
 *        left unregistered on purpose: the alternative is an app that silently
 *        keeps responding to a key the user has explicitly moved away from.
 * WHERE: The SettingsChanged listener.
 */
fn rebind_dictation_hotkey(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let binding = dictation_binding(&state.db);
    if let Err(err) = binding.bindable() {
        tracing::warn!(reason = %err, "ignoring an unbindable hotkey");
        return;
    }

    // Release BEFORE binding. The two mechanisms are exclusive, and a chord and
    // a modifier tap both live would mean two ways to start a recording, one of
    // which the user thinks they removed.
    release_dictation_binding(app);

    match bind_dictation(app, &binding) {
        Ok(()) => tracing::info!(accelerator = binding.to_accelerator(), "hotkey rebound"),
        // Nothing is registered now, and that is deliberate: an app that keeps
        // answering a key the user has explicitly moved away from is worse than
        // one with no hotkey until the next attempt.
        Err(err) => tracing::error!(error = %err, "could not register the new hotkey"),
    }
}

/**
 * WHAT:  Makes the OS agree with `general.launch_at_login`.
 * WHY:   Reads the current OS state first and only acts on a difference, so a
 *        settings write that did not touch this key does not rewrite the
 *        LaunchAgent plist on every save.
 * WHERE: Launch, and the SettingsChanged listener.
 */
fn apply_launch_at_login(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let wanted = matches!(
        services::settings::get_setting(&state.db, keys::LAUNCH_AT_LOGIN)
            .ok()
            .flatten()
            .or_else(|| {
                registry::setting_def(keys::LAUNCH_AT_LOGIN).map(|def| def.default.clone())
            }),
        Some(SettingValue::Bool(true))
    );

    let manager = app.autolaunch();
    let current = manager.is_enabled().unwrap_or(false);
    if current == wanted {
        return;
    }

    let outcome = if wanted {
        manager.enable()
    } else {
        manager.disable()
    };

    match outcome {
        Ok(()) => tracing::info!(enabled = wanted, "launch at login applied"),
        // Reported rather than fatal: the app works fine, it just will not
        // start itself. Silence here is what made this setting look supported.
        Err(err) => tracing::error!(error = %err, enabled = wanted, "could not apply launch at login"),
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: on_dictation_hotkey, RecordingMode, push_to_talk
 * WHAT:  Translates one hotkey event into the right session event, honouring
 *        the user's chosen hotkey behaviour.
 * WHY:   The two modes give the SAME key opposite meanings on key-up, so the
 *        mode has to be resolved before the event is chosen:
 *          - Toggle: press starts, press again stops. Key-up is ignored
 *            entirely — without that the handler fires twice per press and the
 *            second call stops the recording the first one just started.
 *          - Push-to-talk: press starts, RELEASE stops. Ignoring key-up here
 *            would leave the recording running forever.
 *        Deciding this here rather than sending a generic "hotkey" event and
 *        branching inside the FSM keeps the machine's vocabulary about what
 *        happened rather than about which key produced it.
 *
 *        A stray press while already recording is dropped rather than treated
 *        as a stop, because key repeat fires while a key is held.
 * WHERE: The global shortcut handler.
 */
fn on_dictation_hotkey(app: &AppHandle, key_state: ShortcutState) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let state = state.inner().clone();

    tauri::async_runtime::spawn(async move {
        let mode = recording_mode(&state.db);
        let capturing = state.current_state().is_capturing();

        let event = match (mode, key_state) {
            // Push-to-talk: the release is the stop.
            (RecordingMode::PushToTalk, ShortcutState::Pressed) if !capturing => {
                SessionEvent::StartRequested
            }
            (RecordingMode::PushToTalk, ShortcutState::Released) if capturing => {
                SessionEvent::StopRequested
            }
            // Toggle: only the press means anything.
            (RecordingMode::Toggle, ShortcutState::Pressed) => {
                if capturing {
                    SessionEvent::StopRequested
                } else {
                    SessionEvent::StartRequested
                }
            }
            // Key repeat while held, or a release in toggle mode.
            _ => return,
        };

        // Stamped here, at the press, because this is the only place that
        // knows when it happened. Feeds HotkeyDispatch and TotalFinalize.
        state.session.stamp_request();
        if let Err(err) = state.session.send(event).await {
            tracing::warn!(error = %err, "hotkey event was not delivered");
        }
    });
}

/// The user's hotkey behaviour, falling back to the registry default.
fn recording_mode(db: &Database) -> RecordingMode {
    let stored = services::settings::get_setting(db, keys::DICTATION_MODE)
        .ok()
        .flatten();

    match stored {
        Some(SettingValue::Choice(value)) if value == "push_to_talk" => RecordingMode::PushToTalk,
        _ => RecordingMode::Toggle,
    }
}

/// The user's binding if they have set one, otherwise the registry default.
fn dictation_binding(db: &Database) -> HotkeyBinding {
    let stored = services::settings::get_setting(db, keys::DICTATION_HOTKEY).ok().flatten();

    if let Some(SettingValue::Hotkey(binding)) = stored {
        return binding;
    }

    registry::hotkey_defs()
        .first()
        .map(|def| def.default.clone())
        .unwrap_or(HotkeyBinding {
            modifiers: vec![KeyModifier::Option],
            key: "Space".to_string(),
        })
}

/**
 * WHAT:  Converts our binding into the plugin's Shortcut type.
 * WHY:   Codes are matched explicitly rather than parsed from a string, so an
 *        unbindable key is a typed error at registration instead of a shortcut
 *        that silently never fires.
 * WHERE: register_hotkeys.
 */
fn to_shortcut(binding: &HotkeyBinding) -> AppResult<Shortcut> {
    let mut modifiers = Modifiers::empty();
    for modifier in &binding.modifiers {
        modifiers |= match modifier {
            KeyModifier::Command => Modifiers::SUPER,
            KeyModifier::Control => Modifiers::CONTROL,
            KeyModifier::Option => Modifiers::ALT,
            KeyModifier::Shift => Modifiers::SHIFT,
        };
    }

    let code = code_from_name(&binding.key).ok_or_else(|| {
        AppError::new(
            ErrorCode::HotkeyRegistrationFailed,
            "That key cannot be used as a shortcut.",
        )
        .recoverable()
        .with_detail(format!("unmapped key `{}`", binding.key))
    })?;

    Ok(Shortcut::new(Some(modifiers), code))
}

/// Maps a `KeyboardEvent.code` name onto the plugin's Code. Covers what a user
/// can realistically bind; anything else is refused rather than guessed at.
pub(crate) fn code_from_name(name: &str) -> Option<Code> {
    use Code::*;
    Some(match name {
        "Space" => Space,
        "Enter" => Enter,
        "Tab" => Tab,
        "Backquote" => Backquote,
        "Backslash" => Backslash,
        "Semicolon" => Semicolon,
        "Quote" => Quote,
        "Comma" => Comma,
        "Period" => Period,
        "Slash" => Slash,
        "F1" => F1, "F2" => F2, "F3" => F3, "F4" => F4, "F5" => F5, "F6" => F6,
        "F7" => F7, "F8" => F8, "F9" => F9, "F10" => F10, "F11" => F11, "F12" => F12,
        "F13" => F13, "F14" => F14, "F15" => F15, "F16" => F16, "F17" => F17,
        "F18" => F18, "F19" => F19,
        "KeyA" => KeyA, "KeyB" => KeyB, "KeyC" => KeyC, "KeyD" => KeyD, "KeyE" => KeyE,
        "KeyF" => KeyF, "KeyG" => KeyG, "KeyH" => KeyH, "KeyI" => KeyI, "KeyJ" => KeyJ,
        "KeyK" => KeyK, "KeyL" => KeyL, "KeyM" => KeyM, "KeyN" => KeyN, "KeyO" => KeyO,
        "KeyP" => KeyP, "KeyQ" => KeyQ, "KeyR" => KeyR, "KeyS" => KeyS, "KeyT" => KeyT,
        "KeyU" => KeyU, "KeyV" => KeyV, "KeyW" => KeyW, "KeyX" => KeyX, "KeyY" => KeyY,
        "KeyZ" => KeyZ,
        "Digit0" => Digit0, "Digit1" => Digit1, "Digit2" => Digit2, "Digit3" => Digit3,
        "Digit4" => Digit4, "Digit5" => Digit5, "Digit6" => Digit6, "Digit7" => Digit7,
        "Digit8" => Digit8, "Digit9" => Digit9,
        _ => return None,
    })
}

/**
 * SOURCE OF TRUTH KEYWORDS: show_first_window
 * WHAT:  Opens onboarding on a fresh install, and nothing at all afterwards.
 * WHY:   Murmur is invisible until summoned — that is the product. So a normal
 *        launch shows NO window; the app lives in the menu bar and the only
 *        thing a user ever sees is the pill. Onboarding is the one exception,
 *        and it is gated on a setting rather than on whether a model exists, so
 *        that someone who deliberately deleted their model is not dragged back
 *        through setup.
 * WHERE: The last step of setup.
 */
fn show_first_window(app: &AppHandle, db: &Database) -> AppResult<()> {
    let complete = matches!(
        services::settings::get_setting(db, keys::ONBOARDING_COMPLETE).ok().flatten(),
        Some(SettingValue::Bool(true))
    );

    if complete {
        return Ok(());
    }

    if let Some(window) = app.get_webview_window(ONBOARDING_WINDOW) {
        // Activate the PROCESS first. An accessory app is never frontmost, so
        // a window it shows is not ordered onto the screen — it exists, its
        // webview runs, and the user sees nothing. Same failure the tray menu
        // had; see tray::show_dashboard.
        #[cfg(target_os = "macos")]
        let _ = app.show();

        let _ = window.show();
        let _ = window.set_focus();
        tracing::info!("onboarding opened");
    }
    Ok(())
}

/**
 * SOURCE OF TRUTH KEYWORDS: show_dashboard_on_launch, MURMUR_SHOW_DASHBOARD
 * WHAT:  Opens the dashboard at launch when the environment asks for it.
 * WHY:   Murmur is invisible until summoned, so nothing opens the dashboard on
 *        a normal run — which also means an automated session cannot get at it,
 *        because the only affordance is a tray click. This gives a way in
 *        WITHOUT changing the shipped behaviour: absent the variable, nothing
 *        happens at all.
 * WHERE: Read once at the end of setup.
 */
fn show_dashboard_on_launch(app: &AppHandle) {
    if std::env::var("MURMUR_SHOW_DASHBOARD").is_err() {
        return;
    }
    tracing::info!("MURMUR_SHOW_DASHBOARD set — opening the dashboard");
    crate::tray::show_dashboard(app);
}


/**
 * SOURCE OF TRUTH KEYWORDS: set_escape_registered, escape_shortcut,
 *   on_escape, dynamic_escape
 * WHAT:  Binds Escape while a recording is live, and releases it the moment
 *        the recording ends.
 * WHY:   Escape belongs to whatever app the user is actually working in. A
 *        global grab held for the process lifetime would break the Escape key
 *        everywhere — closing a dialog, leaving a text field, exiting vim —
 *        for the sake of a key we need for a few seconds at a time. So it is
 *        registered on entering a capturing state and unregistered on leaving
 *        one, and the registration is idempotent because state changes emit
 *        more often than they transition.
 * WHERE: Called by the session actor from emit_state.
 */
pub fn set_escape_registered(app: &AppHandle, registered: bool) {
    let shortcut = Shortcut::new(None, Code::Escape);
    let manager = app.global_shortcut();

    if registered {
        if manager.is_registered(shortcut) {
            return;
        }
        let handler_app = app.clone();
        if let Err(err) = manager.on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            on_escape(&handler_app);
        }) {
            tracing::warn!(error = %err, "could not bind Escape for cancellation");
        }
        return;
    }

    if manager.is_registered(shortcut) {
        if let Err(err) = manager.unregister(shortcut) {
            tracing::warn!(error = %err, "could not release Escape");
        }
    }
}

/**
 * WHAT:  Turns an Escape press into arm-cancel or abort-cancel.
 * WHY:   The first Escape arms a countdown; the second aborts it and resumes.
 *        Deciding which from the CURRENT state keeps the FSM's vocabulary about
 *        what happened rather than about how many times a key was pressed.
 * WHERE: The dynamically registered Escape handler.
 */
fn on_escape(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let state = state.inner().clone();

    tauri::async_runtime::spawn(async move {
        let event = match state.current_state() {
            crate::types::SessionState::CancelPending { .. } => SessionEvent::CancelAborted,
            _ => SessionEvent::CancelArmed,
        };
        let _ = state.session.send(event).await;
    });
}


/**
 * SOURCE OF TRUTH KEYWORDS: watch_permissions, permissions_without_restart
 * WHAT:  Watches the OS grants and pushes them to the windows when one changes.
 * WHY:   Nothing tells an app that its permissions moved. The grant happens in
 *        System Settings, which is a different process, and macOS sends no
 *        notification — so an app either polls or stays wrong.
 *
 *        The UI used to re-check on window focus. That is the obvious answer
 *        and it is not enough here: Murmur is an accessory app that usually has
 *        NO window on screen when the switch is flipped, so there is no focus
 *        event to hang it on. The operator's report was exactly this — grant it,
 *        come back, and the app still says it is missing.
 *
 *        Polling is the honest mechanism rather than a workaround: both checks
 *        are cheap local calls (AXIsProcessTrusted, and an AVFoundation status
 *        read that does not touch the device), and there is no event to
 *        subscribe to. It emits only on CHANGE, so a stable machine costs one
 *        comparison a second and no IPC at all.
 *
 *        It keeps watching after everything is granted, at a slower cadence,
 *        because a permission can be REVOKED while the app runs and an app that
 *        only ever learns good news would go on believing it can paste.
 * WHERE: Started once by setup.
 */
fn watch_permissions(app: &AppHandle) {
    use crate::ipc::commands::system::PermissionReport;
    use crate::ipc::events::PermissionsChanged;
    use std::time::Duration;
    use tauri_specta::Event as _;

    /// While something is missing the user is probably in System Settings
    /// right now, so the screen has to keep up with them.
    const EAGER: Duration = Duration::from_millis(750);
    /// Once everything is granted this is only watching for a revocation.
    const RELAXED: Duration = Duration::from_secs(10);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last: Option<Vec<PermissionReport>> = None;

        loop {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            let permissions = Arc::clone(&state.ports.permissions);

            let reports: Vec<PermissionReport> =
                [OsPermission::Microphone, OsPermission::Accessibility]
                    .into_iter()
                    .map(|permission| PermissionReport {
                        permission,
                        state: permissions.check(permission),
                    })
                    .collect();

            let all_granted = reports.iter().all(|report| report.state.is_granted());

            if last.as_deref() != Some(reports.as_slice()) {
                if last.is_some() {
                    // Only from the second reading on: the first is the startup
                    // state, which report_permissions has already logged.
                    tracing::info!(?reports, "an OS permission changed");
                }
                let _ = (PermissionsChanged {
                    reports: reports.clone(),
                })
                .emit(&app);
                last = Some(reports);
            }

            tokio::time::sleep(if all_granted { RELAXED } else { EAGER }).await;
        }
    });
}

/**
 * SOURCE OF TRUTH KEYWORDS: attach_sidebar, addChildWindow, detached_rail
 * WHAT:  Sizes the navigation rail, places it to the left of the dashboard, and
 *        makes macOS responsible for keeping it there.
 * WHY:   The rail is a separate WINDOW because the gap between it and the main
 *        panel has to be a real hole showing the desktop. One window cannot do
 *        that — its vibrancy is a single rectangle covering the whole frame, so
 *        the gap would be glass rather than nothing.
 *
 *        `addChildWindow` is the load-bearing call. It makes AppKit move, order,
 *        minimise and hide the rail with its parent, which is the entire reason
 *        this is viable: tracking the dashboard ourselves on move and resize
 *        events is the version that lags a frame behind every drag and flickers
 *        on a zoom.
 *
 *        The HEIGHT is computed from the same design tokens the rail is drawn
 *        with, and from the number of nav entries in the registry, rather than
 *        being a number kept in step by hand. Add a capability with a nav entry
 *        and the window grows by exactly one item with nothing to remember —
 *        which is the same promise the registry makes everywhere else.
 * WHERE: Called once by setup, after the windows exist.
 */
fn attach_sidebar(app: &AppHandle) {
    use crate::tray::design_token;

    let (Some(rail), Some(dashboard)) = (
        app.get_webview_window(SIDEBAR_WINDOW),
        app.get_webview_window(DASHBOARD_WINDOW),
    ) else {
        tracing::warn!("the rail or the dashboard is missing; leaving them unattached");
        return;
    };

    let nav_items = crate::registry::CAPABILITIES
        .iter()
        .filter(|capability| capability.nav.is_some())
        .count() as f64;

    let width = design_token("--rail-width");
    let padding = design_token("--rail-padding");
    let height = 2.0 * padding
        + design_token("--mark-size-md")
        + design_token("--rail-mark-gap")
        + nav_items * design_token("--rail-item-size")
        + (nav_items - 1.0).max(0.0) * design_token("--rail-item-gap");

    if let Err(err) = rail.set_size(tauri::LogicalSize::new(width, height)) {
        tracing::warn!(error = %err, "could not size the rail");
    }


    let _ = dashboard;
    tracing::info!(nav_items, width, height, "rail sized from the design tokens");
}

/**
 * SOURCE OF TRUTH KEYWORDS: attach_rail, addChildWindow_orders_it_in
 * WHAT:  Makes the rail a child of the dashboard, once, the first time the
 *        dashboard is actually on screen.
 * WHY:   `addChildWindow` DISPLAYS the child. That is the whole bug this
 *        function exists to avoid: attaching during setup — before the
 *        dashboard has been shown — put the rail on screen immediately, at its
 *        default position, hanging beside nothing. It appeared at the bottom of
 *        the display with no window near it, which reads as a placement bug and
 *        is a lifecycle one.
 *
 *        So it is attached only after its parent exists on screen and the rail
 *        has been placed beside it. From then on AppKit owns the relationship:
 *        move, order, minimise and hide all follow the parent for free.
 * WHERE: Called by tray::show_dashboard, after place_rail.
 */
pub fn attach_rail(app: &AppHandle) {
    // Attaching twice is not harmful, but it is not free either, and a second
    // call re-orders the window for no reason.
    use std::sync::atomic::{AtomicBool, Ordering};
    static ATTACHED: AtomicBool = AtomicBool::new(false);
    if ATTACHED.swap(true, Ordering::SeqCst) {
        return;
    }

    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2_app_kit::{NSWindow, NSWindowOrderingMode};

        let (Some(rail), Some(dashboard)) = (
            app.get_webview_window(SIDEBAR_WINDOW),
            app.get_webview_window(DASHBOARD_WINDOW),
        ) else {
            return;
        };

        let (Ok(child), Ok(parent)) = (rail.ns_window(), dashboard.ns_window()) else {
            return;
        };

        // SAFETY: both handles come from Tauri for windows that exist, and this
        // runs on the main thread from a Tauri event.
        unsafe {
            let child = child as *mut NSWindow;
            let parent = parent as *mut NSWindow;
            let (Some(child), Some(parent)) = (Retained::retain(child), Retained::retain(parent))
            else {
                return;
            };
            parent.addChildWindow_ordered(&child, NSWindowOrderingMode::Above);
        }

        tracing::info!("rail attached to the dashboard");
    }

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/**
 * SOURCE OF TRUTH KEYWORDS: keep_rail_centred, resize
 * WHAT:  Re-centres the rail when the dashboard changes height.
 * WHY:   AppKit carries a child window at a FIXED OFFSET when the parent moves,
 *        which is most of what we want and is why this is a child window at
 *        all. A resize is the case it cannot cover: growing the window
 *        downwards leaves the rail level with the old midline, so the thing
 *        drifts off centre exactly when someone is looking at it.
 * WHERE: Registered once by setup.
 */
fn keep_rail_centred(app: &AppHandle) {
    let handle = app.clone();
    if let Some(dashboard) = app.get_webview_window(DASHBOARD_WINDOW) {
        dashboard.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Resized(_)) {
                place_rail(&handle);
            }
        });
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: place_rail, beside_not_over, vertically_centred
 * WHAT:  Puts the rail immediately to the LEFT of the dashboard, centred on its
 *        vertical midline.
 * WHY:   Called every time the dashboard is shown, and NOT once at startup,
 *        which is the bug this replaces. `attach_sidebar` runs during setup —
 *        before `show_first_window` has placed and centred the dashboard — so
 *        reading its position then returns coordinates it does not have yet.
 *        The rail was pinned to that stale origin and the dashboard then moved
 *        out from under it, which put the rail ON TOP of the window instead of
 *        beside it. It looked like a layout mistake and was a timing one.
 *
 *        Vertically CENTRED rather than top-aligned because the rail is a
 *        widget hanging beside the app, not a second column of it — the thing
 *        it is modelled on floats against the middle of the window's edge.
 *
 *        Only the initial placement is ours. Once the parent moves, AppKit
 *        carries the child at a fixed offset, which is exactly why this is a
 *        child window; re-running it on every drag would fight that.
 * WHERE: Called by tray::show_dashboard, and on a dashboard resize.
 */
pub fn place_rail(app: &AppHandle) {
    let (Some(rail), Some(dashboard)) = (
        app.get_webview_window(SIDEBAR_WINDOW),
        app.get_webview_window(DASHBOARD_WINDOW),
    ) else {
        return;
    };

    let (Ok(origin), Ok(parent), Ok(size), Ok(scale)) = (
        dashboard.outer_position(),
        dashboard.outer_size(),
        rail.outer_size(),
        dashboard.scale_factor(),
    ) else {
        return;
    };

    let gap = (crate::tray::design_token("--rail-detach-gap") * scale).round() as i32;
    let x = origin.x - size.width as i32 - gap;
    let y = origin.y + (parent.height as i32 - size.height as i32) / 2;

    tracing::info!(
        dash_x = origin.x,
        dash_y = origin.y,
        dash_w = parent.width,
        dash_h = parent.height,
        rail_w = size.width,
        rail_h = size.height,
        scale,
        gap,
        target_x = x,
        target_y = y,
        "placing the rail"
    );

    if let Err(err) = rail.set_position(tauri::PhysicalPosition::new(x, y)) {
        tracing::warn!(error = %err, "could not place the rail beside the dashboard");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * SOURCE OF TRUTH KEYWORDS: the_warm_up_never_downloads
     * WHAT:  Startup warm-up acts only on a model that is already on disk.
     * WHY:   The failure this guards is silent and expensive, and only happens
     *        on a machine nobody re-tests: a FRESH install. The warm-up used to
     *        call `ensure`, which downloads, so 574MB of the default model
     *        started arriving before onboarding had asked which model the user
     *        wanted. Choose a different one and they paid for two.
     *
     *        `NotDownloaded` is the only state that must be false. If that ever
     *        flips, the app silently starts pre-empting the user's choice again
     *        and the only symptom is a bandwidth bill.
     */
    #[test]
    fn the_startup_warm_up_never_acts_on_a_missing_model() {
        use crate::types::ModelState;

        assert!(
            !worth_warming(&ModelState::NotDownloaded),
            "the warm-up would download a model the user has not chosen yet"
        );

        for state in [
            ModelState::Ready,
            ModelState::Verifying,
            ModelState::Optimizing,
            ModelState::Failed {
                message: "a stale verdict on a file that may since have changed".into(),
            },
        ] {
            assert!(
                worth_warming(&state),
                "{state:?} has a file on disk and should be verified at startup"
            );
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: constructors_do_not_need_a_runtime,
     *   aborted_on_launch
     * WHAT:  Everything bootstrap builds can be built with NO async runtime
     *        present.
     * WHY:   A build shipped today passed 262 tests, clippy and tsc, and then
     *        aborted the instant it launched: a constructor called
     *        `tokio::spawn` before the runtime existed. Every test passed
     *        because `#[tokio::test]` provides a runtime the real app does not
     *        have at that point in setup — so the suite was testing a world
     *        that only exists inside the suite.
     *
     *        This is a plain test with no tokio attribute. There is no ambient
     *        runtime, so any
     *        `spawn` reached from these constructors panics HERE, at the exact
     *        line, instead of at launch on the operator's machine.
     *
     *        It does not prove the app starts — nothing here can, without
     *        launching it — and it deliberately covers only the constructors a
     *        test can reach without a Tauri AppHandle. What it closes is the
     *        specific class that got through: work scheduled at construction
     *        time rather than at run time.
     */
    #[test]
    fn constructors_do_not_need_an_async_runtime() {
        use crate::adapters::cpal::CpalAudioSource;
        use crate::adapters::macos::{MacosInjector, MacosPermissions};
        use crate::adapters::rules::RuleEnhancer;
        use crate::pipeline::Chunker;
        use crate::session::SessionSettings;

        // Ports.
        let _audio = CpalAudioSource::new();
        let _enhancer = RuleEnhancer::new();
        let _permissions = MacosPermissions::new();
        let _injector = MacosInjector::new(MacosPermissions::new());

        // Pipeline and session pieces.
        let _chunker = Chunker::new();
        let _settings = SessionSettings::defaults();
        let _machine = crate::session::SessionMachine::new(3_000);
        let _latency = crate::telemetry::LatencyRecorder::new();

        // The database, which setup opens before anything is spawned.
        let db = Database::open_in_memory().expect("in-memory database");
        let _from_db = SessionSettings::load(&db);
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: the_stop_chime_is_not_in_the_delivery_worker
     * WHAT:  The delivery worker plays no sound.
     * WHY:   The operator's report: "the turn off sound effect right now
     *        happens when the paste takes place. Instead, the second I turn off
     *        the recording, the sound effects should play." Delivery runs after
     *        the decode, a second or two later, so a chime there is a
     *        confirmation of something that already finished.
     *
     *        Asserted against the source because there is nothing to assert at
     *        runtime — playing a sound has no return value and no state. This is
     *        crude and it is the only thing that would catch the chime being
     *        moved back, which is an easy and reasonable-looking edit.
     */
    #[test]
    fn the_stop_chime_is_not_in_the_delivery_worker() {
        let delivery = include_str!("session/delivery.rs");
        let code: String = delivery
            .lines()
            .filter(|line| {
                let t = line.trim_start();
                !t.starts_with("//") && !t.starts_with('*') && !t.starts_with("/*")
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !code.contains("play_feedback"),
            "the delivery worker plays a sound again; the stop chime belongs on \
             the transition out of capture, in the actor"
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: orphan_recovery_runs_before_the_app_is_reachable
     * WHAT:  `recover_orphans` is called before `app.manage` and before the
     *        session actor is spawned.
     * WHY:   This is an ORDERING guarantee, and ordering has no runtime
     *        assertion — the failure only appears when a webview happens to
     *        invoke start_recording during setup, which is timing-dependent and
     *        will never reproduce on demand. So it is asserted against the
     *        source, crudely and reliably.
     *
     *        What the wrong order does: `manage` makes every IPC command
     *        reachable and the three windows exist before setup runs, so a
     *        session row can be written while setup is still going. Orphan
     *        recovery then marks that LIVE session as crashed, because a row
     *        with no `ended_at` is exactly what a crash leaves behind. The user
     *        is mid-sentence and their session is already in History as
     *        interrupted.
     *
     *        Recovering crashed sessions is only meaningful while no session
     *        can yet exist. That is a fact about the ORDER, so the order is
     *        what gets tested.
     */
    #[test]
    fn orphan_recovery_runs_before_anything_can_start_a_session() {
        let source = include_str!("bootstrap.rs");
        let setup = source
            .split_once("pub fn setup(app: &AppHandle)")
            .expect("setup exists")
            .1;

        let recover = setup.find("recover_orphans(&db)").expect("setup recovers orphans");
        let manage = setup.find("app.manage(state").expect("setup manages state");
        let spawn = setup
            .find("spawn(actor.run(")
            .expect("setup spawns the actor");

        assert!(
            recover < manage,
            "recover_orphans runs after app.manage, so it can mark a live session orphaned"
        );
        assert!(
            recover < spawn,
            "recover_orphans runs after the actor is spawned, so it can race a real session"
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: single_instance_is_first
     * WHAT:  The single-instance plugin is registered before every other plugin.
     * WHY:   Its own documentation requires it, and the reason is that a second
     *        process must be turned away BEFORE it runs any setup. Registered
     *        late, the second process has already opened the same SQLite file
     *        and done damage to the first process's data by the time it is told
     *        to quit.
     *
     *        Untestable at runtime without launching two copies of the app, so
     *        the position in the builder chain is what gets asserted. A future
     *        edit that adds a plugin above it fails here rather than in the
     *        field.
     */
    #[test]
    fn the_single_instance_plugin_is_registered_before_any_other() {
        let source = include_str!("lib.rs");
        let chain = source
            .split_once("tauri::Builder::default()")
            .expect("the builder chain exists")
            .1;

        let single = chain
            .find("tauri_plugin_single_instance::init")
            .expect("the single-instance plugin must be registered");

        // Every other `.plugin(` call must come after it.
        for (offset, _) in chain.match_indices(".plugin(") {
            let named = &chain[offset..];
            if named.starts_with(".plugin(tauri_plugin_single_instance") {
                continue;
            }
            assert!(
                offset > single,
                "a plugin is registered before single-instance; a second launch would run \
                 part of setup against the first process's database before being refused"
            );
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: retention_zero_keeps_everything
     * WHAT:  Retention of 0 deletes nothing; a positive window deletes behind
     *        it.
     * WHY:   0 is the registry default and the description promises it keeps
     *        everything forever. An off-by-one here does not fail loudly — it
     *        erases a user's entire transcript history on the next sweep, in an
     *        app whose whole promise is that their speech is theirs. Worth a
     *        test that cannot be argued with.
     */
    #[test]
    fn a_retention_window_of_zero_deletes_nothing() {
        assert_eq!(retention_cutoff(0, 1_000_000), None);
        // No UI can produce this; a hand-edited database can, and the wrong
        // answer is a cutoff in the FUTURE, which deletes everything.
        assert_eq!(retention_cutoff(-7, 1_000_000), None);
    }

    #[test]
    fn a_retention_window_cuts_off_exactly_that_many_days_back() {
        let day = 24 * 60 * 60 * 1000;
        assert_eq!(retention_cutoff(1, 10 * day), Some(9 * day));
        assert_eq!(retention_cutoff(30, 100 * day), Some(70 * day));
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: retention_reads_the_registry_default
     * WHAT:  With nothing stored, the window is whatever the registry declares.
     * WHY:   The same rule `updates_enabled` follows: the default lives in one
     *        place. A literal here would let the declared default and the
     *        applied default drift, and the drift is invisible until someone's
     *        history is either kept forever or deleted early.
     */
    #[test]
    fn the_retention_window_falls_back_to_the_registry() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        let declared = match registry::setting_def(keys::RETENTION_DAYS).map(|d| d.default.clone()) {
            Some(SettingValue::Number(days)) => days as i64,
            other => panic!("retention_days must be declared as a Number, got {other:?}"),
        };
        assert_eq!(retention_days(&db), declared);

        services::settings::set_setting(
            &db,
            keys::RETENTION_DAYS,
            &SettingValue::Number(14.0),
            1,
        )?;
        assert_eq!(retention_days(&db), 14);
        Ok(())
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: rebind_reads_the_new_binding
     * WHAT:  After a hotkey is written, `dictation_binding` returns the NEW one.
     * WHY:   This is the read the rebind listener depends on. Registering the
     *        new accelerator is a call into the OS that a unit test cannot make,
     *        but the half that was actually broken is this one — nothing ever
     *        re-read the value, so the app kept listening for the old key while
     *        displaying the new one.
     */
    #[test]
    fn a_written_hotkey_is_what_the_rebind_would_register() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        let before = dictation_binding(&db);

        let rebound = HotkeyBinding {
            modifiers: vec![KeyModifier::Control, KeyModifier::Shift],
            key: "KeyD".to_string(),
        };
        assert_ne!(before.to_accelerator(), rebound.to_accelerator());

        services::settings::set_setting(
            &db,
            keys::DICTATION_HOTKEY,
            &SettingValue::Hotkey(rebound.clone()),
            1,
        )?;

        let after = dictation_binding(&db);
        assert_eq!(after.to_accelerator(), rebound.to_accelerator());
        assert!(
            to_shortcut(&after).is_ok(),
            "a rebind must produce something registrable"
        );
        Ok(())
    }

    #[test]
    fn the_default_binding_is_the_one_the_registry_declares() {
        let def = registry::hotkey_defs();
        let first = def.first().expect("dictation declares a hotkey");
        assert_eq!(first.default.key, "Space");
        assert!(first.default.modifiers.contains(&KeyModifier::Option));
    }

    #[test]
    fn the_default_binding_converts_to_a_registrable_shortcut() {
        let binding = registry::hotkey_defs()[0].default.clone();
        assert!(to_shortcut(&binding).is_ok());
    }

    #[test]
    fn an_unmappable_key_is_refused_rather_than_silently_ignored() {
        // A shortcut that registers but never fires is the worst outcome —
        // the user rebinds, sees no error, and nothing works.
        let binding = HotkeyBinding {
            modifiers: vec![KeyModifier::Option],
            key: "NotAKey".into(),
        };
        let err = to_shortcut(&binding).expect_err("must refuse");
        assert_eq!(err.code, ErrorCode::HotkeyRegistrationFailed);
        assert!(err.recoverable, "the user can pick another key");
    }

    #[test]
    fn every_modifier_maps_onto_a_real_one() {
        let binding = HotkeyBinding {
            modifiers: vec![
                KeyModifier::Command,
                KeyModifier::Control,
                KeyModifier::Option,
                KeyModifier::Shift,
            ],
            key: "KeyD".into(),
        };
        assert!(to_shortcut(&binding).is_ok());
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: microphone_entitlement, hardened_runtime
     * WHAT:  The signed bundle must declare the audio-input entitlement.
     * WHY:   Signing with a real identity enables Hardened Runtime, and under
     *        it the microphone is blocked at the OS level without this key —
     *        no matter what the user has granted in Privacy & Security. The app
     *        then records SILENCE: the device opens, the pipeline runs, and the
     *        transcript is empty. Nothing reports an error, which is what makes
     *        it so expensive to diagnose.
     *
     *        Asserted against the checked-in plist rather than the built bundle
     *        so it fails in `cargo test` rather than after someone has shipped
     *        and tried to talk to it.
     * WHERE: Guards entitlements.plist and its reference in tauri.conf.json.
     */
    #[test]
    fn the_bundle_declares_the_microphone_entitlement() {
        let entitlements = include_str!("../entitlements.plist");
        assert!(
            entitlements.contains("com.apple.security.device.audio-input"),
            "entitlements.plist must declare audio-input or the app records silence"
        );

        let config = include_str!("../tauri.conf.json");
        assert!(
            config.contains("\"entitlements\": \"entitlements.plist\""),
            "tauri.conf.json must reference entitlements.plist or it is never applied"
        );
    }

    #[test]
    fn window_labels_match_the_configuration() {
        // A typo here fails at runtime, when a window silently does not appear.
        let config = include_str!("../tauri.conf.json");
        for label in [DASHBOARD_WINDOW, PILL_WINDOW, ONBOARDING_WINDOW] {
            assert!(
                config.contains(&format!("\"label\": \"{label}\"")),
                "window `{label}` is not declared in tauri.conf.json"
            );
        }
    }
}
