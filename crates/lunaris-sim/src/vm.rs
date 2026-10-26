//! Batch execution of graph systems.
//!
//! The interpreter in `graph.rs` walks the expression tree once per entity. This module lowers the same tree to a flat,
//! statically typed register program and runs it over up to `B` entities at a time: every operation is a tight loop over a
//! register of `B` numbers (so the compiler can vectorise it) instead of a tree walk per entity.
//!
//! The interpreter stays the reference. A program must produce exactly what the interpreter does: the same bits in every
//! field, the same structural changes in the same order, the same error message for the same entity (the differential tests in
//! `vm_tests.rs` check this on random programs and worlds). Anything this lowering does not handle with identical semantics
//! (a type mix the interpreter would only reject at run time) is refused and that system keeps using the interpreter.
//!
//! How control flow works without branching per entity: an `if` computes a mask of the rows that take each branch and runs the
//! branch only for them (skipping it when no row does); a write stores only for rows in the current mask; an error (division by
//! zero, a value that does not fit a field) is raised only for rows in the mask, records the message of the entity with the
//! smallest uid, and removes that row from every mask so the rest of its statements are skipped, exactly like the interpreter.
use crate::detmath;
use crate::graph::{binary_math, smoothstep, unary_math, DOp, Expr, MathFn1, MathFn2, Op1, Op2, OpN, Stmt, Val, MAX_DEFERRED_PER_RUN};
use crate::rng::hash_rand;
use lunaris_ecs::{FieldType, StringTable};
use lunaris_json::Json;

/// Entities processed together.
pub(crate) const B: usize = 128;

type Reg = usize;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ty {
    Num,
    Bool,
    Sym,
}

#[derive(Clone, Copy, Debug)]
enum UnOp {
    Neg,
    Abs,
    Floor,
    Ceil,
    Round,
    Sin,
    Cos,
    Not,
}

#[derive(Clone, Copy, Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Min,
    Max,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug)]
enum Op {
    Const { dst: Reg, v: f64 },
    Copy { dst: Reg, src: Reg },
    Get { dst: Reg, term: usize, off: usize, ty: FieldType },
    Dt { dst: Reg },
    Tick { dst: Reg },
    Uid { dst: Reg },
    Res { dst: Reg, idx: usize },
    Rand { dst: Reg, salt: Reg },
    Un { op: UnOp, dst: Reg, a: Reg },
    Sqrt { dst: Reg, a: Reg },
    /// A math-library function that can refuse its argument (the rows where it does fail like the interpreter would).
    UnTry { f: MathFn1, dst: Reg, a: Reg },
    BinTry { f: MathFn2, dst: Reg, a: Reg, b: Reg },
    Smooth { dst: Reg, lo: Reg, hi: Reg, x: Reg },
    Noise { dst: Reg, x: Reg, y: Reg, seed: Reg },
    Bin { op: BinOp, dst: Reg, a: Reg, b: Reg },
    DivMod { is_mod: bool, dst: Reg, a: Reg, b: Reg },
    Clamp { dst: Reg, x: Reg, lo: Reg, hi: Reg },
    Lerp { dst: Reg, a: Reg, b: Reg, t: Reg },
    Blend { dst: Reg, c: Reg, a: Reg, b: Reg },
    /// Narrow the current mask to the rows where `cond` is true (or false when `negate`).
    Push { cond: Reg, negate: bool },
    Pop,
    // statements
    Set { src: Reg, term: usize, off: usize, ty: FieldType, label: String },
    If { cond: Reg, then: Vec<Op>, els: Vec<Op> },
    Despawn,
    Res2 { name: String, src: Reg, add: bool },
    Spawn { comps: Vec<(String, Vec<(String, Reg, Ty)>)> },
    Add { name: String, fields: Vec<(String, Reg, Ty)> },
    Remove { name: String },
    Emit { name: String },
}

#[derive(Debug)]
pub(crate) struct Program {
    ops: Vec<Op>,
    nregs: usize,
}

// ------------------------------------------------------------------------------------------------------------------
// lowering
struct Lower {
    used: usize,
    free: Vec<Reg>,
    locals: Vec<Option<(Reg, Ty)>>,
}

fn field_ty(t: FieldType) -> Ty {
    match t {
        FieldType::Bool => Ty::Bool,
        FieldType::Str => Ty::Sym,
        _ => Ty::Num,
    }
}

impl Lower {
    fn alloc(&mut self) -> Reg {
        self.free.pop().unwrap_or_else(|| {
            self.used += 1;
            self.used - 1
        })
    }

    fn release(&mut self, r: Reg, temp: bool) {
        if temp {
            self.free.push(r);
        }
    }

