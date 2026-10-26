//! Who may connect, and nobody else reading or changing the packets (docs/SECURE.md): connect tokens and an authenticated, encrypted channel, standard library only.
//!
//! * **ChaCha20-Poly1305** (RFC 8439) and **XChaCha20-Poly1305** (draft-irtf-cfrg-xchacha), written from the RFCs and checked against the RFC's own examples and against OpenSSL's
//!   implementation on twenty-seven vectors (`secure_vectors.txt`, made by `tools/gen_secure_vectors.py`): no secret-dependent branch or index anywhere, the tag is compared in constant time.
//! * A **connect token** is what a game's login service (or the server's owner, with `lunaris token`) hands a player: a block sealed under the server's key (the player's id and name,
//!   the two session keys, when it expires) that only the server can open, plus the same keys in the clear for the player. The server opens it, learns who the player is (the name in a token cannot be
//!   chosen by the player any more), and both ends talk with the session keys from then on.
//! * A **channel** seals every datagram with a counter as its nonce (one key for each direction, so nonces never repeat), refuses replays with a 128-packet window, and refuses anything
//!   that was changed, in a few hundred nanoseconds a packet.
//!
//! What this does not give: it does not hide who talks to whom or how much, and it does not keep a cheater who has a valid token from cheating inside the game (the server decides, docs/GAME_SERVER.md).

use std::fmt::Write as _;

/// The size of a key, of a tag, of the counter that opens a sealed datagram.
pub const KEY: usize = 32;
pub const TAG: usize = 16;
pub const COUNTER: usize = 8;
/// What a sealed datagram is longer than its plaintext.
pub const OVERHEAD: usize = COUNTER + TAG;

// ---------------------------------------------------------------------------------------------------------------- ChaCha20

#[inline(always)]
fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

#[inline(always)]
fn rounds(s: &mut [u32; 16]) {
    for _ in 0..10 {
        quarter(s, 0, 4, 8, 12);
        quarter(s, 1, 5, 9, 13);
        quarter(s, 2, 6, 10, 14);
        quarter(s, 3, 7, 11, 15);
        quarter(s, 0, 5, 10, 15);
        quarter(s, 1, 6, 11, 12);
        quarter(s, 2, 7, 8, 13);
        quarter(s, 3, 4, 9, 14);
    }
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn key_words(key: &[u8; KEY]) -> [u32; 8] {
    let mut k = [0u32; 8];
    for (i, w) in k.iter_mut().enumerate() {
        *w = le32(&key[i * 4..]);
    }
    k
}

const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

/// One 64-byte block of the ChaCha20 key stream.
fn block(key: &[u32; 8], counter: u32, nonce: &[u32; 3]) -> [u8; 64] {
    let mut init = [0u32; 16];
    init[..4].copy_from_slice(&SIGMA);
    init[4..12].copy_from_slice(key);
    init[12] = counter;
    init[13..16].copy_from_slice(nonce);
    let mut s = init;
    rounds(&mut s);
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[i * 4..i * 4 + 4].copy_from_slice(&s[i].wrapping_add(init[i]).to_le_bytes());
    }
    out
}

/// `data` XOR the key stream from block `counter` on.
fn xor_stream(key: &[u32; 8], mut counter: u32, nonce: &[u32; 3], data: &mut [u8]) {
    for chunk in data.chunks_mut(64) {
        let ks = block(key, counter, nonce);
        for (d, k) in chunk.iter_mut().zip(ks.iter()) {
            *d ^= k;
        }
        counter = counter.wrapping_add(1);
    }
}

