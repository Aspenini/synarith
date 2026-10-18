//! # Synarith live viewer
//!
//! Egui presents in-memory frames from a persistent worker owning parsing, file
//! watching, reusable CPU/GPU programs, cancellation, and export. No CLI process
//! or temporary preview PNG is involved. Playback starts at zero without a loop
//! endpoint. Paused views still resample when navigating. Camera navigation uses
//! f64; normal views use the f32 CPU/GPU path, with automatic f64 CPU arithmetic
//! for deeper camera views. Compiled constants and parameter values retain f32
//! representation. No examples or algorithm presets are embedded in the UI.
#![deny(missing_docs)]
mod camera;
mod worker;
pub use camera::*;
pub use worker::*;

/// Seekable animation time, independent of frame rendering speed.
#[derive(Clone, Debug)]
pub struct Timeline {
    /// Current time in seconds.
    pub time: f64,
    /// Whether real elapsed time advances the timeline.
    pub playing: bool,
    /// Playback rate multiplier.
    pub speed: f64,
}
impl Default for Timeline {
    fn default() -> Self {
        Self {
            time: 0.0,
            playing: true,
            speed: 1.0,
        }
    }
}
impl Timeline {
    /// Advance without an artificial loop endpoint.
    pub fn advance(&mut self, seconds: f64) {
        if self.playing
            && seconds.is_finite()
            && seconds >= 0.0
            && self.speed.is_finite()
            && self.speed > 0.0
        {
            self.time = (self.time + seconds * self.speed).min(f64::from(f32::MAX));
        }
    }
    /// Seek to a finite nonnegative time without changing play/pause.
    pub fn seek(&mut self, time: f64) {
        if time.is_finite() {
            self.time = time.max(0.0).min(f64::from(f32::MAX));
        }
    }
}
