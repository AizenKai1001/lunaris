//! Smooth random numbers that depend only on where you look: value noise, for scattering, terrain, clouds, wobble and every other procedural thing.
//! Only `+ - * /` and `floor` are used, on 64-bit floats, and the lattice values come from an integer hash, so every machine gives the same bits.

/// A pseudo-random number in [-1, 1) for a lattice point and a seed.
fn lattice(ix: i64, iy: i64, seed: i64) -> f64 {
    let mut h = (ix as u64).wrapping_mul(0x9E37_79B1_85EB_CA87) ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (seed as u64).wrapping_mul(0x1656_67B1_9E37_79F9) ^ 0x9E37_79B9_7F4A_7C15;
    h ^= h >> 32;
    h = h.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 32;
    h = h.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 32;
    // the top 53 bits, as a number in [0, 1), moved to [-1, 1)
    (h >> 11) as f64 / 9_007_199_254_740_992.0 * 2.0 - 1.0
}

/// Value noise in [-1, 1]: smooth (no jumps), the same at integer points for the same seed, different for another seed. `seed` counts in whole numbers
/// (its fraction is dropped). A point that is not a finite number (or is beyond what the lattice can count) gives 0.
pub fn noise2(x: f64, y: f64, seed: f64) -> f64 {
    const LIMIT: f64 = 1.0e9;
    if !(x.is_finite() && y.is_finite() && seed.is_finite()) || x.abs() > LIMIT || y.abs() > LIMIT {
        return 0.0;
    }
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i64, fy as i64);
    let s = seed.clamp(-LIMIT, LIMIT).floor() as i64;
    let (tx, ty) = (x - fx, y - fy);
    // the smooth curve 3t^2 - 2t^3 makes the slope vanish at the lattice points, so the squares join without creases
    let (ux, uy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (a, b) = (lattice(ix, iy, s), lattice(ix + 1, iy, s));
    let (c, d) = (lattice(ix, iy + 1, s), lattice(ix + 1, iy + 1, s));
    let top = a + (b - a) * ux;
    let bottom = c + (d - c) * ux;
    top + (bottom - top) * uy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_in_range_smooth_repeatable_and_depends_on_the_seed() {
        let (mut lo, mut hi, mut sum, mut n) = (f64::MAX, f64::MIN, 0.0, 0.0);
        let mut worst_step = 0.0f64;
        for i in 0..200 {
            for j in 0..200 {
                let (x, y) = (i as f64 * 0.173 - 11.0, j as f64 * 0.211 - 9.0);
                let v = noise2(x, y, 7.0);
                assert!((-1.0..=1.0).contains(&v), "{v} at {x},{y}");
                lo = lo.min(v);
                hi = hi.max(v);
                sum += v;
                n += 1.0;
                // a small step in the input is a small step in the output: no jumps at the lattice lines
                worst_step = worst_step.max((noise2(x + 0.001, y, 7.0) - v).abs()).max((noise2(x, y + 0.001, 7.0) - v).abs());
            }
        }
        assert!(lo < -0.8 && hi > 0.8, "it uses the whole range: {lo}..{hi}");
        assert!((sum / n).abs() < 0.05, "mean {}", sum / n);
        assert!(worst_step < 0.01, "biggest step {worst_step}");
        // the same input, the same number; another seed, another landscape; the fraction of the seed is dropped
        assert_eq!(noise2(3.3, 4.4, 1.0), noise2(3.3, 4.4, 1.0));
        assert_ne!(noise2(3.3, 4.4, 1.0), noise2(3.3, 4.4, 2.0));
        assert_eq!(noise2(3.3, 4.4, 1.0), noise2(3.3, 4.4, 1.9));
        // negative coordinates are as good as positive ones, and the lattice points themselves repeat their value from either side
        assert!((noise2(-0.0001, 5.0, 3.0) - noise2(0.0001, 5.0, 3.0)).abs() < 1e-3);
        // bad inputs give 0, not NaN
        for bad in [f64::NAN, f64::INFINITY, 1.0e300] {
            assert_eq!(noise2(bad, 0.0, 1.0), 0.0);
            assert_eq!(noise2(0.0, bad, 1.0), 0.0);
        }
        assert_eq!(noise2(0.0, 0.0, f64::NAN), 0.0);
    }

    /// Pinned values: if these change, every game that used `noise` changes (and the golden hashes of the examples that use it).
    #[test]
    fn noise_values_are_pinned() {
        let v = [noise2(0.0, 0.0, 0.0), noise2(1.5, 2.5, 0.0), noise2(-3.25, 10.75, 42.0)];
        let text = format!("{:.12} {:.12} {:.12}", v[0], v[1], v[2]);
        assert_eq!(text, PINNED, "noise changed: {text}");
    }

    const PINNED: &str = "-0.389898371417 0.166802109637 0.090576513684";
}
