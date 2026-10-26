//! A neural brain from before the feelers (31 inputs to the leg net, 30 to the body net) must run exactly as the same net given zero weights for the four feelers: the two layouts are different files, one behaviour.
use lunaris_creature::episode::{run_episode, Cfg};
use lunaris_creature::nn::{NNB_H, NNB_IN, NNB_IN_V1, NNB_OUT, NN_H, NN_IN, NN_IN_V1, NN_OUT, NNP, NNP_V1};
use lunaris_creature::{Brain, Flat, Model};

/// Deterministic small weights.
fn weights_v1() -> Vec<f32> {
    let mut z = 0x1234_5678u32;
    (0..NNP_V1)
        .map(|_| {
            z = z.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((z >> 8) as f32 / 16_777_216.0 - 0.5) * 0.6
        })
        .collect()
}

/// The same net in the layout with the feelers: every input row of both nets gets zero weights for the new inputs.
fn to_v2(w: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(NNP);
    let mut pos = 0;
    for (h, i1, i2, o) in [(NN_H, NN_IN_V1, NN_IN, NN_OUT), (NNB_H, NNB_IN_V1, NNB_IN, NNB_OUT)] {
        for r in 0..h {
            out.extend_from_slice(&w[pos + r * i1..pos + (r + 1) * i1]);
            out.extend(std::iter::repeat(0.0).take(i2 - i1));
        }
        pos += h * i1;
        let rest = h * h + h + o * h + o;
        out.extend_from_slice(&w[pos..pos + rest]);
        pos += rest;
    }
    assert_eq!((pos, out.len()), (NNP_V1, NNP));
    out
}

fn brain(w: Vec<f32>) -> Brain {
    let text = format!("{{\"nn\":[{}]}}", w.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(","));
    Brain::from_genome_json(&lunaris_json::parse(&text).unwrap())
}

#[test]
fn a_net_from_before_the_feelers_runs_as_the_same_net_with_zero_weights_for_them() {
    let v1 = weights_v1();
    let v2 = to_v2(&v1);
    let (b1, b2) = (brain(v1), brain(v2));
    assert_eq!(b1.nn.as_ref().map(|w| w.len()), Some(NNP_V1));
    assert_eq!(b2.nn.as_ref().map(|w| w.len()), Some(NNP));
    let model = Model::spider();
    // a climbing level, so that the feelers see surfaces
    for level in [0, 11, 21] {
        let cfg = Cfg { layout_seed: 5, body_seed: 3, level };
        let (mut t1, mut t2) = (Vec::new(), Vec::new());
        let o1 = run_episode(&model, b1.clone(), &cfg, &Flat, 240, 3, Some(&mut t1));
        let o2 = run_episode(&model, b2.clone(), &cfg, &Flat, 240, 3, Some(&mut t2));
        assert_eq!(o1.score.to_bits(), o2.score.to_bits(), "level {level}: the scores differ");
        let (h1, h2): (Vec<u32>, Vec<u32>) = (t1.iter().map(|v| v.to_bits()).collect(), t2.iter().map(|v| v.to_bits()).collect());
        assert_eq!(h1, h2, "level {level}: the poses differ");
    }
}
