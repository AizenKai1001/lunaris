//! Parties (docs/PARTIES.md): friends who group up on a game server. This is the membership: who is in which party, who leads it, who has been invited, how long a seat is kept for a player whose connection dropped, what
//! the players type to run it, and the panel message each member is told. It knows nothing of the simulation (`simgame::party_glue` puts the party's number on the avatars as their `Team`, shares the places to get up at and the
//! land, and gives this the positions for the panel) and nothing of the network: it is a table that a test drives with names and a clock in milliseconds.
//!
//! **Server-authoritative.** A client can only ask (`/invite Bo`, `/accept`, the commands `party_accept`...), and every ask is checked here: a refused one costs the asker a line of the chat log that says why, and nothing else
//! happens. Nobody can be put in a party without saying yes, nobody but the leader can invite, kick, promote or disband, the invitations are limited (three at once and one more every five seconds for an inviter, three waiting
//! for an invitee, thirty seconds each, and twenty seconds of quiet for whoever said no), and a name is only the *cleaned* name of the chat ([`clean_name`]: a line break or a control character in a name cannot make one
//! look like another's, and two players whose names clean to the same text cannot be told apart, so a command by that name is refused). The rules, in the order the tests go through them:
//!
//! * a party has a leader and at most `max` members (`NetHost.party`, 4), and a player is in at most one;
//! * a party of fewer than two ends: its last member is told, and its number is free again (numbers are handed out round and round from 100 and not used again until the range has gone round);
//! * the leader going (`/leave`, kicked is not possible) hands the party to the **oldest member who is here**; a leader whose connection drops does the same at once, and is an ordinary member when it comes back;
//! * a member whose connection drops keeps its seat for [`GRACE_MS`] (60 s: a crash or a minute without a network does not drop the party): it counts as a member (it takes a place, the party does not end for it) but is told nothing and
//!   shows as `away`; a player of the same name who joins in that time takes the seat back.
use super::bytes::{R, W};
use super::simgame::clean_chat;
use lunaris_sim::party::PARTY_TEAM_BASE;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// The message kind of the panel (the next free ones are the neighbours' by the day this was written: 0x40 to 0x49 are the game server's, see `simgame::msg`).
pub mod msg {
    /// To one player: its party and invitation (see [`super::Panel`]).
    pub const PANEL: u8 = 0x4C;
}

/// The byte after [`super::simgame::msg::SAY`] that says the text is for the speaker's party only. A text that starts with a printable character is for the players near (as ever: the server drops control characters from
/// what is said, so no old client starts a text with one).
pub const CH_PARTY: u8 = 0x01;

/// The message a player's game sends to say something to its party only: [`super::simgame::say`] with the channel byte after the kind.
pub fn say(text: &str) -> Vec<u8> {
    let mut w = W::new();
    w.u8(super::simgame::msg::SAY);
    w.u8(CH_PARTY);
    let t = text.as_bytes();
    w.raw(&t[..t.len().min(479)]);
    w.buf
}

/// How long an invitation waits.
pub const INVITE_LIFE_MS: u64 = 30_000;
/// How long a member who is not connected keeps its seat.
pub const GRACE_MS: u64 = 60_000;
/// How long whoever declined is left alone by the one who invited them.
pub const DECLINE_MS: u64 = 20_000;
/// The first party number (its `Team.id`) and how many there are.
pub const ID_BASE: u32 = PARTY_TEAM_BASE;
const ID_SPAN: u32 = 60_000;
/// The most members a party can have, whatever `NetHost.party` says.
pub const MAX_MEMBERS: usize = 16;
/// A name is cut to this many characters (as the chat's is).
pub const NAME_CHARS: usize = 24;
/// Invitations a player may send at once, and the milliseconds before it may send one more.
const INVITE_BURST: f32 = 3.0;
const INVITE_EVERY_MS: f32 = 5_000.0;
/// Invitations that may wait for one player.
const MAX_WAITING: usize = 3;
/// Commands a player may give at once and the most a second after that: nobody can make the table do more than this much work.
const CMD_BURST: f32 = 8.0;
const CMD_PER_S: f32 = 2.0;

/// What a name is as far as parties go: the chat's cleaning (printable characters, one space between words, at most [`NAME_CHARS`]) and `player` for nothing at all.
pub fn clean_name(raw: &str) -> String {
    let name: String = clean_chat(raw.as_bytes()).chars().take(NAME_CHARS).collect();
    if name.is_empty() {
        "player".to_string()
    } else {
        name
    }
}

/// How a name typed by a player is looked for among candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// Nobody is called that.
    None,
    /// More than one is: the command is refused, whoever it was meant for.
    Many,
}

