//! Sound for Lunaris, with nothing but the standard library.
//!
//! * [`wav`]: read a WAV file into a [`Clip`] (8, 16, 24 or 32 bit PCM, or 32/64 bit float; mono or stereo; any sample rate), and write one.
//! * [`synth`]: the built-in voices (a pitch glide of a sine, square, saw or triangle wave, or filtered noise) that make a game audible
//!   before it has any sound files, the same ones the browser editor plays.
//! * [`mixer`]: a stereo mixer at 44.1 kHz. Voices play a clip once or in a loop; a voice can sit at a place in the world, and then it gets
//!   quieter with distance and moves to the left or the right ear with the direction of the listener's ears.
//! * [`device`]: the sound card (Windows, through the system's `waveOut`); elsewhere opening it reports that there is none, and the mixer can
//!   still be rendered to samples (which is how a test, or an AI agent without ears, listens).
//!
//! Sound is **output**: nothing here is read by the simulation, so it never changes a state hash, and two machines may differ in the last bit
//! of a sample (the mixer uses `sin` and `cos` of the platform).

pub mod codec;
pub mod device;
pub mod mixer;
pub mod reverb;
pub mod synth;
pub mod trace;
pub mod wav;

pub use device::Output;
pub use mixer::{Acoustics, Listener, Mixer, Params, RoomSettings, Spatial, Tap, VoiceId, MAX_VOICES, RATE};
pub use synth::{default_voice, Voice};
pub use wav::Clip;

/// A short summary of a block of interleaved stereo samples, for reports.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    /// Largest absolute sample, 0..1 (1 and above = clipping).
    pub peak: f32,
    /// Root mean square of the left channel, then the right one.
    pub rms_left: f32,
    pub rms_right: f32,
    /// How many samples were at full scale.
    pub clipped: u64,
}

/// Peak, RMS per ear and clipped samples of interleaved stereo `samples`.
pub fn levels(samples: &[f32]) -> Levels {
    let (mut peak, mut sl, mut sr, mut clipped) = (0.0f32, 0.0f64, 0.0f64, 0u64);
    for (i, s) in samples.iter().enumerate() {
        peak = peak.max(s.abs());
        if s.abs() >= 0.9999 {
            clipped += 1;
        }
        if i % 2 == 0 {
            sl += (*s as f64) * (*s as f64);
        } else {
            sr += (*s as f64) * (*s as f64);
        }
    }
    let frames = (samples.len() / 2).max(1) as f64;
    Levels { peak, rms_left: (sl / frames).sqrt() as f32, rms_right: (sr / frames).sqrt() as f32, clipped }
}
