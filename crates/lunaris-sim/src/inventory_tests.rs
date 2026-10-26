//! Inventories: the item table, give / take / craft / wear, published totals, and the cards and slots of the interface.
use super::*;
use crate::components::{Ui, UiDrag};
use crate::inventory::{parse_table, Inventory, Station};
use lunaris_json::{parse, Json};

const TABLE: &str = "# test items\n\
item wood | Wood | stack=50 weight=0.5\n\
item stone | Stone | stack=50 weight=1\n\
item axe | Axe | stack=1 weight=2 class=1 power=7 dur=3 icon=2\n\
item helm | Helm | stack=1 weight=3 kind=1 icon=3\n\
recipe axe 1 = wood 5, stone 2 @1\n\
recipe helm 1 = stone 3\n";

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn world_with_table(size: u32) -> Sim {
    let mut s = Sim::new(1);
    let text = TABLE.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{text}","icons":"icons.png","icon_cols":4,"icon_rows":2}}}}"#));
    spawn(&mut s, 10, &format!(r#"{{"Inventory":{{"size":{size},"publish":"have"}}}}"#));
    s
}

fn inv(s: &Sim, uid: u64) -> Inventory {
    *s.world.get::<Inventory>(s.world.entity(uid).unwrap()).unwrap()
}

fn req(s: &mut Sim, uid: u64, f: impl FnOnce(&mut Inventory)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<Inventory>(e).unwrap());
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(f64::NAN)
}

#[test]
fn the_table_reads_items_recipes_and_says_what_it_does_not_understand() {
    let t = parse_table(TABLE);
    assert_eq!(t.items.len(), 4);
    assert_eq!(t.id_of("axe"), 3);
    assert_eq!(t.item(3).unwrap().dur, 3.0);
    assert_eq!(t.item(4).unwrap().kind, 1);
    assert_eq!(t.recipes.len(), 2);
    assert_eq!(t.recipes[0].ingredients, vec![(1, 5), (2, 2)]);
    assert_eq!((t.recipes[0].station, t.recipes[1].station), (1, 0));
    assert!(t.problems.is_empty(), "{:?}", t.problems);
    let bad = parse_table("item a b c | A | stack=x\nitem ok | OK | stack=5 colour=3\nrecipe ok 1 = nothing 2\nwhat\nitem ok | again\n");
    assert_eq!(bad.items.len(), 1);
    assert!(bad.problems.len() >= 5, "{:?}", bad.problems);
}

#[test]
fn give_fills_stacks_first_then_empty_slots_and_reports_what_did_not_fit() {
    let mut s = world_with_table(2);
    req(&mut s, 10, |i| (i.give_id, i.give_n) = (1, 120));
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.id[0], i.count[0], i.id[1], i.count[1]), (1, 50, 1, 50));
    assert_eq!((i.gave, i.left), (100, 20));
    assert_eq!((i.give_id, i.give_n), (0, 0), "requests are cleared");
    // taking 60 empties the second stack
    req(&mut s, 10, |i| (i.take_id, i.take_n) = (1, 60));
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.took, i.id[0], i.count[1]), (60, 0, 40), "{:?}", &i.count[..2]);
    // a tool goes into an empty slot with its durability
    req(&mut s, 10, |i| (i.give_id, i.give_n) = (3, 1));
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.id[0], i.count[0], i.dur[0]), (3, 1, 3.0));
    assert_eq!(i.gave, 1);
    let have = s.world.resource("have.axe");
    assert_eq!(have, Some(1.0));
}

#[test]
fn fill_is_starting_loot_given_once() {
    let mut s = world_with_table(8);
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(0));
    let sym = s.world.strings_mut().intern("wood 70, stone 3, nothing 4, axe 1");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    let i = inv(&s, 10);
    assert_eq!(i.fill.0, 0);
    assert_eq!((res(&s, "have.wood"), res(&s, "have.stone"), res(&s, "have.axe")), (70.0, 3.0, 1.0));
    assert_eq!(i.weight, 70.0 * 0.5 + 3.0 + 2.0);
    assert_eq!(res(&s, "have.weight"), i.weight as f64);
    assert_eq!(res(&s, "have.free"), 4.0, "8 slots, 4 in use");
    assert_eq!((res(&s, "have.can0"), res(&s, "have.can1")), (i.can[0] as f64, 0.0));
}

#[test]
fn the_selected_slot_is_published_with_what_it_holds() {
    let mut s = world_with_table(6);
    let sym = s.world.strings_mut().intern("wood 5, axe 1, helm 1");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    assert_eq!((res(&s, "have.sel"), res(&s, "have.held.id"), res(&s, "have.held.n")), (0.0, 1.0, 5.0));
    req(&mut s, 10, |i| i.sel = 1);
    s.step();
    assert_eq!((res(&s, "have.held.id"), res(&s, "have.held.class"), res(&s, "have.held.power"), res(&s, "have.held.dur")), (3.0, 1.0, 7.0, 3.0));
    req(&mut s, 10, |i| i.sel = 2);
    s.step();
    assert_eq!((res(&s, "have.held.id"), res(&s, "have.held.kind")), (4.0, 1.0));
    req(&mut s, 10, |i| i.sel = 5);
    s.step();
    assert_eq!((res(&s, "have.held.id"), res(&s, "have.held.class")), (0.0, 0.0));
}

