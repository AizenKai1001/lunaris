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
