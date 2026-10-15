//! A small PNG reader for heightmaps, so that the simulation can read the ground it stands on without a dependency: greyscale (or colour,
//! of which the first channel counts) at 8 or 16 bits, any size up to 64 million samples, no interlacing. It checks what it needs and
//! refuses the rest with a message; it never panics on a damaged file.
//!
//! The decompressor is the standard "inflate" (RFC 1951) in the plain table-free form: canonical Huffman codes read bit by bit.

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    hold: u32,
    count: u32,
}

impl<'a> Bits<'a> {
    fn need(&mut self, n: u32) -> Result<u32, String> {
        while self.count < n {
            let b = *self.data.get(self.pos).ok_or("the compressed data ends too soon")?;
            self.pos += 1;
            self.hold |= (b as u32) << self.count;
            self.count += 8;
        }
        let v = self.hold & ((1u32 << n) - 1);
        self.hold >>= n;
        self.count -= n;
        Ok(v)
    }

    fn align(&mut self) {
        self.hold = 0;
        self.count = 0;
    }
}

struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

fn build(lengths: &[u8]) -> Result<Huffman, String> {
    let mut count = [0u16; 16];
    for &l in lengths {
        count[l as usize] += 1;
    }
    // over-subscribed sets are an error; incomplete ones are allowed (a single distance code)
    let mut left: i32 = 1;
    for len in 1..16 {
        left <<= 1;
        left -= count[len] as i32;
        if left < 0 {
            return Err("a damaged code table".to_string());
        }
    }
    let mut offs = [0u16; 16];
    for len in 1..15 {
        offs[len + 1] = offs[len] + count[len];
    }
    let mut symbol = vec![0u16; lengths.len()];
    for (s, &l) in lengths.iter().enumerate() {
        if l != 0 {
            symbol[offs[l as usize] as usize] = s as u16;
            offs[l as usize] += 1;
        }
    }
    Ok(Huffman { count, symbol })
}

