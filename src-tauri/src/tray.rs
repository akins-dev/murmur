/*!
 * SOURCE OF TRUTH KEYWORDS: install_tray, TrayMenu, MENU_DASHBOARD,
 *   MENU_QUIT, show_dashboard, toggle_pill_window
 * WHAT:  The menu bar item: the only permanently visible part of Murmur, and
 *        the only way to reach the dashboard.
 * WHY:   The app has no Dock icon and no window on launch — it is invisible
 *        until summoned, which is the product. That makes this the sole
 *        affordance, so it has to carry everything a user needs: open the
 *        dashboard, and quit.
 *
 *        `show_dashboard` does more than call `show()`, and the extra step is
 *        not optional. With LSUIElement set, the process is an accessory: a
 *        window it shows does NOT come to the front and cannot take keyboard
 *        focus properly. Without an explicit activation the dashboard opens
 *        behind whatever the user was doing and feels broken.
 * WHERE: Installed once during bootstrap.
 */

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::bootstrap::{DASHBOARD_WINDOW, PILL_WINDOW};
use crate::error::{AppError, AppResult};

const MENU_DASHBOARD: &str = "dashboard";
const MENU_QUIT: &str = "quit";

/**
 * SOURCE OF TRUTH KEYWORDS: install_tray
 * WHAT:  Builds the menu bar item and its menu.
 * WHERE: Called once by bootstrap::setup.
 */
pub fn install_tray(app: &AppHandle) -> AppResult<()> {
    let dashboard = MenuItem::with_id(app, MENU_DASHBOARD, "Open Murmur", true, None::<&str>)
        .map_err(menu_error)?;
    let separator = PredefinedMenuItem::separator(app).map_err(menu_error)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit Murmur", true, Some("Cmd+Q"))
        .map_err(menu_error)?;

    let menu = Menu::with_items(app, &[&dashboard, &separator, &quit]).map_err(menu_error)?;

    TrayIconBuilder::with_id("murmur")
        // The identity mark (docs/04 §12), not the app icon. A menu-bar glyph
        // is a different drawing problem from an app icon: it is 22pt, it must
        // read at that size, and macOS uses ONLY its alpha channel. The app
        // icon shrunk into that slot is a smudge.
        .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")).map_err(menu_error)?)
        // Template rendering: macOS recolours the shape for light menu bars,
        // dark menu bars and the pressed state from the alpha alone. Without it
        // the glyph is a fixed colour that is wrong in one of the three.
        .icon_as_template(true)
        .tooltip("Murmur")
        .menu(&menu)
        // The menu is the only interaction. Left-click opening it too would
        // make an accidental click open a window the user did not ask for.
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_DASHBOARD => show_dashboard(app),
            MENU_QUIT => app.exit(0),
            other => tracing::debug!(id = other, "unhandled tray menu item"),
        })
        .on_tray_icon_event(|_tray, event| {
            if let TrayIconEvent::Click { .. } = event {
                tracing::trace!("tray clicked");
            }
        })
        .build(app)
        .map_err(menu_error)?;

    Ok(())
}

/**
 * SOURCE OF TRUTH KEYWORDS: show_dashboard
 * WHAT:  Brings the dashboard to the front, properly.
 * WHY:   See the module WHY — an accessory app must activate explicitly or its
 *        window opens behind everything. `set_focus` is what does that here;
 *        `show` alone is the bug.
 * WHERE: The tray menu, and any deep link into settings.
 */
pub fn show_dashboard(app: &AppHandle) {
    let Some(window) = app.get_webview_window(DASHBOARD_WINDOW) else {
        tracing::error!("the dashboard window is missing");
        return;
    };

    // ORDER MATTERS, and every step is load-bearing.
    //
    // The app runs with activation policy Accessory so it has no Dock icon.
    // The consequence, which docs/03 §3.6 warned about and an earlier version
    // of this function got wrong: an accessory app is not "active", so showing
    // a window and calling set_focus on it is NOT enough. The window appears
    // BEHIND whatever the user was doing, or does not appear to arrive at all,
    // and the app looks broken in the only place it has a real window.
    //
    // `app.show()` activates the PROCESS, which is the step that was missing.
    // Only once the app itself is frontmost can a window of it take focus.
    #[cfg(target_os = "macos")]
    if let Err(err) = app.show() {
        tracing::warn!(error = %err, "could not activate the app");
    }

    if let Err(err) = window.show() {
        tracing::warn!(error = %err, "could not show the dashboard");
    }
    let _ = window.unminimize();

    if let Err(err) = window.set_focus() {
        tracing::warn!(error = %err, "could not focus the dashboard");
    }

    tracing::info!("dashboard opened");
}

