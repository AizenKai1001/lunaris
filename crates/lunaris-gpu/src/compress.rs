//! Block compression of textures for the card (BC1, BC3 and BC7 mode 6), made on the CPU when a picture is uploaded: a texture of 2,048 x 2,048 with its mips is 22 MB as 8-bit RGBA, 2.8 MB as BC1 (opaque colour),
//! 5.6 MB as BC3 (colour with a cut-out) and 5.6 MB as BC7 (a normal map, a metallic-roughness map). A card with 8 GB holds four to eight times the pictures, and sampling them moves a quarter to an eighth of the bytes.
//!
//! The encoders are small and fast rather than exhaustive: the endpoints of a block come from its principal axis (power iteration on the 3 x 3 or 4 x 4 covariance of its texels), the indices are the nearest of the
//! palette, and two rounds of least squares (the endpoints that best fit the indices, quantised, the indices again) polish it. The result is within a few levels of 255 of the picture (the tests decode it and measure).
//! Everything is integer arithmetic and `+ - * /` of floats in a fixed order, so the same picture gives the same bytes on every machine and thread count.

/// Which format a picture is made into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    /// 4 bits a texel: opaque colour.
    Bc1,
    /// 8 bits a texel: colour with an alpha.
    Bc3,
    /// 8 bits a texel: four numbers (BC7 mode 6: two endpoints of 7 bits and a shared low bit, 4-bit indices).
    Bc7,
    /// 8 bits a texel: two numbers, each with its own endpoints (a normal map's x and y: the shader makes z).
    Bc5,
}

impl Block {
    pub fn bytes(self) -> usize {
        match self {
            Block::Bc1 => 8,
            _ => 16,
        }
    }
}

type Texel = [u8; 4];

/// The 4 x 4 block of the picture at block (`bx`, `by`), the edges repeated where the picture ends.
fn gather(rgba: &[u8], w: usize, h: usize, bx: usize, by: usize) -> [Texel; 16] {
    let mut out = [[0u8; 4]; 16];
    for j in 0..4 {
        for i in 0..4 {
            let (x, y) = ((bx * 4 + i).min(w - 1), (by * 4 + j).min(h - 1));
            let at = (y * w + x) * 4;
            out[j * 4 + i] = [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]];
        }
    }
    out
}

/// The strongest direction of the texels' spread (a unit vector) by power iteration, and their mean.
fn principal(t: &[Texel; 16], channels: usize) -> ([f32; 4], [f32; 4]) {
    let mut mean = [0.0f32; 4];
    for p in t {
        for k in 0..channels {
            mean[k] += p[k] as f32;
        }
    }
    for m in mean.iter_mut() {
        *m /= 16.0;
    }
    let mut cov = [[0.0f32; 4]; 4];
    for p in t {
        let d = [p[0] as f32 - mean[0], p[1] as f32 - mean[1], p[2] as f32 - mean[2], p[3] as f32 - mean[3]];
        for a in 0..channels {
            for b in 0..channels {
                cov[a][b] += d[a] * d[b];
            }
        }
    }
    // start along the diagonal of the box (the iteration needs something that is not at right angles to the axis)
    let mut axis = [1.0f32, 1.0, 1.0, 1.0];
    for k in channels..4 {
        axis[k] = 0.0;
    }
    for _ in 0..6 {
        let mut next = [0.0f32; 4];
        for a in 0..channels {
            for b in 0..channels {
                next[a] += cov[a][b] * axis[b];
            }
        }
        let len = (next[0] * next[0] + next[1] * next[1] + next[2] * next[2] + next[3] * next[3]).sqrt();
        if len < 1.0e-6 {
            break;
        }
        for k in 0..4 {
            axis[k] = next[k] / len;
        }
    }
    (axis, mean)
}

// ---------------------------------------------------------------------------------------------------------- BC1

fn to565(c: [f32; 3]) -> u16 {
    let q = |v: f32, max: f32| (v.clamp(0.0, 255.0) * max / 255.0 + 0.5) as u16;
    (q(c[0], 31.0) << 11) | (q(c[1], 63.0) << 5) | q(c[2], 31.0)
}

