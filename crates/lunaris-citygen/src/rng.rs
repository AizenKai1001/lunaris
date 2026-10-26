//! A small deterministic generator (SplitMix64): every building, lot and street is made from hashes of the city's seed and its own place, so a place looks the same every time it is generated, in any order.

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(mix(seed))
    }

    /// A generator keyed by several numbers (a seed, a cell, a lot, a purpose): the same parts give the same stream.
    pub fn keyed(parts: &[u64]) -> Rng {
        let mut h = 0x1234_5678_9ABC_DEF0u64;
        for &p in parts {
            h = mix(h ^ p.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        }
        Rng(h)
    }

    /// An independent generator for a sub-task (so that adding a detail to one part does not change the others).
    pub fn fork(&mut self, salt: u64) -> Rng {
        let a = self.u64();
        Rng::keyed(&[a, salt])
    }

    pub fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// Uniform integer in lo..=hi.
    pub fn int(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.u64() % (hi - lo + 1) as u64) as i32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.u64() % items.len() as u64) as usize]
    }

    /// An index chosen with the given relative weights.
    pub fn weighted(&mut self, w: &[f32]) -> usize {
        let total: f32 = w.iter().sum();
        let mut t = self.f32() * total;
        for (i, x) in w.iter().enumerate() {
            if t < *x {
                return i;
            }
            t -= x;
        }
        w.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_key_gives_the_same_numbers() {
        let mut a = Rng::keyed(&[7, 3, 9]);
        let mut b = Rng::keyed(&[7, 3, 9]);
        for _ in 0..16 {
            assert_eq!(a.u64(), b.u64());
        }
        assert_ne!(Rng::keyed(&[7, 3, 9]).u64(), Rng::keyed(&[7, 3, 10]).u64());
    }

    #[test]
    fn ranges_stay_in_range() {
        let mut r = Rng::new(1);
        for _ in 0..1000 {
            let x = r.range(2.0, 5.0);
            assert!((2.0..5.0).contains(&x));
            let i = r.int(-3, 4);
            assert!((-3..=4).contains(&i));
        }
        assert_eq!(r.int(5, 5), 5);
    }
}
