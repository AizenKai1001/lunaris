//! `PbrMaps`: the pictures a mesh names are loaded as data for the GPU renderer, the numbers are kept in range, a mesh without the component has none.
use super::*;
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

#[test]
fn a_mesh_with_pbr_maps_carries_them_into_the_scene_and_one_without_carries_none() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,6]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"Transform":{},"MeshRenderer":{"mesh":"cube"},"PbrMaps":{"normal":"checker","normal_strength":9.0,"ao_strength":-3.0,"roughness_scale":0.5}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[3,0,0]},"MeshRenderer":{"mesh":"cube"}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[6,0,0]},"MeshRenderer":{"mesh":"cube"},"PbrMaps":{}}"#);
    s.step_n(1);
    let mut tex = TextureCache::new(None);
    let scene = render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
    let by = |uid: u64| scene.meshes.iter().find(|m| m.uid == uid).unwrap_or_else(|| panic!("no mesh {uid}"));
    let p = by(2).pbr.as_ref().expect("the maps were asked for");
    assert!(p.normal.is_some() && p.arm.is_none());
    assert_eq!(p.normal_strength, 4.0, "the strength is kept in range");
    assert_eq!(p.ao_strength, 0.0);
    assert_eq!(p.roughness_scale, 0.5);
    assert!(by(3).pbr.is_none(), "no component, no maps");
    assert!(by(4).pbr.is_none(), "a component with no picture named is the same as none");
}

#[test]
fn a_grass_field_needs_a_terrain_to_grow_on() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,6]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"GrassField":{"mesh":"grass.glb"}}"#);
    s.step_n(1);
    let mut tex = TextureCache::new(None);
    let scene = render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
    assert!(scene.grass.is_empty(), "no terrain, no grass");
    assert!(scene.warnings.iter().any(|w| w.contains("GrassField.terrain")), "{:?}", scene.warnings);
}

#[test]
fn a_grass_field_with_no_terrain_named_grows_on_the_terrain_of_its_own_entity() {
    let dir = std::env::temp_dir().join(format!("lunaris-grassown-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 5, 5);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        enc.write_header().unwrap().write_image_data(&[0x40u8; 5 * 5 * 2]).unwrap();
    }
    std::fs::write(dir.join("flat.png"), out).unwrap();
    let mut s = Sim::new(1);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,6]},"Camera":{}}"#);
    // two tiles that each carry their own grass (as the entities of a streamed cell do: they cannot name each other), and a field with no terrain of its own entity
    spawn(&mut s, 2, r#"{"Transform":{"pos":[-50,0,0]},"Terrain":{"heightmap":"flat.png","size":[40,40],"height":5},"GrassField":{}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[50,0,0]},"Terrain":{"heightmap":"flat.png","size":[40,40],"height":5},"GrassField":{}}"#);
    spawn(&mut s, 4, r#"{"GrassField":{}}"#);
    s.step_n(1);
    let mut tex = TextureCache::new(Some(dir));
    let scene = render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
    let at = |uid: u64| scene.grass.iter().find(|g| g.uid == uid).unwrap_or_else(|| panic!("no grass {uid}: {:?}", scene.warnings)).terrain_pos[0];
    assert_eq!((at(2), at(3)), (-50.0, 50.0), "each grows on the terrain of its own entity");
    assert_eq!(at(4), -50.0, "with none of its own it takes the first terrain");
}

#[test]
fn a_stream_zone_about_to_come_into_range_has_its_files_read_ahead_and_one_out_of_range_or_already_loaded_has_not() {
    use crate::loader::Kind;
    let dir = std::env::temp_dir().join(format!("lunaris-prefetch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = |name: &str, w: u32, depth: png::BitDepth, color: png::ColorType, data: &[u8]| {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, w);
            enc.set_color(color);
            enc.set_depth(depth);
            enc.write_header().unwrap().write_image_data(data).unwrap();
        }
        std::fs::write(dir.join(name), out).unwrap();
    };
    for n in ["near.png", "far.png", "loaded.png"] {
        png(n, 8, png::BitDepth::Eight, png::ColorType::Rgb, &[100u8; 8 * 8 * 3]);
    }
    png("h.png", 5, png::BitDepth::Sixteen, png::ColorType::Grayscale, &[0x40u8; 5 * 5 * 2]);
    let mut s = Sim::new(1);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,0]},"Camera":{}}"#);
    // 500 m away with 600 m of prefetch beyond a 100 m radius: wanted; 5 km away: not; already loaded: not
    spawn(&mut s, 2, r#"{"Transform":{"pos":[500,0,0]},"StreamZone":{"prefab":"none","radius":100,"prefetch":600,"assets":"texture near.png\nheights h.png\nnonsense line"}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[5000,0,0]},"StreamZone":{"prefab":"none","radius":100,"prefetch":600,"assets":"texture far.png"}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[300,0,0]},"StreamZone":{"prefab":"none","radius":100,"prefetch":600,"loaded":true,"assets":"texture loaded.png"}}"#);
    let mut tex = TextureCache::new(Some(dir));
    render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
    let t0 = std::time::Instant::now();
    while tex.loading() > 0 && t0.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        tex.begin_frame();
    }
    tex.begin_frame();
    assert!(tex.is_resident(Kind::Texture, "near.png") && tex.is_resident(Kind::Heights, "h.png"), "read ahead, with no one having asked for them");
    assert!(!tex.is_resident(Kind::Texture, "far.png"), "too far to matter yet");
    assert!(!tex.is_resident(Kind::Texture, "loaded.png"), "its prefab is in the world already");
    assert!(tex.warnings.is_empty(), "{:?}", tex.warnings);
}
