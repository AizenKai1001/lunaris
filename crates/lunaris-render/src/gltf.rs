//! A glTF 2.0 importer (`.glb` and `.gltf`) written for this engine, with no dependencies beyond the JSON parser and the PNG
//! decoder already in use. It flattens a model into one `Mesh`: every mesh node of the default scene, transformed by its
//! node matrix, with the material's base colour as a vertex colour and the first PNG base-colour texture it finds.
//!
//! Supported: triangle lists, strips and fans; indexed or not; `POSITION`, `NORMAL` (flat normals are made when missing),
//! `TEXCOORD_0`, `COLOR_0`; node hierarchies with matrices or translation/rotation/scale; embedded buffers (GLB chunk, `data:`
//! URIs) and relative files inside the workspace; PNG textures; the first skin (joints, inverse bind matrices, `JOINTS_0` and
//! `WEIGHTS_0`) and every animation of translation, rotation and scale (linear, step and cubic spline) of its joints, kept in a
//! `Rig` (see `rig.rs`; the mesh itself is skinned to the rest pose). Not supported (reported as warnings, the rest still loads):
//! more than one skin, morph targets, animation of anything but joints, sparse accessors, JPEG textures, texture transforms, alpha
//! blending.
use crate::math::*;
use crate::mesh::Mesh;
use crate::rig::{decompose, Clip, Interp, Joint, Path as AnimPath, Rig, Track};
use crate::texture::Texture;
use lunaris_json::{parse, Json};
use std::path::{Component, Path};
use std::sync::Arc;

const MAX_FILE: usize = 128 << 20;
const MAX_VERTICES: usize = 4_000_000;
const MAX_DEPTH: usize = 64;

pub struct Imported {
    pub mesh: Mesh,
    pub warnings: Vec<String>,
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return Err(format!("invalid base64 character `{}`", c as char)),
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// A relative path that stays inside `dir`: no absolute paths, no `..`, no drive letters.
fn safe_join(dir: &Path, uri: &str) -> Result<std::path::PathBuf, String> {
    let decoded = uri.replace("%20", " ");
    let rel = Path::new(&decoded);
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) || decoded.contains(':') || decoded.contains('\\') {
        return Err(format!("the file name `{uri}` is not a plain relative path inside the workspace"));
    }
    Ok(dir.join(rel))
}

fn load_uri(uri: &str, dir: Option<&Path>) -> Result<Vec<u8>, String> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (_, data) = rest.split_once(";base64,").ok_or("only base64 data: URIs are supported")?;
        return b64_decode(data);
    }
    let dir = dir.ok_or_else(|| format!("`{uri}` is an external file, but this model was not loaded from a folder"))?;
    let path = safe_join(dir, uri)?;
    let meta = std::fs::metadata(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if meta.len() as usize > MAX_FILE {
        return Err(format!("{} is larger than the {} MB limit", path.display(), MAX_FILE >> 20));
    }
    std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

struct Doc<'a> {
    json: &'a Json,
    buffers: Vec<Vec<u8>>,
}

struct Acc {
    n: usize,
    comps: usize,
    data: Vec<f32>,
}