    fn dest(&mut self, want: Option<Reg>) -> (Reg, bool) {
        match want {
            Some(d) => (d, false),
            None => (self.alloc(), true),
        }
    }

    /// Lower an expression. Returns (register, type, whether the register is a temporary the caller must release).
    fn expr(&mut self, e: &Expr, ops: &mut Vec<Op>, want: Option<Reg>) -> Option<(Reg, Ty, bool)> {
        Some(match e {
            // a tag only matters to the interpreter in a trace; the program has no use for it
            Expr::Tag(inner, _) => return self.expr(inner, ops, want),
            Expr::Const(v) => {
                let (d, tmp) = self.dest(want);
                let (x, ty) = match v {
                    Val::Num(x) => (*x, Ty::Num),
                    Val::Bool(b) => (if *b { 1.0 } else { 0.0 }, Ty::Bool),
                    Val::Sym(s) => (*s as f64, Ty::Sym),
                };
                ops.push(Op::Const { dst: d, v: x });
                (d, ty, tmp)
            }
            Expr::Get(f) => {
                let (d, tmp) = self.dest(want);
                ops.push(Op::Get { dst: d, term: f.term, off: f.offset, ty: f.ty });
                (d, field_ty(f.ty), tmp)
            }
            Expr::Local(slot) => {
                let (r, ty) = self.locals.get(*slot).copied().flatten()?;
                match want {
                    Some(d) => {
                        if d != r {
                            ops.push(Op::Copy { dst: d, src: r });
                        }
                        (d, ty, false)
                    }
                    None => (r, ty, false),
                }
            }
            // contact systems are not batched (their rows are pairs, not an archetype column)
            Expr::UidB | Expr::Contact(_) => return None,
            Expr::Dt => self.leaf(ops, want, |dst| Op::Dt { dst }),
            Expr::Tick => self.leaf(ops, want, |dst| Op::Tick { dst }),
            Expr::Uid => self.leaf(ops, want, |dst| Op::Uid { dst }),
            Expr::Res(idx) => {
                let idx = *idx;
                self.leaf(ops, want, move |dst| Op::Res { dst, idx })
            }
            Expr::Rand(salt) => {
                let (rs, ts, tmp) = self.expr(salt, ops, None)?;
                if ts != Ty::Num {
                    return None;
                }
                self.release(rs, tmp);
                let (d, t) = self.dest(want);
                ops.push(Op::Rand { dst: d, salt: rs });
                (d, Ty::Num, t)
            }
            Expr::U1(op, a) => {
                let (ra, ta, tmp) = self.expr(a, ops, None)?;
                if let Some(f) = unary_math(*op) {
                    if ta != Ty::Num {
                        return None;
                    }
                    self.release(ra, tmp);
                    let (d, t) = self.dest(want);
                    ops.push(Op::UnTry { f, dst: d, a: ra });
                    return Some((d, Ty::Num, t));
                }
                let (uop, want_ty, out_ty) = match op {
                    Op1::Not => (UnOp::Not, Ty::Bool, Ty::Bool),
                    Op1::Neg => (UnOp::Neg, Ty::Num, Ty::Num),
                    Op1::Abs => (UnOp::Abs, Ty::Num, Ty::Num),
                    Op1::Floor => (UnOp::Floor, Ty::Num, Ty::Num),
                    Op1::Ceil => (UnOp::Ceil, Ty::Num, Ty::Num),
                    Op1::Round => (UnOp::Round, Ty::Num, Ty::Num),
                    Op1::Sin => (UnOp::Sin, Ty::Num, Ty::Num),
                    Op1::Cos => (UnOp::Cos, Ty::Num, Ty::Num),
                    Op1::Sqrt => {
                        if ta != Ty::Num {
                            return None;
                        }
                        self.release(ra, tmp);
                        let (d, t) = self.dest(want);
                        ops.push(Op::Sqrt { dst: d, a: ra });
                        return Some((d, Ty::Num, t));
                    }
                    _ => unreachable!("the math library operations were handled above"),
                };
                if ta != want_ty {
                    return None;
                }
                self.release(ra, tmp);
                let (d, t) = self.dest(want);
                ops.push(Op::Un { op: uop, dst: d, a: ra });
                (d, out_ty, t)
            }
            Expr::U2(op, a, b) => {
                let (ra, ta, tmpa) = self.expr(a, ops, None)?;
                let (rb, tb, tmpb) = self.expr(b, ops, None)?;
                let (bop, out_ty) = match op {
                    Op2::Eq | Op2::Ne => {
                        if ta != tb {
                            return None;
                        }
                        (if matches!(op, Op2::Eq) { BinOp::Eq } else { BinOp::Ne }, Ty::Bool)
                    }
                    _ => {
                        if ta != Ty::Num || tb != Ty::Num {
                            return None;
                        }
                        match op {
                            Op2::Sub => (BinOp::Sub, Ty::Num),
                            Op2::Lt => (BinOp::Lt, Ty::Bool),
                            Op2::Le => (BinOp::Le, Ty::Bool),
                            Op2::Gt => (BinOp::Gt, Ty::Bool),
                            Op2::Ge => (BinOp::Ge, Ty::Bool),
                            Op2::Div | Op2::Mod => {
                                self.release(ra, tmpa);
                                self.release(rb, tmpb);
                                let (d, t) = self.dest(want);
                                ops.push(Op::DivMod { is_mod: matches!(op, Op2::Mod), dst: d, a: ra, b: rb });
                                return Some((d, Ty::Num, t));
                            }
                            Op2::Atan2 | Op2::Pow | Op2::Hypot => {
                                self.release(ra, tmpa);
                                self.release(rb, tmpb);
                                let (d, t) = self.dest(want);
                                ops.push(Op::BinTry { f: binary_math(*op).expect("math library operation"), dst: d, a: ra, b: rb });
                                return Some((d, Ty::Num, t));
                            }
                            _ => unreachable!(),
                        }
                    }
                };
                self.release(ra, tmpa);
                self.release(rb, tmpb);
                let (d, t) = self.dest(want);
                ops.push(Op::Bin { op: bop, dst: d, a: ra, b: rb });
                (d, out_ty, t)
            }
            Expr::N(op, args) => {
                if matches!(op, OpN::And | OpN::Or) {
                    // short-circuit: later arguments are only evaluated (and may only fail) where the answer is still open
                    let (acc, t) = self.dest(want);
                    let (r0, t0, tmp0) = self.expr(&args[0], ops, Some(acc))?;
                    debug_assert!(r0 == acc && !tmp0);
                    if t0 != Ty::Bool {
                        return None;
                    }
                    let is_or = matches!(op, OpN::Or);
                    for a in &args[1..] {
                        ops.push(Op::Push { cond: acc, negate: is_or });
                        let (rb, tb, tmpb) = self.expr(a, ops, None)?;
                        if tb != Ty::Bool {
                            return None;
                        }
                        ops.push(Op::Bin { op: if is_or { BinOp::Or } else { BinOp::And }, dst: acc, a: acc, b: rb });
                        ops.push(Op::Pop);
                        self.release(rb, tmpb);
                    }
                    return Some((acc, Ty::Bool, t));
                }
                let bop = match op {
                    OpN::Add => BinOp::Add,
                    OpN::Mul => BinOp::Mul,
                    OpN::Min => BinOp::Min,
                    _ => BinOp::Max,
                };
                // a left fold, like the interpreter: ((a op b) op c) ...
                let (mut racc, ta, mut tmpacc) = self.expr(&args[0], ops, None)?;
                if ta != Ty::Num {
                    return None;
                }
                if args.len() == 1 {
                    // the interpreter still checks the type; a single numeric argument is just itself
                    return match want {
                        Some(d) => {
                            if d != racc {
                                ops.push(Op::Copy { dst: d, src: racc });
                            }
                            self.release(racc, tmpacc);
                            Some((d, Ty::Num, false))
                        }
                        None => Some((racc, Ty::Num, tmpacc)),
                    };
                }
                for (k, a) in args[1..].iter().enumerate() {
                    let (rb, tb, tmpb) = self.expr(a, ops, None)?;
                    if tb != Ty::Num {
                        return None;
                    }
                    self.release(racc, tmpacc);
                    self.release(rb, tmpb);
                    let last = k + 2 == args.len();
                    let (d, t) = if last { self.dest(want) } else { (self.alloc(), true) };
                    ops.push(Op::Bin { op: bop, dst: d, a: racc, b: rb });
                    racc = d;
                    tmpacc = t;
                }
                (racc, Ty::Num, tmpacc)
            }
            Expr::Select(c, a, b) => {
                let (rc, tc, tmpc) = self.expr(c, ops, None)?;
                if tc != Ty::Bool {
                    return None;
                }
                ops.push(Op::Push { cond: rc, negate: false });
                let (ra, ta, tmpa) = self.expr(a, ops, None)?;
                ops.push(Op::Pop);
                ops.push(Op::Push { cond: rc, negate: true });
                let (rb, tb, tmpb) = self.expr(b, ops, None)?;
                ops.push(Op::Pop);
                if ta != tb {
                    return None;
                }
                self.release(rc, tmpc);
                self.release(ra, tmpa);
                self.release(rb, tmpb);
                let (d, t) = self.dest(want);
                ops.push(Op::Blend { dst: d, c: rc, a: ra, b: rb });
                (d, ta, t)
            }
            Expr::Clamp(x, lo, hi) => {
                let (rx, tx, t1) = self.expr(x, ops, None)?;
                let (rl, tl, t2) = self.expr(lo, ops, None)?;
                let (rh, th, t3) = self.expr(hi, ops, None)?;
                if tx != Ty::Num || tl != Ty::Num || th != Ty::Num {
                    return None;
                }
                self.release(rx, t1);
                self.release(rl, t2);
                self.release(rh, t3);
                let (d, t) = self.dest(want);
                ops.push(Op::Clamp { dst: d, x: rx, lo: rl, hi: rh });
                (d, Ty::Num, t)
            }
            Expr::Lerp(a, b, c) => {
                let (ra, ta, t1) = self.expr(a, ops, None)?;
                let (rb, tb, t2) = self.expr(b, ops, None)?;
                let (rt, tt, t3) = self.expr(c, ops, None)?;
                if ta != Ty::Num || tb != Ty::Num || tt != Ty::Num {
                    return None;
                }
                self.release(ra, t1);
                self.release(rb, t2);
                self.release(rt, t3);
                let (d, t) = self.dest(want);
                ops.push(Op::Lerp { dst: d, a: ra, b: rb, t: rt });
                (d, Ty::Num, t)
            }
            Expr::Noise(x, y, seed) => {
                let (rx, tx, t1) = self.expr(x, ops, None)?;
                let (ry, ty, t2) = self.expr(y, ops, None)?;
                let (rs, ts, t3) = self.expr(seed, ops, None)?;
                if tx != Ty::Num || ty != Ty::Num || ts != Ty::Num {
                    return None;
                }
                self.release(rx, t1);
                self.release(ry, t2);
                self.release(rs, t3);
                let (d, t) = self.dest(want);
                ops.push(Op::Noise { dst: d, x: rx, y: ry, seed: rs });
                (d, Ty::Num, t)
            }
            Expr::Smooth(lo, hi, x) => {
                let (rl, tl, t1) = self.expr(lo, ops, None)?;
                let (rh, th, t2) = self.expr(hi, ops, None)?;
                let (rx, tx, t3) = self.expr(x, ops, None)?;
                if tl != Ty::Num || th != Ty::Num || tx != Ty::Num {
                    return None;
                }
                self.release(rl, t1);
                self.release(rh, t2);
                self.release(rx, t3);
                let (d, t) = self.dest(want);
                ops.push(Op::Smooth { dst: d, lo: rl, hi: rh, x: rx });
                (d, Ty::Num, t)
            }
        })
    }

