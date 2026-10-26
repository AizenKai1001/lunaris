//! Parties, the table without a server (docs/PARTIES.md): invitations, leaders, seats kept for a dropped connection, the limits, the abuse cases, the panel message and what is typed. No art: names and a clock in milliseconds.
use super::party::*;

const T: u64 = 1_000;

/// A table of parties of up to `max` members with the players `names` on slots 0, 1, 2 ....
fn table(max: u32, names: &[&str]) -> Parties {
    let mut p = Parties::new(max);
    for (i, n) in names.iter().enumerate() {
        p.join(i, n, 0, T);
    }
    p
}

fn said(p: &mut Parties) -> Vec<(usize, String)> {
    p.take_notes()
}

/// What was said to one slot.
fn to(notes: &[(usize, String)], slot: usize) -> Vec<&str> {
    notes.iter().filter(|n| n.0 == slot).map(|n| n.1.as_str()).collect()
}

/// Slot `a` invites slot `b` (by name), `b` accepts: a party of the two, `a` leading. The notes are taken.
fn pair(p: &mut Parties, a: usize, b: usize, now: u64) {
    let name = p.key_of(b).unwrap().to_string();
    p.invite(a, &name, now);
    p.accept(b, "", now);
    let _ = said(p);
    let _ = p.take_changed();
}

#[test]
fn a_name_is_the_chats_cleaned_name_and_is_found_exactly_then_by_case_and_only_when_it_is_one() {
    assert_eq!(clean_name("  Ada \n Lovelace\u{7} "), "Ada Lovelace", "a line break or a control character in a name cannot make one that looks like another's");
    assert_eq!(clean_name("\u{7}\n"), "player", "a name of nothing is `player`, as in the chat");
    assert_eq!(clean_name(&"x".repeat(80)).chars().count(), NAME_CHARS);
    let names = [(0usize, "Ada"), (1, "ada"), (2, "Bo"), (3, "Cy")];
    assert_eq!(resolve(names.iter().copied(), "Ada"), Ok(0), "exactly as written wins over a name that differs by case");
    assert_eq!(resolve(names.iter().copied(), "ada"), Ok(1));
    assert_eq!(resolve(names.iter().copied(), "bO"), Ok(2), "without regard to the case of plain letters when only one is");
    assert_eq!(resolve(names.iter().copied(), "ADA"), Err(Lookup::Many), "two names differ from it only by case: nobody is guessed");
    assert_eq!(resolve(names.iter().copied(), "Zed"), Err(Lookup::None));
    assert_eq!(resolve(names.iter().copied(), "Ad"), Err(Lookup::None), "no part of a name stands for it: the wrong player must not be invited by a slip");
    assert_eq!(resolve(names.iter().copied(), "  "), Err(Lookup::None));
    let twins = [(0usize, "Bo"), (1, "Bo")];
    assert_eq!(resolve(twins.iter().copied(), "Bo"), Err(Lookup::Many), "two players whose names clean to the same text cannot be told apart");
}

#[test]
fn an_invitation_a_yes_and_a_party_with_a_leader() {
    let mut p = table(4, &["Ada", "Bo", "Cy"]);
    p.invite(0, "Bo", T);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["Invitation sent to Bo."]);
    assert!(to(&n, 1)[0].starts_with("Ada invites you to a party. Type /accept or /decline"), "{n:?}");
    assert_eq!((p.party_of(0), p.party_of(1)), (0, 0), "nobody is in a party until somebody says yes");
    assert!(p.parties.is_empty());
    let w = p.waiting(1, T + 5_000).unwrap();
    assert_eq!((w.0, w.1.as_str(), w.2), (0, "Ada", INVITE_LIFE_MS - 5_000));
    p.accept(1, "", T + 5_000);
    let id = p.party_of(0);
    assert!(id >= ID_BASE && id == p.party_of(1), "a party of the two, with a number from {ID_BASE} up (its team): {id}");
    assert_eq!(p.party_of(2), 0);
    assert!(p.leads(0) && !p.leads(1), "the one who invited leads");
    assert_eq!(p.mates(1), vec![0, 1]);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["Bo joined the party."]);
    assert_eq!(to(&n, 1), ["You joined Ada's party."]);
    assert_eq!(p.take_changed(), vec![0, 1], "the host puts the numbers on both avatars");
    assert!(p.waiting(1, T + 6_000).is_none(), "the invitation is used up");
    // the leader invites a third into the same party
    p.invite(0, "Cy", T + 6_000);
    p.accept(2, "ada", T + 7_000);
    assert_eq!(p.party_of(2), id);
    assert_eq!(p.party(id).unwrap().members.iter().map(|m| m.key.as_str()).collect::<Vec<_>>(), ["Ada", "Bo", "Cy"], "oldest first");
}

