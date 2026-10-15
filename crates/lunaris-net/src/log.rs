//! The input log: every key change of a game, tick by tick, which is everything needed to play the game again from tick 0. The host keeps
//! one (only the changes, so a quiet game costs almost nothing); a late spectator or a player who comes back replays it with a `Replayer`.
use crate::book::is_empty_input;
use crate::key_name;
use crate::wire::hex;
use lunaris_api::Engine;
use lunaris_json::Json;
use std::collections::BTreeMap;

/// One key change: in `tick`, `player`'s keys `values` changed (an object like `{"d": true}`).
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub tick: u64,
    pub player: usize,
    pub values: Json,
}

/// The confirmed inputs of a whole game.
#[derive(Clone, Debug, Default)]
pub struct InputLog {
    /// The key changes, in tick order and, within a tick, in player order. Ticks that are not mentioned changed nothing.
    pub entries: Vec<LogEntry>,
    /// False when the host had to throw the oldest changes away to stay inside its memory limit: the game can no longer be replayed from
    /// tick 0 (see `HostConfig::max_log_bytes`).
    pub complete: bool,
}

/// Apply one tick's inputs to `engine` the way every copy of the game does: each player's changed keys become `input.<key>` resources
/// (`input.p2.<key>` for the second player and so on), in player order. The caller steps the simulation afterwards.
pub fn apply_tick_inputs<'a>(engine: &mut Engine, inputs: impl IntoIterator<Item = (usize, &'a Json)>) -> Result<(), String> {
    for (player, values) in inputs {
        let Some(obj) = values.as_object() else { continue };
        if obj.is_empty() {
            continue;
        }
        let mapped: Vec<(String, Json)> = obj.iter().map(|(k, v)| (key_name(player, k), v.clone())).collect();
        engine.call("input", &Json::obj([("values", Json::Obj(mapped))])).map_err(|e| e.to_string())?;
    }
    Ok(())
}

impl InputLog {
    /// Play the game again on `engine` (a freshly loaded copy of the scene, at tick 0, with the game's seed) up to tick `ticks`, applying
    /// the logged inputs at the ticks they belong to. The result has the state hash every player's copy had at that tick. This is the
    /// reference the tests compare rollback games against.
    pub fn replay(&self, engine: &mut Engine, ticks: u64) -> Result<(), String> {
        let mut next = 0usize;
        while engine.sim.tick < ticks {
            let tick = engine.sim.tick;
            let mut now: Vec<(usize, &Json)> = Vec::new();
            while next < self.entries.len() && self.entries[next].tick <= tick {
                if self.entries[next].tick == tick {
                    now.push((self.entries[next].player, &self.entries[next].values));
                }
                next += 1;
            }
            apply_tick_inputs(engine, now)?;
            engine.sim.step();
        }
        Ok(())
    }
}

/// The receiving end of a log feed (a spectator, or a player who is coming back): collects `{"log": {...}}` chunks and plays the ticks
/// that are complete.
pub struct Replayer {
    /// Key changes that have arrived and not been played, by tick.
    pending: BTreeMap<u64, Vec<(usize, Json)>>,
    /// Every tick below this is complete: the changes of those ticks are all in `pending` or already played.
    pub upto: u64,
    /// How many chunks have been taken in (in order).
    pub chunks: u64,
    /// The number the next chunk must have; chunks that arrive early wait in `held` (the network may deliver in another order than the host
    /// sent: an `upto` that was believed before the chunk with the earlier changes had arrived would play ticks with changes missing).
    next_chunk: u64,
    held: BTreeMap<u64, Json>,
}

impl Replayer {
    pub fn new() -> Replayer {
        Replayer { pending: BTreeMap::new(), upto: 0, chunks: 0, next_chunk: 0, held: BTreeMap::new() }
    }

    /// Take in one `{"log": {"n": 0, "upto": T, "inputs": [[tick, player, values], ...]}}` message. Chunks are put in the order of their
    /// numbers; a message without a number is taken in at once.
    pub fn add(&mut self, msg: &Json) {
        let Some(log) = msg.get("log") else { return };
        match log.get("n").and_then(|v| v.as_u64()) {
            None => self.take_in(log),
            Some(n) if n < self.next_chunk => {} // a copy of a chunk we have
            Some(n) => {
                self.held.insert(n, log.clone());
                while let Some(next) = self.held.remove(&self.next_chunk) {
                    self.take_in(&next);
                    self.next_chunk += 1;
                }
            }
        }
    }

    fn take_in(&mut self, log: &Json) {
        if let Some(list) = log.get("inputs").and_then(|v| v.as_array()) {
            for e in list {
                let (Some(t), Some(p), Some(v)) = (e.index(0).and_then(|v| v.as_u64()), e.index(1).and_then(|v| v.as_u64()), e.index(2)) else { continue };
                if !is_empty_input(v) {
                    self.pending.entry(t).or_default().push((p as usize, v.clone()));
                }
            }
        }
        if let Some(u) = log.get("upto").and_then(|v| v.as_u64()) {
            self.upto = self.upto.max(u);
        }
        self.chunks += 1;
    }