#[test]
fn bag_keeps_equipment_slots_out_of_give() {
    let mut s = world_with_table(6);
    req(&mut s, 10, |i| i.bag = 4);
    req(&mut s, 10, |i| (i.give_id, i.give_n) = (3, 10));
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.gave, i.left), (4, 6), "only the first four slots take things");
    assert_eq!((i.id[4], i.id[5]), (0, 0));
}

#[test]
fn strict_weight_refuses_what_would_be_too_heavy() {
    let mut s = world_with_table(8);
    req(&mut s, 10, |i| {
        i.strict_weight = true;
        i.max_weight = 10.0;
        i.give_id = 2; // stone, weight 1
        i.give_n = 25;
    });
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.gave, i.left), (10, 15));
    assert_eq!(i.weight, 10.0);
}

#[test]
fn craft_takes_the_ingredients_gives_the_result_and_needs_its_station() {
    let mut s = world_with_table(6);
    let sym = s.world.strings_mut().intern("wood 12, stone 4");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    assert_eq!(inv(&s, 10).can, [0b10, 0], "only the recipe without a station can be made");
    req(&mut s, 10, |i| i.craft = 1);
    s.step();
    let i = inv(&s, 10);
    assert_eq!(i.crafted, 0, "no workbench near");
    assert_eq!(res(&s, "have.wood"), 12.0);
    req(&mut s, 10, |i| i.station = 1 << 1);
    s.step();
    assert_eq!(inv(&s, 10).can, [0b11, 0]);
    req(&mut s, 10, |i| i.craft = 1);
    s.step();
    let i = inv(&s, 10);
    assert_eq!(i.crafted, 1);
    assert_eq!((res(&s, "have.wood"), res(&s, "have.stone"), res(&s, "have.axe")), (7.0, 2.0, 1.0));
    // the second recipe needs 3 stone and there are 2 left
    req(&mut s, 10, |i| i.craft = 2);
    s.step();
    assert_eq!(inv(&s, 10).crafted, 0);
    let ev: Vec<String> = s.events.iter().map(|e| e.name.clone()).collect();
    assert!(ev.iter().any(|n| n == "craft"), "{ev:?}");
}

#[test]
fn craft_that_does_not_fit_changes_nothing() {
    let mut s = world_with_table(2);
    let sym = s.world.strings_mut().intern("wood 50, stone 50");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    // two slots, both full of stacks of 50: the axe has nowhere to go (the ingredients would free no slot)
    req(&mut s, 10, |i| {
        i.station = 1 << 1;
        i.craft = 1;
    });
    s.step();
    let i = inv(&s, 10);
    assert_eq!(i.crafted, 0, "not enough room is a refusal");
    assert_eq!((res(&s, "have.wood"), res(&s, "have.stone")), (50.0, 50.0));
}

#[test]
fn wear_uses_a_tool_up_and_breaks_it() {
    let mut s = world_with_table(4);
    req(&mut s, 10, |i| (i.give_id, i.give_n) = (3, 1));
    s.step();
    for k in [2.0f32, 1.0] {
        req(&mut s, 10, |i| i.wear = 1);
        s.step();
        assert_eq!(inv(&s, 10).dur[0], k);
    }
    req(&mut s, 10, |i| i.wear = 1);
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.id[0], i.count[0]), (0, 0), "broken");
    assert!(s.events.iter().any(|e| e.name == "item_break"));
    // wearing an empty slot or a thing with no durability does nothing
    req(&mut s, 10, |i| i.wear = 1);
    s.step();
}