/// The one of `candidates` (an id and its cleaned name) that `text` names: exactly as written, else without regard to the case of plain letters, and only if that is one.
pub fn resolve<'a>(candidates: impl Iterator<Item = (usize, &'a str)>, text: &str) -> Result<usize, Lookup> {
    let want = clean_chat(text.as_bytes());
    if want.is_empty() {
        return Err(Lookup::None);
    }
    let (mut exact, mut loose) = (Vec::new(), Vec::new());
    for (id, key) in candidates {
        if key == want {
            exact.push(id);
        } else if key.eq_ignore_ascii_case(&want) {
            loose.push(id);
        }
    }
    match (exact.len(), loose.len()) {
        (1, _) => Ok(exact[0]),
        (0, 1) => Ok(loose[0]),
        (0, 0) => Err(Lookup::None),
        _ => Err(Lookup::Many),
    }
}

// ------------------------------------------------------------------------------------------------ what players type

/// A command typed in the chat box (a text that starts with `/`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd<'a> {
    Invite(&'a str),
    Accept(&'a str),
    Decline(&'a str),
    Leave,
    Kick(&'a str),
    Disband,
    Promote(&'a str),
    /// `/p text`: to the party only.
    Say(&'a str),
    Help,
}

/// Read a command from a cleaned text (one space between words, nothing round it). `None` for a word that is none of ours.
pub fn parse_command(text: &str) -> Option<Cmd<'_>> {
    let body = text.strip_prefix('/')?;
    let (word, rest) = body.split_once(' ').unwrap_or((body, ""));
    let is = |w: &str| word.eq_ignore_ascii_case(w);
    Some(if is("invite") || is("inv") {
        Cmd::Invite(rest)
    } else if is("accept") || is("join") {
        Cmd::Accept(rest)
    } else if is("decline") || is("deny") {
        Cmd::Decline(rest)
    } else if is("leave") {
        Cmd::Leave
    } else if is("kick") {
        Cmd::Kick(rest)
    } else if is("disband") {
        Cmd::Disband
    } else if is("promote") || is("leader") {
        Cmd::Promote(rest)
    } else if is("p") || is("party") && !rest.is_empty() {
        Cmd::Say(rest)
    } else if is("help") || is("party") || is("?") {
        Cmd::Help
    } else {
        return None;
    })
}

/// What `/help` says.
pub const HELP: [&str; 4] = ["/invite NAME   (just /invite: the nearest player with no party)", "/accept   /decline   /leave", "/kick NAME   /promote NAME   /disband   (the leader)", "/p TEXT  says it to your party, wherever it is"];

// ------------------------------------------------------------------------------------------------ the table

/// A member of a party.
#[derive(Clone, Debug)]
pub struct Member {
    /// The cleaned name.
    pub key: String,
    /// The connection it is on now; `None` while it is away.
    pub slot: Option<usize>,
    /// When it went away (ms), for the grace.
    pub away_since: u64,
}

#[derive(Clone, Debug)]
pub struct Party {
    pub id: u32,
    /// Oldest first.
    pub members: Vec<Member>,
    /// The leader's name; `None` while every member is away (the first one back leads).
    pub leader: Option<String>,
}

impl Party {
    /// The slots of the members that are here.
    pub fn here(&self) -> impl Iterator<Item = usize> + '_ {
        self.members.iter().filter_map(|m| m.slot)
    }
}

/// An invitation that waits.
#[derive(Clone, Copy, Debug)]
struct Invite {
    from: usize,
    to: usize,
    at: u64,
}

/// What the table keeps of each connected player.
#[derive(Clone, Debug)]
pub struct Player {
    pub key: String,
    /// The party it is in (0: none).
    pub party: u32,
    /// The `Team.id` its avatar was made with: what it goes back to when it leaves a party.
    pub base_team: u32,
    invite_credit: f32,
    invite_at: u64,
    cmd_credit: f32,
    cmd_at: u64,
    /// The panel: a hash of the last one told (0: none), when the player's panel was last looked at and when it was last sent (ms), whether the client shows anything, and whether the next is not to wait for the second.
    pub told: u64,
    pub told_at: u64,
    pub sent_at: u64,
    pub shown: bool,
    pub urgent: bool,
}

#[derive(Default)]
pub struct Parties {
    max: usize,
    next: u32,
    pub parties: BTreeMap<u32, Party>,
    pub players: BTreeMap<usize, Player>,
    /// The party of every member by name, away or here: how a player who comes back finds its seat.
    seats: HashMap<String, u32>,
    invites: VecDeque<Invite>,
    /// (the inviter, the invitee, until when): who said no to whom lately.
    declined: Vec<(usize, usize, u64)>,
    /// The earliest moment a member that is away loses its seat (0: nobody is away).
    next_expiry: u64,
    notes: Vec<(usize, String)>,
    /// The connections whose party changed since the host last looked (it puts the numbers on their avatars).
    pub changed: Vec<usize>,
    /// Goes up whenever who is in which party changes.
    pub version: u32,
    /// How many players' clients show a panel (the host keeps it: it is how an idle table costs nothing).
    pub shown_n: usize,
}

impl Parties {
    /// A table for parties of at most `max` members; fewer than 2 is no parties at all.
    pub fn new(max: u32) -> Parties {
        Parties { max: (max as usize).min(MAX_MEMBERS), ..Default::default() }
    }

    pub fn enabled(&self) -> bool {
        self.max >= 2
    }

    pub fn max(&self) -> usize {
        self.max
    }

    /// Nothing to look after: no party, no invitation, no panel on anybody's screen.
    pub fn idle(&self) -> bool {
        self.parties.is_empty() && self.invites.is_empty() && self.shown_n == 0
    }

    /// The lines to tell the players (the slot and the text), and take them.
    pub fn take_notes(&mut self) -> Vec<(usize, String)> {
        std::mem::take(&mut self.notes)
    }

    pub fn take_changed(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.changed)
    }

    fn note(&mut self, slot: usize, text: impl Into<String>) {
        self.notes.push((slot, text.into()));
    }

    fn note_party(&mut self, pid: u32, except: Option<usize>, text: &str) {
        let slots: Vec<usize> = self.parties.get(&pid).map(|p| p.here().filter(|s| Some(*s) != except).collect()).unwrap_or_default();
        for s in slots {
            self.note(s, text);
        }
    }

    /// The panel of every member (and invitee) of party `pid` is told now: something changed.
    fn urgent_party(&mut self, pid: u32) {
        let slots: Vec<usize> = self.parties.get(&pid).map(|p| p.here().collect()).unwrap_or_default();
        for s in slots {
            self.urgent(s);
        }
    }

    pub fn urgent(&mut self, slot: usize) {
        if let Some(p) = self.players.get_mut(&slot) {
            p.urgent = true;
        }
    }

    pub fn party_of(&self, slot: usize) -> u32 {
        self.players.get(&slot).map_or(0, |p| p.party)
    }

    pub fn key_of(&self, slot: usize) -> Option<&str> {
        self.players.get(&slot).map(|p| p.key.as_str())
    }

    pub fn party(&self, id: u32) -> Option<&Party> {
        self.parties.get(&id)
    }

    /// Whether the player on `slot` leads its party.
    pub fn leads(&self, slot: usize) -> bool {
        let Some(p) = self.players.get(&slot) else { return false };
        self.parties.get(&p.party).is_some_and(|party| party.leader.as_deref() == Some(p.key.as_str()))
    }

    /// The party's slots that are here, including `slot`'s own (empty for no party).
    pub fn mates(&self, slot: usize) -> Vec<usize> {
        self.parties.get(&self.party_of(slot)).map(|p| p.here().collect()).unwrap_or_default()
    }

    /// The players who have an invitation waiting (one may be named more than once).
    pub fn invitees(&self) -> impl Iterator<Item = usize> + '_ {
        self.invites.iter().map(|i| i.to)
    }

    /// The invitation waiting for `slot` that `accept` takes (the newest): the inviter's slot, its name and the milliseconds it has left.
    pub fn waiting(&self, slot: usize, now: u64) -> Option<(usize, String, u64)> {
        let i = self.invites.iter().rev().find(|i| i.to == slot)?;
        Some((i.from, self.key_of(i.from).unwrap_or("player").to_string(), (i.at + INVITE_LIFE_MS).saturating_sub(now)))
    }

    /// A player's connection has come: it takes back the seat kept for its name, if there is one and nobody else sits in it.
    pub fn join(&mut self, slot: usize, raw_name: &str, base_team: u32, now: u64) {
        let key = clean_name(raw_name);
        self.players.insert(slot, Player { key: key.clone(), party: 0, base_team, invite_credit: INVITE_BURST, invite_at: now, cmd_credit: CMD_BURST, cmd_at: now, told: 0, told_at: 0, sent_at: 0, shown: false, urgent: false });
        let Some(pid) = self.seats.get(&key).copied() else { return };
        let Some(party) = self.parties.get_mut(&pid) else { return };
        let Some(m) = party.members.iter_mut().find(|m| m.key == key && m.slot.is_none()) else { return };
        m.slot = Some(slot);
        let lead = party.leader.is_none();
        if lead {
            party.leader = Some(key.clone());
        }
        let leader = party.leader.clone().unwrap_or_default();
        if let Some(p) = self.players.get_mut(&slot) {
            p.party = pid;
        }
        self.note_party(pid, Some(slot), &format!("{key} is back."));
        self.note(slot, if lead { "You are back in your party and lead it.".to_string() } else { format!("You are back in {leader}'s party.") });
        self.changed.push(slot);
        self.version = self.version.wrapping_add(1);
        self.urgent_party(pid);
    }

    /// A player's connection has gone: what it asked and was asked is forgotten, and a member keeps its seat for the grace.
    pub fn part(&mut self, slot: usize, now: u64) {
        let Some(me) = self.players.remove(&slot) else { return };
        if me.shown {
            self.shown_n = self.shown_n.saturating_sub(1);
        }
        self.invites.retain(|i| i.from != slot && i.to != slot);
        self.declined.retain(|d| d.0 != slot && d.1 != slot);
        // (what was about to be said to it is not for whoever gets its connection next)
        self.notes.retain(|n| n.0 != slot);
        if me.party == 0 {
            return;
        }
        let Some(party) = self.parties.get_mut(&me.party) else { return };
        if let Some(m) = party.members.iter_mut().find(|m| m.key == me.key && m.slot == Some(slot)) {
            m.slot = None;
            m.away_since = now;
        }
        // (the leader's seat goes to the oldest member who is here; the one who comes back is a member)
        let mut passed = None;
        if party.leader.as_deref() == Some(me.key.as_str()) {
            party.leader = party.members.iter().find(|m| m.slot.is_some()).map(|m| m.key.clone());
            passed = party.leader.clone();
        }
        let pid = me.party;
        self.next_expiry = if self.next_expiry == 0 { now + GRACE_MS } else { self.next_expiry.min(now + GRACE_MS) };
        self.note_party(pid, None, &format!("{} lost the connection (the seat is kept for {} s).", me.key, GRACE_MS / 1000));
        if let Some(l) = passed {
            self.note_party(pid, None, &format!("{l} leads the party now."));
        }
        self.version = self.version.wrapping_add(1);
        self.urgent_party(pid);
    }

    /// Time passes: invitations run out and members that have been away too long lose their seats.
    pub fn tick(&mut self, now: u64) {
        while let Some(i) = self.invites.front().copied() {
            if now.saturating_sub(i.at) < INVITE_LIFE_MS {
                break;
            }
            self.invites.pop_front();
            let (to, from) = (self.key_of(i.to).map(str::to_string), self.key_of(i.from).map(str::to_string));
            if let Some(f) = from {
                self.note(i.to, format!("The invitation from {f} ran out."));
            }
            if let Some(t) = to {
                self.note(i.from, format!("Your invitation to {t} ran out."));
            }
            self.urgent(i.to);
        }
        if !self.declined.is_empty() {
            self.declined.retain(|d| d.2 > now);
        }
        if self.next_expiry != 0 && now >= self.next_expiry {
            let mut next = 0u64;
            let mut dropped: Vec<(u32, String)> = Vec::new();
            for (pid, party) in &self.parties {
                for m in &party.members {
                    if m.slot.is_none() {
                        let until = m.away_since + GRACE_MS;
                        if now >= until {
                            dropped.push((*pid, m.key.clone()));
                        } else {
                            next = if next == 0 { until } else { next.min(until) };
                        }
                    }
                }
            }
            self.next_expiry = next;
            for (pid, key) in dropped {
                self.remove_member(pid, &key, Why::Dropped);
            }
        }
    }

    /// Whether the player may give another command now (a bucket: eight at once, two a second after that). A flood is dropped without a word.
    fn cmd_ok(&mut self, slot: usize, now: u64) -> bool {
        let Some(p) = self.players.get_mut(&slot) else { return false };
        let dt = now.saturating_sub(p.cmd_at) as f32 / 1000.0;
        p.cmd_credit = (p.cmd_credit + dt * CMD_PER_S).min(CMD_BURST);
        p.cmd_at = now;
        if p.cmd_credit < 1.0 {
            return false;
        }
        p.cmd_credit -= 1.0;
        true
    }

    /// Whether the player may give another command now, for what is answered without the table (`/help`, a word that is not ours, a message to the party): the same bucket as the commands'.
    pub fn allow(&mut self, slot: usize, now: u64) -> bool {
        self.cmd_ok(slot, now)
    }

    /// Whether a command now would be let through (the bucket has a token; none is taken): for a command that makes the server search before it asks the table (`/invite` with no name looks for the player nearest to
    /// the speaker), so that a flood of it is dropped before the search and not after.
    pub fn would_allow(&self, slot: usize, now: u64) -> bool {
        self.players.get(&slot).is_some_and(|p| p.cmd_credit + now.saturating_sub(p.cmd_at) as f32 / 1000.0 * CMD_PER_S >= 1.0)
    }

    fn alloc(&mut self) -> u32 {
        loop {
            let id = ID_BASE + self.next % ID_SPAN;
            self.next = self.next.wrapping_add(1);
            if !self.parties.contains_key(&id) {
                return id;
            }
        }
    }

    fn lookup_error(&mut self, slot: usize, e: Lookup, text: &str) {
        let shown: String = clean_chat(text.as_bytes()).chars().take(NAME_CHARS).collect();
        self.note(slot, match e {
            Lookup::None => format!("Nobody is called \"{shown}\"."),
            Lookup::Many => format!("More than one player is called \"{shown}\"."),
        });
    }

    /// `/invite NAME`.
    pub fn invite(&mut self, from: usize, name: &str, now: u64) {
        if !self.cmd_ok(from, now) {
            return;
        }
        let Some(me) = self.players.get_mut(&from) else { return };
        // (an invitation costs a token whatever comes of it: names cannot be tried out for free)
        let dt = now.saturating_sub(me.invite_at) as f32;
        me.invite_credit = (me.invite_credit + dt / INVITE_EVERY_MS).min(INVITE_BURST);
        me.invite_at = now;
        if me.invite_credit < 1.0 {
            let wait = ((1.0 - me.invite_credit) * INVITE_EVERY_MS / 1000.0).ceil() as u32;
            self.note(from, format!("Slow down: you can invite again in {wait} s."));
            return;
        }
        me.invite_credit -= 1.0;
        let (party, my_key) = (me.party, me.key.clone());
        if party != 0 && !self.leads(from) {
            self.note(from, "Only the party leader can invite.");
            return;
        }
        if clean_chat(name.as_bytes()).is_empty() {
            self.note(from, "Say whom: /invite NAME");
            return;
        }
        let target = match resolve(self.players.iter().map(|(s, p)| (*s, p.key.as_str())), name) {
            Ok(t) => t,
            Err(e) => return self.lookup_error(from, e, name),
        };
        let (t_key, t_party) = self.players.get(&target).map(|p| (p.key.clone(), p.party)).unwrap_or_default();
        if target == from {
            self.note(from, "You cannot invite yourself.");
        } else if t_party != 0 && t_party == party {
            self.note(from, format!("{t_key} is in your party already."));
        } else if t_party != 0 {
            self.note(from, format!("{t_key} is in a party already."));
        } else if party != 0 && self.parties.get(&party).is_some_and(|p| p.members.len() >= self.max) {
            self.note(from, format!("Your party is full ({}).", self.max));
        } else if self.declined.iter().any(|d| d.0 == from && d.1 == target && d.2 > now) {
            self.note(from, format!("{t_key} is not taking invitations from you just now."));
        } else if self.invites.iter().any(|i| i.from == from && i.to == target) {
            self.note(from, format!("You have invited {t_key} already."));
        } else if self.invites.iter().filter(|i| i.to == target).count() >= MAX_WAITING {
            self.note(from, format!("{t_key} has too many invitations waiting."));
        } else {
            self.invites.push_back(Invite { from, to: target, at: now });
            self.note(from, format!("Invitation sent to {t_key}."));
            self.note(target, format!("{my_key} invites you to a party. Type /accept or /decline ({} s).", INVITE_LIFE_MS / 1000));
            self.urgent(target);
        }
    }

    /// The invitation to `slot` that `from` names (by name; an empty text: the newest one), as its index in the queue.
    fn find_invite(&mut self, slot: usize, from: &str) -> Result<usize, String> {
        let mine: Vec<(usize, usize)> = self.invites.iter().enumerate().filter(|(_, i)| i.to == slot).map(|(k, i)| (k, i.from)).collect();
        if mine.is_empty() {
            return Err("You have no invitation.".to_string());
        }
        if clean_chat(from.as_bytes()).is_empty() {
            return Ok(mine[mine.len() - 1].0);
        }
        let names: Vec<(usize, String)> = mine.iter().map(|(k, f)| (*k, self.key_of(*f).unwrap_or("player").to_string())).collect();
        match resolve(names.iter().map(|(k, n)| (*k, n.as_str())), from) {
            Ok(k) => Ok(k),
            Err(_) => Err(format!("No invitation from \"{}\".", clean_chat(from.as_bytes()).chars().take(NAME_CHARS).collect::<String>())),
        }
    }

    /// `/accept [NAME]`.
    pub fn accept(&mut self, slot: usize, from: &str, now: u64) {
        if !self.cmd_ok(slot, now) {
            return;
        }
        if self.party_of(slot) != 0 {
            self.note(slot, "You are in a party already. Type /leave first.");
            return;
        }
        let k = match self.find_invite(slot, from) {
            Ok(k) => k,
            Err(text) => return self.note(slot, text),
        };
        let inv = self.invites.remove(k).expect("an index that was just found");
        if now.saturating_sub(inv.at) >= INVITE_LIFE_MS {
            self.note(slot, "That invitation ran out.");
            self.urgent(slot);
            return;
        }
        // (the inviter may have joined another party since: an invitation is for the party its inviter leads, or for a new one)
        let (Some(inviter_key), inviter_party) = (self.key_of(inv.from).map(str::to_string), self.party_of(inv.from)) else {
            self.note(slot, "That invitation is gone.");
            return;
        };
        if inviter_party != 0 && !self.leads(inv.from) {
            self.note(slot, format!("{inviter_key} cannot invite you any more."));
            self.urgent(slot);
            return;
        }
        let my_key = self.key_of(slot).unwrap_or("player").to_string();
        let pid = if inviter_party != 0 {
            inviter_party
        } else {
            let id = self.alloc();
            self.parties.insert(id, Party { id, members: vec![Member { key: inviter_key.clone(), slot: Some(inv.from), away_since: 0 }], leader: Some(inviter_key.clone()) });
            self.seats.insert(inviter_key.clone(), id);
            if let Some(p) = self.players.get_mut(&inv.from) {
                p.party = id;
            }
            self.changed.push(inv.from);
            id
        };
        if self.parties.get(&pid).is_some_and(|p| p.members.len() >= self.max) {
            self.note(slot, "That party is full.");
            self.note(inv.from, format!("{my_key} could not join: your party is full."));
            self.urgent(slot);
            return;
        }
        if let Some(party) = self.parties.get_mut(&pid) {
            party.members.push(Member { key: my_key.clone(), slot: Some(slot), away_since: 0 });
        }
        self.seats.insert(my_key.clone(), pid);
        if let Some(p) = self.players.get_mut(&slot) {
            p.party = pid;
        }
        // (it is in a party: the invitations to it are void, and so are the ones it sent as a player with no party)
        self.invites.retain(|i| i.to != slot && i.from != slot);
        self.changed.push(slot);
        self.version = self.version.wrapping_add(1);
        let leader = self.parties.get(&pid).and_then(|p| p.leader.clone()).unwrap_or_default();
        self.note_party(pid, Some(slot), &format!("{my_key} joined the party."));
        self.note(slot, format!("You joined {leader}'s party."));
        self.urgent_party(pid);
    }

    /// `/decline [NAME]`.
    pub fn decline(&mut self, slot: usize, from: &str, now: u64) {
        if !self.cmd_ok(slot, now) {
            return;
        }
        let k = match self.find_invite(slot, from) {
            Ok(k) => k,
            Err(text) => return self.note(slot, text),
        };
        let inv = self.invites.remove(k).expect("an index that was just found");
        self.declined.push((inv.from, slot, now + DECLINE_MS));
        let me = self.key_of(slot).unwrap_or("player").to_string();
        let who = self.key_of(inv.from).unwrap_or("player").to_string();
        self.note(slot, format!("You declined the invitation from {who}."));
        self.note(inv.from, format!("{me} declined your invitation."));
        self.urgent(slot);
    }

    /// `/leave`.
    pub fn leave(&mut self, slot: usize, now: u64) {
        if !self.cmd_ok(slot, now) {
            return;
        }
        let Some(me) = self.players.get(&slot) else { return };
        if me.party == 0 {
            self.note(slot, "You are not in a party.");
            return;
        }
        let (pid, key) = (me.party, me.key.clone());
        self.remove_member(pid, &key, Why::Left);
    }

    /// `/kick NAME`: the leader sends a member away (one that is away too).
    pub fn kick(&mut self, slot: usize, name: &str, now: u64) {
        if !self.cmd_ok(slot, now) {
            return;
        }
        let Some((pid, target)) = self.lead_target(slot, name, "kick") else { return };
        let me = self.key_of(slot).unwrap_or("player").to_string();
        if target == me {
            self.note(slot, "Use /leave to go yourself.");
            return;
        }
        self.remove_member(pid, &target, Why::Kicked);
    }

    /// `/promote NAME`: the leader gives the party to a member who is here.
    pub fn promote(&mut self, slot: usize, name: &str, now: u64) {
        if !self.cmd_ok(slot, now) {
            return;
        }
        let Some((pid, target)) = self.lead_target(slot, name, "promote") else { return };
        let me = self.key_of(slot).unwrap_or("player").to_string();
        if target == me {
            self.note(slot, "You lead it already.");
            return;
        }
        let here = self.parties.get(&pid).is_some_and(|p| p.members.iter().any(|m| m.key == target && m.slot.is_some()));
        if !here {
            self.note(slot, format!("{target} is away."));
            return;
        }
        if let Some(p) = self.parties.get_mut(&pid) {
            p.leader = Some(target.clone());
        }
        self.note_party(pid, None, &format!("{target} leads the party now."));
        self.version = self.version.wrapping_add(1);
        self.urgent_party(pid);
    }

    /// The party and the member (by name) a leader's command is about; says why not and gives `None` when the asker is no leader or the name is not a member.
    fn lead_target(&mut self, slot: usize, name: &str, what: &str) -> Option<(u32, String)> {
        let pid = self.party_of(slot);
        if pid == 0 {
            self.note(slot, "You are not in a party.");
            return None;
        }
        if !self.leads(slot) {
            self.note(slot, format!("Only the party leader can {what}."));
            return None;
        }
        if clean_chat(name.as_bytes()).is_empty() {
            self.note(slot, format!("Say whom: /{what} NAME"));
            return None;
        }
        let keys: Vec<(usize, String)> = self.parties.get(&pid)?.members.iter().enumerate().map(|(i, m)| (i, m.key.clone())).collect();
        match resolve(keys.iter().map(|(i, k)| (*i, k.as_str())), name) {
            Ok(i) => Some((pid, keys[i].1.clone())),
            Err(e) => {
                // (a name that is nobody's in this party is said as "not in your party", so that the names of other parties' members are not told)
                let shown: String = clean_chat(name.as_bytes()).chars().take(NAME_CHARS).collect();
                self.note(slot, if e == Lookup::Many { format!("More than one member is called \"{shown}\".") } else { format!("\"{shown}\" is not in your party.") });
                None
            }
        }
    }

    /// `/disband`: the leader ends the party.
    pub fn disband(&mut self, slot: usize, now: u64) {
        if !self.cmd_ok(slot, now) {
            return;
        }
        let pid = self.party_of(slot);
        if pid == 0 {
            self.note(slot, "You are not in a party.");
        } else if !self.leads(slot) {
            self.note(slot, "Only the party leader can disband it.");
        } else {
            self.dissolve(pid, "The party was disbanded.");
        }
    }

    /// The slots a party message from `slot` goes to (its party's, the speaker included); `None` with a word to the speaker when it has none.
    pub fn audience(&mut self, slot: usize) -> Option<Vec<usize>> {
        let mates = self.mates(slot);
        if mates.is_empty() {
            self.note(slot, "You are not in a party. Invite somebody: /invite NAME");
            return None;
        }
        Some(mates)
    }

    /// Take a member out of its party and say so; a party of fewer than two ends.
    fn remove_member(&mut self, pid: u32, key: &str, why: Why) {
        let Some(party) = self.parties.get_mut(&pid) else { return };
        let Some(i) = party.members.iter().position(|m| m.key == key) else { return };
        let gone = party.members.remove(i);
        let mut passed = None;
        if party.leader.as_deref() == Some(key) {
            party.leader = party.members.iter().find(|m| m.slot.is_some()).map(|m| m.key.clone());
            passed = party.leader.clone();
        }
        let left = party.members.len();
        self.seats.remove(key);
        if let Some(s) = gone.slot {
            if let Some(p) = self.players.get_mut(&s) {
                p.party = 0;
            }
            self.changed.push(s);
            self.urgent(s);
            self.note(s, match why {
                Why::Left => "You left the party.",
                Why::Kicked => "You were removed from the party.",
                Why::Dropped => "You were dropped from the party.",
            });
        }
        self.version = self.version.wrapping_add(1);
        if left < 2 {
            self.dissolve(pid, "The party ended.");
            return;
        }
        let said = match why {
            Why::Left => format!("{key} left the party."),
            Why::Kicked => format!("{key} was removed from the party."),
            Why::Dropped => format!("{key} was dropped from the party (away too long)."),
        };
        self.note_party(pid, None, &said);
        if let Some(l) = passed {
            self.note_party(pid, None, &format!("{l} leads the party now."));
        }
        self.urgent_party(pid);
    }

    /// End a party: everybody in it is out of it and told `text`, and its number is free.
    fn dissolve(&mut self, pid: u32, text: &str) {
        let Some(party) = self.parties.remove(&pid) else { return };
        for m in &party.members {
            self.seats.remove(&m.key);
            if let Some(s) = m.slot {
                if let Some(p) = self.players.get_mut(&s) {
                    p.party = 0;
                }
                self.changed.push(s);
                self.urgent(s);
                self.note(s, text);
            }
        }
        self.version = self.version.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    Left,
    Kicked,
    Dropped,
}

// ------------------------------------------------------------------------------------------------ the panel

/// What a member is told of its party and its invitation: the message [`msg::PANEL`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Panel {
    /// The party's number (0: in none).
    pub party: u32,
    /// The receiver leads it.
    pub leader: bool,
    pub invite: Option<Invitation>,
    pub members: Vec<Row>,
}

/// An invitation waiting: who from, and the seconds it has left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invitation {
    pub from: String,
    pub secs: u8,
}

