//! `NetHost.sync`: the wire format of the resources the server tells every client, what is due when, and a server and its clients end to end.
use super::netsync::{self, SyncOut};
use lunaris_sim::Sim;

fn names(text: &str) -> Vec<String> {
    netsync::parse_names(text)
}

#[test]
fn the_names_are_valid_resource_names_without_the_engines_own_each_once_and_at_most_32() {
    assert_eq!(names("dn.t dn.rate,dn.force\nzone.radius dn.t"), ["dn.t", "dn.rate", "dn.force", "zone.radius"]);
    assert_eq!(names("input.key.w action.jump game.x scene.y time.scale ok bad-name"), ["ok"], "the engine's own and the invalid are left out");
    let many: String = (0..40).map(|i| format!("r{i} ")).collect();
    assert_eq!(names(&many).len(), netsync::MAX_NAMES);
    assert!(names("").is_empty());
}

#[test]
fn values_travel_in_the_smallest_exact_form_and_come_back_the_same() {
    let list: [(&str, f64); 7] = [("a", 0.0), ("b", 1.0), ("c", 255.0), ("d", 256.0), ("e", -3.5), ("f", 13.2371), ("g", 1.0e300)];
    let m = netsync::encode(&list);
    let back = netsync::parse(&m).unwrap();
    assert_eq!(back.len(), 7);
    for ((n, v), (bn, bv)) in list.iter().zip(back.iter()) {
        assert_eq!((*n, *v), (bn.as_str(), *bv), "exactly as it was sent");
    }
    // sizes: the header 2 bytes, then a name (1 + its length), a tag, and 1 / 4 / 8 bytes of value
    assert_eq!(netsync::encode(&[("dn.rate", 1.0)]).len(), 2 + 1 + 7 + 1 + 1, "a small whole number is one byte");
    assert_eq!(netsync::encode(&[("dn.t", 7.25)]).len(), 2 + 1 + 4 + 1 + 4, "a value an f32 holds exactly is four");
    assert_eq!(netsync::encode(&[("dn.t", 13.2371)]).len(), 2 + 1 + 4 + 1 + 8, "any other is eight");
}

#[test]
fn what_cannot_be_read_is_refused_and_what_must_not_be_set_is_dropped() {
    assert!(netsync::parse(&[0x40, 0]).is_none(), "another kind of message");
    let m = netsync::encode(&[("dn.t", 13.2371), ("dn.rate", 2.0)]);
    for cut in 1..m.len() {
        assert!(netsync::parse(&m[..cut]).is_none(), "a message cut at {cut} is not read as a whole");
    }
    let mut bad_tag = netsync::encode(&[("a", 1.0)]);
    let at = bad_tag.len() - 2;
    bad_tag[at] = 9;
    assert!(netsync::parse(&bad_tag).is_none());
    // a server that names the client's raw input or an invalid name sets nothing there
    let m = netsync::encode(&[("input.key.w", 1.0), ("action.jump", 1.0), ("ok", 2.0), ("bad name", 3.0), ("f", f64::NAN)]);
    assert_eq!(netsync::parse(&m).unwrap(), vec![("ok".to_string(), 2.0)]);
}

#[test]
fn only_what_changed_is_due_and_not_more_often_than_asked_and_everything_comes_again_as_a_refresh() {
    let mut s = Sim::new(1);
    s.world.set_resource("dn.t", Some(7.25)).unwrap();
    s.world.set_resource("dn.rate", Some(1.0)).unwrap();
    let list = names("dn.t dn.rate dn.missing");
    let mut out = SyncOut::default();
    let entries = |m: Option<Vec<u8>>| m.map(|m| netsync::parse(&m).unwrap().into_iter().map(|(n, _)| n).collect::<Vec<_>>());
    // the first time everything that exists is due (a name with no resource is skipped)
    assert_eq!(entries(out.due(&s.world, &list, 100, 20, 1.0)), Some(vec!["dn.t".to_string(), "dn.rate".to_string()]));
    // nothing changed: nothing is due, at any tick before the refresh
    for t in 101..=140 {
        assert!(out.due(&s.world, &list, t, 20, 1.0).is_none(), "tick {t}");
    }
    // a change is not sent before its second is up, and then only that one goes
    s.world.set_resource("dn.t", Some(7.5)).unwrap();
    assert!(out.due(&s.world, &list, 110, 20, 1.0).is_none(), "(a second is 20 ticks and the last message was at tick 100)");
    assert_eq!(entries(out.due(&s.world, &list, 141, 20, 1.0)), Some(vec!["dn.t".to_string()]));
    // `every` 0: a change is due the very next tick
    s.world.set_resource("dn.t", Some(8.0)).unwrap();
    assert_eq!(entries(out.due(&s.world, &list, 142, 20, 0.0)), Some(vec!["dn.t".to_string()]));
    // a resource that appears later is sent when it does
    s.world.set_resource("dn.missing", Some(4.0)).unwrap();
    assert_eq!(entries(out.due(&s.world, &list, 143, 20, 0.0)), Some(vec!["dn.missing".to_string()]));
    // the refresh: five seconds (100 ticks at 20 Hz) after the last one, all of them again though none changed
    assert!(out.due(&s.world, &list, 190, 20, 0.0).is_none());
    let all = entries(out.due(&s.world, &list, 100 + 101, 20, 0.0)).unwrap();
    assert_eq!(all.len(), 3, "{all:?}");
    assert_eq!(netsync::parse(&out.full(&s.world, &list).unwrap()).unwrap().len(), 3, "a client that has just joined gets all of them");
    assert!(SyncOut::default().due(&s.world, &[], 1, 20, 1.0).is_none(), "no names: nothing, ever");
}

#[test]
fn applying_writes_resources_and_counts_only_the_ones_that_changed() {
    let mut s = Sim::new(1);
    let v = vec![("dn.t".to_string(), 7.25), ("dn.rate".to_string(), 1.0)];
    assert_eq!(netsync::apply(&mut s.world, &v), 2);
    assert_eq!((s.world.resource("dn.t"), s.world.resource("dn.rate")), (Some(7.25), Some(1.0)));
    assert_eq!(netsync::apply(&mut s.world, &v), 0, "the same again changes nothing");
}