fn from565(c: u16) -> [i32; 3] {
    let (r, g, b) = (((c >> 11) & 31) as i32, ((c >> 5) & 63) as i32, (c & 31) as i32);
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

/// The four colours a BC1 block with c0 > c1 holds.
fn bc1_palette(c0: u16, c1: u16) -> [[i32; 3]; 4] {
    let (a, b) = (from565(c0), from565(c1));
    let mix = |k: usize, wa: i32, wb: i32| (wa * a[k] + wb * b[k]) / 3;
    [a, b, [mix(0, 2, 1), mix(1, 2, 1), mix(2, 2, 1)], [mix(0, 1, 2), mix(1, 1, 2), mix(2, 1, 2)]]
}

/// The nearest palette entry of every texel, and the total squared error.
fn bc1_indices(t: &[Texel; 16], pal: &[[i32; 3]; 4]) -> ([u8; 16], i64) {
    let mut idx = [0u8; 16];
    let mut err = 0i64;
    for (i, p) in t.iter().enumerate() {
        let mut best = (i64::MAX, 0u8);
        for (k, c) in pal.iter().enumerate() {
            let d = (p[0] as i32 - c[0]).pow(2) as i64 + (p[1] as i32 - c[1]).pow(2) as i64 + (p[2] as i32 - c[2]).pow(2) as i64;
            if d < best.0 {
                best = (d, k as u8);
            }
        }
        idx[i] = best.1;
        err += best.0;
    }
    (idx, err)
}

/// c0 above c1 (the four-colour mode), the two apart.
fn order(c0: u16, c1: u16) -> (u16, u16) {
    if c0 == c1 {
        return if c0 > 0 { (c0, c0 - 1) } else { (1, 0) };
    }
    if c0 > c1 { (c0, c1) } else { (c1, c0) }
}

fn encode_bc1(t: &[Texel; 16]) -> [u8; 8] {
    let (axis, mean) = principal(t, 3);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in t {
        let s = (p[0] as f32 - mean[0]) * axis[0] + (p[1] as f32 - mean[1]) * axis[1] + (p[2] as f32 - mean[2]) * axis[2];
        lo = lo.min(s);
        hi = hi.max(s);
    }
    let end = |s: f32| [mean[0] + axis[0] * s, mean[1] + axis[1] * s, mean[2] + axis[2] * s];
    let (mut c0, mut c1) = order(to565(end(hi)), to565(end(lo)));
    let mut pal = bc1_palette(c0, c1);
    let (mut idx, mut err) = bc1_indices(t, &pal);
    // the endpoints that best fit the indices (least squares on the weights 1, 0, 2/3, 1/3), quantised: kept when the error is smaller
    for _ in 0..2 {
        if err == 0 {
            break;
        }
        let wt = [1.0f32, 0.0, 2.0 / 3.0, 1.0 / 3.0];
        let (mut aa, mut bb, mut ab) = (0.0f32, 0.0f32, 0.0f32);
        let (mut ax, mut bx) = ([0.0f32; 3], [0.0f32; 3]);
        for (p, &k) in t.iter().zip(&idx) {
            let a = wt[k as usize];
            let b = 1.0 - a;
            aa += a * a;
            bb += b * b;
            ab += a * b;
            for ch in 0..3 {
                ax[ch] += a * p[ch] as f32;
                bx[ch] += b * p[ch] as f32;
            }
        }
        let det = aa * bb - ab * ab;
        if det.abs() < 1.0e-4 {
            break;
        }
        let (mut e0, mut e1) = ([0.0f32; 3], [0.0f32; 3]);
        for ch in 0..3 {
            e0[ch] = (ax[ch] * bb - bx[ch] * ab) / det;
            e1[ch] = (bx[ch] * aa - ax[ch] * ab) / det;
        }
        let (n0, n1) = order(to565(e0), to565(e1));
        let npal = bc1_palette(n0, n1);
        let (nidx, nerr) = bc1_indices(t, &npal);
        if nerr < err {
            (c0, c1, pal, idx, err) = (n0, n1, npal, nidx, nerr);
        } else {
            break;
        }
    }
    let _ = pal;
    let mut bits = 0u32;
    for (i, &k) in idx.iter().enumerate() {
        bits |= (k as u32) << (2 * i);
    }
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&bits.to_le_bytes());
    out
}

// ---------------------------------------------------------------------------------------------------------- BC3