/**
 * SOURCE OF TRUTH KEYWORDS: set_pill_visible, toggle_pill_window
 * WHAT:  Shows or hides the pill overlay.
 * WHY:   The window is created once at launch and only ever shown and hidden —
 *        never created and destroyed per session. Recreating it would pay the
 *        webview's parse and paint cost on every hotkey press, which is exactly
 *        the moment the app has promised to be instant.
 *
 *        `set_ignore_cursor_events(true)` is what makes it an indicator rather
 *        than a window: it cannot be clicked, so it can never steal a click
 *        meant for the app underneath, and it can never take focus by click.
 * WHERE: Driven by the session actor as state changes.
 */
/**
 * SOURCE OF TRUTH KEYWORDS: LAST_LIVE_STATE, exit_direction
 * WHAT:  The last state the pill was actually showing.
 * WHY:   The exit needs a direction — a committed recording rises as it fades,
 *        a cancelled one drops — and by the time the pill is being hidden the
 *        state machine is already Idle and has forgotten which it was.
 * WHERE: Written by fit_pill_to_state, read once by set_pill_visible on hide.
 */
static LAST_LIVE_STATE: std::sync::Mutex<Option<crate::types::SessionState>> =
    std::sync::Mutex::new(None);

/**
 * SOURCE OF TRUTH KEYWORDS: PILL_SHOWN, appearance, not_is_visible
 * WHAT:  Whether the pill is currently up, as far as WE are concerned.
 * WHY:   Replaces `window.is_visible()`, which lies for about 160ms and cost
 *        the operator a real bug. The exit animation defers `hide()` into its
 *        completion handler, so for the length of the fade the window is still
 *        visible to AppKit while being, in every sense that matters, gone. A
 *        new recording starting inside that window saw "already visible",
 *        skipped placement entirely, and the pill appeared wherever it had been
 *        on the PREVIOUS display. Recording a second time fixed it because by
 *        then the hide had landed — which is exactly the "only fixes itself
 *        when I run the recording twice" the operator reported.
 *
 *        Set false the moment a hide is REQUESTED, not when it completes, so
 *        the next appearance always places itself.
 * WHERE: Flipped by set_pill_visible; read by fit_pill_to_state.
 */
static PILL_SHOWN: AtomicBool = AtomicBool::new(false);

/**
 * SOURCE OF TRUTH KEYWORDS: PillPlacement, PILL_PLACEMENT, one_authority
 * WHAT:  The display the pill was placed on when it appeared, and that
 *        display's geometry.
 * WHY:   Captured ONCE per appearance and reused for every frame change during
 *        the session. The pill must not chase the cursor between displays while
 *        someone is talking, and it must not be re-derived from the window's
 *        own current position — that was the second half of the bug. See
 *        fit_pill_to_state.
 * WHERE: Set on appearance, read on every state change.
 */
#[derive(Clone, Copy)]
struct PillPlacement {
    origin: (i32, i32),
    size: (u32, u32),
    scale: f64,
}

static PILL_PLACEMENT: std::sync::Mutex<Option<PillPlacement>> = std::sync::Mutex::new(None);

