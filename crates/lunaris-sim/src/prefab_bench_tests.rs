//! How long it takes to spawn a prefab of three thousand children (a cell of a streamed world: trees and houses), stage by stage. `cargo test --release -p lunaris-sim spawning_cost -- --ignored --nocapture`.
use super::*;
use lunaris_json::parse;

#[test]
#[ignore]
fn spawning_cost_of_a_cell_sized_prefab_by_stage() {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Tree".into(), def: parse(r#"{"components":{"MeshRenderer":{"mesh":"cube","roughness":0.8}}}"#).unwrap() }).unwrap();
    let kids: Vec<String> = (0..3000)
        .map(|i| format!(r#"{{"prefab":"Tree","components":{{"LocalTransform":{{"pos":[{},0,{}],"rot":[0,0.1,0,0.99],"scale":[1,2,1]}}}}}}"#, i % 60, i / 60))
        .collect();
    let def = format!(r#"{{"components":{{"Transform":{{"pos":[0,0,0]}}}},"children":[{}]}}"#, kids.join(","));
    s.apply(&Command::DefinePrefab { name: "Cell".into(), def: parse(&def).unwrap() }).unwrap();
    let t = std::time::Instant::now();
    let plan = crate::prefab::plan(&s.scripts.prefabs, "Cell", &[]).unwrap();
    let t_plan = t.elapsed();
    let t = std::time::Instant::now();
    let cmds = crate::prefab::spawn_commands(&plan, s.world.next_uid());
    let t_cmds = t.elapsed();
    let t = std::time::Instant::now();
    crate::command::apply_batch(&mut s.world, &mut s.scripts, &cmds).unwrap();
    let t_apply = t.elapsed();
    println!("plan {t_plan:?}, commands {t_cmds:?}, apply {t_apply:?} for {} entities", plan.len());
    let t = std::time::Instant::now();
    s.step();
    println!("the first tick after: {:?}", t.elapsed());
}
