/*!
 * SOURCE OF TRUTH KEYWORDS: pipeline, vad, chunker, worker, assembler, deliver
 * WHAT:  The path from microphone to pasted text.
 * WHY:   Split by stage so each one is testable alone. The stages that decide
 *        latency (chunker, worker) are separated from the stages that decide
 *        quality (assembler, deliver), because they are tuned against different
 *        evidence and conflating them is how a speed change silently costs
 *        accuracy.
 * WHERE: Driven by session/actor.rs.
 */

pub mod assembler;
pub mod chunker;
pub mod vad;
pub mod worker;

pub use chunker::Chunker;
pub use vad::SpeechDetector;
