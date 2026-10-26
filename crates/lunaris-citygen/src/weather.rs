//! The weather of the city, as data of a scene like the rest of the game: clear, cloudy, rain and fog, each moving into the next over about fifteen seconds. Nothing native: a graph system on the `TimeOfDay` entity picks the
//! state (every 5 minutes of play a new one is drawn; F6 steps through them by hand), eases the amounts `wx` (how overcast), `rain` and `fogw` towards it (in about ten seconds), dims the sun with the cloud, and, in a storm, throws
//! lightning (a flash of ambient light and, a second or three later, thunder). Another system dulls the sky and the haze, sets the fog's thickness and the `Weather.rain` and `Weather.wet` that the GPU renderer draws as streaks over the picture and as a wet, glossy ground (
//! puddles; the rain is a layer in the final pass, a fraction of a millisecond where 7000 particle drops cost 6 ms of CPU); an entity that follows the player's head looks straight up for a roof (`covered`), and under one the streaks are put out and the
//! sound of the rain is muffled; the rain and the wind are looping sounds of the workspace (`sounds/`, made by `tools/citygen/make_weather_sounds.py`).
use lunaris_json::Json;

pub struct Weather {
    pub components: Vec<Json>,
    pub entities: Vec<Json>,
    pub systems: Vec<Json>,
    pub resources: Vec<(String, Json)>,
    /// The bindings to add to the player's `InputMap` (one line each).
    pub bindings: &'static str,
}

/// How long a weather lasts before the next is drawn, in ticks (the walker runs 60 a second: five minutes).
const PERIOD: f64 = 18000.0;

fn n(x: f64) -> Json {
    Json::Float(x)
}

fn arr(a: &[f64]) -> Json {
    Json::arr(a.iter().map(|x| Json::Float(*x)))
}