/// A member as the panel shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// The server's number of the player (`NetPuppet.id` of its puppet on the others' screens).
    pub id: u32,
    /// 0 dead, 255 full.
    pub health: u8,
    /// Metres from the receiver (nearest whole number, [`Row::FAR`] when not known).
    pub dist: u16,
    /// The way from the receiver by the compass, a turn in 256 steps from north (-z) through east (+x).
    pub bearing: u8,
    pub flags: u8,
}

impl Row {
    pub const LEADER: u8 = 1;
    pub const AWAY: u8 = 2;
    pub const ME: u8 = 4;
    pub const DEAD: u8 = 8;
    pub const FAR: u16 = u16::MAX;
}

/// The bearing of a displacement (east `dx`, south `dz`: north is -z) as the byte of [`Row::bearing`].
pub fn bearing_byte(dx: f32, dz: f32) -> u8 {
    if !(dx.is_finite() && dz.is_finite()) || (dx == 0.0 && dz == 0.0) {
        return 0;
    }
    let turns = dx.atan2(-dz) / std::f32::consts::TAU;
    ((turns.rem_euclid(1.0) * 256.0).round() as u32 % 256) as u8
}

/// The eight points of the compass for a bearing byte.
pub fn compass(bearing: u8) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[((bearing as u32 * 8 + 128) / 256 % 8) as usize]
}

