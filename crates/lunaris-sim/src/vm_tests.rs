//! The batch vm must do exactly what the interpreter does. These tests generate random typed programs and random worlds, run
//! both, and compare everything that can differ: the state hash after every tick (fields, structural changes, resources) and
//! the recorded error message of every system.
use super::*;
use lunaris_json::{parse, Json};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

fn obj(k: &str, v: Json) -> Json {
    Json::Obj(vec![(k.to_string(), v)])
}
fn get(path: &str) -> Json {
    obj("get", Json::str(path))
}

#[derive(Clone, Copy, PartialEq)]
enum T {
    Num,
    Bool,
}

struct Gen<'a> {
    rng: &'a mut Rng,
    /// (name, type) of variables in scope
    vars: Vec<(String, T)>,
    next_var: usize,
    /// whether the system's `each` list includes Velocity (paths into components outside `each` do not compile)
    velocity: bool,
}

const NUM_GETS: [&str; 12] = ["Transform.pos[0]", "Transform.pos[1]", "Transform.pos[2]", "Velocity.lin[0]", "Velocity.lin[1]", "Stat.a", "Stat.b", "Stat.c", "Stat.d", "Stat.big", "Stat.a", "Stat.c"];
const NUM_SETS: [&str; 8] = ["Transform.pos[0]", "Transform.pos[1]", "Velocity.lin[0]", "Stat.a", "Stat.b", "Stat.c", "Stat.d", "Stat.big"];

