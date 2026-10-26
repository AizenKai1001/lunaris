//! What the network knows about an entity: a small fixed record, quantised, that is cheap to send whole and idempotent to receive (a lost update
//! costs nothing but staleness: the next one says it all again, so there are no baselines to agree on and nothing to resynchronise).
//!
//! | field | units | bytes on the wire |
//! |---|---|---|
//! | `pos` | 1/64 m per unit (1.6 cm) | 3 to 4 each (zigzag varint) |
//! | `yaw` | 65536 = a full turn | 2 |
//! | `pitch` | 127 = 90 degrees up | 1 (full detail only) |
//! | `vel` | 1/32 m/s per unit | 1 to 2 each (full detail only) |
//! | `flags` | the game's bits (crouched, sliding, wall running, ...) | 1 to 2 |
//! | `health` | 0 to 255 | 1 |
//! | `anim`, `anim_t` | the clip's number and its time in 1/512 s | 1 to 2 + 2 (full detail only) |
//! | `extra` | a class's own number (the item's kind, the weapon held) | 1 to 2 (full detail only) |
//!
//! A player who is near gets the full record (about 30 bytes), one who is far the basic one (about 17). A thing that stays where it is (class `ITEM` or `WORLD`) is never sent in full; its basic record
//! carries its `extra` (what it is: the item's kind, the wall's piece) in one or two bytes more.

use super::bytes::{R, W};
use super::interest::is_static;

/// The units of a position: this many to the metre.
pub const POS_SCALE: f32 = 64.0;
/// The units of a velocity: this many to a metre a second.
pub const VEL_SCALE: f32 = 32.0;
/// The units of an animation time: this many to a second.
pub const ANIM_SCALE: f32 = 512.0;

/// The kinds of entity the network tells apart (the game may use more: a class is only a number the scheduler gives its own rate).
pub mod class {
    /// A player's character.
    pub const PLAYER: u8 = 0;
    /// Something lying on the ground to pick up.
    pub const ITEM: u8 = 1;
    /// Something in flight (a rocket, a grenade): updated every tick near it.
    pub const PROJECTILE: u8 = 2;
    /// A thing that stands in the world and sometimes changes (a door, a supply drop, the zone's centre).
    pub const WORLD: u8 = 3;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct EntityState {
    pub id: u32,
    pub class: u8,
    pub pos: [i32; 3],
    pub yaw: u16,
    pub pitch: i8,
    pub vel: [i16; 3],
    pub flags: u16,
    pub health: u8,
    pub anim: u16,
    pub anim_t: u16,
    pub extra: u16,
}

impl EntityState {
    pub fn pos_m(&self) -> [f32; 3] {
        [self.pos[0] as f32 / POS_SCALE, self.pos[1] as f32 / POS_SCALE, self.pos[2] as f32 / POS_SCALE]
    }

    pub fn set_pos_m(&mut self, p: [f32; 3]) {
        self.pos = [(p[0] * POS_SCALE).round() as i32, (p[1] * POS_SCALE).round() as i32, (p[2] * POS_SCALE).round() as i32];
    }

    pub fn vel_ms(&self) -> [f32; 3] {
        [self.vel[0] as f32 / VEL_SCALE, self.vel[1] as f32 / VEL_SCALE, self.vel[2] as f32 / VEL_SCALE]
    }

    pub fn set_vel_ms(&mut self, v: [f32; 3]) {
        let q = |x: f32| (x * VEL_SCALE).round().clamp(-32767.0, 32767.0) as i16;
        self.vel = [q(v[0]), q(v[1]), q(v[2])];
    }

    /// The facing in radians (0 to 2 pi).
    pub fn yaw_rad(&self) -> f32 {
        self.yaw as f32 * (std::f32::consts::TAU / 65536.0)
    }

    pub fn set_yaw_rad(&mut self, y: f32) {
        let t = y.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        self.yaw = ((t * 65536.0) as u32 & 0xffff) as u16;
    }

    /// The pitch in radians (-pi/2 down to pi/2 up).
    pub fn pitch_rad(&self) -> f32 {
        self.pitch as f32 * (std::f32::consts::FRAC_PI_2 / 127.0)
    }

