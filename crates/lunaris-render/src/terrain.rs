//! The mesh of a `Terrain`: one vertex for every `step`-th sample of its heightmap, two triangles per cell, the same triangles that physics
//! uses (`lunaris_sim::terrain`). The mesh is in the terrain's own space: x and z from -size/2 to size/2, y from 0 (black) to `height`; the
//! entity's position moves it. Smooth normals come from the neighbouring vertices; texture coordinates run 0 to 1 over the whole ground
//! (the component's `uv_scale` repeats them).
use crate::math::{cross, normalize, sub};
use crate::mesh::Mesh;
use lunaris_sim::components::Terrain;
use lunaris_sim::terrain::{Heightfield, Placed};

pub fn build_mesh(field: &Heightfield, g: &Terrain) -> Mesh {
    let p = Placed::new(field, [0.0, 0.0, 0.0], g);
    let (nx, nz) = (p.nx, p.nz);
    let stride = nx + 1;
    let mut pos = Vec::with_capacity(stride * (nz + 1));
    let mut nrm = Vec::with_capacity(stride * (nz + 1));
    let mut uv = Vec::with_capacity(stride * (nz + 1));
    for j in 0..=nz {
        for i in 0..=nx {
            pos.push(p.vertex(i, j));
            let (l, r) = (p.vertex(i.saturating_sub(1), j), p.vertex((i + 1).min(nx), j));
            let (u, d) = (p.vertex(i, j.saturating_sub(1)), p.vertex(i, (j + 1).min(nz)));
            let n = normalize(cross(sub(d, u), sub(r, l)));
            // a flat or degenerate neighbourhood: straight up
            nrm.push(if n[1].is_finite() && n[1] > 0.0 { n } else { [0.0, 1.0, 0.0] });
            uv.push([i as f32 / nx as f32, j as f32 / nz as f32]);
        }
    }
    let mut idx = Vec::with_capacity(nx * nz * 6);
    for j in 0..nz {
        for i in 0..nx {
            let v00 = (j * stride + i) as u32;
            let (v10, v01, v11) = (v00 + 1, v00 + stride as u32, v00 + stride as u32 + 1);
            idx.extend_from_slice(&[v00, v01, v10, v10, v01, v11]);
        }
    }
    Mesh { pos, nrm, uv, col: Vec::new(), texw: Vec::new(), idx, tex: None, rig: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mesh_is_the_ground_physics_stands_on_and_faces_up() {
        let mut s = vec![0u16; 25];
        s[12] = 65535; // a peak in the middle of a 5 x 5 field
        let f = Heightfield::new(5, 5, s).unwrap();
        let g = Terrain { size: [40.0, 40.0], height: 20.0, step: 1, ..Terrain::default() };
        let m = build_mesh(&f, &g);
        assert_eq!((m.pos.len(), m.idx.len()), (25, 4 * 4 * 6));
        assert_eq!(m.pos[0], [-20.0, 0.0, -20.0]);
        assert!((m.pos[12][1] - 20.0).abs() < 1e-3 && m.pos[12][0].abs() < 1e-4 && m.pos[12][2].abs() < 1e-4);
        // every triangle faces up, and the mesh height anywhere agrees with the ground that physics uses
        let p = Placed::new(&f, [0.0, 0.0, 0.0], &g);
        for t in m.idx.chunks(3) {
            let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
            let n = cross(sub(b, a), sub(c, a));
            assert!(n[1] > 0.0, "{a:?} {b:?} {c:?}");
            let centre = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0];
            let ground = p.surface(centre[0], centre[2]).unwrap();
            assert!((ground - centre[1]).abs() < 1e-3, "the triangle at {centre:?}, the ground gives {ground}");
        }
        // the normals lean away from the peak
        assert!(m.nrm[11][0] < -0.1 && m.nrm[13][0] > 0.1 && m.nrm[0][1] > 0.99, "{:?} {:?}", m.nrm[11], m.nrm[13]);
        // a coarser step has fewer vertices
        let coarse = build_mesh(&f, &Terrain { step: 2, ..g });
        assert_eq!(coarse.pos.len(), 9);
    }

    /// A 65 x 65 cone (16 bits, written by the png crate: real compression), in a fresh workspace folder.
    fn cone_workspace(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lunaris-render-terrain-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut bytes = Vec::new();
        for z in 0..65 {
            for x in 0..65 {
                let d = (((x as f32 - 32.0).powi(2) + (z as f32 - 32.0).powi(2)).sqrt() / 32.0).min(1.0);
                bytes.extend_from_slice(&(((1.0 - d) * 65535.0) as u16).to_be_bytes());
            }
        }
        let mut data = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut data, 65, 65);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Sixteen);
            enc.write_header().unwrap().write_image_data(&bytes).unwrap();
        }
        std::fs::write(dir.join("cone.png"), data).unwrap();
        dir
    }

    fn world_with_cone() -> lunaris_sim::Sim {
        use lunaris_json::Json;
        let mut s = lunaris_sim::Sim::new(1);
        let Json::Obj(components) = lunaris_json::parse(r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"cone.png","size":[64,64],"height":20,"color":[0.3,0.6,0.3,1]}}"#).unwrap() else { panic!() };
        s.apply(&lunaris_sim::Command::Spawn { uid: Some(1), components }).unwrap();
        s
    }

    #[test]
    fn a_terrain_is_drawn_picked_and_hit_by_rays_and_a_compressed_png_is_read_by_the_decoder_of_the_simulation() {
        use crate::{raycast_with, render, ExplicitCamera, RayTargets, RenderOptions, TextureCache, View};
        let dir = cone_workspace("draw");
        let mut s = world_with_cone();
        let mut tex = TextureCache::new(Some(dir.clone()));
        // the cone is where physics says it is
        let field = tex.heightfield("cone.png").expect("read");
        assert_eq!((field.cols, field.rows), (65, 65));
        let g = *s.world.get::<Terrain>(s.world.entity(1).unwrap()).unwrap();
        let p = Placed::new(&field, [0.0, 0.0, 0.0], &g);
        assert!((p.surface(0.0, 0.0).unwrap() - 20.0).abs() < 0.05, "{:?}", p.surface(0.0, 0.0));
        // the picture: the ground is the entity at the middle and at the bottom of the frame, and the sky shows above it
        let view = View::Explicit(ExplicitCamera { eye: [0.0, 45.0, 40.0], target: [0.0, 5.0, 0.0], ..ExplicitCamera::default() });
        let f = render(&mut s.world, &mut tex, &RenderOptions { width: 160, height: 90, view, ..RenderOptions::default() }).unwrap();
        assert_eq!(f.pick(80, 45), Some(1));
        assert_eq!(f.pick(80, 85), Some(1));
        assert!(f.coverage() > 0.25, "coverage {}", f.coverage());
        // rays: down onto the peak, and onto the slope; none outside the ground
        let hit = raycast_with(&mut s.world, &mut tex, [0.0, 100.0, 0.0], [0.0, -1.0, 0.0], 200.0, RayTargets::Both).unwrap().unwrap();
        assert!(hit.uid == 1 && (hit.point[1] - 20.0).abs() < 0.1 && hit.normal[1] > 0.5, "{hit:?}");
        let slope = raycast_with(&mut s.world, &mut tex, [16.0, 100.0, 0.0], [0.0, -1.0, 0.0], 200.0, RayTargets::Both).unwrap().unwrap();
        assert!(slope.point[1] > 8.0 && slope.point[1] < 12.0 && slope.normal[0] > 0.2, "{slope:?}");
        assert!(raycast_with(&mut s.world, &mut tex, [80.0, 100.0, 0.0], [0.0, -1.0, 0.0], 200.0, RayTargets::Both).unwrap().is_none());
        // `visible` and `solid` choose which kinds of ray see it
        s.world.get_mut::<Terrain>(s.world.entity(1).unwrap()).unwrap().solid = false;
        assert!(raycast_with(&mut s.world, &mut tex, [0.0, 100.0, 0.0], [0.0, -1.0, 0.0], 200.0, RayTargets::Colliders).unwrap().is_none());
        assert!(raycast_with(&mut s.world, &mut tex, [0.0, 100.0, 0.0], [0.0, -1.0, 0.0], 200.0, RayTargets::Visuals).unwrap().is_some());
        s.world.get_mut::<Terrain>(s.world.entity(1).unwrap()).unwrap().visible = false;
        assert!(raycast_with(&mut s.world, &mut tex, [0.0, 100.0, 0.0], [0.0, -1.0, 0.0], 200.0, RayTargets::Both).unwrap().is_none());
    }

    #[test]
    fn a_missing_heightmap_is_a_warning_and_an_empty_picture_not_a_failure() {
        use crate::{render, RenderOptions, TextureCache};
        let mut s = world_with_cone();
        let mut tex = TextureCache::new(Some(std::env::temp_dir().join("lunaris-no-such-workspace-folder")));
        let f = render(&mut s.world, &mut tex, &RenderOptions { width: 32, height: 24, ..RenderOptions::default() }).unwrap();
        assert_eq!(f.pick(16, 12), None);
        assert!(f.warnings.iter().any(|w| w.contains("terrain cone.png")), "{:?}", f.warnings);
        // asking again for a while neither reads the folder again nor piles up the warning
        for _ in 0..5 {
            let _ = tex.heightfield("cone.png");
        }
        assert!(tex.warnings.is_empty(), "{:?}", tex.warnings);
    }
}
