//! specs 0.20 (storage per component type, joins; no archetypes).
use crate::*;
use specs::prelude::*;

macro_rules! comp {
    ($($name:ident($t:ty)),* $(,)?) => { $(
        struct $name(#[allow(dead_code)] $t);
        impl Component for $name { type Storage = VecStorage<Self>; }
    )* };
}
comp!(Transform([f32; 16]), Position([f32; 3]), Rotation([f32; 3]), Velocity([f32; 3]), Data(f32), CompA(f32), CompB(f32));

macro_rules! letters {
    ($($l:ident),*) => { $(
        struct $l(#[allow(dead_code)] f32);
        impl Component for $l { type Storage = VecStorage<Self>; }
    )* };
}
letters!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

fn world() -> World {
    let mut w = World::new();
    w.register::<Transform>();
    w.register::<Position>();
    w.register::<Rotation>();
    w.register::<Velocity>();
    w.register::<Data>();
    w.register::<CompA>();
    w.register::<CompB>();
    macro_rules! reg {
        ($($l:ident),*) => { $( w.register::<$l>(); )* };
    }
    reg!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
    w
}

fn fill(w: &mut World, n: usize) {
    for _ in 0..n {
        w.create_entity().with(Transform([0.0; 16])).with(Position([0.0; 3])).with(Rotation([0.0; 3])).with(Velocity([1.0, 2.0, 3.0])).build();
    }
}

pub fn hold(n: usize) -> Box<dyn std::any::Any> {
    let mut w = world();
    fill(&mut w, n);
    Box::new(w)
}

pub fn run(rows: &mut Vec<Row>) {
    let t = measure(|| {
        let mut w = world();
        fill(&mut w, INSERT_N);
        std::hint::black_box(&w);
    });
    push(rows, "specs", "simple_insert", INSERT_N, t);

    for (bench, n) in [("simple_iter", ITER_N), ("simple_iter_1m", ITER_BIG_N)] {
        let mut w = world();
        fill(&mut w, n);
        let t = measure(|| {
            let (mut p, v) = w.system_data::<(WriteStorage<Position>, ReadStorage<Velocity>)>();
            for (p, v) in (&mut p, &v).join() {
                p.0[0] += v.0[0];
                p.0[1] += v.0[1];
                p.0[2] += v.0[2];
            }
        });
        push(rows, "specs", bench, n, t);
    }

    let mut w = world();
    macro_rules! each {
        ($($l:ident),*) => { $( for _ in 0..FRAG_PER { w.create_entity().with(Data(1.0)).with($l(0.0)).build(); } )* };
    }
    each!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
    let t = measure(|| {
        let mut d = w.write_storage::<Data>();
        for d in (&mut d).join() {
            d.0 *= 2.0;
        }
    });
    push(rows, "specs", "frag_iter", FRAG_TYPES * FRAG_PER, t);

    let mut w = world();
    let ents: Vec<Entity> = (0..ADD_REMOVE_N).map(|_| w.create_entity().with(CompA(0.0)).build()).collect();
    let t = measure(|| {
        let mut b = w.write_storage::<CompB>();
        for &e in &ents {
            b.insert(e, CompB(0.0)).unwrap();
        }
        for &e in &ents {
            b.remove(e);
        }
    });
    push(rows, "specs", "add_remove", ADD_REMOVE_N, t);
}
