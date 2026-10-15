//! One error type for the whole stack. Every error carries a stable `code`, a message and, when possible, a hint
//! that tells the caller (a person or an AI) what to do next.
use lunaris_json::Json;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct EcsError {
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
    /// Where in the input the problem is, e.g. `Transform.pos[1]`.
    pub path: Option<String>,
}

impl EcsError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        EcsError { code, message: message.into(), hint: None, path: None }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn to_json(&self) -> Json {
        let mut o = Json::obj([("code", Json::str(self.code)), ("message", Json::str(self.message.clone()))]);
        if let Some(h) = &self.hint {
            o.set("hint", Json::str(h.clone()));
        }
        if let Some(p) = &self.path {
            o.set("path", Json::str(p.clone()));
        }
        o
    }
}

impl fmt::Display for EcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(p) = &self.path {
            write!(f, " (at {p})")?;
        }
        if let Some(h) = &self.hint {
            write!(f, " - {h}")?;
        }
        Ok(())
    }
}

impl std::error::Error for EcsError {}

pub type Result<T> = std::result::Result<T, EcsError>;
