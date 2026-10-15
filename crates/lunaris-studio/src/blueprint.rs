//! The model of the Blueprint editor: a graph system (`docs/GRAPH.md`) as nodes with pins and wires placed freely on a canvas, the way
//! Unreal's Blueprints are. White *execution* wires say what happens after what; coloured *data* wires carry values from one node into
//! the pin of another; a pin that nothing is wired to holds a value of its own (a number, true/false, text).
//!
//! The graph and the JSON are two views of one system: [`Bp::from_json`] reads the JSON (and places the nodes), [`Bp::to_json`] writes it.
//! `to_json(from_json(def))` is `def` again for every system the engine accepts (what the node set has no words for becomes a *raw* node
//! that carries its JSON as it is). Positions and comment boxes are not part of the JSON: they are kept in a sidecar file by the window.
use lunaris_json::Json;
use lunaris_script::{BINARY, NARY, SINGLE, TERNARY};

/// What kind of value a pin carries (only used for colours and for refusing nonsense wires; the engine does the real type check).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Num,
    Bool,
    Str,
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cat {
    Event,
    Flow,
    Variable,
    Resource,
    Entity,
    Math,
    Compare,
    Logic,
    Utility,
    Raw,
}

impl Cat {
    pub fn title(self) -> &'static str {
        match self {
            Cat::Event => "Events",
            Cat::Flow => "Flow Control",
            Cat::Variable => "Variables",
            Cat::Resource => "Resources",
            Cat::Entity => "Entities",
            Cat::Math => "Math",
            Cat::Compare => "Comparison",
            Cat::Logic => "Logic",
            Cat::Utility => "Utilities",
            Cat::Raw => "Advanced",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arity {
    /// The inputs of the spec.
    Fixed,
    /// Any number of inputs of one kind (A, B, C ...), at least `min` in the palette.
    Nary { min: usize },
    /// Inputs that have a name each (the fields of a spawned entity).
    Named,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub op: &'static str,
    pub title: &'static str,
    pub cat: Cat,
    pub exec_in: bool,
    /// Labels of the execution outputs (empty label: just an arrow).
    pub exec_out: &'static [&'static str],
    pub inputs: &'static [(&'static str, Ty)],
    pub out: Option<Ty>,
    /// Properties shown on the node: (label, default text).
    pub params: &'static [(&'static str, &'static str)],
    pub arity: Arity,
    pub keywords: &'static str,
}

const fn stmt(op: &'static str, title: &'static str, cat: Cat, inputs: &'static [(&'static str, Ty)], params: &'static [(&'static str, &'static str)], keywords: &'static str) -> Spec {
    Spec { op, title, cat, exec_in: true, exec_out: &[""], inputs, out: None, params, arity: Arity::Fixed, keywords }
}

const fn pure(op: &'static str, title: &'static str, cat: Cat, inputs: &'static [(&'static str, Ty)], out: Ty, params: &'static [(&'static str, &'static str)], keywords: &'static str) -> Spec {
    Spec { op, title, cat, exec_in: false, exec_out: &[], inputs, out: Some(out), params, arity: Arity::Fixed, keywords }
}

const fn nary(op: &'static str, title: &'static str, cat: Cat, ty: Ty, out: Ty, keywords: &'static str) -> Spec {
    let inputs: &'static [(&'static str, Ty)] = match ty {
        Ty::Bool => &[("A", Ty::Bool), ("B", Ty::Bool)],
        _ => &[("A", Ty::Num), ("B", Ty::Num)],
    };
    Spec { op, title, cat, exec_in: false, exec_out: &[], inputs, out: Some(out), params: &[], arity: Arity::Nary { min: 2 }, keywords }
}

const N: Ty = Ty::Num;
const B: Ty = Ty::Bool;

pub const SPECS: &[Spec] = &[
    // flow and statements
    Spec { op: "if", title: "Branch", cat: Cat::Flow, exec_in: true, exec_out: &["True", "False", "Completed"], inputs: &[("Condition", B)], out: None, params: &[], arity: Arity::Fixed, keywords: "if else condition" },
    stmt("set", "Set Field", Cat::Variable, &[("Value", N)], &[("Field", "Transform.pos[0]")], "write assign component"),
    stmt("let", "Set Local Variable", Cat::Variable, &[("Value", N)], &[("Name", "x")], "let variable"),
    stmt("res_set", "Set Resource", Cat::Resource, &[("Value", N)], &[("Resource", "score")], "global score"),
    stmt("res_add", "Add to Resource", Cat::Resource, &[("By", N)], &[("Resource", "score")], "global score increment"),
    stmt("emit", "Emit Event", Cat::Utility, &[], &[("Event", "event")], "sound message"),
    stmt("despawn", "Destroy Entity", Cat::Entity, &[], &[("Entity", "")], "despawn delete remove"),
    Spec { op: "spawn", title: "Spawn Entity", cat: Cat::Entity, exec_in: true, exec_out: &[""], inputs: &[], out: None, params: &[], arity: Arity::Named, keywords: "create new entity" },
    Spec { op: "spawn_prefab", title: "Spawn Prefab", cat: Cat::Entity, exec_in: true, exec_out: &[""], inputs: &[], out: None, params: &[("Prefab", "Bullet")], arity: Arity::Named, keywords: "create instance" },
    Spec { op: "add_comp", title: "Add Component", cat: Cat::Entity, exec_in: true, exec_out: &[""], inputs: &[], out: None, params: &[("Component", "Frozen"), ("On", "")], arity: Arity::Named, keywords: "attach" },
    stmt("remove", "Remove Component", Cat::Entity, &[], &[("Component", "Frozen"), ("On", "")], "detach"),
    // values
    pure("get", "Get Field", Cat::Variable, &[], N, &[("Field", "Transform.pos[0]")], "read component"),
    pure("var", "Get Local Variable", Cat::Variable, &[], N, &[("Name", "x")], "variable"),
    pure("res", "Get Resource", Cat::Resource, &[], N, &[("Resource", "score")], "global score"),
    pure("dt", "Delta Seconds", Cat::Utility, &[], N, &[], "time step"),
    pure("tick", "Tick", Cat::Utility, &[], N, &[], "frame count"),
    pure("uid", "Entity Id", Cat::Utility, &[], N, &[("Entity", "")], "uid"),
    pure("contact", "Contact Info", Cat::Utility, &[], N, &[("Value", "depth")], "touch collision depth normal"),
    pure("rand", "Random", Cat::Utility, &[("Salt", N)], N, &[], "random noise"),
    // math
    nary("add", "Add", Cat::Math, N, N, "plus sum +"),
    nary("mul", "Multiply", Cat::Math, N, N, "times product *"),
    nary("min", "Min", Cat::Math, N, N, "smallest"),
    nary("max", "Max", Cat::Math, N, N, "largest"),
    pure("sub", "Subtract", Cat::Math, &[("A", N), ("B", N)], N, &[], "minus -"),
    pure("div", "Divide", Cat::Math, &[("A", N), ("B", N)], N, &[], "/"),
    pure("mod", "Modulo", Cat::Math, &[("A", N), ("B", N)], N, &[], "remainder %"),
    pure("pow", "Power", Cat::Math, &[("Base", N), ("Exponent", N)], N, &[], "exponent"),
    pure("atan2", "Atan2", Cat::Math, &[("Y", N), ("X", N)], N, &[], "angle direction"),
    pure("hypot", "Length", Cat::Math, &[("X", N), ("Y", N)], N, &[], "hypot distance"),
    pure("neg", "Negate", Cat::Math, &[("X", N)], N, &[], "minus"),
    pure("abs", "Absolute", Cat::Math, &[("X", N)], N, &[], "abs"),
    pure("sqrt", "Square Root", Cat::Math, &[("X", N)], N, &[], "sqrt"),
    pure("floor", "Floor", Cat::Math, &[("X", N)], N, &[], "round down"),
    pure("ceil", "Ceiling", Cat::Math, &[("X", N)], N, &[], "round up"),
    pure("round", "Round", Cat::Math, &[("X", N)], N, &[], ""),
    pure("sign", "Sign", Cat::Math, &[("X", N)], N, &[], ""),
    pure("trunc", "Truncate", Cat::Math, &[("X", N)], N, &[], ""),
    pure("fract", "Fraction", Cat::Math, &[("X", N)], N, &[], ""),
    pure("sin", "Sin", Cat::Math, &[("X", N)], N, &[], "sine"),
    pure("cos", "Cos", Cat::Math, &[("X", N)], N, &[], "cosine"),
    pure("tan", "Tan", Cat::Math, &[("X", N)], N, &[], "tangent"),
    pure("atan", "Atan", Cat::Math, &[("X", N)], N, &[], "arctangent"),
    pure("exp", "Exp", Cat::Math, &[("X", N)], N, &[], "e to the"),
    pure("ln", "Natural Log", Cat::Math, &[("X", N)], N, &[], "log"),
    pure("clamp", "Clamp", Cat::Math, &[("X", N), ("Min", N), ("Max", N)], N, &[], "limit range"),
    pure("lerp", "Lerp", Cat::Math, &[("A", N), ("B", N), ("Alpha", N)], N, &[], "mix interpolate"),
    pure("smoothstep", "Smooth Step", Cat::Math, &[("Edge0", N), ("Edge1", N), ("X", N)], N, &[], "ease"),
    pure("select", "Select", Cat::Math, &[("Condition", B), ("A", N), ("B", N)], N, &[], "choose if"),
    // comparison and logic
    pure("lt", "Less Than", Cat::Compare, &[("A", N), ("B", N)], B, &[], "<"),
    pure("le", "Less Than or Equal", Cat::Compare, &[("A", N), ("B", N)], B, &[], "<="),
    pure("gt", "Greater Than", Cat::Compare, &[("A", N), ("B", N)], B, &[], ">"),
    pure("ge", "Greater Than or Equal", Cat::Compare, &[("A", N), ("B", N)], B, &[], ">="),
    pure("eq", "Equal", Cat::Compare, &[("A", Ty::Any), ("B", Ty::Any)], B, &[], "=="),
    pure("ne", "Not Equal", Cat::Compare, &[("A", Ty::Any), ("B", Ty::Any)], B, &[], "!="),
    nary("and", "AND", Cat::Logic, B, B, "both &&"),
    nary("or", "OR", Cat::Logic, B, B, "either ||"),
    pure("not", "NOT", Cat::Logic, &[("X", B)], B, &[], "negate !"),
    // JSON the node set does not know
    Spec { op: "raw_stmt", title: "Raw Statement", cat: Cat::Raw, exec_in: true, exec_out: &[""], inputs: &[], out: None, params: &[("JSON", "{}")], arity: Arity::Fixed, keywords: "json" },
    pure("raw_expr", "Raw Value", Cat::Raw, &[], Ty::Any, &[("JSON", "0")], "json"),
];

