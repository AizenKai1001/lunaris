//! The physics view of a model file: every triangle of a `.glb` or `.gltf`, in the model's own coordinates, and nothing else.
//!
//! The renderer's importer (`lunaris-render/src/gltf.rs`) turns a model into a drawable `Mesh`. Physics needs only the triangles, and the
//! simulation crate has no dependency on the renderer, so this is a small second reader that follows **the same rules for the geometry**:
//! the node tree of the default scene is walked in the same order, every node's matrix (or translation / rotation / scale) is applied with
//! the same arithmetic, triangle strips and fans are unrolled the same way, and a mirroring node flips the winding back. The result is
//! that the triangles a `MeshCollider` collides with are, bit for bit, the triangles the renderer draws (a test in `lunaris-render`
//! compares the two on every model of the examples), which is what lets a physics ray and a picture ray agree.
//!
//! Not the same on purpose: materials, textures, normals, colours and animations are not read, and a skinned mesh keeps the positions it
//! is stored with (the renderer moves them into the skeleton's rest pose; a model that is bound to a skeleton is a character, not a level).
use lunaris_json::{parse, Json};
use std::path::{Component, Path};

type V3 = [f32; 3];

const MAX_FILE: usize = 128 << 20;
const MAX_VERTICES: usize = 4_000_000;
const MAX_DEPTH: usize = 64;

/// What the reader found: the vertices (already in the model's world, with every node applied), the triangles as indices into them, and
/// things worth telling the person who made the model.
#[derive(Debug, Default)]
pub struct RawModel {
    pub verts: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    pub warnings: Vec<String>,
    /// The model has a skeleton (a skinned mesh): physics uses the stored positions, not the posed ones.
    pub skinned: bool,
}

// ------------------------------------------------------------------------------------------------ matrices (the renderer's M4, copied)

/// A 4 x 4 matrix, column-major: element (row r, column c) is `m[c * 4 + r]`. The same layout and the same arithmetic as the renderer's
/// `M4`, so a vertex lands on exactly the same bits.
#[derive(Clone, Copy)]
struct M4([f32; 16]);

impl M4 {
    fn identity() -> M4 {
        let mut m = [0.0; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        M4(m)
    }

    /// `self * o` (apply `o` first).
    fn mul(&self, o: &M4) -> M4 {
        let mut r = [0.0f32; 16];
        for c in 0..4 {
            for row in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += self.0[k * 4 + row] * o.0[c * 4 + k];
                }
                r[c * 4 + row] = s;
            }
        }
        M4(r)
    }

    fn transform_point(&self, p: V3) -> V3 {
        let m = &self.0;
        let v = [p[0], p[1], p[2], 1.0];
        [
            m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12] * v[3],
            m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13] * v[3],
            m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14] * v[3],
        ]
    }

    /// Translation * rotation * scale.
    fn from_trs(t: V3, q: [f32; 4], s: V3) -> M4 {
        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        let q = if n > 0.0 {
            let i = 1.0 / n;
            [q[0] * i, q[1] * i, q[2] * i, q[3] * i]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);
        let r00 = 1.0 - 2.0 * (yy + zz);
        let r01 = 2.0 * (xy - wz);
        let r02 = 2.0 * (xz + wy);
        let r10 = 2.0 * (xy + wz);
        let r11 = 1.0 - 2.0 * (xx + zz);
        let r12 = 2.0 * (yz - wx);
        let r20 = 2.0 * (xz - wy);
        let r21 = 2.0 * (yz + wx);
        let r22 = 1.0 - 2.0 * (xx + yy);
        let mut m = [0.0; 16];
        m[0] = r00 * s[0];
        m[1] = r10 * s[0];
        m[2] = r20 * s[0];
        m[4] = r01 * s[1];
        m[5] = r11 * s[1];
        m[6] = r21 * s[1];
        m[8] = r02 * s[2];
        m[9] = r12 * s[2];
        m[10] = r22 * s[2];
        m[12] = t[0];
        m[13] = t[1];
        m[14] = t[2];
        m[15] = 1.0;
        M4(m)
    }

    /// The determinant of the 3 x 3 part: negative when the matrix mirrors (the winding of its triangles must be flipped back).
    fn det3(&self) -> f32 {
        let m = &self.0;
        m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
    }
}

