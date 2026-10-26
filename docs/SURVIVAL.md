# Survival needs: hunger, thirst, stamina, bleeding

`Vitals` keeps what a survival game asks of a body: hunger, thirst, stamina and bleeding, with the damage an empty meter does, the slow healing of a fed body, and the numbers a movement script needs (can it
sprint, how fast may it walk). It is plain numbers on a component beside the character's `HitPoints`, so it is saved with the world, replayed bit for bit and replicated like any other state. It draws and plays
nothing: the bars, the stomach growl and the heartbeat are the game's. Code: `crates/lunaris-sim/src/survival.rs` (tests in `survival_tests.rs`).

```json
{ "uid": 5, "components": {
    "Transform": { "pos": [0, 0, 0] }, "CharacterBody": {},
    "HitPoints": { "value": 100, "max": 100 },
    "Vitals": { "hunger_rate": 0.05, "thirst_rate": 0.08, "sprint_cost": 18, "starve_damage": 0.5 } } }
```

## The meters

| field | meaning |
|---|---|
| `hunger`, `thirst`, `stamina` | 0 to 100, 100 is full |
| `hunger_rate`, `thirst_rate` | points lost a second (0.05: full to empty in 33 minutes; 0.08: in 21) |
| `exertion` | hunger and thirst fall this many times faster while sprinting (2); `resting` halves them |
| `sprint_cost`, `stamina_regen`, `stamina_delay` | stamina lost a second while sprinting (18), gained a second when not (12), after this many seconds of not sprinting (1) |
| `starve_damage`, `dehydrate_damage` | hit points lost a second at hunger 0 (0.5) and thirst 0 (0.8) |
| `bleeding` | hit points lost a second until stopped |
| `heal_threshold`, `heal_rate` | with hunger and thirst both at least 70 and no harm being done, the body heals 0.3 a second |

Harm goes to the `HitPoints` of the same entity (`died` and the rest are `HitPoints`' own); a dead character is not touched.

## What a game does with it

Inputs the game sets each tick: `sprint` (the sprint key is down), `resting` (sitting, sleeping, in a shelter). One-tick requests when an item is used, which the engine uses up: `eat` and `drink` add to the
meters, `heal` adds hit points at once, `bandage` true stops the bleeding. Something that makes a character bleed sets `bleeding` (a bullet wound, a fall: the game decides when; a graph system on the event
`hit` can do it).

What the engine answers:

- `can_sprint` and `exhausted`: stamina at 0 makes the character exhausted; it cannot sprint until stamina is back to 25. A movement script only lets the sprint through when `can_sprint` is true.
- `speed_scale`: multiply the walking speed by it (0.6 when exhausted, 0.85 when hunger or thirst is under 10, 0.9 while bleeding; they multiply).
- `status` bits for the interface: 1 hungry (below 25), 2 thirsty (below 25), 4 bleeding, 8 exhausted, 16 starving or dehydrated (a meter at 0: it is taking damage).
- Events when a bit turns on: `hungry`, `thirsty`, `bleeding`, `exhausted`, `starving` (read them with an `EventWatch`, `docs/GAMEPLAY.md`).

A graph system that feeds the keys in and the speed out, for a character that has `CharacterBody` and `Vitals`:

```json
{ "name": "survival_input", "each": ["Vitals"], "do": [
    { "set": "Vitals.sprint", "to": { "gt": [ { "res": "input.shift" }, 0.5 ] } } ] }
```

and, in the system that sets `CharacterBody.walk`, multiply the speed by `{"get": "Vitals.speed_scale"}` and ignore the sprint when `Vitals.can_sprint` is false.

## What to supply

No art is made by the engine: the meter icons and bars, the screen effects of being hungry, wounded or exhausted (a vignette, blurred edges), the sounds (a growl, panting, a heartbeat, a cough), the food and water
and bandage items with their models and icons, and the animations (drinking, eating, bandaging, limping). Item effects are the game's data: when an item of the `Inventory` is used, a system sets `Vitals.eat`,
`.drink`, `.heal` or `.bandage` by the item's own numbers.

## Limits (so far)

- No temperature, sickness, radiation, sleep or hygiene: a game adds them as its own components or systems, or asks for them here. The harm they do is a `HitPoints` subtraction like the others.
- Bleeding is one number (hit points a second): there are no separate wounds, no fractures, no infection.
- The speed effects are fixed numbers (0.6, 0.85, 0.9); a game can ignore `speed_scale` and compute its own from `status`.

## Wounds (2026-10-24)

A hit of a weapon can start a bleed. `Vitals.wound_chance` (0 to 1, default 0: never), `wound_min` (6), `wound_rate` (0.5) and `bleed_max` (2): a hit that gets through the worn armor with at least `wound_min` hit points starts a bleed with the chance
`wound_chance`, adding `wound_rate` hit points a second to `bleeding`, up to `bleed_max`; the bandage (`bandage`) stops it. The chance is a hash of the tick, the victim and the shot (`survival::wound`, `hash_rand`): no state, the same world
bleeds the same way. It applies to every hit of the weapons system (rays, flying rounds, a machine's bite) and not to explosions or falls. A vest that stops most of a round also stops the wound, because the damage that counts is what got through.

