# Sound: files, places in the world, and rays that find out what a room does to them

Lunaris plays sound the way it does everything else: as data an agent can create, read and **check without ears**. A game that makes sound needs three things, all of them components:

| Component | What it is |
|---|---|
| `SoundFile` | `{name, file}`: when something `emit`s the event `name`, the player plays the WAV `file`. With `spatial` (the default) it is placed where the emitting entity was. A `SoundFile` with an empty `file` makes the built-in voice of that name spatial too. |
| `AudioSource` | A sound that plays by itself from its entity's place (a campfire, a machine, music from a radio): `file` (or a built-in `voice`), `looped`, `playing`, `volume`, `pitch`. It follows its entity. |
| `AcousticMaterial` | `{absorption, transmission}` for a surface (an entity with a `Collider`): how much of a reflection's energy it loses (0.02 tile, 0.12 a furnished room, 0.5 thick curtains) and how much of the sound gets through a 0.2 m slab of it (0.03 a wall, 0.3 a wooden door). Without it a collider is a plain wall. |

`Sound` (the older component) still defines the synthesised voice of an event name (`coin`, `hit`, ...), and `{"emit": "coin"}` still just works with no files at all. The **listener** is the active camera.

```
{"entities": [
  {"uid": 1, "components": {"Name": {"value": "Camera"}, "Transform": {"pos": [0, 1.6, 0]}, "Camera": {}}},
  {"uid": 2, "components": {"Transform": {"pos": [8, 1, -5]}, "AudioSource": {"file": "sounds/radio.wav", "looped": true}}},
  {"uid": 3, "components": {"SoundFile": {"name": "door", "file": "sounds/door.wav", "near": 2, "range": 30}}}
]}
```

## Music: `MusicPlayer`

One entity with a `MusicPlayer` plays a playlist: the tracks one after another, the next fading in as the last fades out.

```json
{ "Name": {"value": "Music"},
  "MusicPlayer": { "playlist": "music/town.ogg
music/forest.ogg volume=0.8
# voice:ambient", "crossfade": 3, "shuffle": true } }
```

`playlist` is one track a line: a sound file of the workspace (WAV, Ogg or MP3) or `voice:NAME` (a built-in voice), optionally `volume=0.8`, `#` for a comment. `crossfade` is how many seconds neighbouring tracks overlap (equal power, so the loudness holds; 0 cuts; at most half a track), `volume` scales all of it, `shuffle` plays the list in a random order (fixed for the entity, never the same track twice in a row), `repeat` starts over at the end (false: the music stops after the last track). A graph system controls it by setting fields: `playing` false fades out and pauses, true starts again; adding one to `skip` crossfades to the next track at once (a boss fight: `{"set": "MusicPlayer.skip", "to": {"add": [{"get": "MusicPlayer.skip"}, 1]}}`; or swap the whole `playlist` text). Like all sound it only reads the game: nothing is written back, so the current track is not readable and music never changes a game's state. Tracks are routed through the mixer buses by file path (`route music/* -> music`), so a menu slider on `bus.music.volume` and ducking under voices work on it. `check` reports a track that is not a file; `record_audio` shows the crossfade in its timeline and counts `music:<track>` in `voices_started`.

## Mixer buses: `AudioMixer`

A game's sounds come in groups: music, effects, voices. One `AudioMixer` entity holds the groups as text:

```
bus master volume=1
bus music parent=master volume=0.7
bus sfx parent=master
bus voice parent=master duck=music:0.6
route music/* -> music
route voice:* -> voice
route * -> sfx
```

A **route** sends a sound to the first bus whose pattern fits its file path (`sounds/boom.ogg`) or, for a built-in voice, `voice:NAME`; `*` at the start or end matches anything. A bus has its own `volume`, then its `parent`'s, and so on up.
`duck=music:0.6` on the `voice` bus turns `music` down by 60 percent (quickly in, slowly out) while any sound on `voice` is heard. A settings menu sets the resources `bus.NAME.volume` (a `UiSlider` writes it with one graph statement)
and `bus.NAME.mute` (1 mutes). Looping `AudioSource`s follow the bus as it changes; a one-shot takes the volume it starts with. A mixer text with a mistake is a warning and the sounds play unrouted.

