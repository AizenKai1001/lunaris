//! Bytes in and out: little-endian integers, LEB128 variable-length integers (zigzag for the signed ones) and length-prefixed byte strings.
//! Reading never panics: every read answers `None` when the bytes run out or are malformed, so a packet from the network cannot crash anything.

/// A writer: bytes appended to a vector.
#[derive(Default)]
pub struct W {
    pub buf: Vec<u8>,
}

impl W {
    pub fn new() -> W {
        W { buf: Vec::with_capacity(1200) }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn i8(&mut self, v: i8) {
        self.buf.push(v as u8);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// An unsigned integer in 1 to 10 bytes: 7 bits at a time, low first, the high bit of a byte says another follows.
    pub fn varu(&mut self, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(b);
                return;
            }
            self.buf.push(b | 0x80);
        }
    }

    /// A signed integer, zigzag coded (0, -1, 1, -2, ... become 0, 1, 2, 3, ...) so that small numbers of either sign are short.
    pub fn vari(&mut self, v: i64) {
        self.varu(((v << 1) ^ (v >> 63)) as u64);
    }

    pub fn raw(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// Bytes with their length in front.
    pub fn bytes(&mut self, b: &[u8]) {
        self.varu(b.len() as u64);
        self.buf.extend_from_slice(b);
    }
}

/// A reader over a slice.
pub struct R<'a> {
    b: &'a [u8],
    pub at: usize,
}

impl<'a> R<'a> {
    pub fn new(b: &'a [u8]) -> R<'a> {
        R { b, at: 0 }
    }

    pub fn left(&self) -> usize {
        self.b.len() - self.at
    }

    pub fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    pub fn i8(&mut self) -> Option<i8> {
        self.u8().map(|v| v as i8)
    }

    pub fn u16(&mut self) -> Option<u16> {
        let s = self.b.get(self.at..self.at + 2)?;
        self.at += 2;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }

    pub fn u32(&mut self) -> Option<u32> {
        let s = self.b.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    pub fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }

    pub fn varu(&mut self) -> Option<u64> {
        let mut v = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.u8()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    pub fn vari(&mut self) -> Option<i64> {
        let u = self.varu()?;
        Some(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    pub fn raw(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.varu()? as usize;
        self.raw(n)
    }
}
