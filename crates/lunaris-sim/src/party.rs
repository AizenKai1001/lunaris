//! Parties, as far as the simulation knows them (docs/PARTIES.md). A game server's party system (`lunaris-net`, `scale::party`) keeps who is in which party and puts the **party's number on its members' avatars as their
//! `Team.id`**, from [`PARTY_TEAM_BASE`] up, so that everything that already knows teams treats a party as one side: the weapons never hurt a member of the shooter's own team (docs/COMBAT.md), a machine the goggles took
//! joins its hacker's party, and anything new (explosives, traps, a lock that opens for friends) asks [`same_party`].
//!
//! Numbers below [`PARTY_TEAM_BASE`] are the game's own: 0 is no team (a player of a free-for-all, a barrel), an example game's machines are team 2. A game with more teams of its own keeps them under 100.
use crate::components::Team;
use lunaris_ecs::World;

/// The first `Team.id` a party gets. A party's number is its team id.
pub const PARTY_TEAM_BASE: u32 = 100;

/// The party the entity `uid` is in: its `Team.id` when that is a party's (from [`PARTY_TEAM_BASE`] up), 0 for none (no `Team`, a game's own team, or no such entity).
pub fn party_of(world: &World, uid: u64) -> u32 {
    let Some(e) = world.entity(uid) else { return 0 };
    world.get::<Team>(e).map_or(0, |t| if t.id >= PARTY_TEAM_BASE { t.id } else { 0 })
}

/// Are the entities `a_uid` and `b_uid` in the same party? False when either is in none (two players of no party are not friends), and for an entity and itself that is in none. An entity is in the same party as itself if it has one.
/// Cheap: two lookups.
pub fn same_party(world: &World, a_uid: u64, b_uid: u64) -> bool {
    let a = party_of(world, a_uid);
    a != 0 && a == party_of(world, b_uid)
}
