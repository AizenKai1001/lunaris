//! bevy_ecs 0.15 (the standalone ECS of Bevy; single-threaded loops, no scheduler).
use crate::*;
use bevy_ecs::prelude::*;

#[derive(Component)]
struct Transform([f32; 16]);
#[derive(Component)]
struct Position([f32; 3]);
#[derive(Component)]
struct Rotation([f32; 3]);
#[derive(Component)]
struct Velocity([f32; 3]);
#[derive(Component)]
struct Data(f32);
#[derive(Component)]
struct CompA(f32);
#[derive(Component)]
struct CompB(f32);

macro_rules! letters {
    ($($l:ident),*) => { $( #[derive(Component)] struct $l(#[allow(dead_code)] f32); )* };
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
    push(rows, "bevy_ecs", "simple_insert", INSERT_N, t);

    for (bench, n) in [("simple_iter", ITER_N), ("simple_iter_1m", ITER_BIG_N)] {
        let mut w = World::new();
        w.spawn_batch((0..n).map(|_| four()));
        let mut q = w.query::<(&mut Position, &Velocity)>();
        let t = measure(|| {
            for (mut p, v) in q.iter_mut(&mut w) {
                p.0[0] += v.0[0];
                p.0[1] += v.0[1];
                p.0[2] += v.0[2];
            }
        });
        push(rows, "bevy_ecs", bench, n, t);
    }

    let mut w = World::new();
    macro_rules! each {
        ($($l:ident),*) => { $( w.spawn_batch((0..FRAG_PER).map(|_| (Data(1.0), $l(0.0)))); )* };
    }
    each!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
    let mut q = w.query::<&mut Data>();
    let t = measure(|| {
        for mut d in q.iter_mut(&mut w) {
            d.0 *= 2.0;
        }
    });
    push(rows, "bevy_ecs", "frag_iter", FRAG_TYPES * FRAG_PER, t);

    let mut w = World::new();
    let ents: Vec<Entity> = (0..ADD_REMOVE_N).map(|_| w.spawn(CompA(0.0)).id()).collect();
    let t = measure(|| {
        for &e in &ents {
            w.entity_mut(e).insert(CompB(0.0));
        }
        for &e in &ents {
            w.entity_mut(e).remove::<CompB>();
        }
    });
    push(rows, "bevy_ecs", "add_remove", ADD_REMOVE_N, t);
}