#[test]
fn nobody_is_put_in_a_party_without_saying_yes_and_an_invitation_runs_out_in_thirty_seconds() {
    let mut p = table(4, &["Ada", "Bo"]);
    p.invite(0, "Bo", T);
    let _ = said(&mut p);
    p.tick(T + INVITE_LIFE_MS - 1);
    assert!(p.waiting(1, T + INVITE_LIFE_MS - 1).is_some(), "29.999 s: still there");
    assert!(said(&mut p).is_empty());
    p.tick(T + INVITE_LIFE_MS);
    let n = said(&mut p);
    assert_eq!(to(&n, 1), ["The invitation from Ada ran out."]);
    assert_eq!(to(&n, 0), ["Your invitation to Bo ran out."]);
    p.accept(1, "", T + INVITE_LIFE_MS + 10);
    assert_eq!(to(&said(&mut p), 1), ["You have no invitation."]);
    assert_eq!(p.party_of(1), 0);
    // between two ticks the server has not looked yet: accepting at 31 s is refused too
    p.invite(0, "Bo", T + 40_000);
    let _ = said(&mut p);
    p.accept(1, "", T + 40_000 + INVITE_LIFE_MS + 1);
    assert_eq!(to(&said(&mut p), 1), ["That invitation ran out."]);
    assert_eq!((p.party_of(0), p.party_of(1)), (0, 0));
}

#[test]
fn what_a_player_may_not_do_is_refused_and_says_why() {
    // invite yourself, a name nobody has, two players with one name
    let mut p = table(4, &["Ada", "Bo", "Bo\n"]);
    p.invite(0, "Ada", T);
    p.invite(0, "Zed", T);
    p.invite(0, "bo", T);
    p.invite(0, "", T + 5_000);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["You cannot invite yourself.", "Nobody is called \"Zed\".", "More than one player is called \"bo\".", "Say whom: /invite NAME"]);
    assert!(p.parties.is_empty() && p.invitees().next().is_none());
    // accept with nothing, decline with nothing, leave with no party, kick and disband with no party
    let mut p = table(4, &["Ada", "Bo", "Cy"]);
    p.accept(0, "", T);
    p.decline(0, "", T);
    p.leave(0, T);
    p.kick(0, "Bo", T);
    p.disband(0, T);
    p.promote(0, "Bo", T);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["You have no invitation.", "You have no invitation.", "You are not in a party.", "You are not in a party.", "You are not in a party.", "You are not in a party."]);
    // an invitation to a player who is in a party, the same one or another; and a member who is not the leader invites
    pair(&mut p, 0, 1, T);
    p.invite(0, "Bo", T);
    p.invite(2, "Ada", T);
    p.invite(1, "Cy", T);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["Bo is in your party already."]);
    assert_eq!(to(&n, 2), ["Ada is in a party already."]);
    assert_eq!(to(&n, 1), ["Only the party leader can invite."]);
    assert!(p.waiting(2, T).is_none() && p.waiting(0, T).is_none(), "none of them went");
    // accepting while in a party, and an invitation that waited for a player who joined another party meanwhile
    let mut p = table(4, &["Ada", "Bo", "Cy", "Di"]);
    p.invite(2, "Bo", T);
    p.invite(0, "Bo", T);
    let _ = said(&mut p);
    p.accept(1, "Ada", T);
    let _ = said(&mut p);
    assert_eq!(p.party_of(1), p.party_of(0));
    assert!(p.waiting(1, T).is_none(), "Cy's invitation to Bo is void now that Bo is in a party");
    p.accept(1, "", T);
    assert_eq!(to(&said(&mut p), 1), ["You are in a party already. Type /leave first."]);
    // the leader's commands are the leader's
    p.kick(1, "Ada", T);
    p.promote(1, "Bo", T);
    p.disband(1, T);
    let n = said(&mut p);
    assert_eq!(to(&n, 1), ["Only the party leader can kick.", "Only the party leader can promote.", "Only the party leader can disband it."]);
    assert_eq!(p.party_of(0), p.party_of(1), "nothing happened");
    // a name that is nobody's in this party is not told apart from a player of another party: no names leak
    p.kick(0, "Cy", T);
    p.kick(0, "Zed", T);
    p.kick(0, "Ada", T);
    p.kick(0, "", T);
    p.promote(0, "Ada", T);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["\"Cy\" is not in your party.", "\"Zed\" is not in your party.", "Use /leave to go yourself.", "Say whom: /kick NAME", "You lead it already."]);
}

