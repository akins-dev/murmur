/*!
 * SOURCE OF TRUTH KEYWORDS: ports, TranscriptionEngine, AudioSource,
 *   TextEnhancer, TextInjector, ModelStore, PermissionProvider
 * WHAT:  Barrel for every swappable contract in the app. Traits only.
 * WHY:   This layer holds no logic at all — that is the rule that keeps a port
 *        from quietly becoming a second implementation. A port never knows an
 *        adapter exists, which is what allows an engine to be added as one new
 *        file plus one factory arm, touching no call site.
 * WHERE: Implemented under adapters/; consumed by pipeline/, session/ and ipc/.
 */

pub mod audio;
pub mod engine;
pub mod events;
pub mod enhancer;
pub mod injector;
pub mod models;
pub mod permissions;

pub use audio::{AudioSource, CaptureConfig, CaptureEvent, CaptureSession, SampleSender};
pub use engine::{TranscribeRequest, TranscriptionEngine};
pub use events::{EventSink, NullEventSink};
pub use enhancer::{EnhanceContext, TextEnhancer};
pub use injector::{FrontmostApp, InjectionOutcome, InjectionRequest, TextInjector};
pub use models::{ModelStatus, ModelStore};
pub use permissions::{OsPermission, PermissionProvider, PermissionState};
