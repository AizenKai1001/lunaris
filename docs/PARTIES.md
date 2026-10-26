# Parties: friends who group up on a game server (2026-10-23)

A survival game where players fight each other needs a way for friends to say "these are mine": they do not hurt each other, talk to each other wherever they are, know where the others are and how they are, get up at each other's beds,
build on each other's land and are not hidden from each other by walls. The server decides all of it and a client only asks. Code: `crates/lunaris-net/src/scale/party.rs` (the table: who is in which party, invitations, leaders, seats kept for a
dropped connection, what is typed, the panel message), `scale/simgame/party_glue.rs` (what `SimGame` does with it: the numbers on the avatars, the chat channel, the panels), `crates/lunaris-sim/src/party.rs` (`same_party`, what the
simulation knows), and small additions in `respawn.rs`, `building.rs`, `server.rs` and `occlusion.rs`. Tests: `party_tests.rs`, `simgame_party_tests.rs`, `lunaris-sim`'s `party_tests.rs`. A game that uses it keeps its own team plan outside this repository.

## For a scene

```json
{ "NetHost": { "player": "Avatar", "chat": 120, "party": 4, "occlusion": 16 } }
```

`NetHost.party` is the most members a party has (**4 by default**; 0 or 1 switches parties off: a text that starts with `/` is then chat as it always was). Nothing else is needed on the server. On a player's computer (the client's scene):

| What | Does |
|---|---|
| a `Text` (or `Ui`) entity with a **`NetParty`** | the panel: the engine writes the party into its text (below) and `count`, `party`, `leader`, `invited`, `invite_secs`, `heard` into the component; `lines` (8) is how many lines at most; `invite_hint` is what the game says to do about an invitation (`F3 accept   F4 decline`; empty: `/accept   /decline`); `marker` names a prefab for the **markers** over the mates (below) |
| `NetPlayer.say_party_input` = the uid of a `UiInput` | the party's chat box: what is typed in it and sent with Enter goes to the player's party only. The game opens and focuses it (a graph system) as it does the chat box (`NetPlayer.say_input`) |
| `NetPlayer.cmd` or `act` with the names below | the commands by number, for buttons and keys |

The server writes **`Team.id`** on its members' avatars (their party's number, from **100** up: an example game's machines are team 2, a free-for-all player is 0), so everything that knows teams treats a party as one side; `BuildTool.party` on the avatar's tool; and gives every avatar a `Respawn.group` (its slot plus one) when the prefab left it 0, so that the
places to get up at that a player puts down (docs/RESPAWN.md) have a number to belong to.

## What the players do

| Typed in the chat box (a text that starts with a slash is read by the server, and is never chat) | Does |
|---|---|
| `/invite NAME` | the leader asks a player in (a player with no party who invites makes a party when the other says yes). **`/invite` alone asks the nearest player with no party within 25 m** (no farther than the wallhack defence's safe radius when `NetHost.occlusion` is on: it cannot be used to find a player behind a wall), which is how a player who does not know a name asks the friend standing beside it |
| `/accept [NAME]`, `/decline [NAME]` | answer the newest invitation, or the one from that player |
| `/leave` | leave your party |
| `/kick NAME`, `/promote NAME`, `/disband` | the leader sends a member away (one that is away too), gives the party to a member who is here, or ends it |
| `/p TEXT` (or `/party TEXT`), or anything typed in the party's box | said to the party only, wherever its members are: `[PARTY] Ada: text` in their chat logs (the chat's own limit applies: three at once, then one every 1.2 s) |
| `/help` | four lines |

Names are looked for exactly as typed, then without regard to the case of plain letters, and only if that is one player: no part of a name stands for it, and two players whose names clean to the same text cannot be told apart (a command by that name is refused).
What the server says to a player about its party (an invitation, someone who left, why a command was refused) is a line of its chat log, `[PARTY]: ...`. A text that starts with a slash and is not a command is not said to anybody: `No such command. Type /help.`

The same by number (`NetPlayer.cmd` and `cmd_args`, as every command of docs/GAME_SERVER.md; the prefix `party_` is the engine's while `NetHost.party` is 2 or more, so a game's own command must not start with it): `party_invite` (`cmd_args[0]` the server's number of the player: `NetPuppet.id`, the panel's rows), `party_kick` and `party_promote` (the same), `party_accept` and `party_decline` (the newest invitation, or the one from the player with that number), `party_leave`, `party_disband`.

## What the server decides (and what the tests try)