#[test]
fn a_party_is_as_big_as_the_host_says_and_a_seat_that_is_away_is_taken() {
    let mut p = table(2, &["Ada", "Bo", "Cy"]);
    pair(&mut p, 0, 1, T);
    p.invite(0, "Cy", T);
    assert_eq!(to(&said(&mut p), 0), ["Your party is full (2)."]);
    // a party that fills up between the invitation and the yes
    let mut p = table(3, &["Ada", "Bo", "Cy", "Di"]);
    pair(&mut p, 0, 1, T);
    p.invite(0, "Cy", T);
    p.invite(0, "Di", T);
    p.accept(2, "", T);
    let _ = said(&mut p);
    p.accept(3, "", T);
    let n = said(&mut p);
    assert_eq!(to(&n, 3), ["That party is full."]);
    assert_eq!(to(&n, 0), ["Di could not join: your party is full."]);
    assert_eq!(p.party(p.party_of(0)).unwrap().members.len(), 3);
    // a member who is away still has its seat
    let mut p = table(3, &["Ada", "Bo", "Cy", "Di"]);
    pair(&mut p, 0, 1, T);
    p.part(1, T + 1_000);
    p.invite(0, "Cy", T + 2_000);
    p.accept(2, "", T + 2_000);
    let _ = said(&mut p);
    p.invite(0, "Di", T + 3_000);
    assert_eq!(to(&said(&mut p), 0), ["Your party is full (3)."], "Ada, Cy and Bo, who is away");
    // a table for parties of one is no table: parties are off
    assert!(!Parties::new(1).enabled() && !Parties::new(0).enabled() && Parties::new(2).enabled());
    assert_eq!(Parties::new(1000).max(), MAX_MEMBERS);
}