/// HChaCha20: a key and the first 16 bytes of a 24-byte nonce make the subkey of XChaCha20.
fn hchacha20(key: &[u8; KEY], nonce: &[u8]) -> [u8; KEY] {
    let k = key_words(key);
    let mut s = [0u32; 16];
    s[..4].copy_from_slice(&SIGMA);
    s[4..12].copy_from_slice(&k);
    for i in 0..4 {
        s[12 + i] = le32(&nonce[i * 4..]);
    }
    rounds(&mut s);
    let mut out = [0u8; KEY];
    for (i, w) in [s[0], s[1], s[2], s[3], s[12], s[13], s[14], s[15]].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------- Poly1305

/// Poly1305 with 26-bit limbs (the arithmetic of poly1305-donna): `h = (h + block) * r mod 2^130 - 5`, no secret-dependent branches.
struct Poly1305 {
    r: [u32; 5],
    h: [u32; 5],
    pad: [u32; 4],
    buf: [u8; 16],
    left: usize,
}

impl Poly1305 {
    fn new(key: &[u8; 32]) -> Poly1305 {
        let r = [
            le32(&key[0..]) & 0x03ff_ffff,
            (le32(&key[3..]) >> 2) & 0x03ff_ff03,
            (le32(&key[6..]) >> 4) & 0x03ff_c0ff,
            (le32(&key[9..]) >> 6) & 0x03f0_3fff,
            (le32(&key[12..]) >> 8) & 0x000f_ffff,
        ];
        let pad = [le32(&key[16..]), le32(&key[20..]), le32(&key[24..]), le32(&key[28..])];
        Poly1305 { r, h: [0; 5], pad, buf: [0; 16], left: 0 }
    }

    /// Absorb 16-byte blocks (`hibit` 1 << 24 for a full block, 0 for the last, padded one).
    fn blocks(&mut self, m: &[u8], hibit: u32) {
        let [r0, r1, r2, r3, r4] = self.r;
        let (s1, s2, s3, s4) = (r1 * 5, r2 * 5, r3 * 5, r4 * 5);
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.h;
        for chunk in m.chunks_exact(16) {
            h0 += le32(&chunk[0..]) & 0x03ff_ffff;
            h1 += (le32(&chunk[3..]) >> 2) & 0x03ff_ffff;
            h2 += (le32(&chunk[6..]) >> 4) & 0x03ff_ffff;
            h3 += (le32(&chunk[9..]) >> 6) & 0x03ff_ffff;
            h4 += (le32(&chunk[12..]) >> 8) | hibit;
            let d0 = h0 as u64 * r0 as u64 + h1 as u64 * s4 as u64 + h2 as u64 * s3 as u64 + h3 as u64 * s2 as u64 + h4 as u64 * s1 as u64;
            let mut d1 = h0 as u64 * r1 as u64 + h1 as u64 * r0 as u64 + h2 as u64 * s4 as u64 + h3 as u64 * s3 as u64 + h4 as u64 * s2 as u64;
            let mut d2 = h0 as u64 * r2 as u64 + h1 as u64 * r1 as u64 + h2 as u64 * r0 as u64 + h3 as u64 * s4 as u64 + h4 as u64 * s3 as u64;
            let mut d3 = h0 as u64 * r3 as u64 + h1 as u64 * r2 as u64 + h2 as u64 * r1 as u64 + h3 as u64 * r0 as u64 + h4 as u64 * s4 as u64;
            let mut d4 = h0 as u64 * r4 as u64 + h1 as u64 * r3 as u64 + h2 as u64 * r2 as u64 + h3 as u64 * r1 as u64 + h4 as u64 * r0 as u64;
            let mut c = d0 >> 26;
            h0 = (d0 & 0x03ff_ffff) as u32;
            d1 += c;
            c = d1 >> 26;
            h1 = (d1 & 0x03ff_ffff) as u32;
            d2 += c;
            c = d2 >> 26;
            h2 = (d2 & 0x03ff_ffff) as u32;
            d3 += c;
            c = d3 >> 26;
            h3 = (d3 & 0x03ff_ffff) as u32;
            d4 += c;
            c = d4 >> 26;
            h4 = (d4 & 0x03ff_ffff) as u32;
            h0 += (c * 5) as u32;
            let c2 = h0 >> 26;
            h0 &= 0x03ff_ffff;
            h1 += c2;
        }
        self.h = [h0, h1, h2, h3, h4];
    }

    fn update(&mut self, mut m: &[u8]) {
        if self.left > 0 {
            let want = 16 - self.left;
            let take = want.min(m.len());
            self.buf[self.left..self.left + take].copy_from_slice(&m[..take]);
            self.left += take;
            m = &m[take..];
            if self.left < 16 {
                return;
            }
            let b = self.buf;
            self.blocks(&b, 1 << 24);
            self.left = 0;
        }
        let whole = m.len() & !15;
        if whole > 0 {
            self.blocks(&m[..whole], 1 << 24);
            m = &m[whole..];
        }
        if !m.is_empty() {
            self.buf[..m.len()].copy_from_slice(m);
            self.left = m.len();
        }
    }

    fn finish(mut self) -> [u8; TAG] {
        if self.left > 0 {
            let mut b = [0u8; 16];
            b[..self.left].copy_from_slice(&self.buf[..self.left]);
            b[self.left] = 1;
            self.blocks(&b, 0);
        }
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.h;
        // a full carry
        let mut c = h1 >> 26;
        h1 &= 0x03ff_ffff;
        h2 += c;
        c = h2 >> 26;
        h2 &= 0x03ff_ffff;
        h3 += c;
        c = h3 >> 26;
        h3 &= 0x03ff_ffff;
        h4 += c;
        c = h4 >> 26;
        h4 &= 0x03ff_ffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x03ff_ffff;
        h1 += c;
        // h + -p (that is h - (2^130 - 5)), and pick h or that, without a branch
        let mut g0 = h0.wrapping_add(5);
        c = g0 >> 26;
        g0 &= 0x03ff_ffff;
        let mut g1 = h1.wrapping_add(c);
        c = g1 >> 26;
        g1 &= 0x03ff_ffff;
        let mut g2 = h2.wrapping_add(c);
        c = g2 >> 26;
        g2 &= 0x03ff_ffff;
        let mut g3 = h3.wrapping_add(c);
        c = g3 >> 26;
        g3 &= 0x03ff_ffff;
        let g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);
        let mask = (g4 >> 31).wrapping_sub(1); // all ones if h >= p (g4 did not go negative), else 0
        g0 &= mask;
        g1 &= mask;
        g2 &= mask;
        g3 &= mask;
        let g4 = g4 & mask;
        let nmask = !mask;
        h0 = (h0 & nmask) | g0;
        h1 = (h1 & nmask) | g1;
        h2 = (h2 & nmask) | g2;
        h3 = (h3 & nmask) | g3;
        h4 = (h4 & nmask) | g4;
        // h mod 2^128, plus the pad
        let w0 = h0 | (h1 << 26);
        let w1 = (h1 >> 6) | (h2 << 20);
        let w2 = (h2 >> 12) | (h3 << 14);
        let w3 = (h3 >> 18) | (h4 << 8);
        let mut f = w0 as u64 + self.pad[0] as u64;
        let t0 = f as u32;
        f = w1 as u64 + self.pad[1] as u64 + (f >> 32);
        let t1 = f as u32;
        f = w2 as u64 + self.pad[2] as u64 + (f >> 32);
        let t2 = f as u32;
        f = w3 as u64 + self.pad[3] as u64 + (f >> 32);
        let t3 = f as u32;
        let mut tag = [0u8; TAG];
        tag[0..4].copy_from_slice(&t0.to_le_bytes());
        tag[4..8].copy_from_slice(&t1.to_le_bytes());
        tag[8..12].copy_from_slice(&t2.to_le_bytes());
        tag[12..16].copy_from_slice(&t3.to_le_bytes());
        tag
    }
}

