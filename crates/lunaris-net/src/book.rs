//! The inputs of every player, tick by tick, and what is known about them: the bookkeeping both the host and a rollback client need.
//!
//! An input is "the keys that changed" in one tick (an object; `{}` means nothing changed). Every player sends one for every tick, in
//! order, so for each player there is a point `through` such that **every tick below it is known**. The smallest of those points over all
//! players is the *frontier*: everything below it is settled, nothing there can ever change, and a game state at the frontier is final.
//! Ticks above the frontier may still be partly unknown, and a client that wants to go on simulates them with a *prediction*: a player
//! whose input is not known is assumed to change nothing (keys stay as they were), which is "repeat the last known input state".
//!
//! Input messages may arrive late, twice, or in a different order than they were sent (a real network, or the latency simulator), so the
//! book stores them by tick and works out `through` itself instead of trusting the order.
//!
//! A player who has left can be **frozen**: from a tick `from` on, their input is declared to be empty (the keys they held stay held),
//! which counts as known, so the others are not held up for ever. A frozen seat can be given back at a later tick (`unfreeze`): the ticks in
//! between stay empty, and from then on the player's own inputs are needed again.
use lunaris_json::Json;
use std::collections::BTreeMap;

/// `through` of a player who is frozen with no end: everything from some tick on is known.
pub const OPEN: u64 = u64::MAX;

