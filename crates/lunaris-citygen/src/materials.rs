//! The materials of the city: photographed PBR sets (colour, normal and occlusion-roughness-metallic pictures from Poly Haven, laid on surfaces at their real size) and a few plain ones (glass, paint, metal).
//!
//! The pictures are not made here. `materials.json` (written by tools/citygen/fetch_textures.py) lists the sets that exist and the size in metres one repeat covers; anything the generator names that is not in it falls back
//! to a plain material of a similar colour so that a missing set never stops a city from being made.

use crate::mesh::Tex;
use crate::rng::Rng;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct MatDef {
    pub name: String,
    /// Picture paths relative to the glTF file (`textures/<slug>/diff.jpg` ...); None for a plain material.
    pub diff: Option<String>,
    pub nor: Option<String>,
    pub arm: Option<String>,
    /// Base colour factor in linear light (white for a photographed set).
    pub color: [f32; 3],
    pub rough: f32,
    pub metal: f32,
    /// Metres one repeat of the picture covers.
    pub size: [f32; 2],
    /// Seen from both sides: panes, thin panels and bars that are looked at from inside too.
    pub double_sided: bool,
    /// glTF `MASK` cut-out level (leaves, fences); 0 = opaque.
    pub alpha_cutoff: f32,
    /// The average colour of the surface in linear light (what a far-off building of this material looks like).
    pub avg: [f32; 3],
}

pub struct Lib {
    pub mats: Vec<MatDef>,
    names: HashMap<String, usize>,
}

/// How many street name plates there are (`streetname_0` ... `streetname_127`): 64 names, each as a street and as an avenue.
pub const STREET_PLATES: usize = 128;

/// How many names the sign pictures hold (`SIGN_NAMES`) are laid out in a grid this many across (and `SIGN_NAMES / SIGN_COLUMNS` down).
pub const SIGN_COLUMNS: usize = 8;

/// The rectangle of name `k` in the lettering picture of a colour: (u0, v0, u1, v1), v counted down from the top of the picture.
pub fn sign_cell(k: usize) -> (f32, f32, f32, f32) {
    let rows = SIGN_NAMES.div_ceil(SIGN_COLUMNS) as f32;
    let (c, r) = ((k % SIGN_COLUMNS) as f32, (k / SIGN_COLUMNS) as f32);
    (c / SIGN_COLUMNS as f32, r / rows, (c + 1.0) / SIGN_COLUMNS as f32, (r + 1.0) / rows)
}

/// How many names the sign pictures hold (`signtext_w_0` ... `signtext_b_47`).
pub const SIGN_NAMES: usize = 48;