// ---- slots and cards
fn ui_world() -> Sim {
    let mut s = world_with_table(4);
    spawn(&mut s, 11, r#"{"Inventory":{"size":4}}"#);
    for k in 0..4u64 {
        spawn(&mut s, 20 + k, &format!(r#"{{"Ui":{{"pos":[{},0.1],"size":[0.08,0.08]}},"UiDrop":{{"accept":4294967295}},"InvSlot":{{"inv":10,"index":{k},"label":{}}}}}"#, 0.1 * k as f32, 40 + k));
        spawn(&mut s, 30 + k, &format!(r#"{{"Ui":{{"pos":[0,0],"size":[0.06,0.06],"button":true}},"UiStyle":{{}},"UiDrag":{{"slot":{}}},"InvCard":{{"slot":{}}}}}"#, 20 + k, 20 + k));
        spawn(&mut s, 40 + k, r#"{"Ui":{"pos":[0,0],"size":[0.02,0.02]}}"#);
    }
    // a slot of a second inventory (a chest)
    spawn(&mut s, 50, r#"{"Ui":{"pos":[0.1,0.5],"size":[0.08,0.08]},"UiDrop":{},"InvSlot":{"inv":11,"index":0}}"#);
    spawn(&mut s, 51, r#"{"Ui":{"pos":[0,0],"size":[0.06,0.06],"button":true},"UiStyle":{},"UiDrag":{"slot":50},"InvCard":{"slot":50}}"#);
    // a place that is not an inventory slot
    spawn(&mut s, 60, r#"{"Ui":{"pos":[0.5,0.5],"size":[0.08,0.08]},"UiDrop":{"name":"ground"}}"#);
    s
}

fn drop_card(s: &mut Sim, card: u64, onto: u64) {
    let e = s.world.entity(card).unwrap();
    s.world.get_mut::<UiDrag>(e).unwrap().slot = lunaris_ecs::Uid(onto);
}

fn card_state(s: &Sim, card: u64) -> (bool, u32, u32, u64) {
    let e = s.world.entity(card).unwrap();
    let d = s.world.get::<UiDrag>(e).unwrap();
    (s.world.get::<Ui>(e).unwrap().visible, d.item, d.kind, d.slot.0)
}

#[test]
fn cards_show_what_their_slot_holds_and_labels_the_count() {
    let mut s = ui_world();
    let sym = s.world.strings_mut().intern("wood 7, helm 1");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    assert_eq!(card_state(&s, 30), (true, 1, 0, 20));
    assert_eq!(card_state(&s, 31), (true, 4, 1, 21), "the helm has kind 1: the card says so for the slots' accept");
    assert_eq!(card_state(&s, 32).0, false, "an empty slot hides its card");
    let l0 = s.world.get::<Ui>(s.world.entity(40).unwrap()).unwrap();
    assert!(l0.visible);
    assert_eq!(s.world.strings().get(l0.text.0), "7");
    assert!(!s.world.get::<Ui>(s.world.entity(41).unwrap()).unwrap().visible, "a count of 1 shows no label");
    // the picture: wood has icon 0, the first cell of 4 x 2
    let st = s.world.get::<crate::components::UiStyle>(s.world.entity(30).unwrap()).unwrap();
    assert_eq!(s.world.strings().get(st.image.0), "icons.png");
    assert_eq!(st.image_uv, [0.25 * 0.0 + 0.0, 0.0, 0.25, 0.5]);
}

#[test]
fn dropping_a_card_on_an_empty_slot_moves_the_stack() {
    let mut s = ui_world();
    let sym = s.world.strings_mut().intern("wood 7");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    drop_card(&mut s, 30, 22);
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.id[0], i.id[2], i.count[2]), (0, 1, 7));
    assert_eq!(card_state(&s, 30).3, 20, "the card is back in its own slot");
    assert_eq!(card_state(&s, 32), (true, 1, 0, 22));
    assert_eq!(card_state(&s, 30).0, false);
    assert!(s.events.iter().any(|e| e.name == "inv_move"));
}

#[test]
fn dropping_on_the_same_item_merges_up_to_the_stack_size_and_swapping_trades_places() {
    let mut s = ui_world();
    let sym = s.world.strings_mut().intern("wood 40, helm 1");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    // 40 wood in slot 0, a helm in slot 1; put 20 more wood in slot 2
    req(&mut s, 10, |i| {
        i.id[2] = 1;
        i.count[2] = 20;
    });
    s.step();
    // the interface swaps two cards when one is dropped on a full slot: both are sent to the other's slot
    drop_card(&mut s, 32, 20);
    drop_card(&mut s, 30, 22);
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.count[0], i.count[2]), (10, 50), "merged up to 50 in the target, the rest stays");
    // a swap of two different items
    drop_card(&mut s, 31, 22);
    drop_card(&mut s, 32, 21);
    s.step();
    let i = inv(&s, 10);
    assert_eq!((i.id[1], i.count[1], i.id[2]), (1, 50, 4));
}

#[test]
fn cards_move_things_between_two_inventories() {
    let mut s = ui_world();
    let sym = s.world.strings_mut().intern("wood 9");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    drop_card(&mut s, 30, 50);
    s.step();
    assert_eq!((inv(&s, 10).id[0], inv(&s, 11).id[0], inv(&s, 11).count[0]), (0, 1, 9));
    assert_eq!(card_state(&s, 51).0, true);
    assert_eq!(card_state(&s, 30).3, 20);
    // and back
    drop_card(&mut s, 51, 23);
    s.step();
    assert_eq!((inv(&s, 11).id[0], inv(&s, 10).count[3]), (0, 9));
}

/// The interface of `ui_world` on a client: the bag (10) is the player's own, the chest (11) the container it has open.
fn online_world(connected: bool) -> Sim {
    let mut s = ui_world();
    s.apply(&Command::Add { uid: 10, component: "NetPlayer".into(), value: parse(&format!(r#"{{"connected":{connected}}}"#)).unwrap() }).unwrap();
    s.apply(&Command::Add { uid: 11, component: "NetContainer".into(), value: parse(r#"{"open":true}"#).unwrap() }).unwrap();
    s
}

fn asked(s: &Sim) -> (String, [f32; 4]) {
    let p = *s.world.get::<crate::components::NetPlayer>(s.world.entity(10).unwrap()).unwrap();
    (s.world.strings().get(p.cmd.0).to_string(), p.cmd_args)
}

#[test]
fn on_a_game_server_a_move_in_the_interface_is_also_asked_of_the_server_and_nothing_is_sent_offline() {
    for connected in [false, true] {
        let mut s = online_world(connected);
        let sym = s.world.strings_mut().intern("wood 9");
        req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
        s.step();
        // the stack of slot 0 of the bag goes to the chest's slot 0: place 0 and place 100
        drop_card(&mut s, 30, 50);
        s.step();
        assert_eq!(inv(&s, 11).count[0], 9, "the move shows at once");
        if connected {
            assert_eq!(asked(&s), ("inv_move".to_string(), [0.0, 100.0, 0.0, 0.0]));
        } else {
            assert_eq!(asked(&s).0, "", "offline nothing is asked");
        }
    }
    // back again from the chest to slot 3 of the bag, the cards swapped as the interface does (the empty slot's card goes the other way)
    let mut s = online_world(true);
    let sym = s.world.strings_mut().intern("wood 9");
    req(&mut s, 11, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    drop_card(&mut s, 51, 23);
    drop_card(&mut s, 33, 50);
    s.step();
    assert_eq!(inv(&s, 10).count[3], 9);
    assert_eq!(asked(&s), ("inv_move".to_string(), [100.0, 3.0, 0.0, 0.0]), "the direction is the stack's, not the card's");
}

#[test]
fn a_card_let_go_over_the_ground_asks_the_server_to_drop_it_but_only_the_players_own_things() {
    let mut s = online_world(true);
    let sym = s.world.strings_mut().intern("wood 9, helm 1");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    drop_card(&mut s, 31, 60);
    s.step();
    assert_eq!(asked(&s), ("inv_drop".to_string(), [1.0, 0.0, 0.0, 0.0]));
    // a card of the chest let go over the ground is not the player's to throw away
    let mut s = online_world(true);
    let sym = s.world.strings_mut().intern("wood 9");
    req(&mut s, 11, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    drop_card(&mut s, 51, 60);
    s.step();
    assert_eq!(asked(&s).0, "");
}

#[test]
fn a_card_that_is_later_in_uid_order_than_the_one_it_swaps_with_still_moves_its_stack() {
    // the interface swaps the cards of the two slots on a drop, so both cards are \"moved\"; the hidden card of the empty slot is the lower one when the stack comes from a later inventory
    let mut s = ui_world();
    let sym = s.world.strings_mut().intern("wood 9");
    req(&mut s, 11, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    assert_eq!(inv(&s, 11).count[0], 9);
    // the card 51 (of the chest, the later one) is dropped on the slot 22 whose card 32 is empty: the interface sends card 32 to the slot 50
    drop_card(&mut s, 51, 22);
    drop_card(&mut s, 32, 50);
    s.step();
    assert_eq!((inv(&s, 11).id[0], inv(&s, 10).id[2], inv(&s, 10).count[2]), (0, 1, 9));
    assert_eq!((card_state(&s, 51).3, card_state(&s, 32).3), (50, 22), "every card is back in its own slot");
}

#[test]
fn a_drop_on_something_else_is_reported_not_done() {
    let mut s = ui_world();
    let sym = s.world.strings_mut().intern("wood 9");
    req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
    s.step();
    drop_card(&mut s, 30, 60);
    s.step();
    assert_eq!((res(&s, "inv.out.uid"), res(&s, "inv.out.id"), res(&s, "inv.out.n")), (60.0, 1.0, 9.0));
    assert_eq!(inv(&s, 10).count[0], 9, "nothing was taken");
    assert_eq!(card_state(&s, 30).3, 20);
    s.step();
    assert_eq!(res(&s, "inv.out.uid"), 0.0, "the report lasts a tick");
}

#[test]
fn inventories_are_deterministic_and_replay_exactly() {
    let run = || {
        let mut s = ui_world();
        let sym = s.world.strings_mut().intern("wood 33, stone 5, axe 1, helm 1");
        req(&mut s, 10, |i| i.fill = lunaris_ecs::Sym(sym));
        for k in 0..30u64 {
            if k % 7 == 3 {
                drop_card(&mut s, 30 + (k % 3), 20 + ((k + 1) % 4));
            }
            if k % 5 == 0 {
                req(&mut s, 10, |i| (i.give_id, i.give_n) = (1 + (k % 2) as u32, 3));
            }
            s.step();
        }
        s.world.state_hash()
    };
    assert_eq!(run(), run());
}

const TIMED: &str = "item wood | Wood | stack=50 weight=0.5\nitem plank | Plank | stack=50\nitem box | Box | stack=5\nrecipe plank 2 = wood 1 ~ 2\nrecipe box 1 = plank 2\n";

fn timed_world() -> Sim {
    let mut s = Sim::new(1);
    let text = TIMED.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{text}"}}}}"#));
    spawn(&mut s, 10, r#"{"Inventory":{"size":12,"publish":"have","fill":"wood 5"}}"#);
    s.step_n(2);
    s
}

#[test]
fn a_recipe_can_say_how_long_it_takes() {
    let t = parse_table(TIMED);
    assert!(t.problems.is_empty(), "{:?}", t.problems);
    assert_eq!((t.recipes[0].time, t.recipes[1].time), (2.0, 0.0));
    let bad = parse_table("item a | A\nrecipe a 1 = a 1 ~ soon\n");
    assert!(bad.problems.iter().any(|p| p.contains("not a number of seconds")), "{:?}", bad.problems);
}

#[test]
fn a_timed_craft_takes_the_ingredients_at_once_and_gives_the_result_after_its_seconds_for_every_one_of_the_quantity() {
    let mut s = timed_world();
    assert_eq!(res(&s, "have.wood"), 5.0);
    req(&mut s, 10, |i| {
        i.craft_start = 1;
        i.craft_qty = 2;
    });
    s.step_n(1);
    let i = inv(&s, 10);
    assert_eq!((i.crafting, i.craft_todo), (1, 2), "{i:?}");
    assert!((i.craft_left - 2.0).abs() < 0.05 && (i.craft_total - 2.0).abs() < 1e-6);
    assert_eq!((res(&s, "have.wood"), res(&s, "have.plank")), (4.0, 0.0), "the wood of the first craft is taken at once");
    s.step_n(60);
    assert_eq!(res(&s, "have.plank"), 0.0, "not before its seconds");
    s.step_n(66);
    let i = inv(&s, 10);
    assert_eq!((res(&s, "have.plank"), res(&s, "have.wood"), i.crafting, i.craft_todo), (2.0, 3.0, 1, 1), "the first is made, the second is under way: {i:?}");
    s.step_n(126);
    let i = inv(&s, 10);
    assert_eq!((res(&s, "have.plank"), res(&s, "have.wood"), i.crafting, i.craft_todo), (4.0, 3.0, 0, 0), "both are made: {i:?}");
}

#[test]
fn a_timed_craft_that_cannot_be_made_is_refused_and_a_cancel_gives_the_ingredients_back() {
    let mut s = timed_world();
    req(&mut s, 10, |i| i.craft_start = 2); // (a box needs 2 planks: there are none)
    s.step_n(2);
    assert_eq!(inv(&s, 10).crafting, 0);
    req(&mut s, 10, |i| i.craft_start = 1);
    s.step_n(30);
    assert_eq!((inv(&s, 10).crafting, res(&s, "have.wood")), (1, 4.0));
    req(&mut s, 10, |i| i.craft_cancel = true);
    s.step_n(2);
    let i = inv(&s, 10);
    assert_eq!((i.crafting, i.craft_todo, res(&s, "have.wood"), res(&s, "have.plank")), (0, 0, 5.0, 0.0), "the wood is back and nothing is made: {i:?}");
}

#[test]
fn an_instant_recipe_is_made_as_many_times_as_asked_and_a_clients_copy_does_not_craft() {
    let mut s = timed_world();
    req(&mut s, 10, |i| i.give_id = 2);
    req(&mut s, 10, |i| i.give_n = 6); // (6 planks)
    s.step_n(2);
    req(&mut s, 10, |i| {
        i.craft_start = 2;
        i.craft_qty = 5;
    });
    s.step_n(2);
    assert_eq!((res(&s, "have.box"), res(&s, "have.plank")), (3.0, 0.0), "three boxes from six planks, the fourth could not be made");
    // a client's copy (the player of an online game) counts down for the display and never makes anything
    spawn(&mut s, 11, r#"{"Inventory":{"size":12,"publish":"cl","fill":"wood 3"},"NetPlayer":{"connected":true}}"#);
    s.step_n(2);
    req(&mut s, 11, |i| {
        i.craft_start = 1;
        i.crafting = 1;
        i.craft_left = 1.0;
        i.craft_todo = 1;
    });
    s.step_n(90);
    let c = inv(&s, 11);
    assert_eq!((res(&s, "cl.plank"), res(&s, "cl.wood"), c.craft_left), (0.0, 3.0, 0.0), "{c:?}");
}

#[test]
fn the_older_craft_request_on_a_timed_recipe_is_a_timed_start_and_never_makes_it_at_once() {
    let mut s = timed_world();
    req(&mut s, 10, |i| i.craft = 1); // (recipe 1 takes 2 s)
    s.step_n(2);
    let i = inv(&s, 10);
    assert_eq!((i.crafting, res(&s, "have.plank"), res(&s, "have.wood")), (1, 0.0, 4.0), "started, not made: {i:?}");
    s.step_n(130);
    assert_eq!((inv(&s, 10).crafting, res(&s, "have.plank")), (0, 2.0));
}

#[test]
fn a_cancel_into_a_full_bag_keeps_the_craft_and_a_craft_whose_result_no_longer_fits_waits_and_says_so_once() {
    let mut s = Sim::new(1);
    let text = TIMED.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{text}"}}}}"#));
    spawn(&mut s, 10, r#"{"Inventory":{"size":2,"publish":"have","fill":"wood 1"}}"#);
    s.step_n(2);
    req(&mut s, 10, |i| i.craft_start = 1);
    s.step_n(2);
    assert_eq!((inv(&s, 10).crafting, res(&s, "have.wood")), (1, 0.0));
    // the bag is filled with boxes (2 slots of 5)
    req(&mut s, 10, |i| {
        i.give_id = 3;
        i.give_n = 10;
    });
    s.step_n(2);
    assert_eq!(res(&s, "have.box"), 10.0);
    // a cancel finds no room for the wood: the craft goes on and nothing is lost
    req(&mut s, 10, |i| i.craft_cancel = true);
    s.step_n(2);
    assert_eq!((inv(&s, 10).crafting, res(&s, "have.wood")), (1, 0.0), "the wood is not given back into a full bag");
    // the time runs out with no room for the planks: the craft waits (craft_wait) with its ingredients kept
    s.step_n(130);
    let i = inv(&s, 10);
    assert_eq!((i.crafting, i.craft_wait, res(&s, "have.plank")), (1, true, 0.0), "{i:?}");
    // room is made: the planks arrive
    req(&mut s, 10, |i| {
        i.take_id = 3;
        i.take_n = 5;
    });
    s.step_n(3);
    let i = inv(&s, 10);
    assert_eq!((i.crafting, i.craft_wait, res(&s, "have.plank")), (0, false, 2.0), "{i:?}");
}

#[test]
fn a_recipe_may_say_its_station_and_its_time_in_either_order_and_a_bad_time_drops_the_line() {
    let t = parse_table("item a | A\nitem b | B\nrecipe b 1 = a 1 ~ 5 @2\nrecipe b 2 = a 2 @3 ~ 1\nrecipe b 3 = a 3 ~ soon @2\n");
    assert_eq!(t.recipes.len(), 2, "{:?}", t.problems);
    assert_eq!((t.recipes[0].time, t.recipes[0].station), (5.0, 2));
    assert_eq!((t.recipes[1].time, t.recipes[1].station), (1.0, 3));
    assert!(t.problems.iter().any(|p| p.contains("not a number of seconds")), "{:?}", t.problems);
}

#[test]
fn a_player_copy_that_is_not_connected_crafts_by_itself() {
    let mut s = timed_world();
    spawn(&mut s, 11, r#"{"Inventory":{"size":12,"publish":"cl","fill":"wood 3"},"NetPlayer":{"connected":false}}"#);
    s.step_n(2);
    req(&mut s, 11, |i| i.craft_start = 1);
    s.step_n(130);
    assert_eq!((res(&s, "cl.plank"), res(&s, "cl.wood")), (2.0, 2.0), "offline play of the online scene makes things itself");
}

// ------------------------------------------------------------------------------------------------ stations as entities

/// A world with the test table (recipe 1, the axe, needs station 1; recipe 2, the helm, none), a hero with materials and a bench 2 m away.
fn bench_world() -> Sim {
    let mut s = Sim::new(1);
    let text = TABLE.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{text}"}}}}"#));
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,0,0]},"Inventory":{"size":8,"publish":"have","fill":"wood 12, stone 8"}}"#);
    spawn(&mut s, 20, r#"{"Transform":{"pos":[2,0,0]},"Station":{"bit":1,"radius":3}}"#);
    s.step_n(2);
    s
}

fn put(s: &mut Sim, uid: u64, pos: [f32; 3]) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<crate::components::Transform>(e).unwrap().pos = pos;
}

#[test]
fn a_station_entity_unlocks_its_recipes_for_inventories_in_reach_and_walking_away_locks_them_again() {
    let mut s = bench_world();
    assert_eq!(inv(&s, 10).can, [0b11, 0], "the bench is 2 m away: the axe can be made");
    assert_eq!(res(&s, "have.near"), 2.0, "bit 1 is at hand");
    assert_eq!(inv(&s, 10).station, 0, "the engine never writes the bits into the game's own field");
    req(&mut s, 10, |i| i.craft = 1);
    s.step();
    assert_eq!(inv(&s, 10).crafted, 1, "made at the bench");
    // away from the bench: only the helm
    put(&mut s, 10, [8.0, 0.0, 0.0]);
    s.step();
    assert_eq!(inv(&s, 10).can, [0b10, 0]);
    assert_eq!(res(&s, "have.near"), 0.0);
    req(&mut s, 10, |i| i.craft = 1);
    s.step();
    assert_eq!(inv(&s, 10).crafted, 0, "no bench in reach");
    // the edge: the radius is along the ground and includes the rim; the vertical reach is `height`
    put(&mut s, 10, [5.0, 0.0, 0.0]);
    s.step();
    assert_eq!(res(&s, "have.near"), 2.0, "exactly 3 m away is in reach");
    put(&mut s, 10, [5.05, 0.0, 0.0]);
    s.step();
    assert_eq!(res(&s, "have.near"), 0.0, "3.05 m is not");
    put(&mut s, 10, [2.0, 3.0, 0.0]);
    s.step();
    assert_eq!(res(&s, "have.near"), 0.0, "3 m above the bench (past `height` 2.5: another floor) is not");
    let e = s.world.entity(20).unwrap();
    s.world.get_mut::<Station>(e).unwrap().height = 0.0;
    s.step();
    assert_eq!(res(&s, "have.near"), 2.0, "`height` 0 is no vertical limit");
    // the game's own bits and the bench's bits add up, and the game's field is left as it was
    req(&mut s, 10, |i| i.station = 1 << 2);
    s.step();
    assert_eq!(res(&s, "have.near"), 2.0);
    assert_eq!(inv(&s, 10).station, 1 << 2);
    // the bench goes away: its bit with it, and the published mask goes back to 0
    s.apply(&Command::Despawn { uid: 20 }).unwrap();
    s.step();
    assert_eq!((res(&s, "have.near"), inv(&s, 10).can), (0.0, [0b10, 0]));
}

#[test]
fn a_station_that_cannot_reach_anybody_says_nothing_and_a_world_without_stations_is_unchanged() {
    let mut s = bench_world();
    // an inventory without a Transform, a bit outside 1 to 31, a radius of 0 and a station without a Transform reach nobody
    spawn(&mut s, 11, r#"{"Inventory":{"size":8,"publish":"away","fill":"wood 12, stone 4"}}"#);
    for (uid, comps) in [(21, r#"{"Transform":{},"Station":{"bit":0,"radius":50}}"#), (22, r#"{"Transform":{},"Station":{"bit":32,"radius":50}}"#), (23, r#"{"Transform":{},"Station":{"bit":1,"radius":0}}"#), (24, r#"{"Station":{"bit":1,"radius":50}}"#)] {
        spawn(&mut s, uid, comps);
    }
    s.apply(&Command::Despawn { uid: 20 }).unwrap();
    s.step_n(2);
    assert_eq!(res(&s, "have.near"), 0.0, "none of the four reaches the hero standing on them");
    assert_eq!(inv(&s, 10).can, [0b10, 0]);
    // (an inventory with no Transform is near nothing, even of a bench 50 m wide)
    spawn(&mut s, 25, r#"{"Transform":{},"Station":{"bit":1,"radius":50}}"#);
    s.step_n(2);
    assert_eq!(res(&s, "away.near"), 0.0, "an inventory without a Transform is near nothing");
    assert_eq!(res(&s, "have.near"), 2.0, "while the hero is");
    // no Station at all: the resource is not even made, so nothing about such a world changes
    let mut t = world_with_table(6);
    t.step_n(3);
    assert!(t.world.resource("have.near").is_none());
}

#[test]
fn a_station_counts_in_the_clients_copy_and_for_timed_crafts_too() {
    let text = "item wood | Wood\nitem plank | Plank\nrecipe plank 1 = wood 1 @2 ~ 1\n".replace('\n', "\\n");
    // the player's copy on a client works out the same bits from the same entity: its own `can` and `have.can0` account for the bench
    let mut s = Sim::new(1);
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{text}"}}}}"#));
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,0,0]},"NetPlayer":{"connected":true},"Inventory":{"size":8,"publish":"have","fill":"wood 3"}}"#);
    spawn(&mut s, 20, r#"{"Transform":{"pos":[1,0,1]},"Station":{"bit":2,"radius":2}}"#);
    s.step_n(2);
    assert_eq!(inv(&s, 10).can, [1, 0]);
    assert_eq!((res(&s, "have.can0"), res(&s, "have.near")), (1.0, 4.0));
    // the server's copy: the craft starts at the bench and the one in hand finishes after the walk away
    let mut s = Sim::new(1);
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{text}"}}}}"#));
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,0,0]},"Inventory":{"size":8,"publish":"have","fill":"wood 3"}}"#);
    spawn(&mut s, 20, r#"{"Transform":{"pos":[1,0,1]},"Station":{"bit":2,"radius":2}}"#);
    s.step_n(2);
    req(&mut s, 10, |i| {
        i.craft_start = 1;
        i.craft_qty = 2;
    });
    s.step_n(2);
    assert_eq!((inv(&s, 10).crafting, inv(&s, 10).craft_todo), (1, 2));
    put(&mut s, 10, [9.0, 0.0, 0.0]);
    s.events.clear();
    s.step_n(80);
    let i = inv(&s, 10);
    assert_eq!(res(&s, "have.plank"), 1.0, "the craft in hand finishes after walking away");
    assert_eq!((i.crafting, i.craft_todo), (0, 0), "the second of the quantity needs the bench again, and the queue ends");
    assert_eq!(res(&s, "have.wood"), 2.0, "only the wood of the first was taken (3, minus 1)");
    let names: Vec<&str> = s.events.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"craft_denied_station"), "the end of the queue is told: {names:?}");
}

fn tick_events(s: &mut Sim) -> Vec<String> {
    s.events.clear();
    s.step();
    s.events.iter().map(|e| e.name.clone()).collect()
}

/// Exactly the refusal events `craft_denied` and `craft_denied_WHY` happened.
fn only_denied(ev: &[String], why: &str) -> bool {
    let all: Vec<&String> = ev.iter().filter(|n| n.starts_with("craft_denied")).collect();
    all.len() == 2 && all.iter().any(|n| n.as_str() == "craft_denied") && all.iter().any(|n| **n == format!("craft_denied_{why}"))
}

#[test]
fn a_refused_craft_says_why_station_ingredients_room_or_busy() {
    let mut s = Sim::new(1);
    let text = "item wood | Wood | stack=5\nitem plank | Plank | stack=5\nitem box | Box | stack=1\nrecipe plank 1 = wood 1 @1 ~ 1\nrecipe box 1 = plank 3 ~ 5\nrecipe plank 2 = wood 2\nrecipe box 1 = plank 1\nrecipe box 1 = plank 1 ~ 2\n".replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{text}"}}}}"#));
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,0,0]},"Inventory":{"size":2,"bag":2,"publish":"have","fill":"wood 4"}}"#);
    s.step_n(2);
    // station: recipe 1 needs station 1 and there is no bench in reach (as a timed start and as the older request)
    req(&mut s, 10, |i| i.craft_start = 1);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "station"), "{ev:?}");
    req(&mut s, 10, |i| i.craft = 1);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "station"), "{ev:?}");
    assert_eq!(res(&s, "have.wood"), 4.0, "nothing was taken");
    // ingredients: a box (recipe 2, timed) needs 3 planks and recipe 4 (instant) one, and there are none
    req(&mut s, 10, |i| i.craft_start = 2);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "ingredients"), "{ev:?}");
    req(&mut s, 10, |i| i.craft = 4);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "ingredients"), "{ev:?}");
    // a request for a recipe that does not exist is not a refused craft
    req(&mut s, 10, |i| i.craft_start = 9);
    let ev = tick_events(&mut s);
    assert!(!ev.iter().any(|n| n.starts_with("craft_denied")), "{ev:?}");
    // a craft that works says nothing of refusal (recipe 3: 2 wood into 2 planks; the bag is now wood 2 + plank 2: both slots used)
    req(&mut s, 10, |i| i.craft = 3);
    let ev = tick_events(&mut s);
    assert!(ev.iter().any(|n| n == "craft") && !ev.iter().any(|n| n.starts_with("craft_denied")), "{ev:?}");
    assert_eq!((res(&s, "have.wood"), res(&s, "have.plank")), (2.0, 2.0));
    // room: a box (stack 1) from one plank has no empty slot to go to, instant (recipe 4) and timed (recipe 5, which also says `inv_full` as before)
    req(&mut s, 10, |i| i.craft = 4);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "room"), "{ev:?}");
    req(&mut s, 10, |i| i.craft_start = 5);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "room") && ev.iter().any(|n| n == "inv_full"), "{ev:?}");
    assert_eq!((res(&s, "have.plank"), inv(&s, 10).crafting), (2.0, 0), "nothing was taken and nothing started");
}

