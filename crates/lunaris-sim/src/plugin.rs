//! Native plugins: gameplay code written in any compiled language (Rust, C, C++, Zig, Go, C# NativeAOT ...), running inside the
//! simulation at native speed, with the same determinism rules as graph systems.
//!
//! A graph system whose definition names a `plugin` (a shared library in the workspace) is not interpreted: each tick the engine
//! gathers the fields the definition lists, for up to `BATCH` entities at a time, into plain `double` arrays, calls the library's
//! update function once per batch, and writes the fields listed under `writes` back (converted to each field's type, with the same
//! finiteness checks as a graph `set`). The library never sees the world, only these arrays, so it cannot break the engine's
//! invariants; the arrays make it trivially vectorisable. The C interface is in `crates/lunaris-sim/include/lunaris_plugin.h`.
//!
//! Loading native code is as powerful as running a program, so it is off unless the host turned it on (`--allow-plugins`, the
//! environment variable `LUNARIS_ALLOW_PLUGINS=1`, `Engine::allow_plugins`), and the library must live inside the workspace. A
//! plugin must be a pure function of its inputs (the arrays, `dt`, `tick`, the resources and `rand`): no clock, no threads, no
//! global state; then results are bit-identical on every run. The file's hash is part of the state hash, so a recording made
//! with one build of a plugin does not "replay" with another.
use crate::graph::{DOp, FieldRef};
use crate::rng::hash_rand;
use lunaris_ecs::{Chunk, FieldType, World};
use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Entities per call into the plugin.
pub(crate) const BATCH: usize = 256;
/// The plugin interface version this engine speaks.
pub const PLUGIN_ABI: u32 = 1;

/// One batch as a safe host sees it (what a WebAssembly module is given, copied into its own memory): the gathered fields of up to
/// `BATCH` entities. A kernel writes the arrays named in the system's `writes` and sets `despawn` bytes.
pub struct KernelBatch<'a> {
    pub count: usize,
    pub tick: u64,
    pub dt: f64,
    pub seed: u64,
    pub uids: &'a [u64],
    /// `nfields` arrays; the first `count` entries of each are valid.
    pub fields: &'a mut [Vec<f64>],
    pub res: &'a [f64],
    pub despawn: &'a mut [u8],
}

/// The state a kernel keeps while one system processes the batches of one tick (a WebAssembly instance, say). A new session starts
/// every tick, so nothing survives from one tick to the next and a recording replays exactly.
pub trait KernelSession {
    fn run(&mut self, batch: &mut KernelBatch<'_>) -> Result<(), String>;
}

/// A compiled sandboxed module that can process batches.
pub trait BatchKernel: Send + Sync {
    fn session(&self) -> Result<Box<dyn KernelSession>, String>;
}

/// What the host provides to turn the bytes of a `.wasm` file into a kernel (the `lunaris-wasm` crate does it with an interpreter).
/// `export` is the name of the module's update function, `fuel` the instruction budget of one call.
pub trait WasmLoader: Send + Sync {
    fn load(&self, wasm: &[u8], export: &str, fuel: u64) -> Result<Arc<dyn BatchKernel>, String>;
}

/// A WebAssembly host: the loader and the workspace folder modules are read from. Sandboxed code needs no permission switch: it can
/// touch nothing but the batch it is given.
#[derive(Clone)]
pub struct WasmHost {
    pub root: std::path::PathBuf,
    pub loader: Arc<dyn WasmLoader>,
}

impl std::fmt::Debug for WasmHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WasmHost({})", self.root.display())
    }
}

