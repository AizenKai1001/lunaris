//! legion 0.4.
use crate::*;
use legion::*;

struct Transform([f32; 16]);
struct Position([f32; 3]);
struct Rotation([f32; 3]);
struct Velocity([f32; 3]);
struct Data(f32);
struct CompA(f32);
struct CompB(f32);

macro_rules! letters {
    ($($l:ident),*) => { $( struct $l(#[allow(dead_code)] f32); )* };
}
letters!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

fn four() -> (Transform, Position, Rotation, Velocity) {
    (Transform([0.0; 16]), Position([0.0; 3]), Rotation([0.0; 3]), Velocity([1.0, 2.0, 3.0]))
}

pub fn hold(n: usize) -> Box<dyn std::any::Any> {
    let mut w = World::default();
    w.extend((0..n).map(|_| four()));
    Box::new(w)
}

pub fn run(rows: &mut Vec<Row>) {
    let t = measure(|| {
        let mut w = World::default();
        w.extend((0..INSERT_N).map(|_| four()));
        std::hint::black_box(&w);
    });
    push(rows, "legion", "simple_insert", INSERT_N, t);

    for (bench, n) in [("simple_iter", ITER_N), ("simple_iter_1m", ITER_BIG_N)] {
        let mut w = World::default();
        w.extend((0..n).map(|_| four()));
        let mut q = <(&mut Position, &Velocity)>::query();
        let t = measure(|| {
            for (p, v) in q.iter_mut(&mut w) {
                p.0[0] += v.0[0];
                p.0[1] += v.0[1];
                p.0[2] += v.0[2];
            }
        });
        push(rows, "legion", bench, n, t);
    }

    let mut w = World::default();
    macro_rules! each {
        ($($l:ident),*) => { $( w.extend((0..FRAG_PER).map(|_| (Data(1.0), $l(0.0)))); )* };
    }
    each!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
    let mut q = <&mut Data>::query();
    let t = measure(|| {
        for d in q.iter_mut(&mut w) {
            d.0 *= 2.0;
        }
    });
    push(rows, "legion", "frag_iter", FRAG_TYPES * FRAG_PER, t);

    let mut w = World::default();
    let ents: Vec<Entity> = w.extend((0..ADD_REMOVE_N).map(|_| (CompA(0.0),))).to_vec();
    let t = measure(|| {
        for &e in &ents {
            w.entry(e).unwrap().add_component(CompB(0.0));
        }
        for &e in &ents {
            w.entry(e).unwrap().remove_component::<CompB>();
        }
    });
    push(rows, "legion", "add_remove", ADD_REMOVE_N, t);
}