#[test]
fn a_craft_asked_for_while_one_is_in_hand_is_refused_as_busy_and_a_quantity_that_runs_out_is_not_a_refusal() {
    let mut s = timed_world();
    req(&mut s, 10, |i| i.craft_start = 1);
    let ev = tick_events(&mut s);
    assert!(!ev.iter().any(|n| n.starts_with("craft_denied")), "{ev:?}");
    req(&mut s, 10, |i| i.craft_start = 1);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "busy"), "{ev:?}");
    assert_eq!((inv(&s, 10).crafting, res(&s, "have.wood")), (1, 4.0), "the craft in hand goes on and the second took nothing");
    // an instant recipe with a quantity: three planks make one box (2 each), the second of five has no ingredients: that ends the quantity, it is not a refusal
    let mut s = timed_world();
    req(&mut s, 10, |i| (i.give_id, i.give_n) = (2, 3));
    s.step_n(2);
    req(&mut s, 10, |i| {
        i.craft_start = 2;
        i.craft_qty = 5;
    });
    let ev = tick_events(&mut s);
    assert!(ev.iter().any(|n| n == "craft") && !ev.iter().any(|n| n.starts_with("craft_denied")), "{ev:?}");
    assert_eq!(res(&s, "have.box"), 1.0);
    // ... but asking again with one plank left is
    req(&mut s, 10, |i| i.craft_start = 2);
    let ev = tick_events(&mut s);
    assert!(only_denied(&ev, "ingredients"), "{ev:?}");
}

#[test]
fn the_item_table_last_read_is_found_again_by_its_text_whatever_its_length() {
    // two worlds one after the other on one thread with item tables as long as each other, one item named differently: each gets its own table
    let mut a = world_with_table(8);
    let ta = crate::inventory::table_of(&mut a.world).unwrap().0;
    assert!(ta.id_of("wood") != 0 && ta.id_of("wool") == 0);
    let mut b = Sim::new(1);
    let other = TABLE.replace("item wood | Wood", "item wool | Wool").replace("wood 5", "wool 5");
    assert_eq!(other.len(), TABLE.len());
    let text = other.replace('\n', "\\n");
    spawn(&mut b, 1, &format!(r#"{{"ItemTable":{{"text":"{text}"}}}}"#));
    let tb = crate::inventory::table_of(&mut b.world).unwrap().0;
    assert!(tb.id_of("wool") != 0 && tb.id_of("wood") == 0, "the second world's table, not the first's");
    let ta = crate::inventory::table_of(&mut a.world).unwrap().0;
    assert!(ta.id_of("wood") != 0 && ta.id_of("wool") == 0, "and the first is read again with its own");
}
