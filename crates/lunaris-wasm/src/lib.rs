//! A sandboxed WebAssembly host for Lunaris systems. A system that names a `.wasm` module (`{"wasm": "wasm/swarm.wasm", "each": [...],
//! "fields": [...], "writes": [...], "res": [...]}`) is processed the way a native plugin is (see `docs/PLUGINS.md`): the engine gathers
//! the listed fields of up to 256 entities into arrays of `double`, calls the module's update function once per batch, and writes the
//! fields named in `writes` back. The difference is the sandbox: a module runs on the `wasmi` interpreter inside its own linear memory,
//! can import nothing but `lunaris.rand`, gets a fuel budget per call (an endless loop ends with an error, not a hang), cannot allocate
//! more than 64 MiB, and is started fresh every tick (no state survives from one tick to the next), so it needs no permission switch
//! and a recording replays exactly.
//!
//! The module's side of the contract (version 1):
//!
//! * it exports its linear memory as `memory`, a function `lunaris_wasm_abi() -> i32` returning 1, and the update function
//!   `lunaris_update(batch: i32) -> i32` (the name can differ: `"export": "name"`); a result other than 0 is the system's error;
//! * it may import `(import "lunaris" "rand" (func (param i32 i64) (result f64)))`: the engine's deterministic number in [0, 1) for
//!   (seed, tick, the entity in batch row `param 0`, salt `param 1`);
//! * `batch` is an address in its memory where the engine laid out, for this call (all little-endian):
//!
//! | offset | type | field |
//! |---|---|---|
//! | 0 | u32 | `abi` (1) |
//! | 4 | u32 | `struct_size` (64) |
//! | 8 | u32 | `count`: entities in this batch (at most 256) |
//! | 12 | u32 | `nfields` |
//! | 16 | u32 | `nres` |
//! | 20 | u32 | reserved |
//! | 24 | u64 | `tick` |
//! | 32 | f64 | `dt` |
//! | 40 | u64 | `seed` |
//! | 48 | u32 | `uids`: address of `count` u64 |
//! | 52 | u32 | `fields`: address of `nfields` u32 addresses, each of `count` f64 (write the ones named in `writes`) |
//! | 56 | u32 | `res`: address of `nres` f64 |
//! | 60 | u32 | `despawn`: address of `count` bytes, zero on entry; set one to 1 to delete that entity |
use lunaris_sim::hash_rand;
use lunaris_sim::plugin::{BatchKernel, KernelBatch, KernelSession, WasmLoader};
use std::sync::Arc;
use wasmi::{Caller, Config, Engine, ExternType, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc, ValType};

/// The interface version this host speaks.
pub const ABI: i32 = 1;
const MAX_MODULE_BYTES: usize = 16 << 20;
const MAX_MEMORY_BYTES: usize = 64 << 20;
const PAGE: usize = 65_536;
/// Entities per batch the engine uses (the module's memory is sized for this many).
const BATCH: usize = 256;
const HEADER: usize = 64;

/// The loader the engine uses: `Engine::enable_wasm` in `lunaris-api` installs it.
pub struct Loader;

impl WasmLoader for Loader {
    fn load(&self, wasm: &[u8], export: &str, fuel: u64) -> Result<Arc<dyn BatchKernel>, String> {
        Ok(Arc::new(Kernel::new(wasm, export, fuel)?))
    }
}

/// A validated module, ready to be instantiated once per tick.
struct Kernel {
    engine: Engine,
    module: Module,
    export: String,
    fuel: u64,
}

struct Host {
    limits: StoreLimits,
    tick: u64,
    seed: u64,
    uids: Vec<u64>,
}

fn check_signature(ext: &ExternType, params: &[ValType], results: &[ValType]) -> bool {
    match ext.func() {
        Some(f) => f.params() == params && f.results() == results,
        None => false,
    }
}

impl Kernel {
    fn new(wasm: &[u8], export: &str, fuel: u64) -> Result<Kernel, String> {
        if wasm.len() > MAX_MODULE_BYTES {
            return Err(format!("the module is larger than {} MB", MAX_MODULE_BYTES >> 20));
        }
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|e| format!("not a valid WebAssembly module: {e}"))?;
        for imp in module.imports() {
            let allowed = imp.module() == "lunaris" && imp.name() == "rand" && check_signature(&imp.ty(), &[ValType::I32, ValType::I64], &[ValType::F64]);
            if !allowed {
                return Err(format!("the module imports `{}.{}`; a sandboxed module may import only `lunaris.rand` (i32, i64) -> f64", imp.module(), imp.name()));
            }
        }
        let mut has_abi = false;
        let mut has_update = false;
        let mut has_memory = false;
        for exp in module.exports() {
            match exp.name() {
                "lunaris_wasm_abi" => has_abi = check_signature(&exp.ty(), &[], &[ValType::I32]),
                "memory" => has_memory = exp.ty().memory().is_some(),
                n if n == export => has_update = check_signature(&exp.ty(), &[ValType::I32], &[ValType::I32]),
                _ => {}
            }
        }
        if !has_memory {
            return Err("the module must export its linear memory as `memory`".into());
        }
        if !has_abi {
            return Err("the module must export `lunaris_wasm_abi() -> i32` (it returns 1)".into());
        }
        if !has_update {
            return Err(format!("the module must export the update function `{export}(batch: i32) -> i32`"));
        }
        Ok(Kernel { engine, module, export: export.to_string(), fuel })
    }
}

