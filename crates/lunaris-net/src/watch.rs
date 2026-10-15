//! A spectator: a client without a seat that is sent the whole game from tick 0 (the input log), plays it as fast as it can to catch up, and
//! then follows the game live. It never sends input, so it cannot change anything; it does report its state hash every few ticks, so
//! that the host notices if its copy of the game is not the same.
use crate::client::Welcome;
use crate::log::Replayer;
use crate::wire::{bye, reason_of, send};
use lunaris_api::Engine;
use lunaris_json::Json;
use std::net::TcpStream;
use std::sync::mpsc::{self, TryRecvError};

pub struct Watcher {
    pub welcome: Welcome,
    stream: TcpStream,
    rx: mpsc::Receiver<Json>,
    replay: Replayer,
    /// Send the state hash of every tick that is a multiple of this (0: never).
    pub hash_every: u64,
    /// Set when the host reports that two copies of the game disagree.
    pub desync: Option<String>,
    /// Set when the game is over and this copy has played all of it.
    pub finished: Option<String>,
    end_tick: Option<u64>,
    end_reason: Option<String>,
    events: Vec<String>,
}

impl Watcher {
    pub(crate) fn new(welcome: Welcome, stream: TcpStream, rx: mpsc::Receiver<Json>, hash_every: u64) -> Watcher {
        Watcher { welcome, stream, rx, replay: Replayer::new(), hash_every, desync: None, finished: None, end_tick: None, end_reason: None, events: Vec::new() }
    }

    /// Take in what has arrived and play up to `max_steps` ticks of it. Returns how many were played; 0 means there is nothing new yet (or
    /// the game is over). To catch up at full speed pass a big number; to follow the game at its own pace pass what the clock allows.
    pub fn advance(&mut self, engine: &mut Engine, max_steps: usize) -> Result<usize, String> {
        if self.finished.is_some() {
            return Ok(0);
        }
        loop {
            match self.rx.try_recv() {
                Ok(m) => {
                    if m.get("log").is_some() {
                        self.replay.add(&m);
                    } else if let Some(d) = m.get("desync") {
                        self.desync = Some(d.to_string_lossy());
                    } else if let Some(s) = m.get("seat") {
                        let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let state = s.get("state").and_then(|v| v.as_str()).unwrap_or("?");
                        self.events.push(format!("seat {} ({name}): {state}", s.get("player").and_then(|v| v.as_u64()).unwrap_or(0) + 1));
                    } else if m.get("bye").is_some() {
                        self.end_reason = Some(reason_of(&m));
                        let tick = m.get("bye").and_then(|b| b.get("tick")).and_then(|t| t.as_u64());
                        self.end_tick = Some(tick.unwrap_or(self.replay.upto));
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.end_reason.get_or_insert_with(|| "the connection to the host ended".to_string());
                    self.end_tick.get_or_insert(self.replay.upto);
                    break;
                }
            }
        }
        let (done, hashes) = self.replay.play(engine, max_steps, self.end_tick, self.hash_every)?;
        for (tick, hash) in hashes {
            let _ = send(&mut self.stream, &Json::obj([("hash", Json::obj([("tick", Json::from(tick as i64)), ("hash", Json::str(hash))]))]));
        }
        if let Some(end) = self.end_tick {
            if engine.sim.tick >= end && self.replay.upto >= end {
                self.finished = Some(self.end_reason.clone().unwrap_or_else(|| "the game is over".to_string()));
            }
        }
        Ok(done)
    }

    /// How many ticks the host has told us about that this copy has not played yet.
    pub fn lag(&self, engine: &Engine) -> u64 {
        self.replay.upto.saturating_sub(engine.sim.tick)
    }

    /// News about the seats, oldest first; reading it clears it.
    pub fn take_events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.events)
    }

    /// Stop watching (the game goes on without us).
    pub fn leave(&mut self, reason: &str) {
        let _ = send(&mut self.stream, &bye(reason));
        self.finished.get_or_insert_with(|| reason.to_string());
    }
}

impl Drop for Watcher {
    /// Dropping a watcher hangs up (see `Rollback`'s `Drop`).
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}
