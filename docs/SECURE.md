# Secure play: who may join, and nobody reading or changing the packets

A game server (`lunaris server`, docs/GAME_SERVER.md and docs/NETWORK_SCALE.md) is open to anybody who can send it a datagram, and what it believes of a player is the name the player gives. For a game that is
played on the internet that is not enough, so a server can be run **secure**: only a player with a **connect token** gets in, the name the server uses is the one in the token (the player cannot choose it), and
everything the player and the server send each other after that is **sealed**: authenticated (it cannot be forged, changed or sent again) and encrypted (nobody on the way reads it).

```
lunaris token --gen-key server.key                                   # once: the key the server and whatever makes tokens share (64 hex digits; never overwritten)
lunaris server --scene scenes/game.lunaris.json --secure server.key --protocol-id 4242 --bind 0.0.0.0
lunaris token --key server.key --name Ada --protocol-id 4242 --ttl 300   # prints a token (a long line of hex): hand it to Ada over a safe road (your login service, HTTPS)
lunaris play scenes/game.lunaris.json --connect HOST:7777 --connect-token TOKEN     # or --connect-token @ada.token
```

## What is in it

- **ChaCha20-Poly1305** (RFC 8439) and **XChaCha20-Poly1305** (draft-irtf-cfrg-xchacha), in `crates/lunaris-net/src/scale/secure.rs`, **standard library only**: no dependency, no unsafe except the one call into the operating
  system's random number generator for the keys and nonces of tokens (`BCryptGenRandom` on Windows, `/dev/urandom` elsewhere). They are written from the RFCs, have no secret-dependent branch or table index, compare the tag in constant time,
  and are checked against OpenSSL's answers on twenty-seven vectors (`secure_vectors.txt`, made by `tools/gen_secure_vectors.py`; the RFC 8439 example and the XChaCha draft's are among them). Sealing a 1000-byte datagram takes about
  2 microseconds in the test build: 200 players at 30 packets a second each way is a few milliseconds of one core a second.
- A **connect token** is made with the server's key (by `lunaris token`, or by your own login service from the same recipe: the format is in `secure.rs`): a block sealed under the server's key that says who the player is (an
  id and a name) and holds two random session keys, with the same keys in the clear for the player, for one game (`--protocol-id`), until an expiry (`--ttl`, 300 seconds by default). Only the server can read the sealed block.
- The **handshake** is the one of docs/NETWORK_SCALE.md with the token in the first message (`CONNECT_TOKEN`): the server opens it (refusing a token of another key, game or age, a changed one, or one that is already in use from another
  address), answers with the usual challenge, and the player's answer, the accept, every data packet and the goodbye are sealed with the session keys (`RESPONSE_SEALED`, `ACCEPT_SEALED`, `DATA_SEALED`, `DISCONNECT_SEALED`). A spoofed address still
  never gets a slot, and a reply is never much bigger than the request.
- A sealed packet is `[magic][kind][connection][counter 8 bytes][sealed packet][tag 16 bytes]`: 24 bytes more than a plain one. The counter is the nonce, one key for each direction so no nonce is ever used twice, and a window of
  128 packets refuses any that was seen before or is too old; the header is authenticated. A forged, changed or replayed packet is dropped before the game sees it and counted (`Stats.bad_packets`).
- A player who comes back with a new token before the server has noticed that the old connection is dead takes over from it (same id): the old one is let go.
- **What a token is for the game:** the name in it is the identity (`SimGame` keeps what a player keeps, by name: that is now safe, nobody can be "Ada" without Ada's token).

## From the game's own menu

A game that goes online from its own menu (`NetConnect`, docs/UI.md) uses tokens too: `lunaris play SCENE --connect-token TOKEN` (or `@FILE`) without `--connect` hands the token to the menu, which uses it for the Connect button; or the menu has a text field whose `UiInput` uid is `NetConnect.token_input` (the player pastes the token) or text in `NetConnect.token`. A typed token wins over the one given at start. With a token the player is whoever the token was made for (the name field is ignored); a text that is no token is `status` 5 with the reason. A token is good for one connection: a login service gives a fresh one for every try.

## What it does not do

- It does not hide who talks to whom, or how much, and a player who has a good token can still cheat *inside* the game (the server decides everything it can: docs/GAME_SERVER.md). One cheat that sealing cannot stop is reading what the server rightly sends: a
  client that draws the place of every player it is told about (a wallhack). The server therefore tells a client only about the players its character could see (`NetHost.occlusion`, docs/NETWORK_SCALE.md, "Wallhacks"); loot, machines and
  sounds are not part of that.
- A token is a secret: whoever has the player's token text before the player uses it can use it (it is good for one connection and expires); hand it over safely, and keep the expiry short.
- The server's key must stay on the server and on the service that makes tokens. Changing it (a new `--gen-key`) logs everybody out and invalidates the tokens made with the old one.
- Time: a token's expiry is judged by the server's clock (seconds since 1970): the machine that makes tokens and the server must agree to within the margin of the `--ttl`.
- No key exchange with public keys (the token carries the session keys, as in netcode.io): if a game wants the player to prove they own an account without a login service, that is a service on top (it makes the token).

## Measuring it

`lunaris swarm HOST:PORT --secure server.key --protocol-id N` makes its bots' tokens itself, so a secure server can be loaded like a plain one. Measured on 2026-10-21, real UDP on one machine, release build, the server with
200 swarm bots walking and firing (the same scene as docs/GAME_SERVER.md): a tick took **4.83 ms plain and 5.19 ms secure** (of the 33 ms there is): receiving and opening 200 packets a tick cost 56 us more, sealing and sending
the 200 replies 248 us more (about 1.2 us for each 1000-byte packet), the game's own part did not change. The network grew by the 24 bytes a datagram carries: what the clients send went from 205 to 350 KB/s in all (their packets are
small), what the server sends from 5481 to 5503 KB/s. All 200 bots were connected throughout. `tools/outpost_smoke.py` runs a secure scenario on the real binaries: two players with
tokens play the outpost, a player without one never gets in, and a token used twice is refused.

## Tests

`secure_tests` (7: the crypto against OpenSSL, changed packets and wrong keys, the replay window, tokens sealed and changed and expired, random bytes), `secure_net_tests` (6, end to end in memory with latency and loss and doubled
packets: joining under the name in the token, nothing readable on the wire, plain and token clients with the wrong servers, refused tokens with their reasons, one connection per token and a player who comes back, forged and changed and
replayed packets).
