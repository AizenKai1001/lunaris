# Server robustness: what was thrown at the game server, what broke, what holds (2026-10-24)

The game server (`lunaris server`, `crates/lunaris-net/src/scale/`, `NETWORK_SCALE.md`, `SECURE.md`, `ADMIN.md`) was measured fast and never attacked. This is the record of attacking it: hostile input,
a long soak, a process killed at bad moments. Everything here is on the loopback of a Linux test machine against the real binary or in memory against the real code, never against anything else.

## The tests (all deterministic, all in `cargo test -p lunaris-net`)

`crates/lunaris-net/src/scale/fuzz_tests.rs`, 19 tests, about a quarter of a second; `LUNARIS_FUZZ_ROUNDS=N` multiplies the loops for a long run
(`cargo test --release -p lunaris-net --lib fuzz_ -- --nocapture` prints what the worst datagram cost).

| test | what it feeds the server |
|---|---|
| `fuzz_garbage_datagrams_never_panic_and_each_costs_little` | 40,000 datagrams: noise up to 2 KiB, the right first bytes and noise, data packets for a real connection from a stranger (a stolen session number), a real player's own address with soup for messages, handshakes whose lengths lie |
| `fuzz_real_packets_with_bits_flipped_cut_short_and_doubled_never_panic` | what three real players and the server send to each other, mutated 60 ways each (bit flips, cuts, doubling, huge lengths), from the players' addresses and from strangers: 12,660 datagrams |
| `fuzz_a_flood_of_handshakes_cannot_lock_a_real_player_out` | 5,000 CONNECTs from 5,000 addresses, and 6,000 from one address on 6,000 ports, then a real player |
| `fuzz_a_client_that_never_acknowledges_...` | a client that says it is alive every 100 ms for two minutes and acknowledges nothing, while the game broadcasts every tick |
| `fuzz_positions_at_the_ends_of_the_number_range_...` | movement records at `i32::MIN` / `i32::MAX`, also from a place the game itself put at the end of the range |
| `fuzz_a_name_is_made_safe_at_the_door` | names with line breaks, `#`, controls, direction marks, broken UTF-8, nothing left after cleaning |
| `fuzz_the_admin_can_kick_and_ban_a_player_whose_name_has_a_space_in_it` | `kick Big Bob`, `ban Big Bob 2h` with a `Bob` in the game too |
| `fuzz_the_admin_command_file_survives_binary_junk_and_huge_lines` | `commands.txt` with bytes that are not text, a megabyte of comment with a two-byte letter on the 1 MiB mark, a line of two megabytes |
| `fuzz_the_ban_file_is_written_whole_or_not_at_all` | no `.tmp` left behind |
| `fuzz_a_token_for_a_name_in_any_script_is_good` | tokens for Japanese, accented, emoji names |
| `fuzz_commands_that_no_client_sends_leave_the_game_server_standing` | a real `SimGame` server: 6,000 datagrams of actions with the names of the game and invented names of any length, arguments that are NaN, infinite, negative, 1e30, a length byte that lies, chat of 4,000 bytes, messages that start with a byte the game knows |
| `fuzz_joining_and_leaving_with_names_nobody_used_before_...` | 400 joins and leaves under new names |
| `fuzz_connect_tokens_and_sealed_packets_of_garbage_...` | a server that wants connect tokens: 20,000 tokens with bits flipped, cut short, made-up sealed parts and sealed kinds with noise, then a real token joins and the same token from another address is refused |
| `fuzz_a_save_has_a_checksum_...`, `fuzz_a_damaged_world_save_is_kept_aside_...` (5 kinds of damage), `fuzz_two_damaged_saves_...`, `fuzz_a_half_written_temporary_file_...`, `fuzz_a_save_that_cannot_be_written_...`, `fuzz_a_save_written_over_a_damaged_one_...` | the world save file (below) |

The same tests also run in a debug build, where integer overflow panics (`cargo test -p lunaris-net --lib fuzz_`): the release server wraps, a test or a debug server panics.

