# The server's admin console (2026-10-23)

A running `lunaris server` can be told to kick, ban and mute players, announce something to everybody, save and stop, **by appending lines to a text file**. No network port, no password, no protocol: whoever can write the file is the admin, which is what an owner who has a shell, a supervisor or ssh already is. Code: `crates/lunaris-net/src/scale/admin.rs` (the console), `server.rs` (`Bans`, `NetServer::kick/players/slot_of_name/admin_game`, the ban check at the end of the handshake), `simgame.rs` (`Game::admin`: `say`, `mute`, `unmute`, `save`, `wipe`; `Game::wants_stop` is how a game ends the console's loop). Tests: `simgame_tests.rs` (4: the commands, the files, the wipe, the timed bans).

```
lunaris server --scene scenes/survival_server.lunaris.json --admin admin
echo "players"            >> admin/commands.txt
echo "say Restart in 5 minutes" >> admin/commands.txt
cat admin/output.log
```

`--admin DIR` makes the folder and watches `DIR/commands.txt` (twice a second, only complete lines; what is in the file when the server starts was run by an earlier server and is skipped). Every line is `COMMAND ARGUMENTS` (`#` starts a comment); what the server says goes to `DIR/output.log` after the line it answers (`> kick Ada` then the answer). `DIR/bans.txt` keeps the bans: read when the server starts, written whenever they change (a line `name NAME` or `ip ADDRESS`; a timed ban has `until UNIXSECOND` after it, the clock of the machine, so that it goes on running while the server is stopped; what has run out is dropped when the file is written).

| Command | Does |
|---|---|
| `help` | lists the commands |
| `players` | who is in: slot, name, address, round trip |
| `status` | players in, tick, joins, refusals, bans |
| `kick NAME [reason]` | removes the player at once; its client is told ("You were removed from the server", the refusal code 5) instead of waiting for the silence to time out |
| `ban NAME [FOR]` | bans the player's name and the address it is connected from, and removes it (code 4, "You are banned from this server"). **FOR** (`90s`, `30m`, `2h`, `7d`; added 2026-10-23) makes the ban run out; without it the ban is for good |
| `ban name NAME [FOR]` / `ban ip ADDRESS [FOR]` | bans only that name (works when nobody of that name is in) / only that address (every player behind it, and they are removed) |
| `unban NAME-OR-ADDRESS`, `bans` | lifts a ban (timed or not); lists them, a timed one with the time left |
| `say TEXT` | tells everybody, as `SERVER`, in the chat (cleaned like any chat line) |
| `mute NAME [seconds]`, `unmute NAME` | a muted player's chat is dropped (and the player is told "You are muted"); default 600 seconds |
| `save` | the world is saved now (`NetHost.save`) |
| `event`, `event NAME` | (2026-10-24, docs/EVENTS.md) with no name lists the world's events (the lines of `WorldEvents.table`: how soon each starts next, how many are going, how many started); with a name starts that event at once (its `alive` limit still counts: a supply drop that is on the ground is not made twice) |
| `wipe world yes` | **a wipe** (2026-10-23): the world starts fresh from the scene at the next start and the players keep their characters; the save is written at once with a layout no scene has (the same way a save of another layout is not loaded as a world), the saves from then on say so too, and the server **stops** so that a supervisor can start it again. Without the `yes` it only says what it would do |
| `wipe all yes` | the same and the characters go too: the save file is deleted and nothing is written again (the server stops) |
| `stop` | saves and stops the server (what Ctrl+C does) |

Names are compared without regard to case. A banned name or address is refused at the end of the handshake (before the player gets a slot) with its own reason; a client of the engine shows it in the menu through `NetConnect.message`. **A ban of a name on a plain server is weak** (a name is only what the client says: the address is kept with it by `ban NAME`; a secure server's names are the ones in the tokens: docs/SECURE.md), and an address ban catches everybody behind a shared address.

A game adds its own commands by implementing `Game::admin(ctx, cmd, args) -> Option<String>`; the server offers it every command it does not know, and what the game puts in `ctx.out` goes to the clients like any message.

Not here: a remote (network) console, roles (every writer of the file can do everything), a ban reason in the file, a kick that tells the player a reason text (the code says "removed" or "banned"), whitelists, an audit log of who typed what (the file is the log: keep it).

## Names with spaces, junk in the files (2026-10-24, docs/SERVER_ROBUSTNESS.md)

* A name may have spaces in it (players are called `Big Bob`): `kick`, `ban`, `mute` find the longest name of a player in the game that the line starts with (`kick Big Bob`, `ban Big Bob 2h`, `mute Big Bob 600`), `unban` takes the whole rest of the line, and in `ban name NAME FOR` a last word that starts with a digit is the time (`ban name Big Bob 2h`).
* A name is cleaned when the player joins (`server::clean_name`): no control characters, no `#`, no direction or zero-width marks, single spaces, 32 bytes at most; one with nothing left becomes `player0123`. So a name cannot start a second line in `bans.txt`, in the answers, or in the chat.
* `commands.txt` may hold anything: a line with bytes that are not text is read lossily, a line over a megabyte is skipped to its end (`output.log` says so), the console never stops reading. What an answer repeats of a command is cut at 40 characters. `bans.txt` is written beside and renamed over; a ban file with bytes that are not text still gives the bans on its good lines.