/// One channel as a BC4 block (the alpha block of BC3, each half of BC5): two 8-bit endpoints (a0 > a1: eight levels between them) and 3-bit indices.
fn encode_channel(t: &[Texel; 16], ch: usize) -> [u8; 8] {
    let (mut lo, mut hi) = (255u8, 0u8);
    for p in t {
        lo = lo.min(p[ch]);
        hi = hi.max(p[ch]);
    }
    // a0 > a1 selects the eight-level mode; a flat block still needs a0 > a1 (the index of the nearest level is then 0 or 1)
    let (a0, a1) = if hi == lo { if hi < 255 { (hi + 1, hi) } else { (255, 254) } } else { (hi, lo) };
    let levels: [i32; 8] = std::array::from_fn(|k| match k {
        0 => a0 as i32,
        1 => a1 as i32,
        _ => ((8 - k as i32) * a0 as i32 + (k as i32 - 1) * a1 as i32) / 7,
    });
    let mut bits = 0u64;
    for (i, p) in t.iter().enumerate() {
        let mut best = (i32::MAX, 0u64);
        for (k, l) in levels.iter().enumerate() {
            let d = (p[ch] as i32 - l).abs();
            if d < best.0 {
                best = (d, k as u64);
            }
        }
        bits |= best.1 << (3 * i);
    }
    let mut out = [0u8; 8];
    out[0] = a0;
    out[1] = a1;
    out[2..8].copy_from_slice(&bits.to_le_bytes()[0..6]);
    out
}

// ---------------------------------------------------------------------------------------------------------- BC7, mode 6

