//! The Evolve-0 machine, the subset the creatures' brains use: a register machine over f64 with 64 registers, a memory of `MEMN` numbers, an input of `NTOK` tokens, a random generator and jumps. A line-for-line
//! copy of `vm_run` in `creature_core.h`. The operations (numbers are their index in the operation list of Evolve-0): 0 Nop, 1 Set, 2 Mov, 3 Add, 4 Sub, 5 Mul, 6 Div, 7 Mod, 8 Min, 9 Max, 10 Abs, 11 Neg, 12 Floor,
//! 13 Tanh, 14 Lt, 15 Eq, 16 Load, 17 Store, 19 MemSize, 20 InLen, 21 InGet, 27 Rand, 28 Jmp, 29 Jz, 30 Jnz, 35 Halt, 40 Call, 41 Ret, 45 Djnz, 47 Sin, 48 Cos, 49 RegSet, 50 RegGet; every other number does nothing.
use crate::mathx::{dcos, dsin, dtanh};

pub const NREG: usize = 64;
pub const NTOK: usize = 17 + 8 + 2 * 40 + 6 + 4;
pub const MEMN: usize = 128;

/// One instruction: an operation number, five register numbers (taken modulo 64) and a constant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ins {
    pub op: u8,
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub k: f64,
}

fn ixl(v: f64) -> i64 {
    if v.is_nan() {
        return 0;
    }
    let v = if v > 9.0e15 {
        9.0e15
    } else if v < -9.0e15 {
        -9.0e15
    } else {
        v
    };
    v as i64
}

fn finite_or0(x: f64) -> f64 {
    if x == x && x - x == 0.0 {
        x
    } else {
        0.0
    }
}

fn jumpto(pc: usize, off: i64) -> usize {
    let t = pc as i64 + off;
    if t < 0 {
        0
    } else {
        t as usize
    }
}

fn reg_index(v: f64) -> usize {
    let i = ixl(v) % NREG as i64;
    (if i < 0 { i + NREG as i64 } else { i }) as usize
}

/// Runs `prog` for at most `max_ops` instructions on the registers `reg` (and the memory, the input tokens and the random generator).
pub fn vm_run(prog: &[Ins], reg: &mut [f64; NREG], mem: &mut [f64; MEMN], tok: &[f64; NTOK], rng: &mut u64, max_ops: i32) {
    let len = prog.len();
    let mut pc = 0usize;
    let mut sp = 0usize;
    let mut stack = [0usize; 16];
    for _ in 0..max_ops {
        if pc >= len {
            return;
        }
        let ins = prog[pc];
        let (a, b, c) = ((ins.a & 63) as usize, (ins.b & 63) as usize, (ins.c & 63) as usize);
        let mut next = pc + 1;
        match ins.op {
            1 => reg[a] = ins.k,
            2 => reg[a] = reg[b],
            3 => reg[a] = finite_or0(reg[b] + reg[c]),
            4 => reg[a] = finite_or0(reg[b] - reg[c]),
            5 => reg[a] = finite_or0(reg[b] * reg[c]),
            6 => reg[a] = if reg[c] == 0.0 { 0.0 } else { finite_or0(reg[b] / reg[c]) },
            7 => {
                if reg[c] == 0.0 {
                    reg[a] = 0.0;
                } else {
                    let mut r = reg[b] % reg[c];
                    if r < 0.0 {
                        r += reg[c].abs();
                    }
                    reg[a] = finite_or0(r);
                }
            }
            8 => reg[a] = if reg[b] < reg[c] || reg[c].is_nan() { reg[b] } else { reg[c] },
            9 => reg[a] = if reg[b] > reg[c] || reg[c].is_nan() { reg[b] } else { reg[c] },
            10 => reg[a] = reg[b].abs(),
            11 => reg[a] = -reg[b],
            12 => reg[a] = finite_or0(reg[b].floor()),
            13 => reg[a] = finite_or0(dtanh(reg[b])),
            14 => reg[a] = if reg[b] < reg[c] { 1.0 } else { 0.0 },
            15 => reg[a] = if reg[b] == reg[c] { 1.0 } else { 0.0 },
            16 => {
                let i = ixl(reg[b]);
                reg[a] = if i >= 0 && (i as usize) < MEMN { mem[i as usize] } else { 0.0 };
            }
            17 => {
                let i = ixl(reg[a]);
                if i >= 0 && (i as usize) < MEMN {
                    mem[i as usize] = reg[b];
                }
            }
            19 => reg[a] = MEMN as f64,
            20 => reg[a] = NTOK as f64,
            21 => {
                let i = ixl(reg[b]);
                reg[a] = if i >= 0 {
                    if (i as usize) < NTOK {
                        tok[i as usize]
                    } else {
                        -1.0
                    }
                } else {
                    -1.0
                };
            }
            27 => {
                let mut x = *rng;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                *rng = x;
                reg[a] = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / 9_007_199_254_740_992.0;
            }
            28 => next = jumpto(pc, ixl(ins.k)),
            29 => {
                if reg[a] == 0.0 {
                    next = jumpto(pc, ixl(ins.k));
                }
            }
            30 => {
                if reg[a] != 0.0 {
                    next = jumpto(pc, ixl(ins.k));
                }
            }
            35 => return,
            40 => {
                if sp < 16 {
                    stack[sp] = pc + 1;
                    sp += 1;
                    next = jumpto(pc, ixl(ins.k));
                }
            }
            41 => {
                if sp > 0 {
                    sp -= 1;
                    next = stack[sp];
                } else {
                    return;
                }
            }
            45 => {
                reg[a] -= 1.0;
                if reg[a] > 0.0 {
                    next = jumpto(pc, ixl(ins.k));
                }
            }
            47 => reg[a] = finite_or0(dsin(reg[b])),
            48 => reg[a] = finite_or0(dcos(reg[b])),
            49 => {
                let i = reg_index(reg[a]);
                reg[i] = reg[b];
            }
            50 => {
                let i = reg_index(reg[b]);
                reg[a] = reg[i];
            }
            _ => {}
        }
        pc = next;
    }
}