impl Gen<'_> {
    fn get_path(&mut self) -> &'static str {
        loop {
            let p = *self.rng.pick(&NUM_GETS);
            if self.velocity || !p.starts_with("Velocity") {
                return p;
            }
        }
    }

    fn set_path(&mut self) -> &'static str {
        loop {
            let p = *self.rng.pick(&NUM_SETS);
            if self.velocity || !p.starts_with("Velocity") {
                return p;
            }
        }
    }

    fn constant(&mut self) -> Json {
        match self.rng.below(8) {
            0 => Json::Int(0),
            1 => Json::Int(1),
            2 => Json::Int(-2),
            3 => Json::Float(0.5),
            4 => Json::Float(3.25),
            5 => Json::Float(-7.75),
            6 => Json::Int(100),
            _ => Json::Float(1e9),
        }
    }

    fn num(&mut self, depth: usize) -> Json {
        if depth == 0 || self.rng.below(5) == 0 {
            return match self.rng.below(9) {
                0 | 1 => self.constant(),
                2 | 3 | 4 => get(self.get_path()),
                5 => obj("dt", Json::Bool(true)),
                6 => [obj("tick", Json::Bool(true)), obj("uid", Json::Bool(true))][self.rng.below(2)].clone(),
                7 => obj("res", Json::str(["r0", "r1", "input.k"][self.rng.below(3)])),
                _ => {
                    let nums: Vec<String> = self.vars.iter().filter(|v| v.1 == T::Num).map(|v| v.0.clone()).collect();
                    if nums.is_empty() { self.constant() } else { obj("var", Json::str(nums[self.rng.below(nums.len())].clone())) }
                }
            };
        }
        let d = depth - 1;
        match self.rng.below(20) {
            0 => obj("add", Json::arr((0..2 + self.rng.below(2)).map(|_| self.num(d)).collect::<Vec<_>>())),
            1 => obj("mul", Json::arr((0..2 + self.rng.below(2)).map(|_| self.num(d)).collect::<Vec<_>>())),
            2 => obj("min", Json::arr((0..2).map(|_| self.num(d)).collect::<Vec<_>>())),
            3 => obj("max", Json::arr((0..2).map(|_| self.num(d)).collect::<Vec<_>>())),
            4 => obj("sub", Json::arr([self.num(d), self.num(d)])),
            5 => obj("div", Json::arr([self.num(d), if self.rng.below(2) == 0 { get("Stat.c") } else { self.num(d) }])),
            6 => obj("mod", Json::arr([self.num(d), if self.rng.below(2) == 0 { get("Stat.c") } else { self.num(d) }])),
            7 => obj(["neg", "abs", "floor", "ceil", "round"][self.rng.below(5)], self.num(d)),
            8 => obj("sqrt", self.num(d)),
            9 => obj(["sin", "cos"][self.rng.below(2)], self.num(d)),
            10 => obj("clamp", Json::arr([self.num(d), self.num(d), self.num(d)])),
            11 => obj("lerp", Json::arr([self.num(d), self.num(d), self.num(d)])),
            12 => obj("rand", self.constant()),
            13 | 14 => obj("select", Json::arr([self.boolean(d), self.num(d), self.num(d)])),
            15 => obj(["sign", "fract", "trunc", "tan", "atan", "exp", "ln"][self.rng.below(7)], self.num(d)),
            16 => obj(["atan2", "pow", "hypot"][self.rng.below(3)], Json::arr([self.num(d), self.num(d)])),
            17 => obj("smoothstep", Json::arr([self.num(d), self.num(d), self.num(d)])),
            18 => obj("noise", Json::arr([self.num(d), self.num(d), self.num(d)])),
            _ => self.num(d),
        }
    }

    fn sym(&mut self) -> Json {
        if self.rng.below(2) == 0 { get("Stat.tag") } else { Json::str(["red", "blue", ""][self.rng.below(3)]) }
    }

    fn boolean(&mut self, depth: usize) -> Json {
        if depth == 0 || self.rng.below(6) == 0 {
            return match self.rng.below(5) {
                0 => Json::Bool(self.rng.below(2) == 0),
                1 | 2 => get("Stat.flag"),
                3 => obj("lt", Json::arr([get(self.get_path()), self.constant()])),
                _ => {
                    let bools: Vec<String> = self.vars.iter().filter(|v| v.1 == T::Bool).map(|v| v.0.clone()).collect();
                    if bools.is_empty() { Json::Bool(true) } else { obj("var", Json::str(bools[self.rng.below(bools.len())].clone())) }
                }
            };
        }
        let d = depth - 1;
        match self.rng.below(11) {
            0..=3 => obj(["lt", "le", "gt", "ge"][self.rng.below(4)], Json::arr([self.num(d), self.num(d)])),
            4 => obj(["eq", "ne"][self.rng.below(2)], Json::arr([self.num(d), self.num(d)])),
            5 => obj(["eq", "ne"][self.rng.below(2)], Json::arr([self.sym(), self.sym()])),
            6 => obj(["eq", "ne"][self.rng.below(2)], Json::arr([self.boolean(d), self.boolean(d)])),
            7 | 8 => obj(["and", "or"][self.rng.below(2)], Json::arr((0..2 + self.rng.below(2)).map(|_| self.boolean(d)).collect::<Vec<_>>())),
            9 => obj("not", self.boolean(d)),
            _ => obj("select", Json::arr([self.boolean(d), self.boolean(d), self.boolean(d)])),
        }
    }

    fn block(&mut self, depth: usize, len: usize) -> Json {
        let mark = self.vars.len();
        let stmts: Vec<Json> = (0..len).map(|_| self.stmt(depth)).collect();
        self.vars.truncate(mark);
        Json::Arr(stmts)
    }

    fn stmt(&mut self, depth: usize) -> Json {
        let mut o: Vec<(String, Json)> = Vec::new();
        match self.rng.below(16) {
            0..=5 => {
                o.push(("set".into(), Json::str(self.set_path())));
                o.push(("to".into(), self.num(3)));
            }
            6 => {
                o.push(("set".into(), Json::str("Stat.flag")));
                o.push(("to".into(), self.boolean(3)));
            }
            7 => {
                o.push(("set".into(), Json::str("Stat.tag")));
                o.push(("to".into(), self.sym()));
            }
            8 | 9 => {
                let name = format!("v{}", self.next_var);
                self.next_var += 1;
                let (ty, e) = if self.rng.below(3) == 0 { (T::Bool, self.boolean(3)) } else { (T::Num, self.num(3)) };
                o.push(("let".into(), Json::str(name.clone())));
                o.push(("be".into(), e));
                self.vars.push((name, ty));
            }
            10..=12 if depth > 0 => {
                o.push(("if".into(), self.boolean(3)));
                let len = 1 + self.rng.below(3);
                o.push(("then".into(), self.block(depth - 1, len)));
                if self.rng.below(2) == 0 {
                    let len = 1 + self.rng.below(2);
                    o.push(("else".into(), self.block(depth - 1, len)));
                }
            }
            13 => {
                o.push(("res_add".into(), Json::str(["r0", "total", "r1"][self.rng.below(3)])));
                o.push(("by".into(), self.num(2)));
            }
            14 => {
                // rare structural changes, always behind a condition so most entities survive
                o.push(("if".into(), obj("lt", Json::arr([obj("rand", Json::Int(self.rng.below(5) as i64)), Json::Float(0.05)]))));
                let inner = match self.rng.below(5) {
                    4 => Json::Obj(vec![("emit".into(), Json::str(["ping", "fx.hit", "coin"][self.rng.below(3)]))]),
                    0 => Json::Obj(vec![("despawn".into(), Json::Bool(true))]),
                    1 => Json::Obj(vec![("add".into(), Json::str("Wander")), ("values".into(), obj("strength", self.num(2)))]),
                    2 => Json::Obj(vec![("remove".into(), Json::str("Wander"))]),
                    _ => Json::Obj(vec![("spawn".into(), obj("Stat", Json::Obj(vec![("a".into(), self.num(2)), ("c".into(), self.num(1)), ("flag".into(), self.boolean(2))])))]),
                };
                o.push(("then".into(), Json::Arr(vec![inner])));
            }
            _ => {
                o.push(("res_set".into(), Json::str("last")));
                o.push(("to".into(), self.num(2)));
            }
        }
        Json::Obj(o)
    }
}