fn put_name(w: &mut W, name: &str) {
    let b = name.as_bytes();
    let mut n = b.len().min(60);
    while n > 0 && !name.is_char_boundary(n) {
        n -= 1;
    }
    w.u8(n as u8);
    w.raw(&b[..n]);
}

fn get_name(r: &mut R) -> Option<String> {
    let n = r.u8()? as usize;
    Some(String::from_utf8_lossy(r.raw(n)?).into_owned())
}

impl Panel {
    pub fn is_empty(&self) -> bool {
        self.party == 0 && self.invite.is_none()
    }

    /// The message: kind, flags (1 in a party, 2 leads, 4 invited), the party's number, the invitation (seconds and name) if there is one, the members.
    pub fn message(&self) -> Vec<u8> {
        let mut w = W::new();
        w.u8(msg::PANEL);
        w.u8((self.party != 0) as u8 | (self.leader as u8) << 1 | (self.invite.is_some() as u8) << 2);
        w.varu(self.party as u64);
        if let Some(i) = &self.invite {
            w.u8(i.secs);
            put_name(&mut w, &i.from);
        }
        w.u8(self.members.len().min(MAX_MEMBERS) as u8);
        for m in self.members.iter().take(MAX_MEMBERS) {
            put_name(&mut w, &m.name);
            w.varu(m.id as u64);
            w.u8(m.health);
            w.u16(m.dist);
            w.u8(m.bearing);
            w.u8(m.flags);
        }
        w.buf
    }

