//! The colours of a clear sky by single scattering (Nishita 1993): the air is a Rayleigh layer (blue light scatters most, 8 km scale height) and a Mie layer of haze and dust (1.2 km), the light of the sun is dimmed and
//! reddened on its way through them and scattered towards the eye. The GPU shader (`sky.wgsl`, `atmosphere`) integrates the same thing for every pixel of the sky; this module does it for a few directions, to give the
//! renderer the colour of the zenith and the horizon (the ambient light and the fog) and how much the sun is dimmed (`Atmosphere`, docs/GPU.md). Lengths are in kilometres.
use crate::math::*;

const R_EARTH: f32 = 6360.0;
const R_TOP: f32 = 6420.0;
const BETA_R: V3 = [0.0058, 0.0135, 0.0331];
const BETA_M: f32 = 0.021;
const H_R: f32 = 8.0;
const H_M: f32 = 1.2;

/// What the air is like (the numbers of the `Atmosphere` component).
#[derive(Clone, Copy, Debug)]
pub struct Air {
    /// How much haze: 1 is a very clear day, 2 a clear one, 6 a hazy one.
    pub turbidity: f32,
    /// A multiplier of the Rayleigh layer's density.
    pub rayleigh: f32,
    /// The Mie phase function's asymmetry (0.76 is dust and haze).
    pub mie_g: f32,
    /// How bright the sky is, relative to the sun.
    pub intensity: f32,
}

fn far_hit(o: V3, d: V3, r: f32) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - r * r;
    let disc = b * b - c;
    if disc < 0.0 {
        return -1.0;
    }
    -b + disc.sqrt()
}

/// The factor the light of the sun is multiplied by on its way down to an observer `h_km` above the ground (the colour of the sun: 1 where there is no air, nothing when the planet is in the way).
pub fn sun_transmittance(h_km: f32, sun: V3, air: &Air) -> V3 {
    let o = [0.0, R_EARTH + h_km + 0.0005, 0.0];
    let t = far_hit(o, sun, R_TOP);
    if t <= 0.0 {
        return [0.0; 3];
    }
    let n = 24;
    let ds = t / n as f32;
    let (mut od_r, mut od_m) = (0.0f32, 0.0f32);
    for i in 0..n {
        let p = add(o, scale(sun, (i as f32 + 0.5) * ds));
        let h = length(p) - R_EARTH;
        if h < 0.0 {
            return [0.0; 3];
        }
        od_r += (-h / H_R).exp() * ds * air.rayleigh;
        od_m += (-h / H_M).exp() * ds * air.turbidity * 0.5;
    }
    let tau = [BETA_R[0] * od_r + BETA_M * 1.1 * od_m, BETA_R[1] * od_r + BETA_M * 1.1 * od_m, BETA_R[2] * od_r + BETA_M * 1.1 * od_m];
    [(-tau[0]).exp(), (-tau[1]).exp(), (-tau[2]).exp()]
}