    fn leaf(&mut self, ops: &mut Vec<Op>, want: Option<Reg>, make: impl FnOnce(Reg) -> Op) -> (Reg, Ty, bool) {
        let (d, t) = self.dest(want);
        ops.push(make(d));
        (d, Ty::Num, t)
    }

    fn values(&mut self, vals: &[(String, Expr)], ops: &mut Vec<Op>) -> Option<(Vec<(String, Reg, Ty)>, Vec<Reg>)> {
        let mut out = Vec::new();
        let mut held = Vec::new();
        for (k, e) in vals {
            let (r, t, tmp) = self.expr(e, ops, None)?;
            out.push((k.clone(), r, t));
            if tmp {
                held.push(r); // stays allocated until the statement has used it
            }
        }
        Some((out, held))
    }

    fn block(&mut self, stmts: &[Stmt], ops: &mut Vec<Op>) -> Option<()> {
        let mut declared: Vec<Reg> = Vec::new();
        for s in stmts {
            match s {
                Stmt::Set(f, e) => {
                    let (r, t, tmp) = self.expr(e, ops, None)?;
                    if t != field_ty(f.ty) {
                        return None;
                    }
                    ops.push(Op::Set { src: r, term: f.term, off: f.offset, ty: f.ty, label: f.label.clone() });
                    self.release(r, tmp);
                }
                Stmt::Let(slot, e) => {
                    let r = self.alloc();
                    let (_, t, _) = self.expr(e, ops, Some(r))?;
                    if *slot >= self.locals.len() {
                        self.locals.resize(*slot + 1, None);
                    }
                    self.locals[*slot] = Some((r, t));
                    declared.push(r);
                }
                Stmt::If(c, a, b) => {
                    let (rc, tc, tmp) = self.expr(c, ops, None)?;
                    if tc != Ty::Bool {
                        return None;
                    }
                    self.release(rc, tmp); // the executor copies both masks before running either branch
                    let (mut then, mut els) = (Vec::new(), Vec::new());
                    self.block(a, &mut then)?;
                    self.block(b, &mut els)?;
                    ops.push(Op::If { cond: rc, then, els });
                }
                Stmt::DespawnRole(_) | Stmt::AddRole(..) | Stmt::RemoveRole(..) | Stmt::SpawnPrefab { .. } => return None,
                Stmt::Despawn => ops.push(Op::Despawn),
                Stmt::ResSet(name, e) | Stmt::ResAdd(name, e) => {
                    let (r, t, tmp) = self.expr(e, ops, None)?;
                    if t != Ty::Num {
                        return None;
                    }
                    ops.push(Op::Res2 { name: name.clone(), src: r, add: matches!(s, Stmt::ResAdd(..)) });
                    self.release(r, tmp);
                }
                Stmt::Add(name, fields) => {
                    let (fields, held) = self.values(fields, ops)?;
                    ops.push(Op::Add { name: name.clone(), fields });
                    held.into_iter().for_each(|r| self.release(r, true));
                }
                Stmt::Spawn(comps) => {
                    let mut out = Vec::new();
                    let mut all_held = Vec::new();
                    for (cname, fields) in comps {
                        let (f, held) = self.values(fields, ops)?;
                        all_held.extend(held);
                        out.push((cname.clone(), f));
                    }
                    ops.push(Op::Spawn { comps: out });
                    all_held.into_iter().for_each(|r| self.release(r, true));
                }
                Stmt::Remove(name) => ops.push(Op::Remove { name: name.clone() }),
                // only in a traced system, which has no program
                Stmt::Tag(_) => {}
                Stmt::Emit(name) => ops.push(Op::Emit { name: name.clone() }),
            }
        }
        // variables are only visible inside their block, so their registers can be reused afterwards
        self.free.extend(declared);
        Some(())
    }
}

