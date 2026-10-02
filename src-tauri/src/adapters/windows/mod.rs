/*!
 * SOURCE OF TRUTH KEYWORDS: adapters_windows, WindowsInjector, WindowsPermissions
 * WHAT:  Platform adapter barrel for Windows.
 * WHERE: Instantiated by bootstrap.rs.
 */

pub mod injector;
pub mod permissions;
pub mod sound;

pub use injector::WindowsInjector;
pub use permissions::WindowsPermissions;
pub use sound::{play_feedback, FeedbackSound};
