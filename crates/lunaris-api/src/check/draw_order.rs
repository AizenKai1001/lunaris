//! `check` rules for the drawing order of 2D things (`DrawLayer`, docs/GAMEPLAY.md "Draw order"): a layer that is not a number, and a `DrawLayer` on an
//! entity it cannot order (a `Ui` or `Text` is the screen's overlay, a 3D mesh is depth-tested: neither is sorted by it).
use super::Problems;
use lunaris_ecs::{Result, World};
use lunaris_sim::components::{Decal, DrawLayer, MeshRenderer, Particles, Sprite, Text, Tilemap, Ui};

pub(super) fn run(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut layered: Vec<(u64, f32)> = Vec::new();
    world.for_each_uid::<(&DrawLayer,), _>(|_, uid, (d,)| layered.push((uid, d.layer)))?;
    layered.sort_by_key(|l| l.0);
    for (uid, layer) in layered {
        if !layer.is_finite() {
            problems.add("warning", Some(uid), format!("DrawLayer.layer is {layer}, which is not a number the order can use: the entity is drawn as layer 0"), "set `layer` to an ordinary number: lower is drawn first, the highest is on top");
        }
        let Some(e) = world.entity(uid) else { continue };
        let orders = world.get::<Sprite>(e).is_some() || world.get::<Tilemap>(e).is_some() || world.get::<Decal>(e).is_some() || world.get::<Particles>(e).is_some();
        if orders {
            continue;
        }
        let (message, hint) = if world.get::<Ui>(e).is_some() || world.get::<Text>(e).is_some() {
            (
                "has a DrawLayer, but a Ui or a Text is the screen's overlay: it is drawn over the whole picture in the order of `Ui.layer` and uid, and DrawLayer does not move it between the sprites",
                "use `Ui.layer` to order interface elements; DrawLayer orders Sprite, Tilemap, Decal and Particles",
            )
        } else if world.get::<MeshRenderer>(e).is_some() {
            (
                "has a DrawLayer, but a 3D mesh is drawn by depth (what is in front hides what is behind), not by layer",
                "DrawLayer orders the flat 2D things: Sprite, Tilemap, Decal and Particles",
            )
        } else {
            ("has a DrawLayer but nothing it orders (no Sprite, Tilemap, Decal or Particles)", "put it on the entity that has the Sprite, Tilemap, Decal or Particles")
        };
        problems.add("warning", Some(uid), message.to_string(), hint);
    }
    Ok(())
}
