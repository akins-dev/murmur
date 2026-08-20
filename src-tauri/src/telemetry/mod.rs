/*!
 * SOURCE OF TRUTH KEYWORDS: telemetry, init_tracing, TracingGuard, latency,
 *   LatencyRecorder, now_ms
 * WHAT:  Local logging setup and the per-stage latency recorder.
 * WHY:   Logs are written to rolling files on this machine and are never
 *        transmitted anywhere — the product promise is that nothing leaves the
 *        machine, and a crash reporter would break it. The guard must be held
 *        for the process lifetime: dropping it closes the appender and log
 *        lines silently stop, which is a confusing thing to debug.
 * WHERE: Initialised once at the top of lib.rs setup.
 */

pub mod latency;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub use latency::{now_ms, LatencyRecorder};

/// Holding this alive keeps the log writer running. Drop it and logging stops.
pub struct TracingGuard(#[allow(dead_code)] WorkerGuard);

/**
 * WHAT:  Starts tracing to a rolling daily file in the app's logs directory.
 * WHY:   Non-blocking so a slow disk cannot stall a command; the returned guard
 *        is what flushes the buffer on shutdown. In debug builds it also mirrors
 *        to stderr, because during development the file is the wrong place to
 *        look.
 * WHERE: Called once from lib.rs setup with AppPaths::logs_dir.
 */
pub fn init_tracing(logs_dir: &std::path::Path) -> TracingGuard {
    let appender = tracing_appender::rolling::daily(logs_dir, "murmur.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    // Default to info for our own crate and warn for everything else, so a
    // dependency cannot flood the log. RUST_LOG overrides it during debugging.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,murmur_lib=info"));

    let file_layer = fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true);

    let registry = tracing_subscriber::registry().with(filter).with(file_layer);

    #[cfg(debug_assertions)]
    let registry = registry.with(fmt::layer().with_writer(std::io::stderr));

    // An already-initialised subscriber is not a failure worth aborting over —
    // it happens in tests, where the first init wins and that is correct.
    if registry.try_init().is_err() {
        tracing::debug!("tracing subscriber was already initialised");
    }

    TracingGuard(guard)
}