/// `a == b` in the same time whatever they hold.
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        d |= x ^ y;
    }
    std::hint::black_box(d) == 0
}

// ---------------------------------------------------------------------------------------------------------------- the AEAD

fn nonce_words(n: &[u8; 12]) -> [u32; 3] {
    [le32(&n[0..]), le32(&n[4..]), le32(&n[8..])]
}

fn mac(key: &[u32; 8], nonce: &[u32; 3], aad: &[u8], ct: &[u8]) -> [u8; TAG] {
    let otk_block = block(key, 0, nonce);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&otk_block[..32]);
    let mut p = Poly1305::new(&otk);
    let zeros = [0u8; 16];
    p.update(aad);
    p.update(&zeros[..(16 - aad.len() % 16) % 16]);
    p.update(ct);
    p.update(&zeros[..(16 - ct.len() % 16) % 16]);
    let mut lens = [0u8; 16];
    lens[..8].copy_from_slice(&(aad.len() as u64).to_le_bytes());
    lens[8..].copy_from_slice(&(ct.len() as u64).to_le_bytes());
    p.update(&lens);
    p.finish()
}

/// Seal `buf` in place and append the tag (RFC 8439): `aad` is authenticated, not hidden.
pub fn seal_in_place(key: &[u8; KEY], nonce: &[u8; 12], aad: &[u8], buf: &mut Vec<u8>) {
    seal_from(key, nonce, aad, buf, 0);
}