/// What a plugin receives. Layout is the C struct `LunarisBatch` in `lunaris_plugin.h`.
#[repr(C)]
pub struct RawBatch {
    pub abi: u32,
    pub struct_size: u32,
    pub count: u32,
    pub nfields: u32,
    pub nres: u32,
    pub reserved: u32,
    pub tick: u64,
    pub dt: f64,
    pub seed: u64,
    /// `count` uids.
    pub uids: *const u64,
    /// `nfields` pointers, each to `count` doubles: the listed fields of the batch's entities. Write to the ones named in `writes`.
    pub fields: *mut *mut f64,
    /// `nres` values of the resources listed in `res`.
    pub res: *const f64,
    /// `count` bytes, zero on entry: set one to 1 to despawn that entity.
    pub despawn: *mut u8,
    pub user: *mut c_void,
    /// The engine's deterministic per-entity random number in [0, 1): `rand(batch, row, salt)`.
    pub rand: extern "C" fn(*const RawBatch, u32, u64) -> f64,
}

extern "C" fn host_rand(b: *const RawBatch, row: u32, salt: u64) -> f64 {
    // SAFETY: the plugin passes back the pointer it was given; rows outside the batch read nothing.
    let b = unsafe { &*b };
    if row >= b.count {
        return 0.0;
    }
    // SAFETY: `uids` has `count` entries and `row < count`.
    let uid = unsafe { *b.uids.add(row as usize) };
    hash_rand(b.seed, b.tick, uid, salt) as f64
}

// ------------------------------------------------------------------------------------------------------------------
// loading shared libraries (no crates: the two system calls each platform has)
#[cfg(windows)]
mod sys {
    use std::ffi::{c_char, c_void};
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }
    pub fn open(path: &std::path::Path) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 string.
        let h = unsafe { LoadLibraryW(wide.as_ptr()) };
        if h.is_null() {
            // SAFETY: plain call.
            Err(format!("the system could not load {} (Windows error {})", path.display(), unsafe { GetLastError() }))
        } else {
            Ok(h)
        }
    }
    pub fn symbol(h: *mut c_void, name: &std::ffi::CStr) -> *mut c_void {
        // SAFETY: `h` is a live module and `name` is NUL-terminated.
        unsafe { GetProcAddress(h, name.as_ptr()) }
    }
    pub fn close(h: *mut c_void) {
        // SAFETY: `h` came from LoadLibraryW and is closed once.
        unsafe { FreeLibrary(h) };
    }
}

#[cfg(unix)]
mod sys {
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    #[link(name = "dl")]
    extern "C" {
        fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }
    pub fn open(path: &std::path::Path) -> Result<*mut c_void, String> {
        let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| "the path contains a NUL".to_string())?;
        // SAFETY: `c` is NUL-terminated; RTLD_NOW = 2.
        let h = unsafe { dlopen(c.as_ptr(), 2) };
        if h.is_null() {
            // SAFETY: dlerror returns null or a NUL-terminated string.
            let why = unsafe {
                let e = dlerror();
                if e.is_null() { String::new() } else { CStr::from_ptr(e).to_string_lossy().into_owned() }
            };
            Err(format!("the system could not load {}: {why}", path.display()))
        } else {
            Ok(h)
        }
    }
    pub fn symbol(h: *mut c_void, name: &CStr) -> *mut c_void {
        // SAFETY: `h` is a live handle and `name` is NUL-terminated.
        unsafe { dlsym(h, name.as_ptr()) }
    }
    pub fn close(h: *mut c_void) {
        // SAFETY: `h` came from dlopen and is closed once.
        unsafe { dlclose(h) };
    }
}

/// A loaded shared library, unloaded when the last system using it goes away.
pub struct Library {
    handle: *mut c_void,
}

// SAFETY: a library handle is just an address; the functions inside are called only through `PluginSpec`, whose contract
// (documented above) requires them to be reentrant.
unsafe impl Send for Library {}
// SAFETY: as above.
unsafe impl Sync for Library {}

impl Library {
    fn open(path: &Path) -> Result<Library, String> {
        sys::open(path).map(|handle| Library { handle })
    }

    fn symbol(&self, name: &str) -> Option<*mut c_void> {
        let c = std::ffi::CString::new(name).ok()?;
        let p = sys::symbol(self.handle, &c);
        if p.is_null() { None } else { Some(p) }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        sys::close(self.handle);
    }
}

type UpdateFn = unsafe extern "C" fn(*mut RawBatch) -> i32;

