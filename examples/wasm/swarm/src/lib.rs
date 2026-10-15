//! A Lunaris WebAssembly system: every entity with Transform and Velocity accelerates towards a target point and is deleted when it
//! gets there. It does exactly what `examples/plugins/swarm_graph.json` (a graph system) and `examples/plugins/swarm` (a native plugin)
//! do, with the arithmetic in the same order, so all three give bit-identical results. This one runs inside the engine's sandbox: a
//! fuel budget, a memory cap, and no access to anything but the batch of fields it is given.
//!
//! Build:  python -I tools/build_wasm.py        (needs the `wasm32-unknown-unknown` Rust target)
//! Use:    {"name": "seek", "wasm": "examples/wasm/swarm.wasm", "each": ["Transform", "Velocity"],
//!          "fields": ["Transform.pos[0]", "Transform.pos[2]", "Velocity.lin[0]", "Velocity.lin[2]"],
//!          "writes": ["Velocity.lin[0]", "Velocity.lin[2]"], "res": ["target.x", "target.z", "seek.k"]}
//!
//! The interface (see crates/lunaris-wasm/src/lib.rs): the engine puts a batch in this module's memory and calls `lunaris_update` with
//! its address.
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[link(wasm_import_module = "lunaris")]
extern "C" {
    /// The engine's deterministic per-entity random number in [0, 1): (seed, tick, the entity in this batch row, salt).
    fn rand(row: i32, salt: i64) -> f64;
}

#[no_mangle]
pub extern "C" fn lunaris_wasm_abi() -> i32 {
    1
}

unsafe fn u32_at(base: *const u8, offset: usize) -> u32 {
    (base.add(offset) as *const u32).read_unaligned()
}

/// Fields (in the order of the system's "fields" list): 0 pos.x, 1 pos.z, 2 vel.x, 3 vel.z. Resources: 0 target.x, 1 target.z,
/// 2 seek.k. Writes: 2 and 3.
#[no_mangle]
pub unsafe extern "C" fn lunaris_update(batch: *mut u8) -> i32 {
    if u32_at(batch, 0) != 1 || u32_at(batch, 12) < 4 || u32_at(batch, 16) < 3 {
        return 1; // not the layout this module was written for
    }
    let n = u32_at(batch, 8) as usize;
    let dt = (batch.add(32) as *const f64).read_unaligned();
    let table = u32_at(batch, 52) as *const u32;
    let res = u32_at(batch, 56) as *const f64;
    let despawn = u32_at(batch, 60) as *mut u8;
    let (px, pz) = (table.add(0).read() as *const f64, table.add(1).read() as *const f64);
    let (vx, vz) = (table.add(2).read() as *mut f64, table.add(3).read() as *mut f64);
    let (tx, tz, k) = (*res.add(0), *res.add(1), *res.add(2));
    for i in 0..n {
        let (dx, dz) = (tx - *px.add(i), tz - *pz.add(i));
        // the same operations, in the same order, as the graph system: a + ((d * k) - v * 0.5) * dt + (rand - 0.5) * 0.02
        *vx.add(i) = *vx.add(i) + (dx * k - *vx.add(i) * 0.5) * dt + (rand(i as i32, 3) - 0.5) * 0.02;
        *vz.add(i) = *vz.add(i) + (dz * k - *vz.add(i) * 0.5) * dt + (rand(i as i32, 4) - 0.5) * 0.02;
        if dx * dx + dz * dz < 0.01 {
            *despawn.add(i) = 1;
        }
    }
    0
}
