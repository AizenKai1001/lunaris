//! What the contact solver remembers from one tick to the next: for every pair of bodies that touched, the impulses its contact points ended up with. The
//! solver starts the next tick from them (a "warm start": applying them once before its rounds), which is what makes a stack of boxes stand still instead of
//! being worked out again from nothing every tick. This is **state**, not a cache: it changes what the next tick computes, so it is cloned with a `Sim`, kept in
//! a checkpoint and in a recording (which starts a replay from it), mixed into the state hash when there is any, and emptied by `Sim::clear`. It is not part of a
//! saved scene: a game loaded from a file starts its contacts cold, as the first tick of any scene does. A world without contacts between rich bodies has an
//! empty memory and the same hash as before.
//!
//! A contact point is matched to the one it was a tick ago by position: the old point (kept in the frame of the first body of the pair) must lie within
//! `MATCH_DIST` of the new one and have nearly the same normal (Box2D matches by feature id instead; a distance test in the body frame is the other common
//! way). The normal impulse is copied; the friction impulse is kept as a vector and projected onto the new tangent directions.
use crate::physics_rot::{dot, rotate, sub, unrotate, M3, V3};
use lunaris_ecs::StateHasher;
use lunaris_json::Json;

/// How far (metres) a contact point may have moved, in the frame of its first body, and still be the same point.
pub const MATCH_DIST: f32 = 0.02;
/// How alike the normals must be (the cosine of the angle between them).
pub const MATCH_COS: f32 = 0.9;

/// One contact point of one pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Where it was, in the frame of the first body of the pair (its position taken as the origin).
    pub local: V3,
    pub n: V3,
    /// The impulse along the normal that the solver ended with.
    pub jn: f32,
    /// The friction impulse (a vector in the world).
    pub f: V3,
}

/// The pair of bodies a run of points belongs to (`a` has the smaller uid).
#[derive(Clone, Copy, Debug, PartialEq)]
struct PairKey {
    a: u64,
    b: u64,
    start: u32,
    len: u32,
}

/// All the pairs that touched in the last tick, in uid order, with their points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactMemory {
    pairs: Vec<PairKey>,
    points: Vec<Point>,
}

impl ContactMemory {
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    pub fn clear(&mut self) {
        self.pairs.clear();
        self.points.clear();
    }

    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Starts the next pair (pairs must be added in uid order, the smaller uid first).
    pub fn begin_pair(&mut self, a: u64, b: u64) {
        self.pairs.push(PairKey { a, b, start: self.points.len() as u32, len: 0 });
    }

    /// Adds a point to the pair that was begun last.
    pub fn push_point(&mut self, p: Point) {
        self.points.push(p);
        if let Some(k) = self.pairs.last_mut() {
            k.len += 1;
        }
    }

    /// Takes back the pair that was begun last (it got no point).
    pub fn drop_empty_pair(&mut self) {
        if self.pairs.last().is_some_and(|k| k.len == 0) {
            self.pairs.pop();
        }
    }

    /// The points that the pair `(a, b)` had (`a` < `b`).
    pub fn find(&self, a: u64, b: u64) -> Option<&[Point]> {
        let i = self.pairs.binary_search_by(|p| (p.a, p.b).cmp(&(a, b))).ok()?;
        let k = self.pairs[i];
        Some(&self.points[k.start as usize..(k.start + k.len) as usize])
    }

    /// Splits the memory into the pairs for which `take(a, b)` is false and those for which it is true (both stay in uid order).
    pub fn partition(&self, take: impl Fn(u64, u64) -> bool) -> (ContactMemory, ContactMemory) {
        let (mut rest, mut taken) = (ContactMemory::default(), ContactMemory::default());
        for k in &self.pairs {
            let out = if take(k.a, k.b) { &mut taken } else { &mut rest };
            out.begin_pair(k.a, k.b);
            for p in &self.points[k.start as usize..(k.start + k.len) as usize] {
                out.push_point(*p);
            }
        }
        (rest, taken)
    }

