//! Deterministic math: only `+ - * /`, `floor`, `sqrt` and comparisons, in a fixed order, so that this crate, the GPU trainer (`spidergpu`, `creature_core.h`) and its CPU twin get the same bits.
//! The library's `sin`, `cos`, `atan2`, `exp` and `tanh` differ from machine to machine in the last place, and the creatures' dynamics are chaotic: one different bit in a sine and a walk ends elsewhere.
//! Every function here is a line-for-line copy of its twin in `creature_core.h`; do not "improve" one without the other (and the golden files in `tests/data`).

const TWO_PI_F: f32 = 6.283_185_307_179_586_5;
const INV_TWO_PI_F: f32 = 0.159_154_943_091_895_34;
const PI_F: f32 = std::f32::consts::PI;
const HALF_PI_F: f32 = std::f32::consts::FRAC_PI_2;
const QUARTER_PI_F: f32 = std::f32::consts::FRAC_PI_4;

pub fn dsinf(x: f32) -> f32 {
    let mut r = x - (x * INV_TWO_PI_F + 0.5).floor() * TWO_PI_F; // -pi .. pi
    if r > HALF_PI_F {
        r = PI_F - r;
    } else if r < -HALF_PI_F {
        r = -PI_F - r;
    }
    let r2 = r * r;
    r * (1.0 + r2 * (-1.666_666_67e-1 + r2 * (8.333_333_33e-3 + r2 * (-1.984_126_98e-4 + r2 * (2.755_731_92e-6 + r2 * (-2.505_210_84e-8))))))
}

pub fn dcosf(x: f32) -> f32 {
    dsinf(x + HALF_PI_F)
}

pub fn datan2f(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let mx = if ax > ay { ax } else { ay };
    let mn = if ax > ay { ay } else { ax };
    if mx == 0.0 {
        return 0.0;
    }
    let mut a = mn / mx; // 0 .. 1
    let mut base = 0.0f32;
    if a > 0.41421356237_f32 {
        a = (a - 1.0) / (a + 1.0);
        base = QUARTER_PI_F;
    }
    let s = a * a;
    let mut r = base + a * (1.0 + s * (-3.333_333_33e-1 + s * (2.0e-1 + s * (-1.428_571_43e-1 + s * (1.111_111_11e-1 + s * (-9.090_909_09e-2 + s * 7.692_307_69e-2))))));
    if ay > ax {
        r = HALF_PI_F - r;
    }
    if x < 0.0 {
        r = PI_F - r;
    }
    if y < 0.0 {
        r = -r;
    }
    r
}

const TWO_PI: f64 = 6.283_185_307_179_586_5;
const INV_TWO_PI: f64 = 0.159_154_943_091_895_34;
const PI: f64 = std::f64::consts::PI;
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;

pub fn dsin(x: f64) -> f64 {
    let mut r = x - (x * INV_TWO_PI + 0.5).floor() * TWO_PI;
    if r > HALF_PI {
        r = PI - r;
    } else if r < -HALF_PI {
        r = -PI - r;
    }
    let r2 = r * r;
    // sin r = r (1 - r2/6 (1 - r2/20 (1 - r2/42 (... (1 - r2/420)))))
    let mut acc = 1.0 - r2 / 420.0;
    acc = 1.0 - r2 / 342.0 * acc;
    acc = 1.0 - r2 / 272.0 * acc;
    acc = 1.0 - r2 / 210.0 * acc;
    acc = 1.0 - r2 / 156.0 * acc;
    acc = 1.0 - r2 / 110.0 * acc;
    acc = 1.0 - r2 / 72.0 * acc;
    acc = 1.0 - r2 / 42.0 * acc;
    acc = 1.0 - r2 / 20.0 * acc;
    acc = 1.0 - r2 / 6.0 * acc;
    r * acc
}

pub fn dcos(x: f64) -> f64 {
    dsin(x + HALF_PI)
}

pub fn dexp(x: f64) -> f64 {
    let x = if x > 700.0 {
        700.0
    } else if x < -700.0 {
        -700.0
    } else {
        x
    };
    const LN2: f64 = std::f64::consts::LN_2;
    const INV_LN2: f64 = std::f64::consts::LOG2_E;
    let kf = (x * INV_LN2 + 0.5).floor();
    let r = x - kf * LN2;
    let mut acc = 1.0 + r / 14.0;
    acc = 1.0 + r / 13.0 * acc;
    acc = 1.0 + r / 12.0 * acc;
    acc = 1.0 + r / 11.0 * acc;
    acc = 1.0 + r / 10.0 * acc;
    acc = 1.0 + r / 9.0 * acc;
    acc = 1.0 + r / 8.0 * acc;
    acc = 1.0 + r / 7.0 * acc;
    acc = 1.0 + r / 6.0 * acc;
    acc = 1.0 + r / 5.0 * acc;
    acc = 1.0 + r / 4.0 * acc;
    acc = 1.0 + r / 3.0 * acc;
    acc = 1.0 + r / 2.0 * acc;
    acc = 1.0 + r * acc;
    let k = kf as i32;
    acc * f64::from_bits(((k + 1023) as u64) << 52) // times 2^k
}

pub fn dtanh(x: f64) -> f64 {
    if x > 20.0 {
        return 1.0;
    }
    if x < -20.0 {
        return -1.0;
    }
    let e = dexp(2.0 * x);
    (e - 1.0) / (e + 1.0)
}