/// (name, linear colour, roughness, metallic) of the plain materials.
const PLAIN: &[(&str, [f32; 3], f32, f32)] = &[
    ("glass_dark", [0.020, 0.026, 0.032], 0.04, 0.0),
    ("glass_blue", [0.035, 0.060, 0.095], 0.04, 0.0),
    ("glass_green", [0.030, 0.060, 0.050], 0.05, 0.0),
    ("glass_grey", [0.050, 0.060, 0.070], 0.05, 0.0),
    ("glass_green", [0.020, 0.062, 0.048], 0.05, 0.0),
    ("glass_teal", [0.018, 0.070, 0.078], 0.05, 0.0),
    ("glass_bronze", [0.075, 0.052, 0.030], 0.05, 0.0),
    ("glass_silver", [0.100, 0.110, 0.120], 0.05, 0.0),
    ("glass_black", [0.008, 0.009, 0.011], 0.05, 0.0),
    // a window with a light on (the engine's `emissive` of the entity makes it glow at night; by day it is a pane that is a little warmer than the others)
    ("glass_lit_warm", [0.55, 0.36, 0.17], 0.05, 0.0),
    ("glass_lit_cool", [0.30, 0.42, 0.55], 0.05, 0.0),
    ("curtain_warm", [0.28, 0.23, 0.17], 0.70, 0.0),
    ("curtain_cool", [0.20, 0.22, 0.27], 0.70, 0.0),
    ("blind_white", [0.45, 0.45, 0.42], 0.60, 0.0),
    ("frame_white", [0.55, 0.55, 0.52], 0.45, 0.0),
    ("frame_dark", [0.035, 0.035, 0.040], 0.40, 0.6),
    ("frame_alu", [0.35, 0.36, 0.38], 0.35, 0.9),
    ("frame_brown", [0.10, 0.065, 0.04], 0.50, 0.0),
    ("spandrel_dark", [0.040, 0.045, 0.050], 0.25, 0.6),
    ("spandrel_blue", [0.050, 0.080, 0.120], 0.25, 0.6),
    ("spandrel_cream", [0.35, 0.33, 0.28], 0.50, 0.1),
    ("sign_red", [0.22, 0.022, 0.018], 0.45, 0.0),
    ("sign_blue", [0.030, 0.060, 0.19], 0.45, 0.0),
    ("sign_white", [0.50, 0.50, 0.47], 0.45, 0.0),
    ("sign_yellow", [0.45, 0.33, 0.040], 0.45, 0.0),
    ("sign_black", [0.016, 0.016, 0.018], 0.40, 0.0),
    ("sign_green", [0.030, 0.14, 0.065], 0.45, 0.0),
    ("sign_maroon", [0.10, 0.018, 0.020], 0.45, 0.0),
    ("sign_charcoal", [0.045, 0.047, 0.05], 0.40, 0.2),
    ("sign_gold", [0.38, 0.26, 0.05], 0.30, 0.8),
    ("awning_red", [0.22, 0.032, 0.025], 0.90, 0.0),
    ("awning_green", [0.035, 0.13, 0.06], 0.90, 0.0),
    ("awning_navy", [0.025, 0.04, 0.12], 0.90, 0.0),
    ("awning_cream", [0.40, 0.37, 0.28], 0.90, 0.0),
    ("curtain_red", [0.20, 0.035, 0.03], 0.80, 0.0),
    ("curtain_green", [0.07, 0.16, 0.08], 0.80, 0.0),
    ("curtain_beige", [0.38, 0.32, 0.22], 0.80, 0.0),
    ("curtain_pink", [0.32, 0.17, 0.18], 0.80, 0.0),
    ("interior_warm", [0.060, 0.045, 0.030], 0.05, 0.0),
    ("interior_cool", [0.045, 0.052, 0.062], 0.05, 0.0),
    ("ceiling_white", [0.72, 0.72, 0.70], 0.90, 0.0),
    ("wall_cream", [0.55, 0.49, 0.38], 0.90, 0.0),
    ("wall_white", [0.64, 0.64, 0.62], 0.90, 0.0),
    ("wall_sage", [0.26, 0.33, 0.25], 0.90, 0.0),
    ("wall_blue", [0.26, 0.31, 0.40], 0.90, 0.0),
    ("fabric_grey", [0.10, 0.10, 0.11], 0.95, 0.0),
    ("fabric_blue", [0.04, 0.07, 0.16], 0.95, 0.0),
    ("fabric_rust", [0.20, 0.07, 0.04], 0.95, 0.0),
    ("wood_dark", [0.06, 0.035, 0.02], 0.55, 0.0),
    ("wood_light", [0.30, 0.20, 0.10], 0.55, 0.0),
    ("ceramic_white", [0.75, 0.75, 0.73], 0.15, 0.0),
    ("countertop", [0.08, 0.08, 0.085], 0.30, 0.0),
    ("steel", [0.45, 0.46, 0.48], 0.30, 1.0),
    ("door_wood", [0.16, 0.08, 0.04], 0.55, 0.0),
    ("door_glass", [0.04, 0.06, 0.08], 0.05, 0.0),
    ("equip_metal", [0.30, 0.31, 0.32], 0.50, 0.8),
    ("equip_paint", [0.45, 0.46, 0.44], 0.60, 0.2),
    ("paint_white", [0.70, 0.70, 0.68], 0.55, 0.0),
    ("paint_yellow", [0.60, 0.42, 0.030], 0.55, 0.0),
    ("rubber", [0.020, 0.020, 0.020], 0.90, 0.0),
    ("interior_dark", [0.012, 0.012, 0.014], 0.9, 0.0),
    // the plain white that vertex colours paint on: far-off buildings and ground
    ("far_flat", [1.0, 1.0, 1.0], 0.92, 0.0),
];

/// The plain materials fall into five classes of roughness and metal (glass and glaze, paint and wood, cloth and rubber, painted metal, bare metal): each class is one picture of roughness for the engine and so one piece of a
/// model, where every different pair used to be its own (eighteen pieces and as many draws in a block's walls, now five).
fn surface_class(rough: f32, metal: f32) -> (f32, f32) {
    if metal >= 0.75 {
        (0.32, 0.9)
    } else if metal >= 0.15 {
        (0.35, 0.6)
    } else if rough <= 0.2 {
        (0.06, 0.0)
    } else if rough >= 0.75 {
        (0.88, 0.0)
    } else {
        (0.5, 0.0)
    }
}

