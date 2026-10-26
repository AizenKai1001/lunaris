//! The `ui_audit` tool: every visible `Ui` and `Text` with the box it covers and the box its letters cover, in pixels of a picture of the size asked for
//! (`lunaris_render::ui_audit`, `docs/UI_AUDIT.md`). For quality checks of the interface at different window sizes.
use crate::engine::{uid_json, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;

fn boxed(b: Option<[f32; 4]>) -> Json {
    match b {
        Some(b) => Json::arr(b.iter().map(|v| Json::from((*v as f64 * 100.0).round() / 100.0))),
        None => Json::Null,
    }
}

fn colour(c: [f32; 4]) -> Json {
    Json::arr(c.iter().map(|v| Json::from((*v as f64 * 1000.0).round() / 1000.0)))
}

impl Engine {
    pub(crate) fn ui_audit_tool(&mut self, a: &Args) -> Result<Json> {
        let dim = |key: &str, default: u64| -> Result<u32> {
            let v = a.opt_u64(key)?.unwrap_or(default);
            if !(16..=8192).contains(&v) {
                return Err(EcsError::new("bad_arguments", format!("`{key}` must be from 16 to 8192 pixels")).at(key));
            }
            Ok(v as u32)
        };
        let (width, height) = (dim("width", 1920)?, dim("height", 1080)?);
        let entries = lunaris_render::ui_audit::audit(&mut self.sim.world, &mut self.textures, width, height)?;
        let list: Vec<Json> = entries
            .iter()
            .map(|e| {
                let mut o = Json::obj([
                    ("uid", uid_json(e.uid)),
                    ("name", Json::str(e.name.clone())),
                    ("parent", uid_json(e.parent)),
                    ("layer", Json::from(e.layer as i64)),
                    ("is_text", Json::Bool(e.is_text)),
                    ("rect", boxed(e.rect)),
                    ("ink", boxed(e.ink)),
                    ("label", Json::str(e.label.clone())),
                    ("template", Json::str(e.template.clone())),
                    ("font", Json::str(e.font.clone())),
                    ("em_px", Json::from(e.em_px as f64)),
                    ("button", Json::Bool(e.button)),
                    ("color", colour(e.color)),
                    ("back_color", colour(e.back_color)),
                    ("text_color", colour(e.text_color)),
                    ("image", Json::Bool(e.image)),
                    ("shadow", Json::Bool(e.shadow)),
                    ("anchored", Json::Bool(e.anchored)),
                ]);
                if e.clip.is_some() {
                    o.set("clip", boxed(e.clip));
                }
                o
            })
            .collect();
        Ok(Json::obj([("width", Json::from(width as i64)), ("height", Json::from(height as i64)), ("count", Json::from(list.len() as i64)), ("elements", Json::Arr(list))]))
    }
}