impl BatchKernel for Kernel {
    fn session(&self) -> Result<Box<dyn KernelSession>, String> {
        let limits = StoreLimitsBuilder::new().memory_size(MAX_MEMORY_BYTES).instances(1).memories(1).tables(8).trap_on_grow_failure(false).build();
        let mut store = Store::new(&self.engine, Host { limits, tick: 0, seed: 0, uids: Vec::new() });
        store.limiter(|h| &mut h.limits);
        store.set_fuel(self.fuel).map_err(|e| format!("cannot give the module fuel: {e}"))?;
        let mut linker = <Linker<Host>>::new(&self.engine);
        linker
            .func_wrap("lunaris", "rand", |caller: Caller<'_, Host>, row: i32, salt: i64| -> f64 {
                let h = caller.data();
                match usize::try_from(row).ok().and_then(|r| h.uids.get(r)) {
                    Some(&uid) => hash_rand(h.seed, h.tick, uid, salt as u64) as f64,
                    None => 0.0,
                }
            })
            .map_err(|e| format!("cannot link the module: {e}"))?;
        let instance = linker.instantiate_and_start(&mut store, &self.module).map_err(|e| describe(&e, self.fuel, "while starting"))?;
        let memory = instance.get_memory(&store, "memory").ok_or("the module does not export `memory`")?;
        let abi = instance.get_typed_func::<(), i32>(&store, "lunaris_wasm_abi").map_err(|e| format!("lunaris_wasm_abi: {e}"))?;
        store.set_fuel(self.fuel).map_err(|e| e.to_string())?;
        let version = abi.call(&mut store, ()).map_err(|e| describe(&e, self.fuel, "in lunaris_wasm_abi"))?;
        if version != ABI {
            return Err(format!("the module speaks interface version {version}, this engine speaks {ABI}"));
        }
        let update = instance.get_typed_func::<i32, i32>(&store, &self.export).map_err(|e| format!("{}: {e}", self.export))?;
        Ok(Box::new(Session { store, memory, update, fuel: self.fuel, export: self.export.clone(), region: None }))
    }
}

/// Fuel trouble gets a plain explanation; everything else keeps the interpreter's own message.
fn describe(e: &wasmi::Error, fuel: u64, what: &str) -> String {
    if e.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
        format!("the module used more than {fuel} instructions in one call ({what}): an endless loop, or work that needs a larger `fuel` budget")
    } else {
        format!("the module failed {what}: {e}")
    }
}

/// Where in the module's memory the batch lives, worked out once (the module's memory grows by the pages the batch needs).
#[derive(Clone, Copy)]
struct Region {
    base: usize,
    nfields: usize,
    nres: usize,
    uids: usize,
    field_table: usize,
    fields: usize,
    res: usize,
    despawn: usize,
}

struct Session {
    store: Store<Host>,
    memory: Memory,
    update: TypedFunc<i32, i32>,
    fuel: u64,
    export: String,
    region: Option<Region>,
}

fn align8(x: usize) -> usize {
    (x + 7) & !7
}

impl Session {
    fn region(&mut self, nfields: usize, nres: usize) -> Result<Region, String> {
        if let Some(r) = self.region {
            if r.nfields == nfields && r.nres == nres {
                return Ok(r);
            }
        }
        // header | uids | table of field addresses | field arrays | resources | despawn flags, all for a full batch
        let uids = HEADER;
        let field_table = align8(uids + BATCH * 8);
        let fields = align8(field_table + nfields * 4);
        let res = align8(fields + nfields * BATCH * 8);
        let despawn = align8(res + nres * 8);
        let total = align8(despawn + BATCH);
        let pages = total.div_ceil(PAGE) as u64;
        let before = self.memory.grow(&mut self.store, pages).map_err(|e| format!("the module cannot get {pages} more pages of memory for a batch: {e}"))?;
        let base = before as usize * PAGE;
        let r = Region { base, nfields, nres, uids: base + uids, field_table: base + field_table, fields: base + fields, res: base + res, despawn: base + despawn };
        self.region = Some(r);
        Ok(r)
    }
}