pub fn build() -> Weather {
    let get = |k: &str| Json::obj([("get", Json::str(k))]);
    let res = |k: &str| Json::obj([("res", Json::str(k))]);
    let var = |k: &str| Json::obj([("var", Json::str(k))]);
    let op2 = |op: &str, a: Json, b: Json| Json::obj([(op, Json::arr([a, b]))]);
    let sel = |c: Json, a: Json, b: Json| Json::obj([("select", Json::arr([c, a, b]))]);
    let tick = Json::obj([("tick", Json::Bool(true))]);
    let dt = Json::obj([("dt", Json::Bool(true))]);
    let field = |name: &str, default: f64| Json::obj([("name", Json::str(name)), ("type", Json::str("f32")), ("default", n(default))]);
    let set = |path: &str, to: Json| Json::obj([("set", Json::str(path)), ("to", to)]);
    let res_set = |name: &str, to: Json| Json::obj([("res_set", Json::str(name)), ("to", to)]);
    let res_add = |name: &str, by: Json| Json::obj([("res_add", Json::str(name)), ("by", by)]);
    // a value per state (0 clear, 1 cloudy, 2 rain, 3 fog), by the target state in `wx_target`
    let by_state = |v: [f64; 4]| sel(op2("eq", res("wx_target"), n(0.0)), n(v[0]), sel(op2("eq", res("wx_target"), n(1.0)), n(v[1]), sel(op2("eq", res("wx_target"), n(2.0)), n(v[2]), n(v[3]))));
    let within = |since: Json| op2("and", op2("ge", since.clone(), n(0.0)), op2("lt", since, n(180.0)));

    let components = vec![
        Json::obj([("name", Json::str("WeatherSound")), ("doc", Json::str("A loop of the weather and the hours: kind 1 rain, 2 wind, 3 the day's birds, 4 the night's crickets; its volume follows the weather and the light and is muffled under a roof.")), ("fields", Json::arr([field("kind", 1.0)]))]),
        Json::obj([("name", Json::str("WeatherLabel")), ("doc", Json::str("A line of the HUD that names the weather for a moment after F6: state 0 clear, 1 cloudy, 2 rain, 3 fog.")), ("fields", Json::arr([field("state", 0.0)]))]),
    ];

    let mut entities: Vec<Json> = Vec::new();
    // the roof finder: an entity that follows the player's head and looks straight up (13 m) for something solid; the rain itself is the picture's (`Environment.rain`)
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Rain roof probe"))])),
        ("Transform", Json::obj([("pos", arr(&[0.0, 14.0, 0.0]))])),
        ("RainCloud", Json::Obj(Vec::new())),
        ("Probe", Json::obj([("up", n(0.5)), ("depth", n(1.0)), ("ray_from", arr(&[0.0, -12.5, 0.0])), ("ray_dir", arr(&[0.0, 1.0, 0.0])), ("ray_max", n(13.0))])),
    ]));
    // the picture's weather: the rain streaks and the wet ground (`Weather`, which the GPU renderer reads)
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Weather"))])),
        ("Weather", Json::obj([("rain", n(0.0)), ("wet", n(0.0))])),
    ]));
    // the loops (flat, not placed in the world) and the thunder
    for (kind, file) in [(1.0, "sounds/rain.wav"), (2.0, "sounds/wind.wav"), (3.0, "sounds/amb_day.wav"), (4.0, "sounds/amb_night.wav")] {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Weather sound {kind}")))])),
            ("AudioSource", Json::obj([("file", Json::str(file)), ("volume", n(0.0)), ("looped", Json::Bool(true)), ("playing", Json::Bool(true)), ("spatial", Json::Bool(false))])),
            ("WeatherSound", Json::obj([("kind", n(kind))])),
        ]));
    }
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Sound thunder"))])),
        ("SoundFile", Json::obj([("name", Json::str("thunder")), ("file", Json::str("sounds/thunder.wav")), ("volume", n(1.6)), ("spatial", Json::Bool(false))])),
    ]));
    for (state, name) in ["CLEAR", "CLOUDY", "RAIN", "FOG"].iter().enumerate() {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Weather label {name}")))])),
            ("Text", Json::obj([("value", Json::str(format!("WEATHER: {name}   (F6)"))), ("pos", arr(&[0.38, 0.16])), ("size", n(0.03)), ("color", arr(&[0.85, 0.92, 1.0, 0.0]))])),
            ("WeatherLabel", Json::obj([("state", n(state as f64))])),
        ]));
    }

    let mut systems: Vec<Json> = Vec::new();
    // the weather itself (on the entity of the day: it exists exactly once)
    systems.push(Json::obj([
        ("name", Json::str("weather")),
        ("doc", Json::str("Draws a new weather every five minutes (F6 steps by hand), eases how overcast, rainy and foggy it is towards it, dims the sun with the cloud and throws lightning and thunder in a storm.")),
        ("each", Json::arr([Json::str("TimeOfDay")])),
        ("order", Json::from(4)),
        ("do", Json::arr([
            // a new draw at every period (never at the very start: a game begins clear)
            Json::obj([("if", op2("and", op2("lt", op2("mod", tick.clone(), n(PERIOD)), n(0.5)), op2("gt", tick.clone(), n(100.0)))), ("then", Json::arr([
                Json::obj([("let", Json::str("u")), ("be", Json::obj([("rand", n(5.0))]))]),
                res_set("wx_target", sel(op2("lt", var("u"), n(0.35)), n(0.0), sel(op2("lt", var("u"), n(0.6)), n(1.0), sel(op2("lt", var("u"), n(0.85)), n(2.0), n(3.0))))),
            ]))]),
            Json::obj([("if", op2("gt", res("action.weather.pressed"), n(0.5))), ("then", Json::arr([
                res_set("wx_target", op2("mod", op2("add", res("wx_target"), n(1.0)), n(4.0))),
                res_set("wlabel_tick", tick.clone()),
            ]))]),
            // the amounts ease towards the state's (about 16 s to most of the way)
            res_add("wx", op2("mul", op2("mul", op2("sub", by_state([0.12, 0.62, 0.95, 0.55]), res("wx")), dt.clone()), n(0.12))),
            res_add("rain", op2("mul", op2("mul", op2("sub", by_state([0.0, 0.0, 1.0, 0.0]), res("rain")), dt.clone()), n(0.14))),
            res_add("fogw", op2("mul", op2("mul", op2("sub", by_state([0.0, 0.0, 0.2, 1.0]), res("fogw")), dt.clone()), n(0.08))),
            // the ground wets in a rain in about ten seconds and dries over a couple of minutes
            res_add("wetness", op2("mul", dt.clone(), sel(op2("gt", res("rain"), n(0.3)), op2("mul", op2("sub", n(1.0), res("wetness")), n(0.1)), op2("mul", res("wetness"), n(-0.012))))),
            // the sun is dimmed by the cloud (it is 3 on a clear day)
            set("TimeOfDay.sun_intensity", op2("mul", n(3.0), op2("sub", n(1.0), op2("mul", res("wx"), n(0.8))))),
            // lightning in a storm: a flash that dies away, thunder some time after it (the light is quicker than the sound)
            res_set("flash", op2("mul", res("flash"), n(0.86))),
            Json::obj([("if", op2("and", op2("gt", res("rain"), n(0.7)), op2("lt", Json::obj([("rand", n(3.0))]), n(0.0007)))), ("then", Json::arr([
                res_set("flash", n(1.0)),
                res_set("thunder_t", op2("add", tick.clone(), op2("add", n(50.0), Json::obj([("floor", op2("mul", Json::obj([("rand", n(9.0))]), n(200.0)))])))),
            ]))]),
            Json::obj([("if", op2("and", op2("gt", res("thunder_t"), n(0.5)), op2("ge", tick.clone(), res("thunder_t")))), ("then", Json::arr([
                Json::obj([("emit", Json::str("thunder"))]),
                res_set("thunder_t", n(0.0)),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // the sky: dulled and darker under cloud, the haze thicker in fog and rain, a flash of lightning on the ambient light (the engine writes the hour's colours each tick before the graph systems run)
    let mut sky: Vec<Json> = vec![
        Json::obj([("let", Json::str("dull")), ("be", Json::obj([("clamp", Json::arr([op2("add", op2("mul", res("wx"), n(0.8)), op2("mul", res("fogw"), n(0.5))), n(0.0), n(1.0)]))]))]),
        Json::obj([("let", Json::str("mt")), ("be", op2("div", op2("add", op2("add", get("Environment.sky_top[0]"), get("Environment.sky_top[1]")), get("Environment.sky_top[2]")), n(3.0)))]),
        Json::obj([("let", Json::str("mh")), ("be", op2("div", op2("add", op2("add", get("Environment.sky_horizon[0]"), get("Environment.sky_horizon[1]")), get("Environment.sky_horizon[2]")), n(3.0)))]),
        Json::obj([("let", Json::str("mf")), ("be", op2("div", op2("add", op2("add", get("Environment.fog_color[0]"), get("Environment.fog_color[1]")), get("Environment.fog_color[2]")), n(3.0)))]),
    ];
    for k in 0..3 {
        for (field, mean, dim, strength) in [("sky_top", "mt", 0.30, 0.85), ("sky_horizon", "mh", 0.22, 0.85), ("fog_color", "mf", 0.12, 0.9)] {
            let path = format!("Environment.{field}[{k}]");
            let c = get(&path);
            let greyed = op2("add", c.clone(), op2("mul", op2("sub", var(mean), c), op2("mul", var("dull"), n(strength))));
            sky.push(set(&path, op2("mul", greyed, op2("sub", n(1.0), op2("mul", res("wx"), n(dim))))));
        }
    }
    sky.push(set("Environment.fog", op2("add", op2("add", n(0.0006), op2("mul", res("fogw"), n(0.0048))), op2("mul", res("rain"), n(0.0009)))));
    sky.push(set("Environment.clouds", op2("add", n(0.3), op2("mul", res("wx"), n(0.7)))));
    sky.push(set("Environment.ambient", op2("add", op2("mul", get("Environment.ambient"), op2("sub", n(1.0), op2("mul", res("wx"), n(0.12)))), op2("mul", res("flash"), n(3.0)))));
    systems.push(Json::obj([
        ("name", Json::str("weather_sky")),
        ("doc", Json::str("The sky, the haze and the fog follow the weather: duller and darker under cloud, thick in fog, lit by a flash of lightning.")),
        ("each", Json::arr([Json::str("Environment")])),
        ("order", Json::from(8)),
        ("do", Json::Arr(sky)),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("weather_picture")),
        ("doc", Json::str("The picture's rain is the rain that is seen (none under a roof); the ground is wet after rain, mostly dry indoors.")),
        ("each", Json::arr([Json::str("Weather")])),
        ("order", Json::from(10)),
        ("do", Json::arr([
            set("Weather.rain", res("rain_seen")),
            set("Weather.wet", op2("mul", res("wetness"), op2("sub", n(1.0), op2("mul", res("covered"), n(0.85))))),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // the roof finder follows the player's head; the eye ray straight up says whether something solid is overhead, and then the rain is not drawn (`covered` eases 0 to 1 and back in about a third of a second)
    systems.push(Json::obj([
        ("name", Json::str("rain_cloud")),
        ("doc", Json::str("The rain follows the player and is not drawn where there is a roof overhead.")),
        ("each", Json::arr([Json::str("RainCloud"), Json::str("Probe"), Json::str("Transform")])),
        ("order", Json::from(9)),
        ("do", Json::arr([
            set("Transform.pos[0]", res("eye_x")),
            set("Transform.pos[1]", op2("add", res("eye_y"), n(13.0))),
            set("Transform.pos[2]", res("eye_z")),
            res_add("covered", op2("mul", op2("sub", sel(get("Probe.hit"), n(1.0), n(0.0)), res("covered")), op2("mul", dt.clone(), n(3.0)))),
            res_set("rain_seen", op2("mul", res("rain"), op2("sub", n(1.0), res("covered")))),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // the loops: rain by the rain, wind by the storm and the cloud; both muffled under a roof
    systems.push(Json::obj([
        ("name", Json::str("weather_audio")),
        ("doc", Json::str("The rain and the wind are as loud as the weather, and quieter under a roof.")),
        ("each", Json::arr([Json::str("WeatherSound"), Json::str("AudioSource")])),
        ("order", Json::from(10)),
        ("do", Json::arr([
            Json::obj([("let", Json::str("roof")), ("be", op2("sub", n(1.0), op2("mul", res("covered"), n(0.7))))]),
            set("AudioSource.volume", sel(op2("eq", get("WeatherSound.kind"), n(1.0)),
                op2("mul", op2("mul", res("rain"), n(0.7)), var("roof")),
                sel(op2("eq", get("WeatherSound.kind"), n(2.0)),
                    op2("mul", op2("add", n(0.05), op2("add", op2("mul", res("rain"), n(0.3)), op2("add", op2("mul", res("wx"), n(0.12)), op2("mul", res("fogw"), n(0.04))))), var("roof")),
                    // the day's birds and the night's crickets, the one fading into the other with the dusk, both put down by rain and muffled indoors
                    sel(op2("eq", get("WeatherSound.kind"), n(3.0)),
                        op2("mul", op2("mul", op2("mul", op2("sub", n(1.0), res("night")), op2("sub", n(1.0), op2("mul", res("rain"), n(0.6)))), n(0.55)), var("roof")),
                        op2("mul", op2("mul", op2("mul", res("night"), op2("sub", n(1.0), op2("mul", res("rain"), n(0.5)))), n(0.5)), var("roof")))))),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("weather_label")),
        ("doc", Json::str("The name of the weather shows for three seconds after F6.")),
        ("each", Json::arr([Json::str("WeatherLabel"), Json::str("Text")])),
        ("order", Json::from(16)),
        ("do", Json::arr([set("Text.color[3]", sel(op2("and", op2("eq", res("wx_target"), get("WeatherLabel.state")), within(op2("sub", tick.clone(), res("wlabel_tick")))), n(0.95), n(0.0)))])),
        ("enabled", Json::Bool(true)),
    ]));

    let resources = ["wx_target", "wx", "rain", "rain_seen", "wetness", "fogw", "covered", "flash", "thunder_t", "eye_y"].iter().map(|k| (k.to_string(), n(0.0))).chain(std::iter::once(("wlabel_tick".to_string(), n(-10000.0)))).collect();
    Weather { components, entities, systems, resources, bindings: "weather = f6" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weather_fits_together() {
        let w = build();
        assert_eq!(w.components.len(), 2);
        assert_eq!(w.entities.len(), 1 + 1 + 4 + 1 + 4);
        assert_eq!(w.systems.len(), 6);
        assert!(w.resources.iter().any(|(k, _)| k == "wx_target"));
    }
}