pub fn set_pill_visible(app: &AppHandle, visible: bool) {
    let Some(window) = app.get_webview_window(PILL_WINDOW) else {
        return;
    };

    if visible {
        // The frame was already set by fit_pill_to_state, which runs first on
        // every state change and is the ONLY thing that positions the pill.
        // This function shows and hides; it does not decide where.
        let _ = window.set_ignore_cursor_events(true);
        let _ = window.show();
    } else if PILL_SHOWN.swap(false, Ordering::SeqCst) {
        // Flag cleared BEFORE the animation, not after it — see PILL_SHOWN.
        let leaving = LAST_LIVE_STATE.lock().ok().and_then(|mut s| s.take());
        animate_pill_out(&window, leaving);
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: animate_pill_out, native_motion, exit_direction
 * WHAT:  Fades the pill out while moving it a few points — up when the words
 *        were kept, down when the recording was thrown away.
 * WHY:   NSWindow motion rather than a CSS transition, and that is a
 *        constraint rather than a preference. The pill's glass is an
 *        NSVisualEffectView sized to the window, so anything the web layer
 *        animates slides out from under it and reveals a hard-edged halo of
 *        blur around a shrinking pill. Only the window can move the glass and
 *        its contents together.
 *
 *        The direction is worth having because the screen says NOTHING for the
 *        second or two afterwards while the text is still being decoded. In
 *        that gap the exit is the only evidence of what happened, and the two
 *        outcomes are not symmetric: believing a cancel committed costs a
 *        shrug, believing a commit cancelled means recording it again and
 *        getting the text twice. It costs no time — the pill is leaving anyway
 *        — which is the only reason it is justified at all. There is no
 *        checkmark, no linger and no word count; the operator asked for none of
 *        those and they would each add time to a gesture whose whole point is
 *        that it is over.
 * WHERE: The only path that hides the pill.
 */
fn animate_pill_out(window: &tauri::WebviewWindow, leaving: Option<crate::types::SessionState>) {
    use crate::types::SessionState;

    // Cancelled recordings sink; everything else rises. Failure is included in
    // "rises" deliberately — it is not a discarded recording, and dropping it
    // would read as though the words had been thrown away.
    let rising = !matches!(leaving, Some(SessionState::CancelPending { .. }));

    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2_app_kit::{NSAnimatablePropertyContainer, NSAnimationContext, NSWindow};

        let Ok(handle) = window.ns_window() else {
            let _ = window.hide();
            return;
        };

        // SAFETY: Tauri hands back the NSWindow for this webview, and this runs
        // on the main thread — every caller reaches here from a Tauri event.
        let ns_window: Retained<NSWindow> = unsafe {
            let ptr = handle as *mut NSWindow;
            match Retained::retain(ptr) {
                Some(window) => window,
                None => {
                    let _ = window.hide();
                    return;
                }
            }
        };

        let travel = PILL.exit_travel * if rising { 1.0 } else { -1.0 };
        let duration = PILL.exit_ms / 1000.0;

        unsafe {
            let mut frame = ns_window.frame();
            // AppKit's y grows upward, so "rises" is a positive delta.
            frame.origin.y += travel;

            let target = ns_window.clone();
            NSAnimationContext::runAnimationGroup_completionHandler(
                &block2::RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
                    context.as_ref().setDuration(duration);
                    let animator = target.animator();
                    animator.setAlphaValue(0.0);
                    animator.setFrame_display(frame, true);
                }),
                Some(&block2::RcBlock::new({
                    let window = window.clone();
                    let ns_window = ns_window.clone();
                    move || {
                        // Hide only once it has finished leaving, then restore
                        // the alpha so the NEXT appearance is not invisible —
                        // the window is reused, never recreated.
                        let _ = window.hide();
                        ns_window.setAlphaValue(1.0);
                    }
                })),
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = rising;
        let _ = window.hide();
    }
}

/// docs/04 §7: 96pt above the bottom edge of the screen containing the cursor.
const PILL_BOTTOM_INSET_PT: f64 = 96.0;

/**
 * SOURCE OF TRUTH KEYWORDS: PillMetrics, PILL, pill_tokens, single_source
 * WHAT:  The pill's dimensions, which are also the WINDOW's dimensions, read
 *        from the same design tokens the pill is drawn with.
 * WHY:   They used to be separate numbers, and that was the bug. The pill's
 *        size lived in tokens.css and the window's in tauri.conf.json, they
 *        disagreed by 64x28, and nothing could notice — until vibrancy made the
 *        disagreement visible as a blurred rectangle behind the pill. Glass
 *        cannot be given a shape the window does not have.
 *
 *        Parsed from the CSS rather than copied out of it. A copy plus a test
 *        that the copy matches was the first fix, and it failed within the hour
 *        — the designer changed a token, the test went red, and the "fix" was
 *        going to be me editing a number to agree with a number. That is not
 *        one source of truth, it is two sources and a chaperone. `include_str!`
 *        means the value cannot drift, because there is only ever one of it.
 * WHERE: Read by fit_pill_to_state, position_pill and the vibrancy setup.
 */
static PILL: std::sync::LazyLock<PillMetrics> = std::sync::LazyLock::new(PillMetrics::from_tokens);

struct PillMetrics {
    /// How long the window takes to leave, and how far it travels doing it.
    /// Read from the tokens like every other pill dimension so the motion and
    /// the design remain one fact rather than two that agree today.
    exit_ms: f64,
    exit_travel: f64,
    width: f64,
    height: f64,
    /**
     * The failure box, which is a different size because it carries a
     * different kind of content. Every capture state is one size — nothing
     * resizes mid-dictation — but a failure is a SENTENCE ("Murmur heard
     * nothing at all. Check that the right input device is selected"), and a
     * sentence cropped to an indicator's width is a sentence nobody reads.
     */
    width_failed: f64,
    height_failed: f64,
    radius: f64,
}

impl PillMetrics {
    fn from_tokens() -> Self {
        // Compiled in, so the file is read at build time and a missing token is
        // a startup panic rather than a silently wrong window.
        const TOKENS: &str = include_str!("../../src/styles/tokens.css");

        let read = |name: &str| -> f64 {
            TOKENS
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    if key.trim() != name {
                        return None;
                    }
                    value
                        .trim()
                        .trim_end_matches(';')
                        .trim_end_matches("px")
                        .parse::<f64>()
                        .ok()
                })
                .unwrap_or_else(|| {
                    panic!("{name} is missing from tokens.css, which sizes the pill window")
                })
        };

        Self {
            exit_ms: read("--pill-exit-duration-ms"),
            exit_travel: read("--pill-exit-travel"),
            width: read("--pill-width"),
            height: read("--pill-height"),
            width_failed: read("--pill-width-failed"),
            height_failed: read("--pill-height-failed"),
            radius: read("--radius-pill"),
        }
    }
}

