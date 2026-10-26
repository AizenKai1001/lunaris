//! Cameras that draw into a picture instead of the screen: a minimap, a security monitor, a mirror, a portrait of a character, a scope. A `CameraView` entity
//! says which camera to look through and how big the picture is; before each main picture the views are drawn (by the reference renderer, with no interface and
//! no views of their own) and kept under the name `view:UID` (UID the view entity's own uid) like any other texture: a `Ui` picture (`UiStyle.image`), the
//! texture of a mesh, a sprite. A view that looks at a monitor showing itself sees the monitor as it was one picture ago, so feedback never loops.
use crate::render::{render, RenderOptions, View};
use crate::texture::{Texture, TextureCache};
use lunaris_ecs::{Result, World};
use lunaris_sim::components::CameraView;

/// The name of the texture a `CameraView` entity draws into.
pub fn view_name(uid: u64) -> String {
    format!("view:{uid}")
}

/// A finished picture as a texture: display colours squared into the working space, row 0 at the top.
fn texture_of(rgba: &[u8], width: u32, height: u32) -> Texture {
    let texels = rgba.chunks_exact(4).map(|p| {
        let c = |v: u8| (v as f32 / 255.0) * (v as f32 / 255.0);
        [c(p[0]), c(p[1]), c(p[2]), 1.0]
    });
    Texture { width, height, texels: crate::texture::Texels::F32(texels.collect()), heights: None }
}

/// Draw the views that are due. Called at the start of every main picture.
pub(crate) fn update(world: &mut World, textures: &mut TextureCache, threads: usize) -> Result<()> {
    let mut views: Vec<(u64, CameraView)> = Vec::new();
    world.for_each_uid::<(&CameraView,), _>(|_, uid, (v,)| views.push((uid, *v)))?;
    views.sort_by_key(|v| v.0);
    for (uid, v) in views {
        if !v.active {
            continue;
        }
        let name = view_name(uid);
        // one picture in `every`, the first one always
        let seen = textures.view_tick(uid);
        if seen % v.every.clamp(1, 600) as u64 != 0 {
            continue;
        }
        let (w, h) = (v.width.clamp(16, 1024), v.height.clamp(16, 1024));
        if world.entity(v.camera.0).is_none() {
            let msg = format!("CameraView {uid}: there is no entity {} to look through (its `camera`)", v.camera.0);
            if !textures.warnings.contains(&msg) {
                textures.warnings.push(msg);
            }
            continue;
        }
        let opts = RenderOptions { width: w, height: h, view: View::World(Some(v.camera.0)), supersample: 1, threads };
        let was = std::mem::replace(&mut textures.nested, true);
        let frame = render(world, textures, &opts);
        textures.nested = was;
        match frame {
            Ok(f) => {
                for w in &f.warnings {
                    if !textures.warnings.contains(w) {
                        textures.warnings.push(w.clone());
                    }
                }
                textures.put(&name, texture_of(&f.rgba, f.width, f.height));
            }
            Err(e) => {
                let msg = format!("CameraView {uid}: {e}");
                if !textures.warnings.contains(&msg) {
                    textures.warnings.push(msg);
                }
            }
        }
    }
    Ok(())
}
