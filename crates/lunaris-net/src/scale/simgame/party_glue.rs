//! The party system's side of [`SimGame`] (docs/PARTIES.md): what `scale::party` decides is put on the avatars (their `Team`, the tool's party), what the players type is read (`/invite Bo`, `/p text`, the commands
//! `party_accept` ...), and each member is told its panel. A child of `simgame` so that it reaches the server's private tables without changing their visibility.
//!
//! Costs: nothing while no party and no invitation exists (a test of one flag a tick); with parties, a pass over the players that are in one or are shown a panel (a comparison each), and for each of them, once a second or when
//! something changed, a message of about 30 bytes a member. Nothing here looks at a player that is in no party.
use super::{chat, health_byte, Ctx, SimGame};
use crate::scale::party::{clean_name, parse_command, Cmd, Invitation, Panel, Parties, Row, CH_PARTY, HELP};
use lunaris_sim::components::{BuildTool, HitPoints, Respawn, Team, Transform};
use lunaris_sim::party::PARTY_TEAM_BASE;
use std::time::Instant;

/// What a game server keeps for its parties besides the table.
#[derive(Default)]
pub(super) struct PartyState {
    pub(super) parties: Parties,
    /// The clock (milliseconds) of the last tick: what a join and a leave, which are not given a context, are stamped with.
    now: u64,
    /// What the wallhack defence is told (`Game::allies`): a member's server number and its party; the version of the list; the `Parties::version` it was made for.
    pub(super) allies: Vec<(u32, u32)>,
    pub(super) allies_version: u32,
    built: u32,
    /// A member's avatar had no server number yet when the list was made: make it again.
    pending_net: bool,
    /// The slots a pass over the panels looks at (kept so that it does not allocate every tick).
    scratch: Vec<usize>,
}

impl PartyState {
    /// A state for parties of at most `max` members (`NetHost.party`).
    pub(super) fn new(max: u32) -> PartyState {
        PartyState { parties: Parties::new(max), ..Default::default() }
    }
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h | 1
}

/// How far `/invite` with no name looks for a player, in metres: no farther than the wallhack defence's safe radius (`NetHost.occlusion`), so that asking cannot find a player behind a wall that the server hides.
const NEAR_M: f32 = 25.0;

fn clock(ctx: &Ctx) -> u64 {
    ctx.tick as u64 * 1000 / ctx.tick_hz.max(1) as u64
}

impl SimGame {
    /// A player has joined: it takes back the seat kept for its name, its avatar gets the party's numbers, and the places it gets up at get a number of its own.
    pub(super) fn party_join(&mut self, slot: usize, name: &str) {
        if !self.party.parties.enabled() {
            return;
        }
        let Some(uid) = self.avatars.get(&slot).map(|a| a.uid) else { return };
        let e = self.sim.world.entity(uid);
        // (the team the avatar was made with: a party's number that a saved character brought back is not one to go back to)
        let base = e.and_then(|e| self.sim.world.get::<Team>(e)).map_or(0, |t| if t.id >= PARTY_TEAM_BASE { 0 } else { t.id });
        // (a number for the places to get up at, which a sleeping bag the player puts down is given: docs/RESPAWN.md; the scene may have given it one)
        if let Some(r) = e.and_then(|e| self.sim.world.get_mut::<Respawn>(e)) {
            if r.group == 0 {
                r.group = slot as u32 + 1;
            }
        }
        self.party.parties.join(slot, name, base, self.party.now);
        self.party_apply(slot);
    }

    /// A player has gone: its seat is kept for a minute.
    pub(super) fn party_leave(&mut self, slot: usize) {
        if self.party.parties.enabled() {
            self.party.parties.part(slot, self.party.now);
        }
    }

    /// Put the party's numbers on the avatar of `slot`: its `Team` (the party's number, or what it was made with) and its tool's `party`.
    fn party_apply(&mut self, slot: usize) {
        let Some((uid, head)) = self.avatars.get(&slot).map(|a| (a.uid, a.head)) else { return };
        let Some(e) = self.sim.world.entity(uid) else { return };
        let pid = self.party.parties.party_of(slot);
        let base = self.party.parties.players.get(&slot).map_or(0, |p| p.base_team);
        let team = if pid != 0 { pid } else { base };
        match self.sim.world.get_mut::<Team>(e) {
            Some(t) => t.id = team,
            None if team != 0 => {
                let _ = self.sim.world.insert(e, Team { id: team });
            }
            None => {}
        }
        let tool = if head != 0 { self.sim.world.entity(head) } else { Some(e) }.unwrap_or(e);
        if let Some(t) = self.sim.world.get_mut::<BuildTool>(tool) {
            t.party = pid;
        }
    }

