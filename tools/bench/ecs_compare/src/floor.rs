//! The hardware floor: the same work on plain arrays, no ECS at all. An ECS cannot beat this; how close it comes is the measure.
use crate::*;

#[derive(Clone, Copy)]
struct P([f32; 3]);
#[derive(Clone, Copy)]
struct V([f32; 3]);

pub fn run(rows: &mut Vec<Row>) {
    // structure of arrays: what an archetype column layout can reach
    for (bench, n) in [("simple_iter", ITER_N), ("simple_iter_1m", ITER_BIG_N)] {
        let mut p = vec![P([0.0; 3]); n];
        let v = vec![V([1.0, 2.0, 3.0]); n];
        let t = measure(|| {
            for (p, v) in p.iter_mut().zip(v.iter()) {
                p.0[0] += v.0[0];
                p.0[1] += v.0[1];
                p.0[2] += v.0[2];
            }
            std::hint::black_box(&mut p);
        });
        push(rows, "floor", bench, n, t);
    }
    // fragmented: 26 arrays of 20 floats, one loop over each
    let mut data: Vec<Vec<f32>> = (0..FRAG_TYPES).map(|_| vec![1.0; FRAG_PER]).collect();
    let t = measure(|| {
        for d in data.iter_mut() {
            for x in d.iter_mut() {
                *x *= 2.0;
            }
        }
        std::hint::black_box(&mut data);
    });
    push(rows, "floor", "frag_iter", FRAG_TYPES * FRAG_PER, t);
}