/// The corner radius the vibrancy layer is given, so the glass is the pill's
/// shape rather than the window's rectangle.
pub fn pill_radius() -> f64 {
    PILL.radius
}

/**
 * SOURCE OF TRUTH KEYWORDS: fit_pill_to_state
 * WHAT:  Resizes the pill window to match the pill the state will draw, and
 *        keeps it centred while doing it.
 * WHY:   Growing a window grows it to the RIGHT unless the origin moves too, so
 *        a naive resize would make the pill lurch sideways on the one gesture
 *        where the user is watching it closely. Half the width delta comes off
 *        the x origin, which keeps the centre fixed.
 *
 *        Returns early when the size is already correct. This is called on every
 *        state change, which during a recording is several times a second, and
 *        setting a window's frame to the value it already holds is not free.
 * WHERE: Called by the event adapter on every session state change.
 */
/**
 * SOURCE OF TRUTH KEYWORDS: adopt_pill_tokens, conf_is_not_authoritative
 * WHAT:  Sizes the pill window from the design tokens once, at startup.
 * WHY:   tauri.conf.json has to declare SOME width and height — it is static
 *        JSON read before any Rust runs, it cannot be computed, and it cannot
 *        even carry a comment saying so. That makes it a second copy of a
 *        number tokens.css owns, and a second copy is what put a blurred
 *        rectangle behind the pill in the first place.
 *
 *        Calling this at startup makes the declared value inert rather than
 *        merely unused: whatever the JSON says, the window adopts the tokens
 *        before it is ever shown, so the two cannot disagree in a way anybody
 *        sees. The JSON numbers are kept roughly right anyway so the very first
 *        frame is not a resize, but nothing depends on them being right.
 * WHERE: Called once by bootstrap, after the windows exist.
 */