#[test]
fn invitations_are_limited_so_that_nobody_can_be_made_to_listen() {
    // three at once and then one every five seconds for an inviter
    let mut p = table(8, &["Ada", "Bo", "Cy", "Di", "Ed"]);
    for n in ["Bo", "Cy", "Di"] {
        p.invite(0, n, T);
    }
    let _ = said(&mut p);
    p.invite(0, "Ed", T);
    assert_eq!(to(&said(&mut p), 0), ["Slow down: you can invite again in 5 s."]);
    assert!(p.waiting(4, T).is_none());
    p.invite(0, "Ed", T + 4_000);
    assert_eq!(to(&said(&mut p), 0), ["Slow down: you can invite again in 1 s."], "the token comes back at the rate it was taken");
    p.invite(0, "Ed", T + 5_000);
    assert_eq!(to(&said(&mut p), 0), ["Invitation sent to Ed."]);
    // a name that is nobody's costs a token too: names cannot be tried out for free
    let mut p = table(8, &["Ada", "Bo"]);
    for n in ["x1", "x2", "x3", "Bo"] {
        p.invite(0, n, T);
    }
    let n = said(&mut p);
    assert_eq!(to(&n, 0).last(), Some(&"Slow down: you can invite again in 5 s."), "{n:?}");
    // the same inviter twice
    let mut p = table(8, &["Ada", "Bo"]);
    p.invite(0, "Bo", T);
    p.invite(0, "Bo", T + 100);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["Invitation sent to Bo.", "You have invited Bo already."]);
    // three invitations waiting for one player, and no more
    let mut p = table(8, &["Ada", "Bo", "Cy", "Di", "Ed"]);
    for from in 0..3 {
        p.invite(from, "Ed", T);
    }
    let _ = said(&mut p);
    p.invite(3, "Ed", T);
    assert_eq!(to(&said(&mut p), 3), ["Ed has too many invitations waiting."]);
    // the newest is taken without a name, the one named otherwise
    p.accept(4, "ada", T);
    assert_eq!(p.party_of(4), p.party_of(0));
    // somebody who said no is left alone by that inviter for twenty seconds
    let mut p = table(8, &["Ada", "Bo"]);
    p.invite(0, "Bo", T);
    let _ = said(&mut p);
    p.decline(1, "", T + 1_000);
    let n = said(&mut p);
    assert_eq!(to(&n, 1), ["You declined the invitation from Ada."]);
    assert_eq!(to(&n, 0), ["Bo declined your invitation."]);
    p.invite(0, "Bo", T + 2_000);
    assert_eq!(to(&said(&mut p), 0), ["Bo is not taking invitations from you just now."]);
    p.tick(T + 1_000 + DECLINE_MS);
    p.invite(0, "Bo", T + 1_000 + DECLINE_MS);
    assert_eq!(to(&said(&mut p), 0), ["Invitation sent to Bo."]);
    // commands: eight at once and two a second after that, the rest dropped without a word
    let mut p = table(4, &["Ada"]);
    for _ in 0..20 {
        p.leave(0, T);
    }
    assert_eq!(said(&mut p).len(), 8, "a flood of commands costs the table a few comparisons and says nothing");
    assert!(!p.would_allow(0, T) && !p.would_allow(0, T + 400) && p.would_allow(0, T + 500), "a command that makes the server search first can be asked about before, without taking the token");
    assert!(p.would_allow(0, T + 500) && p.would_allow(0, T + 500), "asking takes nothing");
    for _ in 0..5 {
        p.leave(0, T + 1_000);
    }
    assert_eq!(said(&mut p).len(), 2, "and two a second after that");
}

#[test]
fn the_leader_going_hands_the_party_to_the_oldest_member_and_a_party_of_one_ends() {
    let mut p = table(4, &["Ada", "Bo", "Cy"]);
    pair(&mut p, 0, 1, T);
    p.invite(0, "Cy", T);
    p.accept(2, "", T);
    let _ = said(&mut p);
    let id = p.party_of(0);
    // the leader leaves: Bo, the oldest member, leads
    p.leave(0, T + 1_000);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["You left the party."]);
    assert_eq!(to(&n, 1), ["Ada left the party.", "Bo leads the party now."]);
    assert_eq!(to(&n, 2), ["Ada left the party.", "Bo leads the party now."]);
    assert!(p.leads(1) && !p.leads(2) && p.party_of(0) == 0);
    assert_eq!(p.party(id).unwrap().members.len(), 2);
    // the new leader leaves: one is left, and a party of one ends: Cy is told, the number is free
    p.leave(1, T + 2_000);
    let n = said(&mut p);
    assert_eq!(to(&n, 2), ["The party ended."]);
    assert_eq!((p.party_of(1), p.party_of(2)), (0, 0));
    assert!(p.parties.is_empty());
    let mut changed = p.take_changed();
    changed.sort_unstable();
    changed.dedup();
    assert_eq!(changed, vec![0, 1, 2], "every avatar that was touched is looked at again by the host");
    // a number is not used again at once: the next party gets another
    pair(&mut p, 0, 1, T + 3_000);
    assert_ne!(p.party_of(0), id, "the numbers go round and round");
    assert_eq!(p.party_of(0), ID_BASE + 1);
}