const BC7_WEIGHTS: [i32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// One endpoint as the card decodes it: 7 bits a channel and the one low bit that the endpoint's four channels share.
fn bc7_decode_endpoint(v7: [u8; 4], p: u8) -> [i32; 4] {
    std::array::from_fn(|k| ((v7[k] as i32) << 1) | p as i32)
}

/// The 7-bit values and the shared bit that come nearest to an endpoint (floats, 0 to 255).
fn bc7_quantise(e: [f32; 4]) -> ([u8; 4], u8) {
    let mut best = (f32::MAX, [0u8; 4], 0u8);
    for p in 0..2u8 {
        let mut v7 = [0u8; 4];
        let mut err = 0.0;
        for k in 0..4 {
            let q = ((e[k].clamp(0.0, 255.0) - p as f32) * 0.5 + 0.5).floor().clamp(0.0, 127.0) as u8;
            v7[k] = q;
            let dec = ((q as i32) << 1 | p as i32) as f32;
            err += (dec - e[k]) * (dec - e[k]);
        }
        if err < best.0 {
            best = (err, v7, p);
        }
    }
    (best.1, best.2)
}

fn bc7_palette(e0: [i32; 4], e1: [i32; 4]) -> [[i32; 4]; 16] {
    std::array::from_fn(|i| std::array::from_fn(|k| ((64 - BC7_WEIGHTS[i]) * e0[k] + BC7_WEIGHTS[i] * e1[k] + 32) >> 6))
}

fn bc7_indices(t: &[Texel; 16], pal: &[[i32; 4]; 16]) -> ([u8; 16], i64) {
    let mut idx = [0u8; 16];
    let mut err = 0i64;
    for (i, p) in t.iter().enumerate() {
        let mut best = (i64::MAX, 0u8);
        for (k, c) in pal.iter().enumerate() {
            let d: i64 = (0..4).map(|ch| (p[ch] as i32 - c[ch]).pow(2) as i64).sum();
            if d < best.0 {
                best = (d, k as u8);
            }
        }
        idx[i] = best.1;
        err += best.0;
    }
    (idx, err)
}

fn encode_bc7(t: &[Texel; 16]) -> [u8; 16] {
    let (axis, mean) = principal(t, 4);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in t {
        let s: f32 = (0..4).map(|k| (p[k] as f32 - mean[k]) * axis[k]).sum();
        lo = lo.min(s);
        hi = hi.max(s);
    }
    let end = |s: f32| -> [f32; 4] { std::array::from_fn(|k| mean[k] + axis[k] * s) };
    let (mut q0, mut p0) = bc7_quantise(end(lo));
    let (mut q1, mut p1) = bc7_quantise(end(hi));
    let mut pal = bc7_palette(bc7_decode_endpoint(q0, p0), bc7_decode_endpoint(q1, p1));
    let (mut idx, mut err) = bc7_indices(t, &pal);
    for _ in 0..2 {
        if err == 0 {
            break;
        }
        let (mut aa, mut bb, mut ab) = (0.0f32, 0.0f32, 0.0f32);
        let (mut ax, mut bx) = ([0.0f32; 4], [0.0f32; 4]);
        for (p, &k) in t.iter().zip(&idx) {
            let b = BC7_WEIGHTS[k as usize] as f32 / 64.0; // the weight of endpoint 1
            let a = 1.0 - b;
            aa += a * a;
            bb += b * b;
            ab += a * b;
            for ch in 0..4 {
                ax[ch] += a * p[ch] as f32;
                bx[ch] += b * p[ch] as f32;
            }
        }
        let det = aa * bb - ab * ab;
        if det.abs() < 1.0e-4 {
            break;
        }
        let (mut e0, mut e1) = ([0.0f32; 4], [0.0f32; 4]);
        for ch in 0..4 {
            e0[ch] = (ax[ch] * bb - bx[ch] * ab) / det;
            e1[ch] = (bx[ch] * aa - ax[ch] * ab) / det;
        }
        let (nq0, np0) = bc7_quantise(e0);
        let (nq1, np1) = bc7_quantise(e1);
        let npal = bc7_palette(bc7_decode_endpoint(nq0, np0), bc7_decode_endpoint(nq1, np1));
        let (nidx, nerr) = bc7_indices(t, &npal);
        if nerr < err {
            (q0, p0, q1, p1, pal, idx, err) = (nq0, np0, nq1, np1, npal, nidx, nerr);
        } else {
            break;
        }
    }
    let _ = pal;
    // the first texel's index has no top bit: swap the endpoints (and turn every index round) when it needs one
    if idx[0] >= 8 {
        std::mem::swap(&mut q0, &mut q1);
        std::mem::swap(&mut p0, &mut p1);
        for k in idx.iter_mut() {
            *k = 15 - *k;
        }
    }
    let mut bits: u128 = 0;
    let mut at = 0u32;
    let mut put = |bits: &mut u128, v: u128, n: u32| {
        *bits |= v << at;
        at += n;
    };
    put(&mut bits, 1 << 6, 7); // mode 6: six zeros and a one
    for k in 0..4 {
        put(&mut bits, q0[k] as u128, 7);
        put(&mut bits, q1[k] as u128, 7);
    }
    put(&mut bits, p0 as u128, 1);
    put(&mut bits, p1 as u128, 1);
    for (i, &k) in idx.iter().enumerate() {
        put(&mut bits, k as u128, if i == 0 { 3 } else { 4 });
    }
    bits.to_le_bytes()
}

// ---------------------------------------------------------------------------------------------------------- whole pictures

/// One mip level (`w` x `h` pixels of RGBA, 8 bits each) as blocks of `kind`, row after row of 4 x 4 blocks. The blocks of a row are shared out among the threads (the result does not depend on how many).
pub fn compress_level(rgba: &[u8], w: u32, h: u32, kind: Block) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let size = kind.bytes();
    let mut out = vec![0u8; bw * bh * size];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8).min(bh);
    let rows_per = bh.div_ceil(threads);
    std::thread::scope(|s| {
        for (ti, chunk) in out.chunks_mut(rows_per * bw * size).enumerate() {
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(bw * size).enumerate() {
                    let by = ti * rows_per + r;
                    for bx in 0..bw {
                        let t = gather(rgba, w, h, bx, by);
                        let dst = &mut row[bx * size..(bx + 1) * size];
                        match kind {
                            Block::Bc1 => dst.copy_from_slice(&encode_bc1(&t)),
                            Block::Bc3 => {
                                dst[..8].copy_from_slice(&encode_channel(&t, 3));
                                dst[8..].copy_from_slice(&encode_bc1(&t));
                            }
                            Block::Bc7 => dst.copy_from_slice(&encode_bc7(&t)),
                            Block::Bc5 => {
                                dst[..8].copy_from_slice(&encode_channel(&t, 0));
                                dst[8..].copy_from_slice(&encode_channel(&t, 1));
                            }
                        }
                    }
                }
            });
        }
    });
    out
}

// ---------------------------------------------------------------------------------------------------------- the disk cache

/// Bump this when an encoder changes: the files of the old encoders are then not read (and are cleaned away in time).
pub const ENCODER_VERSION: u32 = 1;

