//! A mesh as glTF 2.0: one primitive and one material per part. Photographed materials carry their colour, normal and occlusion-roughness-metallic pictures; plain ones a colour and a tiny
//! roughness-metallic picture (written beside the file), because the engine takes a material's roughness and metallic from that picture. `asset.extras.lunaris_lod` is `whole`: the engine's own
//! vertex-merging levels of detail are not wanted on a building; the generator makes its own.

use crate::materials::Lib;
use crate::mesh::MeshBuilder;
use lunaris_json::Json;
use std::collections::HashMap;
use std::io;
use std::path::Path;

fn flat_arm_name(rough: f32, metal: f32) -> String {
    format!("textures/_flat/arm_r{}_m{}.png", (rough * 100.0).round() as i32, (metal * 100.0).round() as i32)
}

fn ensure_flat_arm(dir: &Path, rough: f32, metal: f32) -> io::Result<String> {
    let rel = flat_arm_name(rough, metal);
    let path = dir.join(&rel);
    if !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let px = [255u8, (rough.clamp(0.0, 1.0) * 255.0).round() as u8, (metal.clamp(0.0, 1.0) * 255.0).round() as u8];
        let rgb: Vec<u8> = (0..16).flat_map(|_| px).collect();
        std::fs::write(&path, crate::png::encode_rgb(4, 4, &rgb))?;
    }
    Ok(rel)
}

fn f(v: f32) -> Json {
    Json::Float(v as f64)
}

struct Gltf {
    bin: Vec<u8>,
    views: Vec<Json>,
    images: Vec<Json>,
    textures: Vec<Json>,
    tex_of: HashMap<String, usize>,
}

impl Gltf {
    fn view(&mut self, bytes: &[u8], target: u32) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        self.views.push(Json::obj([("buffer", Json::from(0)), ("byteOffset", Json::from(self.bin.len())), ("byteLength", Json::from(bytes.len())), ("target", Json::from(target as i64))]));
        self.bin.extend_from_slice(bytes);
        self.views.len() - 1
    }

    fn texture(&mut self, uri: &str) -> usize {
        if let Some(i) = self.tex_of.get(uri) {
            return *i;
        }
        self.images.push(Json::obj([("uri", Json::str(uri))]));
        self.textures.push(Json::obj([("source", Json::from(self.images.len() - 1)), ("sampler", Json::from(0))]));
        self.tex_of.insert(uri.to_string(), self.textures.len() - 1);
        self.textures.len() - 1
    }
}

fn flat(v: &[[f32; 3]]) -> Vec<u8> {
    v.iter().flat_map(|p| p.iter().flat_map(|x| x.to_le_bytes())).collect()
}

/// Write `dir/<name>.gltf` and `dir/<name>.bin`. Returns the number of triangles written.
pub fn write(dir: &Path, name: &str, mesh: &MeshBuilder, lib: &Lib) -> io::Result<usize> {
    std::fs::create_dir_all(dir)?;
    let (text, bin, triangles) = assemble(mesh, lib, Some(dir), Some(&format!("{name}.bin")))?;
    std::fs::write(dir.join(format!("{name}.bin")), &bin)?;
    std::fs::write(dir.join(format!("{name}.gltf")), text)?;
    Ok(triangles)
}

/// The mesh as one binary glTF (`.glb`), in memory. The pictures of the plain materials' roughness (`ensure_flat_arms`) must exist beside where the model is loaded from.
pub fn to_glb(mesh: &MeshBuilder, lib: &Lib) -> io::Result<Vec<u8>> {
    let (text, bin, _) = assemble(mesh, lib, None, None)?;
    let mut json = text.into_bytes();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut bin = bin;
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    out.extend_from_slice(&bin);
    Ok(out)
}