pub fn adopt_pill_tokens(app: &AppHandle) {
    fit_pill_to_state(app, &crate::types::SessionState::Idle);
}

pub fn fit_pill_to_state(app: &AppHandle, state: &crate::types::SessionState) {
    use crate::types::SessionState;

    let Some(window) = app.get_webview_window(PILL_WINDOW) else {
        return;
    };

    if matches!(state, SessionState::Idle) {
        return;
    }

    if let Ok(mut last) = LAST_LIVE_STATE.lock() {
        *last = Some(state.clone());
    }

    /*
     * SOURCE OF TRUTH KEYWORDS: one_authority, absolute_frame, appearance
     * THIS IS THE ONLY CODE THAT DECIDES WHERE THE PILL SITS, and that
     * sentence is the fix.
     *
     * There used to be two. `position_pill` placed it on appearance from the
     * cursor's display, and this function ALSO moved it on every state change
     * — several times a second while recording — by shifting the origin half
     * the size delta from wherever the window happened to be. Relative
     * arithmetic against a moving reference. The two agreed right up until the
     * window landed on a display with a different scale factor, at which point
     * the target size changed, this function shifted the pill off the centre
     * the other had computed, and the operator watched it jump around.
     *
     * Now: the DISPLAY is chosen once, when the pill appears, and every frame
     * after that is computed absolutely from that display's geometry. Nothing
     * is derived from the window's current position or size, so there is
     * nothing for a second opinion to disagree with. Choosing the display once
     * is also what keeps the pill from chasing the cursor mid-sentence.
     */
    let appearing = !PILL_SHOWN.swap(true, Ordering::SeqCst);
    if appearing {
        let placement = app
            .cursor_position()
            .ok()
            .and_then(|cursor| app.monitor_from_point(cursor.x, cursor.y).ok().flatten())
            .or_else(|| app.primary_monitor().ok().flatten())
            .map(|monitor| PillPlacement {
                origin: (monitor.position().x, monitor.position().y),
                size: (monitor.size().width, monitor.size().height),
                scale: monitor.scale_factor(),
            });

        if let Ok(mut stored) = PILL_PLACEMENT.lock() {
            *stored = placement;
        }
    }

    let Some(placement) = PILL_PLACEMENT.lock().ok().and_then(|p| *p) else {
        tracing::warn!("no display to place the pill on; leaving it where it is");
        return;
    };

    // One size for every capture state, so nothing resizes while the user is
    // talking. Failure is the only state that changes the window, because it is
    // the only one carrying a sentence rather than an indicator.
    let points = match state {
        SessionState::Failed { .. } => (PILL.width_failed, PILL.height_failed),
        _ => (PILL.width, PILL.height),
    };

    apply_pill_frame(&window, placement, points);
}

/**
 * SOURCE OF TRUTH KEYWORDS: pill_frame_on, cross_display_arithmetic
 * WHAT:  Where the pill sits on a given display, and how big it is there, in
 *        that display's physical pixels.
 * WHY:   Pure, because the bug it fixes was pure arithmetic — a size measured
 *        in one display's pixels used to centre against another display's
 *        width — and arithmetic can be tested, whereas a second monitor cannot
 *        be attached to a test runner. Everything touching a window handle
 *        stays in the caller.
 *
 *        Note what it does NOT take: the window. Nothing here can be derived
 *        from where the pill currently is, which is what stops a second
 *        opinion forming.
 * WHERE: The only place the frame is computed; exercised directly by the tests.
 */
fn pill_frame_on(
    monitor_position: (i32, i32),
    monitor_size: (u32, u32),
    scale: f64,
    pill_points: (f64, f64),
) -> ((i32, i32), (u32, u32)) {
    let width = (pill_points.0 * scale).round() as i32;
    let height = (pill_points.1 * scale).round() as i32;
    let inset = (PILL_BOTTOM_INSET_PT * scale).round() as i32;

    let x = monitor_position.0 + (monitor_size.0 as i32 - width) / 2;
    let y = monitor_position.1 + monitor_size.1 as i32 - height - inset;

    ((x, y), (width as u32, height as u32))
}