impl Doc<'_> {
    fn arr(&self, key: &str) -> &[Json] {
        self.json.get(key).and_then(|v| v.as_array()).unwrap_or(&[])
    }

    fn accessor(&self, idx: usize) -> Result<Acc, String> {
        let a = self.arr("accessors").get(idx).ok_or_else(|| format!("accessor {idx} does not exist"))?;
        if a.get("sparse").is_some() {
            return Err("sparse accessors are not supported".into());
        }
        let count = a.get("count").and_then(|v| v.as_u64()).ok_or("accessor without count")? as usize;
        let comps = match a.get("type").and_then(|v| v.as_str()) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            Some("MAT4") => 16,
            other => return Err(format!("unsupported accessor type {other:?}")),
        };
        let (csize, kind) = match a.get("componentType").and_then(|v| v.as_u64()) {
            Some(5120) => (1, 5120),
            Some(5121) => (1, 5121),
            Some(5122) => (2, 5122),
            Some(5123) => (2, 5123),
            Some(5125) => (4, 5125),
            Some(5126) => (4, 5126),
            other => return Err(format!("unsupported component type {other:?}")),
        };
        let normalized = a.get("normalized").and_then(|v| v.as_bool()).unwrap_or(false);
        if count > MAX_VERTICES * 3 {
            return Err(format!("accessor with {count} elements is too large"));
        }
        let Some(view_idx) = a.get("bufferView").and_then(|v| v.as_u64()) else {
            return Ok(Acc { n: count, comps, data: vec![0.0; count * comps] });
        };
        let view = self.arr("bufferViews").get(view_idx as usize).ok_or_else(|| format!("bufferView {view_idx} does not exist"))?;
        let buf = self.buffers.get(view.get("buffer").and_then(|v| v.as_u64()).unwrap_or(0) as usize).ok_or("a bufferView names a buffer that does not exist")?;
        let base = view.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize + a.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let elem = comps * csize;
        let stride = view.get("byteStride").and_then(|v| v.as_u64()).map(|s| s as usize).unwrap_or(elem).max(elem);
        if count > 0 && base + (count - 1) * stride + elem > buf.len() {
            return Err("an accessor reaches past the end of its buffer".into());
        }
        let mut data = Vec::with_capacity(count * comps);
        for i in 0..count {
            for c in 0..comps {
                let o = base + i * stride + c * csize;
                let v = match kind {
                    5126 => f32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]),
                    5125 => u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]) as f32,
                    5123 => {
                        let x = u16::from_le_bytes([buf[o], buf[o + 1]]) as f32;
                        if normalized { x / 65535.0 } else { x }
                    }
                    5122 => {
                        let x = i16::from_le_bytes([buf[o], buf[o + 1]]) as f32;
                        if normalized { (x / 32767.0).max(-1.0) } else { x }
                    }
                    5121 => {
                        let x = buf[o] as f32;
                        if normalized { x / 255.0 } else { x }
                    }
                    _ => {
                        let x = buf[o] as i8 as f32;
                        if normalized { (x / 127.0).max(-1.0) } else { x }
                    }
                };
                data.push(v);
            }
        }
        Ok(Acc { n: count, comps, data })
    }

    /// An integer accessor (indices) read exactly, without going through f32.
    fn indices(&self, idx: usize) -> Result<Vec<u32>, String> {
        let a = self.arr("accessors").get(idx).ok_or_else(|| format!("accessor {idx} does not exist"))?;
        let count = a.get("count").and_then(|v| v.as_u64()).ok_or("accessor without count")? as usize;
        let kind = a.get("componentType").and_then(|v| v.as_u64());
        let csize = match kind {
            Some(5121) => 1,
            Some(5123) => 2,
            Some(5125) => 4,
            other => return Err(format!("indices must be unsigned bytes, shorts or ints (got {other:?})")),
        };
        let view = self.arr("bufferViews").get(a.get("bufferView").and_then(|v| v.as_u64()).ok_or("indices without a bufferView")? as usize).ok_or("a bufferView does not exist")?;
        let buf = self.buffers.get(view.get("buffer").and_then(|v| v.as_u64()).unwrap_or(0) as usize).ok_or("a bufferView names a buffer that does not exist")?;
        let base = view.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize + a.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if count > MAX_VERTICES * 3 || base + count * csize > buf.len() {
            return Err("an index accessor reaches past the end of its buffer".into());
        }
        Ok((0..count)
            .map(|i| {
                let o = base + i * csize;
                match csize {
                    1 => buf[o] as u32,
                    2 => u16::from_le_bytes([buf[o], buf[o + 1]]) as u32,
                    _ => u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]),
                }
            })
            .collect())
    }

    fn node_matrix(&self, node: &Json) -> M4 {
        if let Some(m) = node.get("matrix").and_then(|v| v.as_array()).filter(|m| m.len() == 16) {
            let mut a = [0.0f32; 16];
            for (i, v) in m.iter().enumerate() {
                a[i] = v.as_f64().unwrap_or(0.0) as f32;
            }
            return M4(a);
        }
        let v3 = |k: &str, d: [f32; 3]| node.get(k).and_then(|v| v.as_array()).filter(|a| a.len() == 3).map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32]).unwrap_or(d);
        let rot = node.get("rotation").and_then(|v| v.as_array()).filter(|a| a.len() == 4).map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32, a[3].as_f64().unwrap_or(1.0) as f32]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        M4::from_trs(v3("translation", [0.0; 3]), rot, v3("scale", [1.0; 3]))
    }
}

