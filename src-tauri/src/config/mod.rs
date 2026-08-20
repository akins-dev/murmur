/*!
 * SOURCE OF TRUTH KEYWORDS: config, paths, AppPaths
 * WHAT:  Barrel for filesystem locations and compile-time defaults.
 * WHERE: Used by db/, telemetry/ and the model store.
 */

pub mod paths;

pub use paths::AppPaths;