    /// The pairs of both memories in uid order (a pair that is in both comes from `self`).
    pub fn merged(&self, other: &ContactMemory) -> ContactMemory {
        let key = |m: &ContactMemory, i: usize| (m.pairs[i].a, m.pairs[i].b);
        let mut out = ContactMemory::default();
        let (mut i, mut j) = (0, 0);
        while i < self.pairs.len() || j < other.pairs.len() {
            let from_self = j == other.pairs.len() || (i < self.pairs.len() && key(self, i) <= key(other, j));
            let (src, at) = if from_self { (self, i) } else { (other, j) };
            if from_self && j < other.pairs.len() && key(other, j) == key(self, i) {
                j += 1;
            }
            let k = src.pairs[at];
            out.begin_pair(k.a, k.b);
            for p in &src.points[k.start as usize..(k.start + k.len) as usize] {
                out.push_point(*p);
            }
            if from_self {
                i += 1;
            } else {
                j += 1;
            }
        }
        out
    }

    /// Adds the memory to a state hash.
    pub fn hash_into(&self, h: &mut StateHasher) {
        h.u64(self.pairs.len() as u64);
        for k in &self.pairs {
            h.u64(k.a);
            h.u64(k.b);
            h.u64(k.len as u64);
        }
        for q in &self.points {
            for v in [q.local, q.n, q.f] {
                for c in v {
                    h.u64(c.to_bits() as u64);
                }
            }
            h.u64(q.jn.to_bits() as u64);
        }
    }

    /// As JSON for a recording (numbers as hex bit patterns, so nothing is rounded): `[[a, b, [[lx, ly, lz, nx, ny, nz, jn, fx, fy, fz], ...]], ...]`.
    pub fn to_json(&self) -> Json {
        let bits = |x: f32| Json::str(format!("{:08x}", x.to_bits()));
        Json::arr(self.pairs.iter().map(|k| {
            let pts = self.points[k.start as usize..(k.start + k.len) as usize].iter().map(|p| {
                let all = [p.local[0], p.local[1], p.local[2], p.n[0], p.n[1], p.n[2], p.jn, p.f[0], p.f[1], p.f[2]];
                Json::arr(all.into_iter().map(bits))
            });
            Json::arr([Json::str(format!("{:016x}", k.a)), Json::str(format!("{:016x}", k.b)), Json::arr(pts)])
        }))
    }

    /// The inverse of `to_json`; `None` for anything that is not what it writes.
    pub fn from_json(j: &Json) -> Option<ContactMemory> {
        let mut m = ContactMemory::default();
        let hex64 = |v: &Json| v.as_str().and_then(|s| u64::from_str_radix(s, 16).ok());
        let hex32 = |v: &Json| v.as_str().and_then(|s| u32::from_str_radix(s, 16).ok()).map(f32::from_bits);
        for pair in j.as_array()? {
            let pair = pair.as_array()?;
            if pair.len() != 3 {
                return None;
            }
            let (a, b) = (hex64(&pair[0])?, hex64(&pair[1])?);
            if let Some(last) = m.pairs.last() {
                if (last.a, last.b) >= (a, b) {
                    return None;
                }
            }
            m.begin_pair(a, b);
            for p in pair[2].as_array()? {
                let v: Vec<f32> = p.as_array()?.iter().map(hex32).collect::<Option<_>>()?;
                if v.len() != 10 {
                    return None;
                }
                m.push_point(Point { local: [v[0], v[1], v[2]], n: [v[3], v[4], v[5]], jn: v[6], f: [v[7], v[8], v[9]] });
            }
            if m.pairs.last().is_some_and(|k| k.len == 0) {
                return None;
            }
        }
        Some(m)
    }
}

/// For the `count` new contact points of a pair (`new(i)`: the arm from the first body's centre to point `i`, and its normal), the old point each one
/// continues, if any, written to `out`: the closest old point within `MATCH_DIST` (in the frame of the first body, whose axes now are `axes`) with a normal
/// within `MATCH_COS`; an old point is used once. Ties go to the lower index, so the answer is the same everywhere. `used` is scratch space.
pub fn match_points(old: &[Point], axes: &M3, count: usize, new: impl Fn(usize) -> (V3, V3), used: &mut Vec<bool>, out: &mut Vec<Option<usize>>) {
    used.clear();
    used.resize(old.len(), false);
    out.clear();
    for i in 0..count {
        let (arm, n) = new(i);
        let mut best: Option<(f32, usize)> = None;
        for (k, o) in old.iter().enumerate() {
            if used[k] || dot(o.n, n) < MATCH_COS {
                continue;
            }
            let d = sub(rotate(axes, o.local), arm);
            let d2 = dot(d, d);
            if d2 <= MATCH_DIST * MATCH_DIST && best.is_none_or(|(b, _)| d2 < b) {
                best = Some((d2, k));
            }
        }
        out.push(best.map(|(_, k)| {
            used[k] = true;
            k
        }));
    }
}