#[test]
fn kick_promote_and_disband_are_the_leaders() {
    let mut p = table(4, &["Ada", "Bo", "Cy", "Di"]);
    pair(&mut p, 0, 1, T);
    for (n, s) in [("Cy", 2), ("Di", 3)] {
        p.invite(0, n, T);
        p.accept(s, "", T);
    }
    let _ = said(&mut p);
    let _ = p.take_changed();
    // the leader sends Cy away
    p.kick(0, "cy", T + 1_000);
    let n = said(&mut p);
    assert_eq!(to(&n, 2), ["You were removed from the party."]);
    assert_eq!(to(&n, 0), ["Cy was removed from the party."]);
    assert_eq!(p.party_of(2), 0);
    assert_eq!(p.take_changed(), vec![2]);
    // the leader gives the party to Di, and Ada is a member
    p.promote(0, "Di", T + 1_000);
    assert_eq!(to(&said(&mut p), 3), ["Di leads the party now."]);
    assert!(p.leads(3) && !p.leads(0));
    p.kick(0, "Bo", T + 2_000);
    assert_eq!(to(&said(&mut p), 0), ["Only the party leader can kick."], "Ada is not the leader now");
    // the new leader ends it
    p.disband(3, T + 2_000);
    let n = said(&mut p);
    for s in [0, 1, 3] {
        assert_eq!(to(&n, s), ["The party was disbanded."], "{s}");
        assert_eq!(p.party_of(s), 0);
    }
    assert!(p.parties.is_empty());
}

#[test]
fn a_member_who_loses_the_connection_keeps_its_seat_for_a_minute_and_the_leader_passes_on_at_once() {
    let mut p = table(4, &["Ada", "Bo", "Cy"]);
    pair(&mut p, 0, 1, T);
    p.invite(0, "Cy", T);
    p.accept(2, "", T);
    let _ = said(&mut p);
    let id = p.party_of(0);
    // Bo drops: the party is told, Bo's seat stays, Bo is not among those who are here
    p.part(1, T + 10_000);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["Bo lost the connection (the seat is kept for 60 s)."]);
    assert_eq!(p.mates(0), vec![0, 2]);
    assert_eq!(p.party(id).unwrap().members.len(), 3, "an absent member counts: it takes a place");
    p.tick(T + 10_000 + GRACE_MS - 1);
    assert_eq!(p.party(id).unwrap().members.len(), 3, "59.999 s: the seat is still Bo's");
    // Bo comes back on another connection: the same party, not the leader, the host is told to put the numbers on its avatar
    let _ = p.take_changed();
    p.join(5, "Bo", 0, T + 40_000);
    assert_eq!(p.party_of(5), id);
    assert_eq!(p.take_changed(), vec![5]);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["Bo is back."]);
    assert_eq!(to(&n, 5), ["You are back in Ada's party."]);
    p.tick(T + 200_000);
    assert_eq!(p.party(id).unwrap().members.len(), 3, "it came back in time: there is nothing to give up");
    // a member who does not come back loses the seat after a minute, the party goes on with two
    p.part(5, T + 300_000);
    let _ = said(&mut p);
    p.tick(T + 300_000 + GRACE_MS);
    let n = said(&mut p);
    assert_eq!(to(&n, 0), ["Bo was dropped from the party (away too long)."]);
    assert_eq!(p.party(id).unwrap().members.len(), 2);
    // the leader drops: the oldest member who is here leads at once, and the old leader is an ordinary member when it is back
    p.part(0, T + 400_000);
    let n = said(&mut p);
    assert_eq!(to(&n, 2), ["Ada lost the connection (the seat is kept for 60 s).", "Cy leads the party now."]);
    assert!(p.leads(2));
    p.join(7, "Ada", 0, T + 410_000);
    assert!(p.party_of(7) == id && !p.leads(7) && p.leads(2), "Cy keeps the party");
    assert_eq!(to(&said(&mut p), 7), ["You are back in Cy's party."]);
    // a player with the same name that comes while the member is here does not take its seat
    p.join(8, "Cy", 0, T + 411_000);
    assert_eq!(p.party_of(8), 0, "the seat is taken by the one that sits in it");
}

