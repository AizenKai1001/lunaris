//! Mixer buses: groups of sounds with a common volume, and ducking (one group turning another down while it is heard), written as text in an `AudioMixer`
//! component and used by the audio director. See `docs/AUDIO.md`.
//!
//! ```text
//! bus master volume=1
//! bus music parent=master volume=0.7
//! bus sfx parent=master
//! bus voice parent=master duck=music:0.6
//! route music/* -> music
//! route voice/* -> voice
//! route * -> sfx
//! ```
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub parent: Option<usize>,
    pub volume: f32,
    /// While a sound on this bus is heard, these buses (by index) are turned down by that share.
    pub ducks: Vec<(usize, f32)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BusSet {
    pub buses: Vec<Bus>,
    routes: Vec<(String, usize)>,
}

fn glob(pattern: &str, name: &str) -> bool {
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        _ if pattern == "*" => true,
        (Some(rest), Some(_)) if pattern.len() > 1 => name.contains(rest.trim_end_matches('*')),
        (Some(rest), None) => name.ends_with(rest),
        (None, Some(rest)) => name.starts_with(rest),
        _ => name == pattern,
    }
}

impl BusSet {
    pub fn parse(text: &str) -> Result<BusSet, String> {
        let mut set = BusSet::default();
        struct Pending {
            parent: Option<String>,
            ducks: Vec<(String, f32)>,
        }
        let mut pending: Vec<Pending> = Vec::new();
        let mut routes: Vec<(String, String)> = Vec::new();
        for (n, line) in text.split(['\n', ';']).enumerate() {
            let n = n + 1;
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("bus ") {
                let mut words = rest.split_whitespace();
                let name = words.next().ok_or_else(|| format!("line {n}: `bus` needs a name"))?;
                let mut bus = Bus { name: name.to_string(), parent: None, volume: 1.0, ducks: Vec::new() };
                let mut p = Pending { parent: None, ducks: Vec::new() };
                for w in words {
                    match w.split_once('=') {
                        Some(("parent", v)) => p.parent = Some(v.to_string()),
                        Some(("volume", v)) => bus.volume = v.parse::<f32>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("line {n}: volume must be a number from 0 up, not `{v}`"))?,
                        Some(("duck", v)) => {
                            let (target, amount) = v.split_once(':').ok_or_else(|| format!("line {n}: duck=BUS:AMOUNT, for example duck=music:0.6"))?;
                            let a = amount.parse::<f32>().ok().filter(|x| (0.0..=1.0).contains(x)).ok_or_else(|| format!("line {n}: the duck amount must be 0 to 1, not `{amount}`"))?;
                            p.ducks.push((target.to_string(), a));
                        }
                        _ => return Err(format!("line {n}: unknown bus option `{w}` (parent=NAME, volume=N, duck=BUS:AMOUNT)")),
                    }
                }
                if set.buses.iter().any(|b| b.name == bus.name) {
                    return Err(format!("line {n}: bus `{}` is declared twice", bus.name));
                }
                set.buses.push(bus);
                pending.push(p);
            } else if let Some(rest) = line.strip_prefix("route ") {
                let (pat, bus) = rest.split_once("->").ok_or_else(|| format!("line {n}: route PATTERN -> BUS"))?;
                routes.push((pat.trim().to_string(), bus.trim().to_string()));
            } else {
                return Err(format!("line {n}: `{line}` is not a bus or a route"));
            }
        }
        let index = |set: &BusSet, name: &str| set.buses.iter().position(|b| b.name == name).ok_or_else(|| format!("the bus `{name}` is not declared"));
        for (i, p) in pending.iter().enumerate() {
            if let Some(parent) = &p.parent {
                let pi = index(&set, parent)?;
                if pi == i {
                    return Err(format!("bus `{}` is its own parent", set.buses[i].name));
                }
                set.buses[i].parent = Some(pi);
            }
            for (t, a) in &p.ducks {
                let ti = index(&set, t)?;
                set.buses[i].ducks.push((ti, *a));
            }
        }
        // a parent chain that loops would never end
        for i in 0..set.buses.len() {
            let (mut at, mut steps) = (i, 0);
            while let Some(p) = set.buses[at].parent {
                at = p;
                steps += 1;
                if steps > set.buses.len() {
                    return Err(format!("the parents of bus `{}` go round in a circle", set.buses[i].name));
                }
            }
        }
        for (pat, bus) in routes {
            let bi = index(&set, &bus)?;
            set.routes.push((pat, bi));
        }
        Ok(set)
    }

    /// The bus a sound goes to, by its file path or (for built-in voices) `voice:NAME`; the first route that matches wins.
    pub fn route(&self, label: &str) -> Option<usize> {
        self.routes.iter().find(|(p, _)| glob(p, label)).map(|(_, b)| *b)
    }

    /// The volume of a bus with everything above it: its own, the settings resource `bus.NAME.volume` (and `bus.NAME.mute`), and its parents'.
    pub fn gain(&self, bus: usize, resource: &dyn Fn(&str) -> Option<f64>, duck: &BTreeMap<usize, f32>) -> f32 {
        let (mut g, mut at) = (1.0f32, Some(bus));
        while let Some(i) = at {
            let b = &self.buses[i];
            let setting = resource(&format!("bus.{}.volume", b.name)).map_or(1.0, |v| v.clamp(0.0, 4.0) as f32);
            let muted = resource(&format!("bus.{}.mute", b.name)).is_some_and(|v| v > 0.5);
            g *= if muted { 0.0 } else { b.volume * setting };
            g *= 1.0 - duck.get(&i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            at = b.parent;
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "bus master volume=1\nbus music parent=master volume=0.5\nbus sfx parent=master\nbus voice parent=master duck=music:0.6\nroute music/* -> music\nroute voice:* -> voice\nroute *.ogg -> sfx\nroute * -> master";

    #[test]
    fn buses_parse_route_and_multiply_down_the_tree_with_settings_and_ducking() {
        let b = BusSet::parse(TEXT).unwrap();
        assert_eq!(b.buses.len(), 4);
        let name = |i: Option<usize>| i.map(|i| b.buses[i].name.clone());
        assert_eq!(name(b.route("music/theme.ogg")), Some("music".into()));
        assert_eq!(name(b.route("voice:hit")), Some("voice".into()));
        assert_eq!(name(b.route("sounds/boom.ogg")), Some("sfx".into()));
        assert_eq!(name(b.route("anything.wav")), Some("master".into()));
        let none = |_: &str| None;
        let music = b.route("music/theme.ogg").unwrap();
        assert!((b.gain(music, &none, &BTreeMap::new()) - 0.5).abs() < 1e-6);
        // a settings menu: the master slider and a mute
        let settings = |n: &str| match n {
            "bus.master.volume" => Some(0.5),
            "bus.sfx.mute" => Some(1.0),
            _ => None,
        };
        assert!((b.gain(music, &settings, &BTreeMap::new()) - 0.25).abs() < 1e-6);
        assert_eq!(b.gain(b.route("x.ogg").unwrap(), &settings, &BTreeMap::new()), 0.0);
        // ducking: the voice bus turns music down by 60 percent
        let mut duck = BTreeMap::new();
        duck.insert(music, 0.6);
        assert!((b.gain(music, &none, &duck) - 0.2).abs() < 1e-6);
        assert!(b.buses[b.route("voice:x").unwrap()].ducks == vec![(music, 0.6)]);
    }

    #[test]
    fn bus_text_reports_what_is_wrong() {
        for (bad, word) in [("bus a parent=b", "not declared"), ("bus a\nbus a", "twice"), ("bus a parent=a", "own parent"), ("bus a parent=b\nbus b parent=a", "circle"), ("bus a volume=x", "number"), ("bus a duck=b", "duck=BUS:AMOUNT"), ("route x -> nothing", "not declared"), ("play loud", "not a bus"), ("bus a loud=1", "unknown bus option")] {
            let e = BusSet::parse(bad).unwrap_err();
            assert!(e.contains(word), "{bad:?} gave {e:?}");
        }
    }
}
