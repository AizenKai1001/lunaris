//! Deterministic math. `sin`, `cos`, `tan` and friends from the standard library call the platform's libm, whose results can
//! differ in the last bits between operating systems and CPUs. These use only `+ - * /` and exact operations
//! (`floor`, `sqrt`) in f64, so they give the same bits everywhere. Accuracy is about 1e-11, far below f32 precision.
pub const PI: f64 = std::f64::consts::PI;
const TWO_PI: f64 = 2.0 * std::f64::consts::PI;
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;

/// Sine of `x` radians.
pub fn sin(x: f64) -> f64 {
    // reduce to [-pi, pi]
    let k = (x / TWO_PI + 0.5).floor();
    let mut r = x - k * TWO_PI;
    // fold to [-pi/2, pi/2] using sin(pi - r) = sin(r)
    if r > HALF_PI {
        r = PI - r;
    } else if r < -HALF_PI {
        r = -PI - r;
    }
    // Taylor series through r^17: the truncation error at pi/2 is about 1e-13
    let r2 = r * r;
    let mut term = r;
    let mut sum = r;
    let mut n = 1.0;
    for _ in 0..8 {
        term = -term * r2 / ((n + 1.0) * (n + 2.0));
        sum += term;
        n += 2.0;
    }
    sum
}

/// Cosine of `x` radians.
pub fn cos(x: f64) -> f64 {
    sin(x + HALF_PI)
}

/// Tangent of `x` radians (large near odd multiples of pi/2).
pub fn tan(x: f64) -> f64 {
    sin(x) / cos(x)
}

// ln 2 split in two: HI has trailing zero bits, so k * HI is exact for the |k| used here (the fdlibm constants, shortest round-trip text)
const LN2_HI: f64 = 6.931_471_803_691_238e-1;
const LN2_LO: f64 = 1.908_214_929_270_587_7e-10;
const SQRT_2: f64 = std::f64::consts::SQRT_2;

/// Arc tangent, in (-pi/2, pi/2).
pub fn atan(x: f64) -> f64 {
    let negative = x < 0.0;
    let a = x.abs();
    // atan(a) = pi/2 - atan(1/a) brings the argument into [0, 1]; three halvings atan(t) = 2 atan(t / (1 + sqrt(1 + t^2)))
    // bring it to at most tan(pi/32) = 0.0985, where a short series is accurate to the last bits
    let flip = a > 1.0;
    let mut t = if flip { 1.0 / a } else { a };
    for _ in 0..3 {
        t /= 1.0 + (1.0 + t * t).sqrt();
    }
    let t2 = t * t;
    let (mut term, mut sum) = (t, t);
    for k in 1..12 {
        term = -term * t2;
        sum += term / (2 * k + 1) as f64;
    }
    let mut r = 8.0 * sum;
    if flip {
        r = HALF_PI - r;
    }
    if negative {
        -r
    } else {
        r
    }
}

/// The angle of the point (`x`, `y`) in (-pi, pi], like `f64::atan2(y, x)`; `atan2(0, 0)` is 0.
pub fn atan2(y: f64, x: f64) -> f64 {
    if x == 0.0 {
        return if y > 0.0 {
            HALF_PI
        } else if y < 0.0 {
            -HALF_PI
        } else {
            0.0
        };
    }
    let a = atan(y / x);
    if x > 0.0 {
        a
    } else if y >= 0.0 {
        a + PI
    } else {
        a - PI
    }
}

/// e to the power `x`. Infinite above about 709.78 and 0 below -700 (callers decide whether that is an error).
pub fn exp(x: f64) -> f64 {
    if x > 709.78 {
        return f64::INFINITY;
    }
    if x < -700.0 {
        return 0.0;
    }
    let k = (x / std::f64::consts::LN_2 + 0.5).floor();
    // r = x - k ln 2 with ln 2 split in two parts, so the subtraction loses (almost) nothing
    let r = (x - k * LN2_HI) - k * LN2_LO;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..17 {
        term = term * r / n as f64;
        sum += term;
    }
    // times 2^k, built from the exponent bits (k is within -1010..=1024 here; split the last step to stay finite)
    let k = k as i64;
    let half = k / 2;
    let p = |e: i64| f64::from_bits(((e + 1023) as u64) << 52);
    sum * p(half) * p(k - half)
}