/// The skeleton of skin `si` and all animations that move its joints.
#[allow(clippy::too_many_arguments)]
fn build_rig(doc: &Doc<'_>, si: usize, node_world: &[Option<M4>], parent_node: &[Option<usize>], mesh: &Mesh, vj: Vec<[u16; 4]>, vw: Vec<[f32; 4]>, warnings: &mut Vec<String>) -> Result<Rig, String> {
    let nodes = doc.arr("nodes");
    let skin = doc.arr("skins").get(si).ok_or_else(|| format!("skin {si} does not exist"))?;
    let joint_nodes: Vec<usize> = skin.get("joints").and_then(|v| v.as_array()).unwrap_or(&[]).iter().filter_map(|n| n.as_u64()).map(|n| n as usize).collect();
    if joint_nodes.is_empty() || joint_nodes.len() > 65_535 {
        return Err(format!("a skin needs between 1 and 65535 joints (this one has {})", joint_nodes.len()));
    }
    if vj.iter().flatten().any(|&j| j as usize >= joint_nodes.len()) {
        return Err("a vertex uses a joint the skin does not have".into());
    }
    let ibm = match skin.get("inverseBindMatrices").and_then(|v| v.as_u64()) {
        Some(a) => Some(doc.accessor(a as usize)?).filter(|a| a.comps == 16 && a.n >= joint_nodes.len()),
        None => None,
    };
    let node_to_joint: std::collections::HashMap<usize, usize> = joint_nodes.iter().enumerate().map(|(j, n)| (*n, j)).collect();
    let mut joints: Vec<Joint> = Vec::with_capacity(joint_nodes.len());
    for (j, &ni) in joint_nodes.iter().enumerate() {
        let node = nodes.get(ni).ok_or_else(|| format!("joint node {ni} does not exist"))?;
        let name = node.get("name").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| format!("joint{j}"));
        let (t, r, sc) = decompose(&doc.node_matrix(node));
        let mut parent = None;
        let mut up = parent_node.get(ni).copied().flatten();
        let mut guard = 0;
        while let Some(pn) = up {
            if let Some(&pj) = node_to_joint.get(&pn) {
                parent = Some(pj);
                break;
            }
            up = parent_node.get(pn).copied().flatten();
            guard += 1;
            if guard > 1000 {
                break;
            }
        }
        let base = if parent.is_none() { parent_node.get(ni).copied().flatten().and_then(|pn| node_world.get(pn).copied().flatten()).unwrap_or_else(M4::identity) } else { M4::identity() };
        let inverse_bind = match &ibm {
            Some(a) => {
                let mut m = [0.0f32; 16];
                m.copy_from_slice(&a.data[j * 16..j * 16 + 16]);
                M4(m)
            }
            None => M4::identity(),
        };
        joints.push(Joint { name, parent, base, t, r, s: sc, inverse_bind });
    }
    // every joint after its parent
    let depth_of = |mut j: usize| {
        let mut d = 0;
        while let Some(p) = joints[j].parent {
            j = p;
            d += 1;
            if d > joints.len() {
                break;
            }
        }
        d
    };
    let mut order: Vec<usize> = (0..joints.len()).collect();
    order.sort_by_key(|&j| (depth_of(j), j));
    // the animations
    let mut clips: Vec<Clip> = Vec::new();
    for (ai, anim) in doc.arr("animations").iter().enumerate() {
        let name = anim.get("name").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| format!("clip{ai}"));
        let samplers = anim.get("samplers").and_then(|v| v.as_array()).unwrap_or(&[]);
        let mut tracks: Vec<Track> = Vec::new();
        let mut duration = 0.0f32;
        for ch in anim.get("channels").and_then(|v| v.as_array()).unwrap_or(&[]) {
            let target = ch.get("target");
            let node = target.and_then(|t| t.get("node")).and_then(|v| v.as_u64()).map(|n| n as usize);
            let Some(&joint) = node.and_then(|n| node_to_joint.get(&n)) else {
                warnings.push(format!("animation `{name}` moves something that is not a joint of the skeleton: skipped"));
                continue;
            };
            let path = match target.and_then(|t| t.get("path")).and_then(|v| v.as_str()) {
                Some("translation") => AnimPath::Translation,
                Some("rotation") => AnimPath::Rotation,
                Some("scale") => AnimPath::Scale,
                other => {
                    warnings.push(format!("animation `{name}`: the property {other:?} is not supported: skipped"));
                    continue;
                }
            };
            let Some(sampler) = ch.get("sampler").and_then(|v| v.as_u64()).and_then(|s| samplers.get(s as usize)) else { continue };
            let (Some(input), Some(output)) = (sampler.get("input").and_then(|v| v.as_u64()), sampler.get("output").and_then(|v| v.as_u64())) else { continue };
            let (times, values) = match (doc.accessor(input as usize), doc.accessor(output as usize)) {
                (Ok(t), Ok(v)) => (t.data, v.data),
                _ => continue,
            };
            let interp = match sampler.get("interpolation").and_then(|v| v.as_str()) {
                Some("STEP") => Interp::Step,
                Some("CUBICSPLINE") => Interp::Cubic,
                _ => Interp::Linear,
            };
            let comps = if path == AnimPath::Rotation { 4 } else { 3 };
            let need = times.len() * comps * if interp == Interp::Cubic { 3 } else { 1 };
            if times.is_empty() || values.len() != need || times.windows(2).any(|w| w[1] < w[0]) {
                warnings.push(format!("animation `{name}`: a channel has keys that do not fit its values (or are not in time order): skipped"));
                continue;
            }
            duration = duration.max(*times.last().unwrap_or(&0.0));
            tracks.push(Track { joint, path, interp, times, values });
        }
        if !tracks.is_empty() {
            clips.push(Clip { name, duration, tracks });
        }
    }
    Ok(Rig { joints, order, clips, bind_pos: mesh.pos.clone(), bind_nrm: mesh.nrm.clone(), vj, vw })
}