// ------------------------------------------------------------------------------------------------ files

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

/// Is `name` the path of a model file (`.glb` or `.gltf`)? The same test the renderer makes for `MeshRenderer.mesh`.
pub fn is_model_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".glb") || l.ends_with(".gltf")
}

/// Read the model `name` (a relative path inside the workspace `root`, without `..`) and return its triangles.
pub fn read_model(root: &Path, name: &str) -> Result<RawModel, String> {
    let rel = Path::new(name);
    if name.is_empty() {
        return Err("the model name is empty".to_string());
    }
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("model paths are relative to the workspace and cannot use `..`".to_string());
    }
    if !is_model_name(name) {
        return Err(format!("`{name}` is not a model file: a MeshCollider needs a .glb or .gltf of the workspace (the built-in shapes such as `cube` are not files: use a Collider for those)"));
    }
    let path = root.join(rel);
    // a model a procedural provider makes (`crate::procgen`) is not on the disk
    if let Some(bytes) = crate::procgen::file(name) {
        return parse_model(&bytes?, path.parent());
    }
    let meta = std::fs::metadata(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if meta.len() as usize > MAX_FILE {
        return Err(format!("the model is larger than the {} MB limit", MAX_FILE >> 20));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_model(&bytes, path.parent())
}

// ------------------------------------------------------------------------------------------------ the document

struct Doc<'a> {
    json: &'a Json,
    buffers: Vec<Vec<u8>>,
}

impl Doc<'_> {
    fn arr(&self, key: &str) -> &[Json] {
        self.json.get(key).and_then(|v| v.as_array()).unwrap_or(&[])
    }

    /// The positions of an accessor (a vec3 list), converted like the renderer converts them.
    fn positions(&self, idx: usize) -> Result<Vec<V3>, String> {
        let a = self.arr("accessors").get(idx).ok_or_else(|| format!("accessor {idx} does not exist"))?;
        if a.get("sparse").is_some() {
            return Err("sparse accessors are not supported".into());
        }
        let count = a.get("count").and_then(|v| v.as_u64()).ok_or("accessor without count")? as usize;
        if a.get("type").and_then(|v| v.as_str()) != Some("VEC3") {
            return Err("POSITION must be a vec3".into());
        }
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
            return Ok(vec![[0.0; 3]; count]);
        };
        let view = self.arr("bufferViews").get(view_idx as usize).ok_or_else(|| format!("bufferView {view_idx} does not exist"))?;
        let buf = self.buffers.get(view.get("buffer").and_then(|v| v.as_u64()).unwrap_or(0) as usize).ok_or("a bufferView names a buffer that does not exist")?;
        let base = view.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize + a.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let elem = 3 * csize;
        let stride = view.get("byteStride").and_then(|v| v.as_u64()).map(|s| s as usize).unwrap_or(elem).max(elem);
        if count > 0 && base + (count - 1) * stride + elem > buf.len() {
            return Err("an accessor reaches past the end of its buffer".into());
        }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let mut p = [0.0f32; 3];
            for (c, slot) in p.iter_mut().enumerate() {
                let o = base + i * stride + c * csize;
                *slot = match kind {
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
            }
            out.push(p);
        }
        Ok(out)
    }

    /// An integer accessor (indices) read exactly.
    fn indices(&self, idx: usize) -> Result<Vec<u32>, String> {
        let a = self.arr("accessors").get(idx).ok_or_else(|| format!("accessor {idx} does not exist"))?;
        let count = a.get("count").and_then(|v| v.as_u64()).ok_or("accessor without count")? as usize;
        let csize = match a.get("componentType").and_then(|v| v.as_u64()) {
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
        let v3 = |k: &str, d: V3| node.get(k).and_then(|v| v.as_array()).filter(|a| a.len() == 3).map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32]).unwrap_or(d);
        let rot = node.get("rotation").and_then(|v| v.as_array()).filter(|a| a.len() == 4).map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32, a[3].as_f64().unwrap_or(1.0) as f32]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        M4::from_trs(v3("translation", [0.0; 3]), rot, v3("scale", [1.0; 3]))
    }
}

