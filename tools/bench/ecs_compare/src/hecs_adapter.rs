//! hecs 0.10.
use crate::*;
use hecs::{Entity, World};

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
    let mut w = World::new();
    w.spawn_batch((0..n).map(|_| four()));
    Box::new(w)
}

pub fn run(rows: &mut Vec<Row>) {
    let t = measure(|| {
        let mut w = World::new();
        w.spawn_batch((0..INSERT_N).map(|_| four()));
        std::hint::black_box(&w);
    });
    push(rows, "hecs", "simple_insert", INSERT_N, t);

    for (bench, n) in [("simple_iter", ITER_N), ("simple_iter_1m", ITER_BIG_N)] {
        let mut w = World::new();
        w.spawn_batch((0..n).map(|_| four()));
        let t = measure(|| {
            for (_e, (p, v)) in w.query_mut::<(&mut Position, &Velocity)>() {
                p.0[0] += v.0[0];
                p.0[1] += v.0[1];
                p.0[2] += v.0[2];
            }
        });
        push(rows, "hecs", bench, n, t);
    }

    let mut w = World::new();
    macro_rules! each {
        ($($l:ident),*) => { $( w.spawn_batch((0..FRAG_PER).map(|_| (Data(1.0), $l(0.0)))); )* };
    }
    each!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
    let t = measure(|| {
        for (_e, d) in w.query_mut::<&mut Data>() {
            d.0 *= 2.0;
        }
    });
    push(rows, "hecs", "frag_iter", FRAG_TYPES * FRAG_PER, t);

    let mut w = World::new();
    let ents: Vec<Entity> = w.spawn_batch((0..ADD_REMOVE_N).map(|_| (CompA(0.0),))).collect();
    let t = measure(|| {
        for &e in &ents {
            w.insert_one(e, CompB(0.0)).unwrap();
        }
        for &e in &ents {
            w.remove_one::<CompB>(e).unwrap();
        }
    });
    push(rows, "hecs", "add_remove", ADD_REMOVE_N, t);
}