/// Lower a compiled system body. `None` means "keep interpreting this one".
pub(crate) fn lower(body: &[Stmt], nlocals: usize) -> Option<Program> {
    let mut l = Lower { used: 0, free: Vec::new(), locals: vec![None; nlocals] };
    let mut ops = Vec::new();
    l.block(body, &mut ops)?;
    Some(Program { ops, nregs: l.used.max(1) })
}

// ------------------------------------------------------------------------------------------------------------------
// execution
/// What a batch needs to know about the world for the rows it covers.
pub(crate) struct Ctx<'a> {
    /// Column base pointers of the system's terms in the current archetype.
    pub bases: &'a [*mut u8],
    pub sizes: &'a [usize],
    pub res: &'a [f64],
    pub tick: u64,
    pub dt: f64,
    pub seed: u64,
    pub strings: &'a StringTable,
}

pub(crate) struct Machine {
    regs: Vec<[f64; B]>,
    masks: Vec<[bool; B]>,
    depth: usize,
    n: usize,
    row0: usize,
    uids: [u64; B],
    seq: [u32; B],
}

impl Machine {
    pub fn new(p: &Program) -> Machine {
        Machine { regs: vec![[0.0; B]; p.nregs], masks: vec![[false; B]; 4], depth: 0, n: 0, row0: 0, uids: [0; B], seq: [0; B] }
    }
}

