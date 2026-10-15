//! `save_game`, `load_game` and `saves`: saved games in the workspace folder `saves/`.
use crate::engine::{hex, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::{parse, Json};

fn slot_name(a: &Args) -> Result<String> {
    let v = a.0.get("slot").ok_or_else(|| a.missing("slot", "string or number"))?;
    let name = match v {
        Json::Str(s) => s.clone(),
        other => match other.as_u64() {
            Some(n) => format!("slot{n}"),
            None => return Err(EcsError::new("type_mismatch", "`slot` must be a name or a number").at("slot")),
        },
    };
    if name.is_empty() || name.len() > 40 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(EcsError::new("bad_slot", format!("`{name}` is not a slot name")).at("slot").hint("letters, digits, `_` and `-`, up to 40 characters, or a number"));
    }
    Ok(name)
}

impl Engine {
    pub(crate) fn save_game_tool(&mut self, a: &Args) -> Result<Json> {
        let name = slot_name(a)?;
        let text = self.sim.save_json().to_string_pretty().map_err(|e| EcsError::new("write_error", e.to_string()))?;
        let path = self.write_text(&format!("saves/{name}.json"), &text)?;
        Ok(Json::obj([("slot", Json::str(name)), ("saved_to", Json::str(path.display().to_string())), ("bytes", Json::from(text.len())), ("tick", Json::from(self.sim.tick as i64)), ("entities", Json::from(self.sim.world.len()))]))
    }

    pub(crate) fn load_game_tool(&mut self, a: &Args) -> Result<Json> {
        let name = slot_name(a)?;
        if self.sim.is_recording() {
            return Err(EcsError::new("recording_active", "stop the recording before loading a game").hint("call record_stop"));
        }
        let text = self.read_text(&format!("saves/{name}.json")).map_err(|e| e.hint("`saves` lists the saved games"))?;
        let save = parse(&text).map_err(|e| EcsError::new("bad_save", e.to_string()).at("slot"))?;
        let created = self.sim.load_save(&save)?;
        self.clear_history();
        Ok(Json::obj([("slot", Json::str(name)), ("entities", Json::from(created.len())), ("tick", Json::from(self.sim.tick as i64)), ("hash", hex(self.sim.state_hash()))]))
    }

    pub(crate) fn saves_tool(&mut self, _a: &Args) -> Result<Json> {
        let dir = self.root.join("saves");
        let mut out: Vec<(String, Json)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut names: Vec<String> = rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".json")).collect();
            names.sort();
            for n in names.into_iter().take(200) {
                let slot = n.trim_end_matches(".json").to_string();
                let text = std::fs::read_to_string(dir.join(&n)).unwrap_or_default();
                let (mut tick, mut entities) = (Json::Null, Json::Null);
                if let Ok(j) = parse(&text) {
                    tick = j.get("save").and_then(|s| s.get("tick")).cloned().unwrap_or(Json::Null);
                    entities = Json::from(j.get("entities").and_then(|e| e.as_array()).map_or(0, |e| e.len()));
                }
                out.push((slot.clone(), Json::obj([("slot", Json::str(slot)), ("bytes", Json::from(text.len())), ("tick", tick), ("entities", entities)])));
            }
        }
        Ok(Json::obj([("saves", Json::arr(out.into_iter().map(|(_, j)| j)))]))
    }
}
