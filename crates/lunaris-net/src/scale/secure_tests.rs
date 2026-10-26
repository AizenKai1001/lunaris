//! The crypto: ChaCha20-Poly1305 and XChaCha20-Poly1305 against OpenSSL's answers (twenty-seven vectors, with the RFC 8439 example and the XChaCha draft's among them), a forged or changed or replayed
//! packet refused, a token that is sealed, opened, expired, changed, for another game or another key.
use super::secure::*;

fn vectors() -> Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
    let text = include_str!("secure_vectors.txt");
    let field = |s: &str| if s == "-" { Vec::new() } else { unhex(s).unwrap() };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f[0].to_string(), field(f[1]), field(f[2]), field(f[3]), field(f[4]), field(f[5]))
        })
        .collect()
}

#[test]
fn the_seals_are_what_openssl_makes_and_open_again() {
    let v = vectors();
    assert_eq!(v.len(), 27);
    for (kind, key, nonce, aad, plain, sealed) in v {
        let key: [u8; 32] = key.try_into().unwrap();
        if kind == "aead" {
            let nonce: [u8; 12] = nonce.try_into().unwrap();
            let mut buf = plain.clone();
            seal_in_place(&key, &nonce, &aad, &mut buf);
            assert_eq!(buf, sealed, "aead of {} bytes", plain.len());
            let mut back = sealed.clone();
            assert!(open_in_place(&key, &nonce, &aad, &mut back).is_some());
            assert_eq!(back, plain);
        } else {
            let nonce: [u8; 24] = nonce.try_into().unwrap();
            assert_eq!(xseal(&key, &nonce, &aad, &plain), sealed, "xaead of {} bytes", plain.len());
            assert_eq!(xopen(&key, &nonce, &aad, &sealed), Some(plain));
        }
    }
}

#[test]
fn a_changed_packet_a_wrong_key_nonce_or_header_and_a_short_one_are_refused() {
    let key = [7u8; 32];
    let nonce = [9u8; 12];
    let mut sealed = b"attack at dawn".to_vec();
    seal_in_place(&key, &nonce, b"header", &mut sealed);
    for i in 0..sealed.len() {
        for bit in [0x01u8, 0x80] {
            let mut bad = sealed.clone();
            bad[i] ^= bit;
            assert!(open_in_place(&key, &nonce, b"header", &mut bad).is_none(), "a flipped bit in byte {i} went through");
        }
    }
    let mut a = sealed.clone();
    assert!(open_in_place(&[8u8; 32], &nonce, b"header", &mut a).is_none(), "another key");
    let mut a = sealed.clone();
    assert!(open_in_place(&key, &[1u8; 12], b"header", &mut a).is_none(), "another nonce");
    let mut a = sealed.clone();
    assert!(open_in_place(&key, &nonce, b"headeR", &mut a).is_none(), "another header");
    let mut a = sealed[..10].to_vec();
    assert!(open_in_place(&key, &nonce, b"header", &mut a).is_none(), "cut short");
    let mut a = Vec::new();
    assert!(open_in_place(&key, &nonce, b"header", &mut a).is_none(), "nothing");
    let mut ok = sealed.clone();
    assert!(open_in_place(&key, &nonce, b"header", &mut ok).is_some());
    assert_eq!(ok, b"attack at dawn");
}

#[test]
fn a_channel_seals_both_ways_refuses_replays_old_packets_and_forgeries() {
    let (a, b) = ([1u8; 32], [2u8; 32]);
    let mut client = Channel::new(a, b);
    let mut server = Channel::new(b, a);
    let mut sent = Vec::new();
    for i in 0..300u32 {
        let mut out = Vec::new();
        client.seal(b"hdr", &i.to_le_bytes(), &mut out);
        assert_eq!(out.len(), 4 + OVERHEAD);
        sent.push(out);
    }
    // in order
    for (i, p) in sent.iter().enumerate().take(100) {
        assert_eq!(server.open(b"hdr", p), Some((i as u32).to_le_bytes().to_vec()));
    }
    // the same again: replays
    assert!(server.open(b"hdr", &sent[99]).is_none(), "the newest again");
    assert!(server.open(b"hdr", &sent[50]).is_none(), "an old one again");
    // a jump ahead, then the ones in between are still welcome once, and the ones 128 behind are not
    assert!(server.open(b"hdr", &sent[200]).is_some());
    assert!(server.open(b"hdr", &sent[150]).is_some(), "late but inside the window");
    assert!(server.open(b"hdr", &sent[150]).is_none(), "and only once");
    assert!(server.open(b"hdr", &sent[250]).is_some(), "the newest so far");
    assert!(server.open(b"hdr", &sent[110]).is_none(), "more than 128 behind");
    assert!(server.open(b"hdr", &sent[123]).is_some(), "127 behind: just inside the window");
    assert!(server.open(b"hdr", &sent[122]).is_none(), "128 behind: one too far");
    // a change, the wrong header, the other direction's key
    let mut bad = sent[280].clone();
    bad[12] ^= 1;
    assert!(server.open(b"hdr", &bad).is_none());
    assert!(server.open(b"HDR", &sent[280]).is_none());
    assert!(client.open(b"hdr", &sent[280]).is_none(), "the client cannot open what the client sealed");
    // a forgery does not move the window: the real packet is still accepted
    assert!(server.open(b"hdr", &sent[280]).is_some());
    // and the other way
    let mut out = Vec::new();
    server.seal(b"x", b"hello", &mut out);
    assert_eq!(client.open(b"x", &out), Some(b"hello".to_vec()));
}

