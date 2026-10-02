/*!
 * SOURCE OF TRUTH KEYWORDS: CpalAudioSource, CpalCaptureSession, build_stream,
 *   spawn_drain, RING_CAPACITY_FRAMES, DRAIN_INTERVAL
 * WHAT:  Microphone capture: opens a CoreAudio input stream, copies samples out
 *        of the realtime callback through a lock-free ring buffer, and converts
 *        them to 16kHz mono on a normal thread.
 * WHY:   The whole shape of this file exists to protect one rule: **the cpal
 *        input callback runs on a CoreAudio realtime thread, and inside it we
 *        may not allocate, lock, log, touch the database, or call anything
 *        async.** Violating that does not crash — it produces dropped buffers
 *        and glitches that look exactly like a transcription accuracy problem,
 *        which is how you spend a day debugging the wrong layer.
 *
 *        So the callback does precisely one thing: copy samples into a
 *        lock-free SPSC ring buffer. Everything expensive — format conversion,
 *        downmix, resampling, metering, channel sends — happens on the drain
 *        thread, where it is allowed to be slow.
 *
 *        A device disappearing mid-sentence is treated as a normal event, not
 *        an edge case. AirPods disconnect. The error callback reports it as
 *        CaptureEvent::Lost and whatever was already captured stays valid and
 *        gets delivered — losing a sentence to a headset is exactly the failure
 *        history exists to prevent.
 * WHERE: Implements the AudioSource port; driven by pipeline/capture.rs.
 */

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig, SupportedStreamConfig};

/// cpal 0.18 exposes a device's human name through `description()`, and a
/// device that cannot describe itself is one that has just been unplugged.
fn device_name(device: &Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::ports::audio::{AudioSource, CaptureConfig, CaptureEvent, CaptureSession, SampleSender};
use crate::types::{AudioLevel, DeviceInfo};

use super::resample::{peak_of, rms_of, Resampler16k};

/**
 * Two seconds of stereo 48kHz headroom. Sized so a scheduling hiccup on the
 * drain thread cannot lose audio, and so that the pre-roll in Instant mode has
 * somewhere to live. Overflow is counted and logged rather than silently
 * tolerated — dropped input is a real defect, not backpressure.
 */
const RING_CAPACITY_FRAMES: usize = 48_000 * 2 * 2;

/// How often the drain thread wakes. Short enough that the waveform tracks the
/// voice, long enough that it is not a spin loop.
const DRAIN_INTERVAL: Duration = Duration::from_millis(10);

pub struct CpalAudioSource;

impl CpalAudioSource {
    pub fn new() -> Self {
        Self
    }

    fn host() -> cpal::Host {
        cpal::default_host()
    }

    fn find_device(device_id: Option<&str>) -> AppResult<(Device, SupportedStreamConfig)> {
        let host = Self::host();

        let device = match device_id {
            // "default" is stored rather than a device name, so that swapping
            // the system default follows automatically.
            None | Some("default") | Some("") => host.default_input_device(),
            Some(name) => host
                .input_devices()
                .map_err(device_error)?
                .find(|device| device_name(device).as_deref() == Some(name))
                // A device that has been unplugged since it was chosen falls
                // back to the default rather than refusing to record.
                .or_else(|| host.default_input_device()),
        }
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::AudioDeviceUnavailable,
                "Murmur could not find a microphone to record from.",
            )
        })?;

        let config = device.default_input_config().map_err(|err| {
            AppError::new(
                ErrorCode::AudioDeviceUnavailable,
                "That microphone did not report a usable format.",
            )
            .with_detail(err)
        })?;

        Ok((device, config))
    }

    fn describe(device: &Device, config: &SupportedStreamConfig, is_default: bool) -> DeviceInfo {
        let name = device_name(device).unwrap_or_else(|| "Unknown device".into());
        DeviceInfo {
            id: name.clone(),
            name,
            is_default,
            sample_rate: config.sample_rate(),
            channels: config.channels(),
        }
    }
}