#[inline(always)]
fn bin(regs: &mut [[f64; B]], dst: Reg, a: Reg, b: Reg, n: usize, f: impl Fn(f64, f64) -> f64) {
    if dst != a && dst != b {
        let p = regs.as_mut_ptr();
        // SAFETY: dst differs from a and b, so the mutable slice does not overlap the shared ones (which may overlap each other).
        let (d, x, y) = unsafe { (std::slice::from_raw_parts_mut(p.add(dst).cast::<f64>(), n), std::slice::from_raw_parts(p.add(a).cast::<f64>(), n), std::slice::from_raw_parts(p.add(b).cast::<f64>(), n)) };
        for ((d, x), y) in d.iter_mut().zip(x).zip(y) {
            *d = f(*x, *y);
        }
    } else {
        for i in 0..n {
            let (x, y) = (regs[a][i], regs[b][i]);
            regs[dst][i] = f(x, y);
        }
    }
}

#[inline(always)]
fn un(regs: &mut [[f64; B]], dst: Reg, a: Reg, n: usize, f: impl Fn(f64) -> f64) {
    if dst != a {
        let p = regs.as_mut_ptr();
        // SAFETY: dst differs from a.
        let (d, x) = unsafe { (std::slice::from_raw_parts_mut(p.add(dst).cast::<f64>(), n), std::slice::from_raw_parts(p.add(a).cast::<f64>(), n)) };
        for (d, x) in d.iter_mut().zip(x) {
            *d = f(*x);
        }
    } else {
        for v in &mut regs[dst][..n] {
            *v = f(*v);
        }
    }
}

fn val_json(x: f64, ty: Ty, strings: &StringTable) -> Json {
    match ty {
        Ty::Num => Json::Float(x),
        Ty::Bool => Json::Bool(x != 0.0),
        Ty::Sym => Json::Str(strings.get(x as u32).to_string()),
    }
}

