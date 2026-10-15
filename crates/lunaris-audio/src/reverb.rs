//! A reverberator: eight delay lines fed back through a Hadamard mix (a feedback delay network), with the loop gain set so that a sound falls
//! by 60 dB in the requested `rt60`, and a low-pass in the loop so that the high pitches die out first, the way they do in a real room.

use crate::mixer::RATE;

/// Delay line lengths in samples at 44.1 kHz (primes, so the echoes do not stack up on each other).
const LENGTHS: [usize; 8] = [1301, 1637, 1811, 1931, 2347, 2647, 2999, 3229];
const MAX_PREDELAY: f32 = 0.1;

struct Line {
    buf: Vec<f32>,
    pos: usize,
    gain: f32,
    lp: f32,
}

pub struct Reverb {
    lines: Vec<Line>,
    pre: Vec<f32>,
    pre_pos: usize,
    pre_len: usize,
    damping: f32,
}

impl Default for Reverb {
    fn default() -> Reverb {
        Reverb::new()
    }
}

impl Reverb {
    pub fn new() -> Reverb {
        let scale = RATE as f32 / 44_100.0;
        let lines = LENGTHS.iter().map(|&n| Line { buf: vec![0.0; ((n as f32 * scale) as usize).max(8)], pos: 0, gain: 0.0, lp: 0.0 }).collect();
        let mut r = Reverb { lines, pre: vec![0.0; (MAX_PREDELAY * RATE as f32) as usize + 1], pre_pos: 0, pre_len: 0, damping: 0.2 };
        r.set(1.0, 0.2, 0.0);
        r
    }

    /// `rt60`: seconds to fall by 60 dB (0.05 to 10); `damping` 0..0.95: how much faster the high pitches fade; `predelay` seconds (up to 0.1).
    pub fn set(&mut self, rt60: f32, damping: f32, predelay: f32) {
        let rt = if rt60.is_finite() { rt60.clamp(0.05, 10.0) } else { 1.0 };
        for l in &mut self.lines {
            let seconds = l.buf.len() as f32 / RATE as f32;
            l.gain = 10f32.powf(-3.0 * seconds / rt);
        }
        self.damping = if damping.is_finite() { damping.clamp(0.0, 0.95) } else { 0.2 };
        let pd = if predelay.is_finite() { predelay.clamp(0.0, MAX_PREDELAY) } else { 0.0 };
        self.pre_len = (pd * RATE as f32) as usize;
    }

    pub fn clear(&mut self) {
        for l in &mut self.lines {
            l.buf.iter_mut().for_each(|x| *x = 0.0);
            l.lp = 0.0;
        }
        self.pre.iter_mut().for_each(|x| *x = 0.0);
    }