fn server_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = (i * 7 + 3) as u8;
    }
    k
}

fn token(name: &str, now: u64, ttl: u64) -> ConnectToken {
    ConnectToken::issue(&server_key(), 77, 12345, name, now, ttl, [5u8; 24], [6u8; 32], [7u8; 32])
}

#[test]
fn a_token_is_sealed_for_the_server_and_says_who_the_player_is_and_which_keys_they_share() {
    let t = token("Ada", 1000, 60);
    assert_eq!((t.protocol_id, t.expire), (77, 1060));
    assert!(!t.private.windows(3).any(|w| w == b"Ada"), "the name is not in the sealed block in the clear");
    let c = open_token(&server_key(), 77, t.expire, &t.nonce, &t.private).expect("the server opens it");
    assert_eq!((c.client_id, c.name.as_str(), c.client_to_server, c.server_to_client), (12345, "Ada", [6u8; 32], [7u8; 32]));
    // the player and the server end up with the two ends of one channel
    let mut p = t.channel();
    let mut s = server_channel(&c);
    let mut out = Vec::new();
    p.seal(b"", b"from the player", &mut out);
    assert_eq!(s.open(b"", &out), Some(b"from the player".to_vec()));
    let mut out = Vec::new();
    s.seal(b"", b"from the server", &mut out);
    assert_eq!(p.open(b"", &out), Some(b"from the server".to_vec()));
}

#[test]
fn a_token_for_another_key_game_or_expiry_or_with_a_change_is_not_opened_and_the_text_form_round_trips() {
    let t = token("Ada", 1000, 60);
    assert!(open_token(&[0u8; 32], 77, t.expire, &t.nonce, &t.private).is_none(), "another server's key");
    assert!(open_token(&server_key(), 78, t.expire, &t.nonce, &t.private).is_none(), "another game");
    assert!(open_token(&server_key(), 77, t.expire + 1, &t.nonce, &t.private).is_none(), "a later expiry than the one it was made with");
    let mut n = t.nonce;
    n[0] ^= 1;
    assert!(open_token(&server_key(), 77, t.expire, &n, &t.private).is_none(), "another nonce");
    for i in 0..t.private.len() {
        let mut p = t.private.clone();
        p[i] ^= 0x10;
        assert!(open_token(&server_key(), 77, t.expire, &t.nonce, &p).is_none(), "a change at {i}");
    }
    let text = t.to_hex();
    assert_eq!(ConnectToken::from_hex(&text), Some(t.clone()));
    assert_eq!(ConnectToken::from_hex(&format!("  {text}\n")), Some(t.clone()), "spaces round it do not matter");
    assert!(ConnectToken::from_hex(&text[..text.len() - 2]).is_none(), "cut short");
    assert!(ConnectToken::from_hex("zz").is_none() && ConnectToken::from_hex("").is_none());
    // a name longer than the most is cut
    let long = token(&"x".repeat(80), 0, 1);
    assert_eq!(long.name.len(), MAX_NAME);
    assert_eq!(open_token(&server_key(), 77, long.expire, &long.nonce, &long.private).unwrap().name.len(), MAX_NAME);
}

#[test]
fn random_bytes_differ_each_time() {
    let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
    random_bytes(&mut a);
    random_bytes(&mut b);
    assert_ne!(a, b);
    assert_ne!(a, [0u8; 32]);
}

#[test]
fn sealing_costs_what_the_docs_say() {
    // not a test of speed that can fail: the figure is printed with `cargo test -- --nocapture` and quoted in docs/SECURE.md
    let key = [3u8; 32];
    let mut ch = Channel::new(key, key);
    let data = vec![0x5au8; 1000];
    let n = 20_000;
    let t = std::time::Instant::now();
    let mut out = Vec::with_capacity(1100);
    for _ in 0..n {
        out.clear();
        ch.seal(b"hdr!", &data, &mut out);
    }
    let per = t.elapsed().as_secs_f64() / n as f64;
    eprintln!("sealing a 1000-byte datagram: {:.2} us ({:.0} MB/s)", per * 1e6, 1000.0 / per / 1e6);
}