    /// Play complete ticks (below `upto` and below `limit`), at most `max_steps` of them. Returns the state hash after every tick whose number
    /// is a multiple of `hash_every` (0: none), as `(tick, hash)`: what a spectator reports so that the host can check its copy.
    pub fn play(&mut self, engine: &mut Engine, max_steps: usize, limit: Option<u64>, hash_every: u64) -> Result<(usize, Vec<(u64, String)>), String> {
        let stop = limit.map_or(self.upto, |l| l.min(self.upto));
        let mut done = 0;
        let mut hashes = Vec::new();
        while done < max_steps && engine.sim.tick < stop {
            let tick = engine.sim.tick;
            if let Some(mut list) = self.pending.remove(&tick) {
                // the same order every copy uses: by player
                list.sort_by_key(|(p, _)| *p);
                apply_tick_inputs(engine, list.iter().map(|(p, v)| (*p, v)))?;
            }
            engine.sim.step();
            done += 1;
            if hash_every > 0 && tick % hash_every == 0 {
                hashes.push((tick, hex(engine.sim.state_hash())));
            }
        }
        // changes of ticks we have already passed can only be a protocol error; do not let them pile up
        let passed = engine.sim.tick;
        self.pending = self.pending.split_off(&passed);
        Ok((done, hashes))
    }
}

impl Default for Replayer {
    fn default() -> Self {
        Replayer::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::parse;

    fn chunk(n: u64, upto: u64, entries: &str) -> Json {
        parse(&format!(r#"{{"log": {{"n": {n}, "upto": {upto}, "inputs": [{entries}]}}}}"#)).expect("json")
    }

    const SCENE: &str = r#"{"lunaris": 1, "components": [{"name": "P1", "fields": []}, {"name": "P2", "fields": []}],
      "entities": [{"uid": 1, "components": {"Transform": {"pos": [0, 0, 0]}, "Velocity": {"lin": [0, 0, 0]}, "P1": {}}},
                   {"uid": 2, "components": {"Transform": {"pos": [0, 0, 5]}, "Velocity": {"lin": [0, 0, 0]}, "P2": {}}}],
      "systems": [{"name": "m1", "each": ["P1", "Velocity"], "do": [{"set": "Velocity.lin[0]", "to": {"mul": [6, {"sub": [{"res": "input.d"}, {"res": "input.a"}]}]}}]},
                  {"name": "m2", "each": ["P2", "Velocity"], "do": [{"set": "Velocity.lin[0]", "to": {"mul": [6, {"sub": [{"res": "input.p2.d"}, {"res": "input.p2.a"}]}]}}]}]}"#;

    fn engine() -> Engine {
        let mut e = Engine::new(1, ".");
        e.call("load_scene", &Json::obj([("scene", parse(SCENE).expect("scene"))])).expect("load");
        e
    }

    #[test]
    fn chunks_are_taken_in_the_order_of_their_numbers_whatever_order_they_arrive_in() {
        let mut r = Replayer::new();
        r.add(&chunk(1, 20, r#"[15, 0, {"d": true}]"#));
        assert_eq!((r.upto, r.chunks), (0, 0), "chunk 1 may not be believed before chunk 0, which has changes of earlier ticks, has arrived");
        r.add(&chunk(0, 10, r#"[3, 1, {"a": true}]"#));
        assert_eq!((r.upto, r.chunks), (20, 2));
        r.add(&chunk(0, 10, ""));
        assert_eq!(r.chunks, 2, "a copy of a chunk is ignored");
    }

    #[test]
    fn the_log_and_a_replayer_fed_chunk_by_chunk_play_the_same_game() {
        let log = InputLog {
            entries: vec![LogEntry { tick: 2, player: 0, values: parse(r#"{"d": true}"#).expect("json") }, LogEntry { tick: 2, player: 1, values: parse(r#"{"a": true}"#).expect("json") }, LogEntry { tick: 7, player: 0, values: parse(r#"{"d": false}"#).expect("json") }],
            complete: true,
        };
        let mut a = engine();
        log.replay(&mut a, 12).expect("replay");
        // the same changes as chunks, split in the middle of tick 2 and delivered in the wrong order
        let mut r = Replayer::new();
        let mut b = engine();
        r.add(&chunk(1, 7, r#"[2, 1, {"a": true}]"#));
        r.add(&chunk(2, 12, r#"[7, 0, {"d": false}]"#));
        let (done, _) = r.play(&mut b, 100, None, 0).expect("play");
        assert_eq!(done, 0, "nothing may be played before the first chunk is here");
        r.add(&chunk(0, 2, r#"[2, 0, {"d": true}]"#));
        let (done, hashes) = r.play(&mut b, 100, None, 5).expect("play");
        assert_eq!(done, 12);
        assert_eq!(b.sim.state_hash(), a.sim.state_hash());
        assert_eq!(hashes.iter().map(|(t, _)| *t).collect::<Vec<_>>(), vec![0, 5, 10], "a hash after every tick that is a multiple of 5");
    }
}