    /// One sample in, one stereo sample out.
    pub fn process(&mut self, input: f32) -> (f32, f32) {
        let x = if self.pre_len == 0 {
            input
        } else {
            let n = self.pre.len();
            self.pre[self.pre_pos] = input;
            let read = (self.pre_pos + n - self.pre_len) % n;
            self.pre_pos = (self.pre_pos + 1) % n;
            self.pre[read]
        };
        let mut o = [0.0f32; 8];
        for (k, l) in self.lines.iter().enumerate() {
            o[k] = l.buf[l.pos];
        }
        // fast Walsh-Hadamard transform of the eight outputs, scaled to keep the energy
        let mut m = o;
        let mut h = 1;
        while h < 8 {
            let mut i = 0;
            while i < 8 {
                for j in i..i + h {
                    let (a, b) = (m[j], m[j + h]);
                    m[j] = a + b;
                    m[j + h] = a - b;
                }
                i += h * 2;
            }
            h *= 2;
        }
        let norm = 1.0 / (8.0f32).sqrt();
        let d = self.damping;
        for (k, l) in self.lines.iter_mut().enumerate() {
            let fed = m[k] * norm * l.gain;
            l.lp = fed * (1.0 - d) + l.lp * d;
            // a little of the input goes into every line, alternating in sign so that the lines start out different
            let inject = x * if k % 2 == 0 { 0.3 } else { -0.3 };
            l.buf[l.pos] = l.lp + inject;
            l.pos = (l.pos + 1) % l.buf.len();
        }
        let left = (o[0] - o[1] + o[2] - o[3] + o[4] - o[5] + o[6] - o[7]) * 0.25;
        let right = (o[0] + o[1] - o[2] - o[3] + o[4] + o[5] - o[6] - o[7]) * 0.25;
        (left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The time a unit impulse takes to fall by 60 dB, measured the way a room-acoustics program does: backward integration of the energy,
    /// a line fitted between -5 and -25 dB and scaled to 60 dB.
    fn measured_rt60(rt60: f32, damping: f32) -> f32 {
        let mut r = Reverb::new();
        r.set(rt60, damping, 0.0);
        let n = (RATE as f32 * (rt60 * 1.6 + 0.5)) as usize;
        let mut e: Vec<f64> = Vec::with_capacity(n);
        for i in 0..n {
            let (l, _) = r.process(if i == 0 { 1.0 } else { 0.0 });
            e.push((l as f64) * (l as f64));
        }
        let mut edc = vec![0.0f64; n + 1];
        for i in (0..n).rev() {
            edc[i] = edc[i + 1] + e[i];
        }
        let db = |i: usize| 10.0 * (edc[i] / edc[0]).max(1e-30).log10();
        let t_at = |level: f64| (0..n).find(|&i| db(i) <= level).unwrap() as f64 / RATE as f64;
        ((t_at(-25.0) - t_at(-5.0)) * 3.0) as f32
    }

    #[test]
    fn the_tail_falls_by_sixty_decibels_in_the_time_asked_for() {
        for rt in [0.4f32, 1.0, 2.5] {
            let got = measured_rt60(rt, 0.0);
            assert!((got - rt).abs() < rt * 0.25, "asked {rt} s, measured {got} s");
        }
        // damping makes the broadband tail fall faster, never slower
        assert!(measured_rt60(2.0, 0.7) < measured_rt60(2.0, 0.0));
    }

    #[test]
    fn it_stays_stable_stays_quiet_when_nothing_goes_in_and_pre_delays() {
        let mut r = Reverb::new();
        r.set(10.0, 0.0, 0.0);
        let mut peak = 0.0f32;
        for i in 0..RATE as usize * 20 {
            let (l, rr) = r.process(if i % 4410 == 0 { 0.5 } else { 0.0 });
            assert!(l.is_finite() && rr.is_finite());
            peak = peak.max(l.abs()).max(rr.abs());
        }
        assert!(peak < 3.0, "{peak}");
        r.clear();
        for _ in 0..1000 {
            assert_eq!(r.process(0.0), (0.0, 0.0));
        }
        // with a pre-delay of 50 ms nothing comes out for the first 50 ms plus the shortest line
        let mut p = Reverb::new();
        p.set(1.0, 0.0, 0.05);
        let first = (0..RATE as usize / 10).position(|i| {
            let (l, r) = p.process(if i == 0 { 1.0 } else { 0.0 });
            l != 0.0 || r != 0.0
        });
        assert!(first.unwrap() >= (0.05 * RATE as f32) as usize + 1000, "{first:?}");
        // the two ears get different signals
        let mut s = Reverb::new();
        s.set(1.0, 0.0, 0.0);
        let mut diff = 0.0f32;
        for i in 0..8000 {
            let (l, r) = s.process(if i == 0 { 1.0 } else { 0.0 });
            diff += (l - r).abs();
        }
        assert!(diff > 0.01, "{diff}");
        // nonsense settings are made safe
        let mut w = Reverb::new();
        w.set(f32::NAN, f32::INFINITY, -4.0);
        assert!(w.process(1.0).0.is_finite());
    }
}
