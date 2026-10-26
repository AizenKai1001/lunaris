use super::*;
use lunaris_json::parse as json;

const GRAVITY: &str = r#"
/// Fall, and bounce with some energy loss.
system gravity each Transform, Velocity without Frozen order 100 {
    Velocity.lin[1] = Velocity.lin[1] - 9.8 * dt;
    if Transform.pos[1] < 0 {
        Transform.pos[1] = 0;
        Velocity.lin[1] = -Velocity.lin[1] * 0.8;
    }
}
"#;

const GRAVITY_JSON: &str = r#"{
  "name": "gravity",
  "doc": "Fall, and bounce with some energy loss.",
  "each": ["Transform", "Velocity"],
  "without": ["Frozen"],
  "order": 100,
  "do": [
    { "set": "Velocity.lin[1]", "to": { "sub": [ { "get": "Velocity.lin[1]" }, { "mul": [9.8, { "dt": true }] } ] } },
    { "if": { "lt": [ { "get": "Transform.pos[1]" }, 0 ] },
      "then": [ { "set": "Transform.pos[1]", "to": 0 },
                { "set": "Velocity.lin[1]", "to": { "mul": [ { "neg": { "get": "Velocity.lin[1]" } }, 0.8 ] } } ] }
  ]
}"#;

fn one(src: &str) -> Compiled {
    match compile(src) {
        Ok(mut v) => v.remove(0),
        Err(e) => panic!("{}", e.iter().map(|d| d.to_text()).collect::<Vec<_>>().join("\n")),
    }
}

/// The same document with objects in a fixed key order and one-element argument lists of one-argument operations unwrapped.
fn canon(j: &Json) -> Json {
    match j {
        Json::Obj(o) => {
            let mut pairs: Vec<(String, Json)> = o
                .iter()
                .map(|(k, v)| {
                    let v = canon(v);
                    let v = match (&v, SINGLE.contains(&k.as_str())) {
                        (Json::Arr(a), true) if a.len() == 1 => a[0].clone(),
                        _ => v,
                    };
                    (k.clone(), v)
                })
                .collect();
            // `enabled: true` is what the engine says for a system nobody switched off
            pairs.retain(|(k, v)| !(k == "enabled" && *v == Json::Bool(true)));
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            Json::Obj(pairs)
        }
        Json::Arr(a) => Json::Arr(a.iter().map(canon).collect()),
        // 1 and 1.0 are different JSON but the same number to the engine
        other => other.clone(),
    }
}

#[test]
fn the_gravity_example_of_the_docs_compiles_to_exactly_its_json() {
    let c = one(GRAVITY);
    assert_eq!(c.name, "gravity");
    assert_eq!(c.def, json(GRAVITY_JSON).unwrap(), "{}", c.def.to_string_pretty().unwrap());
}

#[test]
fn and_the_json_decompiles_to_the_same_text_style() {
    let text = decompile(&json(GRAVITY_JSON).unwrap()).unwrap();
    assert_eq!(text.trim(), GRAVITY.trim());
    // and the loop closes
    assert_eq!(one(&text).def, json(GRAVITY_JSON).unwrap());
}

#[test]
fn every_system_of_every_example_survives_text_and_back() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("examples folder") {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".lunaris.json") {
            continue;
        }
        let scene = json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for def in scene.get("systems").and_then(|s| s.as_array()).unwrap_or(&[]) {
            if def.get("do").is_none() {
                continue; // a plugin or wasm system
            }
            let text = decompile(def).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let back = match compile(&text) {
                Ok(v) => v.into_iter().next().unwrap(),
                Err(e) => panic!("{}: {}\n{text}", path.display(), e.iter().map(|d| d.to_text()).collect::<Vec<_>>().join("\n")),
            };
            assert_eq!(canon(&back.def), canon(def), "{} / {}\n{text}", path.display(), back.name);
            // printing the compiled text again gives the same text: the text is the canonical form
            assert_eq!(decompile(&back.def).unwrap(), text, "{}", back.name);
            seen += 1;
        }
    }
    assert!(seen >= 20, "found {seen} systems: is the examples folder where it should be?");
}