/// The pictures far-off walls wear, under `dir/textures/_far/`: `far_win_l.png` is one bay of a facade (a window with a sill on plain wall: 3.6 m wide, one floor high) and `far_glass_l.png` one module of a curtain wall (a bright
/// spandrel band at the floor and a mullion on the dark glass). The vertex colours of the wall tint them; at a distance the mip levels average them to a plain wall that is a little darker than its colour. The alpha of
/// the pictures is the glass (255 over a window's glass, 0 elsewhere): the shader lights some of the windows at night (a material with a negative `emissiveFactor`, see `lit_mesh` in scene.wgsl) and the mips average the
/// alpha to the share of the wall that is window.
pub fn ensure_far_textures(dir: &Path) -> io::Result<()> {
    let out = dir.join("textures").join("_far");
    std::fs::create_dir_all(&out)?;
    let draw = |name: &str, f: &dyn Fn(f32, f32) -> [u8; 4]| -> io::Result<()> {
        let path = out.join(name);
        if path.exists() {
            return Ok(());
        }
        let n = 128u32;
        let mut rgba = Vec::with_capacity((n * n * 4) as usize);
        for y in 0..n {
            for x in 0..n {
                rgba.extend_from_slice(&f((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32));
            }
        }
        std::fs::write(path, crate::png::encode_rgba(n, n, &rgba))
    };
    // (u, v) in 0..1, v counted from the top of the picture; the fourth number is the glass
    draw("far_win_l.png", &|u, v| {
        let in_win = (0.30..=0.70).contains(&u) && (0.26..=0.72).contains(&v);
        if in_win {
            if (u - 0.5).abs() < 0.012 || (v - 0.49).abs() < 0.012 { [205, 205, 205, 0] } else { [58, 70, 88, 255] }
        } else if (0.72..0.77).contains(&v) && (0.27..=0.73).contains(&u) {
            [238, 238, 238, 0]
        } else if v > 0.94 {
            [236, 236, 236, 0]
        } else {
            [255, 255, 255, 0]
        }
    })?;
    draw("far_glass_l.png", &|u, v| {
        if v > 0.74 {
            [214, 220, 228, 0]
        } else if !(0.02..=0.98).contains(&u) {
            [150, 158, 170, 0]
        } else {
            [78, 92, 112, 255]
        }
    })
}

/// Write the tiny roughness-metallic pictures that every plain material of the library needs, under `dir/textures/_flat/`.
pub fn ensure_flat_arms(dir: &Path, lib: &Lib) -> io::Result<()> {
    for m in &lib.mats {
        if m.arm.is_none() {
            ensure_flat_arm(dir, m.rough, m.metal)?;
        }
    }
    Ok(())
}

fn assemble(mesh: &MeshBuilder, lib: &Lib, flat_dir: Option<&Path>, buffer_uri: Option<&str>) -> io::Result<(String, Vec<u8>, usize)> {
    let mut g = Gltf { bin: Vec::new(), views: Vec::new(), images: Vec::new(), textures: Vec::new(), tex_of: HashMap::new() };
    let mut accs: Vec<Json> = Vec::new();
    let mut prims: Vec<Json> = Vec::new();
    let mut mats: Vec<Json> = Vec::new();
    let mut triangles = 0usize;
    for (mi, part) in mesh.parts.iter().enumerate() {
        if part.idx.is_empty() {
            continue;
        }
        triangles += part.idx.len() / 3;
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &part.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let n = part.pos.len();
        let vp = g.view(&flat(&part.pos), 34962);
        accs.push(Json::obj([("bufferView", Json::from(vp)), ("componentType", Json::from(5126)), ("count", Json::from(n)), ("type", Json::str("VEC3")), ("min", Json::arr(lo.map(f))), ("max", Json::arr(hi.map(f)))]));
        let a_pos = accs.len() - 1;
        let vn = g.view(&flat(&part.nrm), 34962);
        accs.push(Json::obj([("bufferView", Json::from(vn)), ("componentType", Json::from(5126)), ("count", Json::from(n)), ("type", Json::str("VEC3"))]));
        let a_nrm = accs.len() - 1;
        let uvb: Vec<u8> = part.uv.iter().flat_map(|p| p.iter().flat_map(|x| x.to_le_bytes())).collect();
        let vu = g.view(&uvb, 34962);
        accs.push(Json::obj([("bufferView", Json::from(vu)), ("componentType", Json::from(5126)), ("count", Json::from(n)), ("type", Json::str("VEC2"))]));
        let a_uv = accs.len() - 1;
        let vc = g.view(&flat(&part.col), 34962);
        accs.push(Json::obj([("bufferView", Json::from(vc)), ("componentType", Json::from(5126)), ("count", Json::from(n)), ("type", Json::str("VEC3"))]));
        let a_col = accs.len() - 1;
        let ib: Vec<u8> = part.idx.iter().flat_map(|x| x.to_le_bytes()).collect();
        let vi = g.view(&ib, 34963);
        accs.push(Json::obj([("bufferView", Json::from(vi)), ("componentType", Json::from(5125)), ("count", Json::from(part.idx.len())), ("type", Json::str("SCALAR"))]));
        let a_idx = accs.len() - 1;
        let def = &lib.mats[mi];
        let mut pbr: Vec<(&str, Json)> = Vec::new();
        if let Some(d) = &def.diff {
            let t = g.texture(d);
            pbr.push(("baseColorTexture", Json::obj([("index", Json::from(t))])));
            pbr.push(("baseColorFactor", Json::arr([f(1.0), f(1.0), f(1.0), f(1.0)])));
        } else {
            pbr.push(("baseColorFactor", Json::arr([f(def.color[0]), f(def.color[1]), f(def.color[2]), f(1.0)])));
        }
        let arm_uri = match (&def.arm, flat_dir) {
            (Some(a), _) => a.clone(),
            (None, Some(d)) => ensure_flat_arm(d, def.rough, def.metal)?,
            (None, None) => flat_arm_name(def.rough, def.metal),
        };
        let ta = g.texture(&arm_uri);
        pbr.push(("metallicRoughnessTexture", Json::obj([("index", Json::from(ta))])));
        pbr.push(("metallicFactor", f(1.0)));
        pbr.push(("roughnessFactor", f(1.0)));
        let mut m: Vec<(&str, Json)> = vec![("name", Json::str(def.name.clone())), ("pbrMetallicRoughness", Json::obj(pbr))];
        if def.double_sided {
            m.push(("doubleSided", Json::Bool(true)));
        }
        if def.name.starts_with("glass_lit") {
            m.push(("emissiveFactor", Json::arr([Json::Float(1.0), Json::Float(1.0), Json::Float(1.0)])));
        }
        // far walls: a negative factor asks the shader for night lights in the windows (the alpha of their picture is the glass, the shader chooses which windows are lit)
        if def.name == "far_win" || def.name == "far_glass" {
            m.push(("emissiveFactor", Json::arr([Json::Float(-1.0), Json::Float(-1.0), Json::Float(-1.0)])));
        }
        if def.alpha_cutoff > 0.0 {
            m.push(("alphaMode", Json::str("MASK")));
            m.push(("alphaCutoff", f(def.alpha_cutoff)));
        }
        if let Some(nm) = &def.nor {
            let t = g.texture(nm);
            m.push(("normalTexture", Json::obj([("index", Json::from(t))])));
        }
        mats.push(Json::obj(m));
        prims.push(Json::obj([
            ("attributes", Json::obj([("POSITION", Json::from(a_pos)), ("NORMAL", Json::from(a_nrm)), ("TEXCOORD_0", Json::from(a_uv)), ("COLOR_0", Json::from(a_col))])),
            ("indices", Json::from(a_idx)),
            ("material", Json::from(mats.len() - 1)),
        ]));
    }
    let mut extras = vec![("lunaris_lod", Json::str("whole"))];
    if !mesh.occluders.is_empty() {
        extras.push(("lunaris_occluders", Json::Arr(mesh.occluders.iter().map(|o| Json::Arr(o.iter().map(|x| Json::Float(*x as f64)).collect())).collect())));
    }
    let mut doc: Vec<(&str, Json)> = vec![
        ("asset", Json::obj([("version", Json::str("2.0")), ("generator", Json::str("lunaris-citygen")), ("extras", Json::obj(extras))])),
        ("scene", Json::from(0)),
        ("scenes", Json::arr([Json::obj([("nodes", Json::arr([Json::from(0)]))])])),
        ("nodes", Json::arr([Json::obj([("mesh", Json::from(0))])])),
        ("meshes", Json::arr([Json::obj([("primitives", Json::Arr(prims))])])),
        ("materials", Json::Arr(mats)),
        ("buffers", Json::arr([match buffer_uri {
            Some(u) => Json::obj([("uri", Json::str(u)), ("byteLength", Json::from(g.bin.len()))]),
            None => Json::obj([("byteLength", Json::from(g.bin.len()))]),
        }])),
        ("bufferViews", Json::Arr(g.views.clone())),
        ("accessors", Json::Arr(accs)),
        ("samplers", Json::arr([Json::obj([("magFilter", Json::from(9729)), ("minFilter", Json::from(9987)), ("wrapS", Json::from(10497)), ("wrapT", Json::from(10497))])])),
    ];
    if !g.images.is_empty() {
        doc.push(("images", Json::Arr(g.images.clone())));
        doc.push(("textures", Json::Arr(g.textures.clone())));
    }
    let text = Json::obj(doc).to_string_compact().map_err(|e| io::Error::new(io::ErrorKind::Other, e.0))?;
    Ok((text, g.bin, triangles))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Tex;

    #[test]
    fn a_written_model_parses_back_with_its_materials_and_pictures() {
        let lib = Lib::from_json(r#"{"brick_wall_001":{"size_m":[3.0,3.0]}}"#);
        let mut m = MeshBuilder::new();
        let t = Tex { size: [3.0, 3.0], off: [0.0, 0.0] };
        let brick = lib.id("brick_wall_001");
        m.quad_tex(brick, [[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 3.0, 0.0], [0.0, 3.0, 0.0]], [[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0]], &t, [1.0; 3], [0.0, 0.0, 1.0]);
        m.quad_tex(lib.id("glass_dark"), [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]], [[0.0; 2]; 4], &t, [1.0; 3], [0.0, 0.0, 1.0]);
        let dir = std::env::temp_dir().join("lunaris_citygen_gltf_test");
        let _ = std::fs::remove_dir_all(&dir);
        let tris = write(&dir, "unit", &m, &lib).unwrap();
        assert_eq!(tris, 4);
        let doc = lunaris_json::parse(&std::fs::read_to_string(dir.join("unit.gltf")).unwrap()).unwrap();
        assert_eq!(doc.get("materials").unwrap().as_array().unwrap().len(), 2);
        let images: Vec<&str> = doc.get("images").unwrap().as_array().unwrap().iter().map(|i| i.get("uri").unwrap().as_str().unwrap()).collect();
        assert!(images.contains(&"textures/brick_wall_001/diff.jpg") && images.contains(&"textures/brick_wall_001/nor.jpg"));
        assert!(images.iter().any(|u| u.starts_with("textures/_flat/arm_r6_m0")));
        assert!(dir.join("textures/_flat/arm_r6_m0.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_glb_has_its_header_json_and_binary_chunks_that_add_up() {
        let lib = Lib::from_json("");
        let mut m = MeshBuilder::new();
        let t = Tex { size: [1.0, 1.0], off: [0.0, 0.0] };
        m.quad_tex(lib.id("glass_dark"), [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]], [[0.0; 2]; 4], &t, [1.0; 3], [0.0, 0.0, 1.0]);
        let glb = to_glb(&m, &lib).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
        let jlen = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        let json = std::str::from_utf8(&glb[20..20 + jlen]).unwrap();
        let doc = lunaris_json::parse(json).unwrap();
        assert!(doc.get("buffers").unwrap().as_array().unwrap()[0].get("uri").is_none());
        let bstart = 20 + jlen;
        assert_eq!(&glb[bstart + 4..bstart + 8], b"BIN\0");
        assert_eq!(bstart + 8 + u32::from_le_bytes(glb[bstart..bstart + 4].try_into().unwrap()) as usize, glb.len());
    }
}