enum Backend {
    Native { _lib: Arc<Library>, update: UpdateFn },
    Wasm(Arc<dyn BatchKernel>),
}

/// Everything needed to run one plugin system.
pub(crate) struct PluginSpec {
    backend: Backend,
    /// Gathered into `fields` arrays, in this order.
    pub(crate) fields: Vec<FieldRef>,
    /// Indices into `fields` that are written back after the call.
    pub(crate) writes: Vec<usize>,
    pub(crate) parallel: bool,
    /// FNV-1a of the library file: part of the state hash.
    pub(crate) file_hash: u64,
    pub(crate) path: String,
    pub(crate) export: String,
}

impl PluginSpec {
    /// "plugin" (native code) or "wasm" (sandboxed WebAssembly).
    pub(crate) fn kind(&self) -> &'static str {
        match self.backend {
            Backend::Native { .. } => "plugin",
            Backend::Wasm(_) => "wasm",
        }
    }
}

impl std::fmt::Debug for PluginSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PluginSpec({} :: {}, {} fields, {} written)", self.path, self.export, self.fields.len(), self.writes.len())
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Open `rel` (a path inside `root`) and find `export`. Errors are plain text for the caller to wrap.
pub(crate) fn load(root: &Path, rel: &str, export: &str, fields: Vec<FieldRef>, writes: Vec<usize>, parallel: bool) -> Result<PluginSpec, String> {
    let norm = lunaris_json::safe_relative(rel).map_err(|_| format!("`{rel}` must be a relative path inside the workspace folder"))?;
    let full = root.join(norm);
    let bytes = std::fs::read(&full).map_err(|e| format!("cannot read the plugin {}: {e}", full.display()))?;
    let mut abs = std::fs::canonicalize(&full).map_err(|e| format!("cannot find the plugin {}: {e}", full.display()))?;
    // the Windows loader is happiest without the `\\?\` verbatim prefix canonicalize adds
    let text = abs.to_string_lossy().to_string();
    if let Some(stripped) = text.strip_prefix(r"\\?\") {
        abs = std::path::PathBuf::from(stripped);
    }
    let lib = Arc::new(Library::open(&abs)?);
    let abi = lib.symbol("lunaris_plugin_abi").ok_or_else(|| format!("{rel} does not export `lunaris_plugin_abi`: it is not a Lunaris plugin"))?;
    // SAFETY: the symbol is documented to be `uint32_t lunaris_plugin_abi(void)`.
    let version = unsafe { std::mem::transmute::<*mut c_void, extern "C" fn() -> u32>(abi)() };
    if version != PLUGIN_ABI {
        return Err(format!("{rel} speaks plugin interface version {version}, this engine speaks {PLUGIN_ABI}"));
    }
    let update = lib.symbol(export).ok_or_else(|| format!("{rel} does not export a function named `{export}`"))?;
    // SAFETY: documented signature `int32_t update(LunarisBatch*)`.
    let update = unsafe { std::mem::transmute::<*mut c_void, UpdateFn>(update) };
    Ok(PluginSpec { backend: Backend::Native { _lib: lib, update }, fields, writes, parallel, file_hash: fnv(&bytes), path: rel.to_string(), export: export.to_string() })
}

/// Read the WebAssembly module `rel` (a path inside the host's workspace) and prepare it. Errors are plain text for the caller to wrap.
pub(crate) fn load_wasm(host: &WasmHost, rel: &str, export: &str, fuel: u64, fields: Vec<FieldRef>, writes: Vec<usize>) -> Result<PluginSpec, String> {
    let norm = lunaris_json::safe_relative(rel).map_err(|_| format!("`{rel}` must be a relative path inside the workspace folder"))?;
    let full = host.root.join(norm);
    let meta = std::fs::metadata(&full).map_err(|e| format!("cannot read the module {}: {e}", full.display()))?;
    if meta.len() > 16 << 20 {
        return Err(format!("{} is larger than the 16 MB limit for a module", full.display()));
    }
    let bytes = std::fs::read(&full).map_err(|e| format!("cannot read the module {}: {e}", full.display()))?;
    let kernel = host.loader.load(&bytes, export, fuel).map_err(|e| format!("{rel}: {e}"))?;
    Ok(PluginSpec { backend: Backend::Wasm(kernel), fields, writes, parallel: false, file_hash: fnv(&bytes), path: rel.to_string(), export: export.to_string() })
}

