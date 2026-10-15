//! What rollback costs, measured: taking a checkpoint of the simulation, going back to one, playing a tick, and a whole rollback of 8 ticks
//! (go back, play 8 ticks again, taking the checkpoints a client takes as it plays). The numbers are printed (`cargo test -p lunaris-net
//! --release --test rollback_cost -- --nocapture`) and quoted in `docs/NETWORK.md`. What is asserted is that a rollback is exact: the state
//! after it is the state of a game that never went back.
use lunaris_api::Engine;
use lunaris_json::Json;
use std::hint::black_box;
use std::time::Instant;

fn load(scene: &str) -> Engine {
    let mut e = Engine::new(1, "../..");
    e.call("load_scene", &Json::obj([("path", Json::str(scene))])).expect("load the scene");
    e
}

fn micros(t: Instant, n: u32) -> f64 {
    t.elapsed().as_secs_f64() * 1e6 / n as f64
}

#[test]
fn what_a_rollback_of_eight_ticks_costs() {
    println!("ROLLBACK COST (microseconds, this machine, this build profile)");
    println!("{:<34} {:>8} {:>10} {:>10} {:>9} {:>9} {:>14}", "scene", "entities", "Sim::clone", "checkpoint", "rewind", "step", "rollback of 8");
    for scene in ["examples/skirmish.lunaris.json", "examples/duel.lunaris.json", "examples/orb_dash.lunaris.json", "examples/pile.lunaris.json"] {
        let mut e = load(scene);
        e.sim.step_n(90); // a game that has been going on for a while: things have moved, events have happened
        let reps = 400u32;

        let t = Instant::now();
        for _ in 0..reps {
            black_box(e.sim.clone());
        }
        let clone_us = micros(t, reps);

        let t = Instant::now();
        for _ in 0..reps {
            black_box(e.sim.checkpoint());
        }
        let checkpoint_us = micros(t, reps);

        let cp = e.sim.checkpoint();
        let t = Instant::now();
        for _ in 0..reps {
            e.sim.rewind_to(&cp).expect("rewind");
        }
        let rewind_us = micros(t, reps);

        // one tick, on its own
        let t = Instant::now();
        for _ in 0..reps {
            e.sim.rewind_to(&cp).expect("rewind");
            e.sim.step();
        }
        let step_us = micros(t, reps) - rewind_us;

        // what the client does when a guess was wrong: back to a saved tick, then the 8 ticks again, saving a checkpoint before each
        let want = {
            let mut straight = e.sim.clone();
            straight.rewind_to(&cp).expect("rewind");
            straight.step_n(8);
            straight.state_hash()
        };
        let mut ring = Vec::with_capacity(9);
        let t = Instant::now();
        for _ in 0..reps {
            ring.clear();
            e.sim.rewind_to(&cp).expect("rewind");
            for _ in 0..8 {
                ring.push(e.sim.checkpoint());
                e.sim.step();
            }
        }
        let rollback_us = micros(t, reps);
        black_box(&ring);
        assert_eq!(e.sim.state_hash(), want, "{scene}: a rollback must give exactly the state of a game that never went back");
        assert_eq!(e.sim.tick, cp.tick() + 8);
        println!("{:<34} {:>8} {:>10.1} {:>10.1} {:>9.1} {:>9.1} {:>14.1}", scene, e.sim.world.len(), clone_us, checkpoint_us, rewind_us, step_us, rollback_us);
    }
}
