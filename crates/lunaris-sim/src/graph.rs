//! Graph systems: gameplay logic as data. A system is a JSON document (a typed expression/statement graph) that runs for every
//! entity matching a component query, inside the same deterministic fixed step as native systems. Because it is data it can be
//! written by an AI or a node editor, changed while the simulation runs, undone, recorded, replayed and saved with the scene,
//! with no compiler involved.
//!
//! ```json
//! { "name": "gravity", "each": ["Transform", "Velocity"],
//!   "do": [
//!     { "set": "Velocity.lin[1]", "to": { "sub": [ { "get": "Velocity.lin[1]" }, { "mul": [9.8, { "dt": true }] } ] } },
//!     { "if": { "lt": [ { "get": "Transform.pos[1]" }, 0 ] },
//!       "then": [ { "set": "Transform.pos[1]", "to": 0 },
//!                 { "set": "Velocity.lin[1]", "to": { "mul": [ { "neg": { "get": "Velocity.lin[1]" } }, 0.8 ] } } ] } ] }
//! ```
//!
//! Values are numbers (f64 inside the interpreter), booleans and interned strings. Writes convert to the field's type and refuse
//! non-finite numbers. Structural changes (`spawn`, `despawn`, `add`, `remove`) are queued and applied after the system has
//! visited every entity, ordered by the issuing entity's uid, so the result never depends on storage order.
use crate::command::{apply as apply_command, Command};
use crate::detmath;
use crate::physics::Contact;
use crate::rng::hash_rand;
use lunaris_ecs::{valid_identifier, valid_resource_name, ComponentId, EcsError, FieldType, Result, StateHasher, StringTable, World};
use lunaris_json::Json;
use std::sync::Arc;
use std::time::Instant;

pub(crate) const MAX_DEFERRED_PER_RUN: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Bool(bool),
    Sym(u32),
}