pub fn spec(op: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.op == op)
}

/// Where a wire starts or ends on a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PinKind {
    ExecIn,
    ExecOut(usize),
    DataIn(usize),
    DataOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pin {
    pub node: u32,
    pub kind: PinKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wire {
    /// The output end (an `ExecOut` or `DataOut` pin).
    pub from: Pin,
    /// The input end (an `ExecIn` or `DataIn` pin).
    pub to: Pin,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: u32,
    pub op: String,
    pub pos: [f32; 2],
    /// The properties shown on the node, in the order of the spec.
    pub params: Vec<String>,
    /// Pin names of a node with named inputs.
    pub named: Vec<String>,
    /// How many inputs a node with any number of them has.
    pub n_in: usize,
    /// What each unconnected data input holds.
    pub defaults: Vec<Json>,
    /// The JSON of a raw node.
    pub raw: Option<Json>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub title: String,
    /// Index into the palette of comment colours.
    pub color: usize,
}

/// What belongs to the system as a whole (the red event node at the start).
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub name: String,
    pub doc: String,
    pub each: Vec<String>,
    pub without: Vec<String>,
    pub order: Option<i64>,
    /// `Some((a, b, triggers))` for a system of touching pairs.
    pub contact: Option<(Vec<String>, Vec<String>, String)>,
    pub enabled: bool,
    pub event_pos: [f32; 2],
}

impl Default for Meta {
    fn default() -> Meta {
        Meta { name: "new_system".to_string(), doc: String::new(), each: vec!["Transform".to_string()], without: vec![], order: None, contact: None, enabled: true, event_pos: [0.0, 0.0] }
    }
}

/// The id of the event node: it is always there and has the execution output that starts the system.
pub const EVENT: u32 = 0;

#[derive(Clone, Debug, PartialEq)]
pub struct Bp {
    pub meta: Meta,
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    pub comments: Vec<Comment>,
    next_id: u32,
}

pub const HEADER: f32 = 28.0;
pub const ROW: f32 = 22.0;
/// A row of statements is wrapped to a new row when it gets wider than this.
const MAX_ROW: f32 = 1500.0;

// ---------------------------------------------------------------------------------------------------------------- nodes

impl Node {
    pub fn new(id: u32, op: &str, pos: [f32; 2]) -> Node {
        let sp = spec(op);
        let params = sp.map(|s| s.params.iter().map(|(_, d)| d.to_string()).collect()).unwrap_or_default();
        let (n_in, defaults, named) = match sp {
            Some(s) => match s.arity {
                Arity::Named => (0, vec![], vec![]),
                _ => (s.inputs.len(), s.inputs.iter().map(|(_, t)| default_of(*t)).collect(), vec![]),
            },
            None => (0, vec![], vec![]),
        };
        let raw = match op {
            "raw_stmt" => Some(Json::obj([("emit", Json::str("event"))])),
            "raw_expr" => Some(Json::Int(0)),
            _ => None,
        };
        Node { id, op: op.to_string(), pos, params, named, n_in, defaults, raw }
    }

    pub fn spec(&self) -> &'static Spec {
        spec(&self.op).unwrap_or(&SPECS[1])
    }

    pub fn arity(&self) -> Arity {
        self.spec().arity
    }

    /// How many data inputs the node has.
    pub fn inputs(&self) -> usize {
        match self.arity() {
            Arity::Fixed => self.spec().inputs.len(),
            Arity::Nary { .. } => self.n_in,
            Arity::Named => self.named.len(),
        }
    }

    pub fn input_label(&self, i: usize) -> String {
        match self.arity() {
            Arity::Fixed => self.spec().inputs.get(i).map(|x| x.0.to_string()).unwrap_or_default(),
            Arity::Nary { .. } => ((b'A' + (i % 26) as u8) as char).to_string(),
            Arity::Named => self.named.get(i).cloned().unwrap_or_default(),
        }
    }

    pub fn input_ty(&self, i: usize) -> Ty {
        match self.arity() {
            Arity::Fixed => self.spec().inputs.get(i).map(|x| x.1).unwrap_or(Ty::Any),
            Arity::Nary { .. } => self.spec().inputs.first().map(|x| x.1).unwrap_or(Ty::Num),
            Arity::Named => Ty::Any,
        }
    }

    pub fn out_ty(&self) -> Option<Ty> {
        self.spec().out
    }

    pub fn exec_outs(&self) -> usize {
        self.spec().exec_out.len()
    }

    pub fn title(&self) -> &'static str {
        self.spec().title
    }

    pub fn add_input(&mut self, name: &str) {
        match self.arity() {
            Arity::Nary { .. } => {
                self.n_in += 1;
                self.defaults.push(default_of(self.input_ty(0)));
            }
            Arity::Named => {
                self.named.push(name.to_string());
                self.defaults.push(Json::Int(0));
            }
            Arity::Fixed => {}
        }
    }

    /// Remove input `i`; the caller moves the wires.
    pub fn remove_input(&mut self, i: usize) {
        match self.arity() {
            Arity::Nary { .. } if self.n_in > 1 && i < self.n_in => {
                self.n_in -= 1;
                if i < self.defaults.len() {
                    self.defaults.remove(i);
                }
            }
            Arity::Named if i < self.named.len() => {
                self.named.remove(i);
                if i < self.defaults.len() {
                    self.defaults.remove(i);
                }
            }
            _ => {}
        }
    }
}

pub fn default_of(t: Ty) -> Json {
    match t {
        Ty::Bool => Json::Bool(false),
        Ty::Str => Json::str(""),
        _ => Json::Int(0),
    }
}

/// The width and height of a node on the canvas (the same numbers draw it and lay it out).
pub fn node_size(n: &Node) -> [f32; 2] {
    let sp = n.spec();
    let rows_left = usize::from(sp.exec_in) + n.inputs();
    let rows_right = (sp.exec_out.len() + usize::from(sp.out.is_some())).max(usize::from(sp.exec_out.is_empty() && sp.out.is_none()));
    let pin_rows = rows_left.max(rows_right);
    let params = sp.params.len();
    let h = HEADER + 6.0 + pin_rows as f32 * ROW + if params > 0 { 4.0 + params as f32 * ROW } else { 0.0 } + 6.0;
    let cw = 7.2;
    let mut left_w: f32 = 0.0;
    if sp.exec_in {
        left_w = 24.0;
    }
    for i in 0..n.inputs() {
        let lit = 50.0;
        left_w = left_w.max(26.0 + n.input_label(i).chars().count() as f32 * cw + lit);
    }
    let mut right_w: f32 = 0.0;
    for l in sp.exec_out {
        right_w = right_w.max(26.0 + l.chars().count() as f32 * cw);
    }
    if sp.out.is_some() {
        right_w = right_w.max(26.0);
    }
    let mut params_w: f32 = 0.0;
    for (i, (label, _)) in sp.params.iter().enumerate() {
        let v = n.params.get(i).map(|s| s.chars().count()).unwrap_or(0).clamp(4, 28);
        params_w = params_w.max(18.0 + (label.chars().count() + v) as f32 * cw + 14.0);
    }
    let title_w = 30.0 + sp.title.chars().count() as f32 * 7.6;
    let w = title_w.max(left_w + right_w + 16.0).max(params_w).clamp(120.0, 420.0);
    [w.ceil(), h]
}

/// Where a pin is, relative to the node's top left corner.
pub fn pin_offset(n: &Node, kind: PinKind) -> [f32; 2] {
    let sp = n.spec();
    let [w, _] = node_size(n);
    let row_y = |r: usize| HEADER + 6.0 + r as f32 * ROW + ROW / 2.0;
    match kind {
        PinKind::ExecIn => [10.0, row_y(0)],
        PinKind::ExecOut(k) => [w - 10.0, row_y(k)],
        PinKind::DataIn(i) => [10.0, row_y(usize::from(sp.exec_in) + i)],
        PinKind::DataOut => [w - 10.0, row_y(sp.exec_out.len())],
    }
}