fn build_sim(rng: &mut Rng, entities: usize) -> Sim {
    let mut s = Sim::new(rng.next());
    s.threads = 1 + rng.below(3);
    s.apply(&Command::Define { def: parse(r#"{"name":"Stat","fields":[{"name":"a","type":"f32"},{"name":"b","type":"f64"},{"name":"c","type":"i32"},{"name":"d","type":"u32"},{"name":"big","type":"i64"},{"name":"flag","type":"bool"},{"name":"tag","type":"str"}]}"#).unwrap() }).unwrap();
    for i in 0..entities {
        let mut comps: Vec<(String, Json)> = vec![
            ("Transform".into(), parse(&format!(r#"{{"pos":[{},{},{}]}}"#, (rng.below(40) as f64 - 20.0) / 4.0, rng.below(10), rng.below(7))).unwrap()),
            ("Stat".into(), parse(&format!(r#"{{"a":{},"b":{},"c":{},"d":{},"big":{},"flag":{},"tag":"{}"}}"#, rng.below(9) as f64 - 4.0, (rng.below(100) as f64) / 7.0, rng.below(5) as i64 - 2, rng.below(5), rng.below(1000) as i64 - 500, rng.below(2) == 0, ["red", "blue", ""][rng.below(3)])).unwrap()),
        ];
        if i % 3 != 0 {
            comps.push(("Velocity".into(), parse(&format!(r#"{{"lin":[{},{},0]}}"#, rng.below(5) as f64 - 2.0, rng.below(5) as f64 - 2.0)).unwrap()));
        }
        if i % 5 == 0 {
            comps.push(("Wander".into(), parse("{}").unwrap()));
        }
        s.apply(&Command::Spawn { uid: Some(i as u64 + 1), components: comps }).unwrap();
    }
    for (k, v) in [("r0", 3.0), ("r1", -1.5), ("input.k", 1.0)] {
        s.apply(&Command::SetResource { name: k.into(), value: Some(v) }).unwrap();
    }
    s
}

fn last_errors(s: &Sim) -> Vec<Option<String>> {
    s.scripts.systems.iter().map(|x| x.last_error.clone()).collect()
}

#[test]
fn the_batch_vm_matches_the_interpreter_on_random_programs() {
    let seed = std::env::var("LUNARIS_VM_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(11u64);
    let programs: usize = std::env::var("LUNARIS_VM_PROGRAMS").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let mut rng = Rng(seed);
    let (mut lowered, mut with_errors, mut compared) = (0usize, 0usize, 0usize);
    let mut unlowered: Vec<String> = Vec::new();
    for p in 0..programs {
        let entities = [1usize, 7, 130, 257, 300][rng.below(5)];
        let world_seed = rng.next();
        let mut a = build_sim(&mut Rng(world_seed), entities);
        let mut b = build_sim(&mut Rng(world_seed), entities);
        b.scripts.interpret_only = true;
        let velocity = rng.below(2) == 0;
        let mut g = Gen { rng: &mut rng, vars: Vec::new(), next_var: 0, velocity };
        let each = if velocity { r#"["Transform","Velocity","Stat"]"# } else { r#"["Transform","Stat"]"# };
        let len = 2 + g.rng.below(5);
        let body = g.block(2, len);
        let def = parse(&format!(r#"{{"name":"prog","each":{each},"do":{}}}"#, body.to_string_compact().unwrap())).unwrap();
        // the same definition goes to both simulations; a definition either compiles for both or for neither
        let (ra, rb) = (a.apply(&Command::DefineSystem { def: def.clone() }), b.apply(&Command::DefineSystem { def }));
        assert_eq!(ra.is_ok(), rb.is_ok());
        let Ok(_) = ra else { continue };
        if a.scripts.systems[0].uses_batch() {
            lowered += 1;
        } else if unlowered.len() < 3 {
            unlowered.push(a.scripts.systems[0].def.to_string_compact().unwrap());
        }
        for tick in 0..10 {
            a.step();
            b.step();
            assert_eq!(a.state_hash(), b.state_hash(), "program {p}, tick {tick}: the vm and the interpreter disagree\n{}", a.scripts.systems[0].def.to_string_pretty().unwrap());
            assert_eq!(last_errors(&a), last_errors(&b), "program {p}, tick {tick}: error messages differ");
            assert_eq!(a.world.resources(), b.world.resources());
            assert_eq!(a.events, b.events, "program {p}, tick {tick}: events differ");
            compared += 1;
        }
        if last_errors(&a)[0].is_some() {
            with_errors += 1;
        }
    }
    // the generator must actually exercise the vm, and errors, or the comparison proves little
    assert!(lowered * 10 >= programs * 8, "only {lowered} of {programs} programs lowered to the vm; some that did not:
{}", unlowered.join("
"));
    assert!(with_errors * 10 >= programs, "only {with_errors} programs hit a runtime error");
    println!("vm vs interpreter: {programs} programs, {lowered} lowered, {with_errors} with runtime errors, {compared} tick comparisons");
}

#[test]
fn short_circuit_and_branches_only_raise_errors_where_they_run() {
    // `Stat.c` is 0 for some entities. The division sits behind a guard, in an untaken select branch and in an untaken `if`:
    // the interpreter never evaluates it for those entities, so the vm must not report it either.
    let mut rng = Rng(5);
    let mut s = build_sim(&mut rng, 300);
    s.apply(&Command::DefineSystem { def: parse(r#"{"name":"guarded","each":["Transform","Stat"],"do":[
        {"set":"Stat.a","to":{"select":[{"ne":[{"get":"Stat.c"},0]},{"div":[10,{"get":"Stat.c"}]},-1]}},
        {"if":{"and":[{"ne":[{"get":"Stat.c"},0]},{"gt":[{"div":[10,{"get":"Stat.c"}]},0]}]},"then":[{"set":"Stat.b","to":1}]},
        {"if":{"ne":[{"get":"Stat.c"},0]},"then":[{"set":"Stat.d","to":{"mod":[17,{"get":"Stat.c"}]}}]}]}"#).unwrap() }).unwrap();
    assert!(s.scripts.systems[0].uses_batch());
    s.step_n(3);
    assert_eq!(s.scripts.systems[0].last_error, None, "no entity may report an error from a branch it did not take");
    // and an unguarded division reports the entity with the smallest uid that hit it, the same text as the interpreter
    let mut a = build_sim(&mut Rng(5), 300);
    let mut b = build_sim(&mut Rng(5), 300);
    b.scripts.interpret_only = true;
    let def = parse(r#"{"name":"unguarded","each":["Transform","Stat"],"do":[{"set":"Stat.a","to":{"div":[10,{"get":"Stat.c"}]}},{"set":"Stat.b","to":2}]}"#).unwrap();
    a.apply(&Command::DefineSystem { def: def.clone() }).unwrap();
    b.apply(&Command::DefineSystem { def }).unwrap();
    a.step();
    b.step();
    assert!(a.scripts.systems[0].last_error.as_deref().unwrap().contains("division by zero"));
    assert_eq!(last_errors(&a), last_errors(&b));
    assert_eq!(a.state_hash(), b.state_hash(), "rows that failed must keep exactly the writes that happened before the error");
}

#[test]
fn programs_that_mix_types_stay_on_the_interpreter() {
    let mut s = build_sim(&mut Rng(1), 10);
    // adding a boolean to a number is only an error at run time in the interpreter; the vm refuses to lower it
    s.apply(&Command::DefineSystem { def: parse(r#"{"name":"mixed","each":["Stat"],"do":[{"set":"Stat.a","to":{"add":[{"get":"Stat.flag"},1]}}]}"#).unwrap() }).unwrap();
    assert!(!s.scripts.systems[0].uses_batch());
    s.step();
    assert!(s.scripts.systems[0].last_error.as_deref().unwrap().contains("expected a number"));
}

#[test]
fn rounding_and_modulo_agree_on_the_awkward_values() {
    // exact halves and negative operands are where rounding conventions differ
    let build = |interpret: bool| {
        let mut s = Sim::new(1);
        s.apply(&Command::Define { def: parse(r#"{"name":"Stat","fields":[{"name":"a","type":"f32"},{"name":"b","type":"f64"},{"name":"c","type":"f64"}]}"#).unwrap() }).unwrap();
        for (i, v) in [-2.5, -1.5, -0.5, 0.5, 1.5, 2.5, -7.25, 7.75].iter().enumerate() {
            s.apply(&Command::Spawn { uid: Some(i as u64 + 1), components: vec![("Stat".into(), parse(&format!(r#"{{"b":{v}}}"#)).unwrap())] }).unwrap();
        }
        s.scripts.interpret_only = interpret;
        s.apply(&Command::DefineSystem { def: parse(r#"{"name":"r","each":["Stat"],"do":[
            {"set":"Stat.a","to":{"round":{"get":"Stat.b"}}},
            {"set":"Stat.c","to":{"add":[{"mod":[{"get":"Stat.b"},3]},{"floor":{"get":"Stat.b"}},{"ceil":{"get":"Stat.b"}}]}}]}"#).unwrap() }).unwrap();
        s.step();
        s
    };
    let (a, b) = (build(false), build(true));
    assert!(a.scripts.systems[0].uses_batch());
    assert_eq!(a.state_hash(), b.state_hash());
}

