//! A brain that is a small recurrent neural network instead of programs: the line-for-line twin of the one in `creature_core.h` (see there for the layout of the weights). Two nets of one hidden layer with
//! recurrence: the LEG net, whose weights run for each of the 8 legs (each with its own hidden state), and the BODY net (the pressure, the silk and two signals). Every sum is added in the order of the loops
//! below, with no fused multiply-add and the machine's own `tanh`, so this crate, the trainer's CPU twin and its GPU kernel agree to the last bit.
use crate::mathx::dtanh;

pub const NN_H: usize = 24;
pub const NN_IN: usize = 35;
pub const NN_OUT: usize = 6;
pub const NNB_H: usize = 16;
pub const NNB_IN: usize = 34;
pub const NNB_OUT: usize = 7;
pub const NN_LEG_P: usize = NN_H * NN_IN + NN_H * NN_H + NN_H + NN_OUT * NN_H + NN_OUT;
pub const NN_BODY_P: usize = NNB_H * NNB_IN + NNB_H * NNB_H + NNB_H + NNB_OUT * NNB_H + NNB_OUT;
/// The weights of a spider's brain: the leg net, then the body net.
pub const NNP: usize = NN_LEG_P + NN_BODY_P;

/// The first nets (before the feelers were added to the senses) had 31 inputs to the leg net and 30 to the body net; brains of that size still run, on the senses they had.
pub const NN_IN_V1: usize = 31;
pub const NNB_IN_V1: usize = 30;
pub const NN_LEG_P_V1: usize = NN_H * NN_IN_V1 + NN_H * NN_H + NN_H + NN_OUT * NN_H + NN_OUT;
pub const NN_BODY_P_V1: usize = NNB_H * NNB_IN_V1 + NNB_H * NNB_H + NNB_H + NNB_OUT * NNB_H + NNB_OUT;
pub const NNP_V1: usize = NN_LEG_P_V1 + NN_BODY_P_V1;

pub fn nn_tanh(x: f32) -> f32 {
    dtanh(x as f64) as f32
}

/// One step of a net: the inputs `x` and the previous hidden state `hp` give the new hidden state `hn` and the outputs `o`.
pub fn nn_step<const H: usize, const IN: usize, const OUT: usize>(w: &[f32], x: &[f32; IN], hp: &[f32; H], hn: &mut [f32; H], o: &mut [f32; OUT]) {
    let win = &w[0..H * IN];
    let wrec = &w[H * IN..H * IN + H * H];
    let bb = &w[H * IN + H * H..H * IN + H * H + H];
    let wout = &w[H * IN + H * H + H..H * IN + H * H + H + OUT * H];
    let bout = &w[H * IN + H * H + H + OUT * H..H * IN + H * H + H + OUT * H + OUT];
    for h in 0..H {
        let mut a = bb[h];
        for i in 0..IN {
            a += win[h * IN + i] * x[i];
        }
        for j in 0..H {
            a += wrec[h * H + j] * hp[j];
        }
        hn[h] = nn_tanh(a);
    }
    for k in 0..OUT {
        let mut a = bout[k];
        for h in 0..H {
            a += wout[k * H + h] * hn[h];
        }
        o[k] = a;
    }
}