pub fn event_size(meta: &Meta) -> [f32; 2] {
    let rows = 2 + usize::from(!meta.without.is_empty()) + usize::from(meta.order.is_some());
    let title = if meta.contact.is_some() { "On Contact".len() } else { "Every Tick".len() };
    let lines = event_lines(meta);
    let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(title + 6);
    [(widest as f32 * 7.2 + 44.0).clamp(170.0, 420.0), HEADER + 8.0 + rows as f32 * ROW]
}

/// The properties shown on the event node.
pub fn event_lines(meta: &Meta) -> Vec<String> {
    let mut v = vec![meta.name.clone()];
    match &meta.contact {
        Some((a, b, t)) => {
            v.push(format!("a: {}", a.join(", ")));
            v.push(format!("b: {}{}", b.join(", "), if t == "all" || t.is_empty() { String::new() } else { format!("  ({t})") }));
        }
        None => v.push(format!("for each: {}", if meta.each.is_empty() { "(every entity)".to_string() } else { meta.each.join(", ") })),
    }
    if !meta.without.is_empty() {
        v.push(format!("without: {}", meta.without.join(", ")));
    }
    if let Some(o) = meta.order {
        v.push(format!("order: {o}"));
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------- the graph

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub node: Option<u32>,
    pub message: String,
}

impl Bp {
    pub fn new(name: &str) -> Bp {
        Bp { meta: Meta { name: name.to_string(), ..Meta::default() }, nodes: vec![], wires: vec![], comments: vec![], next_id: 1 }
    }

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn add_node(&mut self, op: &str, pos: [f32; 2]) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(Node::new(id, op, pos));
        id
    }

    fn push_node(&mut self, mut n: Node) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        n.id = id;
        self.nodes.push(n);
        id
    }

    /// The wire that ends at this input pin, if any.
    pub fn wire_into(&self, to: Pin) -> Option<&Wire> {
        self.wires.iter().find(|w| w.to == to)
    }

    /// The wires that start at this output pin.
    pub fn wires_from(&self, from: Pin) -> impl Iterator<Item = &Wire> {
        self.wires.iter().filter(move |w| w.from == from)
    }

    pub fn is_connected(&self, pin: Pin) -> bool {
        self.wires.iter().any(|w| w.from == pin || w.to == pin)
    }

    /// Does the node have this pin?
    pub fn pin_exists(&self, p: Pin) -> bool {
        if p.node == EVENT {
            return matches!(p.kind, PinKind::ExecOut(0));
        }
        let Some(n) = self.node(p.node) else { return false };
        match p.kind {
            PinKind::ExecIn => n.spec().exec_in,
            PinKind::ExecOut(k) => k < n.exec_outs(),
            PinKind::DataIn(i) => i < n.inputs(),
            PinKind::DataOut => n.out_ty().is_some(),
        }
    }

    /// Can a wire go from `a` to `b` (in either order of the arguments)? Returns the wire as (output, input).
    pub fn can_connect(&self, a: Pin, b: Pin) -> Option<Wire> {
        if !self.pin_exists(a) || !self.pin_exists(b) {
            return None;
        }
        let (from, to) = match (a.kind, b.kind) {
            (PinKind::ExecOut(_), PinKind::ExecIn) => (a, b),
            (PinKind::ExecIn, PinKind::ExecOut(_)) => (b, a),
            (PinKind::DataOut, PinKind::DataIn(_)) => (a, b),
            (PinKind::DataIn(_), PinKind::DataOut) => (b, a),
            _ => return None,
        };
        if from.node == to.node {
            return None;
        }
        // no wire back to something that already runs before or feeds this node: the graph stays a tree of values and a sequence of steps
        if self.reaches(to.node, from.node, matches!(from.kind, PinKind::DataOut)) {
            return None;
        }
        // an exec input can only come from one place: its statement list has one owner
        Some(Wire { from, to })
    }

    /// Is `target` reachable from `start` by following wires forward (execution wires, or data wires going into inputs)?
    fn reaches(&self, start: u32, target: u32, data: bool) -> bool {
        let mut seen = vec![start];
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            if n == target {
                return true;
            }
            for w in &self.wires {
                let is_data = matches!(w.from.kind, PinKind::DataOut);
                if is_data == data && w.from.node == n && !seen.contains(&w.to.node) {
                    seen.push(w.to.node);
                    stack.push(w.to.node);
                }
            }
        }
        false
    }

    /// Wire two pins. An input takes one wire (the old one goes), an execution output leads to one place. Returns whether it was allowed.
    pub fn connect(&mut self, a: Pin, b: Pin) -> bool {
        let Some(w) = self.can_connect(a, b) else { return false };
        self.wires.retain(|x| x.to != w.to);
        if matches!(w.from.kind, PinKind::ExecOut(_)) {
            self.wires.retain(|x| x.from != w.from);
        }
        self.wires.push(w);
        true
    }

    pub fn disconnect(&mut self, pin: Pin) {
        self.wires.retain(|w| w.from != pin && w.to != pin);
    }

    pub fn remove_node(&mut self, id: u32) {
        if id == EVENT {
            return;
        }
        self.nodes.retain(|n| n.id != id);
        self.wires.retain(|w| w.from.node != id && w.to.node != id);
    }

    /// Remove input pin `i` of a node and shift the wires of the pins after it.
    pub fn remove_input(&mut self, id: u32, i: usize) {
        let Some(n) = self.node_mut(id) else { return };
        let before = n.inputs();
        n.remove_input(i);
        if n.inputs() == before {
            return;
        }
        self.wires.retain(|w| !(w.to.node == id && w.to.kind == PinKind::DataIn(i)));
        for w in self.wires.iter_mut() {
            if w.to.node == id {
                if let PinKind::DataIn(k) = w.to.kind {
                    if k > i {
                        w.to.kind = PinKind::DataIn(k - 1);
                    }
                }
            }
        }
    }

    /// Copy the nodes with these ids (and the wires between them); returns the new ids.
    pub fn duplicate(&mut self, ids: &[u32], offset: [f32; 2]) -> Vec<u32> {
        let mut map: Vec<(u32, u32)> = Vec::new();
        for &id in ids {
            if id == EVENT {
                continue;
            }
            if let Some(n) = self.node(id).cloned() {
                let mut copy = n;
                copy.pos = [copy.pos[0] + offset[0], copy.pos[1] + offset[1]];
                let new = self.push_node(copy);
                map.push((id, new));
            }
        }
        let find = |id: u32| map.iter().find(|(o, _)| *o == id).map(|(_, n)| *n);
        let mut added = Vec::new();
        for w in self.wires.clone() {
            if let (Some(f), Some(t)) = (find(w.from.node), find(w.to.node)) {
                added.push(Wire { from: Pin { node: f, kind: w.from.kind }, to: Pin { node: t, kind: w.to.kind } });
            }
        }
        self.wires.extend(added);
        map.into_iter().map(|(_, n)| n).collect()
    }

    // ------------------------------------------------------------------------------------------------ JSON to graph

    pub fn from_json(def: &Json) -> Result<Bp, String> {
        if def.as_object().is_none() {
            return Err("a system definition is a JSON object".to_string());
        }
        for k in ["plugin", "wasm"] {
            if def.get(k).is_some() {
                return Err(format!("this is a {k} system (compiled code): it has no nodes"));
            }
        }
        let names = |v: Option<&Json>| -> Vec<String> { v.and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default() };
        let mut bp = Bp::new(def.get("name").and_then(|n| n.as_str()).unwrap_or("system"));
        bp.meta.doc = def.get("doc").and_then(|d| d.as_str()).unwrap_or("").to_string();
        bp.meta.each = names(def.get("each"));
        bp.meta.without = names(def.get("without"));
        bp.meta.order = def.get("order").and_then(|o| o.as_i64());
        bp.meta.enabled = def.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
        if let Some(oc) = def.get("on_contact") {
            bp.meta.contact = Some((names(oc.get("a")), names(oc.get("b")), oc.get("triggers").and_then(|t| t.as_str()).unwrap_or("all").to_string()));
        }
        let stmts = def.get("do").and_then(|d| d.as_array()).ok_or("the system has no `do` list")?;
        let first = bp.read_block(stmts);
        if let Some(f) = first {
            bp.wires.push(Wire { from: Pin { node: EVENT, kind: PinKind::ExecOut(0) }, to: Pin { node: f, kind: PinKind::ExecIn } });
        }
        bp.auto_layout();
        Ok(bp)
    }

    /// Read a list of statements as a chain of nodes; returns the first.
    fn read_block(&mut self, stmts: &[Json]) -> Option<u32> {
        let mut first = None;
        let mut prev: Option<Pin> = None;
        for s in stmts {
            let (id, next_pin) = self.read_stmt(s);
            if first.is_none() {
                first = Some(id);
            }
            if let Some(p) = prev {
                self.wires.push(Wire { from: p, to: Pin { node: id, kind: PinKind::ExecIn } });
            }
            prev = Some(Pin { node: id, kind: next_pin });
        }
        first
    }

    /// An expression as the source of a pin: a node whose output feeds it, or a value the pin holds.
    fn read_expr(&mut self, e: &Json, into: Pin) -> Json {
        match e {
            Json::Int(_) | Json::Float(_) | Json::Bool(_) | Json::Str(_) | Json::Null => e.clone(),
            Json::Obj(o) if o.len() == 1 => {
                let id = self.read_expr_node(&o[0].0, &o[0].1, e);
                self.wires.push(Wire { from: Pin { node: id, kind: PinKind::DataOut }, to: into });
                Json::Int(0)
            }
            other => {
                let mut n = Node::new(0, "raw_expr", [0.0, 0.0]);
                n.raw = Some(other.clone());
                n.params = vec![other.to_string_compact().unwrap_or_default()];
                let id = self.push_node(n);
                self.wires.push(Wire { from: Pin { node: id, kind: PinKind::DataOut }, to: into });
                Json::Int(0)
            }
        }
    }

    fn read_expr_node(&mut self, op: &str, arg: &Json, whole: &Json) -> u32 {
        let known = spec(op).filter(|s| s.out.is_some() && !s.op.starts_with("raw"));
        let Some(sp) = known else { return self.raw_expr(whole) };
        let mut n = Node::new(0, op, [0.0, 0.0]);
        let text = |v: &Json| v.as_str().map(str::to_string);
        match op {
            "get" | "var" | "res" => match text(arg) {
                Some(t) => n.params = vec![t],
                None => return self.raw_expr(whole),
            },
            "dt" | "tick" => {
                if *arg != Json::Bool(true) {
                    return self.raw_expr(whole);
                }
            }
            "uid" => match arg {
                Json::Bool(true) => n.params = vec![String::new()],
                Json::Str(r) if r == "a" || r == "b" => n.params = vec![r.clone()],
                _ => return self.raw_expr(whole),
            },
            "contact" => match text(arg) {
                Some(t) => n.params = vec![t],
                None => return self.raw_expr(whole),
            },
            _ => {
                let args: Vec<Json> = match arg {
                    Json::Arr(a) => a.clone(),
                    one => vec![one.clone()],
                };
                let single = SINGLE.contains(&op);
                match sp.arity {
                    Arity::Nary { .. } => {
                        if args.is_empty() || !NARY.contains(&op) || !matches!(arg, Json::Arr(_)) {
                            return self.raw_expr(whole);
                        }
                        n.n_in = args.len();
                        n.defaults = args.iter().map(|_| Json::Int(0)).collect();
                    }
                    _ => {
                        let want = sp.inputs.len();
                        let shape_ok = if single { !matches!(arg, Json::Arr(a) if a.len() != 1) } else { matches!(arg, Json::Arr(_)) && args.len() == want };
                        let known_op = single || BINARY.contains(&op) || TERNARY.contains(&op);
                        if !known_op || !shape_ok || args.len() != want {
                            return self.raw_expr(whole);
                        }
                        n.defaults = args.iter().map(|_| Json::Int(0)).collect();
                    }
                }
                let id = self.push_node(n);
                for (i, a) in args.iter().enumerate() {
                    let d = self.read_expr(a, Pin { node: id, kind: PinKind::DataIn(i) });
                    if let Some(node) = self.node_mut(id) {
                        node.defaults[i] = d;
                    }
                }
                return id;
            }
        }
        self.push_node(n)
    }

    fn raw_expr(&mut self, whole: &Json) -> u32 {
        let mut n = Node::new(0, "raw_expr", [0.0, 0.0]);
        n.raw = Some(whole.clone());
        n.params = vec![whole.to_string_compact().unwrap_or_default()];
        self.push_node(n)
    }

    fn raw_stmt(&mut self, whole: &Json) -> (u32, PinKind) {
        let mut n = Node::new(0, "raw_stmt", [0.0, 0.0]);
        n.raw = Some(whole.clone());
        n.params = vec![whole.to_string_compact().unwrap_or_default()];
        (self.push_node(n), PinKind::ExecOut(0))
    }

    /// Read one statement; returns its node and the execution output the statement after it hangs on.
    fn read_stmt(&mut self, s: &Json) -> (u32, PinKind) {
        let Some(o) = s.as_object() else { return self.raw_stmt(s) };
        let first = o.first().map(|(k, _)| k.as_str()).unwrap_or("");
        let get = |k: &str| s.get(k);
        let keys_ok = |allowed: &[&str]| o.iter().all(|(k, _)| allowed.contains(&k.as_str()));
        let text = |k: &str| s.get(k).and_then(|v| v.as_str()).map(str::to_string);
        match first {
            "set" | "let" | "res_set" | "res_add" => {
                let (vk, allowed): (&str, &[&str]) = match first {
                    "set" | "res_set" => ("to", if first == "set" { &["set", "to"] } else { &["res_set", "to"] }),
                    "let" => ("be", &["let", "be"]),
                    _ => ("by", &["res_add", "by"]),
                };
                let (Some(name), Some(val)) = (text(first), get(vk)) else { return self.raw_stmt(s) };
                if !keys_ok(allowed) {
                    return self.raw_stmt(s);
                }
                let mut n = Node::new(0, first, [0.0, 0.0]);
                n.params = vec![name];
                let id = self.push_node(n);
                let d = self.read_expr(val, Pin { node: id, kind: PinKind::DataIn(0) });
                if let Some(node) = self.node_mut(id) {
                    node.defaults[0] = d;
                }
                (id, PinKind::ExecOut(0))
            }
            "emit" if keys_ok(&["emit"]) => match text("emit") {
                Some(name) => {
                    let mut n = Node::new(0, "emit", [0.0, 0.0]);
                    n.params = vec![name];
                    (self.push_node(n), PinKind::ExecOut(0))
                }
                None => self.raw_stmt(s),
            },
            "despawn" if keys_ok(&["despawn"]) => {
                let role = match get("despawn") {
                    Some(Json::Bool(true)) => String::new(),
                    Some(Json::Str(r)) if r == "a" || r == "b" => r.clone(),
                    _ => return self.raw_stmt(s),
                };
                let mut n = Node::new(0, "despawn", [0.0, 0.0]);
                n.params = vec![role];
                (self.push_node(n), PinKind::ExecOut(0))
            }
            "remove" if keys_ok(&["remove", "on"]) => {
                let (Some(c), on) = (text("remove"), get("on")) else { return self.raw_stmt(s) };
                let on = match on {
                    None => String::new(),
                    Some(Json::Str(r)) if r == "a" || r == "b" => r.clone(),
                    _ => return self.raw_stmt(s),
                };
                let mut n = Node::new(0, "remove", [0.0, 0.0]);
                n.params = vec![c, on];
                (self.push_node(n), PinKind::ExecOut(0))
            }
            "add" if keys_ok(&["add", "values", "on"]) => {
                let Some(c) = text("add") else { return self.raw_stmt(s) };
                let on = match get("on") {
                    None => String::new(),
                    Some(Json::Str(r)) if r == "a" || r == "b" => r.clone(),
                    _ => return self.raw_stmt(s),
                };
                let vals = match get("values") {
                    None => vec![],
                    Some(Json::Obj(v)) => v.clone(),
                    _ => return self.raw_stmt(s),
                };
                let mut n = Node::new(0, "add_comp", [0.0, 0.0]);
                n.params = vec![c, on];
                n.named = vals.iter().map(|(k, _)| k.clone()).collect();
                n.defaults = vals.iter().map(|_| Json::Int(0)).collect();
                let id = self.push_node(n);
                for (i, (_, e)) in vals.iter().enumerate() {
                    let d = self.read_expr(e, Pin { node: id, kind: PinKind::DataIn(i) });
                    if let Some(node) = self.node_mut(id) {
                        node.defaults[i] = d;
                    }
                }
                (id, PinKind::ExecOut(0))
            }
            "spawn" if keys_ok(&["spawn"]) => {
                let Some(Json::Obj(comps)) = get("spawn") else { return self.raw_stmt(s) };
                let mut named: Vec<(String, Json)> = Vec::new();
                for (c, f) in comps {
                    let Some(fields) = f.as_object() else { return self.raw_stmt(s) };
                    if fields.is_empty() {
                        named.push((c.clone(), Json::Null));
                    }
                    for (k, e) in fields {
                        named.push((format!("{c}.{k}"), e.clone()));
                    }
                }
                let mut n = Node::new(0, "spawn", [0.0, 0.0]);
                n.named = named.iter().map(|(k, _)| k.clone()).collect();
                n.defaults = named.iter().map(|_| Json::Int(0)).collect();
                let id = self.push_node(n);
                for (i, (_, e)) in named.iter().enumerate() {
                    if *e == Json::Null && !self.node(id).map(|n| n.named[i].contains('.')).unwrap_or(false) {
                        continue;
                    }
                    let d = self.read_expr(e, Pin { node: id, kind: PinKind::DataIn(i) });
                    if let Some(node) = self.node_mut(id) {
                        node.defaults[i] = d;
                    }
                }
                (id, PinKind::ExecOut(0))
            }
            "spawn_prefab" if keys_ok(&["spawn_prefab", "at", "velocity", "with"]) => {
                let Some(name) = text("spawn_prefab") else { return self.raw_stmt(s) };
                let mut named: Vec<(String, Json)> = Vec::new();
                for k in ["at", "velocity"] {
                    if let Some(v) = get(k) {
                        let Some(a) = v.as_array() else { return self.raw_stmt(s) };
                        if a.len() > 3 {
                            return self.raw_stmt(s);
                        }
                        for (i, e) in a.iter().enumerate() {
                            named.push((format!("{k}.{}", ["x", "y", "z"][i]), e.clone()));
                        }
                    }
                }
                if let Some(w) = get("with") {
                    let Some(comps) = w.as_object() else { return self.raw_stmt(s) };
                    for (c, f) in comps {
                        let Some(fields) = f.as_object() else { return self.raw_stmt(s) };
                        for (k, e) in fields {
                            named.push((format!("with.{c}.{k}"), e.clone()));
                        }
                    }
                }
                let mut n = Node::new(0, "spawn_prefab", [0.0, 0.0]);
                n.params = vec![name];
                n.named = named.iter().map(|(k, _)| k.clone()).collect();
                n.defaults = named.iter().map(|_| Json::Int(0)).collect();
                let id = self.push_node(n);
                for (i, (_, e)) in named.iter().enumerate() {
                    let d = self.read_expr(e, Pin { node: id, kind: PinKind::DataIn(i) });
                    if let Some(node) = self.node_mut(id) {
                        node.defaults[i] = d;
                    }
                }
                (id, PinKind::ExecOut(0))
            }
            "if" if keys_ok(&["if", "then", "else"]) => {
                let (Some(cond), Some(Json::Arr(then))) = (get("if"), get("then")) else { return self.raw_stmt(s) };
                let els = match get("else") {
                    None => vec![],
                    Some(Json::Arr(e)) => e.clone(),
                    _ => return self.raw_stmt(s),
                };
                let id = self.push_node(Node::new(0, "if", [0.0, 0.0]));
                let d = self.read_expr(cond, Pin { node: id, kind: PinKind::DataIn(0) });
                if let Some(node) = self.node_mut(id) {
                    node.defaults[0] = d;
                }
                if let Some(f) = self.read_block(then) {
                    self.wires.push(Wire { from: Pin { node: id, kind: PinKind::ExecOut(0) }, to: Pin { node: f, kind: PinKind::ExecIn } });
                }
                if let Some(f) = self.read_block(&els) {
                    self.wires.push(Wire { from: Pin { node: id, kind: PinKind::ExecOut(1) }, to: Pin { node: f, kind: PinKind::ExecIn } });
                }
                (id, PinKind::ExecOut(2))
            }
            _ => self.raw_stmt(s),
        }
    }

    // ------------------------------------------------------------------------------------------------ graph to JSON

    /// The system as JSON, or what is wrong with the graph.
    pub fn to_json(&self) -> Result<Json, Vec<Problem>> {
        let mut problems: Vec<Problem> = Vec::new();
        let mut pairs: Vec<(String, Json)> = vec![("name".to_string(), Json::str(self.meta.name.clone()))];
        if !self.meta.doc.is_empty() {
            pairs.push(("doc".to_string(), Json::str(self.meta.doc.clone())));
        }
        let arr = |v: &[String]| Json::Arr(v.iter().map(|s| Json::str(s.clone())).collect());
        match &self.meta.contact {
            Some((a, b, t)) => {
                let mut oc = vec![("a".to_string(), arr(a)), ("b".to_string(), arr(b))];
                if !t.is_empty() && t != "all" {
                    oc.push(("triggers".to_string(), Json::str(t.clone())));
                }
                pairs.push(("on_contact".to_string(), Json::Obj(oc)));
            }
            None => pairs.push(("each".to_string(), arr(&self.meta.each))),
        }
        if !self.meta.without.is_empty() {
            pairs.push(("without".to_string(), arr(&self.meta.without)));
        }
        if let Some(o) = self.meta.order {
            pairs.push(("order".to_string(), Json::Int(o)));
        }
        if !self.meta.enabled {
            pairs.push(("enabled".to_string(), Json::Bool(false)));
        }
        let mut visited: Vec<u32> = Vec::new();
        let list = self.write_chain(Pin { node: EVENT, kind: PinKind::ExecOut(0) }, &mut visited, &mut problems);
        pairs.push(("do".to_string(), Json::Arr(list)));
        if problems.is_empty() {
            Ok(Json::Obj(pairs))
        } else {
            Err(problems)
        }
    }

    /// The nodes that are not reached from the event node: they do nothing.
    pub fn unreachable(&self) -> Vec<u32> {
        let mut seen: Vec<u32> = vec![EVENT];
        let mut stack = vec![EVENT];
        while let Some(n) = stack.pop() {
            for w in &self.wires {
                if w.from.node == n && !seen.contains(&w.to.node) {
                    seen.push(w.to.node);
                    stack.push(w.to.node);
                }
            }
        }
        self.nodes.iter().filter(|n| n.spec().exec_in && !seen.contains(&n.id)).map(|n| n.id).collect()
    }

    fn write_chain(&self, start: Pin, visited: &mut Vec<u32>, problems: &mut Vec<Problem>) -> Vec<Json> {
        let mut out = Vec::new();
        let mut at = self.wires_from(start).next().map(|w| w.to.node);
        while let Some(id) = at {
            if visited.contains(&id) {
                problems.push(Problem { node: Some(id), message: "the execution wires go in a circle here".to_string() });
                return out;
            }
            visited.push(id);
            let Some(n) = self.node(id) else { return out };
            let next_pin = if n.op == "if" { 2 } else { 0 };
            out.push(self.write_stmt(n, visited, problems));
            at = self.wires_from(Pin { node: id, kind: PinKind::ExecOut(next_pin) }).next().map(|w| w.to.node);
        }
        out
    }

    fn input(&self, n: &Node, i: usize, stack: &mut Vec<u32>, problems: &mut Vec<Problem>) -> Json {
        match self.wire_into(Pin { node: n.id, kind: PinKind::DataIn(i) }) {
            Some(w) => self.write_expr(w.from.node, stack, problems),
            None => n.defaults.get(i).cloned().unwrap_or(Json::Int(0)),
        }
    }

    fn param<'a>(&self, n: &'a Node, i: usize, required: bool, problems: &mut Vec<Problem>) -> &'a str {
        let v = n.params.get(i).map(|s| s.as_str()).unwrap_or("");
        if required && v.trim().is_empty() {
            problems.push(Problem { node: Some(n.id), message: format!("{} needs a {}", n.title(), n.spec().params.get(i).map(|p| p.0.to_lowercase()).unwrap_or_default()) });
        }
        v
    }

    fn write_stmt(&self, n: &Node, visited: &mut Vec<u32>, problems: &mut Vec<Problem>) -> Json {
        let mut stack = vec![];
        let role = |s: &str| if s.is_empty() { Json::Bool(true) } else { Json::str(s) };
        match n.op.as_str() {
            "set" => Json::obj([("set", Json::str(self.param(n, 0, true, problems))), ("to", self.input(n, 0, &mut stack, problems))]),
            "let" => Json::obj([("let", Json::str(self.param(n, 0, true, problems))), ("be", self.input(n, 0, &mut stack, problems))]),
            "res_set" => Json::obj([("res_set", Json::str(self.param(n, 0, true, problems))), ("to", self.input(n, 0, &mut stack, problems))]),
            "res_add" => Json::obj([("res_add", Json::str(self.param(n, 0, true, problems))), ("by", self.input(n, 0, &mut stack, problems))]),
            "emit" => Json::obj([("emit", Json::str(self.param(n, 0, true, problems)))]),
            "despawn" => Json::obj([("despawn", role(self.param(n, 0, false, problems)))]),
            "remove" => {
                let mut p = vec![("remove".to_string(), Json::str(self.param(n, 0, true, problems)))];
                let on = self.param(n, 1, false, problems);
                if !on.is_empty() {
                    p.push(("on".to_string(), Json::str(on)));
                }
                Json::Obj(p)
            }
            "add_comp" => {
                let mut p = vec![("add".to_string(), Json::str(self.param(n, 0, true, problems)))];
                if !n.named.is_empty() {
                    let mut vals = Vec::new();
                    for (i, name) in n.named.iter().enumerate() {
                        if name.trim().is_empty() {
                            problems.push(Problem { node: Some(n.id), message: "a field of Add Component has no name".to_string() });
                        }
                        vals.push((name.clone(), self.input(n, i, &mut stack, problems)));
                    }
                    p.push(("values".to_string(), Json::Obj(vals)));
                }
                let on = self.param(n, 1, false, problems);
                if !on.is_empty() {
                    p.push(("on".to_string(), Json::str(on)));
                }
                Json::Obj(p)
            }
            "spawn" => {
                let mut comps: Vec<(String, Vec<(String, Json)>)> = Vec::new();
                for (i, name) in n.named.iter().enumerate() {
                    let (c, f) = match name.split_once('.') {
                        Some((c, f)) => (c.to_string(), Some(f.to_string())),
                        None => (name.clone(), None),
                    };
                    if c.trim().is_empty() {
                        problems.push(Problem { node: Some(n.id), message: "a pin of Spawn Entity has no component name".to_string() });
                    }
                    let slot = match comps.iter().position(|(k, _)| *k == c) {
                        Some(p) => p,
                        None => {
                            comps.push((c.clone(), vec![]));
                            comps.len() - 1
                        }
                    };
                    if let Some(f) = f {
                        let e = self.input(n, i, &mut stack, problems);
                        comps[slot].1.push((f, e));
                    }
                }
                Json::obj([("spawn", Json::Obj(comps.into_iter().map(|(c, f)| (c, Json::Obj(f))).collect()))])
            }
            "spawn_prefab" => {
                let mut p = vec![("spawn_prefab".to_string(), Json::str(self.param(n, 0, true, problems)))];
                for key in ["at", "velocity"] {
                    let mut items: Vec<Json> = Vec::new();
                    for (i, name) in n.named.iter().enumerate() {
                        if let Some(axis) = name.strip_prefix(&format!("{key}.")) {
                            let _ = axis;
                            items.push(self.input(n, i, &mut stack, problems));
                        }
                    }
                    if !items.is_empty() {
                        p.push((key.to_string(), Json::Arr(items)));
                    }
                }
                let mut with: Vec<(String, Vec<(String, Json)>)> = Vec::new();
                for (i, name) in n.named.iter().enumerate() {
                    if let Some(rest) = name.strip_prefix("with.") {
                        let Some((c, f)) = rest.split_once('.') else {
                            problems.push(Problem { node: Some(n.id), message: format!("`{name}` should look like with.Component.field") });
                            continue;
                        };
                        let e = self.input(n, i, &mut stack, problems);
                        match with.iter().position(|(k, _)| k == c) {
                            Some(slot) => with[slot].1.push((f.to_string(), e)),
                            None => with.push((c.to_string(), vec![(f.to_string(), e)])),
                        }
                    } else if !name.starts_with("at.") && !name.starts_with("velocity.") {
                        problems.push(Problem { node: Some(n.id), message: format!("`{name}` is not at.x/y/z, velocity.x/y/z or with.Component.field") });
                    }
                }
                if !with.is_empty() {
                    p.push(("with".to_string(), Json::Obj(with.into_iter().map(|(c, f)| (c, Json::Obj(f))).collect())));
                }
                Json::Obj(p)
            }
            "if" => {
                let cond = self.input(n, 0, &mut stack, problems);
                let then = self.write_chain(Pin { node: n.id, kind: PinKind::ExecOut(0) }, visited, problems);
                let els = self.write_chain(Pin { node: n.id, kind: PinKind::ExecOut(1) }, visited, problems);
                let mut p = vec![("if".to_string(), cond), ("then".to_string(), Json::Arr(then))];
                if !els.is_empty() {
                    p.push(("else".to_string(), Json::Arr(els)));
                }
                Json::Obj(p)
            }
            "raw_stmt" => n.raw.clone().unwrap_or(Json::Null),
            _ => Json::Null,
        }
    }

    fn write_expr(&self, id: u32, stack: &mut Vec<u32>, problems: &mut Vec<Problem>) -> Json {
        let Some(n) = self.node(id) else { return Json::Int(0) };
        if stack.contains(&id) {
            problems.push(Problem { node: Some(id), message: "a value depends on itself".to_string() });
            return Json::Int(0);
        }
        stack.push(id);
        let p = |i: usize, required: bool, problems: &mut Vec<Problem>| self.param(n, i, required, problems).to_string();
        let out = match n.op.as_str() {
            "get" => Json::obj([("get", Json::str(p(0, true, problems)))]),
            "var" => Json::obj([("var", Json::str(p(0, true, problems)))]),
            "res" => Json::obj([("res", Json::str(p(0, true, problems)))]),
            "dt" => Json::obj([("dt", Json::Bool(true))]),
            "tick" => Json::obj([("tick", Json::Bool(true))]),
            "uid" => {
                let r = p(0, false, problems);
                Json::obj([("uid", if r.is_empty() { Json::Bool(true) } else { Json::str(r) })])
            }
            "contact" => Json::obj([("contact", Json::str(p(0, true, problems)))]),
            "raw_expr" => n.raw.clone().unwrap_or(Json::Null),
            op => {
                let args: Vec<Json> = (0..n.inputs()).map(|i| self.input(n, i, stack, problems)).collect();
                if SINGLE.contains(&op) {
                    Json::obj([(op.to_string(), args.into_iter().next().unwrap_or(Json::Int(0)))])
                } else {
                    Json::obj([(op.to_string(), Json::Arr(args))])
                }
            }
        };
        stack.pop();
        out
    }

    // ------------------------------------------------------------------------------------------------ layout

    /// Place every node: the execution flow goes from left to right, the nodes that feed a statement stand to its left.
    pub fn auto_layout(&mut self) {
        self.meta.event_pos = [0.0, 0.0];
        let ev = event_size(&self.meta);
        let start_x = ev[0] + 70.0;
        let first = self.wires_from(Pin { node: EVENT, kind: PinKind::ExecOut(0) }).next().map(|w| w.to.node);
        if let Some(f) = first {
            self.layout_chain(f, start_x, 0.0);
        }
        // nodes that are not wired to the event (none when read from JSON) go below
        let free: Vec<u32> = self.nodes.iter().filter(|n| n.spec().exec_in && self.wire_into(Pin { node: n.id, kind: PinKind::ExecIn }).is_none() && Some(n.id) != first).map(|n| n.id).collect();
        let mut y = 400.0;
        for id in free {
            let (_, bottom) = self.layout_chain(id, start_x, y);
            y = bottom + 40.0;
        }
    }

    /// Width of the value nodes feeding `id` (to its left), counting the gap.
    fn tree_width(&self, id: u32) -> f32 {
        let Some(n) = self.node(id) else { return 0.0 };
        let mut best: f32 = 0.0;
        for i in 0..n.inputs() {
            if let Some(w) = self.wire_into(Pin { node: id, kind: PinKind::DataIn(i) }) {
                best = best.max(self.tree_width(w.from.node) + 40.0);
            }
        }
        node_size(n)[0] + best
    }

    fn tree_height(&self, id: u32) -> f32 {
        let Some(n) = self.node(id) else { return 0.0 };
        let mut sum = 0.0;
        let mut any = false;
        for i in 0..n.inputs() {
            if let Some(w) = self.wire_into(Pin { node: id, kind: PinKind::DataIn(i) }) {
                if any {
                    sum += 10.0;
                }
                sum += self.tree_height(w.from.node);
                any = true;
            }
        }
        sum.max(node_size(n)[1])
    }

    /// Put value node `id` with its right edge at `right`, its top at `top`, and its feeders to its left. Returns the height used.
    fn layout_tree(&mut self, id: u32, right: f32, top: f32) -> f32 {
        let size = match self.node(id) {
            Some(n) => node_size(n),
            None => return 0.0,
        };
        if let Some(n) = self.node_mut(id) {
            n.pos = [right - size[0], top];
        }
        let feeders: Vec<u32> = {
            let n = self.node(id).cloned();
            n.map(|n| (0..n.inputs()).filter_map(|i| self.wire_into(Pin { node: id, kind: PinKind::DataIn(i) }).map(|w| w.from.node)).collect()).unwrap_or_default()
        };
        let mut y = top;
        for f in feeders {
            let h = self.layout_tree(f, right - size[0] - 40.0, y);
            y += h + 10.0;
        }
        (y - top - if y > top { 10.0 } else { 0.0 }).max(size[1])
    }

    /// Lay out a chain of statements starting at `id`; returns (the right end, the bottom).
    fn layout_chain(&mut self, id: u32, x: f32, y: f32) -> (f32, f32) {
        let mut at = Some(id);
        let mut y = y;
        let (mut cx, mut bottom) = (x, y);
        let mut widest: f32 = x;
        let mut guard = 0;
        while let Some(cur) = at {
            guard += 1;
            if guard > 10_000 {
                break;
            }
            let Some(n) = self.node(cur).cloned() else { break };
            let size = node_size(&n);
            // the nodes that feed this one stand to its left
            let feeders: Vec<u32> = (0..n.inputs()).filter_map(|i| self.wire_into(Pin { node: cur, kind: PinKind::DataIn(i) }).map(|w| w.from.node)).collect();
            let tree_w = feeders.iter().map(|f| self.tree_width(*f) + 40.0).fold(0.0f32, f32::max);
            let mut sx = cx + tree_w;
            // a long chain goes on in a new row below, like text that wraps
            if sx + size[0] > x + MAX_ROW && cx > x + 1.0 {
                cx = x;
                y = bottom + 56.0;
                sx = cx + tree_w;
            }
            if let Some(m) = self.node_mut(cur) {
                m.pos = [sx, y];
            }
            let mut ty = y + 8.0;
            for f in &feeders {
                let h = self.layout_tree(*f, sx - 40.0, ty);
                ty += h + 10.0;
            }
            let mut band_bottom = (y + size[1]).max(if feeders.is_empty() { 0.0 } else { ty - 10.0 });
            let mut right = sx + size[0];
            if n.op == "if" {
                // three lanes to the right: True, False, Completed
                let lane_x = sx + size[0] + 70.0;
                let mut ly = y;
                let mut max_right = lane_x;
                for k in 0..3 {
                    let first = self.wires_from(Pin { node: cur, kind: PinKind::ExecOut(k) }).next().map(|w| w.to.node);
                    if let Some(f) = first {
                        let (r, b) = self.layout_chain(f, lane_x, ly);
                        max_right = max_right.max(r);
                        ly = b + 36.0;
                    } else {
                        ly += 0.0;
                    }
                }
                band_bottom = band_bottom.max(ly - 36.0);
                right = max_right;
                bottom = bottom.max(band_bottom);
                widest = widest.max(right);
                cx = right;
                break;
            }
            bottom = bottom.max(band_bottom);
            widest = widest.max(right);
            cx = right + 60.0;
            at = self.wires_from(Pin { node: cur, kind: PinKind::ExecOut(0) }).next().map(|w| w.to.node);
        }
        (cx.max(widest), bottom)
    }

    pub fn bounds(&self) -> ([f32; 2], [f32; 2]) {
        let ev = event_size(&self.meta);
        let (mut lo, mut hi) = (self.meta.event_pos, [self.meta.event_pos[0] + ev[0], self.meta.event_pos[1] + ev[1]]);
        for n in &self.nodes {
            let s = node_size(n);
            lo = [lo[0].min(n.pos[0]), lo[1].min(n.pos[1])];
            hi = [hi[0].max(n.pos[0] + s[0]), hi[1].max(n.pos[1] + s[1])];
        }
        for c in &self.comments {
            lo = [lo[0].min(c.pos[0]), lo[1].min(c.pos[1])];
            hi = [hi[0].max(c.pos[0] + c.size[0]), hi[1].max(c.pos[1] + c.size[1])];
        }
        (lo, hi)
    }

    // ------------------------------------------------------------------------------------------------ the sidecar file

    pub fn to_layout(&self) -> Json {
        let pin_json = |p: Pin| -> Json {
            let (k, i) = match p.kind {
                PinKind::ExecIn => ("xi", 0),
                PinKind::ExecOut(i) => ("xo", i),
                PinKind::DataIn(i) => ("di", i),
                PinKind::DataOut => ("do", 0),
            };
            Json::arr([Json::from(p.node), Json::str(k), Json::from(i)])
        };
        Json::obj([
            ("version", Json::Int(1)),
            ("event", Json::arr([Json::Float(self.meta.event_pos[0] as f64), Json::Float(self.meta.event_pos[1] as f64)])),
            (
                "nodes",
                Json::arr(self.nodes.iter().map(|n| {
                    let mut o = Json::obj([("id", Json::from(n.id)), ("op", Json::str(n.op.clone())), ("pos", Json::arr([Json::Float(n.pos[0] as f64), Json::Float(n.pos[1] as f64)])), ("params", Json::arr(n.params.iter().map(|s| Json::str(s.clone())))), ("named", Json::arr(n.named.iter().map(|s| Json::str(s.clone())))), ("n_in", Json::from(n.n_in)), ("defaults", Json::Arr(n.defaults.clone()))]);
                    if let Some(r) = &n.raw {
                        o.set("raw", r.clone());
                    }
                    o
                })),
            ),
            ("wires", Json::arr(self.wires.iter().map(|w| Json::arr([pin_json(w.from), pin_json(w.to)])))),
            ("comments", Json::arr(self.comments.iter().map(|c| Json::obj([("pos", Json::arr([Json::Float(c.pos[0] as f64), Json::Float(c.pos[1] as f64)])), ("size", Json::arr([Json::Float(c.size[0] as f64), Json::Float(c.size[1] as f64)])), ("title", Json::str(c.title.clone())), ("color", Json::from(c.color))])))),
        ])
    }

    /// Read a sidecar and give back the graph it holds, with the meta of `def` (the sidecar has the layout, the JSON the facts).
    pub fn from_layout(layout: &Json, def: &Json) -> Option<Bp> {
        let mut bp = Bp::from_json(def).ok()?;
        let f = |j: &Json| j.as_f64().unwrap_or(0.0) as f32;
        let pair = |j: Option<&Json>| -> [f32; 2] { j.and_then(|a| a.as_array()).map(|a| [a.first().map(f).unwrap_or(0.0), a.get(1).map(f).unwrap_or(0.0)]).unwrap_or([0.0, 0.0]) };
        bp.nodes.clear();
        bp.wires.clear();
        bp.next_id = 1;
        bp.meta.event_pos = pair(layout.get("event"));
        for n in layout.get("nodes")?.as_array()? {
            let id = n.get("id")?.as_u64()? as u32;
            let strings = |k: &str| -> Vec<String> { n.get(k).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default() };
            spec(n.get("op")?.as_str()?)?;
            bp.nodes.push(Node { id, op: n.get("op")?.as_str()?.to_string(), pos: pair(n.get("pos")), params: strings("params"), named: strings("named"), n_in: n.get("n_in").and_then(|v| v.as_u64()).unwrap_or(0) as usize, defaults: n.get("defaults").and_then(|a| a.as_array()).map(|a| a.to_vec()).unwrap_or_default(), raw: n.get("raw").cloned() });
            bp.next_id = bp.next_id.max(id + 1);
        }
        let pin = |j: &Json| -> Option<Pin> {
            let a = j.as_array()?;
            let node = a.first()?.as_u64()? as u32;
            let i = a.get(2)?.as_u64()? as usize;
            let kind = match a.get(1)?.as_str()? {
                "xi" => PinKind::ExecIn,
                "xo" => PinKind::ExecOut(i),
                "di" => PinKind::DataIn(i),
                "do" => PinKind::DataOut,
                _ => return None,
            };
            Some(Pin { node, kind })
        };
        for w in layout.get("wires")?.as_array()? {
            let a = w.as_array()?;
            bp.wires.push(Wire { from: pin(a.first()?)?, to: pin(a.get(1)?)? });
        }
        for c in layout.get("comments").and_then(|c| c.as_array()).unwrap_or(&[]) {
            bp.comments.push(Comment { pos: pair(c.get("pos")), size: pair(c.get("size")), title: c.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(), color: c.get("color").and_then(|v| v.as_u64()).unwrap_or(0) as usize });
        }
        // the sidecar is trusted only if it says the same thing as the JSON
        let again = bp.to_json().ok()?;
        (lunaris_json_eq(&again, def)).then_some(bp)
    }
}