// ------------------------------------------------------------------------------------------------ random expressions

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn random_expr(r: &mut Rng, depth: usize) -> Json {
    if depth == 0 || r.below(4) == 0 {
        return match r.below(12) {
            0 => Json::Int(r.below(100) as i64),
            1 => Json::Int(-(r.below(100) as i64)),
            2 => Json::Float(r.below(1000) as f64 / 8.0),
            3 => Json::Float(-(r.below(1000) as f64) / 8.0 - 0.125),
            4 => Json::Float(1.0e-7 * (1 + r.below(9)) as f64),
            5 => Json::Bool(r.below(2) == 0),
            6 => json(r#"{"get":"Transform.pos[1]"}"#).unwrap(),
            7 => json(r#"{"var":"speed"}"#).unwrap(),
            8 => json(r#"{"res":"input.space"}"#).unwrap(),
            9 => json(r#"{"dt":true}"#).unwrap(),
            10 => json(r#"{"tick":true}"#).unwrap(),
            _ => Json::str("a \"quoted\" text"),
        };
    }
    let mut all: Vec<&str> = Vec::new();
    all.extend(NARY);
    all.extend(BINARY);
    all.extend(TERNARY);
    all.extend(SINGLE);
    let op = all[r.below(all.len())];
    if SINGLE.contains(&op) {
        return Json::obj([(op.to_string(), random_expr(r, depth - 1))]);
    }
    let n = if NARY.contains(&op) {
        1 + r.below(4)
    } else if BINARY.contains(&op) {
        2
    } else {
        3
    };
    Json::obj([(op.to_string(), Json::Arr((0..n).map(|_| random_expr(r, depth - 1)).collect()))])
}

fn wrap(e: Json) -> Json {
    Json::obj([
        ("name", Json::str("t")),
        ("each", Json::arr([Json::str("Transform")])),
        ("do", Json::arr([Json::obj([("set", Json::str("Transform.pos[0]")), ("to", e)])])),
    ])
}

#[test]
fn random_expressions_print_and_parse_back_to_the_same_json() {
    let mut r = Rng(0x9E3779B97F4A7C15);
    for i in 0..3000 {
        let depth = 1 + r.below(5);
        let e = random_expr(&mut r, depth);
        let def = wrap(e);
        let text = decompile(&def).unwrap();
        match compile(&text) {
            Ok(v) => assert_eq!(v[0].def, def, "case {i}\n{text}"),
            Err(e) => panic!("case {i}: {}\n{text}", e.iter().map(|d| d.to_text()).collect::<Vec<_>>().join("\n")),
        }
    }
}

#[test]
fn precedence_and_flattening_follow_the_usual_rules() {
    let e = |src: &str| one(&format!("system t each A {{ A.x = {src}; }}")).def.get("do").unwrap().index(0).unwrap().get("to").unwrap().to_string_compact().unwrap();
    assert_eq!(e("1 + 2 * 3"), r#"{"add":[1,{"mul":[2,3]}]}"#);
    assert_eq!(e("1 + 2 + 3"), r#"{"add":[1,2,3]}"#);
    assert_eq!(e("(1 + 2) + 3"), r#"{"add":[{"add":[1,2]},3]}"#);
    assert_eq!(e("10 - 2 - 3"), r#"{"sub":[{"sub":[10,2]},3]}"#);
    assert_eq!(e("10 - (2 - 3)"), r#"{"sub":[10,{"sub":[2,3]}]}"#);
    assert_eq!(e("a < b && c || d"), r#"{"or":[{"and":[{"lt":[{"var":"a"},{"var":"b"}]},{"var":"c"}]},{"var":"d"}]}"#);
    assert_eq!(e("-x * 2"), r#"{"mul":[{"neg":{"var":"x"}},2]}"#);
    assert_eq!(e("2 * -3"), r#"{"mul":[2,-3]}"#);
    assert_eq!(e("!(a == b)"), r#"{"not":{"eq":[{"var":"a"},{"var":"b"}]}}"#);
    assert_eq!(e("clamp(x, 0, 1)"), r#"{"clamp":[{"var":"x"},0,1]}"#);
    assert_eq!(e("$input.space + $score"), r#"{"add":[{"res":"input.space"},{"res":"score"}]}"#);
    assert_eq!(e("1e-3 + 2.5"), r#"{"add":[0.001,2.5]}"#);
    assert_eq!(e("a.Transform.pos[0]"), r#"{"get":"a.Transform.pos[0]"}"#);
    assert_eq!(e("uid(b)"), r#"{"uid":"b"}"#);
}

#[test]
fn compound_assignments_resources_and_every_statement_form() {
    let src = r#"
system everything each Transform, Velocity {
    Transform.pos[0] += 1;
    Transform.pos[1] -= dt;
    Transform.pos[2] *= 2;
    Transform.pos[2] /= 4;
    let speed = hypot(Velocity.lin[0], Velocity.lin[2]);
    $score += 1;
    $score -= speed;
    $timer = 10;
    emit "ping";
    despawn;
    spawn { Transform { pos: 0 }, Velocity {} };
    spawn_prefab "Bullet" at (1, 2, 3) velocity (0, 0, speed) with { Tag { n: 4 } };
    add Frozen;
    add Health { hp: 10 };
    remove Frozen;
    if speed > 3 { despawn; } else if speed > 1 { emit "slow"; } else { $x = 1; }
}
"#;
    let c = one(src);
    let text = decompile(&c.def).unwrap();
    assert_eq!(one(&text).def, c.def, "\n{text}");
    let d = c.def.get("do").unwrap().as_array().unwrap();
    assert_eq!(d[0].to_string_compact().unwrap(), r#"{"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[0]"},1]}}"#);
    assert_eq!(d[6].to_string_compact().unwrap(), r#"{"res_add":"score","by":{"neg":{"var":"speed"}}}"#);
    assert_eq!(d[11].to_string_compact().unwrap(), r#"{"spawn_prefab":"Bullet","at":[1,2,3],"velocity":[0,0,{"var":"speed"}],"with":{"Tag":{"n":4}}}"#);
    // an else-if chain is a nested if in the else list
    assert_eq!(d[15].get("else").unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn contact_systems_and_their_roles() {
    let src = r#"
/// Pick up orbs.
system collect on_contact a(Player) b(Orb) triggers only order 5 disabled {
    $score += b.Orb.value;
    despawn b;
    add Glow on a;
    remove Glow on b;
    if contact.depth > 0.1 && uid(a) < uid(b) { emit "bump"; }
}
"#;
    let c = one(src);
    assert_eq!(c.def.get("on_contact").unwrap().to_string_compact().unwrap(), r#"{"a":["Player"],"b":["Orb"],"triggers":"only"}"#);
    assert_eq!(c.def.get("enabled").unwrap().as_bool(), Some(false));
    let text = decompile(&c.def).unwrap();
    assert_eq!(one(&text).def, c.def, "\n{text}");
    assert!(text.contains("/// Pick up orbs."));
}

#[test]
fn what_the_language_has_no_words_for_is_kept_as_json() {
    let def = json(r#"{"name":"odd","each":["A"],"do":[{"future_statement":{"x":1}},{"set":"A.x","to":{"future_op":[1,2]}},{"set":"A.x","to":{"get":"weird path!"}}]}"#).unwrap();
    let text = decompile(&def).unwrap();
    assert!(text.contains("json("), "{text}");
    assert_eq!(one(&text).def, def);
    // a compiled plugin has no script form
    assert!(decompile(&json(r#"{"name":"p","plugin":"x.dll","each":["A"]}"#).unwrap()).is_err());
}

// ------------------------------------------------------------------------------------------------ errors

fn first_error(src: &str) -> Diagnostic {
    compile(src).expect_err("should not compile").remove(0)
}

#[test]
fn errors_say_where_and_what() {
    let src = "system t each A {\n    A.x = 1\n    A.y = 2;\n}\n";
    let e = first_error(src);
    assert_eq!((e.line, e.col), (2, 12), "{e:?}");
    assert!(e.message.contains("expected `;`"), "{e:?}");

    let e = first_error("system t each A { A.x = frobnicate(1); }");
    assert!(e.message.contains("not a function") && e.hint.as_deref().is_some_and(|h| h.contains("sqrt")), "{e:?}");

    let e = first_error("system t each A { A.x = clamp(1, 2); }");
    assert!(e.message.contains("exactly 3"), "{e:?}");

    let e = first_error("system t each A { if A.x > 1 { A.y = 2; }");
    assert!(e.message.contains("never closed"), "{e:?}");

    let e = first_error("system t each A { emit \"open; }");
    assert!(e.message.contains("never closed"), "{e:?}");

    let e = first_error("system t each A { x = 3; }");
    assert!(e.message.contains("not something that can be assigned"), "{e:?}");

    let all = compile("system t each A { A.x = 1 @ 2; }").expect_err("errors");
    assert!(all.iter().any(|d| d.message.contains("unexpected character `@`") && d.col == 27), "{all:?}");

    let e = first_error("A.x = 1;");
    assert!(e.message.contains("expected `system`"), "{e:?}");

    let e = first_error("");
    assert!(e.message.contains("no system"), "{e:?}");

    let e = first_error("system t each A { let if = 3; }");
    assert!(e.message.contains("keyword"), "{e:?}");
}

#[test]
fn several_mistakes_are_all_reported() {
    let src = "system t each A {\n    A.x = ;\n    A.y = 2;\n    A.z = (1 + ;\n    emit;\n}\n";
    let errors = compile(src).expect_err("errors");
    let lines: Vec<usize> = errors.iter().map(|d| d.line).collect();
    assert_eq!(lines, vec![2, 4, 5], "{errors:#?}");
}

#[test]
fn an_error_path_from_the_engine_points_into_the_text() {
    let src = "system t each Transform {\n    Transform.pos[0] = 1 + sqr(2);\n    Transform.pos[1] = 2 * (3 - bad);\n}\n";
    // sqr is not a function, so this does not compile: use a valid one for the path test
    let src2 = src.replace("sqr(2)", "sqrt(2)");
    let c = one(&src2);
    let at = |p: &str| c.locate(p).map(|r| src2[r].to_string());
    assert_eq!(at("do[0].to.add[1]").as_deref(), Some("sqrt(2)"));
    assert_eq!(at("do[0].to.add[1].sqrt").as_deref(), Some("sqrt(2)"));
    assert_eq!(at("do[0].to.add[1].sqrt.neg").as_deref(), Some("sqrt(2)"), "an unknown deeper path falls back to the nearest part");
    assert_eq!(at("do[1].to.mul[1].sub[1]").as_deref(), Some("bad"));
    assert_eq!(at("do[1].set").as_deref(), Some("Transform.pos[1]"));
    assert_eq!(at("do[1]").as_deref(), Some("Transform.pos[1] = 2 * (3 - bad);"));
    assert_eq!(at("name").as_deref(), Some("t"));
    assert!(at("do[9].to").is_some(), "falls back to the whole system");
}

#[test]
fn the_reader_never_panics_on_nonsense() {
    let vocab = [
        "system", "each", "without", "on_contact", "a", "b", "(", ")", "{", "}", "[", "]", ",", ";", ":", ".", "=", "+=", "-", "-=", "*", "/", "%", "<", "<=", "!", "!=", "&&", "||", "if", "else", "let", "x", "Transform", "pos",
        "1", "2.5", "1e9", "1e", "\"text\"", "\"open", "$score", "$", "$\"a b\"", "// c\n", "/// doc\n", "/* c */", "/*", "json(", "json({\"a\":1})", "uid", "dt", "spawn", "spawn_prefab", "at", "add", "remove", "on", "é", "😀", "\n",
    ];
    let mut r = Rng(12345);
    for _ in 0..4000 {
        let n = 1 + r.below(40);
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(vocab[r.below(vocab.len())]);
            if r.below(3) != 0 {
                s.push(' ');
            }
        }
        let _ = compile(&s);
        let spans = highlight(&s);
        // the colours cover real, ordered, non-overlapping pieces of the text
        let mut at = 0;
        for h in &spans {
            assert!(h.start >= at && h.end >= h.start && h.end <= s.len() && s.is_char_boundary(h.start) && s.is_char_boundary(h.end), "{s:?}");
            at = h.end;
        }
    }
}

#[test]
fn highlighting_names_the_parts() {
    let src = "system fall each Transform { Transform.pos[1] -= sin(x) * 2; // hi\n $s = \"t\"; }";
    let hl = highlight(src);
    let class_of = |word: &str| {
        let start = src.find(word).unwrap();
        hl.iter().find(|h| h.start == start).unwrap().class
    };
    assert_eq!(class_of("system"), Class::Keyword);
    assert_eq!(class_of("fall"), Class::Variable);
    assert_eq!(class_of("Transform"), Class::Type);
    assert_eq!(class_of("pos"), Class::Property);
    assert_eq!(class_of("sin"), Class::Function);
    assert_eq!(class_of("2"), Class::Number);
    assert_eq!(class_of("// hi"), Class::Comment);
    assert_eq!(class_of("$s"), Class::Resource);
    assert_eq!(class_of("\"t\""), Class::String);
    assert_eq!(class_of("-="), Class::Operator);
}

#[test]
fn line_and_column_conversions_agree() {
    let src = "ab\ncdé\n\nlast";
    let idx = LineIndex::new(src);
    for off in 0..=src.len() {
        if !src.is_char_boundary(off) {
            continue;
        }
        let (l, c) = idx.line_col(src, off);
        assert_eq!(idx.offset(src, l, c), off, "offset {off} -> {l}:{c}");
    }
    assert_eq!(idx.line_count(), 4);
}

#[test]
fn a_file_may_hold_several_systems_and_names_must_differ() {
    let two = "system a each A { A.x = 1; }\nsystem b each B { B.y = 2; }";
    let v = compile(two).unwrap();
    assert_eq!(v.len(), 2);
    assert_eq!(&two[v[1].range.clone()], "system b each B { B.y = 2; }");
    let e = compile("system a each A { }\nsystem a each A { }").unwrap_err();
    assert!(e[0].message.contains("already a system named `a`"), "{e:?}");
}

#[test]
fn a_resource_name_may_have_a_number_between_dots() {
    let v = compile("system a each A { $game.has.1 = A.x; A.x = $game.has.1 + 1; A.y = $a.5; }").unwrap();
    let text = v[0].def.to_string_compact().unwrap();
    assert!(text.contains(r#""res_set":"game.has.1""#) && text.contains(r#"{"res":"game.has.1"}"#) && text.contains(r#"{"res":"a.5"}"#), "{text}");
}

#[test]
fn a_field_of_with_can_be_an_array_in_square_brackets_and_prints_back_the_same() {
    let src = "system mark each Transform {\n    let k = Transform.pos[0];\n    spawn_prefab \"Mark\" at (1, 2, 3) with { Decal { normal: [0, k, 1], size: [2, 3] }, Transform { rot: [0, 0, 0, 1], scale: [] } };\n}\n";
    let c = one(src);
    let d = c.def.get("do").unwrap().as_array().unwrap();
    assert_eq!(d[1].to_string_compact().unwrap(), r#"{"spawn_prefab":"Mark","at":[1,2,3],"with":{"Decal":{"normal":[0,{"var":"k"},1],"size":[2,3]},"Transform":{"rot":[0,0,0,1],"scale":[]}}}"#);
    let text = decompile(&c.def).unwrap();
    assert!(text.contains("normal: [0, k, 1]"), "{text}");
    assert_eq!(one(&text).def, c.def, "\n{text}");
    // a trailing comma is fine, a missing bracket or comma is told
    assert!(compile("system a each A { spawn_prefab \"M\" with { C { v: [1, 2,] } }; }").is_ok());
    let e = compile("system a each A { spawn_prefab \"M\" with { C { v: [1 2] } }; }").unwrap_err();
    assert!(e.iter().any(|d| d.message.contains("expected `,` or `]`")), "{e:?}");
    let e = compile("system a each A { spawn_prefab \"M\" with { C { v: [1, 2, w: 3 } }; }").unwrap_err();
    assert!(!e.is_empty());
}