    /// What `say` does first: the party channel and the commands. True when the text was dealt with here (it is then not for the players near).
    pub(super) fn party_say(&mut self, ctx: &mut Ctx, slot: usize, body: &[u8]) -> bool {
        let (to_party, bytes) = match body.first() {
            Some(&CH_PARTY) => (true, &body[1..]),
            _ => (false, body),
        };
        if !self.party.parties.enabled() {
            // (a party's word on a server that has no parties is nobody's: it is not turned into talk for the players near)
            return to_party;
        }
        let text = super::clean_chat(bytes);
        if text.is_empty() {
            return to_party;
        }
        let now = clock(ctx);
        if text.starts_with('/') {
            match parse_command(&text) {
                // (no name: the player nearest to you that is in no party, as a walk up to a player and a key does in Rust; the search over the players is only made for a command that is not dropped, so a flood of these costs what any command does)
                Some(Cmd::Invite("")) if !self.party.parties.would_allow(slot, now) => {}
                Some(Cmd::Invite("")) => match self.nearest_free(slot, if self.host.occlusion > 0.0 { NEAR_M.min(self.host.occlusion) } else { NEAR_M }) {
                    Some(name) => self.party.parties.invite(slot, &name, now),
                    None => {
                        if self.party.parties.allow(slot, now) {
                            ctx.out.to(slot, chat("[PARTY]", "Nobody is near you to invite. Say whom: /invite NAME"));
                        }
                    }
                },
                Some(Cmd::Invite(n)) => self.party.parties.invite(slot, n, now),
                Some(Cmd::Accept(n)) => self.party.parties.accept(slot, n, now),
                Some(Cmd::Decline(n)) => self.party.parties.decline(slot, n, now),
                Some(Cmd::Leave) => self.party.parties.leave(slot, now),
                Some(Cmd::Kick(n)) => self.party.parties.kick(slot, n, now),
                Some(Cmd::Disband) => self.party.parties.disband(slot, now),
                Some(Cmd::Promote(n)) => self.party.parties.promote(slot, n, now),
                Some(Cmd::Say(t)) => self.party_chat(ctx, slot, t),
                Some(Cmd::Help) => {
                    if self.party.parties.allow(slot, now) {
                        for line in HELP {
                            ctx.out.to(slot, chat("[PARTY]", line));
                        }
                    }
                }
                None => {
                    if self.party.parties.allow(slot, now) {
                        ctx.out.to(slot, chat("[PARTY]", "No such command. Type /help."));
                    }
                }
            }
            self.flush_party(ctx);
            return true;
        }
        if to_party {
            self.party_chat(ctx, slot, &text);
            self.flush_party(ctx);
            return true;
        }
        false
    }

    /// The name of the player nearest to `slot` within `range` metres that is in no party (and is not `slot`): what `/invite` with no name invites.
    fn nearest_free(&self, slot: usize, range: f32) -> Option<String> {
        let world = &self.sim.world;
        let at = |s: usize| self.avatars.get(&s).and_then(|a| world.entity(a.uid)).and_then(|e| world.get::<Transform>(e)).map(|t| t.pos);
        let me = at(slot)?;
        let mut best: Option<(f32, usize)> = None;
        for &s in self.avatars.keys() {
            if s == slot || self.party.parties.party_of(s) != 0 {
                continue;
            }
            let Some(p) = at(s) else { continue };
            let d = ((p[0] - me[0]).powi(2) + (p[1] - me[1]).powi(2) + (p[2] - me[2]).powi(2)).sqrt();
            if d <= range && best.is_none_or(|b| d < b.0) {
                best = Some((d, s));
            }
        }
        best.and_then(|(_, s)| self.party.parties.key_of(s).map(str::to_string))
    }