/// Natural logarithm of `x`, which must be positive and finite (callers check).
pub fn ln(x: f64) -> f64 {
    let mut bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64;
    if e == 0 {
        // subnormal: scale by 2^54 first
        bits = (x * 18_014_398_509_481_984.0).to_bits();
        e = ((bits >> 52) & 0x7ff) as i64 - 54;
    }
    e -= 1023;
    // the mantissa as a number in [1, 2), moved to [sqrt(2)/2, sqrt(2)) so the series argument stays small
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | (1023u64 << 52));
    if m > SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    // ln(m) = 2 atanh(s), s = (m - 1) / (m + 1), |s| <= 0.1716
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let (mut term, mut sum) = (s, s);
    for k in 1..12 {
        term *= s2;
        sum += term / (2 * k + 1) as f64;
    }
    let e = e as f64;
    e * LN2_HI + (e * LN2_LO + 2.0 * sum)
}

/// Integer power by repeated squaring (a deterministic stand-in for `powf` when the exponent is a whole number).
pub fn powi(mut base: f32, mut exp: u32) -> f32 {
    let mut acc = 1.0f32;
    while exp > 0 {
        if exp & 1 == 1 {
            acc *= base;
        }
        base *= base;
        exp >>= 1;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_standard_library_to_high_precision() {
        let mut worst = 0.0f64;
        let mut x = -200.0;
        while x < 200.0 {
            worst = worst.max((sin(x) - x.sin()).abs()).max((cos(x) - x.cos()).abs());
            x += 0.0137;
        }
        assert!(worst < 1e-10, "worst error {worst}");
        assert!((tan(0.7) - 0.7f64.tan()).abs() < 1e-10);
        assert_eq!(sin(0.0), 0.0);
    }

    #[test]
    fn exact_symmetries() {
        assert_eq!(sin(-0.3), -sin(0.3));
        assert!((sin(PI / 6.0) - 0.5).abs() < 1e-12);
        assert!((cos(PI / 3.0) - 0.5).abs() < 1e-12);
    }

    /// The new functions against the platform's, over a wide range of arguments.
    #[test]
    fn atan_atan2_exp_and_ln_match_the_standard_library() {
        let rel = |a: f64, b: f64| if b == 0.0 { a.abs() } else { ((a - b) / b).abs() };
        let mut worst = [0.0f64; 4];
        let mut x = -50.0;
        while x < 50.0 {
            worst[0] = worst[0].max((atan(x) - x.atan()).abs());
            worst[1] = worst[1].max(rel(exp(x * 0.2), (x * 0.2).exp()));
            if x > 0.0 {
                worst[2] = worst[2].max((ln(x) - x.ln()).abs() / x.ln().abs().max(1.0));
            }
            x += 0.00731;
        }
        let mut a = 0.0f64;
        while a < 6.3 {
            for (y, x) in [(a.sin(), a.cos()), (-a.sin(), a.cos()), (a.sin() * 3.0, -a.cos()), (-2.0 * a.cos(), -a.sin())] {
                worst[3] = worst[3].max((atan2(y, x) - y.atan2(x)).abs());
            }
            a += 0.0123;
        }
        assert!(worst.iter().all(|w| *w < 1e-13), "worst errors {worst:?}");
        // extremes
        assert!((exp(700.0) / 700.0f64.exp() - 1.0).abs() < 1e-13);
        assert!(exp(710.0).is_infinite() && exp(-800.0) == 0.0);
        assert!((ln(1e-300) - 1e-300f64.ln()).abs() < 1e-11);
        assert!((ln(5e-324) - 5e-324f64.ln()).abs() < 1e-11, "subnormal");
        assert!((ln(1.7e308) - 1.7e308f64.ln()).abs() < 1e-11);
        assert_eq!(ln(1.0), 0.0);
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(atan(0.0), 0.0);
        assert!((atan(f64::INFINITY) - HALF_PI).abs() < 1e-15);
        // axes and the origin
        assert_eq!(atan2(0.0, 0.0), 0.0);
        assert_eq!(atan2(1.0, 0.0), HALF_PI);
        assert_eq!(atan2(-1.0, 0.0), -HALF_PI);
        assert!((atan2(0.0, -1.0) - PI).abs() < 1e-15);
    }

    #[test]
    fn the_split_ln2_constants_are_the_exact_fdlibm_values() {
        assert_eq!(LN2_HI.to_bits(), 0x3fe6_2e42_fee0_0000);
        assert_eq!(LN2_LO.to_bits(), 0x3dea_39ef_3579_3c76);
    }

    #[test]
    fn integer_powers() {
        assert_eq!(powi(2.0, 10), 1024.0);
        assert_eq!(powi(3.0, 0), 1.0);
        assert_eq!(powi(0.5, 3), 0.125);
    }
}
