//! A round city's plan as data for a modelling program (the game's Blender builder, `art/city/`): the rings, every block with its tile, quad, land use and the frame of its park, and every lot with the building the engine decided on it (its volumes,
//! floors, roofs and doors). The engine keeps the collision shells, the insides, the doors and the crates of these buildings, so what a modelling program makes of their outsides has to stand on the same footprints and open at the same doors.

use crate::building::{exterior_with, spec_of};
use crate::materials::Lib;
use crate::plan::{LandUse, Plan};
use crate::polar::{BANDS, P2};
use crate::streetscape;
use lunaris_json::Json;

fn n(x: f32) -> Json {
    Json::Float(x as f64)
}

fn p2(p: P2) -> Json {
    Json::arr([n(p[0]), n(p[1])])
}

fn quad(q: [P2; 4]) -> Json {
    Json::arr(q.map(p2))
}

/// The whole round plan of `plan` (which must be a polar one) as JSON.
pub fn polar_plan(lib: &Lib, plan: &Plan) -> Json {
    let pol = plan.polar.expect("a polar plan");
    let sec = plan.section;
    let mut blocks: Vec<Json> = Vec::new();
    let mut ids: Vec<(i32, i32)> = vec![(-1, 0)];
    for b in 0..=BANDS {
        for s in 0..pol.counts[b as usize] as i32 {
            ids.push((b, s));
        }
    }
    for (i, j) in ids {
        let use_ = plan.landuse(i, j);
        let (c, ux, uz) = plan.block_frame(i, j);
        let (hw, hd) = if i < 0 { (0.0, 0.0) } else { plan.local_rect(i, j) };
        let mut o: Vec<(&str, Json)> = vec![
            ("i", Json::from(i)),
            ("j", Json::from(j)),
            ("use", Json::str(format!("{use_:?}"))),
            ("tile", quad(plan.tile_poly(i, j))),
            ("quad", quad(plan.block_poly(i, j))),
            ("frame", Json::obj([("c", p2(c)), ("ux", p2(ux)), ("uz", p2(uz)), ("hw", n(hw)), ("hd", n(hd))])),
        ];
        if i >= 0 {
            let cnt = pol.counts[i as usize] as i32;
            o.push(("avenue_left", Json::Bool(pol.avenue(i, j))));
            o.push(("avenue_right", Json::Bool(pol.avenue(i, (j + 1) % cnt))));
            o.push(("ring_extra_in", n(pol.ring_extra(i))));
            o.push(("ring_extra_out", n(pol.ring_extra(i + 1))));
        }
        if use_ == LandUse::Park && i >= 0 {
            let rect = (-hw, hw, -hd, hd);
            let l = streetscape::park_layout_rect(plan, i, j, rect);
            o.push(("park", Json::obj([("kind", Json::str(format!("{:?}", l.kind))), ("fx", n(l.fx)), ("fz", n(l.fz)), ("hx", n(l.hx)), ("hz", n(l.hz)), ("along_x", Json::Bool(l.along_x))])));
        }
        let mut lots: Vec<Json> = Vec::new();
        for lp in plan.lots(i, j) {
            let spec = spec_of(lib, &lp.lot, lp.kind, lp.key);
            let (doors, glass) = exterior_with(&mut crate::mesh::MeshBuilder::new(), lib, &spec, false);
            let mut js = spec.to_json(&doors, &glass);
            if let Json::Obj(v) = &mut js {
                v.push(("index".to_string(), Json::from(lp.index as i64)));
            }
            lots.push(js);
        }
        o.push(("lots", Json::Arr(lots)));
        blocks.push(Json::obj(o));
    }
    let opt = |o: &Option<String>| o.as_ref().map_or(Json::Null, |s| Json::str(s.clone()));
    let materials: Vec<Json> = lib
        .mats
        .iter()
        .map(|m| {
            Json::obj([
                ("name", Json::str(m.name.clone())),
                ("diff", opt(&m.diff)),
                ("nor", opt(&m.nor)),
                ("arm", opt(&m.arm)),
                ("color", Json::arr(m.color.map(n))),
                ("rough", n(m.rough)),
                ("metal", n(m.metal)),
                ("size", Json::arr(m.size.map(n))),
                ("double_sided", Json::Bool(m.double_sided)),
                ("alpha_cutoff", n(m.alpha_cutoff)),
                ("avg", Json::arr(m.avg.map(n))),
            ])
        })
        .collect();
    Json::obj([
        ("materials", Json::Arr(materials)),
        ("seed", Json::str(plan.seed.to_string())),
        ("section", Json::obj([("carriageway", n(sec.carriageway)), ("sidewalk", n(sec.sidewalk)), ("kerb_h", n(sec.kerb_h))])),
        ("rings", Json::arr(pol.rings.map(n))),
        ("counts", Json::arr(pol.counts.map(|c| Json::from(c as i64)))),
        ("extra", n(crate::polar::EXTRA)),
        ("blocks", Json::Arr(blocks)),
    ])
}