/// Process the chunks claimed from `next`, like the interpreter and the vm do: gather, call, scatter.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn run_chunks(world: &World, spec: &PluginSpec, terms: &[lunaris_ecs::ComponentId], chunks: &[Chunk], next: &AtomicUsize, res: &[f64], tick: u64, dt: f32, seed: u64) -> (Vec<(u64, u32, DOp)>, Option<(u64, String)>) {
    let sizes: Vec<usize> = terms.iter().map(|&t| world.info(t).size).collect();
    let mut session: Option<Box<dyn KernelSession>> = match &spec.backend {
        Backend::Wasm(k) => match k.session() {
            Ok(s) => Some(s),
            Err(e) => return (Vec::new(), Some((0, format!("tick {tick}: {e}")))),
        },
        Backend::Native { .. } => None,
    };
    let nf = spec.fields.len();
    let mut bufs: Vec<Vec<f64>> = (0..nf).map(|_| vec![0.0; BATCH]).collect();
    let mut ptrs: Vec<*mut f64> = bufs.iter_mut().map(|b| b.as_mut_ptr()).collect();
    let mut uids = vec![0u64; BATCH];
    let mut despawn = vec![0u8; BATCH];
    let mut bases: Vec<*mut u8> = vec![std::ptr::null_mut(); terms.len()];
    let mut deferred: Vec<(u64, u32, DOp)> = Vec::new();
    let mut best: Option<(u64, String)> = None;
    let fail = |best: &mut Option<(u64, String)>, uid: u64, msg: String| {
        if best.as_ref().is_none_or(|(u, _)| uid < *u) {
            *best = Some((uid, format!("tick {tick}, entity {uid}: {msg}")));
        }
    };
    loop {
        let k = next.fetch_add(1, Ordering::Relaxed);
        let Some(ch) = chunks.get(k) else { break };
        let arch = &world.archetypes()[ch.arch];
        for (slot, &t) in bases.iter_mut().zip(terms) {
            *slot = arch.column_base(t).expect("matching archetype has the component");
        }
        let mut row0 = ch.start;
        while row0 < ch.end {
            let n = (ch.end - row0).min(BATCH);
            for (i, u) in uids[..n].iter_mut().enumerate() {
                *u = world.uid_of(arch.entities()[row0 + i]).unwrap_or(0);
            }
            // gather
            for (fi, f) in spec.fields.iter().enumerate() {
                let (base, size) = (bases[f.term], sizes[f.term]);
                let dst = &mut bufs[fi];
                // SAFETY: rows row0..row0+n of this chunk's archetype are in range; the offset comes from the layout.
                unsafe {
                    let first = base.add(row0 * size + f.offset);
                    macro_rules! gather {
                        ($t:ty, $conv:expr) => {{
                            let conv: fn($t) -> f64 = $conv;
                            for (i, d) in dst[..n].iter_mut().enumerate() {
                                *d = conv(first.add(i * size).cast::<$t>().read_unaligned());
                            }
                        }};
                    }
                    match f.ty {
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
            despawn[..n].fill(0);
            // (re)take the raw pointers after the safe writes above
            for (p, b) in ptrs.iter_mut().zip(bufs.iter_mut()) {
                *p = b.as_mut_ptr();
            }
            let outcome: Result<(), String> = match (&mut session, &spec.backend) {
                (Some(sess), _) => sess.run(&mut KernelBatch { count: n, tick, dt: dt as f64, seed, uids: &uids[..n], fields: &mut bufs, res, despawn: &mut despawn[..n] }),
                (None, Backend::Native { update, .. }) => {
                    let mut batch = RawBatch {
                        abi: PLUGIN_ABI,
                        struct_size: std::mem::size_of::<RawBatch>() as u32,
                        count: n as u32,
                        nfields: nf as u32,
                        nres: res.len() as u32,
                        reserved: 0,
                        tick,
                        dt: dt as f64,
                        seed,
                        uids: uids.as_ptr(),
                        fields: ptrs.as_mut_ptr(),
                        res: res.as_ptr(),
                        despawn: despawn.as_mut_ptr(),
                        user: std::ptr::null_mut(),
                        rand: host_rand,
                    };
                    // SAFETY: the batch points at arrays that live through the call; the plugin's contract is to stay inside them.
                    let code = unsafe { update(&mut batch) };
                    if code != 0 {
                        Err(format!("the plugin `{}` returned error code {code}", spec.path))
                    } else {
                        Ok(())
                    }
                }
                (None, Backend::Wasm(_)) => Err("a WebAssembly session did not start".to_string()),
            };
            if let Err(msg) = outcome {
                fail(&mut best, uids[0], msg);
            } else {
                // scatter
                for &fi in &spec.writes {
                    let f = &spec.fields[fi];
                    let (base, size) = (bases[f.term], sizes[f.term]);
                    for i in 0..n {
                        let x = bufs[fi][i];
                        // SAFETY: as for the gather; this row belongs to this chunk and nothing else references the field.
                        unsafe {
                            let p = base.add((row0 + i) * size + f.offset);
                            match f.ty {
                                FieldType::Bool => p.write((x != 0.0) as u8),
                                FieldType::Str => p.cast::<u32>().write(x as u32),
                                _ => {
                                    if !x.is_finite() {
                                        fail(&mut best, uids[i], format!("the plugin set {} to a non-finite number", f.label));
                                        continue;
                                    }
                                    match f.ty {
                                        FieldType::F32 => {
                                            let y = x as f32;
                                            if !y.is_finite() {
                                                fail(&mut best, uids[i], format!("{x} does not fit in the f32 field {}", f.label));
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
                for i in 0..n {
                    if despawn[i] != 0 {
                        deferred.push((uids[i], 1, DOp::Despawn));
                    }
                }
            }
            row0 += n;
        }
    }
    (deferred, best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    /// The C header documents this exact layout (64-bit); a plugin compiled against the header must see the same offsets.
    #[test]
    fn the_batch_layout_matches_lunaris_plugin_h() {
        assert_eq!(offset_of!(RawBatch, abi), 0);
        assert_eq!(offset_of!(RawBatch, struct_size), 4);
        assert_eq!(offset_of!(RawBatch, count), 8);
        assert_eq!(offset_of!(RawBatch, nfields), 12);
        assert_eq!(offset_of!(RawBatch, nres), 16);
        assert_eq!(offset_of!(RawBatch, reserved), 20);
        assert_eq!(offset_of!(RawBatch, tick), 24);
        assert_eq!(offset_of!(RawBatch, dt), 32);
        assert_eq!(offset_of!(RawBatch, seed), 40);
        assert_eq!(offset_of!(RawBatch, uids), 48);
        assert_eq!(offset_of!(RawBatch, fields), 56);
        assert_eq!(offset_of!(RawBatch, res), 64);
        assert_eq!(offset_of!(RawBatch, despawn), 72);
        assert_eq!(offset_of!(RawBatch, user), 80);
        assert_eq!(offset_of!(RawBatch, rand), 88);
        assert_eq!(size_of::<RawBatch>(), 96);
        // and the header says the same thing in its own words
        let header = include_str!("../include/lunaris_plugin.h");
        for line in ["uint32_t abi;", "uint32_t struct_size;", "uint32_t count;", "uint32_t nfields;", "uint32_t nres;", "uint32_t reserved;", "uint64_t tick;", "double   dt;", "uint64_t seed;", "const uint64_t* uids;", "double** fields;", "const double* res;", "uint8_t* despawn;", "void* user;"] {
            assert!(header.contains(line), "the header should declare `{line}`");
        }
    }

    #[test]
    fn the_fnv_hash_is_stable() {
        assert_eq!(fnv(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
