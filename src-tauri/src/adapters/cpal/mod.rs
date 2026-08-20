/*!
 * SOURCE OF TRUTH KEYWORDS: cpal_adapter, CpalAudioSource, Resampler16k
 * WHAT:  The AudioSource implementation backed by cpal / CoreAudio.
 * WHERE: Constructed by adapters::build; the only implementation of the
 *        AudioSource port on macOS.
 */

pub mod resample;
pub mod source;

pub use source::CpalAudioSource;
