//! The built-in voices: a game is audible before it has a single sound file. A [`Voice`] is a wave (sine, square, saw, triangle or low-passed
//! noise) whose pitch glides from `freq` to `freq_end` over `duration`, with a short attack and an exponential fall to -80 dB.
//! Common event names (`coin`, `hit`, `jump`, ...) have a voice of their own; any other name gets a small blip that is the same every time.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    /// 0 sine, 1 square, 2 saw, 3 triangle, 4 noise.
    pub wave: u32,
    pub freq: f32,
    /// Pitch at the end (0 = stay at `freq`); the glide is exponential. For noise this is the cut-off of a low-pass filter.
    pub freq_end: f32,
    pub duration: f32,
    pub volume: f32,
}

pub fn default_voice(name: &str) -> Voice {
    let v = |wave, freq, freq_end, duration, volume| Voice { wave, freq, freq_end, duration, volume };
    match name {
        "coin" => v(1, 880.0, 1320.0, 0.12, 0.22),
        "hit" => v(4, 200.0, 0.0, 0.12, 0.3),
        "jump" => v(0, 300.0, 700.0, 0.15, 0.25),
        "win" => v(3, 440.0, 880.0, 0.55, 0.3),
        "lose" => v(2, 330.0, 82.0, 0.7, 0.22),
        "click" => v(1, 600.0, 0.0, 0.04, 0.12),
        "shoot" => v(2, 900.0, 200.0, 0.15, 0.18),
        "explosion" => v(4, 300.0, 40.0, 0.5, 0.4),
        other => {
            // any other name gets its own small blip, the same one every time
            let h = other.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
            v(h % 4, 300.0 + (h % 600) as f32, 0.0, 0.1, 0.15)
        }
    }
}

/// Render one voice to mono samples in -1..1 at `rate` samples per second.
pub fn synth(v: &Voice, rate: u32) -> Vec<f32> {
    let rate_f = rate as f32;
    let dur = if v.duration.is_finite() { v.duration.clamp(0.02, 3.0) } else { 0.15 };
    let n = (dur * rate_f) as usize;
    let mut out = Vec::with_capacity(n);
    let (mut phase, mut seed, mut lp) = (0.0f32, 0x2545_F491u32, 0.0f32);
    let clean = |x: f32, d: f32| if x.is_finite() { x } else { d };
    let (freq, freq_end) = (clean(v.freq, 440.0), clean(v.freq_end, 0.0));
    let (f0, f1) = (freq.max(20.0), if freq_end > 0.0 { freq_end.max(20.0) } else { freq.max(20.0) });
    let volume = clean(v.volume, 0.3).clamp(0.0, 1.0);
    for i in 0..n {
        let t = i as f32 / n as f32;
        // 5 ms attack, then an exponential fall to -80 dB at the end
        let attack = ((i as f32 / rate_f) / 0.005).min(1.0);
        let gain = volume * attack * (0.0001f32).powf(t);
        let s = if v.wave == 4 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (seed >> 8) as f32 / (1 << 23) as f32 - 1.0;
            let cutoff = f0 * (f1 / f0).powf(t);
            let k = (cutoff / rate_f * 6.0).clamp(0.01, 1.0);
            lp += k * (white - lp);
            lp
        } else {
            let freq = f0 * (f1 / f0).powf(t);
            phase = (phase + freq / rate_f).fract();
            match v.wave {
                1 => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                2 => 2.0 * phase - 1.0,
                3 => 4.0 * (phase - 0.5).abs() - 1.0,
                _ => (phase * std::f32::consts::TAU).sin(),
            }
        };
        out.push(s * gain);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_have_the_expected_length_and_pitch_and_never_exceed_their_volume() {
        let coin = default_voice("coin");
        let s = synth(&coin, 22_050);
        assert_eq!(s.len(), (0.12 * 22_050f32) as usize);
        assert!(s.iter().all(|x| x.abs() <= 0.22 + 1e-6));
        // a rising glide crosses zero more often at the end than at the start
        let crossings = |a: &[f32]| a.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        let (head, tail) = (crossings(&s[..s.len() / 4]), crossings(&s[s.len() * 3 / 4..]));
        assert!(tail > head, "{head} -> {tail}");
        // the same voice at another rate lasts the same time
        assert_eq!(synth(&coin, 44_100).len(), (0.12 * 44_100f32) as usize);
        assert_eq!(synth(&Voice { wave: 4, ..default_voice("hit") }, 44_100), synth(&default_voice("hit"), 44_100), "noise is deterministic");
        // unknown names get a stable blip of their own
        assert_eq!(default_voice("zork"), default_voice("zork"));
        assert_ne!(default_voice("zork"), default_voice("plugh"));
    }

    #[test]
    fn nonsense_numbers_make_a_quiet_safe_voice_not_a_panic() {
        for v in [
            Voice { wave: 9, freq: f32::NAN, freq_end: f32::INFINITY, duration: f32::NAN, volume: f32::NAN },
            Voice { wave: 0, freq: -5.0, freq_end: -1.0, duration: 1e9, volume: 50.0 },
            Voice { wave: 4, freq: 0.0, freq_end: 1e9, duration: 0.0, volume: -1.0 },
        ] {
            let s = synth(&v, 44_100);
            assert!(s.iter().all(|x| x.is_finite() && x.abs() <= 1.0), "{v:?}");
            assert!(s.len() <= 3 * 44_100 + 1);
        }
    }
}