fn decode(b: &mut Bits, h: &Huffman) -> Result<usize, String> {
    let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
    for len in 1..16 {
        code |= b.need(1)? as i32;
        let count = h.count[len] as i32;
        if code - count < first {
            return h.symbol.get((index + (code - first)) as usize).map(|s| *s as usize).ok_or_else(|| "a damaged code".to_string());
        }
        index += count;
        first += count;
        first <<= 1;
        code <<= 1;
    }
    Err("a code that does not exist".to_string())
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn codes(b: &mut Bits, out: &mut Vec<u8>, limit: usize, lit: &Huffman, dist: &Huffman) -> Result<(), String> {
    loop {
        let sym = decode(b, lit)?;
        if sym < 256 {
            if out.len() >= limit {
                return Err("more data than the picture can hold".to_string());
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let s = sym - 257;
            if s >= 29 {
                return Err("a damaged length".to_string());
            }
            let len = LBASE[s] as usize + b.need(LEXT[s] as u32)? as usize;
            let ds = decode(b, dist)?;
            if ds >= 30 {
                return Err("a damaged distance".to_string());
            }
            let d = DBASE[ds] as usize + b.need(DEXT[ds] as u32)? as usize;
            if d > out.len() {
                return Err("a copy from before the start".to_string());
            }
            if out.len() + len > limit {
                return Err("more data than the picture can hold".to_string());
            }
            for _ in 0..len {
                let v = out[out.len() - d];
                out.push(v);
            }
        }
    }
}

/// Inflate a raw deflate stream; `limit` is the most output allowed (the size the picture must have).
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut b = Bits { data, pos: 0, hold: 0, count: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(limit.min(1 << 26));
    loop {
        let last = b.need(1)?;
        match b.need(2)? {
            0 => {
                b.align();
                let p = b.pos;
                let head = data.get(p..p + 4).ok_or("a stored block is cut short")?;
                let len = u16::from_le_bytes([head[0], head[1]]) as usize;
                let nlen = u16::from_le_bytes([head[2], head[3]]);
                if len as u16 != !nlen {
                    return Err("a damaged stored block".to_string());
                }
                let body = data.get(p + 4..p + 4 + len).ok_or("a stored block is cut short")?;
                if out.len() + len > limit {
                    return Err("more data than the picture can hold".to_string());
                }
                out.extend_from_slice(body);
                b.pos = p + 4 + len;
            }
            1 => {
                let mut l = [0u8; 288];
                for (i, v) in l.iter_mut().enumerate() {
                    *v = if i < 144 {
                        8
                    } else if i < 256 {
                        9
                    } else if i < 280 {
                        7
                    } else {
                        8
                    };
                }
                let lit = build(&l)?;
                let dist = build(&[5u8; 30])?;
                codes(&mut b, &mut out, limit, &lit, &dist)?;
            }
            2 => {
                let nlen = b.need(5)? as usize + 257;
                let ndist = b.need(5)? as usize + 1;
                let ncode = b.need(4)? as usize + 4;
                if nlen > 286 || ndist > 30 {
                    return Err("a damaged block header".to_string());
                }
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut lengths = [0u8; 320];
                for &o in ORDER.iter().take(ncode) {
                    lengths[o] = b.need(3)? as u8;
                }
                let lencode = build(&lengths[..19])?;
                let mut idx = 0;
                let mut all = [0u8; 320];
                while idx < nlen + ndist {
                    let sym = decode(&mut b, &lencode)?;
                    if sym < 16 {
                        all[idx] = sym as u8;
                        idx += 1;
                    } else {
                        let (value, rep) = match sym {
                            16 => {
                                if idx == 0 {
                                    return Err("a damaged table".to_string());
                                }
                                (all[idx - 1], 3 + b.need(2)? as usize)
                            }
                            17 => (0, 3 + b.need(3)? as usize),
                            _ => (0, 11 + b.need(7)? as usize),
                        };
                        if idx + rep > nlen + ndist {
                            return Err("a damaged table".to_string());
                        }
                        for _ in 0..rep {
                            all[idx] = value;
                            idx += 1;
                        }
                    }
                }
                if all[256] == 0 {
                    return Err("a block without an end".to_string());
                }
                let lit = build(&all[..nlen])?;
                let dist = build(&all[nlen..nlen + ndist])?;
                codes(&mut b, &mut out, limit, &lit, &dist)?;
            }
            _ => return Err("a damaged block".to_string()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

/// A decoded heightmap: samples scaled to 0..=65535, row by row from the top.
#[derive(Debug)]
pub struct Decoded {
    pub cols: usize,
    pub rows: usize,
    pub samples: Vec<u16>,
}

const MAX_SAMPLES: usize = 64 * 1024 * 1024;

pub fn decode_png(bytes: &[u8]) -> Result<Decoded, String> {
    if bytes.len() < 8 || bytes[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Err("this is not a PNG file".to_string());
    }
    let (mut w, mut h, mut depth, mut color, mut interlace) = (0usize, 0usize, 0u8, 0u8, 0u8);
    let mut idat: Vec<u8> = Vec::new();
    let mut at = 8;
    let mut seen_header = false;
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes.get(at + 8..at + 8 + len).ok_or("a chunk is cut short")?;
        match kind {
            b"IHDR" => {
                if body.len() < 13 {
                    return Err("a damaged header".to_string());
                }
                w = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                h = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                depth = body[8];
                color = body[9];
                interlace = body[12];
                seen_header = true;
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    if !seen_header || w == 0 || h == 0 {
        return Err("the PNG has no header".to_string());
    }
    if w.checked_mul(h).is_none_or(|n| n > MAX_SAMPLES) {
        return Err(format!("{w} x {h} samples is more than a heightmap may have ({MAX_SAMPLES})"));
    }
    if interlace != 0 {
        return Err("interlaced PNGs are not supported (save it without interlacing)".to_string());
    }
    if depth != 8 && depth != 16 {
        return Err(format!("a heightmap must be 8 or 16 bits deep (this one is {depth})"));
    }
    let channels = match color {
        0 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err("a heightmap must be greyscale (or colour): palette pictures are not supported".to_string()),
    };
    let bytes_per_sample = (depth / 8) as usize;
    let bpp = channels * bytes_per_sample;
    let stride = w * bpp;
    let raw_len = h * (stride + 1);
    if idat.len() < 6 {
        return Err("the picture has no data".to_string());
    }
    // zlib: two header bytes, the deflate stream, four check bytes
    if idat[0] & 0x0F != 8 || (u16::from(idat[0]) << 8 | u16::from(idat[1])) % 31 != 0 {
        return Err("the picture data is not zlib".to_string());
    }
    let raw = inflate(&idat[2..], raw_len)?;
    if raw.len() != raw_len {
        return Err("the picture data is shorter than the picture".to_string());
    }
    // undo the row filters
    let mut img = vec![0u8; h * stride];
    for y in 0..h {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (before, row) = img.split_at_mut(y * stride);
        let row = &mut row[..stride];
        let prev: &[u8] = if y == 0 { &[] } else { &before[(y - 1) * stride..] };
        for x in 0..stride {
            let a = if x >= bpp { row[x - bpp] as i32 } else { 0 };
            let b = if y > 0 { prev[x] as i32 } else { 0 };
            let c = if y > 0 && x >= bpp { prev[x - bpp] as i32 } else { 0 };
            let add = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => return Err("a damaged row".to_string()),
            };
            row[x] = (src[x] as i32 + add) as u8;
        }
    }
    let mut samples = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let p = y * stride + x * bpp;
            samples.push(if bytes_per_sample == 2 { u16::from_be_bytes([img[p], img[p + 1]]) } else { img[p] as u16 * 257 });
        }
    }
    Ok(Decoded { cols: w, rows: h, samples })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut k = kind.to_vec();
        k.extend_from_slice(body);
        out.extend_from_slice(&k);
        out.extend_from_slice(&crc(&k).to_be_bytes());
    }

    /// A PNG with stored (uncompressed) blocks and the given filter on every row.
    pub fn make_png(w: usize, h: usize, depth: u8, color: u8, filter: u8, samples: &[u16]) -> Vec<u8> {
        let channels = match color {
            0 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        };
        let bps = (depth / 8) as usize;
        let bpp = channels * bps;
        let stride = w * bpp;
        let mut plain = vec![0u8; h * stride];
        for y in 0..h {
            for x in 0..w {
                for c in 0..channels {
                    let v = samples[(y * w + x) * channels + c];
                    let p = y * stride + (x * channels + c) * bps;
                    if bps == 2 {
                        plain[p..p + 2].copy_from_slice(&v.to_be_bytes());
                    } else {
                        plain[p] = v as u8;
                    }
                }
            }
        }
        // filter each row
        let mut raw = Vec::new();
        for y in 0..h {
            raw.push(filter);
            for x in 0..stride {
                let a = if x >= bpp { plain[y * stride + x - bpp] as i32 } else { 0 };
                let b = if y > 0 { plain[(y - 1) * stride + x] as i32 } else { 0 };
                let c = if y > 0 && x >= bpp { plain[(y - 1) * stride + x - bpp] as i32 } else { 0 };
                let pred = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => (a + b) / 2,
                    _ => {
                        let p = a + b - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                        if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            b
                        } else {
                            c
                        }
                    }
                };
                raw.push((plain[y * stride + x] as i32 - pred) as u8);
            }
        }
        let mut z = vec![0x78, 0x01];
        let mut it = raw.chunks(60_000).peekable();
        if raw.is_empty() {
            z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
        }
        while let Some(c) = it.next() {
            z.push(if it.peek().is_none() { 1 } else { 0 });
            z.extend_from_slice(&(c.len() as u16).to_le_bytes());
            z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
            z.extend_from_slice(c);
        }
        z.extend_from_slice(&[0, 0, 0, 0]); // the check value is not looked at
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(w as u32).to_be_bytes());
        ihdr.extend_from_slice(&(h as u32).to_be_bytes());
        ihdr.extend_from_slice(&[depth, color, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &z);
        chunk(&mut png, b"IEND", &[]);
        png
    }

    #[test]
    fn a_16_bit_greyscale_picture_comes_back_sample_for_sample_through_every_filter() {
        let (w, h) = (7, 5);
        let samples: Vec<u16> = (0..w * h).map(|i| ((i * 2521) % 65536) as u16).collect();
        for filter in 0..=4u8 {
            let png = make_png(w, h, 16, 0, filter, &samples);
            let d = decode_png(&png).unwrap_or_else(|e| panic!("filter {filter}: {e}"));
            assert_eq!((d.cols, d.rows), (w, h));
            assert_eq!(d.samples, samples, "filter {filter}");
        }
    }

    #[test]
    fn eight_bit_and_colour_pictures_are_scaled_or_read_by_their_first_channel() {
        let g: Vec<u16> = vec![0, 128, 255, 7];
        let d = decode_png(&make_png(2, 2, 8, 0, 1, &g)).unwrap();
        assert_eq!(d.samples, vec![0, 128 * 257, 255 * 257, 7 * 257]);
        let rgb: Vec<u16> = vec![10, 99, 99, 20, 99, 99, 30, 99, 99, 40, 99, 99];
        let d = decode_png(&make_png(2, 2, 8, 2, 4, &rgb)).unwrap();
        assert_eq!(d.samples, vec![10 * 257, 20 * 257, 30 * 257, 40 * 257]);
    }

    #[test]
    fn damaged_and_unsupported_files_are_refused_with_words_and_never_panic() {
        assert!(decode_png(b"not a png at all").unwrap_err().contains("not a PNG"));
        let good = make_png(4, 4, 16, 0, 0, &vec![1u16; 16]);
        // every prefix of a good file, and every single flipped byte, must come back as an error or as a picture, never a panic
        for cut in 0..good.len() {
            let _ = decode_png(&good[..cut]);
        }
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 0xFF;
            let _ = decode_png(&bad);
        }
        let mut interlaced = good.clone();
        interlaced[8 + 8 + 12] = 1; // IHDR interlace byte
        assert!(decode_png(&interlaced).unwrap_err().contains("interlaced"));
        let palette = make_png(2, 2, 8, 0, 0, &[1, 2, 3, 4]);
        let mut p = palette.clone();
        p[8 + 8 + 9] = 3; // colour type 3
        assert!(decode_png(&p).unwrap_err().contains("palette"));
    }

    fn from_hex(h: &str) -> Vec<u8> {
        (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn inflate_reads_fixed_and_dynamic_blocks_and_stops_a_bomb() {
        // made by zlib: a fixed block with a match ("abc" four times)
        let fixed = from_hex("4b4c4a4e842100");
        let out = inflate(&fixed, 100).unwrap();
        assert_eq!(out, b"abcabcabcabc");
        assert!(inflate(&fixed, 5).unwrap_err().contains("more data"));
        // made by zlib: a block with its own code tables
        let text: String = (0..700).map(|i| format!("{}-{} ", i % 97, (i * i) % 13)).collect();
        let dynamic = from_hex(DYNAMIC);
        assert_eq!(dynamic[0] >> 1 & 3, 2, "it is a dynamic block");
        assert_eq!(inflate(&dynamic, 10_000).unwrap(), text.as_bytes());
        // too little room is an error, and so is every cut of the data
        assert!(inflate(&dynamic, 100).is_err());
        for cut in 0..dynamic.len() {
            let _ = inflate(&dynamic[..cut], 10_000);
        }
        assert!(inflate(&[], 10).is_err());
    }

    const DYNAMIC: &str = "4dd75bb2e3300845d1a97802a9c2801f9affc4a27d50887e6ea57114d92c8368fbd8717ecec33f79c4671cf989e3fa9c7edc9fd38e873f2fff1c337edafcc279ce6f9e3e979cc1dae4d345ece6eac3f7b4e21c2c76d3df9388fbbce8ece23917f8c5bef7fc117ff8f4121bf36ad8fc5e684938cb23f43715b9b8c85ef170cf73af23c6fc919c3b1dc9dda5f32031bf975a9217cb534f948f222f17d9ebb2b9e0220397cf1fb9824fdcdd7571f5261d5a72bd2cbff450b72943e7bc78b3d71d73c14d26ee6bfec87df389bbbb5fae92bb474b9e5359d5433da108097fd8ebb9e782874c3ceffc9167ee74bcdcdd7bceab2fb97bb5e44da9e8a1de5b11b2feb2d73be6824126c6397f64e034b8bb91f3ea2077434bc4a2bb7165957cf0c0dc34b7f091fbc1cffdd843ec8efb59f0863c11d234e907f42979e0cb1df66cf610bbe37e16bc214f8434057b0588c8035feeb067b387d81df7b3e00d79604953ea45061179e0cb1df66cf610bbe37e16bc214f84345dec7581883cf0e50e7b367b88dd713f0bde9027c22b76b3d70322f2c0973becd9ec2176c7fd2c78431e58b2feb2d70b22f2c0973becd9ec2176c7fd2c78439e08591fec3578c57806de0de5977c883d0ebd16bf829f6b054b9a4edebf1344e4812f77d8b3d943ecbe15bc214f843439db3888c8035feeb067b387d87d2b78439e08690af60a109107bedc61cf660fb1fb56f0863c11d274b1d70522f2c0973becd9ec2176df0ade90079657ec66af1b44e4812f77d8b3d943ecbe15bc214f848c3fecf580883cf0e50e7b367b88ddb78237e48990f597bd06af18f2c0973becd9ec71e8e5f815fcccaaf2a1663f1f845b60df8fdc61cf660fb1fb56f0863c91d5ec07f42979e0cb1df66cf610bb6f056fc803bb9afd803e250f7cb9c39ecd1e62f7ade00d7922abd90fe853f2c0973becd9ec2176df0ade9027b29afd803e250f7cb9c39ecd1e62f7ade00d7922abd90fe853f2c0973becd9ec2176df0ade90077635fb017d4a1ef872873d9b3dc4ee5bc11bf24456b31f873475c6cbfdd0f126f638ea455a056fc803bb9afd803efb8c2f77d8b3d943ecbe15bc214f6435fb017df6195feeb067b387d87d2b78439ec86af603faec33bedc61cf660fb1fb56f0863cb0abd90fe8b3cff872873d9b3dc4ee5bc11bf24456b31fd0679ff1e50e7b367b88ddb78237e489ac663fa0cf3ee3cb1df66cf610bb6f056fc81359cd7e409f7dc6973becf9638fa3e6ae55f0331f4a879afdbc0576d3195fee3dda893dc4ee5bc11bf24456b31fd0679ff1e5dea39dd843ecbe15bc210fec6af603faec33bedc7bb4137b88ddb78237e489ac663fa0cf3ee3cbbd473bb187d87d2b78439ec86af603faec33bedc7bb4137b88ddb78237e4815dcd7e409f7dc6977b8f76620fb1fb56f0863c91d5ec07f4d9677cb9f76827f610bb6f056fc813a9663f0d71d0192ff735dac11ebf917e15bc210fec6af603faec33bedc7bb4137bfc47fa55f0863c91d5ec07f4d9677cb9f76827f6f88ff4abe00d7922abd90fe8b3cff872efd14eecf11fe957c11bf2c0ae663fa0cf3ee3cbbd473bb1c77fa45f056fc81359cd7e409f7dc6977b8f76628fff48bf0ade9027b29afd803efb8c2ff71eedc41eff917e15bc210fd76af603faec33bedc7fa31decf11be9abe0edf7ff399afddc83b575c6977b8f76628fff48bf0adefeff9f53b31fc717";
}