/// The frame-of-the-first-body position to store for a contact point at the arm `arm` from that body's centre.
pub fn to_local(axes: &M3, arm: V3) -> V3 {
    unrotate(axes, arm)
}

#[cfg(test)]
mod tests {
    use super::*;

    const I: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    fn pt(local: V3) -> Point {
        Point { local, n: [0.0, 1.0, 0.0], jn: 1.0, f: [0.0; 3] }
    }

    fn matched(old: &[Point], axes: &M3, news: &[(V3, V3)]) -> Vec<Option<usize>> {
        let (mut used, mut out) = (Vec::new(), Vec::new());
        match_points(old, axes, news.len(), |i| news[i], &mut used, &mut out);
        out
    }

    #[test]
    fn a_point_is_matched_by_position_and_used_once() {
        let old = [pt([0.5, 0.0, 0.5]), pt([-0.5, 0.0, 0.5]), pt([0.5, 0.0, -0.5])];
        // the pair has moved a millimetre; one new point is at a corner that was not there
        let up = [0.0, 1.0, 0.0];
        let news = [([0.501, 0.0, 0.5], up), ([-0.499, 0.0, 0.5], up), ([0.5, 0.0, 0.5], up), ([2.0, 0.0, 0.0], up)];
        assert_eq!(matched(&old, &I, &news), vec![Some(0), Some(1), None, None], "the third new point is near an old point that the first one already has");
        // a different normal does not match
        assert_eq!(matched(&old, &I, &[([0.5, 0.0, 0.5], [1.0, 0.0, 0.0])]), vec![None]);
        // the first body has turned a quarter turn about y: the old corner (0.5, 0, 0.5) is now at (0.5, 0, -0.5)
        let turned: M3 = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
        assert_eq!(matched(&old, &turned, &[(rotate(&turned, [0.5, 0.0, 0.5]), up)]), vec![Some(0)]);
    }

    #[test]
    fn pairs_are_found_and_the_hash_follows_the_contents() {
        let mut a = ContactMemory::default();
        assert!(a.is_empty());
        a.begin_pair(1, 2);
        a.push_point(pt([0.0; 3]));
        a.push_point(pt([1.0, 0.0, 0.0]));
        a.begin_pair(1, 5);
        a.push_point(pt([0.0; 3]));
        assert_eq!(a.find(1, 2).map(|p| p.len()), Some(2));
        assert_eq!(a.find(1, 5).map(|p| p.len()), Some(1));
        assert!(a.find(1, 3).is_none() && a.find(0, 2).is_none());
        let hash = |m: &ContactMemory| {
            let mut h = StateHasher::default();
            m.hash_into(&mut h);
            h.finish()
        };
        let mut b = a.clone();
        assert_eq!(hash(&a), hash(&b));
        b.points[0].jn = 1.0001;
        assert_ne!(hash(&a), hash(&b));
        b.clear();
        assert!(b.is_empty() && b.find(1, 2).is_none());
    }

    #[test]
    fn json_round_trips_every_bit() {
        let mut a = ContactMemory::default();
        a.begin_pair(3, 9);
        a.push_point(Point { local: [0.1, -0.2, 0.30000001], n: [0.0, 1.0, 0.0], jn: 0.123456789, f: [1.0e-9, -2.5, 0.0] });
        a.begin_pair(4, 5);
        a.push_point(pt([1.0, 2.0, 3.0]));
        let text = a.to_json().to_string_compact().unwrap();
        let back = ContactMemory::from_json(&lunaris_json::parse(&text).unwrap()).unwrap();
        assert_eq!(a, back);
        assert_eq!(ContactMemory::from_json(&Json::arr([])).unwrap(), ContactMemory::default());
        assert!(ContactMemory::from_json(&Json::str("x")).is_none());
    }
}