    /// A message to the speaker's party, wherever its members are: `[PARTY] Ada: text` in their logs. The chat's own limit (three at once, then one every 1.2 s) applies.
    fn party_chat(&mut self, ctx: &mut Ctx, slot: usize, text: &str) {
        // (a player that is in no party is told so, and is not told so a thousand times a second: the commands' limit)
        if !self.party.parties.allow(slot, clock(ctx)) {
            return;
        }
        let Some(audience) = self.party.parties.audience(slot) else { return };
        let Some(av) = self.avatars.get_mut(&slot) else { return };
        let since = ctx.tick.wrapping_sub(av.chat_tick) as f32 / ctx.tick_hz.max(1) as f32;
        av.chat_credit = (av.chat_credit + since / 1.2).min(3.0);
        av.chat_tick = ctx.tick;
        if av.chat_credit < 1.0 {
            return;
        }
        av.chat_credit -= 1.0;
        let line = chat(&format!("[PARTY] {}", clean_name(&av.name)), text);
        for s in audience {
            ctx.out.to(s, line.clone());
        }
    }

    /// The commands a game's interface can say by number instead of by typing (`NetPlayer.cmd`): `party_invite` and `party_kick` and `party_promote` with the server's number of the player (`NetPuppet.id`, the panel's rows) in
    /// the first argument, `party_accept` and `party_decline` (the newest invitation, or the one from the player with that number), `party_leave` and `party_disband`.
    pub(super) fn party_action(&mut self, ctx: &mut Ctx, slot: usize, name: &str, args: [f32; 4]) {
        if !self.party.parties.enabled() {
            return;
        }
        let now = clock(ctx);
        let named = if args[0] >= 1.0 { self.avatars.values().find(|a| a.net != 0 && a.net == args[0] as u32).map(|a| clean_name(&a.name)) } else { None };
        let who = named.as_deref().unwrap_or("");
        match name {
            "party_invite" if named.is_some() => self.party.parties.invite(slot, who, now),
            "party_kick" if named.is_some() => self.party.parties.kick(slot, who, now),
            "party_promote" if named.is_some() => self.party.parties.promote(slot, who, now),
            "party_invite" | "party_kick" | "party_promote" => ctx.out.to(slot, chat("[PARTY]", "No such player here.")),
            "party_accept" => self.party.parties.accept(slot, who, now),
            "party_decline" => self.party.parties.decline(slot, who, now),
            "party_leave" => self.party.parties.leave(slot, now),
            "party_disband" => self.party.parties.disband(slot, now),
            _ => {}
        }
        self.flush_party(ctx);
    }

    /// Every tick: invitations run out, seats are given up, what the table says is put on the avatars, and the panels are told.
    pub(super) fn tick_parties(&mut self, ctx: &mut Ctx) {
        let now = clock(ctx);
        self.party.now = now;
        if !self.party.parties.enabled() {
            return;
        }
        if self.party.parties.idle() {
            // (nothing to look after: the notes and changes of a command are flushed by the command itself, a time only matters to an invitation or a seat)
            self.party.parties.tick(now);
            self.flush_party(ctx);
            return;
        }
        let t = Instant::now();
        self.party.parties.tick(now);
        self.flush_party(ctx);
        self.tell_panels(ctx, now);
        self.stats.party_us += t.elapsed().as_micros() as u64;
        self.stats.party_ticks += 1;
    }

    /// Tell the players what the table has to say (lines of the chat log), put its changes on the avatars, and make the list of allies again when it changed.
    fn flush_party(&mut self, ctx: &mut Ctx) {
        for (slot, text) in self.party.parties.take_notes() {
            ctx.out.to(slot, chat("[PARTY]", &text));
        }
        for slot in self.party.parties.take_changed() {
            self.party_apply(slot);
        }
        if self.party.parties.version != self.party.built || self.party.pending_net {
            self.rebuild_allies();
        }
    }

    /// The list the server's wallhack defence is given: each member that is here and has its server number, with its party.
    fn rebuild_allies(&mut self) {
        let mut list: Vec<(u32, u32)> = Vec::new();
        let mut pending = false;
        for party in self.party.parties.parties.values() {
            for slot in party.here() {
                match self.avatars.get(&slot).map(|a| a.net) {
                    Some(net) if net != 0 => list.push((net, party.id)),
                    _ => pending = true,
                }
            }
        }
        list.sort_unstable();
        self.party.pending_net = pending;
        self.party.built = self.party.parties.version;
        if list != self.party.allies {
            self.party.allies = list;
            self.party.allies_version = self.party.allies_version.wrapping_add(1);
        }
    }