/// Parse a `.glb` (binary) or `.gltf` (JSON) file. `dir` is the folder external files are resolved in (None: embedded data only).
pub fn import(bytes: &[u8], dir: Option<&Path>) -> Result<Imported, String> {
    if bytes.len() > MAX_FILE {
        return Err(format!("the model is larger than the {} MB limit", MAX_FILE >> 20));
    }
    let (json_text, mut glb_bin): (String, Option<Vec<u8>>) = if bytes.starts_with(b"glTF") {
        if bytes.len() < 20 || u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) != 2 {
            return Err("only glTF 2.0 binaries are supported".into());
        }
        let total = (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize).min(bytes.len());
        let (mut off, mut json, mut bin) = (12usize, None, None);
        while off + 8 <= total {
            let len = u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]) as usize;
            let kind = &bytes[off + 4..off + 8];
            let end = off + 8 + len;
            if end > total {
                return Err("a chunk of the .glb file reaches past its end".into());
            }
            let body = &bytes[off + 8..end];
            if kind == b"JSON" && json.is_none() {
                json = Some(String::from_utf8(body.to_vec()).map_err(|_| "the JSON chunk is not UTF-8")?);
            } else if kind == b"BIN\0" && bin.is_none() {
                bin = Some(body.to_vec());
            }
            off = (end + 3) & !3;
        }
        (json.ok_or("the .glb file has no JSON chunk")?, bin)
    } else {
        (String::from_utf8(bytes.to_vec()).map_err(|_| "a .gltf file must be UTF-8 JSON (or a .glb binary)")?, None)
    };
    let json = parse(&json_text).map_err(|e| format!("the model's JSON is invalid: {e}"))?;
    let version = json.get("asset").and_then(|a| a.get("version")).and_then(|v| v.as_str()).unwrap_or("");
    if !version.starts_with('2') {
        return Err(format!("only glTF 2.x is supported (this is `{version}`)"));
    }
    let mut warnings: Vec<String> = Vec::new();
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for (i, b) in json.get("buffers").and_then(|v| v.as_array()).unwrap_or(&[]).iter().enumerate() {
        match b.get("uri").and_then(|v| v.as_str()) {
            Some(uri) => buffers.push(load_uri(uri, dir).map_err(|e| format!("buffer {i}: {e}"))?),
            None => buffers.push(glb_bin.take().ok_or_else(|| format!("buffer {i} has no uri and the file has no binary chunk"))?),
        }
    }
    let doc = Doc { json: &json, buffers };

    // ---- the node tree of the default scene
    let nodes = doc.arr("nodes");
    let mut roots: Vec<usize> = Vec::new();
    let scene = json.get("scene").and_then(|v| v.as_u64()).map(|s| s as usize).unwrap_or(0);
    if let Some(s) = doc.arr("scenes").get(scene) {
        roots.extend(s.get("nodes").and_then(|v| v.as_array()).unwrap_or(&[]).iter().filter_map(|n| n.as_u64()).map(|n| n as usize));
    } else {
        let mut is_child = vec![false; nodes.len()];
        for n in nodes {
            for c in n.get("children").and_then(|v| v.as_array()).unwrap_or(&[]) {
                if let Some(c) = c.as_u64().filter(|&c| (c as usize) < nodes.len()) {
                    is_child[c as usize] = true;
                }
            }
        }
        roots.extend((0..nodes.len()).filter(|&i| !is_child[i]));
    }

    let mut out = Mesh { pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), col: Vec::new(), texw: Vec::new(), idx: Vec::new(), tex: None, rig: None };
    let mut texture_choice: Option<usize> = None; // image index
    // the node tree as it is walked (the skeleton needs where each node stands), and the skinning data of every vertex
    let mut node_world: Vec<Option<M4>> = vec![None; nodes.len()];
    let mut parent_node: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut chosen_skin: Option<usize> = None;
    let mut vj: Vec<[u16; 4]> = Vec::new();
    let mut vw: Vec<[f32; 4]> = Vec::new();
    let mut warned_skins = false;
    let mut stack: Vec<(usize, M4, usize)> = roots.into_iter().rev().map(|r| (r, M4::identity(), 0)).collect();
    let mut visited = 0usize;
    while let Some((ni, parent, depth)) = stack.pop() {
        visited += 1;
        if depth > MAX_DEPTH || visited > 100_000 {
            return Err("the node tree is too deep or too large (is it cyclic?)".into());
        }
        let Some(node) = nodes.get(ni) else { return Err(format!("node {ni} does not exist")) };
        let world = parent.mul(&doc.node_matrix(node));
        node_world[ni] = Some(world);
        for c in node.get("children").and_then(|v| v.as_array()).unwrap_or(&[]).iter().rev() {
            if let Some(c) = c.as_u64() {
                if (c as usize) < nodes.len() {
                    parent_node[c as usize] = Some(ni);
                }
                stack.push((c as usize, world, depth + 1));
            }
        }
        let Some(mi) = node.get("mesh").and_then(|v| v.as_u64()) else { continue };
        let mesh = doc.arr("meshes").get(mi as usize).ok_or_else(|| format!("mesh {mi} does not exist"))?;
        let nm = world.normal_matrix();
        // a mirroring transform flips the winding, so flip the triangles back
        let node_det = {
            let m = &world.0;
            m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
        };
        let node_skin = node.get("skin").and_then(|v| v.as_u64()).map(|x| x as usize);
        for (pi, prim) in mesh.get("primitives").and_then(|v| v.as_array()).unwrap_or(&[]).iter().enumerate() {
            let mode = prim.get("mode").and_then(|v| v.as_u64()).unwrap_or(4);
            if !(4..=6).contains(&mode) {
                warnings.push(format!("mesh {mi} primitive {pi}: draw mode {mode} (points, lines) is not supported and was skipped"));
                continue;
            }
            let attrs = prim.get("attributes").ok_or_else(|| format!("mesh {mi} primitive {pi} has no attributes"))?;
            let pos_acc = doc.accessor(attrs.get("POSITION").and_then(|v| v.as_u64()).ok_or_else(|| format!("mesh {mi} primitive {pi} has no POSITION"))? as usize)?;
            if pos_acc.comps != 3 {
                return Err("POSITION must be a vec3".into());
            }
            let n = pos_acc.n;
            if out.pos.len() + n > MAX_VERTICES {
                return Err(format!("the model has more than {MAX_VERTICES} vertices"));
            }
            let nrm_acc = match attrs.get("NORMAL").and_then(|v| v.as_u64()) {
                Some(a) => Some(doc.accessor(a as usize)?).filter(|a| a.comps == 3 && a.n == n),
                None => None,
            };
            let uv_acc = match attrs.get("TEXCOORD_0").and_then(|v| v.as_u64()) {
                Some(a) => Some(doc.accessor(a as usize)?).filter(|a| a.comps == 2 && a.n == n),
                None => None,
            };
            let col_acc = match attrs.get("COLOR_0").and_then(|v| v.as_u64()) {
                Some(a) => Some(doc.accessor(a as usize)?).filter(|a| (a.comps == 3 || a.comps == 4) && a.n == n),
                None => None,
            };
            // skinned? (the node uses a skin, and this primitive carries joints and weights)
            let mut jw: Option<(Acc, Acc)> = None;
            if let Some(si) = node_skin {
                if chosen_skin.is_none() {
                    chosen_skin = Some(si);
                }
                if chosen_skin == Some(si) {
                    if let (Some(ja), Some(wa)) = (attrs.get("JOINTS_0").and_then(|v| v.as_u64()), attrs.get("WEIGHTS_0").and_then(|v| v.as_u64())) {
                        let (j, w) = (doc.accessor(ja as usize)?, doc.accessor(wa as usize)?);
                        if j.comps == 4 && w.comps == 4 && j.n == n && w.n == n {
                            jw = Some((j, w));
                        }
                    }
                } else if !warned_skins {
                    warned_skins = true;
                    warnings.push("this model has more than one skin: only the first is used, the others are shown in their rest pose".to_string());
                }
            }
            let skinned = jw.is_some();
            // a skinned primitive keeps its own (bind pose) coordinates: the skeleton places it, not the mesh node
            let (world, nm, det) = if skinned { (M4::identity(), M4::identity().normal_matrix(), 1.0f32) } else { (world, nm, node_det) };
            // material: base colour factor (linear) and the first base colour texture
            let mut factor = [1.0f32; 3];
            let mut prim_image: Option<usize> = None;
            if let Some(mat) = prim.get("material").and_then(|v| v.as_u64()).and_then(|m| doc.arr("materials").get(m as usize)) {
                if let Some(pbr) = mat.get("pbrMetallicRoughness") {
                    if let Some(f) = pbr.get("baseColorFactor").and_then(|v| v.as_array()).filter(|f| f.len() >= 3) {
                        factor = [f[0].as_f64().unwrap_or(1.0) as f32, f[1].as_f64().unwrap_or(1.0) as f32, f[2].as_f64().unwrap_or(1.0) as f32];
                    }
                    if let Some(t) = pbr.get("baseColorTexture").and_then(|t| t.get("index")).and_then(|v| v.as_u64()) {
                        if let Some(img) = doc.arr("textures").get(t as usize).and_then(|t| t.get("source")).and_then(|v| v.as_u64()) {
                            prim_image = Some(img as usize);
                            if texture_choice.is_none() {
                                texture_choice = Some(img as usize);
                            }
                        }
                    }
                }
            }
            let textured = prim_image.is_some() && prim_image == texture_choice;
            if prim_image.is_some() && !textured {
                warnings.push(format!("mesh {mi} primitive {pi}: a model shows one texture; this primitive's own texture is shown as its plain colour"));
            }
            let base = out.pos.len() as u32;
            // indices (triangle list, strip or fan)
            let raw: Vec<u32> = match prim.get("indices").and_then(|v| v.as_u64()) {
                Some(a) => doc.indices(a as usize)?,
                None => (0..n as u32).collect(),
            };
            if raw.iter().any(|&i| i as usize >= n) {
                return Err(format!("mesh {mi} primitive {pi}: an index points past the last vertex"));
            }
            let mut tris: Vec<[u32; 3]> = Vec::new();
            match mode {
                4 => tris.extend(raw.chunks_exact(3).map(|t| [t[0], t[1], t[2]])),
                5 => {
                    for k in 0..raw.len().saturating_sub(2) {
                        tris.push(if k % 2 == 0 { [raw[k], raw[k + 1], raw[k + 2]] } else { [raw[k + 1], raw[k], raw[k + 2]] });
                    }
                }
                _ => {
                    for k in 1..raw.len().saturating_sub(1) {
                        tris.push([raw[0], raw[k], raw[k + 1]]);
                    }
                }
            }
            for i in 0..n {
                let p = [pos_acc.data[i * 3], pos_acc.data[i * 3 + 1], pos_acc.data[i * 3 + 2]];
                out.pos.push(world.transform_point(p));
                if let Some(a) = &nrm_acc {
                    let v = [a.data[i * 3], a.data[i * 3 + 1], a.data[i * 3 + 2]];
                    out.nrm.push(normalize([nm[0][0] * v[0] + nm[0][1] * v[1] + nm[0][2] * v[2], nm[1][0] * v[0] + nm[1][1] * v[1] + nm[1][2] * v[2], nm[2][0] * v[0] + nm[2][1] * v[1] + nm[2][2] * v[2]]));
                } else {
                    out.nrm.push([0.0; 3]); // filled with face normals below
                }
                out.uv.push(match &uv_acc {
                    Some(a) => [a.data[i * 2], 1.0 - a.data[i * 2 + 1]], // glTF puts v = 0 at the top; the renderer's v runs bottom to top
                    None => [0.0, 0.0],
                });
                let vc = match &col_acc {
                    Some(a) => [a.data[i * a.comps], a.data[i * a.comps + 1], a.data[i * a.comps + 2]],
                    None => [1.0, 1.0, 1.0],
                };
                // glTF colours are linear already, which is the renderer's working space
                out.col.push([factor[0] * vc[0], factor[1] * vc[1], factor[2] * vc[2]]);
                out.texw.push(if textured { 1.0 } else { 0.0 });
                match &jw {
                    Some((j, w)) => {
                        let ji = [j.data[i * 4] as u16, j.data[i * 4 + 1] as u16, j.data[i * 4 + 2] as u16, j.data[i * 4 + 3] as u16];
                        let mut wi = [w.data[i * 4].max(0.0), w.data[i * 4 + 1].max(0.0), w.data[i * 4 + 2].max(0.0), w.data[i * 4 + 3].max(0.0)];
                        let sum = wi[0] + wi[1] + wi[2] + wi[3];
                        if sum > 0.0 {
                            for x in &mut wi {
                                *x /= sum;
                            }
                        }
                        vj.push(ji);
                        vw.push(wi);
                    }
                    None => {
                        vj.push([0; 4]);
                        vw.push([0.0; 4]);
                    }
                }
            }
            let flat = nrm_acc.is_none();
            let idx_start = out.idx.len();
            for t in &tris {
                let tri = if det < 0.0 { [t[0], t[2], t[1]] } else { *t };
                if flat {
                    // flat shading for a model without normals: every triangle gets its own vertices
                    let (a, b, c) = (out.pos[(base + tri[0]) as usize], out.pos[(base + tri[1]) as usize], out.pos[(base + tri[2]) as usize]);
                    let fnorm = normalize(cross(sub(b, a), sub(c, a)));
                    let start = out.pos.len() as u32;
                    for &vi in &tri {
                        let src = (base + vi) as usize;
                        out.pos.push(out.pos[src]);
                        out.nrm.push(fnorm);
                        out.uv.push(out.uv[src]);
                        out.col.push(out.col[src]);
                        out.texw.push(out.texw[src]);
                        vj.push(vj[src]);
                        vw.push(vw[src]);
                    }
                    out.idx.extend_from_slice(&[start, start + 1, start + 2]);
                } else {
                    out.idx.extend_from_slice(&[base + tri[0], base + tri[1], base + tri[2]]);
                }
            }
            if flat {
                // the originals were only the source of the per-triangle copies: drop them and renumber the triangles
                let n = n as u32;
                for i in idx_start..out.idx.len() {
                    out.idx[i] -= n;
                }
                let r = base as usize..(base + n) as usize;
                out.pos.drain(r.clone());
                out.nrm.drain(r.clone());
                out.uv.drain(r.clone());
                out.col.drain(r.clone());
                out.texw.drain(r.clone());
                vj.drain(r.clone());
                vw.drain(r);
            }
        }
    }
    if out.idx.is_empty() {
        return Err("the model has no triangles".into());
    }
    // ---- the skeleton and its animations
    if let Some(si) = chosen_skin.filter(|_| vw.iter().any(|w| w.iter().any(|&x| x > 0.0))) {
        match build_rig(&doc, si, &node_world, &parent_node, &out, vj, vw, &mut warnings) {
            Ok(rig) => {
                // the mesh itself is shown in the rest pose of the skeleton
                let posed = rig.pose(&out, None, 0.0, false);
                out.pos = posed.pos;
                out.nrm = posed.nrm;
                out.rig = Some(Arc::new(rig));
            }
            Err(e) => warnings.push(format!("the skeleton was skipped: {e}")),
        }
    }
    // ---- the texture, if any
    if let Some(img) = texture_choice {
        let loaded: Result<Texture, String> = (|| {
            let image = doc.arr("images").get(img).ok_or("the texture's image does not exist")?;
            let data = if let Some(view) = image.get("bufferView").and_then(|v| v.as_u64()) {
                let v = doc.arr("bufferViews").get(view as usize).ok_or("the image's bufferView does not exist")?;
                let buf = doc.buffers.get(v.get("buffer").and_then(|b| b.as_u64()).unwrap_or(0) as usize).ok_or("the image's buffer does not exist")?;
                let off = v.get("byteOffset").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                let len = v.get("byteLength").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                buf.get(off..off + len).ok_or("the image reaches past its buffer")?.to_vec()
            } else {
                load_uri(image.get("uri").and_then(|u| u.as_str()).ok_or("the image has neither a bufferView nor a uri")?, dir)?
            };
            if !data.starts_with(b"\x89PNG") {
                return Err("only PNG textures are supported (this one is another format, for example JPEG)".to_string());
            }
            Texture::from_image_bytes(&data)
        })();
        match loaded {
            Ok(t) => out.tex = Some(Arc::new(t)),
            Err(e) => warnings.push(format!("texture skipped: {e}")),
        }
    }
    Ok(Imported { mesh: out, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le_f32(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn b64(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            s.push(T[(n >> 18) as usize & 63] as char);
            s.push(T[(n >> 12) as usize & 63] as char);
            s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
            s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
        }
        s
    }

    /// A single triangle in the XY plane facing +Z, with normals and a red material, inside a node translated by (1, 2, 3).
    fn triangle_gltf() -> String {
        let mut bin = le_f32(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        bin.extend(le_f32(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
        bin.extend([0u8, 0, 1, 0, 2, 0, 0, 0]); // u16 indices 0 1 2 + padding
        format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"translation":[1,2,3],"mesh":0}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1}},"indices":2,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[1,0.25,0,1]}}}}],
            "buffers":[{{"byteLength":{},"uri":"data:application/octet-stream;base64,{}"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":36}},{{"buffer":0,"byteOffset":72,"byteLength":6}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#,
            bin.len(),
            b64(&bin)
        )
    }

    #[test]
    fn a_small_gltf_with_a_transform_a_material_and_normals_imports() {
        let m = import(triangle_gltf().as_bytes(), None).unwrap();
        assert!(m.warnings.is_empty(), "{:?}", m.warnings);
        let mesh = m.mesh;
        assert_eq!(mesh.idx, vec![0, 1, 2]);
        assert_eq!(mesh.pos[0], [1.0, 2.0, 3.0]);
        assert_eq!(mesh.pos[1], [2.0, 2.0, 3.0]);
        assert_eq!(mesh.nrm[0], [0.0, 0.0, 1.0]);
        assert_eq!(mesh.col[0], [1.0, 0.25, 0.0], "the base colour factor becomes a vertex colour");
    }

    #[test]
    fn a_glb_container_carries_the_same_model_and_flat_normals_are_made_when_missing() {
        // GLB: JSON chunk + BIN chunk; this model has no normals and no indices
        let bin = le_f32(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let json = format!(r#"{{"asset":{{"version":"2.0"}},"nodes":[{{"mesh":0,"scale":[2,2,2]}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"buffers":[{{"byteLength":36}}],"bufferViews":[{{"buffer":0,"byteLength":36}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}}]}}"#);
        let mut json_bytes = json.into_bytes();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }
        let mut glb = Vec::new();
        glb.extend(b"glTF");
        glb.extend(2u32.to_le_bytes());
        glb.extend(((12 + 8 + json_bytes.len() + 8 + bin.len()) as u32).to_le_bytes());
        glb.extend((json_bytes.len() as u32).to_le_bytes());
        glb.extend(b"JSON");
        glb.extend(&json_bytes);
        glb.extend((bin.len() as u32).to_le_bytes());
        glb.extend(b"BIN\0");
        glb.extend(&bin);
        let m = import(&glb, None).unwrap().mesh;
        assert_eq!(m.idx.len(), 3);
        assert_eq!(m.pos[1], [2.0, 0.0, 0.0], "the node scale applies");
        assert_eq!(m.nrm[0], [0.0, 0.0, 1.0], "the counter-clockwise triangle faces +Z");
    }

    #[test]
    fn bad_models_are_refused_with_a_reason() {
        let e = |s: &str| import(s.as_bytes(), None).err().unwrap_or_else(|| "(accepted)".to_string());
        assert!(e("not json").contains("invalid"));
        assert!(e(r#"{"asset":{"version":"1.0"}}"#).contains("2.x"));
        assert!(e(r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"../../secret.bin"}]}"#).contains("not a plain relative path") || e(r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"../../secret.bin"}]}"#).contains("external file"));
        assert!(e(r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"C:/x.bin"}]}"#).contains("external file") || e(r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"C:/x.bin"}]}"#).contains("plain relative"));
        assert!(e(r#"{"asset":{"version":"2.0"},"nodes":[],"meshes":[]}"#).contains("no triangles"));
        // an accessor that reaches past its buffer
        let bad = triangle_gltf().replace(r#""count":3,"type":"VEC3"}"#, r#""count":300,"type":"VEC3"}"#);
        assert!(e(&bad).contains("past the end"), "{}", e(&bad));
        // a node cycle must not hang
        let cyc = r#"{"asset":{"version":"2.0"},"nodes":[{"children":[1]},{"children":[0]}],"scenes":[{"nodes":[0]}]}"#;
        assert!(e(cyc).contains("too deep") || e(cyc).contains("no triangles"));
        assert!(safe_join(Path::new("/w"), "models/a.bin").is_ok());
        assert!(safe_join(Path::new("/w"), "../a.bin").is_err());
        assert!(safe_join(Path::new("/w"), "/etc/passwd").is_err());
    }
}