impl Val {
    fn kind(&self) -> &'static str {
        match self {
            Val::Num(_) => "a number",
            Val::Bool(_) => "a boolean",
            Val::Sym(_) => "a string",
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------
// compiled form
#[derive(Clone, Debug)]
pub(crate) struct FieldRef {
    pub(crate) term: usize,
    pub(crate) offset: usize,
    pub(crate) ty: FieldType,
    pub(crate) label: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Op1 {
    Neg,
    Abs,
    Sqrt,
    Floor,
    Ceil,
    Round,
    Sin,
    Cos,
    Not,
    // the math library: shared by the interpreter and the vm through `unary_math`
    Sign,
    Fract,
    Trunc,
    Tan,
    Atan,
    Exp,
    Ln,
}

/// The fallible math functions that are not plain IEEE operations. The interpreter and the batch vm both call exactly these,
/// so they cannot disagree.
pub(crate) type MathFn1 = fn(f64) -> std::result::Result<f64, &'static str>;
pub(crate) type MathFn2 = fn(f64, f64) -> std::result::Result<f64, &'static str>;

pub(crate) fn unary_math(op: Op1) -> Option<MathFn1> {
    let f: MathFn1 = match op {
        Op1::Sign => |x| Ok(if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }),
        Op1::Fract => |x| Ok(x - x.floor()),
        Op1::Trunc => |x| Ok(x.trunc()),
        Op1::Tan => |x| {
            let c = detmath::cos(x);
            if c.abs() < 1e-15 {
                Err("tan of an odd multiple of pi/2")
            } else {
                Ok(detmath::sin(x) / c)
            }
        },
        Op1::Atan => |x| Ok(detmath::atan(x)),
        Op1::Exp => |x| {
            let r = detmath::exp(x);
            if r.is_finite() { Ok(r) } else { Err("exp: the result is too large") }
        },
        Op1::Ln => |x| if x > 0.0 { Ok(detmath::ln(x)) } else { Err("ln of a number that is not positive") },
        _ => return None,
    };
    Some(f)
}

pub(crate) fn binary_math(op: Op2) -> Option<MathFn2> {
    let f: MathFn2 = match op {
        Op2::Atan2 => |y, x| Ok(detmath::atan2(y, x)),
        Op2::Hypot => |x, y| {
            let r = (x * x + y * y).sqrt();
            if r.is_finite() { Ok(r) } else { Err("hypot: the result is too large") }
        },
        Op2::Pow => |a, b| {
            let whole = b == b.floor();
            let r = if b == 0.0 {
                1.0
            } else if a == 0.0 {
                if b < 0.0 {
                    return Err("pow: zero to a negative power");
                }
                0.0
            } else if whole && b.abs() <= 1024.0 {
                // repeated squaring: exact steps, defined for negative bases too
                let (mut base, mut n, mut acc) = (a, b.abs() as u32, 1.0f64);
                while n > 0 {
                    if n & 1 == 1 {
                        acc *= base;
                    }
                    base *= base;
                    n >>= 1;
                }
                if b < 0.0 { 1.0 / acc } else { acc }
            } else if a < 0.0 {
                return Err("pow: a negative number to a fractional power");
            } else {
                detmath::exp(b * detmath::ln(a))
            };
            if r.is_finite() { Ok(r) } else { Err("pow: the result is too large") }
        },
        _ => return None,
    };
    Some(f)
}

/// `smoothstep(lo, hi, x)`: 0 below `lo`, 1 above `hi`, an S-curve between.
pub(crate) fn smoothstep(lo: f64, hi: f64, x: f64) -> std::result::Result<f64, &'static str> {
    if lo == hi {
        return Err("smoothstep: lo and hi are equal");
    }
    // max then min (not `clamp`): a NaN from an overflowing difference becomes 0 here, in both back ends alike
    #[allow(clippy::manual_clamp)]
    let t = ((x - lo) / (hi - lo)).max(0.0).min(1.0);
    Ok(t * t * (3.0 - 2.0 * t))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Op2 {
    Sub,
    Div,
    Mod,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Atan2,
    Pow,
    Hypot,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OpN {
    Add,
    Mul,
    Min,
    Max,
    And,
    Or,
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Const(Val),
    Get(FieldRef),
    Local(usize),
    Dt,
    Tick,
    Uid,
    /// The uid of the second entity of a contact (role `b`).
    UidB,
    /// A property of the contact being handled: 0 depth, 1..=3 the normal (a to b), 4 trigger.
    Contact(usize),
    /// A world resource, by index into `Compiled::res` (its value is read once per system run).
    Res(usize),
    Rand(Box<Expr>),
    U1(Op1, Box<Expr>),
    U2(Op2, Box<Expr>, Box<Expr>),
    N(OpN, Vec<Expr>),
    Select(Box<Expr>, Box<Expr>, Box<Expr>),
    Clamp(Box<Expr>, Box<Expr>, Box<Expr>),
    Lerp(Box<Expr>, Box<Expr>, Box<Expr>),
    Smooth(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Value noise in [-1, 1] at (x, y) for a seed.
    Noise(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    Set(FieldRef, Expr),
    Let(usize, Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Despawn,
    /// Contact systems: despawn, add to or remove from the entity in role `a` (0) or `b` (1).
    DespawnRole(usize),
    AddRole(usize, String, Vec<(String, Expr)>),
    RemoveRole(usize, String),
    ResSet(String, Expr),
    ResAdd(String, Expr),
    /// Announce an event (a sound, a message to the outside); see `Sim::events`.
    Emit(String),
    Spawn(Vec<(String, Vec<(String, Expr)>)>),
    /// Spawn a prefab: `with` is `(component, field values)`, `at` the root's `Transform.pos`, `velocity` its `Velocity.lin`.
    SpawnPrefab { name: String, with: Vec<(String, Vec<(String, Expr)>)>, at: Option<Vec<Expr>>, velocity: Option<Vec<Expr>> },
    Add(String, Vec<(String, Expr)>),
    Remove(String),
}

/// Which overlaps an `on_contact` system reacts to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Triggers {
    All,
    Only,
    Solid,
}

#[derive(Clone, Debug)]
pub(crate) struct ContactSpec {
    /// `terms[..na]` are the components required of role `a`, the rest of role `b`.
    pub(crate) na: usize,
    pub(crate) triggers: Triggers,
}

#[derive(Debug)]
pub(crate) struct Compiled {
    pub(crate) terms: Vec<ComponentId>,
    pub(crate) without: Vec<ComponentId>,
    pub(crate) body: Vec<Stmt>,
    pub(crate) nlocals: usize,
    pub(crate) nodes: usize,
    /// Names of the resources the system reads, in `Expr::Res` index order.
    pub(crate) res: Vec<String>,
    /// Set for `on_contact` systems.
    pub(crate) contact: Option<ContactSpec>,
    /// The batch form of `body`, when every construct in it could be lowered (see `vm`); `None` means interpret.
    pub(crate) program: Option<crate::vm::Program>,
    /// Set for `plugin` systems: native code does the work (see `plugin`).
    pub(crate) plugin: Option<crate::plugin::PluginSpec>,
}

#[derive(Clone, Debug)]
pub struct ScriptSystem {
    pub name: String,
    pub doc: String,
    pub order: i64,
    pub enabled: bool,
    /// The definition exactly as given (the source of truth; the compiled form is derived from it).
    pub def: Json,
    compiled: Arc<Compiled>,
    pub last_error: Option<String>,
    /// Wall-clock milliseconds of the last run (profiling information only, never part of the state).
    pub last_ms: f64,
}

impl ScriptSystem {
    /// A number that is the same for a system and its clones and different for a system that was defined again (even with the same
    /// text): `Sim::checkpoint` uses it to notice that the systems it did not copy have been replaced.
    pub(crate) fn identity(&self) -> usize {
        Arc::as_ptr(&self.compiled) as *const u8 as usize
    }

    pub fn each(&self) -> Vec<String> {
        self.def.get("each").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }

    pub fn nodes(&self) -> usize {
        self.compiled.nodes
    }

    /// Names of the world resources this system reads.
    pub fn resources(&self) -> &[String] {
        &self.compiled.res
    }

    /// Whether this system runs as a batch program (fast path) rather than through the interpreter.
    pub fn uses_batch(&self) -> bool {
        self.compiled.program.is_some()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScriptSet {
    pub systems: Vec<ScriptSystem>,
    /// Events emitted by graph systems since the simulation last collected them: (name, uid of the issuing entity).
    pub events: Vec<(String, u64, Option<[f32; 3]>)>,
    /// Run every graph system with the one-entity-at-a-time interpreter even when a batch program exists (for tests and comparisons).
    pub interpret_only: bool,
    /// `Some(workspace folder)` when the host allows native plugins (`plugin` systems); `None` = switched off. Host policy, not state.
    pub plugin_root: Option<std::path::PathBuf>,
    /// The sandboxed WebAssembly host (`wasm` systems), when the program embedding the engine installed one. Host policy, not state.
    pub wasm: Option<crate::plugin::WasmHost>,
    /// Named entity templates (see `prefab`), saved with the scene and part of the state hash.
    pub prefabs: crate::prefab::Prefabs,
    /// What each running behaviour tree remembers (see `gameplay`): by entity, the hash of its script and one number per node.
    pub bt: std::collections::BTreeMap<u64, (u64, Vec<f32>)>,
}

impl ScriptSet {
    pub fn get(&self, name: &str) -> Option<&ScriptSystem> {
        self.systems.iter().find(|s| s.name == name)
    }

    /// Insert or replace by name; returns the previous definition.
    pub fn put(&mut self, sys: ScriptSystem) -> Option<ScriptSystem> {
        if let Some(slot) = self.systems.iter_mut().find(|s| s.name == sys.name) {
            Some(std::mem::replace(slot, sys))
        } else {
            self.systems.push(sys);
            None
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<ScriptSystem> {
        let i = self.systems.iter().position(|s| s.name == name)?;
        Some(self.systems.remove(i))
    }

    /// Hash of everything that changes behaviour: definitions, order and enabled flags (sorted by name).
    pub fn hash_into(&self, h: &mut StateHasher) {
        let mut v: Vec<&ScriptSystem> = self.systems.iter().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        h.u64(v.len() as u64);
        for s in v {
            h.bytes(s.name.as_bytes());
            h.u64(s.order as u64);
            h.u64(s.enabled as u64);
            h.bytes(s.def.to_string_lossy().as_bytes());
            if let Some(p) = &s.compiled.plugin {
                h.u64(p.file_hash); // a recording made with one build of a plugin must not replay with another
            }
        }
        if !self.bt.is_empty() {
            h.u64(self.bt.len() as u64);
            for (uid, (script, mem)) in &self.bt {
                h.u64(*uid);
                h.u64(*script);
                for v in mem {
                    h.u64(v.to_bits() as u64);
                }
            }
        }
        h.u64(self.prefabs.len() as u64);
        for (name, def) in &self.prefabs {
            h.bytes(name.as_bytes());
            h.bytes(def.to_string_lossy().as_bytes());
        }
    }

    /// Definitions as stored in scenes (`enabled` is part of each definition object).
    pub fn defs_json(&self) -> Json {
        Json::arr(self.systems.iter().map(|s| {
            let mut d = s.def.clone();
            d.set("enabled", Json::Bool(s.enabled));
            d
        }))
    }
}

// ------------------------------------------------------------------------------------------------------------------
// compilation
struct Cx<'a> {
    world: &'a mut World,
    terms: Vec<(String, ComponentId)>,
    scopes: Vec<(String, usize)>,
    nlocals: usize,
    nodes: usize,
    res: Vec<String>,
    /// Some(na) in an `on_contact` system: paths start with `a.` or `b.`, and `terms[..na]` belong to role a.
    contact_na: Option<usize>,
}

const EXPR_OPS: &str = "get var res contact dt tick uid rand add sub mul div mod neg abs sign sqrt floor ceil round trunc fract sin cos tan atan atan2 exp ln pow hypot min max clamp lerp smoothstep noise lt le gt ge eq ne and or not select";
const STMT_KINDS: &str = "set let if despawn spawn spawn_prefab add remove res_set res_add emit";

fn bad(msg: impl Into<String>, at: &str) -> EcsError {
    EcsError::new("bad_graph", msg).at(at)
}

fn single_key<'j>(j: &'j Json, at: &str) -> Result<(&'j str, &'j Json)> {
    match j {
        Json::Obj(p) if p.len() == 1 => Ok((&p[0].0, &p[0].1)),
        Json::Obj(p) => Err(bad(format!("an operation object has exactly one key, this one has {}", p.len()), at).hint("write each operation as {\"op\": arguments}")),
        other => Err(bad(format!("expected an operation object, got {}", other.type_name()), at)),
    }
}

impl<'a> Cx<'a> {
    fn path(&self, path: &str, at: &str) -> Result<FieldRef> {
        // in an `on_contact` system a path starts with the role: a.Transform.pos[0], b.Orb.value
        let (range, path_in_role) = match self.contact_na {
            Some(na) => {
                let (role, rest) = path.split_once('.').ok_or_else(|| bad(format!("`{path}` is not a field path"), at).hint("in an on_contact system start with the role: a.Transform.pos[0] or b.Orb.value"))?;
                match role {
                    "a" => (0..na, rest),
                    "b" => (na..self.terms.len(), rest),
                    _ => return Err(bad(format!("`{path}` must start with `a.` or `b.`"), at).hint("in an on_contact system `a` is the first entity of the pair and `b` the second: a.Transform.pos[0]")),
                }
            }
            None => (0..self.terms.len(), path),
        };
        let (comp, rest) = path_in_role.split_once('.').ok_or_else(|| bad(format!("`{path}` is not a field path"), at).hint("use Component.field or Component.field[index], e.g. Transform.pos[1]"))?;
        let term = self.terms[range.clone()].iter().position(|(n, _)| n == comp).map(|p| p + range.start).ok_or_else(|| {
            let key = if self.contact_na.is_some() { "on_contact" } else { "each" };
            let list = self.terms[range.clone()].iter().map(|t| t.0.as_str()).collect::<Vec<_>>().join(", ");
            bad(format!("component `{comp}` is not in this system's `{key}` list"), at).hint(format!("the list is [{list}]; add {comp} to it to read or write {path}"))
        })?;
        let (fname, idx) = match rest.split_once('[') {
            Some((f, i)) => (f, Some(i.strip_suffix(']').and_then(|n| n.parse::<usize>().ok()).ok_or_else(|| bad(format!("bad array index in `{path}`"), at).hint("use field[0], field[1], ..."))?)),
            None => (rest, None),
        };
        let info = self.world.info(self.terms[term].1);
        let field = info.field(fname).ok_or_else(|| {
            bad(format!("{comp} has no field `{fname}`"), at).hint(format!("fields of {comp}: {}", info.fields.iter().map(|f| if f.count > 1 { format!("{}[{}]", f.name, f.count) } else { f.name.clone() }).collect::<Vec<_>>().join(", ")))
        })?;
        let elem = match (field.count, idx) {
            (1, None) => 0,
            (1, Some(0)) => 0,
            (1, Some(_)) => return Err(bad(format!("{comp}.{fname} is not an array"), at)),
            (n, Some(i)) if (i as u32) < n => i,
            (n, Some(i)) => return Err(bad(format!("index {i} is out of range for {comp}.{fname} (length {n})"), at)),
            (n, None) => return Err(bad(format!("{comp}.{fname} is an array of {n}: pick an element, e.g. {comp}.{fname}[0]"), at)),
        };
        Ok(FieldRef { term, offset: field.offset + elem * field.ty.size(), ty: field.ty, label: path.to_string() })
    }

    fn expr(&mut self, j: &Json, at: &str) -> Result<Expr> {
        self.nodes += 1;
        match j {
            Json::Int(i) => Ok(Expr::Const(Val::Num(*i as f64))),
            Json::Float(f) => Ok(Expr::Const(Val::Num(*f))),
            Json::Bool(b) => Ok(Expr::Const(Val::Bool(*b))),
            Json::Str(s) => Ok(Expr::Const(Val::Sym(self.world.strings_mut().intern(s)))),
            Json::Null => Err(bad("null is not a value", at)),
            Json::Arr(_) => Err(bad("an array is not a value here", at).hint("operations take their arguments inside the operation, e.g. {\"add\": [1, 2]}")),
            Json::Obj(_) => {
                let (op, arg) = single_key(j, at)?;
                let at = format!("{at}.{op}");
                match op {
                    "get" => Ok(Expr::Get(self.path(arg.as_str().ok_or_else(|| bad("`get` needs a field path string", &at))?, &at)?)),
                    "var" => {
                        let n = arg.as_str().ok_or_else(|| bad("`var` needs a variable name", &at))?;
                        let slot = self.scopes.iter().rev().find(|(name, _)| name == n).map(|(_, s)| *s).ok_or_else(|| bad(format!("no variable named `{n}` is defined at this point"), &at).hint("define it earlier with {\"let\": \"name\", \"be\": ...}"))?;
                        Ok(Expr::Local(slot))
                    }
                    "res" => {
                        let n = arg.as_str().filter(|n| valid_resource_name(n)).ok_or_else(|| bad("`res` needs a resource name such as \"score\" or \"input.space\"", &at).hint("letters, digits and underscores in dot-separated parts; a resource that was never set reads as 0"))?;
                        let idx = match self.res.iter().position(|r| r == n) {
                            Some(i) => i,
                            None => {
                                self.res.push(n.to_string());
                                self.res.len() - 1
                            }
                        };
                        Ok(Expr::Res(idx))
                    }
                    "dt" => Ok(Expr::Dt),
                    "tick" => Ok(Expr::Tick),
                    "uid" => match (self.contact_na, arg) {
                        (None, _) | (Some(_), Json::Bool(true)) => Ok(Expr::Uid),
                        (Some(_), Json::Str(r)) if r == "a" => Ok(Expr::Uid),
                        (Some(_), Json::Str(r)) if r == "b" => Ok(Expr::UidB),
                        _ => Err(bad("in an on_contact system `uid` is \"a\" or \"b\"", &at)),
                    },
                    "contact" => {
                        if self.contact_na.is_none() {
                            return Err(bad("`contact` can only be used in an on_contact system", &at).hint("define the system with \"on_contact\": {\"a\": [...], \"b\": [...]} instead of \"each\""));
                        }
                        let k = ["depth", "nx", "ny", "nz", "trigger"].iter().position(|n| arg.as_str() == Some(n)).ok_or_else(|| bad("`contact` is one of depth, nx, ny, nz, trigger", &at).hint("nx, ny and nz are the contact normal pointing from a to b"))?;
                        Ok(Expr::Contact(k))
                    }
                    "rand" => Ok(Expr::Rand(Box::new(self.expr(arg, &at)?))),
                    "neg" | "abs" | "sqrt" | "floor" | "ceil" | "round" | "sin" | "cos" | "not" | "sign" | "fract" | "trunc" | "tan" | "atan" | "exp" | "ln" => {
                        let a = self.one(arg, &at)?;
                        let o = match op {
                            "neg" => Op1::Neg,
                            "abs" => Op1::Abs,
                            "sqrt" => Op1::Sqrt,
                            "floor" => Op1::Floor,
                            "ceil" => Op1::Ceil,
                            "round" => Op1::Round,
                            "sin" => Op1::Sin,
                            "cos" => Op1::Cos,
                            "sign" => Op1::Sign,
                            "fract" => Op1::Fract,
                            "trunc" => Op1::Trunc,
                            "tan" => Op1::Tan,
                            "atan" => Op1::Atan,
                            "exp" => Op1::Exp,
                            "ln" => Op1::Ln,
                            _ => Op1::Not,
                        };
                        Ok(Expr::U1(o, Box::new(a)))
                    }
                    "sub" | "div" | "mod" | "lt" | "le" | "gt" | "ge" | "eq" | "ne" | "atan2" | "pow" | "hypot" => {
                        let args = self.args(arg, &at, 2, 2)?;
                        let mut it = args.into_iter();
                        let (a, b) = (it.next().unwrap(), it.next().unwrap());
                        let o = match op {
                            "sub" => Op2::Sub,
                            "div" => Op2::Div,
                            "mod" => Op2::Mod,
                            "lt" => Op2::Lt,
                            "le" => Op2::Le,
                            "gt" => Op2::Gt,
                            "ge" => Op2::Ge,
                            "eq" => Op2::Eq,
                            "atan2" => Op2::Atan2,
                            "pow" => Op2::Pow,
                            "hypot" => Op2::Hypot,
                            _ => Op2::Ne,
                        };
                        Ok(Expr::U2(o, Box::new(a), Box::new(b)))
                    }
                    "add" | "mul" | "min" | "max" | "and" | "or" => {
                        let args = self.args(arg, &at, 1, 64)?;
                        let o = match op {
                            "add" => OpN::Add,
                            "mul" => OpN::Mul,
                            "min" => OpN::Min,
                            "max" => OpN::Max,
                            "and" => OpN::And,
                            _ => OpN::Or,
                        };
                        Ok(Expr::N(o, args))
                    }
                    "select" | "clamp" | "lerp" | "smoothstep" | "noise" => {
                        let mut args = self.args(arg, &at, 3, 3)?.into_iter();
                        let (a, b, c) = (Box::new(args.next().unwrap()), Box::new(args.next().unwrap()), Box::new(args.next().unwrap()));
                        Ok(match op {
                            "select" => Expr::Select(a, b, c),
                            "clamp" => Expr::Clamp(a, b, c),
                            "smoothstep" => Expr::Smooth(a, b, c),
                            "noise" => Expr::Noise(a, b, c),
                            _ => Expr::Lerp(a, b, c),
                        })
                    }
                    other => Err(bad(format!("unknown operation `{other}`"), &at).hint(format!("expression operations: {EXPR_OPS}"))),
                }
            }
        }
    }

    /// The `"on": "a" | "b"` of `add` / `remove`: required in an `on_contact` system, not allowed elsewhere.
    fn role_of(&self, on: Option<&Json>, at: &str) -> Result<Option<usize>> {
        match (self.contact_na, on) {
            (None, None) => Ok(None),
            (None, Some(_)) => Err(bad("`on` is only used in on_contact systems", &format!("{at}.on")).hint("in an `each` system add and remove apply to the entity being visited")),
            (Some(_), Some(Json::Str(r))) if r == "a" || r == "b" => Ok(Some((r == "b") as usize)),
            (Some(_), _) => Err(bad("in an on_contact system say which entity with \"on\": \"a\" or \"on\": \"b\"", at)),
        }
    }

    fn one(&mut self, arg: &Json, at: &str) -> Result<Expr> {
        match arg {
            Json::Arr(a) if a.len() == 1 => self.expr(&a[0], &format!("{at}[0]")),
            Json::Arr(_) => Err(bad("this operation takes exactly one argument", at)),
            other => self.expr(other, at),
        }
    }

    fn args(&mut self, arg: &Json, at: &str, min: usize, max: usize) -> Result<Vec<Expr>> {
        let a = arg.as_array().ok_or_else(|| bad("expected an array of arguments", at).hint("e.g. {\"add\": [1, 2]}"))?;
        if a.len() < min || a.len() > max {
            return Err(bad(if min == max { format!("this operation takes exactly {min} arguments, got {}", a.len()) } else { format!("this operation takes {min} to {max} arguments, got {}", a.len()) }, at));
        }
        a.iter().enumerate().map(|(i, x)| self.expr(x, &format!("{at}[{i}]"))).collect()
    }

    fn values(&mut self, obj: &Json, at: &str) -> Result<Vec<(String, Expr)>> {
        let pairs = obj.as_object().ok_or_else(|| bad("expected an object of field values", at))?;
        pairs.iter().map(|(k, v)| Ok((k.clone(), self.expr(v, &format!("{at}.{k}"))?))).collect()
    }

    fn block(&mut self, list: &Json, at: &str) -> Result<Vec<Stmt>> {
        let arr = list.as_array().ok_or_else(|| bad("expected an array of statements", at))?;
        let mark = self.scopes.len();
        let mut out = Vec::with_capacity(arr.len());
        for (i, s) in arr.iter().enumerate() {
            out.push(self.stmt(s, &format!("{at}[{i}]"))?);
        }
        self.scopes.truncate(mark);
        Ok(out)
    }

    fn stmt(&mut self, j: &Json, at: &str) -> Result<Stmt> {
        self.nodes += 1;
        let pairs = j.as_object().ok_or_else(|| bad("expected a statement object", at).hint(format!("statements: {STMT_KINDS}")))?;
        let key = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v);
        let first = pairs.first().map(|(k, _)| k.as_str()).unwrap_or("");
        match first {
            "set" => {
                let path = key("set").and_then(|v| v.as_str()).ok_or_else(|| bad("`set` needs a field path string", at))?;
                let to = key("to").ok_or_else(|| bad("`set` needs a `to` value", at).hint("{\"set\": \"Transform.pos[0]\", \"to\": 5}"))?;
                let fr = self.path(path, &format!("{at}.set"))?;
                Ok(Stmt::Set(fr, self.expr(to, &format!("{at}.to"))?))
            }
            "let" => {
                let name = key("let").and_then(|v| v.as_str()).filter(|n| valid_identifier(n)).ok_or_else(|| bad("`let` needs a variable name (letters, digits, underscore)", at))?;
                let be = key("be").ok_or_else(|| bad("`let` needs a `be` value", at).hint("{\"let\": \"speed\", \"be\": {\"get\": \"Velocity.lin[0]\"}}"))?;
                let e = self.expr(be, &format!("{at}.be"))?;
                let slot = self.nlocals;
                self.nlocals += 1;
                self.scopes.push((name.to_string(), slot));
                Ok(Stmt::Let(slot, e))
            }
            "if" => {
                let cond = self.expr(key("if").ok_or_else(|| bad("`if` needs a condition", at))?, &format!("{at}.if"))?;
                let then = self.block(key("then").ok_or_else(|| bad("`if` needs a `then` list", at))?, &format!("{at}.then"))?;
                let els = match key("else") {
                    Some(e) => self.block(e, &format!("{at}.else"))?,
                    None => Vec::new(),
                };
                Ok(Stmt::If(cond, then, els))
            }
            "despawn" => match (self.contact_na, key("despawn")) {
                (None, _) => Ok(Stmt::Despawn),
                (Some(_), Some(Json::Str(r))) if r == "a" || r == "b" => Ok(Stmt::DespawnRole((r == "b") as usize)),
                _ => Err(bad("in an on_contact system say which entity: {\"despawn\": \"a\"} or {\"despawn\": \"b\"}", at)),
            },
            "emit" => {
                let name = key("emit").and_then(|v| v.as_str()).filter(|n| valid_resource_name(n)).ok_or_else(|| bad("`emit` needs an event name such as \"coin\"", at).hint("letters, digits and underscores in dot-separated parts: {\"emit\": \"coin\"}"))?;
                Ok(Stmt::Emit(name.to_string()))
            }
            "res_set" | "res_add" => {
                let name = key(first).and_then(|v| v.as_str()).filter(|n| valid_resource_name(n)).ok_or_else(|| bad(format!("`{first}` needs a resource name such as \"score\""), at))?;
                let (word, hint) = if first == "res_set" { ("to", "{\"res_set\": \"score\", \"to\": 0}") } else { ("by", "{\"res_add\": \"score\", \"by\": 10}") };
                let v = key(word).ok_or_else(|| bad(format!("`{first}` needs a `{word}` value"), at).hint(hint))?;
                let e = self.expr(v, &format!("{at}.{word}"))?;
                Ok(if first == "res_set" { Stmt::ResSet(name.to_string(), e) } else { Stmt::ResAdd(name.to_string(), e) })
            }
            "spawn" => {
                let comps = key("spawn").and_then(|v| v.as_object()).ok_or_else(|| bad("`spawn` needs an object of components", at).hint("{\"spawn\": {\"Transform\": {\"pos\": [..]}}}: give each field as a value, array fields as pos_0.. is not supported; spawn then use set from another system"))?;
                let mut out = Vec::new();
                for (cname, fields) in comps {
                    self.world.require_component(cname).map_err(|e| e.at(format!("{at}.spawn.{cname}")))?;
                    out.push((cname.clone(), self.values(fields, &format!("{at}.spawn.{cname}"))?));
                }
                Ok(Stmt::Spawn(out))
            }
            "spawn_prefab" => {
                let name = key("spawn_prefab").and_then(|v| v.as_str()).ok_or_else(|| bad("`spawn_prefab` needs the name of a prefab", at).hint("{\"spawn_prefab\": \"Bullet\", \"at\": [x, y, z], \"velocity\": [vx, vy, vz]} (define it with define_prefab or in the scene's `prefabs`)"))?;
                if let Some((k, _)) = pairs.iter().find(|(k, _)| !["spawn_prefab", "at", "velocity", "with"].contains(&k.as_str())) {
                    return Err(bad(format!("unknown key `{k}` in `spawn_prefab`"), at).hint("keys: spawn_prefab, at, velocity, with"));
                }
                let vec3 = |this: &mut Self, k: &str| -> Result<Option<Vec<Expr>>> {
                    match key(k) {
                        None => Ok(None),
                        Some(Json::Arr(a)) if a.len() == 3 => a.iter().enumerate().map(|(i, x)| this.expr(x, &format!("{at}.{k}[{i}]"))).collect::<Result<Vec<_>>>().map(Some),
                        Some(_) => Err(bad(format!("`{k}` must be an array of three values [x, y, z]"), at)),
                    }
                };
                let (pos, vel) = (vec3(self, "at")?, vec3(self, "velocity")?);
                let mut with = Vec::new();
                match key("with") {
                    None => {}
                    Some(Json::Obj(comps)) => {
                        for (cname, fields) in comps {
                            self.world.require_component(cname).map_err(|e| e.at(format!("{at}.with.{cname}")))?;
                            with.push((cname.clone(), self.values(fields, &format!("{at}.with.{cname}"))?));
                        }
                    }
                    Some(_) => return Err(bad("`with` must be an object of components: {\"Health\": {\"hp\": 5}}", at)),
                }
                Ok(Stmt::SpawnPrefab { name: name.to_string(), with, at: pos, velocity: vel })
            }
            "add" => {
                let cname = key("add").and_then(|v| v.as_str()).ok_or_else(|| bad("`add` needs a component name", at))?;
                self.world.require_component(cname).map_err(|e| e.at(format!("{at}.add")))?;
                let values = match key("values") {
                    Some(v) => self.values(v, &format!("{at}.values"))?,
                    None => Vec::new(),
                };
                match self.role_of(key("on"), at)? {
                    Some(r) => Ok(Stmt::AddRole(r, cname.to_string(), values)),
                    None => Ok(Stmt::Add(cname.to_string(), values)),
                }
            }
            "remove" => {
                let cname = key("remove").and_then(|v| v.as_str()).ok_or_else(|| bad("`remove` needs a component name", at))?;
                self.world.require_component(cname).map_err(|e| e.at(format!("{at}.remove")))?;
                match self.role_of(key("on"), at)? {
                    Some(r) => Ok(Stmt::RemoveRole(r, cname.to_string())),
                    None => Ok(Stmt::Remove(cname.to_string())),
                }
            }
            other => Err(bad(format!("unknown statement `{other}`"), at).hint(format!("statements: {STMT_KINDS}"))),
        }
    }
}

/// Check and compile a system definition.
pub fn compile(world: &mut World, def: &Json) -> Result<ScriptSystem> {
    compile_with(world, def, None, None)
}

/// `compile`, with the workspace folder native plugins may be loaded from (`None` = plugins are not allowed).
pub fn compile_with(world: &mut World, def: &Json, plugin_root: Option<&std::path::Path>, wasm: Option<&crate::plugin::WasmHost>) -> Result<ScriptSystem> {
    let obj = def.as_object().ok_or_else(|| bad("a system definition must be an object", "").hint("{\"name\": ..., \"each\": [...], \"do\": [...]}"))?;
    for (k, _) in obj {
        if !["name", "doc", "order", "each", "on_contact", "without", "do", "enabled", "plugin", "wasm", "fuel", "export", "fields", "writes", "res", "parallel"].contains(&k.as_str()) {
            return Err(bad(format!("unknown key `{k}` in a system definition"), k).hint("keys: name, doc, order, each (or on_contact), without, do, enabled; plugin systems also plugin, export, fields, writes, res, parallel; wasm systems wasm, export, fuel, fields, writes, res"));
        }
    }
    let name = def.get("name").and_then(|v| v.as_str()).filter(|n| valid_identifier(n) && n.len() <= 64).ok_or_else(|| bad("`name` must be a string of letters, digits and underscores (max 64)", "name"))?;
    let mut terms: Vec<(String, ComponentId)> = Vec::new();
    let mut without = Vec::new();
    let mut contact_na: Option<usize> = None;
    let mut triggers = Triggers::All;
    match (def.get("each"), def.get("on_contact")) {
        (Some(_), Some(_)) => return Err(bad("a system has `each` or `on_contact`, not both", "on_contact").hint("`each` visits entities one at a time; `on_contact` runs for each pair of touching entities")),
        (None, Some(oc)) => {
            let ob = oc.as_object().ok_or_else(|| bad("`on_contact` must be an object like {\"a\": [\"Player\"], \"b\": [\"Orb\"]}", "on_contact"))?;
            for (k, _) in ob {
                if !["a", "b", "triggers"].contains(&k.as_str()) {
                    return Err(bad(format!("unknown key `{k}` in `on_contact`"), &format!("on_contact.{k}")).hint("keys: a, b, triggers"));
                }
            }
            if def.get("without").is_some() {
                return Err(bad("`without` is not used with `on_contact`", "without"));
            }
            for role in ["a", "b"] {
                let list = oc.get(role).and_then(|v| v.as_array()).filter(|l| !l.is_empty()).ok_or_else(|| bad(format!("`on_contact.{role}` must list the components that entity needs"), &format!("on_contact.{role}")).hint("e.g. \"on_contact\": {\"a\": [\"Player\"], \"b\": [\"Orb\"]}"))?;
                let start = terms.len();
                for (i, c) in list.iter().enumerate() {
                    let at = format!("on_contact.{role}[{i}]");
                    let cname = c.as_str().ok_or_else(|| bad("component names are strings", &at))?;
                    let cid = world.require_component(cname).map_err(|e| e.at(at.clone()))?;
                    if terms[start..].iter().any(|(n, _)| n == cname) {
                        return Err(bad(format!("`{cname}` is listed twice"), &at));
                    }
                    terms.push((cname.to_string(), cid));
                }
                if role == "a" {
                    contact_na = Some(terms.len());
                }
            }
            triggers = match oc.get("triggers").map(|v| v.as_str()) {
                None | Some(Some("all")) => Triggers::All,
                Some(Some("only")) => Triggers::Only,
                Some(Some("solid")) => Triggers::Solid,
                _ => return Err(bad("`triggers` is \"all\", \"only\" or \"solid\"", "on_contact.triggers").hint("\"only\" reacts to trigger overlaps, \"solid\" to real collisions, \"all\" (the default) to both")),
            };
        }
        (each, None) => {
            let each = each.and_then(|v| v.as_array()).filter(|a| !a.is_empty()).ok_or_else(|| bad("`each` must list the components an entity needs to be visited", "each").hint("e.g. \"each\": [\"Transform\", \"Velocity\"]; or use \"on_contact\" to react to touching pairs"))?;
            for (i, c) in each.iter().enumerate() {
                let cname = c.as_str().ok_or_else(|| bad("component names are strings", &format!("each[{i}]")))?;
                let cid = world.require_component(cname).map_err(|e| e.at(format!("each[{i}]")))?;
                if terms.iter().any(|(n, _): &(String, ComponentId)| n == cname) {
                    return Err(bad(format!("`{cname}` is listed twice"), &format!("each[{i}]")));
                }
                terms.push((cname.to_string(), cid));
            }
            if let Some(w) = def.get("without") {
                for (i, c) in w.as_array().ok_or_else(|| bad("`without` must be an array of component names", "without"))?.iter().enumerate() {
                    let cname = c.as_str().ok_or_else(|| bad("component names are strings", &format!("without[{i}]")))?;
                    without.push(world.require_component(cname).map_err(|e| e.at(format!("without[{i}]")))?);
                }
            }
        }
    }
    let order = match def.get("order") {
        None => 100,
        Some(v) => v.as_i64().ok_or_else(|| bad("`order` must be an integer", "order"))?,
    };
    let enabled = def.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    if def.get("plugin").is_some() && def.get("wasm").is_some() {
        return Err(bad("a system is a native `plugin` or a sandboxed `wasm` module, not both", "wasm"));
    }
    if let Some(plugin) = def.get("plugin") {
        return compile_plugin(world, def, name, plugin, false, terms, without, contact_na, order, enabled, plugin_root, wasm);
    }
    if let Some(module) = def.get("wasm") {
        return compile_plugin(world, def, name, module, true, terms, without, contact_na, order, enabled, plugin_root, wasm);
    }
    for k in ["export", "fields", "writes", "res", "parallel", "fuel"] {
        if def.get(k).is_some() {
            return Err(bad(format!("`{k}` belongs to plugin and wasm systems"), k).hint("a plugin system has `plugin` (a shared library in the workspace), a wasm system has `wasm` (a .wasm file); graph systems have `do`"));
        }
    }
    let mut cx = Cx { world, terms: terms.clone(), scopes: Vec::new(), nlocals: 0, nodes: 0, res: Vec::new(), contact_na };
    let body = cx.block(def.get("do").ok_or_else(|| bad("`do` (the list of statements) is missing", "do"))?, "do")?;
    let (nlocals, nodes, res) = (cx.nlocals, cx.nodes, cx.res);
    let program = if contact_na.is_some() { None } else { crate::vm::lower(&body, nlocals) };
    let mut clean = def.clone();
    if let Json::Obj(p) = &mut clean {
        p.retain(|(k, _)| k != "enabled");
    }
    Ok(ScriptSystem {
        name: name.to_string(),
        doc: def.get("doc").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        order,
        enabled,
        def: clean,
        compiled: Arc::new(Compiled { terms: terms.into_iter().map(|t| t.1).collect(), without, body, nlocals, nodes, res, contact: contact_na.map(|na| ContactSpec { na, triggers }), program, plugin: None }),
        last_error: None,
        last_ms: 0.0,
    })
}

/// A system whose work is done by a native plugin: `{"plugin": "plugins/x.dll", "each": [...], "fields": [...], "writes": [...], "res": [...]}`.
#[allow(clippy::too_many_arguments)]
fn compile_plugin(world: &mut World, def: &Json, name: &str, plugin: &Json, is_wasm: bool, terms: Vec<(String, ComponentId)>, without: Vec<ComponentId>, contact_na: Option<usize>, order: i64, enabled: bool, plugin_root: Option<&std::path::Path>, wasm: Option<&crate::plugin::WasmHost>) -> Result<ScriptSystem> {
    let key = if is_wasm { "wasm" } else { "plugin" };
    let path = plugin.as_str().filter(|p| !p.is_empty()).ok_or_else(|| {
        if is_wasm {
            bad("`wasm` is the path of a WebAssembly module inside the workspace", "wasm").hint("e.g. \"wasm\": \"wasm/swarm.wasm\"")
        } else {
            bad("`plugin` is the path of a shared library inside the workspace", "plugin").hint("e.g. \"plugin\": \"plugins/swarm.dll\" (Windows), \"plugins/libswarm.so\" (Linux)")
        }
    })?;
    if def.get("do").is_some() || contact_na.is_some() {
        return Err(bad(format!("a {key} system has `each` and `{key}`, not `do` or `on_contact`"), key).hint("the compiled code does what `do` would"));
    }
    let root = if is_wasm {
        None
    } else {
        Some(plugin_root.ok_or_else(|| {
            bad("native plugins are switched off", "plugin").hint("start the engine with --allow-plugins (or set LUNARIS_ALLOW_PLUGINS=1, or call Engine::allow_plugins) if you trust this plugin: it runs as native code; or use a sandboxed `wasm` module, which needs no permission")
        })?)
    };
    let wasm_host = if is_wasm { Some(wasm.ok_or_else(|| bad("this program has no WebAssembly host", "wasm").hint("the `lunaris` command line has one; a program that embeds the engine installs one in `ScriptSet::wasm` (`Engine::new` does)"))?) } else { None };
    let strings = |key: &str| -> Result<Vec<String>> {
        match def.get(key) {
            None => Ok(Vec::new()),
            Some(v) => v
                .as_array()
                .ok_or_else(|| bad(format!("`{key}` must be an array of strings"), key))?
                .iter()
                .enumerate()
                .map(|(i, s)| s.as_str().map(str::to_string).ok_or_else(|| bad("expected a string", &format!("{key}[{i}]"))))
                .collect(),
        }
    };
    let field_paths = strings("fields")?;
    let write_paths = strings("writes")?;
    let res = strings("res")?;
    if field_paths.len() > 256 || res.len() > 256 {
        return Err(bad("a plugin system may list at most 256 fields and 256 resources", "fields"));
    }
    let cx = Cx { world, terms: terms.clone(), scopes: Vec::new(), nlocals: 0, nodes: 0, res: Vec::new(), contact_na: None };
    let mut fields = Vec::with_capacity(field_paths.len());
    for (i, p) in field_paths.iter().enumerate() {
        fields.push(cx.path(p, &format!("fields[{i}]"))?);
    }
    let mut writes = Vec::new();
    for (i, p) in write_paths.iter().enumerate() {
        let at = format!("writes[{i}]");
        let idx = field_paths.iter().position(|f| f == p).ok_or_else(|| bad(format!("`{p}` is written but not listed in `fields`"), &at).hint("every written field must also be in `fields`, which is the list the plugin sees (by index)"))?;
        if !writes.contains(&idx) {
            writes.push(idx);
        }
    }
    for (i, r) in res.iter().enumerate() {
        if !valid_resource_name(r) {
            return Err(bad(format!("`{r}` is not a valid resource name"), &format!("res[{i}]")));
        }
    }
    let export = def.get("export").map(|e| e.as_str().ok_or_else(|| bad("`export` is the name of the update function", "export"))).transpose()?.unwrap_or("lunaris_update");
    let parallel = def.get("parallel").and_then(|p| p.as_bool()).unwrap_or(false);
    let spec = if let Some(host) = wasm_host {
        let fuel = match def.get("fuel") {
            None => 20_000_000,
            Some(v) => v.as_u64().filter(|f| (1000..=4_000_000_000).contains(f)).ok_or_else(|| bad("`fuel` is the instruction budget of one call: an integer from 1000 to 4000000000", "fuel"))?,
        };
        crate::plugin::load_wasm(host, path, export, fuel, fields, writes).map_err(|m| bad(m, "wasm"))?
    } else {
        crate::plugin::load(root.expect("native systems have a root"), path, export, fields, writes, parallel).map_err(|m| bad(m, "plugin"))?
    };
    let mut clean = def.clone();
    if let Json::Obj(p) = &mut clean {
        p.retain(|(k, _)| k != "enabled");
    }
    Ok(ScriptSystem {
        name: name.to_string(),
        doc: def.get("doc").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        order,
        enabled,
        def: clean,
        compiled: Arc::new(Compiled { terms: terms.into_iter().map(|t| t.1).collect(), without, body: Vec::new(), nlocals: 0, nodes: 0, res, contact: None, program: None, plugin: Some(spec) }),
        last_error: None,
        last_ms: 0.0,
    })
}

// ------------------------------------------------------------------------------------------------------------------
// execution
struct Env<'a> {
    bases: Vec<*mut u8>,
    sizes: Vec<usize>,
    row: usize,
    locals: Vec<Val>,
    uid: u64,
    /// Contact systems: the uid of the second entity and the contact's properties.
    uid_b: u64,
    contact: [f64; 5],
    dt: f64,
    tick: u64,
    seed: u64,
    strings: &'a StringTable,
    /// Values of the resources this system reads (see `Compiled::res`), taken before any entity runs.
    res: &'a [f64],
}

pub(crate) enum DOp {
    Emit(String),
    DespawnUid(u64),
    AddUid(u64, String, Json),
    RemoveUid(u64, String),
    ResSet(String, f64),
    ResAdd(String, f64),
    Spawn(Vec<(String, Json)>),
    SpawnPrefab(String, Vec<(String, Json)>),
    Despawn,
    Add(String, Json),
    Remove(String),
}

fn num(v: Val) -> std::result::Result<f64, String> {
    match v {
        Val::Num(x) => Ok(x),
        other => Err(format!("expected a number, got {}", other.kind())),
    }
}

fn boolean(v: Val) -> std::result::Result<bool, String> {
    match v {
        Val::Bool(b) => Ok(b),
        other => Err(format!("expected a boolean, got {}", other.kind())),
    }
}

impl Env<'_> {
    /// # Safety
    /// The pointers in `bases` must be valid for the current row.
    unsafe fn read(&self, f: &FieldRef) -> Val {
        // SAFETY: the base points at the column of this term, the row is in range, offsets come from the layout.
        unsafe {
            let p = self.bases[f.term].add(self.row * self.sizes[f.term] + f.offset);
            match f.ty {
                FieldType::F32 => Val::Num(p.cast::<f32>().read() as f64),
                FieldType::F64 => Val::Num(p.cast::<f64>().read()),
                FieldType::I32 => Val::Num(p.cast::<i32>().read() as f64),
                FieldType::U32 => Val::Num(p.cast::<u32>().read() as f64),
                FieldType::I64 => Val::Num(p.cast::<i64>().read() as f64),
                FieldType::U64 | FieldType::Uid => Val::Num(p.cast::<u64>().read() as f64),
                FieldType::Bool => Val::Bool(p.read() != 0),
                FieldType::Str => Val::Sym(p.cast::<u32>().read()),
            }
        }
    }

    /// # Safety
    /// As for `read`; the caller guarantees no other reference to this field is live.
    unsafe fn write(&self, f: &FieldRef, v: Val) -> std::result::Result<(), String> {
        // SAFETY: as above.
        unsafe {
            let p = self.bases[f.term].add(self.row * self.sizes[f.term] + f.offset);
            match f.ty {
                FieldType::F32 | FieldType::F64 | FieldType::I32 | FieldType::U32 | FieldType::I64 | FieldType::U64 | FieldType::Uid => {
                    let x = num(v).map_err(|e| format!("cannot set {}: {e}", f.label))?;
                    if !x.is_finite() {
                        return Err(format!("cannot set {} to a non-finite number", f.label));
                    }
                    match f.ty {
                        FieldType::F32 => {
                            let y = x as f32;
                            if !y.is_finite() {
                                return Err(format!("{x} does not fit in the f32 field {}", f.label));
                            }
                            p.cast::<f32>().write(y)
                        }
                        FieldType::F64 => p.cast::<f64>().write(x),
                        FieldType::I32 => p.cast::<i32>().write(x as i32),
                        FieldType::U32 => p.cast::<u32>().write(x as u32),
                        FieldType::I64 => p.cast::<i64>().write(x as i64),
                        _ => p.cast::<u64>().write(x as u64),
                    }
                }
                FieldType::Bool => p.write(boolean(v).map_err(|e| format!("cannot set {}: {e}", f.label))? as u8),
                FieldType::Str => match v {
                    Val::Sym(s) => p.cast::<u32>().write(s),
                    other => return Err(format!("cannot set {}: expected a string, got {}", f.label, other.kind())),
                },
            }
        }
        Ok(())
    }
}

fn eval(e: &Expr, env: &Env) -> std::result::Result<Val, String> {
    Ok(match e {
        Expr::Const(v) => *v,
        // SAFETY: the environment's pointers are valid for the current row (see `run`).
        Expr::Get(f) => unsafe { env.read(f) },
        Expr::Local(i) => env.locals[*i],
        Expr::Dt => Val::Num(env.dt),
        Expr::Tick => Val::Num(env.tick as f64),
        Expr::Uid => Val::Num(env.uid as f64),
        Expr::UidB => Val::Num(env.uid_b as f64),
        Expr::Contact(k) => Val::Num(env.contact[*k]),
        Expr::Res(i) => Val::Num(env.res[*i]),
        Expr::Rand(salt) => {
            let s = num(eval(salt, env)?)?;
            Val::Num(hash_rand(env.seed, env.tick, env.uid, s as u64) as f64)
        }
        Expr::U1(op, a) => {
            let v = eval(a, env)?;
            match op {
                Op1::Not => Val::Bool(!boolean(v)?),
                _ => {
                    let x = num(v)?;
                    Val::Num(match op {
                        Op1::Neg => -x,
                        Op1::Abs => x.abs(),
                        Op1::Sqrt => {
                            if x < 0.0 {
                                return Err("sqrt of a negative number".into());
                            }
                            x.sqrt()
                        }
                        Op1::Floor => x.floor(),
                        Op1::Ceil => x.ceil(),
                        Op1::Round => (x + 0.5).floor(),
                        Op1::Sin => detmath::sin(x),
                        Op1::Cos => detmath::cos(x),
                        Op1::Not => unreachable!(),
                        other => unary_math(*other).expect("every other operation is in the math library")(x)?,
                    })
                }
            }
        }
        Expr::U2(op, a, b) => {
            let (x, y) = (eval(a, env)?, eval(b, env)?);
            match op {
                Op2::Eq => Val::Bool(x == y),
                Op2::Ne => Val::Bool(x != y),
                _ => {
                    let (x, y) = (num(x)?, num(y)?);
                    match op {
                        Op2::Sub => Val::Num(x - y),
                        Op2::Div => {
                            if y == 0.0 {
                                return Err("division by zero".into());
                            }
                            Val::Num(x / y)
                        }
                        Op2::Mod => {
                            if y == 0.0 {
                                return Err("modulo by zero".into());
                            }
                            Val::Num(x - y * (x / y).floor())
                        }
                        Op2::Lt => Val::Bool(x < y),
                        Op2::Le => Val::Bool(x <= y),
                        Op2::Gt => Val::Bool(x > y),
                        Op2::Ge => Val::Bool(x >= y),
                        other => Val::Num(binary_math(*other).expect("every other operation is in the math library")(x, y)?),
                    }
                }
            }
        }
        Expr::N(op, args) => match op {
            OpN::And | OpN::Or => {
                let mut acc = matches!(op, OpN::And);
                for a in args {
                    let b = boolean(eval(a, env)?)?;
                    if matches!(op, OpN::And) {
                        acc = acc && b;
                        if !acc {
                            break;
                        }
                    } else {
                        acc = acc || b;
                        if acc {
                            break;
                        }
                    }
                }
                Val::Bool(acc)
            }
            _ => {
                let mut acc = num(eval(&args[0], env)?)?;
                for a in &args[1..] {
                    let x = num(eval(a, env)?)?;
                    acc = match op {
                        OpN::Add => acc + x,
                        OpN::Mul => acc * x,
                        OpN::Min => acc.min(x),
                        _ => acc.max(x),
                    };
                }
                Val::Num(acc)
            }
        },
        Expr::Select(c, a, b) => {
            if boolean(eval(c, env)?)? {
                eval(a, env)?
            } else {
                eval(b, env)?
            }
        }
        Expr::Clamp(x, lo, hi) => {
            let (x, lo, hi) = (num(eval(x, env)?)?, num(eval(lo, env)?)?, num(eval(hi, env)?)?);
            if lo > hi {
                return Err("clamp: the lower bound is above the upper bound".into());
            }
            Val::Num(x.max(lo).min(hi))
        }
        Expr::Lerp(a, b, t) => {
            let (a, b, t) = (num(eval(a, env)?)?, num(eval(b, env)?)?, num(eval(t, env)?)?);
            Val::Num(a + (b - a) * t)
        }
        Expr::Smooth(lo, hi, x) => {
            let (lo, hi, x) = (num(eval(lo, env)?)?, num(eval(hi, env)?)?, num(eval(x, env)?)?);
            Val::Num(smoothstep(lo, hi, x)?)
        }
        Expr::Noise(x, y, seed) => {
            let (x, y, seed) = (num(eval(x, env)?)?, num(eval(y, env)?)?, num(eval(seed, env)?)?);
            Val::Num(crate::noise::noise2(x, y, seed))
        }
    })
}

fn to_json(v: Val, strings: &StringTable) -> Json {
    match v {
        Val::Num(x) => Json::Float(x),
        Val::Bool(b) => Json::Bool(b),
        Val::Sym(s) => Json::Str(strings.get(s).to_string()),
    }
}

fn values_json(vals: &[(String, Expr)], env: &Env) -> std::result::Result<Json, String> {
    let mut o = Vec::with_capacity(vals.len());
    for (k, e) in vals {
        o.push((k.clone(), to_json(eval(e, env)?, env.strings)));
    }
    Ok(Json::Obj(o))
}

fn exec(stmts: &[Stmt], env: &mut Env, out: &mut Vec<(u64, u32, DOp)>, seq: &mut u32) -> std::result::Result<(), String> {
    for s in stmts {
        match s {
            Stmt::Set(f, e) => {
                let v = eval(e, env)?;
                // SAFETY: bases are valid for this row; the interpreter holds no other reference into the world.
                unsafe { env.write(f, v)? };
            }
            Stmt::Let(slot, e) => {
                let v = eval(e, env)?;
                env.locals[*slot] = v;
            }
            Stmt::If(c, a, b) => {
                if boolean(eval(c, env)?)? {
                    exec(a, env, out, seq)?;
                } else {
                    exec(b, env, out, seq)?;
                }
            }
            Stmt::Despawn => push(out, env.uid, seq, DOp::Despawn)?,
            Stmt::Emit(name) => push(out, env.uid, seq, DOp::Emit(name.clone()))?,
            Stmt::DespawnRole(r) => {
                let target = if *r == 0 { env.uid } else { env.uid_b };
                push(out, env.uid, seq, DOp::DespawnUid(target))?;
            }
            Stmt::AddRole(r, name, fields) => {
                let target = if *r == 0 { env.uid } else { env.uid_b };
                let value = values_json(fields, env)?;
                push(out, env.uid, seq, DOp::AddUid(target, name.clone(), value))?;
            }
            Stmt::RemoveRole(r, name) => {
                let target = if *r == 0 { env.uid } else { env.uid_b };
                push(out, env.uid, seq, DOp::RemoveUid(target, name.clone()))?;
            }
            Stmt::ResSet(name, e) | Stmt::ResAdd(name, e) => {
                let x = num(eval(e, env)?)?;
                if !x.is_finite() {
                    return Err(format!("resource {name} cannot be set to a non-finite number"));
                }
                let op = if matches!(s, Stmt::ResSet(..)) { DOp::ResSet(name.clone(), x) } else { DOp::ResAdd(name.clone(), x) };
                push(out, env.uid, seq, op)?;
            }
            Stmt::Spawn(comps) => {
                let mut v = Vec::new();
                for (name, fields) in comps {
                    v.push((name.clone(), values_json(fields, env)?));
                }
                push(out, env.uid, seq, DOp::Spawn(v))?;
            }
            Stmt::SpawnPrefab { name, with, at, velocity } => {
                let mut over: Vec<(String, Json)> = Vec::new();
                for (cname, fields) in with {
                    crate::prefab::merge_component(&mut over, cname, &values_json(fields, env)?);
                }
                for (cname, field, exprs) in [("Transform", "pos", at), ("Velocity", "lin", velocity)] {
                    if let Some(exprs) = exprs {
                        let mut v = Vec::with_capacity(3);
                        for e in exprs {
                            v.push(to_json(eval(e, env)?, env.strings));
                        }
                        crate::prefab::merge_component(&mut over, cname, &Json::obj([(field, Json::Arr(v))]));
                    }
                }
                push(out, env.uid, seq, DOp::SpawnPrefab(name.clone(), over))?;
            }
            Stmt::Add(name, fields) => {
                let value = values_json(fields, env)?;
                push(out, env.uid, seq, DOp::Add(name.clone(), value))?;
            }
            Stmt::Remove(name) => push(out, env.uid, seq, DOp::Remove(name.clone()))?,
        }
    }
    Ok(())
}

fn push(out: &mut Vec<(u64, u32, DOp)>, uid: u64, seq: &mut u32, op: DOp) -> std::result::Result<(), String> {
    if out.len() >= MAX_DEFERRED_PER_RUN {
        return Err(format!("more than {MAX_DEFERRED_PER_RUN} structural changes in one tick"));
    }
    *seq += 1;
    out.push((uid, *seq, op));
    Ok(())
}

/// Run every enabled script system, in (order, definition) order. Errors are recorded on the system (the offending
/// entity's remaining statements are skipped) and never abort the simulation.
pub fn run_all(world: &mut World, scripts: &mut ScriptSet, contacts: &[Contact], tick: u64, dt: f32, seed: u64, threads: usize) {
    let mut emitted: Vec<(String, u64, Option<[f32; 3]>)> = Vec::new();
    let mut order: Vec<usize> = (0..scripts.systems.len()).filter(|&i| scripts.systems[i].enabled).collect();
    order.sort_by_key(|&i| (scripts.systems[i].order, i));
    for i in order {
        let t0 = Instant::now();
        let compiled = scripts.systems[i].compiled.clone();
        let name = scripts.systems[i].name.clone();
        let error = run_one(world, &compiled, &name, tick, dt, seed, threads, scripts.interpret_only, contacts, &scripts.prefabs, &mut emitted);
        let s = &mut scripts.systems[i];
        s.last_ms = t0.elapsed().as_secs_f64() * 1000.0;
        if error.is_some() {
            s.last_error = error;
        }
    }
    scripts.events.extend(emitted);
}

/// The batch (vm) path: the same work as `run_chunks`, but rows are processed `vm::B` at a time by the lowered program.
fn run_chunks_vm(world: &World, c: &Compiled, chunks: &[lunaris_ecs::Chunk], next: &std::sync::atomic::AtomicUsize, res: &[f64], tick: u64, dt: f32, seed: u64) -> (Vec<(u64, u32, DOp)>, Option<(u64, String)>) {
    let prog = c.program.as_ref().expect("checked by the caller");
    let sizes: Vec<usize> = c.terms.iter().map(|&t| world.info(t).size).collect();
    let mut machine = crate::vm::Machine::new(prog);
    let mut deferred: Vec<(u64, u32, DOp)> = Vec::new();
    let mut best: Option<(u64, String)> = None;
    let mut bases: Vec<*mut u8> = vec![std::ptr::null_mut(); c.terms.len()];
    let mut uids = [0u64; crate::vm::B];
    loop {
        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(ch) = chunks.get(k) else { break };
        let arch = &world.archetypes()[ch.arch];
        for (slot, &t) in bases.iter_mut().zip(&c.terms) {
            *slot = arch.column_base(t).expect("matching archetype has the component");
        }
        let ctx = crate::vm::Ctx { bases: &bases, sizes: &sizes, res, tick, dt: dt as f64, seed, strings: world.strings() };
        let mut row = ch.start;
        while row < ch.end {
            let n = (ch.end - row).min(crate::vm::B);
            for (i, u) in uids[..n].iter_mut().enumerate() {
                *u = world.uid_of(arch.entities()[row + i]).unwrap_or(0);
            }
            crate::vm::run_batch(&mut machine, prog, &ctx, row, &uids[..n], &mut deferred, &mut best);
            row += n;
        }
    }
    (deferred, best)
}

fn run_chunks(world: &World, c: &Compiled, chunks: &[lunaris_ecs::Chunk], next: &std::sync::atomic::AtomicUsize, res: &[f64], tick: u64, dt: f32, seed: u64, interpret_only: bool) -> (Vec<(u64, u32, DOp)>, Option<(u64, String)>) {
    if let Some(p) = &c.plugin {
        return crate::plugin::run_chunks(world, p, &c.terms, chunks, next, res, tick, dt, seed);
    }
    if !interpret_only && c.program.is_some() {
        return run_chunks_vm(world, c, chunks, next, res, tick, dt, seed);
    }
    let sizes: Vec<usize> = c.terms.iter().map(|&t| world.info(t).size).collect();
    let mut env = Env { bases: vec![std::ptr::null_mut(); c.terms.len()], sizes, row: 0, locals: vec![Val::Num(0.0); c.nlocals], uid: 0, uid_b: 0, contact: [0.0; 5], dt: dt as f64, tick, seed, strings: world.strings(), res };
    let mut deferred: Vec<(u64, u32, DOp)> = Vec::new();
    let mut first_error: Option<(u64, String)> = None;
    loop {
        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(ch) = chunks.get(k) else { break };
        let arch = &world.archetypes()[ch.arch];
        for (slot, &t) in env.bases.iter_mut().zip(&c.terms) {
            *slot = arch.column_base(t).expect("matching archetype has the component");
        }
        for row in ch.start..ch.end {
            let e = arch.entities()[row];
            env.row = row;
            env.uid = world.uid_of(e).unwrap_or(0);
            let mut seq = 0u32; // numbered per entity, so merging chunks in any order gives the same final order
            if let Err(msg) = exec(&c.body, &mut env, &mut deferred, &mut seq) {
                // keep the error of the entity with the smallest uid so the message is the same on every run
                if first_error.as_ref().is_none_or(|(u, _)| env.uid < *u) {
                    first_error = Some((env.uid, format!("tick {tick}, entity {}: {msg}", env.uid)));
                }
            }
        }
    }
    (deferred, first_error)
}

/// Run an `on_contact` system: once for every ordered pair (a, b) of touching entities where `a` has the role-a components and `b`
/// the role-b components, in uid order. Pairs are handled one after another (their writes may touch either entity), so the
/// result never depends on threads; structural changes are queued like those of any graph system.
fn run_contact(world: &World, c: &Compiled, spec: &ContactSpec, contacts: &[Contact], tick: u64, dt: f32, seed: u64) -> (Vec<(u64, u32, DOp)>, Option<String>) {
    let (a_terms, b_terms) = c.terms.split_at(spec.na);
    let has_all = |e: lunaris_ecs::Entity, need: &[ComponentId]| {
        let have = world.components_of(e);
        need.iter().all(|t| have.contains(t))
    };
    let mut pairs: Vec<(u64, u64, [f64; 5])> = Vec::new();
    for ct in contacts {
        match (spec.triggers, ct.trigger) {
            (Triggers::Only, false) | (Triggers::Solid, true) => continue,
            _ => {}
        }
        for (ua, ub, sign) in [(ct.a, ct.b, 1.0f64), (ct.b, ct.a, -1.0f64)] {
            let (Some(ea), Some(eb)) = (world.entity(ua), world.entity(ub)) else { continue };
            if has_all(ea, a_terms) && has_all(eb, b_terms) {
                pairs.push((ua, ub, [ct.depth as f64, ct.normal[0] as f64 * sign, ct.normal[1] as f64 * sign, ct.normal[2] as f64 * sign, if ct.trigger { 1.0 } else { 0.0 }]));
            }
        }
    }
    pairs.sort_by_key(|p| (p.0, p.1));
    let res: Vec<f64> = c.res.iter().map(|n| world.resource(n).unwrap_or(0.0)).collect();
    let sizes: Vec<usize> = c.terms.iter().map(|&t| world.info(t).size).collect();
    let mut env = Env { bases: vec![std::ptr::null_mut(); c.terms.len()], sizes, row: 0, locals: vec![Val::Num(0.0); c.nlocals], uid: 0, uid_b: 0, contact: [0.0; 5], dt: dt as f64, tick, seed, strings: world.strings(), res: &res };
    let mut deferred: Vec<(u64, u32, DOp)> = Vec::new();
    let mut seq = 0u32; // one counter for the whole run: with the uid of `a` it keeps the pairs' effects in processing order
    let mut first_error: Option<String> = None;
    for (ua, ub, info) in pairs {
        let (Some(ea), Some(eb)) = (world.entity(ua), world.entity(ub)) else { continue };
        for (t, &cid) in c.terms.iter().enumerate() {
            let e = if t < spec.na { ea } else { eb };
            let (arch_index, row) = world.locate(e).expect("both entities were just found alive");
            let base = world.archetypes()[arch_index].column_base(cid).expect("the entity has this component (checked above)");
            // SAFETY: the row is in range; the pointer is used only while this pair is handled, and nothing else touches the world meanwhile.
            env.bases[t] = unsafe { base.add(row * env.sizes[t]) };
        }
        env.row = 0;
        env.uid = ua;
        env.uid_b = ub;
        env.contact = info;
        if let Err(msg) = exec(&c.body, &mut env, &mut deferred, &mut seq) {
            if first_error.is_none() {
                first_error = Some(format!("tick {tick}, contact {ua}-{ub}: {msg}"));
            }
        }
    }
    (deferred, first_error)
}

#[allow(clippy::too_many_arguments)]
fn run_one(world: &mut World, c: &Compiled, name: &str, tick: u64, dt: f32, seed: u64, threads: usize, interpret_only: bool, contacts: &[Contact], prefabs: &crate::prefab::Prefabs, events: &mut Vec<(String, u64, Option<[f32; 3]>)>) -> Option<String> {
    let mut first_error: Option<String> = None;
    let mut deferred: Vec<(u64, u32, DOp)> = Vec::new();
    if let Some(spec) = &c.contact {
        let (d, e) = run_contact(world, c, spec, contacts, tick, dt, seed);
        deferred = d;
        first_error = e;
    } else {
        let w: &World = world;
        let total: usize = w.matching_archetypes(&c.terms, &c.without).iter().map(|&i| w.archetypes()[i].len()).sum();
        let parallel = threads > 1 && total >= 2048 && c.plugin.as_ref().is_none_or(|p| p.parallel);
        let chunks = w.chunks(&c.terms, &c.without, if parallel { (total / (threads * 4)).clamp(512, 32_768) } else { usize::MAX / 2 });
        let next = std::sync::atomic::AtomicUsize::new(0);
        let res: Vec<f64> = c.res.iter().map(|n| w.resource(n).unwrap_or(0.0)).collect();
        let res = &res[..];
        let mut results: Vec<(Vec<(u64, u32, DOp)>, Option<(u64, String)>)> = Vec::new();
        if parallel {
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..threads.min(chunks.len().max(1))).map(|_| scope.spawn(|| run_chunks(w, c, &chunks, &next, res, tick, dt, seed, interpret_only))).collect();
                for h in handles {
                    results.push(h.join().expect("graph worker panicked"));
                }
            });
        } else {
            results.push(run_chunks(w, c, &chunks, &next, res, tick, dt, seed, interpret_only));
        }
        let mut best: Option<(u64, String)> = None;
        for (mut d, e) in results {
            deferred.append(&mut d);
            if let Some((u, m)) = e {
                if best.as_ref().is_none_or(|(bu, _)| u < *bu) {
                    best = Some((u, m));
                }
            }
        }
        first_error = best.map(|b| b.1).or(first_error);
    }
    // structural changes: ordered by the issuing entity's uid, so the result does not depend on storage order
    deferred.sort_by_key(|d| (d.0, d.1));
    for (uid, _, op) in deferred {
        let cmd = match op {
            DOp::Emit(event) => {
                // where the emitter was when it spoke (it may be despawned later in this very list: a coin that is picked up)
                let pos = world.entity(uid).and_then(|e| world.get::<crate::components::Transform>(e)).map(|t| t.pos);
                events.push((event, uid, pos));
                continue;
            }
            DOp::Spawn(components) => Command::Spawn { uid: None, components },
            DOp::SpawnPrefab(prefab, overrides) => {
                // a prefab makes several entities: all or nothing, with consecutive uids
                let made = crate::prefab::plan(prefabs, &prefab, &overrides).map(|p| crate::prefab::spawn_commands(&p, world.next_uid()));
                match made {
                    Ok(cmds) => {
                        for cmd in cmds {
                            if let Err(e) = apply_command(world, &mut ScriptSet::default(), &cmd) {
                                if first_error.is_none() {
                                    first_error = Some(format!("tick {tick}, system {name}: spawning prefab `{prefab}`: {}", e.message));
                                }
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(format!("tick {tick}, system {name}: prefab `{prefab}`: {}", e.message));
                        }
                    }
                }
                continue;
            }
            DOp::Despawn => Command::Despawn { uid },
            DOp::Add(component, value) => Command::Add { uid, component, value },
            DOp::Remove(component) => Command::Remove { uid, component },
            // contact systems can queue the same change several times (an orb touched by two players is despawned twice):
            // a change whose target is already gone is simply skipped
            DOp::DespawnUid(target) => {
                if world.entity(target).is_none() {
                    continue;
                }
                Command::Despawn { uid: target }
            }
            DOp::AddUid(target, component, value) => {
                if world.entity(target).is_none() {
                    continue;
                }
                Command::Add { uid: target, component, value }
            }
            DOp::RemoveUid(target, component) => {
                let present = world.entity(target).zip(world.component_id(&component)).is_some_and(|(e, cid)| world.components_of(e).contains(&cid));
                if !present {
                    continue;
                }
                Command::Remove { uid: target, component }
            }
            DOp::ResSet(name, v) => Command::SetResource { name, value: Some(v) },
            DOp::ResAdd(name, by) => {
                let now = world.resource(&name).unwrap_or(0.0);
                Command::SetResource { name, value: Some(now + by) }
            }
        };
        // these commands are re-derived on replay, so they bypass the recording; the script set is not touched by them
        if let Err(e) = apply_command(world, &mut ScriptSet::default(), &cmd) {
            if first_error.is_none() {
                first_error = Some(format!("tick {tick}, system {name}: {}", e.message));
            }
        }
    }
    first_error
}

/// A summary of a system for listings.
pub fn describe(sys: &ScriptSystem) -> Json {
    let mut o = Json::obj([
        ("name", Json::str(sys.name.clone())),
        ("kind", Json::str(sys.compiled.plugin.as_ref().map(|p| p.kind()).unwrap_or("graph"))),
        ("enabled", Json::Bool(sys.enabled)),
        ("order", Json::from(sys.order)),
        ("each", Json::arr(sys.each().into_iter().map(Json::str))),
        ("nodes", Json::from(sys.nodes())),
        ("batch", Json::Bool(sys.uses_batch())),
        ("resources", Json::arr(sys.compiled.res.iter().map(|r| Json::str(r.clone())))),
        ("last_ms", Json::from((sys.last_ms * 1000.0).round() / 1000.0)),
    ]);
    if let Some(oc) = sys.def.get("on_contact") {
        o.set("on_contact", oc.clone());
    }
    if !sys.doc.is_empty() {
        o.set("doc", Json::str(sys.doc.clone()));
    }
    if let Some(e) = &sys.last_error {
        o.set("last_error", Json::str(e.clone()));
    }
    o
}
