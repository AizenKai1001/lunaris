//! Raycast vehicles as an agent meets them: `check` names what is wrong with a car, `render` can draw its wheels, the controls are plain fields that `set`
//! writes, and the example `examples/driving.lunaris.json` plays itself in `examples/tests/driving.test.json`.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &j(args)).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

const WHEELS: &str = "defaults: radius=0.35 rest=0.3 travel=0.2 spring=35000 damper=3200 grip=1.2\\nfl: -0.8 -0.15 -1.3 steer=1\\nfr: 0.8 -0.15 -1.3 steer=1\\nrl: -0.8 -0.15 1.3 drive=1 handbrake=1\\nrr: 0.8 -0.15 1.3 drive=1 handbrake=1";

fn car_with(e: &mut Engine, uid: u64, wheels: &str, vehicle_extra: &str, others: &str) {
    call(e, "spawn", &format!(r#"{{"uid":{uid},"components":{{"Transform":{{"pos":[0,0.72,0]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":1,"size":[1.8,0.7,4.2]}},"RigidBody":{{"mass":1200}},"Vehicle":{{"wheels":"{wheels}"{vehicle_extra}}}{others}}}}}"#));
}

fn problems(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

fn about_cars(e: &mut Engine) -> Vec<String> {
    problems(e).into_iter().filter(|m| m.contains("Vehicle") || m.contains("wheel") || m.contains("spring") || m.contains("chassis") || m.contains("brake") || m.contains("steer") || m.contains("engine")).collect()
}

#[test]
fn check_is_quiet_about_a_good_car_and_names_what_is_wrong_with_a_bad_one() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[100,1,100]}}}"#);
    car_with(&mut e, 2, WHEELS, "", "");
    assert_eq!(about_cars(&mut e), Vec::<String>::new(), "a good car");

    // a table that does not parse, with its line
    call(&mut e, "set", r#"{"uid":2,"component":"Vehicle","value":{"wheels":"fl: -0.8 -0.15 -1.3\nfr: banana"}}"#);
    let m = about_cars(&mut e);
    assert!(m.iter().any(|x| x.starts_with("error:") && x.contains("Vehicle.wheels: line 2") && x.contains("not a number")), "{m:?}");
    // no wheels
    call(&mut e, "set", r#"{"uid":2,"component":"Vehicle","value":{"wheels":""}}"#);
    assert!(about_cars(&mut e).iter().any(|x| x.contains("has no wheels")), "{:?}", about_cars(&mut e));

    // absurd numbers
    let table = |w: &str| format!("fl: -0.8 -0.15 -1.3 steer=1 {w}\\nfr: 0.8 -0.15 -1.3 steer=1 {w}\\nrl: -0.8 -0.15 1.3 drive=1 {w}\\nrr: 0.8 -0.15 1.3 drive=1 {w}");
    let set_wheels = |e: &mut Engine, w: &str| {
        let text = table(w);
        e.call("set", &j(&format!(r#"{{"uid":2,"component":"Vehicle","value":{{"wheels":"{text}"}}}}"#))).unwrap();
    };
    set_wheels(&mut e, "radius=9");
    assert!(about_cars(&mut e).iter().any(|x| x.contains("radius 9 m is not a wheel")), "{:?}", about_cars(&mut e));
    set_wheels(&mut e, "grip=40");
    assert!(about_cars(&mut e).iter().any(|x| x.contains("grip 40 is not a tyre")));
    set_wheels(&mut e, "travel=0.9 rest=0.3");
    assert!(about_cars(&mut e).iter().any(|x| x.contains("travel 0.9 m must be above 0 and at most `rest`")));
    // springs that are too stiff to be stable at 60 ticks a second, and ones too soft for the weight
    set_wheels(&mut e, "spring=3000000");
    assert!(about_cars(&mut e).iter().any(|x| x.contains("too stiff") && x.contains("300 kg a wheel")), "{:?}", about_cars(&mut e));
    set_wheels(&mut e, "spring=8000");
    assert!(about_cars(&mut e).iter().any(|x| x.contains("too soft for the car") && x.contains("bump stops")), "{:?}", about_cars(&mut e));
    set_wheels(&mut e, "spring=35000 damper=900000");
    assert!(about_cars(&mut e).iter().any(|x| x.contains("damper") && x.contains("too strong")), "{:?}", about_cars(&mut e));
    // `auto` is always in range
    set_wheels(&mut e, "spring=auto damper=auto");
    assert_eq!(about_cars(&mut e), Vec::<String>::new(), "auto springs are tuned to the weight");
}

#[test]
fn check_says_what_the_chassis_lacks_and_which_controls_do_nothing() {
    let mut e = engine();
    // a Vehicle alone
    call(&mut e, "spawn", &format!(r#"{{"uid":3,"components":{{"Transform":{{}},"Vehicle":{{"wheels":"{WHEELS}"}}}}}}"#));
    let m = about_cars(&mut e);
    assert!(m.iter().any(|x| x.contains("no RigidBody")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no Collider")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no Velocity")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no AngularVelocity")), "{m:?}");
    // a rigid body with no mass
    call(&mut e, "add_component", r#"{"uid":3,"component":"RigidBody","value":{"mass":0}}"#);
    assert!(about_cars(&mut e).iter().any(|x| x.contains("no mass")), "{:?}", about_cars(&mut e));
    // controls that do nothing: nothing drives, nothing steers, nothing brakes
    let mut e = engine();
    car_with(&mut e, 2, "fl: -0.8 -0.15 -1.3 brake=0\\nfr: 0.8 -0.15 -1.3 brake=0\\nrl: -0.8 -0.15 1.3 brake=0\\nrr: 0.8 -0.15 1.3 brake=0", "", "");
    let m = about_cars(&mut e);
    assert!(m.iter().any(|x| x.contains("no wheel has `drive`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no wheel has `steer`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no wheel has `brake`")), "{m:?}");
    // a wheel model that is not there, one that has another parent, one that is a child but cannot be placed
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":9,"components":{"Transform":{}}}"#);
    call(&mut e, "spawn", r#"{"uid":10,"components":{"Transform":{},"Parent":{"target":9},"LocalTransform":{}}}"#);
    call(&mut e, "spawn", r#"{"uid":11,"components":{"Transform":{},"Parent":{"target":2}}}"#);
    car_with(&mut e, 2, "fl: -0.8 -0.15 -1.3 steer=1 drive=1 entity=40\\nfr: 0.8 -0.15 -1.3 steer=1 drive=1 entity=10\\nrl: -0.8 -0.15 1.3 drive=1 entity=11\\nrr: 0.8 -0.15 1.3 drive=1", "", "");
    let m = about_cars(&mut e);
    assert!(m.iter().any(|x| x.contains("entity 40 does not exist")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("entity 10 has a Parent that is not this vehicle")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("entity 11 is a child of the vehicle but has no LocalTransform")), "{m:?}");
    // a nonsense number in the engine
    let mut e = engine();
    car_with(&mut e, 2, WHEELS, r#","engine_force":0,"top_speed":0,"steer_max":120"#, "");
    let m = about_cars(&mut e);
    assert!(m.iter().any(|x| x.contains("engine_force is 0")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("top_speed is 0")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("steer_max 120")), "{m:?}");
}

#[test]
fn the_controls_are_plain_fields_and_the_outputs_are_read_back() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}}"#);
    car_with(&mut e, 2, WHEELS, "", "");
    call(&mut e, "step", r#"{"ticks":120}"#);
    let car = |e: &mut Engine| call(e, "get", r#"{"uid":2,"components":["Vehicle"]}"#).get("components").unwrap().get("Vehicle").unwrap().clone();
    let num = |v: &Json, k: &str| v.get(k).unwrap().as_f64().unwrap();
    assert_eq!(num(&car(&mut e), "on_ground"), 4.0);
    assert_eq!(num(&car(&mut e), "contact"), 15.0);
    call(&mut e, "set", r#"{"uid":2,"component":"Vehicle","value":{"throttle":1}}"#);
    call(&mut e, "step", r#"{"ticks":180}"#);
    let v = car(&mut e);
    assert!(num(&v, "speed") > 10.0, "{}", num(&v, "speed"));
    let comp = v.get("compression").unwrap().as_array().unwrap();
    assert_eq!(comp.len(), 8, "the output arrays have room for eight wheels");
    assert!(comp[0].as_f64().unwrap() > 0.0 && comp[4].as_f64().unwrap() == 0.0, "four wheels, the rest zero");
    // the schema documents it
    let s = call(&mut e, "schema", r#"{"component":"Vehicle"}"#);
    assert!(s.to_string_lossy().contains("raycast vehicle"), "schema says what it is");
}

#[test]
fn the_wheels_overlay_draws_the_rays_struts_and_wheels() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{"pos":[5,2,6]},"Camera":{"clear":[0,0,0,1]}}}"#);
    call(&mut e, "spawn", r#"{"uid":20,"components":{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[100,1,100]}}}"#);
    car_with(&mut e, 2, WHEELS, "", "");
    call(&mut e, "step", r#"{"ticks":120}"#);
    let count = |e: &mut Engine, debug: &str, color: [u8; 3]| -> usize {
        let path = format!("dbg/{}.png", debug.replace(['"', '[', ']', ','], ""));
        call(e, "render", &format!(r#"{{"width":320,"height":200,"quality":"reference","image":false,"path":"{path}","eye":[5,2,6],"target":[0,0.4,0]{}}}"#, if debug.is_empty() { String::new() } else { format!(r#","debug":{debug}"#) }));
        let bytes = std::fs::read(e.root.join(&path)).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0u8; dec.output_buffer_size().unwrap()];
        let info = dec.next_frame(&mut buf).unwrap();
        let ch = info.color_type.samples();
        buf.chunks(ch).filter(|p| p[0] == color[0] && p[1] == color[1] && p[2] == color[2]).count()
    };
    assert_eq!(count(&mut e, "", [60, 220, 90]), 0, "nothing without the overlay");
    let on_the_ground = count(&mut e, r#"["wheels"]"#, [60, 220, 90]);
    assert!(on_the_ground > 40, "four wheels on the ground, in green: {on_the_ground}");
    assert!(count(&mut e, r#"["wheels"]"#, [240, 210, 40]) > 10, "the struts, in yellow");
    assert!(count(&mut e, r#"["wheels"]"#, [110, 110, 130]) > 5, "the rays, dark");
    assert_eq!(count(&mut e, r#"["wheels"]"#, [230, 70, 60]), 0, "no wheel in the air");
    // lift the car into the air: the wheels turn red
    call(&mut e, "set", r#"{"uid":2,"component":"Transform","value":{"pos":[0,5,0]}}"#);
    call(&mut e, "set", r#"{"uid":2,"component":"Velocity","value":{"lin":[0,0,0]}}"#);
    call(&mut e, "set", r#"{"uid":2,"component":"RigidBody","value":{"gravity_scale":0}}"#);
    call(&mut e, "step", r#"{"ticks":2}"#);
    let path = "dbg/air.png";
    call(&mut e, "render", &format!(r#"{{"width":320,"height":200,"quality":"reference","image":false,"path":"{path}","eye":[5,6,8],"target":[0,5,0],"debug":["wheels"]}}"#));
    let bytes = std::fs::read(e.root.join(path)).unwrap();
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0u8; dec.output_buffer_size().unwrap()];
    let info = dec.next_frame(&mut buf).unwrap();
    let ch = info.color_type.samples();
    let red = buf.chunks(ch).filter(|p| p[0] == 230 && p[1] == 70 && p[2] == 60).count();
    assert!(red > 40, "wheels in the air are red: {red}");
}