/// Parse a `.glb` (binary) or `.gltf` (JSON) file. `dir` is the folder external files are resolved in (None: embedded data only).
pub fn parse_model(bytes: &[u8], dir: Option<&Path>) -> Result<RawModel, String> {
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
            let end = off.saturating_add(8).saturating_add(len);
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
    let mut out = RawModel::default();
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for (i, b) in json.get("buffers").and_then(|v| v.as_array()).unwrap_or(&[]).iter().enumerate() {
        match b.get("uri").and_then(|v| v.as_str()) {
            Some(uri) => buffers.push(load_uri(uri, dir).map_err(|e| format!("buffer {i}: {e}"))?),
            None => buffers.push(glb_bin.take().ok_or_else(|| format!("buffer {i} has no uri and the file has no binary chunk"))?),
        }
    }
    let doc = Doc { json: &json, buffers };

    // ---- the node tree of the default scene (the same walk as the renderer's importer)
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

    let mut chosen_skin: Option<usize> = None;
    let mut stack: Vec<(usize, M4, usize)> = roots.into_iter().rev().map(|r| (r, M4::identity(), 0)).collect();
    let mut visited = 0usize;
    while let Some((ni, parent, depth)) = stack.pop() {
        visited += 1;
        if depth > MAX_DEPTH || visited > 100_000 {
            return Err("the node tree is too deep or too large (is it cyclic?)".into());
        }
        let Some(node) = nodes.get(ni) else { return Err(format!("node {ni} does not exist")) };
        let world = parent.mul(&doc.node_matrix(node));
        for c in node.get("children").and_then(|v| v.as_array()).unwrap_or(&[]).iter().rev() {
            if let Some(c) = c.as_u64() {
                stack.push((c as usize, world, depth + 1));
            }
        }
        let Some(mi) = node.get("mesh").and_then(|v| v.as_u64()) else { continue };
        let mesh = doc.arr("meshes").get(mi as usize).ok_or_else(|| format!("mesh {mi} does not exist"))?;
        let node_det = world.det3();
        let node_skin = node.get("skin").and_then(|v| v.as_u64()).map(|x| x as usize);
        for (pi, prim) in mesh.get("primitives").and_then(|v| v.as_array()).unwrap_or(&[]).iter().enumerate() {
            let mode = prim.get("mode").and_then(|v| v.as_u64()).unwrap_or(4);
            if !(4..=6).contains(&mode) {
                out.warnings.push(format!("mesh {mi} primitive {pi}: draw mode {mode} (points, lines) is not supported and was skipped"));
                continue;
            }
            let attrs = prim.get("attributes").ok_or_else(|| format!("mesh {mi} primitive {pi} has no attributes"))?;
            let positions = doc.positions(attrs.get("POSITION").and_then(|v| v.as_u64()).ok_or_else(|| format!("mesh {mi} primitive {pi} has no POSITION"))? as usize)?;
            let n = positions.len();
            if out.verts.len() + n > MAX_VERTICES {
                return Err(format!("the model has more than {MAX_VERTICES} vertices"));
            }
            // skinned? (the node uses the first skin, and this primitive carries joints and weights): it keeps its own coordinates
            let mut skinned = false;
            if let Some(si) = node_skin {
                if chosen_skin.is_none() {
                    chosen_skin = Some(si);
                }
                if chosen_skin == Some(si) && attrs.get("JOINTS_0").and_then(|v| v.as_u64()).is_some() && attrs.get("WEIGHTS_0").and_then(|v| v.as_u64()).is_some() {
                    skinned = true;
                }
            }
            if skinned {
                out.skinned = true;
            }
            let (matrix, det) = if skinned { (M4::identity(), 1.0f32) } else { (world, node_det) };
            let base = out.verts.len() as u32;
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
            out.verts.extend(positions.iter().map(|p| matrix.transform_point(*p)));
            for t in &tris {
                let t = if det < 0.0 { [t[0], t[2], t[1]] } else { *t };
                out.tris.push([base + t[0], base + t[1], base + t[2]]);
            }
        }
    }
    if out.tris.is_empty() {
        return Err("the model has no triangles".into());
    }
    if out.skinned {
        out.warnings.push("this model is bound to a skeleton: its collision uses the positions it is stored with (the bind pose), not the posed ones".to_string());
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn le_f32(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    /// A GLB with one mesh of the given triangles (three corner positions each), as a plain list of vertices without indices.
    pub fn glb_of(triangles: &[[V3; 3]]) -> Vec<u8> {
        let flat: Vec<f32> = triangles.iter().flatten().flatten().copied().collect();
        let bin = le_f32(&flat);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"buffers":[{{"byteLength":{len}}}],"bufferViews":[{{"buffer":0,"byteLength":{len}}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":{count},"type":"VEC3"}}]}}"#,
            len = bin.len(),
            count = triangles.len() * 3
        );
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
        glb
    }

    #[test]
    fn a_glb_gives_its_triangles_and_node_transforms_apply() {
        let glb = glb_of(&[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]);
        let m = parse_model(&glb, None).unwrap();
        assert_eq!(m.tris, vec![[0, 1, 2]]);
        assert_eq!(m.verts[1], [1.0, 0.0, 0.0]);
        // a node with a scale and a translation
        let json = r#"{"asset":{"version":"2.0"},"nodes":[{"mesh":0,"scale":[2,2,2],"translation":[1,2,3]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}]}"#;
        let m = parse_model(json.as_bytes(), None).unwrap();
        assert_eq!(m.verts[1], [3.0, 2.0, 3.0]);
        assert_eq!(m.verts[2], [1.0, 4.0, 3.0]);
    }

    #[test]
    fn a_mirroring_node_flips_the_winding_back() {
        let json = r#"{"asset":{"version":"2.0"},"nodes":[{"mesh":0,"scale":[-1,1,1]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}]}"#;
        let m = parse_model(json.as_bytes(), None).unwrap();
        assert_eq!(m.tris, vec![[0, 2, 1]]);
    }

    #[test]
    fn bad_models_are_refused_with_a_reason_and_never_panic() {
        let e = |s: &str| parse_model(s.as_bytes(), None).err().unwrap_or_else(|| "(accepted)".to_string());
        assert!(e("not json").contains("invalid"));
        assert!(e(r#"{"asset":{"version":"1.0"}}"#).contains("2.x"));
        assert!(e(r#"{"asset":{"version":"2.0"},"nodes":[],"meshes":[]}"#).contains("no triangles"));
        assert!(e(r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"../../secret.bin"}]}"#).contains("not a plain relative path") || e(r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"../../secret.bin"}]}"#).contains("external file"));
        let cyc = r#"{"asset":{"version":"2.0"},"nodes":[{"children":[1]},{"children":[0]}],"scenes":[{"nodes":[0]}]}"#;
        assert!(e(cyc).contains("too deep") || e(cyc).contains("no triangles"));
        // truncated binaries
        let glb = glb_of(&[[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]);
        for cut in [3usize, 11, 19, 30, glb.len() - 5] {
            let _ = parse_model(&glb[..cut], None);
        }
        assert!(read_model(Path::new("/w"), "../x.glb").is_err());
        assert!(read_model(Path::new("/w"), "cube").is_err());
        assert!(read_model(Path::new("/w"), "").is_err());
    }
}