* A party has a leader and at most `NetHost.party` members; a player is in at most one. **Nobody is put in a party without saying yes**: an invitation waits 30 s, is for "the party its inviter leads (or a new one)" when it is accepted, and is void when its inviter has gone, joined another party as a member, or the invitee joined one.
* **Only the leader invites, kicks, promotes and disbands.** A member who is not the leader is told so.
* **Limits**: an inviter may send three invitations at once and one more every 5 s (a name that is nobody's costs a token too: names cannot be tried out for free); at most three invitations wait for one player; a player who declines is left alone by that inviter for 20 s; one invitation at a time from one player to another; and every player may give eight commands at once and two a second after that (the rest are dropped without a word).
* **A party of fewer than two ends** (its last member is told); its number is free again, and numbers are handed out round and round from 100 (so a number is not used again until 60,000 others have been), which is what keeps a machine a member hacked from joining the next party that gets the number.
* **The leader going** (`/leave`) hands the party to the oldest member who is here; a leader whose connection drops does the same at once and is an ordinary member when it comes back. If every member is away the first one back leads.
* **A connection that drops keeps its seat for 60 s** (a crash, a minute without a network): the member shows as `away`, takes a place (the party is as full as it was), is told nothing (its avatar went with its connection), and the player of the same name who joins within the minute takes the seat back. After the minute the seat is given up (a party of one ends). On a plain server a name is all the identity there is, for a party's seat as for what `NetHost.persist` keeps by name: run `--secure` (docs/SECURE.md) and the name is the one in the connect token.
* Names are the chat's cleaned names (`clean_chat`: printable characters, one space between words, 24 characters): a line break or a control character in a name cannot make one that looks like another's.
* The abuse cases are tests (`party_tests.rs`, one assertion each): inviting yourself, a name nobody has, two players with one name, a player who is in a party (yours or another's), a member who invites, a full party, accepting an expired invitation (also between two ticks), accepting with none or while in a party, an invitation that waited for a player who joined another party meanwhile, the leader's commands from a member, a name that is nobody's in the party (not told apart from a member of another party), a flood of commands and of invitations, the leader leaving, a dropped connection and its seat, the same name while the seat is taken, a message cut short or with the channel byte and nothing after it.

## What the simulation sees

`lunaris_sim::party` (`PARTY_TEAM_BASE` 100, `party_of(world, uid)`, **`same_party(world, a_uid, b_uid)`**): the party of an entity is its `Team.id` when that is 100 or more. A pair is "friends" when both are in the same party; no team and no party are never friends, not even with themselves. For anything that hurts or helps in an area (explosives, traps, a lock that opens for friends): ask `same_party`.

* **Weapons** (docs/COMBAT.md): a round never hurts a member of the shooter's own team and does not stop at one (it goes on to the next thing), so friendly fire is off inside a party and on for everyone else, with nothing special in the weapon code. A machine the goggles took joins its hacker's team, so a party's hacked drone is not shot by the party.
* **Places to get up at** (docs/RESPAWN.md): a player's own places (a `SpawnPoint` of the player's `Respawn.group`) first; then, **new**, the places of the groups of its party mates (the lowest uid, like its own: a place that is `once` is used up by whoever gets up on it); then the shared places. The places are not changed when a party forms or ends: they are the owner's, and a member only uses a mate's while they are in the party together.
* **Land** (docs/BUILDING.md): `BuildTool.party` is the party of whoever holds the tool; the tool of one member is not refused (`not_yours`) inside the `BuildUpkeep` of another (the cupboard's `owner` is a number a game gives, the `owner` of the tool that put it down: the engine finds the party of that number among the tools in the world). It follows who is here: **while the owner of the land is not in the game (no tool with its number) its party mates are refused**, and land of a player who has left the party is not theirs. Pieces keep the owner that built them.

## What is told: the panel and the chat

`PANEL` (`0x4C`, to one player; `party::Panel`): flags (in a party, leads, invited), the party's number, the invitation (seconds left, who from), and for each member (at most 16) its name, the server's number of the player, health (0 dead, 255 full), distance in metres from the receiver (65535: not known), bearing (a turn in 256 steps from north, **north is -z**, east +x) and flags (leader, away, you, dead): about 14 bytes a member. The client writes `NetParty` text from it (`Panel::lines`), for example

```
PARTY (3)
* Ada   100%   you
- Bo   64%   42 m NE
- Cy   dead   1.5 km W
- Di   away
```

or, while an invitation waits, `Ada invites you to a party (24 s)` and the hint. It is told **when something changed** (a member came, went, was hurt, a leader changed, an invitation came or ran out: at once), **at most once a second** while only the distances change (members walking), **all of it again every five seconds**, and an empty one **once** to a player who left a party, so that its screen is cleared. The seconds an invitation has left are counted down on the client's own clock (they are not news by themselves: the refresh every five seconds sets them right). A player who is in no party and has no invitation is told nothing.

Chat (`NetHost.chat` and the party's are separate: parties work with proximity chat off): `SAY` (`0x05`) is `[0x05][text]` for the players near, **`[0x05][0x01][text]` for the party** (a channel byte after the kind: a text never starts with a control character, the server drops them, so every old message still means what it did; on a server with no parties the party's word is dropped, not turned into talk for the players near). The party's line reaches its members as the ordinary `CHAT` message (`0x49`) with the tag in the speaker's name, `[PARTY] Ada`, so that an old client shows it right too.

## Markers over the mates

`NetParty.marker` names a prefab: a `Text` with a `UiAnchor` (`world` is the offset above the feet: `[0, 2.25, 0]`). For each member of the panel that is not the player itself and not away, **whose puppet this client shows** (the member is within the interest radius), the engine makes one from the prefab, pins it to the puppet (`UiAnchor.entity`), writes the member's name into its `Text`, gives it the panel's `Parent` (so that it goes with the HUD) and takes it away when the member goes, leaves the party or is no longer shown (`NetDriver::mark_mates`). `UiAnchor` draws over the finished picture, so a marker shows **through walls**, which is the point: the server tells members about each other past walls (below), and the marker says where. A client with no `marker` makes none. Tests: `a_party_mate_you_can_see_gets_a_marker_pinned_to_its_puppet_and_a_stranger_does_not` (one over each mate, none for a stranger, the parent, gone when the mate is kicked or the party ends, none left in the world).

## Not hidden behind walls

With `NetHost.occlusion` on (docs/NETWORK_SCALE.md, "Wallhacks") a player behind a wall farther than the safe radius is not told to a client. **The members of a party are told to each other within the interest radius (150 m) whatever is in the way.** `Game::allies()` gives the server a list (the server's number of each member that is here, and its party) and a version; the server makes each client's list of mates only when the version changes, and `Sightings::filter_except` leaves those out of the rays. A client with no party runs exactly the code it ran before (`filter_except` with no exceptions is `filter`) plus one comparison for the whole list. It is the same function when the server makes the packets on several threads (from 24 clients on, `ServerConfig::threads`): the test `the_wall_exemption_holds_when_the_server_makes_the_packets_on_several_threads` runs the wall with 26 players on four threads. Measured: see "What it costs".

## What it costs

Measured on a Linux test machine (release build, a shared machine: numbers within about 4 percent of each other are the same number; the three runs of one configuration differ by that much), in memory, with the real survival scene and 200 players who walk about
(`SURVIVAL_SCENE=.../survival.lunaris.json cargo test --release -p lunaris-net party_ -- --ignored --nocapture`: `party_cost_with_200_players` and `party_filter_costs_a_client_with_no_party_what_it_always_did`):

* **Nothing for a player with no party.** The game's part of a server tick (`SimGame::on_tick`: the input, two steps of the simulation, what the players are told, the replication), best of three: **1847 us with parties off** (`NetHost.party` 0), **1890 us with parties on and nobody in one**, 1873 us with 100 parties of two, 1860 us with 50 parties of four (the three runs of one configuration differ by up to 2.5 percent). They are one number to the noise of the machine. With nobody in a party the party system's work in a tick is a test of one flag (`Parties::idle`), an empty `tick` and an empty `flush` (no clock is even read).
* **A party costs what it is told.** With 200 players in 100 parties of two the party part of the tick (the pass over the members, the panels that are due, the invitations and the seats) took **11 us a tick** (11.0 to 11.2), 13.5 to 13.7 us for 50 parties of four: 0.04 percent of the 33 ms a tick has. It tells **195 panels a second** (one a member: a member's panel is built at most once a second) of 30 bytes for a pair (56 for a party of four): **5.9 KB/s in all for 200 players in pairs, 11.1 KB/s in parties of four**; nothing to a player in no party. A party is a few hundred bytes of table, a connected player 150 bytes.
* **The wallhack exemption.** A client with no party runs the two `retain`s of `Sightings::filter` as before and pays one `Vec::is_empty` for the whole packet; the server makes each client's list of mates only when the game's list changes (a party formed or ended, a member's connection came or went), not every tick. The worst case for the filter, 199 candidates with every pair tested (a fresh table): **9.0 to 9.2 us a packet with no party, 9.0 to 9.1 us for a member of a party of four** (a `contains` over three numbers for each candidate it would otherwise test; the three mates cost no ray: 551 rays for 554; an earlier run on the same code read 8.7 and 8.9 to 9.0, so the difference is inside the noise). 200 players all in parties of four would cost 0.06 ms a tick at the very most.

## What is not done

* **No persistence**: a party lives as long as its players are in the game (and the minute of grace); a server restart or a player who stays away dissolves it. Clans that last are a game's table on top.
* **Markers are names only**: no distance on the marker (the panel has it), the same size at any distance, none for a mate farther than the interest radius (150 m: no puppet to pin to), and no map.
* A party's number is its `Team.id`, so a game that has teams of its own must keep them below 100.
* Land: see above (the owner of the land has to be in the game, and the owner number is the one of the tool: `slot + 1`, which a game that gives players stable numbers will want to change).
* No friends list, mute or block, and no invitation sent to a player that is not in the game.

## What to supply

A party icon (a small badge for the panel's header; 64 x 64, white on transparent so that the game can tint it), a leader's star (32 x 32), an invitation sound (short, 0.5 to 1 s, a soft double chime), a joined / left sound (a single soft tick), a party chat sound (quieter than the proximity chat's) and, if the game wants teammate markers, a diamond marker (64 x 64). None of it is made here.