    pub fn set_pitch_rad(&mut self, p: f32) {
        self.pitch = (p / (std::f32::consts::FRAC_PI_2 / 127.0)).round().clamp(-127.0, 127.0) as i8;
    }

    pub fn anim_time(&self) -> f32 {
        self.anim_t as f32 / ANIM_SCALE
    }

    pub fn set_anim_time(&mut self, t: f32) {
        self.anim_t = ((t * ANIM_SCALE) as u32 & 0xffff) as u16;
    }
}

/// How much of a record goes into a packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// Everything: what a near entity needs to be drawn and animated.
    Full,
    /// The place, the facing, the flags and the health: what a far one needs to be a dot on the horizon.
    Basic,
}

/// Writes a record. The id is first, then a byte of class and detail (the top bit is full detail).
pub fn write(w: &mut W, s: &EntityState, d: Detail) {
    w.varu(s.id as u64);
    w.u8(s.class & 0x7f | if d == Detail::Full { 0x80 } else { 0 });
    for a in 0..3 {
        w.vari(s.pos[a] as i64);
    }
    w.u16(s.yaw);
    w.varu(s.flags as u64);
    w.u8(s.health);
    if d == Detail::Full {
        w.i8(s.pitch);
        for a in 0..3 {
            w.vari(s.vel[a] as i64);
        }
        w.varu(s.anim as u64);
        w.u16(s.anim_t);
        w.varu(s.extra as u64);
    } else if is_static(s.class) {
        // (a thing that stays where it is is never sent in full, and what it is, its kind, is what a client needs to show it: one or two bytes)
        w.varu(s.extra as u64);
    }
}

/// Reads a record; the fields the detail leaves out come back as zero.
pub fn read(r: &mut R) -> Option<(EntityState, Detail)> {
    let id = u32::try_from(r.varu()?).ok()?;
    let cd = r.u8()?;
    let full = cd & 0x80 != 0;
    let mut s = EntityState { id, class: cd & 0x7f, ..Default::default() };
    for a in 0..3 {
        s.pos[a] = i32::try_from(r.vari()?).ok()?;
    }
    s.yaw = r.u16()?;
    s.flags = u16::try_from(r.varu()?).ok()?;
    s.health = r.u8()?;
    if full {
        s.pitch = r.i8()?;
        for a in 0..3 {
            s.vel[a] = i16::try_from(r.vari()?).ok()?;
        }
        s.anim = u16::try_from(r.varu()?).ok()?;
        s.anim_t = r.u16()?;
        s.extra = u16::try_from(r.varu()?).ok()?;
    } else if is_static(s.class) {
        s.extra = u16::try_from(r.varu()?).ok()?;
    }
    Some((s, if full { Detail::Full } else { Detail::Basic }))
}

/// What the client says about its own character each time (its id is the server's business, so it is not sent): the same fields as a full record.
pub fn write_own(w: &mut W, s: &EntityState) {
    let mut t = *s;
    t.id = 0;
    write(w, &t, Detail::Full);
}

/// Linear interpolation of two records: the place and the velocity blend, the facing goes the short way round, the rest is the later one's.
pub fn lerp(a: &EntityState, b: &EntityState, t: f32) -> EntityState {
    let t = t.clamp(0.0, 1.0);
    let mut out = *b;
    for i in 0..3 {
        out.pos[i] = a.pos[i] + ((b.pos[i] - a.pos[i]) as f32 * t).round() as i32;
        out.vel[i] = (a.vel[i] as f32 + (b.vel[i] as f32 - a.vel[i] as f32) * t).round() as i16;
    }
    let (ya, yb) = (a.yaw as i32, b.yaw as i32);
    let mut d = yb - ya;
    if d > 32768 {
        d -= 65536;
    } else if d < -32768 {
        d += 65536;
    }
    out.yaw = ((ya as f32 + d as f32 * t).round() as i32).rem_euclid(65536) as u16;
    out.pitch = (a.pitch as f32 + (b.pitch as f32 - a.pitch as f32) * t).round() as i8;
    out
}
