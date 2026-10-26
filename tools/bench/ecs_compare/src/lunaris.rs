//! The Lunaris adapter. Components are declared with `component!` (plain data, schema for the tools and the editor, hashable state).
use crate::*;
use lunaris_ecs::{component, Entity, World};

component! { pub struct Transform { pub m: [f32; 16] = [0.0; 16] } }
component! { pub struct Position { pub v: [f32; 3] = [0.0; 3] } }
component! { pub struct Rotation { pub v: [f32; 3] = [0.0; 3] } }
component! { pub struct Velocity { pub v: [f32; 3] = [1.0, 2.0, 3.0] } }
component! { pub struct Data { pub v: f32 = 1.0 } }
component! { pub struct CompA { pub v: f32 = 0.0 } }
component! { pub struct CompB { pub v: f32 = 0.0 } }

macro_rules! letters {
    ($($l:ident),*) => { $( component! { pub struct $l { pub v: f32 = 0.0 } } )* };
}
letters!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

fn spawn_fragmented(w: &mut World) {
    macro_rules! each {
        ($($l:ident),*) => { $( for _ in 0..FRAG_PER { w.spawn_with2(Data::default(), $l::default()).unwrap(); } )* };
    }
    each!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
}

fn four() -> (Transform, Position, Rotation, Velocity) {
    (Transform::default(), Position::default(), Rotation::default(), Velocity::default())
}

/// The first version of this benchmark: the four components through the lowest-level spawn, which allocates on every call.
fn spawn_four_raw(w: &mut World) -> Entity {
    let ids = (
        w.register_native::<Transform>().unwrap(),
        w.register_native::<Position>().unwrap(),
        w.register_native::<Rotation>().unwrap(),
        w.register_native::<Velocity>().unwrap(),
    );
    let (t, p, r, v) = (Transform::default(), Position::default(), Rotation::default(), Velocity::default());
    // SAFETY: each pointer references a live value of the component registered under that id.
    unsafe {
        w.spawn_raw(
            None,
            &[
                (ids.0, (&t as *const Transform).cast()),
                (ids.1, (&p as *const Position).cast()),
                (ids.2, (&r as *const Rotation).cast()),
                (ids.3, (&v as *const Velocity).cast()),
            ],
        )
        .unwrap()
    }
}

/// A world of `n` entities with the four components of simple_insert, kept alive by the caller (for the memory measurement).
pub fn hold(n: usize) -> Box<dyn std::any::Any> {
    let mut w = World::new();
    w.spawn_batch((0..n).map(|_| four())).unwrap();
    Box::new(w)
}

pub fn run(rows: &mut Vec<Row>) {
    // simple_insert: the batch spawn (the idiomatic bulk path, like hecs spawn_batch), a loop of single bundle spawns, and the old raw path
    let t = measure(|| {
        let mut w = World::new();
        w.spawn_batch((0..INSERT_N).map(|_| four())).unwrap();
        std::hint::black_box(&w);
    });
    push(rows, "lunaris", "simple_insert", INSERT_N, t);
    let t = measure(|| {
        let mut w = World::new();
        for _ in 0..INSERT_N {
            w.spawn_bundle(four()).unwrap();
        }
        std::hint::black_box(&w);
    });
    push(rows, "lunaris-loop", "simple_insert", INSERT_N, t);
    let t = measure(|| {
        let mut w = World::new();
        for _ in 0..INSERT_N {
            spawn_four_raw(&mut w);
        }
        std::hint::black_box(&w);
    });
    push(rows, "lunaris-raw", "simple_insert", INSERT_N, t);

    // simple_iter at the suite's size and at one million
    for (bench, n) in [("simple_iter", ITER_N), ("simple_iter_1m", ITER_BIG_N)] {
        let mut w = World::new();
        w.spawn_batch((0..n).map(|_| four())).unwrap();
        let t = measure(|| {
            w.for_each::<(&mut Position, &Velocity), _>(|_, (p, v)| {
                p.v[0] += v.v[0];
                p.v[1] += v.v[1];
                p.v[2] += v.v[2];
            })
            .unwrap();
        });
        push(rows, "lunaris", bench, n, t);
    }

    // frag_iter
    let mut w = World::new();
    spawn_fragmented(&mut w);
    let t = measure(|| {
        w.for_each::<(&mut Data,), _>(|_, (d,)| d.v *= 2.0).unwrap();
    });
    push(rows, "lunaris", "frag_iter", FRAG_TYPES * FRAG_PER, t);

    // add_remove
    let mut w = World::new();
    let ents: Vec<Entity> = w.spawn_batch((0..ADD_REMOVE_N).map(|_| (CompA::default(),))).unwrap();
    let t = measure(|| {
        for &e in &ents {
            w.insert(e, CompB::default()).unwrap();
        }
        for &e in &ents {
            w.remove_typed::<CompB>(e).unwrap();
        }
    });
    push(rows, "lunaris", "add_remove", ADD_REMOVE_N, t);
}