struct Run<'a, 'b> {
    m: &'a mut Machine,
    c: &'a Ctx<'b>,
    out: &'a mut Vec<(u64, u32, DOp)>,
    best: &'a mut Option<(u64, String)>,
}

impl Run<'_, '_> {
    fn fail(&mut self, i: usize, msg: String) {
        let uid = self.m.uids[i];
        if self.best.as_ref().is_none_or(|(u, _)| uid < *u) {
            *self.best = Some((uid, format!("tick {}, entity {uid}: {msg}", self.c.tick)));
        }
        for d in 0..=self.m.depth {
            self.m.masks[d][i] = false;
        }
    }

    fn mask(&self) -> [bool; B] {
        self.m.masks[self.m.depth]
    }

    fn push_mask(&mut self, new: [bool; B]) {
        self.m.depth += 1;
        if self.m.depth >= self.m.masks.len() {
            self.m.masks.push([false; B]);
        }
        let d = self.m.depth;
        self.m.masks[d] = new;
    }

    fn deferred(&mut self, i: usize, op: DOp) {
        if self.out.len() >= MAX_DEFERRED_PER_RUN {
            self.fail(i, format!("more than {MAX_DEFERRED_PER_RUN} structural changes in one tick"));
            return;
        }
        self.m.seq[i] += 1;
        self.out.push((self.m.uids[i], self.m.seq[i], op));
    }