/// The same system, up to the order of keys and `"enabled": true`.
pub fn lunaris_json_eq(a: &Json, b: &Json) -> bool {
    fn norm(j: &Json) -> Json {
        match j {
            Json::Obj(o) => {
                let mut v: Vec<(String, Json)> = o.iter().filter(|(k, v)| !(k == "enabled" && *v == Json::Bool(true))).map(|(k, v)| (k.clone(), norm(v))).collect();
                v.sort_by(|x, y| x.0.cmp(&y.0));
                Json::Obj(v)
            }
            Json::Arr(a) => Json::Arr(a.iter().map(norm).collect()),
            Json::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => Json::Int(*f as i64),
            other => other.clone(),
        }
    }
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::parse;

    fn def(s: &str) -> Json {
        parse(s).unwrap()
    }

    const GRAVITY: &str = r#"{"name":"gravity","doc":"Fall.","each":["Transform","Velocity"],"without":["Frozen"],"order":100,"do":[
        {"set":"Velocity.lin[1]","to":{"sub":[{"get":"Velocity.lin[1]"},{"mul":[9.8,{"dt":true}]}]}},
        {"if":{"lt":[{"get":"Transform.pos[1]"},0]},"then":[{"set":"Transform.pos[1]","to":0},{"set":"Velocity.lin[1]","to":{"mul":[{"neg":{"get":"Velocity.lin[1]"}},0.8]}}]},
        {"emit":"landed"}]}"#;

    #[test]
    fn a_system_becomes_nodes_with_pins_and_goes_back_to_the_same_json() {
        let d = def(GRAVITY);
        let bp = Bp::from_json(&d).unwrap();
        // the three statements, the branch's two, and every operation is a node; the numbers are values held by pins
        let ops: Vec<&str> = bp.nodes.iter().map(|n| n.op.as_str()).collect();
        for op in ["set", "if", "emit", "sub", "mul", "get", "dt", "lt", "neg"] {
            assert!(ops.contains(&op), "{op} in {ops:?}");
        }
        assert!(!ops.contains(&"const"), "constants live in pins");
        let back = bp.to_json().unwrap();
        assert!(lunaris_json_eq(&back, &d), "{}", back.to_string_pretty().unwrap());
        // the `if` has its three execution outputs wired: True, False (empty) and Completed
        let iff = bp.nodes.iter().find(|n| n.op == "if").unwrap();
        assert!(bp.wires_from(Pin { node: iff.id, kind: PinKind::ExecOut(0) }).next().is_some());
        assert!(bp.wires_from(Pin { node: iff.id, kind: PinKind::ExecOut(1) }).next().is_none());
        assert!(bp.wires_from(Pin { node: iff.id, kind: PinKind::ExecOut(2) }).next().is_some());
    }

    #[test]
    fn every_system_of_every_example_survives_the_trip_through_nodes() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with(".lunaris.json") {
                continue;
            }
            let scene = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for d in scene.get("systems").and_then(|s| s.as_array()).unwrap_or(&[]) {
                if d.get("do").is_none() {
                    continue;
                }
                let bp = Bp::from_json(d).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let back = bp.to_json().unwrap_or_else(|p| panic!("{}: {p:?}", path.display()));
                assert!(lunaris_json_eq(&back, d), "{}\n{}\n{}", path.display(), d.to_string_compact().unwrap(), back.to_string_compact().unwrap());
                assert!(bp.unreachable().is_empty(), "{}", path.display());
                // the layout puts nothing on top of anything
                for (i, a) in bp.nodes.iter().enumerate() {
                    let (sa, ra) = (node_size(a), a.pos);
                    for b in bp.nodes.iter().skip(i + 1) {
                        let (sb, rb) = (node_size(b), b.pos);
                        let overlap = ra[0] < rb[0] + sb[0] - 1.0 && rb[0] < ra[0] + sa[0] - 1.0 && ra[1] < rb[1] + sb[1] - 1.0 && rb[1] < ra[1] + sa[1] - 1.0;
                        assert!(!overlap, "{}: {} overlaps {} in {}", path.display(), a.op, b.op, d.get("name").unwrap().as_str().unwrap());
                    }
                }
                seen += 1;
            }
        }
        assert!(seen >= 20, "{seen}");
    }

    #[test]
    fn what_the_nodes_cannot_say_is_kept_as_raw_nodes() {
        let d = def(r#"{"name":"odd","each":["A"],"do":[{"future":{"x":1}},{"set":"A.x","to":{"future_op":[1,2]}},{"set":"A.y","to":{"add":[]}}]}"#);
        let bp = Bp::from_json(&d).unwrap();
        assert!(bp.nodes.iter().any(|n| n.op == "raw_stmt") && bp.nodes.iter().any(|n| n.op == "raw_expr"));
        assert!(lunaris_json_eq(&bp.to_json().unwrap(), &d));
    }

    #[test]
    fn wires_follow_the_rules_of_pins() {
        let mut bp = Bp::new("t");
        let set = bp.add_node("set", [0.0, 0.0]);
        let add = bp.add_node("add", [0.0, 0.0]);
        let get = bp.add_node("get", [0.0, 0.0]);
        let emit = bp.add_node("emit", [0.0, 0.0]);
        let out = |n| Pin { node: n, kind: PinKind::DataOut };
        let din = |n, i| Pin { node: n, kind: PinKind::DataIn(i) };
        assert!(bp.connect(out(get), din(add, 0)));
        assert!(bp.connect(out(add), din(set, 0)));
        // a value cannot depend on itself, an output cannot go into an output, an execution wire needs execution pins
        assert!(!bp.connect(out(set), din(add, 1)), "set has no value output");
        assert!(!bp.connect(out(add), din(add, 1)));
        assert!(!bp.connect(out(add), out(get)));
        assert!(!bp.connect(out(get), Pin { node: emit, kind: PinKind::ExecIn }));
        // an input takes one wire: the new one replaces the old
        let get2 = bp.add_node("var", [0.0, 0.0]);
        assert!(bp.connect(out(get2), din(add, 0)));
        assert_eq!(bp.wires.iter().filter(|w| w.to == din(add, 0)).count(), 1);
        assert!(bp.wire_into(din(add, 0)).unwrap().from.node == get2);
        // the execution wires: event -> set -> emit; going back is refused
        let x = |n| Pin { node: n, kind: PinKind::ExecIn };
        assert!(bp.connect(Pin { node: EVENT, kind: PinKind::ExecOut(0) }, x(set)));
        assert!(bp.connect(Pin { node: set, kind: PinKind::ExecOut(0) }, x(emit)));
        assert!(!bp.connect(Pin { node: emit, kind: PinKind::ExecOut(0) }, x(set)));
        let json = bp.to_json().unwrap();
        assert_eq!(json.get("do").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(json.get("do").unwrap().index(0).unwrap().to_string_compact().unwrap(), r#"{"set":"Transform.pos[0]","to":{"add":[{"var":"x"},0]}}"#);
    }

    #[test]
    fn problems_name_the_node_and_unreached_nodes_are_found() {
        let mut bp = Bp::new("t");
        let a = bp.add_node("set", [0.0, 0.0]);
        bp.connect(Pin { node: EVENT, kind: PinKind::ExecOut(0) }, Pin { node: a, kind: PinKind::ExecIn });
        bp.node_mut(a).unwrap().params[0] = String::new();
        let err = bp.to_json().unwrap_err();
        assert!(err.iter().any(|p| p.node == Some(a) && p.message.contains("field")), "{err:?}");
        let lonely = bp.add_node("emit", [0.0, 0.0]);
        assert_eq!(bp.unreachable(), vec![lonely]);
        // an execution circle is refused when wiring, and reported if it is there anyway
        let b = bp.add_node("emit", [0.0, 0.0]);
        bp.node_mut(a).unwrap().params[0] = "A.x".to_string();
        assert!(bp.connect(Pin { node: a, kind: PinKind::ExecOut(0) }, Pin { node: b, kind: PinKind::ExecIn }));
        assert!(!bp.connect(Pin { node: b, kind: PinKind::ExecOut(0) }, Pin { node: a, kind: PinKind::ExecIn }));
    }

    #[test]
    fn named_and_growing_pins_keep_their_wires_in_step() {
        let mut bp = Bp::new("t");
        let s = bp.add_node("spawn", [0.0, 0.0]);
        for name in ["Transform.pos", "Transform.scale", "Velocity.lin", "Tag"] {
            bp.node_mut(s).unwrap().add_input(name);
        }
        let v = bp.add_node("dt", [0.0, 0.0]);
        let w = bp.add_node("tick", [0.0, 0.0]);
        bp.connect(Pin { node: v, kind: PinKind::DataOut }, Pin { node: s, kind: PinKind::DataIn(1) });
        bp.connect(Pin { node: w, kind: PinKind::DataOut }, Pin { node: s, kind: PinKind::DataIn(2) });
        bp.connect(Pin { node: EVENT, kind: PinKind::ExecOut(0) }, Pin { node: s, kind: PinKind::ExecIn });
        bp.remove_input(s, 0);
        assert_eq!(bp.node(s).unwrap().named, vec!["Transform.scale", "Velocity.lin", "Tag"]);
        assert_eq!(bp.wire_into(Pin { node: s, kind: PinKind::DataIn(0) }).unwrap().from.node, v, "the wires moved up with their pins");
        let j = bp.to_json().unwrap();
        assert_eq!(j.get("do").unwrap().index(0).unwrap().to_string_compact().unwrap(), r#"{"spawn":{"Transform":{"scale":{"dt":true}},"Velocity":{"lin":{"tick":true}},"Tag":{}}}"#);
        // and the round trip
        let back = Bp::from_json(&j).unwrap().to_json().unwrap();
        assert!(lunaris_json_eq(&back, &j));
    }

    #[test]
    fn the_layout_file_brings_the_positions_back_but_only_for_the_same_system() {
        let d = def(GRAVITY);
        let mut bp = Bp::from_json(&d).unwrap();
        bp.nodes[1].pos = [777.0, 333.0];
        bp.comments.push(Comment { pos: [0.0, 0.0], size: [300.0, 200.0], title: "Bounce".to_string(), color: 2 });
        let file = bp.to_layout();
        let text = file.to_string_pretty().unwrap();
        let again = Bp::from_layout(&parse(&text).unwrap(), &d).expect("the same system");
        assert_eq!(again.nodes[1].pos, [777.0, 333.0]);
        assert_eq!(again.comments.len(), 1);
        // a different system: the file is ignored
        let other = def(r#"{"name":"gravity","each":["Transform"],"do":[{"emit":"x"}]}"#);
        assert!(Bp::from_layout(&parse(&text).unwrap(), &other).is_none());
    }

    #[test]
    fn random_editing_never_breaks_the_graph_or_loops() {
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut rnd = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for _ in 0..40 {
            let mut bp = Bp::new("fuzz");
            for _ in 0..200 {
                match rnd(6) {
                    0 | 1 => {
                        let op = SPECS[rnd(SPECS.len())].op;
                        bp.add_node(op, [rnd(500) as f32, rnd(500) as f32]);
                    }
                    2 | 3 | 4 => {
                        let ids: Vec<u32> = std::iter::once(EVENT).chain(bp.nodes.iter().map(|n| n.id)).collect();
                        let pick = |r: usize, bp: &Bp, ids: &[u32]| -> Pin {
                            let id = ids[r % ids.len()];
                            let kinds: Vec<PinKind> = match bp.node(id) {
                                None => vec![PinKind::ExecOut(0)],
                                Some(n) => {
                                    let mut k = vec![PinKind::ExecIn, PinKind::DataOut];
                                    k.extend((0..3).map(PinKind::ExecOut));
                                    k.extend((0..n.inputs().max(1)).map(PinKind::DataIn));
                                    k
                                }
                            };
                            Pin { node: id, kind: kinds[r / 7 % kinds.len()] }
                        };
                        let (a, b) = (pick(rnd(10_000), &bp, &ids), pick(rnd(10_000), &bp, &ids));
                        bp.connect(a, b);
                    }
                    _ => {
                        if let Some(id) = bp.nodes.get(rnd(bp.nodes.len().max(1))).map(|n| n.id) {
                            if rnd(3) == 0 {
                                bp.remove_node(id);
                            } else if let Some(n) = bp.node_mut(id) {
                                n.add_input("Transform.pos");
                            }
                        }
                    }
                }
            }
            // whatever was done, it writes JSON or says why not, lays out, and the sidecar round trip holds
            let _ = bp.to_json();
            bp.auto_layout();
            let _ = bp.unreachable();
            let _ = Bp::from_layout(&bp.to_layout(), &Json::Null);
            if let Ok(j) = bp.to_json() {
                let back = Bp::from_json(&j).expect("what is written can be read").to_json().expect("and written again");
                assert!(lunaris_json_eq(&back, &j), "{}
{}", j.to_string_compact().unwrap(), back.to_string_compact().unwrap());
            }
        }
    }

    #[test]
    fn duplicating_copies_nodes_and_the_wires_between_them() {
        let d = def(GRAVITY);
        let mut bp = Bp::from_json(&d).unwrap();
        let ids: Vec<u32> = bp.nodes.iter().filter(|n| n.op == "mul" || n.op == "dt").map(|n| n.id).collect();
        let (n0, w0) = (bp.nodes.len(), bp.wires.len());
        let copies = bp.duplicate(&ids, [20.0, 20.0]);
        assert_eq!(copies.len(), ids.len());
        assert_eq!(bp.nodes.len(), n0 + ids.len());
        assert!(bp.wires.len() > w0, "the dt -> mul wire was copied");
    }
}