/// The three programs of a creature's brain, as the trainer saves them (`genomes` of a champions file): `setup` runs once at the start, `leg` is run for each of the 8 legs every tick, `body` once a tick.
#[derive(Clone, Debug, Default)]
pub struct Brain {
    pub setup: Vec<Ins>,
    pub leg: Vec<Ins>,
    pub body: Vec<Ins>,
    /// The weights of a neural brain (`nn::NNP` of them), which replaces the three programs.
    pub nn: Option<Vec<f32>>,
}

fn section_from(v: Option<&lunaris_json::Json>) -> Vec<Ins> {
    let mut out = Vec::new();
    for item in v.and_then(|v| v.as_array()).unwrap_or(&[]) {
        let Some(a) = item.as_array() else { continue };
        let n = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
        // operations the machine does not know (the CPU-only ones of Evolve-0) do nothing, as on the GPU
        let op = n(0) as usize;
        let known = matches!(op, 0..=17 | 19 | 20 | 21 | 27 | 28 | 29 | 30 | 35 | 40 | 41 | 45 | 47..=50);
        out.push(Ins { op: if known { op as u8 } else { 0 }, a: n(1) as u8, b: n(2) as u8, c: n(3) as u8, d: n(4) as u8, e: n(5) as u8, k: n(6) });
    }
    out
}

impl Brain {
    /// A brain from one genome of a champions file: its `setup`, `predict` (the leg program) and `learn` (the body program) sections.
    pub fn from_genome_json(g: &lunaris_json::Json) -> Brain {
        let nn = g.get("nn").and_then(|v| v.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect::<Vec<f32>>()).filter(|w| w.len() == crate::nn::NNP || w.len() == crate::nn::NNP_V1);
        Brain { setup: section_from(g.get("setup")), leg: section_from(g.get("predict")), body: section_from(g.get("learn")), nn }
    }

    /// The brains of a champions file, best first.
    pub fn from_champions_json(text: &str) -> Result<Vec<Brain>, String> {
        let doc = lunaris_json::parse(text).map_err(|e| format!("not JSON: {e:?}"))?;
        let list = doc.get("genomes").and_then(|g| g.as_array()).ok_or("no genomes in the file")?;
        Ok(list.iter().map(Brain::from_genome_json).collect())
    }
}