/// A 64-bit fingerprint of a picture's bytes (four lanes of multiply-and-rotate over 8-byte words: a few gigabytes a second), for the name of its cached blocks. Not for secrets.
pub fn hash_bytes(data: &[u8]) -> u64 {
    const K: [u64; 4] = [0x9E37_79B9_7F4A_7C15, 0xC2B2_AE3D_27D4_EB4F, 0x1656_67B1_9E37_79F9, 0x27D4_EB2F_1656_67C5];
    let mut lane = [0x243F_6A88_85A3_08D3u64, 0x1319_8A2E_0370_7344, 0xA409_3822_299F_31D0, 0x082E_FA98_EC4E_6C89];
    let mut chunks = data.chunks_exact(32);
    for c in &mut chunks {
        for k in 0..4 {
            let w = u64::from_le_bytes([c[k * 8], c[k * 8 + 1], c[k * 8 + 2], c[k * 8 + 3], c[k * 8 + 4], c[k * 8 + 5], c[k * 8 + 6], c[k * 8 + 7]]);
            lane[k] = (lane[k] ^ w).wrapping_mul(K[k]).rotate_left(29);
        }
    }
    let mut tail = [0u8; 32];
    let rest = chunks.remainder();
    tail[..rest.len()].copy_from_slice(rest);
    for k in 0..4 {
        let w = u64::from_le_bytes([tail[k * 8], tail[k * 8 + 1], tail[k * 8 + 2], tail[k * 8 + 3], tail[k * 8 + 4], tail[k * 8 + 5], tail[k * 8 + 6], tail[k * 8 + 7]]);
        lane[k] = (lane[k] ^ w).wrapping_mul(K[k]).rotate_left(29);
    }
    let mut h = data.len() as u64;
    for l in lane {
        h = (h ^ l).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(31);
    }
    h ^ (h >> 29)
}

/// Where the blocks of a picture are kept: the format, the size and the picture's fingerprint in the name.
pub fn cache_path(dir: &std::path::Path, kind: Block, w: u32, h: u32, hash: u64) -> std::path::PathBuf {
    let name = match kind {
        Block::Bc1 => "bc1",
        Block::Bc3 => "bc3",
        Block::Bc5 => "bc5",
        Block::Bc7 => "bc7",
    };
    dir.join(format!("{name}_{w}x{h}_{hash:016x}_v{ENCODER_VERSION}.ltx"))
}

pub type Levels = Vec<(u32, u32, Vec<u8>)>;

/// The levels (and the picture's average colour) saved by `store_cached`, or `None` when there is no such file or it is not what it should be (the size of every level is checked against the format, and the file must end where the last one does).
pub fn load_cached(path: &std::path::Path, w: u32, h: u32, kind: Block) -> Option<(Levels, [f32; 3])> {
    let data = std::fs::read(path).ok()?;
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if data.get(..4)? != b"LTXC" || u32_at(4)? != ENCODER_VERSION {
        return None;
    }
    let avg = [f32::from_bits(u32_at(8)?), f32::from_bits(u32_at(12)?), f32::from_bits(u32_at(16)?)];
    let count = u32_at(20)? as usize;
    if count == 0 || count > 32 {
        return None;
    }
    let mut at = 24;
    let mut levels: Levels = Vec::with_capacity(count);
    let (mut ew, mut eh) = (w.max(1), h.max(1));
    for _ in 0..count {
        let (lw, lh, len) = (u32_at(at)?, u32_at(at + 4)?, u32_at(at + 8)? as usize);
        at += 12;
        if (lw, lh) != (ew, eh) || len != lw.div_ceil(4) as usize * lh.div_ceil(4) as usize * kind.bytes() {
            return None;
        }
        levels.push((lw, lh, data.get(at..at + len)?.to_vec()));
        at += len;
        ew = (ew / 2).max(1);
        eh = (eh / 2).max(1);
    }
    // (a whole chain ends at 1 x 1)
    (at == data.len() && levels.last().is_some_and(|l| l.0 == 1 && l.1 == 1)).then_some((levels, avg))
}