#[test]
fn a_party_whose_members_are_all_away_waits_for_the_first_back_and_ends_when_the_last_seat_runs_out() {
    let mut p = table(4, &["Ada", "Bo"]);
    pair(&mut p, 0, 1, T);
    let id = p.party_of(0);
    p.part(0, T + 1_000);
    p.part(1, T + 2_000);
    assert!(p.party(id).is_some() && p.party(id).unwrap().leader.is_none(), "nobody is here: nobody leads");
    p.join(4, "Bo", 0, T + 3_000);
    assert!(p.leads(4), "the first back leads");
    assert_eq!(to(&said(&mut p), 4), ["You are back in your party and lead it."]);
    // everybody gone for good: Ada's seat runs out first, which leaves one member, and a party of one ends (Bo's own minute does not matter any more)
    p.part(4, T + 10_000);
    p.tick(T + 61_000);
    assert!(p.parties.is_empty(), "Ada's seat ran out (a minute after T + 1 s): a party of one is no party");
    p.join(9, "Bo", 0, T + 62_000);
    assert_eq!(p.party_of(9), 0, "there is nothing to come back to");
}

#[test]
fn a_connection_that_goes_takes_what_it_asked_and_was_asked_with_it() {
    let mut p = table(4, &["Ada", "Bo", "Cy"]);
    p.invite(0, "Bo", T);
    p.invite(1, "Cy", T);
    let _ = said(&mut p);
    p.part(0, T + 1_000);
    assert!(p.waiting(1, T + 1_000).is_none(), "an invitation from a player who has gone is void");
    p.part(2, T + 1_000);
    assert!(p.invitees().next().is_none());
    assert!(p.idle(), "nothing is left to look after");
    // a new player on a slot that was used does not get what was meant for the one before it
    let mut p = table(4, &["Ada", "Bo"]);
    p.invite(0, "Bo", T);
    p.part(1, T);
    p.join(1, "Zed", 0, T);
    assert!(said(&mut p).iter().all(|n| n.0 != 1), "the note for the player who left is not told to the one who came");
    assert!(p.waiting(1, T).is_none());
}

#[test]
fn the_panel_message_round_trips_and_a_cut_one_is_not_read() {
    let panel = Panel {
        party: 103,
        leader: true,
        invite: None,
        members: vec![
            Row { name: "Ada".into(), id: 7, health: 255, dist: 0, bearing: 0, flags: Row::ME | Row::LEADER },
            Row { name: "Bo ✓".into(), id: 12_345, health: 64, dist: 1500, bearing: 200, flags: 0 },
            Row { name: "Cy".into(), id: 0, health: 0, dist: Row::FAR, bearing: 0, flags: Row::AWAY },
        ],
    };
    let bytes = panel.message();
    assert!(bytes.len() < 60, "a few bytes a member: {}", bytes.len());
    assert_eq!(Panel::parse(&bytes), Some(panel.clone()));
    for cut in 0..bytes.len() {
        assert!(Panel::parse(&bytes[..cut]).is_none_or(|p| p != panel), "a message cut at {cut} is not the whole one");
    }
    assert_eq!(Panel::parse(&[0x49, 0, 0]), None, "another kind of message");
    let inv = Panel { party: 0, leader: false, invite: Some(Invitation { from: "Ada".into(), secs: 27 }), members: vec![] };
    assert_eq!(Panel::parse(&inv.message()), Some(inv.clone()));
    assert!(!inv.is_empty() && Panel::default().is_empty());
    assert_eq!(Panel::parse(&Panel::default().message()), Some(Panel::default()), "the empty panel clears a screen");
    // a name longer than a length byte holds is cut at a letter
    let long = Panel { party: 1, leader: false, invite: None, members: vec![Row { name: "é".repeat(200), id: 1, health: 1, dist: 1, bearing: 1, flags: 0 }] };
    let back = Panel::parse(&long.message()).unwrap();
    assert!(back.members[0].name.chars().all(|c| c == 'é') && back.members[0].name.len() <= 60);
    // more members than a party can have are not sent
    let many = Panel { party: 1, leader: false, invite: None, members: (0..40).map(|i| Row { name: format!("p{i}"), id: i, health: 1, dist: 1, bearing: 1, flags: 0 }).collect() };
    assert_eq!(Panel::parse(&many.message()).unwrap().members.len(), MAX_MEMBERS);
}