fn put_u32(mem: &mut [u8], at: usize, v: u32) {
    mem[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(mem: &mut [u8], at: usize, v: u64) {
    mem[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn put_f64(mem: &mut [u8], at: usize, v: f64) {
    mem[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

impl KernelSession for Session {
    fn run(&mut self, b: &mut KernelBatch<'_>) -> Result<(), String> {
        let (nf, nres, n) = (b.fields.len(), b.res.len(), b.count);
        if n > BATCH {
            return Err(format!("a batch of {n} entities is larger than {BATCH}"));
        }
        let r = self.region(nf, nres)?;
        {
            let h = self.store.data_mut();
            h.tick = b.tick;
            h.seed = b.seed;
            h.uids.clear();
            h.uids.extend_from_slice(&b.uids[..n]);
        }
        {
            let mem = self.memory.data_mut(&mut self.store);
            if mem.len() < r.despawn + BATCH {
                return Err("the module's memory shrank".into());
            }
            put_u32(mem, r.base, 1);
            put_u32(mem, r.base + 4, HEADER as u32);
            put_u32(mem, r.base + 8, n as u32);
            put_u32(mem, r.base + 12, nf as u32);
            put_u32(mem, r.base + 16, nres as u32);
            put_u32(mem, r.base + 20, 0);
            put_u64(mem, r.base + 24, b.tick);
            put_f64(mem, r.base + 32, b.dt);
            put_u64(mem, r.base + 40, b.seed);
            put_u32(mem, r.base + 48, r.uids as u32);
            put_u32(mem, r.base + 52, r.field_table as u32);
            put_u32(mem, r.base + 56, r.res as u32);
            put_u32(mem, r.base + 60, r.despawn as u32);
            for (i, u) in b.uids[..n].iter().enumerate() {
                put_u64(mem, r.uids + i * 8, *u);
            }
            for f in 0..nf {
                let at = r.fields + f * BATCH * 8;
                put_u32(mem, r.field_table + f * 4, at as u32);
                for (i, v) in b.fields[f][..n].iter().enumerate() {
                    put_f64(mem, at + i * 8, *v);
                }
            }
            for (i, v) in b.res.iter().enumerate() {
                put_f64(mem, r.res + i * 8, *v);
            }
            mem[r.despawn..r.despawn + n].fill(0);
        }
        self.store.set_fuel(self.fuel).map_err(|e| e.to_string())?;
        let code = self.update.call(&mut self.store, r.base as i32).map_err(|e| describe(&e, self.fuel, &format!("in {}", self.export)))?;
        if code != 0 {
            return Err(format!("the module returned error code {code}"));
        }
        let mem = self.memory.data(&self.store);
        for f in 0..nf {
            let at = r.fields + f * BATCH * 8;
            for (i, v) in b.fields[f][..n].iter_mut().enumerate() {
                let bytes: [u8; 8] = mem[at + i * 8..at + i * 8 + 8].try_into().expect("8 bytes");
                *v = f64::from_le_bytes(bytes);
            }
        }
        b.despawn[..n].copy_from_slice(&mem[r.despawn..r.despawn + n]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRELUDE: &str = r#"(memory (export "memory") 1) (func (export "lunaris_wasm_abi") (result i32) i32.const 1)"#;

    fn run_once(wat: &str, fuel: u64) -> Result<Vec<f64>, String> {
        let kernel = Kernel::new(wat.as_bytes(), "lunaris_update", fuel)?;
        let mut session = kernel.session()?;
        let uids = [1u64, 2, 3];
        let mut fields = vec![vec![10.0f64; 256]];
        let mut despawn = [0u8; 3];
        let mut batch = KernelBatch { count: 3, tick: 5, dt: 0.016, seed: 9, uids: &uids, fields: &mut fields, res: &[], despawn: &mut despawn };
        session.run(&mut batch)?;
        Ok(fields[0][..3].to_vec())
    }

    #[test]
    fn a_module_sees_the_batch_and_can_write_it_back() {
        // adds 1.5 to every value of field 0 (the table holds the address of that array at header offset 52)
        let wat = format!(
            r#"(module {PRELUDE}
               (func (export "lunaris_update") (param $b i32) (result i32)
                 (local $arr i32) (local $i i32)
                 (local.set $arr (i32.load (i32.load offset=52 (local.get $b))))
                 (block $done (loop $each
                   (br_if $done (i32.ge_u (local.get $i) (i32.load offset=8 (local.get $b))))
                   (f64.store (i32.add (local.get $arr) (i32.shl (local.get $i) (i32.const 3)))
                     (f64.add (f64.load (i32.add (local.get $arr) (i32.shl (local.get $i) (i32.const 3)))) (f64.const 1.5)))
                   (local.set $i (i32.add (local.get $i) (i32.const 1)))
                   (br $each)))
                 i32.const 0))"#
        );
        assert_eq!(run_once(&wat, 1_000_000).unwrap(), vec![11.5, 11.5, 11.5]);
    }

    #[test]
    fn fuel_ends_an_endless_loop_on_an_ordinary_stack() {
        let wat = format!(r#"(module {PRELUDE} (func (export "lunaris_update") (param i32) (result i32) (loop $l (br $l)) i32.const 0))"#);
        let err = run_once(&wat, 200_000).unwrap_err();
        assert!(err.contains("used more than 200000 instructions"), "{err}");
    }

    #[test]
    fn rand_is_the_engines_deterministic_number() {
        // writes rand(row 1, salt 3) into field 0 row 0
        let wat = format!(
            r#"(module (import "lunaris" "rand" (func $rand (param i32 i64) (result f64))) {PRELUDE}
               (func (export "lunaris_update") (param $b i32) (result i32)
                 (f64.store (i32.load (i32.load offset=52 (local.get $b))) (call $rand (i32.const 1) (i64.const 3)))
                 i32.const 0))"#
        );
        let got = run_once(&wat, 1_000_000).unwrap()[0];
        assert_eq!(got, hash_rand(9, 5, 2, 3) as f64);
    }
}

/// Hostile modules: whatever they do, the call ends with an error (and on an ordinary thread stack), never a hang or a crash.
#[cfg(test)]
mod hostile {
    use super::*;
    const PRELUDE: &str = r#"(memory (export "memory") 1) (func (export "lunaris_wasm_abi") (result i32) i32.const 1)"#;
    fn count_loop(n: u32) -> String {
        format!(r#"(module {PRELUDE} (func (export "lunaris_update") (param i32) (result i32) (local $i i32) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (i32.const {n}))) (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) i32.const 0))"#)
    }
    fn go(wat: &str, fuel: u64) -> Result<(), String> {
        let kernel = Kernel::new(wat.as_bytes(), "lunaris_update", fuel)?;
        let mut session = kernel.session()?;
        let mut fields = vec![vec![0.0f64; 256]];
        let mut despawn = [0u8; 1];
        let mut batch = KernelBatch { count: 1, tick: 0, dt: 0.0, seed: 0, uids: &[1], fields: &mut fields, res: &[], despawn: &mut despawn };
        session.run(&mut batch)
    }
    fn endless(body: &str) -> String {
        format!(r#"(module {PRELUDE} (func (export "lunaris_update") (param i32) (result i32) (local $i i32) (loop $l {body} (br $l)) i32.const 0))"#)
    }
    #[test]
    fn endless_with_br_if() {
        let wat = format!(r#"(module {PRELUDE} (func (export "lunaris_update") (param i32) (result i32) (loop $l (br_if $l (i32.const 1))) i32.const 0))"#);
        let e = go(&wat, 50_000).unwrap_err();
        assert!(e.contains("50000 instructions"), "{e}");
    }
    #[test]
    fn endless_followed_by_unreachable() {
        let wat = format!(r#"(module {PRELUDE} (func (export "lunaris_update") (param i32) (result i32) (loop $l (br $l)) unreachable))"#);
        let e = go(&wat, 50_000).unwrap_err();
        assert!(e.contains("50000 instructions"), "{e}");
    }
    #[test]
    fn endless_with_a_nop() {
        let e = go(&endless("(nop)"), 50_000).unwrap_err();
        assert!(e.contains("50000 instructions"), "{e}");
    }
    #[test]
    fn endless_with_arithmetic() {
        let e = go(&endless("(local.set $i (i32.add (local.get $i) (i32.const 1)))"), 50_000).unwrap_err();
        assert!(e.contains("50000 instructions"), "{e}");
    }
    #[test]
    fn endless_recursion_is_a_trap() {
        let wat = format!(r#"(module {PRELUDE} (func $r (call $r)) (func (export "lunaris_update") (param i32) (result i32) (call $r) i32.const 0))"#);
        let e = go(&wat, 50_000_000).unwrap_err();
        assert!(e.contains("failed in") || e.contains("instructions"), "{e}");
    }
    #[test]
    fn asking_for_a_gigabyte_of_memory_gets_a_refusal_not_the_memory() {
        // memory.grow returns -1 when the cap says no; the module stores that and returns it as its result code
        let wat = format!(r#"(module {PRELUDE} (func (export "lunaris_update") (param i32) (result i32) (memory.grow (i32.const 20000))))"#);
        let e = go(&wat, 1_000_000).unwrap_err();
        assert!(e.contains("error code -1"), "{e}");
    }
    #[test]
    fn finite_loop_with_enough_fuel() {
        assert!(go(&count_loop(1000), 1_000_000).is_ok());
    }
    #[test]
    fn finite_loop_with_too_little_fuel() {
        let e = go(&count_loop(1_000_000), 5_000).unwrap_err();
        assert!(e.contains("5000 instructions"), "{e}");
    }
}