Sound files: WAV (8, 16, 24 or 32 bit PCM or 32/64 bit float), **Ogg Vorbis** (`.ogg`) and **MP3**, mono or stereo (more channels keep the first two), any sample rate; they are told apart by their first bytes, not by the name; paths are relative to the project folder (and cannot leave it). A file that is missing or damaged is a *warning* (the sound is skipped, the report says why), never a crash. `tools/make_sounds.py` makes the three small files the examples use from nothing but arithmetic.

## Where a sound is, and how it reaches you

A positioned sound is mixed like this (all in `crates/lunaris-audio`):

* **Distance**: full loudness inside `near` metres, `near / distance` beyond that (the inverse-distance law, 6 dB per doubling), the last quarter of `range` fading to nothing.
* **Left and right**: equal-power panning by the direction of the listener's right ear, so turning the camera moves the sound across the picture. Gains glide over each block of samples, so nothing clicks when a source moves.
* **Pitch and rate**: any file rate is resampled with linear interpolation; `pitch` plays it faster or slower.
* **32 voices at once**; a new one replaces the quietest voice that is not a loop.

## What the room does to it: tracing sound with rays

With acoustics on (the default) every positioned sound is traced through the scene's `Collider` boxes and spheres by a bounding-volume hierarchy (a scene of 10,000 colliders costs about 6 ms to analyse and about 0.3 ms per source). **This is geometric acoustics, a model and not a wave solver**; the rules are stated so that you know what to trust.

**The room around the listener** is measured by shooting 192 rays in every direction. Each ray bounces (mostly like a mirror, a third of the time scattered like a matte wall), loses the absorption of every surface it touches, and is lost for good when it leaves through an opening. The decay of the energy that is left, integrated backwards (Schroeder), gives the **reverberation time RT60** (the slope of the first 5 dB, the *early decay time*, which is what you hear when rooms are joined by doorways and the decay has two slopes). The share of directions in which something is within 60 m is the **closedness**: about 1 indoors, about 0.5 on a floor under open sky. The reverb (eight delay lines through a Hadamard matrix, with a low-pass in the loop so the highs die first) gets that RT60, damping from the mean absorption, a pre-delay from the nearest surface, and a level from the critical distance of a room that size, scaled by `closedness^4` so open air has none.

How close is it to the textbook? `cargo run --release -p lunaris-audio --example rooms`:

| room | Eyring's formula | traced |
|---|---|---|
| bathroom 3 x 2.5 x 3 m, absorption 0.03 | 2.48 s | 2.36 s |
| living room 6 x 3 x 5 m, 0.2 | 0.52 s | 0.49 s |
| classroom 9 x 3 x 8 m, 0.12 | 1.11 s | 1.08 s |
| sports hall 30 x 10 x 40 m, 0.1 | 4.83 s | 4.75 s |
| studio 5 x 3 x 4 m, 0.5 | 0.15 s | 0.12 s |
| a floor under open sky | (none) | 0.12 s, closedness 0.48 |

**What is in the way** of each source: rays go from the listener to seven points on a small sphere around it; every collider a ray passes through multiplies the amplitude by its `transmission` raised to (thickness / 0.2 m) (at least one 0.2 m slab), so thick walls pass less and a corner changes the sound gradually, not with a click. What gets through is low-passed: `20 kHz x (0.04 + 0.96 sqrt(transmission))`, and the air takes some of the highs over a distance.

**The first echoes** of the source: from the listener, 96 rays go out and bounce up to twice; at each hit the source is looked at again (visibility through other walls counts), and the loudness of that echo follows from the geometry of a matte patch (the patch a ray stands for, the angles, the distance to the source, what the surface absorbs). The echoes are merged into at most eleven taps, one per delay bucket from 5 ms to 250 ms, each with a delay, a level and a direction, so the first reflection off a wall to your right comes from your right.