#[test]
fn the_panel_says_who_is_where_by_the_compass_and_the_text_is_what_the_screen_shows() {
    // north is -z, east +x
    assert_eq!(bearing_byte(0.0, -10.0), 0);
    assert_eq!(bearing_byte(10.0, 0.0), 64);
    assert_eq!(bearing_byte(0.0, 10.0), 128);
    assert_eq!(bearing_byte(-10.0, 0.0), 192);
    assert_eq!(bearing_byte(10.0, -10.0), 32);
    assert_eq!(bearing_byte(0.0, 0.0), 0);
    assert_eq!(bearing_byte(f32::NAN, 1.0), 0);
    let names: Vec<&str> = [0u8, 32, 64, 96, 128, 160, 192, 224, 255].iter().map(|b| compass(*b)).collect();
    assert_eq!(names, ["N", "NE", "E", "SE", "S", "SW", "W", "NW", "N"]);
    let panel = Panel {
        party: 103,
        leader: true,
        invite: None,
        members: vec![
            Row { name: "Ada".into(), id: 7, health: 255, dist: 0, bearing: 0, flags: Row::ME | Row::LEADER },
            Row { name: "Bo".into(), id: 8, health: 128, dist: 42, bearing: 32, flags: 0 },
            Row { name: "Cy".into(), id: 9, health: 0, dist: 1500, bearing: 192, flags: Row::DEAD },
            Row { name: "Di".into(), id: 0, health: 0, dist: Row::FAR, bearing: 0, flags: Row::AWAY },
        ],
    };
    assert_eq!(panel.lines(0, ""), ["PARTY (4)", "* Ada   100%   you", "- Bo   50%   42 m NE", "- Cy   dead   1.5 km W", "- Di   away"]);
    let inv = Panel { party: 0, leader: false, invite: Some(Invitation { from: "Ada".into(), secs: 27 }), members: vec![] };
    assert_eq!(inv.lines(24, ""), ["Ada invites you to a party (24 s)", "/accept   /decline"], "the seconds are the client's own count");
    assert_eq!(inv.lines(5, " F3 accept   F4 decline "), ["Ada invites you to a party (5 s)", "F3 accept   F4 decline"], "what the game says to do about it");
    assert!(Panel::default().lines(0, "").is_empty(), "nothing to show: an empty text");
}

