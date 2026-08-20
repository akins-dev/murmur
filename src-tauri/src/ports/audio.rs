/*!
 * SOURCE OF TRUTH KEYWORDS: AudioSource, CaptureConfig, CaptureEvent,
 *   CaptureSession, SampleSender
 * WHAT:  The trait for microphone capture, and the events a live capture emits.
 * WHY:   The port hands out already-normalised 16kHz mono f32 frames, so no
 *        caller ever deals with device format. Device rate, channel count and
 *        sample type are the adapter's problem precisely because getting them
 *        wrong is silent — it degrades accuracy rather than failing, and then
 *        looks like a model problem.
 * WHERE: Implemented by adapters/cpal; driven by pipeline/capture.rs.
 */

use tokio::sync::mpsc::Sender;

use crate::error::{AppError, AppResult};
use crate::types::{AudioLevel, CaptureMode, DeviceInfo};

#[derive(Debug, Clone, Default)]
pub struct CaptureConfig {
    /// None means the system default input device.
    pub device_id: Option<String>,
    pub mode: CaptureMode,
}

/**
 * SOURCE OF TRUTH KEYWORDS: CaptureEvent
 * WHAT:  What a live capture pushes downstream.
 * WHY:   `Lost` is a first-class variant, not an error return, because a
 *        headset disconnecting mid-sentence is a normal event rather than an
 *        edge case — and the correct response is to transcribe and deliver what
 *        was already captured, not to discard it.
 * WHERE: Sent over a BOUNDED channel from the drain thread to pipeline/capture.
 */
#[derive(Debug)]
pub enum CaptureEvent {
    /// Normalised 16kHz mono f32 samples.
    Samples(Vec<f32>),
    /// A windowed meter reading for the pill. Cheap, frequent, droppable.
    Level(AudioLevel),
    /// The device went away. Whatever was captured before this is still good.
    Lost(AppError),
}

pub type SampleSender = Sender<CaptureEvent>;

/**
 * SOURCE OF TRUTH KEYWORDS: CaptureSession
 * WHAT:  A handle to one open microphone stream.
 * WHY:   Consuming `self` on stop makes use-after-stop unrepresentable, which
 *        matters because the underlying stream owns a realtime callback that
 *        must not outlive the buffers it writes into.
 * WHERE: Returned by AudioSource::start; held by pipeline/capture.rs.
 */
pub trait CaptureSession: Send {
    fn device(&self) -> &DeviceInfo;
    /// The device's native rate, before resampling. Recorded for diagnostics.
    fn native_sample_rate(&self) -> u32;
    fn stop(self: Box<Self>) -> AppResult<()>;
}

pub trait AudioSource: Send + Sync {
    fn list_devices(&self) -> AppResult<Vec<DeviceInfo>>;
    fn default_device(&self) -> AppResult<DeviceInfo>;

    /// Opens the device and begins delivering CaptureEvents to `sink`.
    /// Blocking, and expected to be fast — this sits on the hotkey path in
    /// OnDemand mode, which is why the measured device-open cost is a metric.
    fn start(&self, config: &CaptureConfig, sink: SampleSender)
        -> AppResult<Box<dyn CaptureSession>>;
}