impl Lib {
    /// The library from the text of `materials.json` ("" for none: only the plain materials exist).
    pub fn from_json(text: &str) -> Lib {
        let mut lib = Lib { mats: Vec::new(), names: HashMap::new() };
        for (name, color, rough, metal) in PLAIN {
            let (rough, metal) = surface_class(*rough, *metal);
            let (rough, metal) = (&rough, &metal);
            let thin = ["glass", "interior_", "frame_", "spandrel", "curtain", "blind", "door_glass", "steel"].iter().any(|p| name.starts_with(p));
            lib.add(MatDef { name: name.to_string(), diff: None, nor: None, arm: None, color: *color, rough: *rough, metal: *metal, size: [1.0, 1.0], double_sided: thin, alpha_cutoff: 0.0, avg: *color });
        }
        // the two pictures far-off walls wear (`gltf::ensure_far_textures` writes them): a bay of windows, and a curtain wall's bands
        for (name, bay) in [("far_win", [3.6, 3.3]), ("far_glass", [1.8, 3.9])] {
            lib.add(MatDef { name: name.to_string(), diff: Some(format!("textures/_far/{name}_l.png")), nor: None, arm: None, color: [1.0; 3], rough: 0.85, metal: 0.0, size: bay, double_sided: false, alpha_cutoff: 0.0, avg: [0.5; 3] });
        }
        // the words of the shop signs (`tools/citygen/make_signs.py`): letters on a transparent picture, white for a dark sign and black for a light one
        // (one picture for all the names of a colour, `SIGN_COLUMNS` across: a block of shops is then one piece of the model for all its lettering, not one for each name)
        for tag in ["w", "b"] {
            lib.add(MatDef { name: format!("signtext_{tag}"), diff: Some(format!("textures/_signs/atlas_{tag}.png")), nor: None, arm: None, color: [1.0; 3], rough: 0.5, metal: 0.0, size: [1.0, 1.0], double_sided: false, alpha_cutoff: 0.5, avg: [0.5; 3] });
        }
        for m in 0..STREET_PLATES {
            lib.add(MatDef { name: format!("streetname_{m}"), diff: Some(format!("textures/_signs/street_{m}.png")), nor: None, arm: None, color: [1.0; 3], rough: 0.4, metal: 0.0, size: [1.0, 1.0], double_sided: false, alpha_cutoff: 0.0, avg: [0.1, 0.3, 0.2] });
        }
        if let Ok(doc) = lunaris_json::parse(if text.trim().is_empty() { "{}" } else { text }) {
            if let Some(pairs) = doc.as_object() {
                for (slug, v) in pairs {
                    let avg = v.get("avg").and_then(|a| a.as_array()).map(|a| [a.first().and_then(|x| x.as_f64()).unwrap_or(0.5) as f32, a.get(1).and_then(|x| x.as_f64()).unwrap_or(0.5) as f32, a.get(2).and_then(|x| x.as_f64()).unwrap_or(0.5) as f32]).unwrap_or([0.4; 3]);
                    let size = v.get("size_m").and_then(|a| a.as_array()).map(|a| [a.first().and_then(|x| x.as_f64()).unwrap_or(2.0) as f32, a.get(1).and_then(|x| x.as_f64()).unwrap_or(2.0) as f32]).unwrap_or([2.0, 2.0]);
                    lib.add(MatDef { name: slug.clone(), diff: Some(format!("textures/{slug}/diff.jpg")), nor: Some(format!("textures/{slug}/nor.jpg")), arm: Some(format!("textures/{slug}/arm.jpg")), color: [1.0; 3], rough: 0.8, metal: 0.0, size, double_sided: false, alpha_cutoff: 0.0, avg });
                }
            }
        }
        lib
    }

    /// Add a material (from a prop model) and return its number.
    pub fn add_material(&mut self, m: MatDef) -> usize {
        let id = self.mats.len();
        self.add(m);
        id
    }

    fn add(&mut self, m: MatDef) {
        self.names.insert(m.name.clone(), self.mats.len());
        self.mats.push(m);
    }

    pub fn has(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    /// The material number of a name. A name that does not exist gives the first of `fallbacks` that does, then a grey plain material.
    pub fn id_or(&self, name: &str, fallbacks: &[&str]) -> usize {
        if let Some(i) = self.names.get(name) {
            return *i;
        }
        for f in fallbacks {
            if let Some(i) = self.names.get(*f) {
                return *i;
            }
        }
        self.names["equip_paint"]
    }

    pub fn id(&self, name: &str) -> usize {
        self.id_or(name, &[])
    }

    /// How material `id`'s pictures are laid: at their real size, shifted by a random amount so that two walls of the same material differ.
    pub fn tex(&self, id: usize, rng: &mut Rng) -> Tex {
        let s = self.mats[id].size;
        Tex { size: s, off: [rng.range(0.0, s[0]), rng.range(0.0, s[1])] }
    }

    /// The same, without a random offset.
    pub fn tex_plain(&self, id: usize) -> Tex {
        Tex { size: self.mats[id].size, off: [0.0, 0.0] }
    }

    pub fn photographed(&self, id: usize) -> bool {
        self.mats[id].diff.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_materials_exist_without_any_pictures_and_unknown_names_fall_back() {
        let lib = Lib::from_json("");
        assert!(lib.has("glass_dark") && !lib.has("brick_wall_001"));
        assert_eq!(lib.id_or("brick_wall_001", &["nothing", "glass_blue"]), lib.id("glass_blue"));
    }

    #[test]
    fn a_photographed_set_comes_with_its_size() {
        let lib = Lib::from_json(r#"{"brick_wall_001":{"size_m":[3.0,3.0]}}"#);
        let id = lib.id("brick_wall_001");
        assert!(lib.photographed(id));
        assert_eq!(lib.mats[id].size, [3.0, 3.0]);
        assert_eq!(lib.mats[id].diff.as_deref(), Some("textures/brick_wall_001/diff.jpg"));
    }
}