#[test]
fn what_a_player_types_is_a_command_only_if_it_is_one_of_ours() {
    assert_eq!(parse_command("/invite Ada Lovelace"), Some(Cmd::Invite("Ada Lovelace")));
    assert_eq!(parse_command("/INVITE bo"), Some(Cmd::Invite("bo")));
    assert_eq!(parse_command("/accept"), Some(Cmd::Accept("")));
    assert_eq!(parse_command("/accept Ada"), Some(Cmd::Accept("Ada")));
    assert_eq!(parse_command("/decline"), Some(Cmd::Decline("")));
    assert_eq!(parse_command("/leave"), Some(Cmd::Leave));
    assert_eq!(parse_command("/kick Bo"), Some(Cmd::Kick("Bo")));
    assert_eq!(parse_command("/disband"), Some(Cmd::Disband));
    assert_eq!(parse_command("/promote Bo"), Some(Cmd::Promote("Bo")));
    assert_eq!(parse_command("/p hello there"), Some(Cmd::Say("hello there")));
    assert_eq!(parse_command("/party hello"), Some(Cmd::Say("hello")));
    assert_eq!(parse_command("/party"), Some(Cmd::Help));
    assert_eq!(parse_command("/help"), Some(Cmd::Help));
    assert_eq!(parse_command("/shrug"), None, "a word that is not ours");
    assert_eq!(parse_command("/"), None);
    assert_eq!(parse_command("hello /invite Bo"), None, "only a text that starts with a slash");
    assert_eq!(parse_command("/invitee Bo"), None, "the whole word");
}

/// What it costs to filter 200 candidates for a client that has no party, and for one that has a party of four (whose three mates are never tested): `cargo test --release -p lunaris-net party_filter -- --ignored --nocapture`.
#[test]
#[ignore]
fn party_filter_costs_a_client_with_no_party_what_it_always_did() {
    use super::interest::Candidate;
    use super::occlusion::{Counters, Eyes, Occlusion, Params, Sightings};
    use super::state::{class, EntityState};
    use std::time::Instant;
    struct Walls;
    impl Eyes for Walls {
        fn occludes(&self, c: u8) -> bool {
            c == class::PLAYER
        }
        // (a wall at x = 0: lines that cross it are blocked)
        fn blocked(&self, a: [f32; 3], b: [f32; 3]) -> bool {
            a[0] * b[0] < 0.0
        }
    }
    let mut flat: Vec<EntityState> = Vec::new();
    for i in 0..200u32 {
        let mut s = EntityState { class: class::PLAYER, id: i + 1, ..Default::default() };
        s.set_pos_m([-60.0 + (i % 20) as f32 * 6.0, 0.0, -50.0 + (i / 20) as f32 * 10.0]);
        flat.push(s);
    }
    let cfg = Occlusion::default();
    let p = Params::new(&cfg, 100, 30, 16.0, 40.0, 100.0, 16.0, 1, 0);
    let own = flat[0];
    let cands = |flat: &[EntityState]| -> Vec<Candidate> {
        flat.iter().enumerate().skip(1).map(|(i, s)| Candidate { idx: i as u32, id: s.id, dist: (((s.pos_m()[0] - own.pos_m()[0]).powi(2) + (s.pos_m()[2] - own.pos_m()[2]).powi(2)) as f32).sqrt(), class: s.class }).collect()
    };
    let mates = [30u32, 90, 150];
    let rounds = 2000;
    let mut results = Vec::new();
    for with_mates in [false, true, false, true] {
        let (mut seen, mut left) = (0usize, 0usize);
        let t = Instant::now();
        for r in 0..rounds {
            // (a fresh table each time: every pair is tested, which is the dearest a tick can be)
            let mut s = Sightings::default();
            let mut list = cands(&flat);
            let mut rays = 100_000u32;
            let mut n = Counters::default();
            let p = Params { tick: p.tick + r, ..p.clone() };
            if with_mates {
                s.filter_except(&Walls, &p, &own, &flat, &mut list, &mut rays, &mut n, &mates);
            } else {
                s.filter(&Walls, &p, &own, &flat, &mut list, &mut rays, &mut n);
            }
            seen += list.len();
            left += rays as usize;
        }
        let us = t.elapsed().as_secs_f64() * 1e6 / rounds as f64;
        results.push((with_mates, us, seen / rounds as usize, (100_000 * rounds as usize - left) / rounds as usize));
    }
    for (m, us, told, rays) in &results {
        println!("{}: {us:.1} us to filter 199 candidates (every pair tested), {told} told, {rays} rays", if *m { "a party of four (3 mates)" } else { "no party            " });
    }
}