Live, against the real binary and the real scene (the game's `tools/`): `net_fuzz.py` (7 phases of hostile UDP), `crash_recovery_smoke.py` (`kill -9` during saves), `soak.py` (25 minutes with 300 players coming and going), `ab_load.py` (the tick before and after).

## What broke, and what was done (each has a test that failed before the fix)

| # | what | what an attacker or the world could do | fix |
|---|---|---|---|
| 1 | The table of half-open handshakes (`pending`, 4,096) refused every newcomer when full, and entries live 10 s | one address opening 6,000 ports, or spoofed addresses (a CONNECT costs the sender nothing), kept every real player out: 410 packets a second were enough | `MAX_PENDING_PER_IP` (128: a swarm of 1,500 bots from one machine joins in a few seconds, one address cannot fill the table) in `make_room_for_handshake`; a full table drops its oldest eighth for the newcomer. Live: 2.5 million CONNECTs in 12 s from one address, the honest player (another address) got in 6 of 6 |
| 2 | `ReliableOut` of a client that never acknowledges grew for ever (every broadcast is kept, and looked through every tick) | memory and per-tick work growing with time, per such client; the client is not timed out because it sends packets | `MAX_BACKLOG` (1,024): over it the client is removed (`Stats::backlog_kicks`) |
| 3 | The admin console read `commands.txt` with `read_to_string` of the next 1 MiB | one line of bytes that are not text, or a file over 1 MiB (a two-byte letter on the cut), or a line of a megabyte with no end, stopped the console for good (it re-read the same bytes every half second and never advanced) | bytes, lossy per line, whole lines only, an over-long line is skipped to its end and said so in `output.log`; what is echoed is shortened |
| 4 | `kick`, `ban`, `unban`, `mute` took the first word as the name | a player called `Big Bob` could not be kicked or banned by name (only `ban ip`, which takes everybody behind that address) | `NetServer::player_in`: the longest name of a player in the game that the command line starts with; `ban name NAME FOR` takes a last word that starts with a digit as the time |
| 5 | Names went in as the client said them (32 bytes, anything) | line breaks and `#` in a name reach the ban file, the admin's player list, the chat; a name with `#` could not be typed at the console at all | `clean_name`: no controls, no `#`, no direction/zero-width marks, single spaces, at most 32 bytes, `player0123` when nothing is left. Chat text drops the same marks |
| 6 | `ConnectToken::issue` cut a name at 32 characters, `open_token` refuses more than 32 bytes | a token for a name of 11 or more kana (or 17 accented letters) was refused by the server: those players could not join a secure server | cut at 32 bytes on a character boundary |
| 7 | A client's action name, and the kind of a `build:KIND`, were interned in the world's string table for good | 300 messages a second of 255-byte invented names: 3,024 new strings in a test of 6,000 datagrams; the table is cloned by every save and never shrinks | a name must be 40 printable ASCII characters at most; a string that is not in the table is added only while the network has added fewer than `MAX_MINTED` (256) in all; the scene's own strings are found without adding |
| 8 | A save that could not be written was counted as a save and not reported; a panic in the save thread was thrown away (`let _ = h.join()`) | a full disk: the server says "saves 23" and has none | `SimStats::saves_failed`, a line on the standard error |
| 9 | A world save that does not parse was silently ignored and the world started fresh; the next save overwrote it | one bad byte lost the world (the supervisor's backups helped only under the supervisor) | see "The world save" |
| 10 | `on_state`: `s.pos[a] - old[a]` and `p.abs()` on 32-bit numbers | in a debug build a movement record with `i32::MIN` panics; in release it wraps (found no way to get a bad position through, but it is not code to rely on) | differences in 64 bits, `unsigned_abs` |
| 11 | `bans.txt` was written in place, and read with `read_to_string` | a crash in the middle left half a ban list; a ban file with a byte that is not text meant no bans at all | written beside and renamed; read lossily (the lines round the junk count) |

## The world save (`NetHost.save`, `SimGame::write_save`, `SimGame::read_save`)

* The text of a save ends with a member `"checksum": "<16 hex digits>"`, an FNV-1a hash of all the text before it (`seal_save`, `check_save`). It is still JSON: a reader that does not know the member skips it,
  the supervisor (`ops/supervisor.py`) checks it too. A save without one (older server) is read as it is, and the server says so.
* Writing: the text goes to `world.tmp` and to the disk (`sync_all`), then the save that is there is kept as `world.bak` (a hard link: the old file stays whole under the new name while the new one is renamed over
  `world.json`, so there is no moment without a complete save) **if it is itself whole**, otherwise it is put aside as `world.corrupt-<unixsecond>.json`; then the rename, then the folder is synced. `.tmp` is never read.
* Reading (`read_save`): `world.json`, then `world.bak`. A file that is not text, is cut short, has a wrong checksum or does not parse is damaged: it is renamed `world.corrupt-<unixsecond>.json` (kept, never read again),
  the server says what it found on its standard error, and goes on to the next; if none is whole the world starts fresh from the scene and says so. `wipe all yes` removes `.bak` too (a wiped world must not come back).
* A save that does not fit the scene (`docs/GAME_SERVER.md`) is handled as before.

## Memory (the soak, `tools/soak.py` and `rss_idle.py` of the game)

No leak of anything the game makes was found: in 22 minutes with 300 players and 20 hand-written bots joining and leaving (3,300 joins) entities, file descriptors, threads and half-open handshakes stayed flat. The server's resident memory is not flat:
it follows the size of the world save (correlation 0.96), which grows with the number of different names that have played, because a stored character is kept for ever (`MAX_STORED`, 20,000 names). A save needs a copy of the world, its tree and its text (about twenty times the file),
which the allocator keeps at its high-water mark: 200 players standing about hold 38 MB from the third minute on; the same 200 names coming and going hold 37 MB; with new names now and then and a save a minute 54 to 59 MB; with no save at all 34 MB; a save every 5 s (12 times the saves) 52 MB: the level is set by the first big save, not by how many there are.
`malloc_trim` after each save did nothing and was not kept. The save now drops the copy of the world as soon as its tree is built and the tree as soon as its text is (`write_save`), seals the text in place (`seal_save`) and checks the file it is about to keep as `.bak` in pieces (`check_save_file`); a first version that read the old file whole made the numbers worse.
Not found: where the 40 to 65 KB for each new name goes (a stored character is about 6 KB of tree). A time to live for stored characters (done 2026-10-24: `NetHost.character_days`, 30 by default, see below) puts a ceiling on it.

## What was thrown and held up (no change needed)

Random bytes, truncated and bit-flipped packets of every kind, oversize datagrams (the receive buffer is 2 KiB, the rest is cut), empty datagrams, a stolen session number from another address (`c.addr != from`),
doubled and old packets (`Acks::on_receive`, `ReliableIn`: a message is handled once), lying length varints (`R::bytes` uses `checked_add`), 2.5 million CONNECTs in 12 s, 34 MB/s of 1200-byte datagrams for 12 s, a client that
joins and sends nothing (gone after the 5 s timeout), commands with every kind of extreme number (`inv_move`, `inv_use`, `inv_drop`, `inv_craft*`, `party_*`, `build:*`; indexes are checked, floats are made finite at the
door), chat of 4,000 bytes (cut at 480, cleaned, rate-limited), sealed packets that were forged, changed or replayed (`secure_net_tests.rs`, unchanged). The worst datagram cost in the in-memory runs was 85 microseconds
(`kind and noise`), the mean about 100 nanoseconds. The server never panicked in any run.

## Not done, and limits

* **A client that sends a packet every few seconds holds a slot for ever** on a plain server (it has a connection, it is not silent). Only connect tokens (`SECURE.md`) make slots cost something.
* **Every distinct player name that joins puts one string in the world's table** (about 60 bytes with the index) and the table never shrinks: 400 joins, 400 strings. It needs a real handshake each time. A per-address join rate
  limit or a table that forgets would end it.
* **Stored characters** are kept for 20,000 names (`MAX_STORED`) and never expire: the save grows with the number of different players (3.3 MB to 6.3 MB in the 22-minute soak with 1,300 names), and the memory of a save
  (the copy of the world and its text) is about ten times the file. **Done (2026-10-24): `NetHost.character_days`** (30 by default, 0 keeps for ever): a stored character carries the time its player last left (the member `_seen` of the stored list, unix seconds, written by `capture` on leaving and at every save for the players who are in), and the server lets go the characters that are older at every save and when a save is loaded (a save of an older server, which has no times, is given the time of the load: nobody is let go for a time nobody wrote down). Test `a_character_that_nobody_comes_back_for_is_let_go_after_its_days`. The 20,000-name ceiling stays as the second line of defence. The strings of the let-go names are still in the world's table until the next restart rebuilds it; the next step would be to count them.
* **The client side is fuzzed since 2026-10-27** (`scale/client_fuzz_tests.rs`, 3 tests, half a second): a real client plays a real match against a real server through a wrapper that damages what the server sends (bits flipped, datagrams cut short, made longer, doubled, filled with 0x00/0xFF/0x7F/0x80 runs, header bytes replaced, replaced by noise, and noise from the server's address that sometimes starts like a real packet), in the handshake and in the match: a third of the datagrams damaged (64 clients, about 3,400 damaged datagrams: most clients still get in), everything damaged, and a client that is in the game when the damage begins and grows from 10 to 90 percent. Nothing panicked, in release or in a debug build where integer overflow panics; the client also looks at every other player (`sample`, `server_ms`) all the time. Found and left: a bit flipped in a `CHALLENGE` can leave a client answering a challenge that was never made, for good (on a real network the datagram's checksum keeps corruption out, and a hostile server can keep a client out by saying nothing anyway). `lerp` in `state.rs` subtracts 32-bit positions (covered by the debug-build run).
* `DemoGame` (the demo match) was read, not fuzzed.
* Secure mode got the same garbage as plain mode through the in-memory tests only for the handshake kinds; the sealed data path is covered by `secure_net_tests.rs`.

## One address, few slots (2026-10-24)
`ServerConfig::max_per_ip` (the command line `--max-per-ip N`; 0, the default, is no limit: the swarm of test bots comes from one address) caps the players one IP address may have in at once; the next is refused in the response step with `REFUSED_ADDRESS` (7), after the ban check and before the name check. Closes the open item `a client that holds slots` of the first robustness report for the case of many sockets from one machine (a single socket that sends one packet every few seconds still holds its slot until the timeout logic of the supervisor, see the report). Test `one_address_may_hold_only_as_many_slots_as_the_server_allows`.