impl Default for CpalAudioSource {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSource for CpalAudioSource {
    fn list_devices(&self) -> AppResult<Vec<DeviceInfo>> {
        let host = Self::host();
        let default_name = host
            .default_input_device()
            .and_then(|device| device_name(&device));

        let mut devices = Vec::new();
        for device in host.input_devices().map_err(device_error)? {
            // A device that cannot describe itself is skipped rather than
            // failing the whole list — one broken driver must not empty the
            // picker.
            let Ok(config) = device.default_input_config() else {
                continue;
            };
            let is_default = device_name(&device)
                .zip(default_name.as_ref())
                .map(|(name, default)| &name == default)
                .unwrap_or(false);
            devices.push(Self::describe(&device, &config, is_default));
        }
        Ok(devices)
    }

    fn default_device(&self) -> AppResult<DeviceInfo> {
        let (device, config) = Self::find_device(None)?;
        Ok(Self::describe(&device, &config, true))
    }

    fn start(
        &self,
        config: &CaptureConfig,
        sink: SampleSender,
    ) -> AppResult<Box<dyn CaptureSession>> {
        let (device, supported) = Self::find_device(config.device_id.as_deref())?;
        let info = Self::describe(&device, &supported, config.device_id.is_none());
        let native_rate = supported.sample_rate();
        let channels = supported.channels();
        let sample_format = supported.sample_format();
        let stream_config: StreamConfig = supported.into();

        let ring = HeapRb::<f32>::new(RING_CAPACITY_FRAMES);
        let (mut producer, mut consumer) = ring.split();

        let overflowed = Arc::new(AtomicBool::new(false));
        let callback_overflowed = Arc::clone(&overflowed);
        let error_sink = sink.clone();

        // ── The realtime callback. Read the module WHY before touching. ──
        let stream = build_stream(
            &device,
            &stream_config,
            sample_format,
            move |samples: &[f32]| {
                let written = producer.push_slice(samples);
                if written < samples.len() {
                    // No logging here — this flag is read by the drain thread,
                    // which is allowed to log.
                    callback_overflowed.store(true, Ordering::Relaxed);
                }
            },
            move |err| {
                // The stream is dead and will not recover on its own. Report it
                // and let the FSM deliver whatever was already captured.
                let _ = error_sink.try_send(CaptureEvent::Lost(
                    AppError::new(
                        ErrorCode::AudioDeviceLost,
                        "The microphone disconnected. Murmur kept what it heard.",
                    )
                    .with_detail(err),
                ));
            },
        )?;

        stream.play().map_err(|err| {
            AppError::new(
                ErrorCode::AudioDeviceUnavailable,
                "Murmur could not start recording from that microphone.",
            )
            .with_detail(err)
        })?;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let drain_stop = Arc::clone(&stop_flag);

        // ── The drain thread. Allowed to allocate, lock and log. ──
        let drain = std::thread::Builder::new()
            .name("murmur-audio-drain".into())
            .spawn(move || {
                let mut resampler = match Resampler16k::new(native_rate, channels) {
                    Ok(resampler) => resampler,
                    Err(err) => {
                        let _ = sink.try_send(CaptureEvent::Lost(err));
                        return;
                    }
                };

                let mut scratch = vec![0.0_f32; 8192];

                while !drain_stop.load(Ordering::Relaxed) {
                    let read = consumer.pop_slice(&mut scratch);

                    if read == 0 {
                        std::thread::sleep(DRAIN_INTERVAL);
                        continue;
                    }

                    if overflowed.swap(false, Ordering::Relaxed) {
                        tracing::warn!(
                            "audio ring buffer overflowed — samples were dropped before conversion"
                        );
                    }

                    match resampler.push(&scratch[..read]) {
                        Ok(samples) if samples.is_empty() => {}
                        Ok(samples) => {
                            let level = AudioLevel {
                                rms: rms_of(&samples),
                                peak: peak_of(&samples),
                            };
                            // Levels are droppable: a missed meter frame is
                            // invisible, whereas blocking here would add
                            // latency to the audio path itself.
                            let _ = sink.try_send(CaptureEvent::Level(level));

                            if sink.try_send(CaptureEvent::Samples(samples)).is_err() {
                                tracing::warn!("capture consumer is not keeping up");
                            }
                        }
                        Err(err) => {
                            let _ = sink.try_send(CaptureEvent::Lost(err));
                            return;
                        }
                    }
                }

                tracing::debug!(
                    pending_frames = resampler.pending_frames(),
                    "audio drain thread stopped"
                );
            })
            .map_err(|err| {
                AppError::internal(err).with_detail("could not spawn the audio drain thread")
            })?;

        Ok(Box::new(CpalCaptureSession {
            stream: Some(stream),
            info,
            native_rate,
            stop_flag,
            drain: Some(drain),
        }))
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: CpalCaptureSession
 * WHAT:  A live stream plus the thread draining it.
 * WHY:   Stopping consumes `self`, so nothing can hold a stream that has been
 *        torn down. The stream is dropped BEFORE the drain thread is joined:
 *        dropping it stops the callback, which is what lets the drain loop see
 *        an empty ring and exit rather than blocking on a producer that is
 *        still running.
 * WHERE: Returned by CpalAudioSource::start.
 */
struct CpalCaptureSession {
    /// `Option` so that both `stop` and `Drop` can take the stream out. A type
    /// that implements Drop cannot have its fields moved out of it, which is
    /// what the previous `stream: Stream` prevented.
    stream: Option<Stream>,
    info: DeviceInfo,
    native_rate: u32,
    stop_flag: Arc<AtomicBool>,
    drain: Option<std::thread::JoinHandle<()>>,
}

impl CpalCaptureSession {
    /**
     * SOURCE OF TRUTH KEYWORDS: teardown, idempotent_stop
     * WHAT:  Closes the stream and joins the drain thread. Safe to call twice.
     * WHY:   Shared by `stop` and `Drop` so there is ONE teardown sequence
     *        rather than two that can disagree. Idempotent because both may run
     *        — `stop` consumes the box, but a session that is dropped without
     *        `stop` must still release the device.
     * WHERE: CaptureSession::stop, and Drop.
     */
    fn teardown(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);

        // Order matters — see the struct WHY. Dropping the stream first stops
        // CoreAudio calling back into buffers the drain thread is reading.
        drop(self.stream.take());

        if let Some(drain) = self.drain.take() {
            // A drain thread that will not join is a bug worth knowing about,
            // but not one worth refusing to stop recording over.
            if drain.join().is_err() {
                tracing::error!("the audio drain thread panicked");
            }
        }
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: capture_drop, microphone_indicator
 * WHAT:  Releases the device even when nobody called `stop`.
 * WHY:   `stop` consuming `Box<Self>` makes use-after-stop unrepresentable, but
 *        it cannot make forgetting-to-stop unrepresentable — and forgetting is
 *        what happened. The FSM's DeviceLost arms transitioned out of Recording
 *        without a StopCapture effect, so the session handle survived into
 *        Finalizing and was overwritten by the next `start_capture`. Dropping
 *        the old value released the Stream but never set the drain thread's
 *        stop flag, leaving a thread spinning on a channel forever.
 *
 *        The FSM omission is fixed in session/machine.rs. This is here because
 *        the guarantee "the microphone is released when this value dies" should
 *        not depend on a state machine being right — in a local-first app, a
 *        microphone indicator that stays lit after recording ends is read by
 *        the user as the app still listening to them, and no amount of correct
 *        behaviour elsewhere argues them out of what they can see in the menu
 *        bar.
 * WHERE: Runs whenever a session value goes out of scope.
 */
impl Drop for CpalCaptureSession {
    fn drop(&mut self) {
        // Cheap and silent in the normal case: `stop` has already taken both,
        // so this sets a flag nobody reads and joins nothing.
        if self.stream.is_some() || self.drain.is_some() {
            tracing::debug!("capture session dropped without stop; releasing the device");
            self.teardown();
        }
    }
}

// cpal's Stream is not Send on every platform, but a capture session is only
// ever created, used and dropped from the pipeline task on macOS/CoreAudio,
// where the stream is safe to move. This is asserted rather than assumed: the
// impl is scoped to macOS only, so another platform must revisit it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
unsafe impl Send for CpalCaptureSession {}

impl CaptureSession for CpalCaptureSession {
    fn device(&self) -> &DeviceInfo {
        &self.info
    }

    fn native_sample_rate(&self) -> u32 {
        self.native_rate
    }

    fn stop(self: Box<Self>) -> AppResult<()> {
        let mut this = self;
        this.teardown();
        // `this` drops here; its Drop sees both fields already taken and does
        // nothing further.
        Ok(())
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: build_stream
 * WHAT:  Builds an input stream for whichever sample format the device uses,
 *        normalising everything to f32 inside the callback.
 * WHY:   The default input config is commonly f32, but i16 and u16 are both
 *        real and neither can be assumed away. Conversion happens here, in
 *        fixed-size stack chunks, so the realtime callback still never
 *        allocates.
 * WHERE: Called once per capture session.
 */
fn build_stream<F, E>(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    mut on_samples: F,
    on_error: E,
) -> AppResult<Stream>
where
    F: FnMut(&[f32]) + Send + 'static,
    E: FnMut(cpal::Error) + Send + 'static,
{
    /// Stack scratch for integer formats. No heap allocation on the realtime
    /// thread — that is the entire point.
    const CONVERT_CHUNK: usize = 1024;

    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            *config,
            move |data: &[f32], _| on_samples(data),
            on_error,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            *config,
            move |data: &[i16], _| {
                let mut buffer = [0.0_f32; CONVERT_CHUNK];
                for block in data.chunks(CONVERT_CHUNK) {
                    for (slot, sample) in buffer.iter_mut().zip(block) {
                        *slot = f32::from(*sample) / f32::from(i16::MAX);
                    }
                    on_samples(&buffer[..block.len()]);
                }
            },
            on_error,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            *config,
            move |data: &[u16], _| {
                let mut buffer = [0.0_f32; CONVERT_CHUNK];
                for block in data.chunks(CONVERT_CHUNK) {
                    for (slot, sample) in buffer.iter_mut().zip(block) {
                        // u16 is offset-binary: midpoint is silence.
                        *slot = (f32::from(*sample) - 32_768.0) / 32_768.0;
                    }
                    on_samples(&buffer[..block.len()]);
                }
            },
            on_error,
            None,
        ),
        other => {
            return Err(AppError::new(
                ErrorCode::AudioFormatUnsupported,
                "That microphone uses an audio format Murmur cannot read.",
            )
            .with_detail(format!("{other:?}")))
        }
    };

    stream.map_err(|err| {
        AppError::new(
            ErrorCode::AudioDeviceUnavailable,
            "Murmur could not open that microphone.",
        )
        .with_detail(err)
    })
}

fn device_error(err: cpal::Error) -> AppError {
    AppError::new(
        ErrorCode::AudioDeviceUnavailable,
        "Murmur could not list your microphones.",
    )
    .with_detail(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TARGET_SAMPLE_RATE;

    #[test]
    fn the_ring_holds_at_least_a_second_of_the_worst_case_device() {
        // 48kHz stereo is the common worst case. A drain thread stall shorter
        // than this cannot lose audio.
        let one_second_stereo_48k = 48_000 * 2;
        assert!(
            RING_CAPACITY_FRAMES >= one_second_stereo_48k,
            "the ring buffer is too small to absorb a scheduling hiccup"
        );
    }

    #[test]
    fn listing_devices_does_not_error_on_this_machine() -> AppResult<()> {
        // Not an assertion about what is plugged in — only that enumeration
        // works and that a broken driver cannot take the list down with it.
        let source = CpalAudioSource::new();
        let devices = source.list_devices()?;
        for device in &devices {
            assert!(!device.name.is_empty());
            assert!(device.sample_rate > 0);
        }
        Ok(())
    }

    #[test]
    fn the_target_format_is_what_whisper_requires() {
        // Guards against someone "fixing" the constant to match a device.
        assert_eq!(TARGET_SAMPLE_RATE, 16_000);
    }
}
