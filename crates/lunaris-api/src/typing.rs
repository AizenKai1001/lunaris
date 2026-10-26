//! `type_text`: typing into a focused `UiInput`, one character per tick, as a person at a keyboard would; with `paste` all of it at once, as Ctrl+V does in a window.
use crate::engine::{Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;

impl Engine {
    pub(crate) fn type_text_tool(&mut self, a: &Args) -> Result<Json> {
        let text = a.str("text")?;
        let enter = a.0.get("enter").and_then(|v| v.as_bool()).unwrap_or(false);
        if a.0.get("paste").and_then(|v| v.as_bool()).unwrap_or(false) {
            // what Ctrl+V does in a window (`lunaris play`): everything goes into the focused field now, between ticks (a connect token is hundreds of characters)
            if text.chars().count() > 4096 {
                return Err(EcsError::new("out_of_range", "at most 4096 characters pasted at a time").at("text"));
            }
            let typed = lunaris_sim::ui_kit::type_into_focused(&mut self.sim.world, text.trim());
            return Ok(Json::obj([("typed", Json::from(typed)), ("ticks", Json::from(0)), ("tick", Json::from(self.sim.tick as i64))]));
        }
        if text.chars().count() > 200 {
            return Err(EcsError::new("out_of_range", "at most 200 characters at a time").at("text"));
        }
        let press = |e: &mut Engine, key: &str, value: f64| -> Result<()> {
            e.call("set_resource", &Json::obj([("name", Json::str(key)), ("value", Json::Float(value))]))?;
            e.call("step", &Json::obj([("ticks", Json::from(1))]))?;
            Ok(())
        };
        let mut typed = 0;
        for c in text.chars() {
            if c.is_control() {
                continue;
            }
            press(self, "input.char", c as u32 as f64)?;
            typed += 1;
        }
        if enter {
            press(self, "input.enter", 1.0)?;
        }
        Ok(Json::obj([("typed", Json::from(typed)), ("ticks", Json::from(typed + enter as usize)), ("tick", Json::from(self.sim.tick as i64))]))
    }
}
