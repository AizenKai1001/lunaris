//! Deterministic randomness. `Rng` is xoshiro256** seeded by SplitMix64 (integer ops only, identical on every platform).
//! `hash_rand` is a stateless generator keyed by (seed, tick, uid, salt): use it for per-entity randomness so results
//! do not depend on the order entities happen to be stored in.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    s: [u64; 4],
}

/// SplitMix64 output function (the finaliser, without the state increment).
#[inline]
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One step of the SplitMix64 generator when used as a hash of `z`.
#[inline]
fn splitmix(z: u64) -> u64 {
    mix64(z.wrapping_add(0x9E37_79B9_7F4A_7C15))
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut sm = seed;
        let mut s = [0u64; 4];
        for x in &mut s {
            sm = sm.wrapping_add(0x9E37_79B9_7F4A_7C15);
            *x = mix64(sm);
        }
        if s == [0; 4] {
            s[0] = 1;
        }
        Rng { s }
    }

    pub fn from_state(s: [u64; 4]) -> Rng {
        Rng { s }
    }

    pub fn state(&self) -> [u64; 4] {
        self.s
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform in [0, 1) with 24 bits of resolution (exactly representable in f32).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / 16_777_216.0)
    }

    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    /// Uniform integer in [0, n) without modulo bias (Lemire's method).
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0);
        let mut m = (self.next_u64() as u32 as u64).wrapping_mul(n as u64);
        let mut l = m as u32;
        if l < n {
            let t = n.wrapping_neg() % n;
            while l < t {
                m = (self.next_u64() as u32 as u64).wrapping_mul(n as u64);
                l = m as u32;
            }
        }
        (m >> 32) as u32
    }
}

/// Stateless random float in [0, 1) from (seed, tick, uid, salt).
#[inline]
pub fn hash_rand(seed: u64, tick: u64, uid: u64, salt: u64) -> f32 {
    let z = splitmix(seed ^ splitmix(tick.wrapping_mul(0xD6E8_FEB8_6659_FD93) ^ splitmix(uid ^ splitmix(salt))));
    (z >> 40) as f32 * (1.0 / 16_777_216.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_matches_the_published_reference_value() {
        // SplitMix64 seeded with 0: the first output is 0xE220A8397B1DCDAF (Vigna's reference implementation).
        assert_eq!(splitmix(0), 0xE220_A839_7B1D_CDAF);
        // and the xoshiro state is seeded from that stream
        assert_eq!(Rng::new(0).state()[0], 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn floats_in_range_and_roughly_uniform() {
        let mut r = Rng::new(7);
        let mut sum = 0.0f64;
        for _ in 0..100_000 {
            let f = r.next_f32();
            assert!((0.0..1.0).contains(&f));
            sum += f as f64;
        }
        assert!((sum / 100_000.0 - 0.5).abs() < 0.01);
    }

    #[test]
    fn below_is_unbiased_enough_and_in_range() {
        let mut r = Rng::new(9);
        let mut counts = [0u32; 6];
        for _ in 0..60_000 {
            counts[r.below(6) as usize] += 1;
        }
        for c in counts {
            assert!((9_000..11_000).contains(&c), "{counts:?}");
        }
    }

    #[test]
    fn hash_rand_is_order_free_and_varies() {
        assert_eq!(hash_rand(1, 2, 3, 4), hash_rand(1, 2, 3, 4));
        assert_ne!(hash_rand(1, 2, 3, 4), hash_rand(1, 3, 3, 4));
        assert_ne!(hash_rand(1, 2, 3, 4), hash_rand(1, 2, 4, 4));
        let mean: f64 = (0..50_000u64).map(|u| hash_rand(5, 0, u, 0) as f64).sum::<f64>() / 50_000.0;
        assert!((mean - 0.5).abs() < 0.01);
    }
}
