//! A Lunaris native plugin: every entity with Transform and Velocity accelerates towards a target point and is deleted when it gets
//! there. It does exactly what the graph system in `examples/plugins/swarm_graph.json` does (the arithmetic is written in the same
//! order, so the two give bit-identical results), but as compiled code working on whole batches of entities at once.
//!
//! Build:  cargo build --release --manifest-path examples/plugins/swarm/Cargo.toml
//! Use:    lunaris ... --allow-plugins, then define a system with  "plugin": "<path to the library>"  (see docs/PLUGINS.md).
//!
//! The interface (crates/lunaris-sim/include/lunaris_plugin.h) is repeated here so the plugin needs nothing from the engine.
#![allow(clippy::missing_safety_doc)]

use std::ffi::c_void;

#[repr(C)]
pub struct LunarisBatch {
    abi: u32,
    struct_size: u32,
    count: u32,
    nfields: u32,
    nres: u32,
    reserved: u32,
    tick: u64,
    dt: f64,
    seed: u64,
    uids: *const u64,
    fields: *mut *mut f64,
    res: *const f64,
    despawn: *mut u8,
    user: *mut c_void,
    rand: extern "C" fn(*const LunarisBatch, u32, u64) -> f64,
}

#[no_mangle]
pub extern "C" fn lunaris_plugin_abi() -> u32 {
    1
}

/// Fields (in the order of the system's "fields" list): 0 pos.x, 1 pos.z, 2 vel.x, 3 vel.z. Resources: 0 target.x, 1 target.z,
/// 2 seek.k. Writes: 2 and 3.
#[no_mangle]
pub unsafe extern "C" fn lunaris_update(batch: *mut LunarisBatch) -> i32 {
    let b = &*batch;
    if b.abi != 1 || b.nfields < 4 || b.nres < 3 {
        return 1; // not the layout this plugin was written for
    }
    let n = b.count as usize;
    let f = std::slice::from_raw_parts(b.fields, b.nfields as usize);
    let (px, pz) = (std::slice::from_raw_parts(f[0], n), std::slice::from_raw_parts(f[1], n));
    let (vx, vz) = (std::slice::from_raw_parts_mut(f[2], n), std::slice::from_raw_parts_mut(f[3], n));
    let res = std::slice::from_raw_parts(b.res, b.nres as usize);
    let (tx, tz, k) = (res[0], res[1], res[2]);
    let despawn = std::slice::from_raw_parts_mut(b.despawn, n);
    let dt = b.dt;
    for i in 0..n {
        let (dx, dz) = (tx - px[i], tz - pz[i]);
        // the same operations, in the same order, as the graph system: a + ((d * k) - v * 0.5) * dt + (rand - 0.5) * 0.02
        vx[i] = vx[i] + (dx * k - vx[i] * 0.5) * dt + ((b.rand)(b, i as u32, 3) - 0.5) * 0.02;
        vz[i] = vz[i] + (dz * k - vz[i] * 0.5) * dt + ((b.rand)(b, i as u32, 4) - 0.5) * 0.02;
        if dx * dx + dz * dz < 0.01 {
            despawn[i] = 1;
        }
    }
    0
}
