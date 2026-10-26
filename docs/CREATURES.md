# Creatures

An entity with a `Creature` is an **evolved animal**: a spider whose body and brain were trained together (by the Evolve-0 project's GPU trainer, `spidergpu`) to walk to a target over any ground, and that the engine runs exactly as it was trained. It is the part of Lunaris for enemies, pets, prey and wildlife that move like animals and not like sliding capsules: eight legs that step, a body that rides over bumps, a gait that speeds up and turns.

```json
{ "uid": 20, "components": {
    "Transform": { "pos": [-2, 0, -1.5] },
    "Creature":  { "brain": "spider_brain.json", "body_seed": 3, "target": 10 } } }
```

That is all: the spider appears at its `Transform`, stands, and walks to entity 10 (or to the point `goal`), forever, turning, speeding up when the target is far, going round bumps. `examples/spiders.lunaris.json` is a scene with three of them chasing a wandering ball.

## The component

| field | meaning |
|---|---|
| `brain` | a file of the workspace: the trainer's `champions.json` (the best programs of a run, best first) or one genome |
| `genome` | which program of the file (0 the best); different numbers give animals that walk differently |
| `model` | the file of another body (empty: the built-in spider) |
| `body_seed` | this animal's own body: its strength, weight, grip and joint stiffness differ a little from another seed's, so two spiders are never twins |
| `target` | the entity it walks to (its `Transform`); 0 means the point `goal` |
| `goal` | the point to walk to (x and z) when there is no `target` |
| `size` | how big the animal is against the one it was trained as: 1 is a spider 1.5 m across, 0.25 a big tarantula, 0.12 a wolf spider. It is the same animal in smaller units (lengths times `size`, masses times `size` cubed, and its clock runs `1 / sqrt(size)` times faster), so it walks the same way, at `sqrt(size)` of the speed |
| `forage` | true: it chooses its own target, the nearest `Food` of the world, and goes to eat it (see below) |
| `enabled` | false freezes it |
| `visible` | false hides the 65 entities that draw it, to draw it yourself |
| `distance`, `speed`, `height`, `upright`, `feet`, `web`, `tension` | **written by the engine every tick**: metres to the target (in 3D), speed (m/s), height of the prosoma above the ground, how upright it is (its up axis against the world's: on a wall this is about 0), how many feet touch a surface, whether a silk thread is held and its tension (newtons) |

While it lives the creature's own `Transform` rides on its prosoma (the front body), so `Transform.pos` is where the animal is: aim, attack when `distance` is small, put a camera on it. Its `Transform` before the first tick is where it is placed and the way its x axis points is the way it faces.

The engine makes the pieces that draw it: 50 entities with a `CreaturePart` (`index` 0 the prosoma, 1 the abdomen, then the six segments of each of the eight legs: coxa, femur, patella, tibia, metatarsus, tarsus) and 15 drawn-on details (eight eyes, fangs, palps, a stripe, spinnerets). They are plain `MeshRenderer`s of built-in shapes; a game with a model of its own sets `visible` to false and moves the bones of its model to the poses of the parts.

## Ground, and what moves it

The ground is every solid `Terrain` of the world (the first that covers the place) and, where there is none, the flat ground at the height where the animal was placed. The creature's physics is its own: a skeleton of 50 rigid bodies, 49 joints, muscles, hydraulics and claws, solved against the ground with warm-started sequential impulses in small steps (about a tenth of a millisecond a tick). Other rigid bodies do not push it and it does not push them. To move it from game code: `sim.creatures.shove(uid, [vx, vy, vz])` gives the whole body a sudden speed (a blow, a gust, an explosion); the brain was trained with such shoves and recovers.

Time: `time.scale` slows, pauses and speeds the creature like everything else (it runs whole ticks, carrying the fraction).

## Food and a house to live in

A `Food` entity (any `Transform`; draw it as you like) is something a foraging creature (`forage` true) goes to and eats: it walks, climbs and webs to the nearest one, and when its body is at the food (within a third of a body) the food appears again at one of the `FoodSpot`s (empty entities with a `Transform`, never one right beside the animal) and the creature goes for the nearest again. `Creature.eaten` counts what it ate, `Food.eaten` how often that food was eaten. `tools/build_spider_house.py` builds `examples/spider_house.lunaris.json`: a furnished two-room house (a table, a bookcase with three shelves, a sofa, a cabinet, a high ledge, a doorway) in which three small spiders live on a rug, with food spots on the floors, the furniture and the walls, and a camera that follows a spider. A small batch file on the owner's machine fetches the newest brain from the trainer and opens it in a window.

**Far targets.** A brain was trained on targets a few metres of its own size away (up to 6.5, mostly 1 to 2.5 creature-metres). A target farther than 3 creature-metres is shown to the brain as a nearer one in the same direction (a far one would be beyond anything it ever sensed and a net just stands still), so the creature walks towards it in stretches; the distance it reports is the true one.

## Walls, ceilings and silk

A spider is not tied to the ground. **Anything solid** in the world is a surface it walks on: every static box `Collider` (a wall, a ramp, a platform, a ceiling, turned any way) is part of the ground it senses and touches, next to the `Terrain`s. Its feet have **adhesive pads** (each foot can hold on with a force it chooses, the leg program's `r5`), so it can climb a wall with its legs alone, walk up a ramp or hang under a ceiling; and it can shoot **silk**: the body program chooses when to hold or cut a thread (`r28` above 500 holds it), which way to shoot it (`r27` the azimuth to the left and `r30` the elevation up, in milliradians, as the prosoma sees them), and whether to reel it in (`r29` positive) or pay it out. The thread leaves the spinnerets at the rear tip of the abdomen, sticks to the first surface within 4 metres along that line, and pulls like a stiff spring that holds up to about four times the animal's weight. Nothing here is scripted: the brain decides everything, and the trainer's climbing levels (ramps of 15 to 70 degrees, a wall, a tall wall) taught it. The thread is drawn as a thin white `CreaturePart` (`index` 200) from the spinnerets to the anchor, hidden when no thread is held.

## The spider

It is built after a real spider, and works like one:

- **Anatomy.** A prosoma and an abdomen on a pedicel that lets it nod; eight legs, four a side, pair I forwards to pair IV backwards, pair IV the longest; each leg has the real segments in order (the trochanter is fused with the femur's base). Joints: the coxa swings the leg forwards and backwards and raises and lowers it; the femur-patella, patella-tibia and tibia-metatarsus joints bend; the metatarsus-tarsus joint is passive. Five joints a leg are driven, forty in all.
- **Hydraulics.** A spider extends its legs with blood pressure, not with extensor muscles. Here one shared pressure sets the stance of all the bending joints at once (half pressure is the rest posture, more extends and raises the body, less crouches; it changes slowly, at most half its range a second, as blood does) and each leg's own brain moves the posture its lift joint and its three bending joints are held at, like a servo, so a leg can lift its own foot and flex its own knee; only the swing joint is driven by a muscle torque.
- **Grip.** The claws and the adhesive hairs of the tarsus grip far better than the shell.
- **Neural brain (alternative).** A champions file with `"kind":"nn"` (genomes `{"nn":[...]}`) holds a small recurrent neural net instead of programs: a leg net (31 inputs, 24 hidden units with recurrence, 6 outputs) run for each leg with that leg's own hidden state, and a body net (30 inputs, 16 hidden, 7 outputs: the pressure, hold, reel, aim azimuth, aim elevation and two signals), 2365 weights in all, reading the same senses as the programs and writing the same registers. It is trained by `spidergpu train_es` (evolution strategies: mirrored weight perturbations, rank-shaped scores, Adam), which in its first run learned flat walking and the 15 to 55 degree ramps from random weights in about ten minutes. The port is bit for bit with the trainer here too (`nn.rs`; the golden episodes include nets).
- **Brain.** Three programs of the Evolve-0 machine: one runs for each leg every tick (with the leg's own senses: its joints, its foot, its place in the walking rhythm, the direction of the target as its side sees it) and one for the body (the pressure and two signals). The senses do not depend on which way the animal faces, and include four feelers from the head (the distance to the nearest surface ahead, above, ahead-and-above and ahead-and-below, up to 1.5 body-metres), so that a brain can feel the edge of the block it is climbing. All of it is in the module notes of `crates/lunaris-creature/src/creature.rs`.

## Exactness

The trainer's physics and machine are ported line for line to Rust, with their own deterministic `sin`, `cos`, `atan2`, `exp` and `tanh` (only `+ - * /`, `floor` and `sqrt`) and no fused multiply-add, so the GPU, the trainer's CPU twin and `lunaris-creature` produce the same bits: a spider walks in the game as it walked in training, and the same on every machine. `crates/lunaris-creature/tests/parity.rs` runs the trainer's golden episodes (hand-written and random programs, flat ground and rough terrain with shoves) and compares every pose bit for bit. The creatures are part of the simulation state: `clone`, `checkpoint`/`rewind_to` (rollback netcode), `state_hash` and `Recording`s in memory include them. (A recording written to a file does not carry the creatures that were alive: a scene that has `Creature` entities makes them again.)

## Making a better one

`lunaris-creature` only runs brains. A brain can be written by hand: `bodybench/handgait` of the Evolve-0 project has a 26-instruction tetrapod gait (swing, lift and knee flexion from the leg's gait phase, steering as a gain on the stroke) that the trainer is seeded with. The trainer is `spidergpu` in the Evolve-0 project (`bodybench/spidergpu` of that project): it trains thousands of spiders at once on a GPU, rewarding walking to targets (smoothly, quickly, upright, on rough ground, through shoves), and writes `champions.json`; copy that into the workspace and name it in `Creature.brain`. `spidergpu train_es --out DIR [--pop 4096 --sigma 0.06 --lr 0.03 --init FILE]` trains a neural brain the same way and writes the same kind of file. `spidergpu export` writes the body model (`spider_model.f32`) and `spidergpu golden` the golden episodes for the tests.

## Tests

`cargo test -p lunaris-creature` (the port against the trainer, bit for bit) and `cargo test -p lunaris-sim creatures` (a spider in a simulation: made and drawn, standing, walking, following an entity, hashed, checkpointed, removed).