    /// The panel of `slot`: its party and the invitation that waits for it, with each member's health and where it is from `slot`.
    fn build_panel(&self, slot: usize, now: u64) -> Panel {
        let ps = &self.party.parties;
        let mut panel = Panel::default();
        let Some(me) = ps.players.get(&slot) else { return panel };
        if let Some((_, from, left)) = ps.waiting(slot, now) {
            panel.invite = Some(Invitation { from, secs: left.div_ceil(1000).min(255) as u8 });
        }
        let Some(party) = ps.party(me.party) else { return panel };
        panel.party = party.id;
        panel.leader = party.leader.as_deref() == Some(me.key.as_str());
        let world = &self.sim.world;
        let at = |slot: usize| self.avatars.get(&slot).and_then(|a| world.entity(a.uid)).and_then(|e| world.get::<Transform>(e).map(|t| (e, t.pos)));
        let mine = at(slot).map(|p| p.1);
        for m in &party.members {
            let mut row = Row { name: m.key.clone(), id: 0, health: 0, dist: Row::FAR, bearing: 0, flags: 0 };
            if party.leader.as_deref() == Some(m.key.as_str()) {
                row.flags |= Row::LEADER;
            }
            match m.slot {
                None => row.flags |= Row::AWAY,
                Some(s) => {
                    if s == slot {
                        row.flags |= Row::ME;
                    }
                    row.id = self.avatars.get(&s).map_or(0, |a| a.net);
                    if let Some((e, pos)) = at(s) {
                        row.health = health_byte(world, e);
                        if world.get::<HitPoints>(e).is_some_and(|h| h.dead) {
                            row.flags |= Row::DEAD;
                        }
                        if let Some(from) = mine {
                            let d = [pos[0] - from[0], pos[1] - from[1], pos[2] - from[2]];
                            row.dist = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() + 0.5).min(65534.0) as u16;
                            row.bearing = crate::scale::party::bearing_byte(d[0], d[2]);
                        }
                    }
                }
            }
            panel.members.push(row);
        }
        panel
    }

    /// Tell each member (and each invited player) its panel: at once when something changed, otherwise at most once a second and only if it is not what it was told, and all of it again every five seconds. A player who has just
    /// left a party is told an empty one, once, so that its screen is cleared.
    fn tell_panels(&mut self, ctx: &mut Ctx, now: u64) {
        let mut slots = std::mem::take(&mut self.party.scratch);
        slots.clear();
        slots.extend(self.party.parties.players.iter().filter(|(_, p)| p.party != 0 || p.shown).map(|(s, _)| *s));
        for s in self.party.parties.invitees() {
            if !slots.contains(&s) {
                slots.push(s);
            }
        }
        for &slot in &slots {
            let Some(p) = self.party.parties.players.get(&slot) else { continue };
            if !(p.urgent || now >= p.told_at + 1000) {
                continue;
            }
            let panel = self.build_panel(slot, now);
            let msg = panel.message();
            // (the seconds an invitation has left count down on the client's own clock: a change of them alone is not news, and the refresh every five seconds sets them right)
            let h = match &panel.invite {
                Some(i) => hash(&Panel { invite: Some(Invitation { from: i.from.clone(), secs: 0 }), ..panel.clone() }.message()),
                None => hash(&msg),
            };
            let Some(p) = self.party.parties.players.get_mut(&slot) else { continue };
            p.told_at = now;
            p.urgent = false;
            if panel.is_empty() && !p.shown {
                continue;
            }
            if h != p.told || now >= p.sent_at + 5000 {
                p.told = h;
                p.sent_at = now;
                let shown = !panel.is_empty();
                if shown != p.shown {
                    p.shown = shown;
                    if shown {
                        self.party.parties.shown_n += 1;
                    } else {
                        self.party.parties.shown_n = self.party.parties.shown_n.saturating_sub(1);
                    }
                }
                ctx.out.to(slot, msg);
            }
        }
        self.party.scratch = slots;
    }
}
