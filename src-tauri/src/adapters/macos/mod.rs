/*!
 * SOURCE OF TRUTH KEYWORDS: macos_adapter, MacosInjector, MacosPermissions,
 *   PasteTiming
 * WHAT:  Everything platform-specific: TCC permissions and text injection.
 * WHY:   This is the only directory that knows macOS exists. Windows and Linux
 *        need a sibling of it and nothing else — which is what makes the
 *        cross-platform claim in the plan a week of adapter work rather than a
 *        rewrite.
 * WHERE: Constructed by adapters::build; implements the TextInjector and
 *        PermissionProvider ports.
 */

pub mod injector;
pub mod modifier_tap;
mod sound;
pub mod permissions;

pub use injector::{MacosInjector, PasteTiming};
pub use sound::{play_feedback, FeedbackSound};
pub use modifier_tap::{watch_modifier_tap, ModifierTap, TAPS_REQUIRED};
pub use permissions::MacosPermissions;
