//! `check` rules for painted terrain (`TerrainLayers`, docs/TERRAIN.md "Layers"): the splat map must be a picture the engine can read, the layer text must parse, every layer must have a
//! texture and a channel to be weighed by, and a layer that is nowhere on the ground is probably the wrong channel.
use super::{file_exists, Problems};
use lunaris_ecs::{Result, World};
use lunaris_render::terrain_layers::{parse_layers, LAYER_SYNTAX};
use lunaris_render::{is_procedural_texture, TextureCache};
use lunaris_sim::components::{Terrain, TerrainLayers};
use std::path::Path;

/// The channels of a splat map, for messages.
const CHANNELS: [&str; 4] = ["red", "green", "blue", "alpha"];

/// The most texels `check` looks at to find the layers that are nowhere on the ground (a map of 64 million texels is looked at every few rows and columns).
const MAX_SCAN: usize = 1_000_000;

pub(super) fn run(world: &mut World, textures: &mut TextureCache, root: &Path, problems: &mut Problems) -> Result<()> {
    let mut painted: Vec<(u64, TerrainLayers)> = Vec::new();
    world.for_each_uid::<(&TerrainLayers,), _>(|_, uid, (l,)| painted.push((uid, *l)))?;
    painted.sort_by_key(|p| p.0);
    for (uid, l) in painted {
        let terrain = world.entity(uid).and_then(|e| world.get::<Terrain>(e).copied());
        let (splat_name, text) = (world.strings().get(l.splat.0).to_string(), world.strings().get(l.layers.0).to_string());
        match terrain {
            None => problems.add("warning", Some(uid), "has TerrainLayers but no Terrain, so there is no ground to paint".into(), "put TerrainLayers on the entity that has the Terrain"),
            Some(g) => {
                if g.color[..3] == [0.34, 0.48, 0.27] {
                    problems.add("warning", Some(uid), "Terrain.color is still the default green, which multiplies every layer: sand and snow come out green".into(), "set Terrain.color to [1, 1, 1, 1] to see the layers as they are (or to a light tint over all of them)");
                }
                let own = world.strings().get(g.texture.0);
                if !own.is_empty() {
                    problems.add("warning", Some(uid), format!("Terrain.texture `{own}` is not used while the terrain has TerrainLayers: the layers are its texture"), "remove Terrain.texture, or move it into the layers text as a layer");
                }
            }
        }
        // the layers
        let specs = match parse_layers(&text) {
            Ok(s) => s,
            Err(e) => {
                problems.add("error", Some(uid), format!("TerrainLayers.layers: {e} (the terrain is drawn with its own texture)"), &format!("one layer a line: {LAYER_SYNTAX}; at most 4"));
                Vec::new()
            }
        };
        for (k, s) in specs.iter().enumerate() {
            if !is_procedural_texture(&s.texture) && !file_exists(root, &s.texture) {
                problems.add("warning", Some(uid), format!("layer {} (`{}`): the texture `{}` is not a built-in material or a file of the workspace, so the magenta placeholder is drawn", k + 1, s.name, s.texture), "grass, dirt, stone, bricks, wood, sand, concrete, tiles, metal, or the path of a PNG or JPEG relative to the workspace folder (`files` lists them)");
            }
        }
        // the splat map
        if splat_name.is_empty() {
            problems.add("error", Some(uid), "TerrainLayers.splat is empty, so there is nothing to weigh the layers by (the terrain is drawn with its own texture)".into(), "name an RGBA PNG of the workspace: red weighs layer 1, green layer 2, blue layer 3, alpha layer 4");
            continue;
        }
        if !file_exists(root, &splat_name) {
            problems.add("error", Some(uid), format!("the splat map `{splat_name}` is not a file of the workspace (the terrain is drawn with its own texture)"), "an RGBA PNG: red weighs layer 1, green layer 2, blue layer 3, alpha layer 4; `files` lists the pictures");
            continue;
        }
        let before = textures.warnings.len();
        let map = textures.splat(&splat_name);
        let why = textures.warnings.split_off(before).join("; ");
        let Some(map) = map else {
            problems.add("error", Some(uid), format!("the splat map `{splat_name}` could not be read: {why} (the terrain is drawn with its own texture)"), "an RGBA PNG (8 or 16 bits) inside the workspace folder");
            continue;
        };
        for (k, s) in specs.iter().enumerate().take(4) {
            if k >= map.channels {
                problems.add(
                    "warning",
                    Some(uid),
                    format!("layer {} (`{}`) is weighed by the {} channel of the splat map, but `{splat_name}` has {} channel{}, so the layer is never shown", k + 1, s.name, CHANNELS[k], map.channels, if map.channels == 1 { "" } else { "s" }),
                    "save the splat map as RGBA (or take the layer out of the text)",
                );
            }
        }
        // a layer that is nowhere on the ground
        let stride = ((map.cols * map.rows) as f64 / MAX_SCAN as f64).sqrt().ceil().max(1.0) as usize;
        let mut most = [0u8; 4];
        for y in (0..map.rows).step_by(stride) {
            for x in (0..map.cols).step_by(stride) {
                let t = map.texels[y * map.cols + x];
                for k in 0..4 {
                    most[k] = most[k].max(t[k]);
                }
            }
        }
        for (k, s) in specs.iter().enumerate().take(map.channels.min(4)) {
            if most[k] < 8 {
                problems.add("warning", Some(uid), format!("layer {} (`{}`) never shows: the {} channel of `{splat_name}` is empty everywhere", k + 1, s.name, CHANNELS[k]), "paint some weight into that channel, or take the layer out");
            }
        }
    }
    Ok(())
}