/// The radiance of the sky in the direction `dir` (up is +y; below the horizon it is the horizon's), `sun` a unit vector towards the sun, for an observer `h_km` above the ground.
pub fn sky_radiance(h_km: f32, dir: V3, sun: V3, air: &Air) -> V3 {
    let o = [0.0, R_EARTH + h_km + 0.0005, 0.0];
    let d = normalize([dir[0], dir[1].max(0.0), dir[2]]);
    let t_max = far_hit(o, d, R_TOP);
    if t_max <= 0.0 {
        return [0.0; 3];
    }
    let n = 16;
    let ds = t_max / n as f32;
    let (mut od_r, mut od_m) = (0.0f32, 0.0f32);
    let (mut sum_r, mut sum_m) = ([0.0f32; 3], [0.0f32; 3]);
    for i in 0..n {
        let p = add(o, scale(d, (i as f32 + 0.5) * ds));
        let h = length(p) - R_EARTH;
        let hr = (-h / H_R).exp() * ds * air.rayleigh;
        let hm = (-h / H_M).exp() * ds * air.turbidity * 0.5;
        od_r += hr;
        od_m += hm;
        // the sun's light on its way to this point
        let tl = far_hit(p, sun, R_TOP);
        if tl <= 0.0 {
            continue;
        }
        let m = 6;
        let dl = tl / m as f32;
        let (mut odr_l, mut odm_l) = (0.0f32, 0.0f32);
        let mut shadow = false;
        for j in 0..m {
            let q = add(p, scale(sun, (j as f32 + 0.5) * dl));
            let hq = length(q) - R_EARTH;
            if hq < 0.0 {
                shadow = true;
                break;
            }
            odr_l += (-hq / H_R).exp() * dl * air.rayleigh;
            odm_l += (-hq / H_M).exp() * dl * air.turbidity * 0.5;
        }
        if shadow {
            continue;
        }
        for k in 0..3 {
            let tau = BETA_R[k] * (od_r + odr_l) + BETA_M * 1.1 * (od_m + odm_l);
            let att = (-tau).exp();
            sum_r[k] += att * hr;
            sum_m[k] += att * hm;
        }
    }
    let cosg = dot(d, sun);
    let g = air.mie_g;
    let ph_r = 3.0 / (16.0 * std::f32::consts::PI) * (1.0 + cosg * cosg);
    let ph_m = 3.0 / (8.0 * std::f32::consts::PI) * ((1.0 - g * g) * (1.0 + cosg * cosg)) / ((2.0 + g * g) * (1.0 + g * g - 2.0 * g * cosg).max(1.0e-4).powf(1.5));
    let i_sun = 20.0 * air.intensity;
    let mut out = [0.0f32; 3];
    for k in 0..3 {
        out[k] = i_sun * (sum_r[k] * BETA_R[k] * ph_r + sum_m[k] * BETA_M * ph_m);
    }
    out
}

/// The sky's colour at the zenith, its colour at the horizon (the mean of four directions round it, away from the sun's glare), and the factor the sun's colour is multiplied by (1 at noon, redder and dimmer
/// towards the horizon, 0 when it is down), for an eye `h_km` above the ground.
pub fn summary(h_km: f32, sun: V3, air: &Air) -> (V3, V3, V3) {
    let zenith = sky_radiance(h_km, [0.0, 1.0, 0.0], sun, air);
    let mut horizon = [0.0f32; 3];
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2 + 0.4;
        let c = sky_radiance(h_km, normalize([a.cos(), 0.12, a.sin()]), sun, air);
        for j in 0..3 {
            horizon[j] += c[j] * 0.25;
        }
    }
    let t_now = sun_transmittance(h_km, sun, air);
    let t_noon = sun_transmittance(h_km, [0.0, 1.0, 0.0], air);
    let factor = [t_now[0] / t_noon[0].max(1.0e-4), t_now[1] / t_noon[1].max(1.0e-4), t_now[2] / t_noon[2].max(1.0e-4)];
    (zenith, horizon, factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AIR: Air = Air { turbidity: 2.0, rayleigh: 1.0, mie_g: 0.76, intensity: 1.0 };

    #[test]
    fn a_noon_sky_is_blue_and_a_sunset_is_red_and_dimmer() {
        let noon = normalize([0.2, 1.0, 0.1]);
        let (zenith, horizon, factor) = summary(0.0, noon, &AIR);
        assert!(zenith[2] > zenith[0] * 1.3, "the zenith is blue: {zenith:?}");
        assert!(horizon.iter().all(|c| *c > 0.05), "the horizon has light: {horizon:?}");
        assert!((factor[0] - 1.0).abs() < 0.05 && (factor[2] - 1.0).abs() < 0.05, "the noon sun is as authored: {factor:?}");
        let low = normalize([1.0, 0.04, 0.0]);
        let (_, _, f) = summary(0.0, low, &AIR);
        assert!(f[0] > f[2] * 2.0, "a low sun is red: {f:?}");
        assert!(f[0] < 0.9, "and dimmer: {f:?}");
        // the sun below the ground gives no sunlight
        let down = normalize([1.0, -0.3, 0.0]);
        let (_, _, g) = summary(0.0, down, &AIR);
        assert_eq!(g, [0.0, 0.0, 0.0]);
        // a hazier day is brighter near the sun and whiter
        let hazy = Air { turbidity: 8.0, ..AIR };
        let toward = normalize([0.9, 0.5, 0.0]);
        let (a, b) = (sky_radiance(0.0, toward, normalize([1.0, 0.5, 0.0]), &AIR), sky_radiance(0.0, toward, normalize([1.0, 0.5, 0.0]), &hazy));
        assert!(b[1] > a[1], "haze brightens the sky round the sun: {b:?} vs {a:?}");
    }
}