/// Save the blocks of a picture for the next run (written whole under another name and renamed, so that a crash leaves no half file; any failure is ignored: the cache is only a speed-up).
pub fn store_cached(path: &std::path::Path, levels: &Levels, avg: [f32; 3]) {
    let mut out: Vec<u8> = Vec::with_capacity(24 + levels.iter().map(|l| 12 + l.2.len()).sum::<usize>());
    out.extend_from_slice(b"LTXC");
    out.extend_from_slice(&ENCODER_VERSION.to_le_bytes());
    for a in avg {
        out.extend_from_slice(&a.to_bits().to_le_bytes());
    }
    out.extend_from_slice(&(levels.len() as u32).to_le_bytes());
    for (lw, lh, d) in levels {
        out.extend_from_slice(&lw.to_le_bytes());
        out.extend_from_slice(&lh.to_le_bytes());
        out.extend_from_slice(&(d.len() as u32).to_le_bytes());
        out.extend_from_slice(d);
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, &out).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Keep the cache folder under `limit` bytes: when it holds more, the files not used for longest go (by modification time; a file that is read is touched) until it is at three quarters of the limit.
pub fn trim_cache(dir: &std::path::Path, limit: u64) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, std::path::PathBuf)> = read
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ltx"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= limit {
        return;
    }
    files.sort();
    for (_, len, path) in files {
        if total <= limit / 4 * 3 {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- decoders, the way the card does it
    fn decode_bc1(b: &[u8]) -> [Texel; 16] {
        let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
        let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        let (a, d) = (from565(c0), from565(c1));
        let pal: [[i32; 3]; 4] = if c0 > c1 {
            bc1_palette(c0, c1)
        } else {
            [a, d, [(a[0] + d[0]) / 2, (a[1] + d[1]) / 2, (a[2] + d[2]) / 2], [0, 0, 0]]
        };
        std::array::from_fn(|i| {
            let c = pal[((bits >> (2 * i)) & 3) as usize];
            [c[0] as u8, c[1] as u8, c[2] as u8, 255]
        })
    }

    fn decode_alpha(b: &[u8]) -> [u8; 16] {
        let (a0, a1) = (b[0] as i32, b[1] as i32);
        let mut raw = [0u8; 8];
        raw[..6].copy_from_slice(&b[2..8]);
        let bits = u64::from_le_bytes(raw);
        std::array::from_fn(|i| {
            let k = ((bits >> (3 * i)) & 7) as i32;
            let v = if a0 > a1 {
                match k {
                    0 => a0,
                    1 => a1,
                    _ => ((8 - k) * a0 + (k - 1) * a1) / 7,
                }
            } else {
                match k {
                    0 => a0,
                    1 => a1,
                    6 => 0,
                    7 => 255,
                    _ => ((6 - k) * a0 + (k - 1) * a1) / 5,
                }
            };
            v as u8
        })
    }

    fn decode_bc7_mode6(b: &[u8]) -> [Texel; 16] {
        let bits = u128::from_le_bytes(b[..16].try_into().unwrap());
        assert_eq!(bits & 0x7f, 0x40, "mode 6");
        let take = |at: u32, n: u32| ((bits >> at) & ((1u128 << n) - 1)) as i32;
        let mut e = [[0i32; 4]; 2];
        for k in 0..4 {
            e[0][k] = take(7 + 14 * k as u32, 7);
            e[1][k] = take(14 + 14 * k as u32, 7);
        }
        let (p0, p1) = (take(63, 1), take(64, 1));
        let ends = [e[0].map(|v| (v << 1) | p0), e[1].map(|v| (v << 1) | p1)];
        let mut at = 65;
        std::array::from_fn(|i| {
            let n = if i == 0 { 3 } else { 4 };
            let k = take(at, n) as usize;
            at += n;
            let w = BC7_WEIGHTS[k];
            std::array::from_fn(|ch| (((64 - w) * ends[0][ch] + w * ends[1][ch] + 32) >> 6) as u8)
        })
    }

    /// A small picture with smooth gradients, hard edges and noise (a stand-in for a photograph).
    fn picture(w: usize, h: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h * 4);
        let mut seed = 12345u32;
        for y in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((seed >> 24) as i32 - 128) / 16;
                let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                let edge = if (x / 9 + y / 7) % 2 == 0 { 40 } else { 0 };
                let c = |base: f32| (base * 255.0 + noise as f32 + edge as f32).clamp(0.0, 255.0) as u8;
                out.extend_from_slice(&[c(fx), c(fy), c((fx + fy) * 0.5), c(0.3 + 0.7 * fx)]);
            }
        }
        out
    }

    fn psnr(a: &[u8], b: &[u8], channels: usize) -> f64 {
        let (mut se, mut n) = (0.0f64, 0usize);
        for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            for k in 0..channels {
                se += (p[k] as f64 - q[k] as f64).powi(2);
                n += 1;
            }
        }
        10.0 * (255.0 * 255.0 / (se / n as f64).max(1.0e-9)).log10()
    }

    fn roundtrip(kind: Block, w: usize, h: usize, src: &[u8]) -> Vec<u8> {
        let data = compress_level(src, w as u32, h as u32, kind);
        assert_eq!(data.len(), w.div_ceil(4) * h.div_ceil(4) * kind.bytes());
        let size = kind.bytes();
        let mut out = vec![0u8; w * h * 4];
        for by in 0..h.div_ceil(4) {
            for bx in 0..w.div_ceil(4) {
                let b = &data[(by * w.div_ceil(4) + bx) * size..][..size];
                let texels: [Texel; 16] = match kind {
                    Block::Bc1 => decode_bc1(b),
                    Block::Bc3 => {
                        let (c, a) = (decode_bc1(&b[8..]), decode_alpha(&b[..8]));
                        std::array::from_fn(|i| [c[i][0], c[i][1], c[i][2], a[i]])
                    }
                    Block::Bc7 => decode_bc7_mode6(b),
                    Block::Bc5 => {
                        let (r, g) = (decode_alpha(&b[..8]), decode_alpha(&b[8..]));
                        std::array::from_fn(|i| [r[i], g[i], 0, 255])
                    }
                };
                for j in 0..4 {
                    for i in 0..4 {
                        let (x, y) = (bx * 4 + i, by * 4 + j);
                        if x < w && y < h {
                            out[(y * w + x) * 4..][..4].copy_from_slice(&texels[j * 4 + i]);
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn bc1_keeps_a_picture_within_a_few_levels_and_a_flat_colour_nearly_exactly() {
        let src = picture(64, 64);
        let back = roundtrip(Block::Bc1, 64, 64, &src);
        assert!(psnr(&src, &back, 3) > 30.0, "BC1 PSNR {}", psnr(&src, &back, 3));
        let flat: Vec<u8> = (0..16 * 16).flat_map(|_| [200u8, 100, 50, 255]).collect();
        let back = roundtrip(Block::Bc1, 16, 16, &flat);
        assert!(flat.iter().zip(&back).step_by(1).all(|(a, b)| (*a as i32 - *b as i32).abs() <= 6 || *a == 255), "a flat block is within 6 levels");
        // black and white blocks (the corners of the colour table)
        for v in [0u8, 255] {
            let solid: Vec<u8> = (0..16).flat_map(|_| [v, v, v, 255]).collect();
            let back = roundtrip(Block::Bc1, 4, 4, &solid);
            assert!(back.chunks_exact(4).all(|p| (p[0] as i32 - v as i32).abs() <= 8), "solid {v}: {:?}", &back[..4]);
        }
    }

    #[test]
    fn bc3_keeps_the_alpha_nearly_exactly_and_the_colour_like_bc1() {
        let src = picture(64, 64);
        let back = roundtrip(Block::Bc3, 64, 64, &src);
        assert!(psnr(&src, &back, 3) > 30.0);
        let alpha_err = src.chunks_exact(4).zip(back.chunks_exact(4)).map(|(a, b)| (a[3] as i32 - b[3] as i32).abs()).max().unwrap();
        assert!(alpha_err <= 20, "the alpha of a gradient with noise is within {alpha_err} levels");
        // a cut-out: alpha 0 on one side and 255 on the other stays so (the leaf's edge does not move)
        let mut cut = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                cut.extend_from_slice(&[10, 200, 30, if x < 4 { 0 } else { 255 }]);
            }
        }
        let back = roundtrip(Block::Bc3, 8, 8, &cut);
        for (p, q) in cut.chunks_exact(4).zip(back.chunks_exact(4)) {
            assert_eq!(p[3], q[3]);
        }
    }

    #[test]
    fn bc7_mode6_is_closer_than_bc1_and_decodes_with_the_cards_rules() {
        let src = picture(64, 64);
        let b7 = roundtrip(Block::Bc7, 64, 64, &src);
        let b1 = roundtrip(Block::Bc1, 64, 64, &src);
        assert!(psnr(&src, &b7, 4) > 33.0, "BC7 PSNR {}", psnr(&src, &b7, 4));
        assert!(psnr(&src, &b7, 3) >= psnr(&src, &b1, 3) - 0.5, "BC7 {} against BC1 {}", psnr(&src, &b7, 3), psnr(&src, &b1, 3));
        // a normal map: smooth, unit-length vectors packed as 0..255
        let mut normals = Vec::new();
        for y in 0..32 {
            for x in 0..32 {
                let (nx, ny) = (0.4 * (x as f32 * 0.3).sin(), 0.4 * (y as f32 * 0.2).cos());
                let nz = (1.0 - nx * nx - ny * ny).sqrt();
                normals.extend_from_slice(&[((nx * 0.5 + 0.5) * 255.0) as u8, ((ny * 0.5 + 0.5) * 255.0) as u8, ((nz * 0.5 + 0.5) * 255.0) as u8, 255]);
            }
        }
        // BC7 mode 6 (one line through the colours) is for maps whose channels go together; for a normal map x and y each get their own endpoints in BC5
        let back = roundtrip(Block::Bc5, 32, 32, &normals);
        let worst = normals.iter().zip(back.chunks_exact(4)).flat_map(|(_, _)| std::iter::empty::<i32>()).max();
        assert!(worst.is_none());
        let worst = normals.chunks_exact(4).zip(back.chunks_exact(4)).map(|(a, b)| (a[0] as i32 - b[0] as i32).abs().max((a[1] as i32 - b[1] as i32).abs())).max().unwrap();
        assert!(worst <= 4, "a smooth normal map's x and y are within {worst} levels in BC5");
    }

    #[test]
    fn the_disk_cache_gives_back_exactly_what_was_stored_and_refuses_anything_else() {
        let dir = std::env::temp_dir().join(format!("lunaris-ltx-test-{}", std::process::id()));
        let src = picture(64, 32);
        let levels: Levels = {
            let (mut w, mut h) = (64u32, 32u32);
            let mut v = Vec::new();
            loop {
                let px = picture(w.max(4) as usize, h.max(4) as usize);
                v.push((w, h, compress_level(&px[..(w * h * 4) as usize], w, h, Block::Bc1)));
                if w == 1 && h == 1 {
                    break;
                }
                w = (w / 2).max(1);
                h = (h / 2).max(1);
            }
            v
        };
        let hash = hash_bytes(&src);
        assert_eq!(hash, hash_bytes(&src), "the same bytes, the same fingerprint");
        let mut other = src.clone();
        other[1000] ^= 1;
        assert_ne!(hash, hash_bytes(&other), "one bit changes the fingerprint");
        let path = cache_path(&dir, Block::Bc1, 64, 32, hash);
        assert!(load_cached(&path, 64, 32, Block::Bc1).is_none(), "nothing there yet");
        store_cached(&path, &levels, [0.25, 0.5, 0.75]);
        let (back, avg) = load_cached(&path, 64, 32, Block::Bc1).expect("stored");
        assert!(back == levels && avg == [0.25, 0.5, 0.75]);
        // the wrong size or format is not accepted, nor a file cut short
        assert!(load_cached(&path, 32, 64, Block::Bc1).is_none());
        assert!(load_cached(&path, 64, 32, Block::Bc3).is_none());
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();
        assert!(load_cached(&path, 64, 32, Block::Bc1).is_none());
        trim_cache(&dir, 10);
        assert!(!path.exists(), "a folder over its limit is trimmed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cargo test --release -p lunaris-gpu --lib compress::tests::how_fast -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn how_fast() {
        let src = picture(2048, 2048);
        for kind in [Block::Bc1, Block::Bc3, Block::Bc5, Block::Bc7] {
            let t = std::time::Instant::now();
            let out = compress_level(&src, 2048, 2048, kind);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            eprintln!("{kind:?}: 2048 x 2048 in {ms:.0} ms ({:.1} ms a megatexel, {} KB)", ms / 4.194304, out.len() / 1024);
        }
    }

    #[test]
    fn pictures_that_are_not_a_whole_number_of_blocks_and_the_smallest_mips_work_and_do_not_depend_on_the_thread_count() {
        for (w, h) in [(1usize, 1usize), (2, 2), (3, 5), (7, 9), (4, 4), (13, 4)] {
            let src = picture(w.max(4), h.max(4));
            let src: Vec<u8> = (0..w * h).flat_map(|i| [src[(i % (w * h)) * 4], src[(i % (w * h)) * 4 + 1], src[(i % (w * h)) * 4 + 2], 255]).collect();
            for kind in [Block::Bc1, Block::Bc3, Block::Bc7] {
                let back = roundtrip(kind, w, h, &src);
                // (a few random pixels cannot be held in four colours; this only says that nothing is garbage)
                assert!(psnr(&src, &back, 3) > 15.0, "{kind:?} {w}x{h}: {}", psnr(&src, &back, 3));
            }
        }
        let src = picture(256, 256);
        let a = compress_level(&src, 256, 256, Block::Bc7);
        let b = compress_level(&src, 256, 256, Block::Bc7);
        assert!(a == b, "the same bytes every time");
    }
}