/// The same for the part of `buf` from `from` on (what comes before it is left as it is).
pub fn seal_from(key: &[u8; KEY], nonce: &[u8; 12], aad: &[u8], buf: &mut Vec<u8>, from: usize) {
    let k = key_words(key);
    let n = nonce_words(nonce);
    xor_stream(&k, 1, &n, &mut buf[from..]);
    let tag = mac(&k, &n, aad, &buf[from..]);
    buf.extend_from_slice(&tag);
}

/// Open `sealed` (ciphertext and tag) in place: the plaintext, or `None` if it was not sealed with this key, nonce and `aad`.
pub fn open_in_place(key: &[u8; KEY], nonce: &[u8; 12], aad: &[u8], sealed: &mut Vec<u8>) -> Option<()> {
    if sealed.len() < TAG {
        return None;
    }
    let k = key_words(key);
    let n = nonce_words(nonce);
    let split = sealed.len() - TAG;
    let tag = mac(&k, &n, aad, &sealed[..split]);
    if !same(&tag, &sealed[split..]) {
        return None;
    }
    sealed.truncate(split);
    xor_stream(&k, 1, &n, sealed);
    Some(())
}

/// XChaCha20-Poly1305: a 24-byte nonce, which can be chosen at random without a repeat in practice.
pub fn xseal(key: &[u8; KEY], nonce: &[u8; 24], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let sub = hchacha20(key, &nonce[..16]);
    let mut n12 = [0u8; 12];
    n12[4..].copy_from_slice(&nonce[16..]);
    let mut buf = plain.to_vec();
    seal_in_place(&sub, &n12, aad, &mut buf);
    buf
}

pub fn xopen(key: &[u8; KEY], nonce: &[u8; 24], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    let sub = hchacha20(key, &nonce[..16]);
    let mut n12 = [0u8; 12];
    n12[4..].copy_from_slice(&nonce[16..]);
    let mut buf = sealed.to_vec();
    open_in_place(&sub, &n12, aad, &mut buf)?;
    Some(buf)
}

// ---------------------------------------------------------------------------------------------------------------- the channel

/// One end of an authenticated, encrypted channel: a key for each direction, a counter for what it sends and a window of what it has received.
pub struct Channel {
    tx: [u8; KEY],
    rx: [u8; KEY],
    tx_counter: u64,
    rx_top: u64,
    /// Bit `i` says that the packet numbered `rx_top - i` has been received.
    rx_seen: u128,
    got_any: bool,
}

impl Channel {
    pub fn new(tx: [u8; KEY], rx: [u8; KEY]) -> Channel {
        Channel { tx, rx, tx_counter: 0, rx_top: 0, rx_seen: 0, got_any: false }
    }