Not modelled: sound bending around corners except through the echoes that happen to see both ends, moving sources' Doppler shift, the travel time of the direct sound, meshes without colliders (they do not stop sound), the triangles of a `MeshCollider` and the shape of a `ConvexCollider` (a level made of a model file muffles nothing yet: put plain `Collider` boxes in its walls for sound), non-box colliders' rotation (colliders are boxes that ignore rotation, as in physics), and one listener only.

## Hearing without ears

Two tools let an agent (or a test) verify sound.

`acoustics` reports what the scene does, without playing anything:

```
lunaris call acoustics '{"listener": [24, 1.6, 0], "source": [24, 1.2, -4]}' --scene examples/echo_tour.lunaris.json
```

returns the room (RT60, closedness, mean free path and absorption, a size estimate, the reverb level, and a one-line verdict such as "hall or cave: long reverberation") and the source (distance, how much direct sound survives, whether there is a clear line of sight, the low-pass in Hz, and the echoes with `delay_ms`, `level_db`, `azimuth_deg` and `elevation_deg`).

`record_audio` runs the simulation for N ticks, mixes everything exactly as the player would (the same code), writes a stereo 44.1 kHz WAV file, and reports the peak and the left and right loudness in dB, how many samples clipped, which sounds started and how often, the measured RT60, and **a loudness timeline every quarter of a second**, so you can check that a sound happens, *when*, and from which side:

```
lunaris call record_audio '{"ticks": 900, "path": "recordings/tour.wav"}' --scene examples/echo_tour.lunaris.json
```

`silent: true` in the report means nothing at all was heard (a missing file, a wrong event name, a listener out of range). `acoustics: false` records with positions and panning only, to hear what the rays add.

## In the native player

`lunaris play scene.json` plays through the sound card (Windows: the system's `waveOut`, four buffers of 512 frames, about 46 ms; Linux: PulseAudio's simple interface, 512-frame writes into a queue of about 25 ms, which PipeWire offers too); `--mute` switches it off and `--volume 0.3` turns it down (`--hidden --frames N --report` runs it without a window and prints which sounds started and the measured room). If there is no sound device the player says so once and carries on silent. The mixer, the tracing and `record_audio` work everywhere (they are plain Rust); the sound card output exists on Windows and Linux so far (macOS has none).

Sound is *output*: nothing in it is read by the simulation, so it never changes a state hash, and replays, lockstep multiplayer and the golden hashes are untouched. Events carry the position of their emitter at the moment of the `emit` (even for an entity that despawns in the same tick, like a picked-up coin), which is what places the sound.

## Example: `examples/echo_tour.lunaris.json`

A 17 second walk through a tiled bathroom (a dripping tap), a carpeted living room (a radio in a store room behind a wooden door), a concrete hall (handclaps) and a courtyard under the sky (handclaps in the distance). `lunaris play examples/echo_tour.lunaris.json` lets you listen; `tools/build_echo.py` builds it, `tools/make_sounds.py` makes the three sound files.

## From Rust

```rust
use lunaris_audio::{Clip, Mixer, Params, Spatial, wav};
let mut mixer = Mixer::new();
let clip = std::sync::Arc::new(wav::decode(&std::fs::read("clap.wav")?)?);
mixer.play(clip, Params { spatial: Some(Spatial::at([4.0, 1.0, -2.0])), ..Default::default() });
let samples = mixer.render_vec(44_100);                 // one second of interleaved stereo f32
// or into the sound card:
let shared = std::sync::Arc::new(std::sync::Mutex::new(mixer));
let _out = lunaris_audio::Output::open(shared.clone())?; // dropping it closes the device
```

`lunaris_audio::trace::{Scene, analyse_room, trace_source}` is the ray tracer on its own (boxes and spheres in, numbers out), and `lunaris_api::Director` is the glue that reads a running `Engine`.