/// True for the input that changes nothing: `{}` (or no object at all).
pub fn is_empty_input(v: &Json) -> bool {
    match v {
        Json::Obj(o) => o.is_empty(),
        Json::Null => true,
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub struct InputBook {
    players: usize,
    /// Ticks below this have been dropped (`prune`); all of them are known.
    base: u64,
    /// The inputs that arrived, by tick, then by player (`None`: not known yet). Only ticks at or above `base`.
    inputs: BTreeMap<u64, Vec<Option<Json>>>,
    /// Per player: every tick below this is known, by an input or by a frozen stretch that has an end.
    through: Vec<u64>,
    /// Per player: a freeze that has not ended: from this tick on the input is empty.
    open: Vec<Option<u64>>,
    /// Per player: frozen stretches that have ended, `[from, to)`.
    closed: Vec<Vec<(u64, u64)>>,
}

impl InputBook {
    /// A book for `players` players in which every tick below `base` is known (a game that starts at tick 0 passes 0).
    pub fn new(players: usize, base: u64) -> InputBook {
        InputBook { players, base, inputs: BTreeMap::new(), through: vec![base; players], open: vec![None; players], closed: vec![Vec::new(); players] }
    }

    pub fn players(&self) -> usize {
        self.players
    }

    fn frozen_at(&self, player: usize, tick: u64) -> bool {
        self.open[player].is_some_and(|f| tick >= f) || self.closed[player].iter().any(|&(f, to)| tick >= f && tick < to)
    }

    fn real(&self, tick: u64, player: usize) -> Option<&Json> {
        self.inputs.get(&tick).and_then(|v| v[player].as_ref())
    }

    /// Move `through[player]` over every tick that is known by now.
    fn advance(&mut self, player: usize) {
        loop {
            let t = self.through[player];
            if let Some(&(_, to)) = self.closed[player].iter().find(|&&(f, to)| t >= f && t < to) {
                self.through[player] = to;
                continue;
            }
            if self.open[player].is_some_and(|f| t >= f) {
                return; // known from here to the end of time: `known_through` says so
            }
            if self.real(t, player).is_some() {
                self.through[player] = t + 1;
                continue;
            }
            return;
        }
    }

    /// Every tick below this is known for `player` (`OPEN` for a frozen seat with no end).
    pub fn known_through(&self, player: usize) -> u64 {
        if self.open[player].is_some_and(|f| self.through[player] >= f) {
            OPEN
        } else {
            self.through[player]
        }
    }

    /// The real inputs of `player` run through this tick (leaves out what a freeze made known): what a host must have received from a seat
    /// before it can say where the seat's freeze begins.
    pub fn received_through(&self, player: usize) -> u64 {
        self.through[player]
    }

    /// The smallest `known_through` over all players: every tick below it is settled. `OPEN` when every seat is frozen with no end.
    pub fn frontier(&self) -> u64 {
        (0..self.players).map(|p| self.known_through(p)).min().unwrap_or(OPEN)
    }

    /// Record the input of `player` for `tick`. False (and nothing changes) when it is not new: a duplicate, an input from a tick that is
    /// already settled for that player, or one for a stretch the player is frozen in.
    pub fn put(&mut self, tick: u64, player: usize, values: Json) -> bool {
        if player >= self.players || tick < self.through[player] || self.frozen_at(player, tick) {
            return false;
        }
        let slot = self.inputs.entry(tick).or_insert_with(|| vec![None; self.players]);
        if slot[player].is_some() {
            return false;
        }
        slot[player] = Some(values);
        self.advance(player);
        true
    }

    /// Like `put`, for a receiver that was told by the host (and so knows the input is real): the input is kept even if the seat is frozen at
    /// that tick *as far as this receiver has been told so far*. The news that a seat is back (`unfreeze`) can arrive after the first inputs
    /// of the player who is back (the network may deliver messages in another order than they were sent), and an input thrown away for
    /// that reason would be missing for ever. While the seat is frozen the kept input does not count (`value_for` and `known_through` go by
    /// the freeze); once it is given back, it does.
    pub fn put_told(&mut self, tick: u64, player: usize, values: Json) -> bool {
        if player >= self.players || tick < self.through[player] {
            return false;
        }
        let slot = self.inputs.entry(tick).or_insert_with(|| vec![None; self.players]);
        if slot[player].is_some() {
            return false;
        }
        slot[player] = Some(values);
        self.advance(player);
        true
    }

    /// The first tick at or after `from` in which `player` really changed a key (an input that is kept and not covered by a freeze).
    pub fn first_change_from(&self, player: usize, from: u64) -> Option<u64> {
        self.inputs.range(from..).find(|(&t, row)| row[player].as_ref().is_some_and(|v| !is_empty_input(v)) && !self.frozen_at(player, t)).map(|(&t, _)| t)
    }

    /// From `from` on, the input of `player` is empty and known (see the module text).
    pub fn freeze(&mut self, player: usize, from: u64) {
        if player < self.players && self.open[player].is_none() {
            self.open[player] = Some(from.max(self.base));
            self.advance(player);
        }
    }

    /// End a freeze at `to`: the ticks from the freeze to `to` stay empty, and from `to` on the player's own inputs are needed again.
    pub fn unfreeze(&mut self, player: usize, to: u64) {
        if player < self.players {
            if let Some(f) = self.open[player].take() {
                self.closed[player].push((f, to.max(f)));
            }
            self.advance(player);
        }
    }

    pub fn is_frozen(&self, player: usize) -> bool {
        self.open[player].is_some()
    }

    /// Is any seat frozen with no end?
    pub fn any_frozen(&self) -> bool {
        self.open.iter().any(|o| o.is_some())
    }

    /// Where the open freeze of `player` begins, if there is one.
    pub fn frozen_from(&self, player: usize) -> Option<u64> {
        self.open[player]
    }

    /// Does a freeze (and not an input) make the input of `player` for `tick` known?
    pub fn is_known_empty_by_freeze(&self, tick: u64, player: usize) -> bool {
        self.frozen_at(player, tick)
    }

    /// Is the input of `player` for `tick` known (arrived, or made known by a freeze)?
    pub fn is_known(&self, tick: u64, player: usize) -> bool {
        tick < self.base || self.frozen_at(player, tick) || self.real(tick, player).is_some()
    }

    /// The input `player` gave for `tick`, if it is known and not empty by a freeze. `None` means "nothing changes": either an input that is
    /// not known yet (a prediction) or a frozen seat.
    pub fn value_for(&self, tick: u64, player: usize) -> Option<&Json> {
        if player >= self.players || self.frozen_at(player, tick) {
            return None;
        }
        self.real(tick, player)
    }

    /// Is any player's input for `tick` still unknown?
    pub fn has_unknown(&self, tick: u64) -> bool {
        (0..self.players).any(|p| !self.is_known(tick, p))
    }

    /// The changes (not empty inputs) of ticks `from..to`, in tick order and, within a tick, in player order.
    pub fn changes(&self, from: u64, to: u64) -> Vec<(u64, usize, Json)> {
        let mut out = Vec::new();
        if from >= to {
            return out;
        }
        for (&t, row) in self.inputs.range(from..to) {
            for (p, v) in row.iter().enumerate() {
                if let Some(v) = v {
                    if !is_empty_input(v) && !self.frozen_at(p, t) {
                        out.push((t, p, v.clone()));
                    }
                }
            }
        }
        out
    }

    /// Every input the book still holds for ticks at or above `from` (including empty ones): what a host forwards to a seat that joins
    /// late. Ticks that are frozen are left out.
    pub fn held_from(&self, from: u64) -> Vec<(u64, usize, Json)> {
        let mut out = Vec::new();
        for (&t, row) in self.inputs.range(from..) {
            for (p, v) in row.iter().enumerate() {
                if let Some(v) = v {
                    if !self.frozen_at(p, t) {
                        out.push((t, p, v.clone()));
                    }
                }
            }
        }
        out
    }

    /// Forget the inputs of ticks below `below` (which must not be above the frontier): they are settled and nobody asks for them again.
    pub fn prune(&mut self, below: u64) {
        if below <= self.base {
            return;
        }
        self.inputs = self.inputs.split_off(&below);
        self.base = below;
        for c in &mut self.closed {
            c.retain(|&(_, to)| to > below);
        }
        for p in 0..self.players {
            self.through[p] = self.through[p].max(below.min(self.known_through(p)));
        }
    }

    pub fn base(&self) -> u64 {
        self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(k: &str) -> Json {
        Json::obj([(k, Json::Bool(true))])
    }

    #[test]
    fn the_frontier_is_the_slowest_players_unbroken_run() {
        let mut b = InputBook::new(2, 0);
        assert_eq!(b.frontier(), 0);
        assert!(b.put(0, 0, Json::obj::<&str>([])));
        assert!(b.put(1, 0, keys("d")));
        assert_eq!(b.frontier(), 0, "player 1 has not said anything");
        assert!(b.put(0, 1, Json::obj::<&str>([])));
        assert_eq!(b.frontier(), 1);
        // an input that arrives early waits for the gap before it to close
        assert!(b.put(2, 1, Json::obj::<&str>([])));
        assert_eq!(b.known_through(1), 1);
        assert!(b.put(1, 1, keys("a")));
        assert_eq!(b.known_through(1), 3);
        assert_eq!(b.frontier(), 2, "player 0 is known through 2");
        assert!(b.has_unknown(2));
        assert!(!b.has_unknown(1));
    }

    #[test]
    fn duplicates_and_stale_inputs_are_ignored() {
        let mut b = InputBook::new(1, 0);
        assert!(b.put(0, 0, keys("a")));
        assert!(!b.put(0, 0, keys("b")), "the first one stays");
        assert_eq!(b.value_for(0, 0), Some(&keys("a")));
        assert!(!b.put(0, 5, keys("a")), "no such player");
        b.put(1, 0, Json::obj::<&str>([]));
        assert!(!b.put(1, 0, keys("a")));
    }

    #[test]
    fn changes_come_out_in_tick_then_player_order_and_leave_out_empty_inputs() {
        let mut b = InputBook::new(3, 0);
        for t in 0..4 {
            for p in (0..3).rev() {
                b.put(t, p, if (t + p as u64) % 2 == 0 { keys("x") } else { Json::obj::<&str>([]) });
            }
        }
        let ch = b.changes(0, 4);
        let order: Vec<(u64, usize)> = ch.iter().map(|(t, p, _)| (*t, *p)).collect();
        assert_eq!(order, vec![(0, 0), (0, 2), (1, 1), (2, 0), (2, 2), (3, 1)]);
        assert!(b.changes(3, 3).is_empty());
    }

    #[test]
    fn a_frozen_seat_does_not_hold_the_others_up_and_can_come_back() {
        let mut b = InputBook::new(2, 0);
        for t in 0..5 {
            b.put(t, 0, Json::obj::<&str>([]));
        }
        b.put(0, 1, Json::obj::<&str>([]));
        b.put(1, 1, Json::obj::<&str>([]));
        assert_eq!(b.frontier(), 2);
        assert_eq!(b.received_through(1), 2);
        b.freeze(1, 2);
        assert_eq!(b.known_through(1), OPEN);
        assert_eq!(b.frontier(), 5, "player 0 is the only one who counts now");
        assert!(b.is_known(4, 1) && b.value_for(4, 1).is_none());
        assert!(!b.put(3, 1, keys("d")), "a frozen seat's inputs are not accepted");
        // the seat comes back at tick 7: the ticks in between are empty, from 7 on its own inputs are needed
        for t in 5..9 {
            b.put(t, 0, Json::obj::<&str>([]));
        }
        b.unfreeze(1, 7);
        assert_eq!(b.known_through(1), 7);
        assert_eq!(b.frontier(), 7);
        assert!(b.put(7, 1, keys("d")));
        assert_eq!(b.frontier(), 8);
        assert!(b.is_frozen(1) == false);
    }

    #[test]
    fn an_input_that_comes_before_the_news_that_its_seat_is_back_is_kept_and_counts_from_then_on() {
        let mut b = InputBook::new(2, 0);
        for t in 0..3 {
            b.put(t, 0, Json::obj::<&str>([]));
            b.put(t, 1, Json::obj::<&str>([]));
        }
        b.freeze(1, 3);
        assert!(!b.put(7, 1, keys("d")), "the strict `put` refuses a seat's input inside a freeze");
        assert!(b.put_told(7, 1, keys("d")), "...the one for receivers that were told by the host keeps it");
        assert!(b.value_for(7, 1).is_none() && b.first_change_from(1, 0).is_none(), "while the seat is frozen the kept input does not count");
        assert_eq!(b.known_through(1), OPEN);
        // the news arrives: the seat is back from tick 7, and the input counts
        b.unfreeze(1, 7);
        assert_eq!(b.value_for(7, 1), Some(&keys("d")));
        assert_eq!(b.first_change_from(1, 7), Some(7));
        assert_eq!(b.first_change_from(1, 8), None);
        assert_eq!(b.known_through(1), 8, "ticks 3 to 6 are empty by the freeze, 7 is the kept input");
    }

    #[test]
    fn prune_drops_settled_ticks_only() {
        let mut b = InputBook::new(2, 0);
        for t in 0..10 {
            b.put(t, 0, keys("a"));
            b.put(t, 1, Json::obj::<&str>([]));
        }
        b.prune(6);
        assert_eq!(b.base(), 6);
        assert_eq!(b.frontier(), 10);
        assert!(b.is_known(2, 0), "dropped ticks count as known");
        assert!(b.value_for(2, 0).is_none());
        assert_eq!(b.changes(0, 10).len(), 4, "ticks 6..10 are still held");
        // a book that starts later: everything before is known
        let mut late = InputBook::new(2, 100);
        assert_eq!(late.frontier(), 100);
        assert!(late.put(100, 0, keys("a")));
        assert!(!late.put(99, 0, keys("a")));
    }
}