    fn nonce(counter: u64) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&counter.to_le_bytes());
        n
    }

    /// Append to `out` the counter, the sealed `plain` and the tag; `aad` (the packet's clear header) is authenticated too.
    pub fn seal(&mut self, aad: &[u8], plain: &[u8], out: &mut Vec<u8>) {
        let c = self.tx_counter;
        self.tx_counter += 1;
        out.extend_from_slice(&c.to_le_bytes());
        let at = out.len();
        out.extend_from_slice(plain);
        seal_from(&self.tx, &Self::nonce(c), aad, out, at);
    }

    /// Open what `seal` made (`sealed` starts with the counter): the plaintext, or `None` for a forgery, a change, a replay or a packet too old.
    pub fn open(&mut self, aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() < OVERHEAD {
            return None;
        }
        let mut cb = [0u8; 8];
        cb.copy_from_slice(&sealed[..8]);
        let c = u64::from_le_bytes(cb);
        // (a counter that was seen, or is more than 128 behind the newest, is refused before any work is done)
        if self.got_any {
            if c <= self.rx_top {
                let back = self.rx_top - c;
                if back >= 128 || (self.rx_seen >> back) & 1 == 1 {
                    return None;
                }
            }
        }
        let mut buf = sealed[8..].to_vec();
        open_in_place(&self.rx, &Self::nonce(c), aad, &mut buf)?;
        // (only a packet that opened moves the window)
        if !self.got_any || c > self.rx_top {
            let shift = if self.got_any { c - self.rx_top } else { 0 };
            self.rx_seen = if shift >= 128 { 0 } else { self.rx_seen << shift };
            self.rx_seen |= 1;
            self.rx_top = c;
            self.got_any = true;
        } else {
            self.rx_seen |= 1u128 << (self.rx_top - c);
        }
        Some(buf)
    }
}

// ---------------------------------------------------------------------------------------------------------------- tokens

const TOKEN_TAG: &[u8] = b"LUNARIS-TOKEN-1";

/// The most a name in a token may be.
pub const MAX_NAME: usize = 32;

/// What a token tells the server about a player (once opened).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claims {
    pub client_id: u64,
    pub name: String,
    /// The key the player seals what it sends with, and the key the server seals what it sends the player with.
    pub client_to_server: [u8; KEY],
    pub server_to_client: [u8; KEY],
}

/// A connect token as the player holds it (and sends the server): `private` is sealed under the server's key, the keys are the player's copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectToken {
    pub protocol_id: u64,
    /// Seconds since 1970, after which the server refuses it.
    pub expire: u64,
    pub nonce: [u8; 24],
    pub private: Vec<u8>,
    pub client_to_server: [u8; KEY],
    pub server_to_client: [u8; KEY],
    /// The name the token was made for (for the player's own screen; the server uses the one it opens).
    pub name: String,
}

fn token_aad(protocol_id: u64, expire: u64) -> Vec<u8> {
    let mut a = TOKEN_TAG.to_vec();
    a.extend_from_slice(&protocol_id.to_le_bytes());
    a.extend_from_slice(&expire.to_le_bytes());
    a
}

impl ConnectToken {
    /// Make a token (what a login service does): `nonce`, `client_to_server` and `server_to_client` must be fresh random bytes ([`random_bytes`]).
    #[allow(clippy::too_many_arguments)]
    pub fn issue(server_key: &[u8; KEY], protocol_id: u64, client_id: u64, name: &str, now_unix: u64, ttl_s: u64, nonce: [u8; 24], client_to_server: [u8; KEY], server_to_client: [u8; KEY]) -> ConnectToken {
        let expire = now_unix.saturating_add(ttl_s);
        // (at most MAX_NAME bytes, not characters, and never half a character: a token that `open_token` would refuse for the length of its name is no token)
        let mut name: String = name.chars().take(MAX_NAME).collect();
        while name.len() > MAX_NAME {
            name.pop();
        }
        let mut plain = Vec::with_capacity(8 + 1 + name.len() + 64);
        plain.extend_from_slice(&client_id.to_le_bytes());
        plain.push(name.len() as u8);
        plain.extend_from_slice(name.as_bytes());
        plain.extend_from_slice(&client_to_server);
        plain.extend_from_slice(&server_to_client);
        let private = xseal(server_key, &nonce, &token_aad(protocol_id, expire), &plain);
        ConnectToken { protocol_id, expire, nonce, private, client_to_server, server_to_client, name }
    }