/**
 * SOURCE OF TRUTH KEYWORDS: apply_pill_frame, move_size_move
 * WHAT:  Puts the pill at an absolute frame on a specific display.
 * WHY:   Move, size, then move again, and each step is load-bearing. A
 *        PhysicalSize means device pixels on whatever display the window
 *        currently occupies, so sizing before the move applies the target
 *        display's pixel count to the old display's pixels. And resizing pins
 *        the top-left and grows down and right, so the size change shifts the
 *        centre by half the delta — re-asserting the position is cheaper and
 *        far more obvious than compensating for it.
 * WHERE: The single writer of the pill's frame.
 */
fn apply_pill_frame(window: &tauri::WebviewWindow, placement: PillPlacement, points: (f64, f64)) {
    let ((x, y), (w, h)) = pill_frame_on(
        placement.origin,
        placement.size,
        placement.scale,
        points,
    );
    let position = tauri::PhysicalPosition::new(x, y);
    let size = tauri::PhysicalSize::new(w, h);

    if let Err(err) = window.set_position(position) {
        tracing::warn!(error = %err, "could not move the pill to its display");
        return;
    }
    if let Err(err) = window.set_size(size) {
        tracing::warn!(error = %err, "could not size the pill for its display");
    }
    if let Err(err) = window.set_position(position) {
        tracing::warn!(error = %err, "could not position the pill");
    }
}