    fn exec(&mut self, ops: &[Op]) {
        let n = self.m.n;
        for op in ops {
            match op {
                Op::Const { dst, v } => self.m.regs[*dst][..n].fill(*v),
                Op::Copy { dst, src } => {
                    let v = self.m.regs[*src];
                    self.m.regs[*dst][..n].copy_from_slice(&v[..n]);
                }
                Op::Get { dst, term, off, ty } => {
                    let (base, size, row0) = (self.c.bases[*term], self.c.sizes[*term], self.m.row0);
                    let d = &mut self.m.regs[*dst];
                    // SAFETY: the base points at this term's column, rows row0..row0+n are in range, offsets come from the layout.
                    unsafe {
                        macro_rules! gather {
                            ($t:ty, $conv:expr) => {{
                                let conv: fn($t) -> f64 = $conv;
                                for (i, v) in d[..n].iter_mut().enumerate() {
                                    *v = conv(base.add((row0 + i) * size + *off).cast::<$t>().read());
                                }
                            }};
                        }
                        match ty {
                            FieldType::F32 => gather!(f32, |x| x as f64),
                            FieldType::F64 => gather!(f64, |x| x),
                            FieldType::I32 => gather!(i32, |x| x as f64),
                            FieldType::U32 | FieldType::Str => gather!(u32, |x| x as f64),
                            FieldType::I64 => gather!(i64, |x| x as f64),
                            FieldType::U64 | FieldType::Uid => gather!(u64, |x| x as f64),
                            FieldType::Bool => gather!(u8, |x| if x != 0 { 1.0 } else { 0.0 }),
                        }
                    }
                }
                Op::Dt { dst } => self.m.regs[*dst][..n].fill(self.c.dt),
                Op::Tick { dst } => self.m.regs[*dst][..n].fill(self.c.tick as f64),
                Op::Uid { dst } => {
                    for i in 0..n {
                        self.m.regs[*dst][i] = self.m.uids[i] as f64;
                    }
                }
                Op::Res { dst, idx } => self.m.regs[*dst][..n].fill(self.c.res[*idx]),
                Op::Rand { dst, salt } => {
                    for i in 0..n {
                        let s = self.m.regs[*salt][i];
                        self.m.regs[*dst][i] = hash_rand(self.c.seed, self.c.tick, self.m.uids[i], s as u64) as f64;
                    }
                }
                Op::Un { op, dst, a } => {
                    let r = &mut self.m.regs;
                    match op {
                        UnOp::Neg => un(r, *dst, *a, n, |x| -x),
                        UnOp::Abs => un(r, *dst, *a, n, f64::abs),
                        UnOp::Floor => un(r, *dst, *a, n, f64::floor),
                        UnOp::Ceil => un(r, *dst, *a, n, f64::ceil),
                        UnOp::Round => un(r, *dst, *a, n, |x| (x + 0.5).floor()),
                        UnOp::Sin => un(r, *dst, *a, n, detmath::sin),
                        UnOp::Cos => un(r, *dst, *a, n, detmath::cos),
                        UnOp::Not => un(r, *dst, *a, n, |x| if x != 0.0 { 0.0 } else { 1.0 }),
                    }
                }
                Op::Sqrt { dst, a } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if mask[i] && self.m.regs[*a][i] < 0.0 {
                            self.fail(i, "sqrt of a negative number".into());
                        }
                    }
                    un(&mut self.m.regs, *dst, *a, n, |x| if x < 0.0 { 0.0 } else { x.sqrt() });
                }
                Op::UnTry { f, dst, a } => {
                    let mask = self.mask();
                    for i in 0..n {
                        match f(self.m.regs[*a][i]) {
                            Ok(v) => self.m.regs[*dst][i] = v,
                            Err(msg) => {
                                if mask[i] {
                                    self.fail(i, msg.into());
                                }
                                self.m.regs[*dst][i] = 0.0;
                            }
                        }
                    }
                }
                Op::BinTry { f, dst, a, b } => {
                    let mask = self.mask();
                    for i in 0..n {
                        match f(self.m.regs[*a][i], self.m.regs[*b][i]) {
                            Ok(v) => self.m.regs[*dst][i] = v,
                            Err(msg) => {
                                if mask[i] {
                                    self.fail(i, msg.into());
                                }
                                self.m.regs[*dst][i] = 0.0;
                            }
                        }
                    }
                }
                Op::Smooth { dst, lo, hi, x } => {
                    let mask = self.mask();
                    for i in 0..n {
                        match smoothstep(self.m.regs[*lo][i], self.m.regs[*hi][i], self.m.regs[*x][i]) {
                            Ok(v) => self.m.regs[*dst][i] = v,
                            Err(msg) => {
                                if mask[i] {
                                    self.fail(i, msg.into());
                                }
                                self.m.regs[*dst][i] = 0.0;
                            }
                        }
                    }
                }
                Op::Bin { op, dst, a, b } => {
                    let r = &mut self.m.regs;
                    let t = |c: bool| if c { 1.0 } else { 0.0 };
                    match op {
                        BinOp::Add => bin(r, *dst, *a, *b, n, |x, y| x + y),
                        BinOp::Sub => bin(r, *dst, *a, *b, n, |x, y| x - y),
                        BinOp::Mul => bin(r, *dst, *a, *b, n, |x, y| x * y),
                        BinOp::Min => bin(r, *dst, *a, *b, n, f64::min),
                        BinOp::Max => bin(r, *dst, *a, *b, n, f64::max),
                        BinOp::Lt => bin(r, *dst, *a, *b, n, |x, y| t(x < y)),
                        BinOp::Le => bin(r, *dst, *a, *b, n, |x, y| t(x <= y)),
                        BinOp::Gt => bin(r, *dst, *a, *b, n, |x, y| t(x > y)),
                        BinOp::Ge => bin(r, *dst, *a, *b, n, |x, y| t(x >= y)),
                        BinOp::Eq => bin(r, *dst, *a, *b, n, |x, y| t(x == y)),
                        BinOp::Ne => bin(r, *dst, *a, *b, n, |x, y| t(x != y)),
                        BinOp::And => bin(r, *dst, *a, *b, n, |x, y| t(x != 0.0 && y != 0.0)),
                        BinOp::Or => bin(r, *dst, *a, *b, n, |x, y| t(x != 0.0 || y != 0.0)),
                    }
                }
                Op::DivMod { is_mod, dst, a, b } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if mask[i] && self.m.regs[*b][i] == 0.0 {
                            self.fail(i, if *is_mod { "modulo by zero" } else { "division by zero" }.into());
                        }
                    }
                    let r = &mut self.m.regs;
                    if *is_mod {
                        bin(r, *dst, *a, *b, n, |x, y| x - y * (x / y).floor());
                    } else {
                        bin(r, *dst, *a, *b, n, |x, y| x / y);
                    }
                }
                Op::Clamp { dst, x, lo, hi } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if mask[i] && self.m.regs[*lo][i] > self.m.regs[*hi][i] {
                            self.fail(i, "clamp: the lower bound is above the upper bound".into());
                        }
                    }
                    for i in 0..n {
                        let r = &self.m.regs;
                        let v = r[*x][i].max(r[*lo][i]).min(r[*hi][i]);
                        self.m.regs[*dst][i] = v;
                    }
                }
                Op::Noise { dst, x, y, seed } => {
                    for i in 0..n {
                        let r = &self.m.regs;
                        let v = crate::noise::noise2(r[*x][i], r[*y][i], r[*seed][i]);
                        self.m.regs[*dst][i] = v;
                    }
                }
                Op::Lerp { dst, a, b, t } => {
                    for i in 0..n {
                        let r = &self.m.regs;
                        let v = r[*a][i] + (r[*b][i] - r[*a][i]) * r[*t][i];
                        self.m.regs[*dst][i] = v;
                    }
                }
                Op::Blend { dst, c, a, b } => {
                    for i in 0..n {
                        let r = &self.m.regs;
                        let v = if r[*c][i] != 0.0 { r[*a][i] } else { r[*b][i] };
                        self.m.regs[*dst][i] = v;
                    }
                }
                Op::Push { cond, negate } => {
                    let mut new = self.mask();
                    for (i, m) in new.iter_mut().enumerate().take(n) {
                        *m = *m && ((self.m.regs[*cond][i] != 0.0) != *negate);
                    }
                    self.push_mask(new);
                }
                Op::Pop => self.m.depth -= 1,
                Op::Set { src, term, off, ty, label } => {
                    let mask = self.mask();
                    let (base, size, row0) = (self.c.bases[*term], self.c.sizes[*term], self.m.row0);
                    for i in 0..n {
                        if !mask[i] {
                            continue;
                        }
                        let x = self.m.regs[*src][i];
                        // SAFETY: as for Get; this row belongs to this chunk, and no other reference to the field is live.
                        unsafe {
                            let p = base.add((row0 + i) * size + *off);
                            match ty {
                                FieldType::Bool => p.write((x != 0.0) as u8),
                                FieldType::Str => p.cast::<u32>().write(x as u32),
                                _ => {
                                    if !x.is_finite() {
                                        self.fail(i, format!("cannot set {label} to a non-finite number"));
                                        continue;
                                    }
                                    match ty {
                                        FieldType::F32 => {
                                            let y = x as f32;
                                            if !y.is_finite() {
                                                self.fail(i, format!("{x} does not fit in the f32 field {label}"));
                                                continue;
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
                            }
                        }
                    }
                }
                Op::If { cond, then, els } => {
                    let cur = self.mask();
                    let (mut t, mut e) = ([false; B], [false; B]);
                    for i in 0..n {
                        let c = self.m.regs[*cond][i] != 0.0;
                        t[i] = cur[i] && c;
                        e[i] = cur[i] && !c;
                    }
                    if t[..n].iter().any(|&b| b) {
                        self.push_mask(t);
                        self.exec(then);
                        self.m.depth -= 1;
                    }
                    if e[..n].iter().any(|&b| b) {
                        self.push_mask(e);
                        self.exec(els);
                        self.m.depth -= 1;
                    }
                }
                Op::Despawn => {
                    let mask = self.mask();
                    for i in 0..n {
                        if mask[i] {
                            self.deferred(i, DOp::Despawn);
                        }
                    }
                }
                Op::Res2 { name, src, add } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if !mask[i] {
                            continue;
                        }
                        let x = self.m.regs[*src][i];
                        if !x.is_finite() {
                            self.fail(i, format!("resource {name} cannot be set to a non-finite number"));
                            continue;
                        }
                        self.deferred(i, if *add { DOp::ResAdd(name.clone(), x) } else { DOp::ResSet(name.clone(), x) });
                    }
                }
                Op::Add { name, fields } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if !mask[i] {
                            continue;
                        }
                        let v = Json::Obj(fields.iter().map(|(k, r, ty)| (k.clone(), val_json(self.m.regs[*r][i], *ty, self.c.strings))).collect());
                        self.deferred(i, DOp::Add(name.clone(), v));
                    }
                }
                Op::Spawn { comps } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if !mask[i] {
                            continue;
                        }
                        let v: Vec<(String, Json)> = comps
                            .iter()
                            .map(|(cname, fields)| (cname.clone(), Json::Obj(fields.iter().map(|(k, r, ty)| (k.clone(), val_json(self.m.regs[*r][i], *ty, self.c.strings))).collect())))
                            .collect();
                        self.deferred(i, DOp::Spawn(v));
                    }
                }
                Op::Emit { name } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if mask[i] {
                            self.deferred(i, DOp::Emit(name.clone()));
                        }
                    }
                }
                Op::Remove { name } => {
                    let mask = self.mask();
                    for i in 0..n {
                        if mask[i] {
                            self.deferred(i, DOp::Remove(name.clone()));
                        }
                    }
                }
            }
        }
    }
}

/// Run `prog` for the `n` rows starting at `row0` of the current archetype (`uids` are those rows' entity uids).
pub(crate) fn run_batch(m: &mut Machine, prog: &Program, c: &Ctx, row0: usize, uids: &[u64], out: &mut Vec<(u64, u32, DOp)>, best: &mut Option<(u64, String)>) {
    let n = uids.len();
    debug_assert!(n > 0 && n <= B);
    m.n = n;
    m.row0 = row0;
    m.depth = 0;
    m.uids[..n].copy_from_slice(uids);
    m.seq[..n].fill(0);
    m.masks[0] = [false; B];
    m.masks[0][..n].fill(true);
    Run { m, c, out, best }.exec(&prog.ops);
}