    /// Read a message of [`msg::PANEL`]; `None` for anything else or a message that is cut short.
    pub fn parse(data: &[u8]) -> Option<Panel> {
        let mut r = R::new(data);
        if r.u8()? != msg::PANEL {
            return None;
        }
        let flags = r.u8()?;
        let party = u32::try_from(r.varu()?).ok()?;
        let invite = if flags & 4 != 0 {
            let secs = r.u8()?;
            Some(Invitation { from: get_name(&mut r)?, secs })
        } else {
            None
        };
        let n = r.u8()? as usize;
        let mut members = Vec::with_capacity(n.min(MAX_MEMBERS));
        for _ in 0..n.min(MAX_MEMBERS) {
            members.push(Row { name: get_name(&mut r)?, id: u32::try_from(r.varu()?).ok()?, health: r.u8()?, dist: r.u16()?, bearing: r.u8()?, flags: r.u8()? });
        }
        Some(Panel { party, leader: flags & 2 != 0, invite, members })
    }

    /// The lines the player's screen shows (`NetParty`), `secs` being what is left of the invitation now and `hint` what the game says to do about it (empty: the commands).
    pub fn lines(&self, secs: u32, hint: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(i) = &self.invite {
            out.push(format!("{} invites you to a party ({} s)", i.from, secs));
            out.push(if hint.trim().is_empty() { "/accept   /decline".to_string() } else { hint.trim().to_string() });
            return out;
        }
        if self.party == 0 {
            return out;
        }
        out.push(format!("PARTY ({})", self.members.len()));
        for m in &self.members {
            let mark = if m.flags & Row::LEADER != 0 { "* " } else { "- " };
            let mut line = format!("{mark}{}", m.name);
            if m.flags & Row::AWAY != 0 {
                line.push_str("   away");
            } else {
                if m.flags & Row::DEAD != 0 || m.health == 0 {
                    line.push_str("   dead");
                } else {
                    line.push_str(&format!("   {}%", (m.health as u32 * 100 + 127) / 255));
                }
                if m.flags & Row::ME != 0 {
                    line.push_str("   you");
                } else if m.dist != Row::FAR {
                    if m.dist >= 1000 {
                        line.push_str(&format!("   {:.1} km {}", m.dist as f32 / 1000.0, compass(m.bearing)));
                    } else {
                        line.push_str(&format!("   {} m {}", m.dist, compass(m.bearing)));
                    }
                }
            }
            out.push(line);
        }
        out
    }
}