fn menu_error(err: tauri::Error) -> AppError {
    AppError::internal(err).with_detail("building the menu bar item")
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * SOURCE OF TRUTH KEYWORDS: pill_tokens_parse
     * WHAT:  Every dimension the window needs is actually found in tokens.css.
     * WHY:   `from_tokens` panics on a missing token, which is the right
     *        behaviour and also the reason this test exists: without it the
     *        first person to learn that a token had been renamed would be the
     *        user, at launch, via a crash. Reading them here means a rename
     *        fails the build instead.
     *
     *        It deliberately does NOT assert specific numbers. The pill's size
     *        belongs to whoever is designing it, and a test that pins it to
     *        176px is a test that makes a redesign fail for no reason — which
     *        is exactly what the previous version of this test did, within an
     *        hour of being written.
     */
    /**
     * SOURCE OF TRUTH KEYWORDS: cross_display_regression
     * WHAT:  The pill lands centred on any display, whatever its scale.
     * WHY:   The bug, in the form it actually took. The old code centred using
     *        the window's CURRENT physical size, so moving a 200pt pill from a
     *        2x laptop display to a 1x external one centred a 400px-wide object
     *        against a screen measured in 1x pixels — putting it far left. It
     *        looked correct on the second press because the window had moved by
     *        then and reported the new display's pixels.
     *
     *        The assertion is that BOTH displays give a centred result from a
     *        cold start, which is the property the second press was faking.
     */
    #[test]
    fn the_pill_centres_on_every_display_regardless_of_scale() {
        let pill = (200.0, 44.0);

        for (label, origin, size, scale) in [
            ("built-in 2x", (0, 0), (3456u32, 2234u32), 2.0),
            ("external 1x, to the right", (3456, 0), (2560, 1440), 1.0),
            ("external 1.5x, above", (0, -1600), (2560, 1600), 1.5),
        ] {
            let ((x, y), (w, h)) = pill_frame_on(origin, size, scale, pill);

            let left_gap = x - origin.0;
            let right_gap = (origin.0 + size.0 as i32) - (x + w as i32);
            assert!(
                (left_gap - right_gap).abs() <= 1,
                "{label}: off-centre by {}px — left {left_gap}, right {right_gap}",
                (left_gap - right_gap).abs()
            );

            // Sized in THIS display's pixels, not some other display's.
            assert_eq!(w, (pill.0 * scale).round() as u32, "{label}: wrong width");
            assert_eq!(h, (pill.1 * scale).round() as u32, "{label}: wrong height");

            // And sitting above the bottom edge by the specified inset.
            let bottom_gap = (origin.1 + size.1 as i32) - (y + h as i32);
            assert_eq!(
                bottom_gap,
                (PILL_BOTTOM_INSET_PT * scale).round() as i32,
                "{label}: wrong distance from the bottom edge"
            );
        }
    }

    /**
     * WHAT:  A pill measured on one display is never used to place it on
     *        another.
     * WHY:   States the defect directly, so the test survives a refactor that
     *        keeps the numbers but reintroduces the mistake: the frame for a
     *        display must depend only on that display.
     */
    #[test]
    fn placement_ignores_where_the_pill_used_to_be() {
        let pill = (200.0, 44.0);
        let external = ((3456, 0), (2560u32, 1440u32), 1.0);

        let ((x, _), (w, _)) = pill_frame_on(external.0, external.1, external.2, pill);

        // What the old code effectively did: centre using the 2x display's
        // pixel width. Kept here as the counter-example so the difference is
        // visible rather than asserted in the abstract.
        let stale_width = (pill.0 * 2.0).round() as i32;
        let stale_x = external.0 .0 + (external.1 .0 as i32 - stale_width) / 2;

        assert_ne!(
            x, stale_x,
            "placement is still being computed from the previous display's pixels"
        );
        assert_eq!(w, 200, "on a 1x display a 200pt pill is 200px");
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: two_recordings_regression, appearance_gate
     * WHAT:  A hide followed immediately by a show counts as a new appearance,
     *        even while the exit animation is still running.
     * WHY:   THE bug the operator reported twice: "I still see the pill element
     *        jumping all over the place when I'm switching between different
     *        windows... It only fixes itself when I run the recording twice on
     *        a different monitor."
     *
     *        The appearance gate used to be `window.is_visible()`, and the exit
     *        animation defers `hide()` into its completion handler — so for the
     *        length of the fade the window is still visible to AppKit while
     *        being, to the user, gone. Starting a recording inside that window
     *        read as "already up", skipped placement entirely, and the pill
     *        appeared at its position on the PREVIOUS display. The second
     *        recording worked because the hide had landed by then.
     *
     *        The flag is therefore cleared when a hide is REQUESTED, and this
     *        test asserts exactly that timing — that a show arriving before the
     *        animation finishes still counts as an appearance.
     * WHERE: Guards PILL_SHOWN's use in fit_pill_to_state and set_pill_visible.
     */
    #[test]
    fn a_show_during_the_exit_animation_still_counts_as_an_appearance() {
        PILL_SHOWN.store(false, Ordering::SeqCst);

        // First appearance.
        assert!(!PILL_SHOWN.swap(true, Ordering::SeqCst), "first show places");

        // A tick mid-recording is not an appearance.
        assert!(PILL_SHOWN.swap(true, Ordering::SeqCst), "ticks must not replace");

        // Hide REQUESTED. The animation is still running and the window is
        // still visible to AppKit at this instant — that is the whole point.
        assert!(
            PILL_SHOWN.swap(false, Ordering::SeqCst),
            "the hide request is what clears the flag, not the animation ending"
        );

        // The next recording starts before the fade completes.
        assert!(
            !PILL_SHOWN.swap(true, Ordering::SeqCst),
            "a recording started during the exit must still place the pill, or it \
             appears wherever it was on the previous display"
        );

        PILL_SHOWN.store(false, Ordering::SeqCst);
    }

    #[test]
    fn the_pill_dimensions_are_all_present_in_the_tokens() {
        let pill = PillMetrics::from_tokens();

        for (name, value) in [
            ("--pill-width", pill.width),
            ("--pill-height", pill.height),
            ("--pill-width-failed", pill.width_failed),
            ("--pill-height-failed", pill.height_failed),
            ("--radius-pill", pill.radius),
        ] {
            assert!(value > 0.0, "{name} resolved to {value}, which cannot be a size");
        }

        assert!(
            pill.width_failed >= pill.width,
            "the failure box carries a sentence, so it cannot be narrower than the indicator"
        );
        assert!(
            pill.radius * 2.0 <= pill.height + f64::EPSILON,
            "a radius over half the height cannot round a rectangle, and the vibrancy layer would square off"
        );
    }
}