    /// The token as text to hand a player: hexadecimal.
    pub fn to_hex(&self) -> String {
        let mut b = Vec::new();
        b.extend_from_slice(TOKEN_TAG);
        b.extend_from_slice(&self.protocol_id.to_le_bytes());
        b.extend_from_slice(&self.expire.to_le_bytes());
        b.extend_from_slice(&self.nonce);
        b.extend_from_slice(&(self.private.len() as u16).to_le_bytes());
        b.extend_from_slice(&self.private);
        b.extend_from_slice(&self.client_to_server);
        b.extend_from_slice(&self.server_to_client);
        b.push(self.name.len() as u8);
        b.extend_from_slice(self.name.as_bytes());
        hex(&b)
    }

    pub fn from_hex(text: &str) -> Option<ConnectToken> {
        let b = unhex(text.trim())?;
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = b.get(at..at + n)?;
            at += n;
            Some(s)
        };
        if take(TOKEN_TAG.len())? != TOKEN_TAG {
            return None;
        }
        let protocol_id = u64::from_le_bytes(take(8)?.try_into().ok()?);
        let expire = u64::from_le_bytes(take(8)?.try_into().ok()?);
        let nonce: [u8; 24] = take(24)?.try_into().ok()?;
        let plen = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let private = take(plen)?.to_vec();
        let client_to_server: [u8; KEY] = take(KEY)?.try_into().ok()?;
        let server_to_client: [u8; KEY] = take(KEY)?.try_into().ok()?;
        let nlen = *take(1)?.first()? as usize;
        let name = String::from_utf8(take(nlen)?.to_vec()).ok()?;
        Some(ConnectToken { protocol_id, expire, nonce, private, client_to_server, server_to_client, name })
    }

    /// The channel the player talks to the server through.
    pub fn channel(&self) -> Channel {
        Channel::new(self.client_to_server, self.server_to_client)
    }
}

/// Open the sealed part of a token (what the server does): the claims, or `None` if it was not sealed with `server_key` for this `protocol_id` and `expire`, or it was changed.
pub fn open_token(server_key: &[u8; KEY], protocol_id: u64, expire: u64, nonce: &[u8; 24], private: &[u8]) -> Option<Claims> {
    let plain = xopen(server_key, nonce, &token_aad(protocol_id, expire), private)?;
    if plain.len() < 8 + 1 + 64 {
        return None;
    }
    let client_id = u64::from_le_bytes(plain[..8].try_into().ok()?);
    let nlen = plain[8] as usize;
    if nlen > MAX_NAME || plain.len() != 9 + nlen + 64 {
        return None;
    }
    let name = String::from_utf8(plain[9..9 + nlen].to_vec()).ok()?;
    let client_to_server: [u8; KEY] = plain[9 + nlen..9 + nlen + KEY].try_into().ok()?;
    let server_to_client: [u8; KEY] = plain[9 + nlen + KEY..].try_into().ok()?;
    Some(Claims { client_id, name, client_to_server, server_to_client })
}

/// The server's side of a token that was just opened: a channel to talk to that player through.
pub fn server_channel(c: &Claims) -> Channel {
    Channel::new(c.server_to_client, c.client_to_server)
}

pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        let _ = write!(s, "{x:02x}");
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

/// Seconds since 1970 by the computer's clock.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Bytes from the operating system's random number generator (the keys and nonces of tokens come from here). Panics only if the system has none.
pub fn random_bytes(buf: &mut [u8]) {
    os_random(buf).expect("the operating system's random number generator");
}

#[cfg(windows)]
fn os_random(buf: &mut [u8]) -> Result<(), ()> {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(algorithm: *mut std::ffi::c_void, buffer: *mut u8, length: u32, flags: u32) -> i32;
    }
    // (BCRYPT_USE_SYSTEM_PREFERRED_RNG: no algorithm handle)
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, 2) };
    if status == 0 {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(not(windows))]
fn os_random(buf: &mut [u8]) -> Result<(), ()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).map_err(|_| ())
}
