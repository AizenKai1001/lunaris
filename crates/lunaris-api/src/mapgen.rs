//! `generate_map`: make a level. Four generators, all integer-only and seeded, so a seed is a level on every machine:
//!
//! * `rooms` (default): the map is cut in two again and again (a binary space partition), a room is carved in every piece, and the two halves of every cut are joined by an
//!   L-shaped corridor, so every room can be reached from every other; `loops` extra corridors between random rooms make circuits to run round.
//! * `caves`: random rock, smoothed by "a cell is rock when five of the nine cells round it are", then only the biggest open space is kept.
//! * `maze`: a perfect maze (recursive backtracker) on a grid of odd cells, with `braid` percent of the dead ends opened into loops.
//! * `walk`: a drunkard's walk from the middle until `fill` percent of the map is open: winding caverns that cannot be cut apart.
//!
//! The result is a grid of floor and wall (`floor` 0 and `wall` 1 by default, so `autotile` can then draw the edges), a `start`, the `exit` farthest from it by
//! walking, spread-out `spawns` for enemies and items, and, with a `uid`, the `Tilemap` of that entity is rewritten (undoable) and the answer also says where the
//! cells are in the world. The simulation's own random numbers are not used, so the call never changes the state of a game it is not writing into.
use crate::engine::{hex, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_sim::components::{Tilemap, Transform};
use lunaris_sim::{Command, Rng};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Rooms,
    Caves,
    Maze,
    Walk,
}

impl Mode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Mode::Rooms => "rooms",
            Mode::Caves => "caves",
            Mode::Maze => "maze",
            Mode::Walk => "walk",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Params {
    pub mode: Mode,
    pub w: usize,
    pub h: usize,
    pub seed: u64,
    pub min_room: usize,
    pub max_room: usize,
    /// extra corridors between random rooms (None: a quarter of the number of rooms)
    pub loops: Option<usize>,
    /// percent of rock at the start (caves) or of the interior to open (walk)
    pub fill: u32,
    pub smooth: u32,
    /// percent of maze dead ends opened into loops
    pub braid: u32,
    pub points: usize,
    /// spawn points keep this many steps from the start
    pub clear: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Room {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Room {
    fn centre(&self) -> (usize, usize) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Level {
    pub w: usize,
    pub h: usize,
    pub open: Vec<bool>,
    pub rooms: Vec<Room>,
    pub start: (usize, usize),
    pub exit: (usize, usize),
    /// steps from the start to the exit
    pub distance: u32,
    pub reachable: usize,
    pub spawns: Vec<(usize, usize)>,
}

impl Level {
    pub(crate) fn floor_cells(&self) -> usize {
        self.open.iter().filter(|o| **o).count()
    }

    pub(crate) fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in (self.w as u64).to_le_bytes().into_iter().chain((self.h as u64).to_le_bytes()).chain(self.open.iter().map(|o| *o as u8)) {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

struct Grid {
    w: usize,
    h: usize,
    open: Vec<bool>,
}

impl Grid {
    fn new(w: usize, h: usize) -> Grid {
        Grid { w, h, open: vec![false; w * h] }
    }

    fn at(&self, x: usize, y: usize) -> bool {
        self.open[y * self.w + x]
    }

    fn carve(&mut self, x: usize, y: usize) {
        if x > 0 && y > 0 && x + 1 < self.w && y + 1 < self.h {
            self.open[y * self.w + x] = true;
        }
    }

    /// Open cells among the four neighbours.
    fn open_sides(&self, x: usize, y: usize) -> usize {
        let mut n = 0;
        if x > 0 && self.at(x - 1, y) {
            n += 1;
        }
        if x + 1 < self.w && self.at(x + 1, y) {
            n += 1;
        }
        if y > 0 && self.at(x, y - 1) {
            n += 1;
        }
        if y + 1 < self.h && self.at(x, y + 1) {
            n += 1;
        }
        n
    }

    /// An L-shaped corridor, going along first or down first by a coin toss.
    fn corridor(&mut self, rng: &mut Rng, a: (usize, usize), b: (usize, usize)) {
        let horizontal_first = rng.below(2) == 0;
        let (corner_x, corner_y) = if horizontal_first { (b.0, a.1) } else { (a.0, b.1) };
        for (p, q) in [(a, (corner_x, corner_y)), ((corner_x, corner_y), b)] {
            let (x0, x1) = (p.0.min(q.0), p.0.max(q.0));
            let (y0, y1) = (p.1.min(q.1), p.1.max(q.1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.carve(x, y);
                }
            }
        }
    }
}

/// The rooms of one piece of the partition (and its sub-pieces), as indexes into `rooms`.
fn partition(rng: &mut Rng, g: &mut Grid, rooms: &mut Vec<Room>, p: &Params, r: Rect, depth: u32) -> Vec<usize> {
    let need = p.min_room + 2;
    let can_cut_x = r.w >= 2 * need;
    let can_cut_y = r.h >= 2 * need;
    let big = r.w > p.max_room + 2 || r.h > p.max_room + 2;
    if depth < 12 && (can_cut_x || can_cut_y) && (big || rng.below(100) < 60) {
        let cut_x = if can_cut_x && can_cut_y {
            if r.w * 4 > r.h * 5 {
                true
            } else if r.h * 4 > r.w * 5 {
                false
            } else {
                rng.below(2) == 0
            }
        } else {
            can_cut_x
        };
        let (first, second) = if cut_x {
            let at = need + rng.below((r.w - 2 * need + 1) as u32) as usize;
            (Rect { w: at, ..r }, Rect { x: r.x + at, w: r.w - at, ..r })
        } else {
            let at = need + rng.below((r.h - 2 * need + 1) as u32) as usize;
            (Rect { h: at, ..r }, Rect { y: r.y + at, h: r.h - at, ..r })
        };
        let mut a = partition(rng, g, rooms, p, first, depth + 1);
        let b = partition(rng, g, rooms, p, second, depth + 1);
        let from = rooms[a[rng.below(a.len() as u32) as usize]].centre();
        let to = rooms[b[rng.below(b.len() as u32) as usize]].centre();
        g.corridor(rng, from, to);
        a.extend(b);
        return a;
    }
    // a leaf: one room with a wall's width of margin on every side
    let (room_w_max, room_h_max) = ((r.w - 2).min(p.max_room), (r.h - 2).min(p.max_room));
    let rw = p.min_room + rng.below((room_w_max - p.min_room + 1) as u32) as usize;
    let rh = p.min_room + rng.below((room_h_max - p.min_room + 1) as u32) as usize;
    let rx = r.x + 1 + rng.below((r.w - 2 - rw + 1) as u32) as usize;
    let ry = r.y + 1 + rng.below((r.h - 2 - rh + 1) as u32) as usize;
    for y in ry..ry + rh {
        for x in rx..rx + rw {
            g.carve(x, y);
        }
    }
    rooms.push(Room { x: rx, y: ry, w: rw, h: rh });
    vec![rooms.len() - 1]
}

fn make_rooms(rng: &mut Rng, g: &mut Grid, p: &Params) -> Vec<Room> {
    let mut rooms = Vec::new();
    partition(rng, g, &mut rooms, p, Rect { x: 0, y: 0, w: g.w, h: g.h }, 0);
    let loops = p.loops.unwrap_or(rooms.len() / 4).min(64);
    if rooms.len() >= 2 {
        for _ in 0..loops {
            let a = rng.below(rooms.len() as u32) as usize;
            let mut b = rng.below(rooms.len() as u32 - 1) as usize;
            if b >= a {
                b += 1;
            }
            let (from, to) = (rooms[a].centre(), rooms[b].centre());
            g.corridor(rng, from, to);
        }
    }
    rooms
}

/// Keep only the largest open region (four-way connected; the first found wins a tie).
fn keep_largest(g: &mut Grid) {
    let n = g.w * g.h;
    let mut seen = vec![false; n];
    let mut best: Vec<usize> = Vec::new();
    let mut stack = Vec::new();
    for s in 0..n {
        if !g.open[s] || seen[s] {
            continue;
        }
        let mut region = Vec::new();
        seen[s] = true;
        stack.push(s);
        while let Some(c) = stack.pop() {
            region.push(c);
            let (x, y) = (c % g.w, c / g.w);
            let mut next = |nx: usize, ny: usize| {
                let i = ny * g.w + nx;
                if g.open[i] && !seen[i] {
                    seen[i] = true;
                    stack.push(i);
                }
            };
            if x > 0 {
                next(x - 1, y);
            }
            if x + 1 < g.w {
                next(x + 1, y);
            }
            if y > 0 {
                next(x, y - 1);
            }
            if y + 1 < g.h {
                next(x, y + 1);
            }
        }
        if region.len() > best.len() {
            best = region;
        }
    }
    for o in g.open.iter_mut() {
        *o = false;
    }
    for c in best {
        g.open[c] = true;
    }
}

fn make_caves(rng: &mut Rng, g: &mut Grid, p: &Params) {
    let (w, h) = (g.w, g.h);
    // a roll of the dice can leave only crumbs of open ground: roll again (a few times) until a tenth of the inside is open
    let enough = ((w - 2) * (h - 2) / 10).max(9);
    for attempt in 0..8 {
        for o in g.open.iter_mut() {
            *o = false;
        }
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                g.open[y * w + x] = rng.below(100) >= p.fill;
            }
        }
        for _ in 0..p.smooth {
            let before = g.open.clone();
            for y in 1..h - 1 {
                for x in 1..w - 1 {
                    let mut rock = 0;
                    for dy in 0..3 {
                        for dx in 0..3 {
                            if !before[(y + dy - 1) * w + (x + dx - 1)] {
                                rock += 1;
                            }
                        }
                    }
                    g.open[y * w + x] = rock < 5;
                }
            }
        }
        keep_largest(g);
        if g.open.iter().filter(|o| **o).count() >= enough || attempt == 7 {
            break;
        }
    }
    if g.open.iter().filter(|o| **o).count() < 9 {
        // all rock: open the middle so there is something to stand on
        let (cx, cy) = (w / 2, h / 2);
        for y in cy - 1..=cy + 1 {
            for x in cx - 1..=cx + 1 {
                g.carve(x, y);
            }
        }
        keep_largest(g);
    }
}

fn make_maze(rng: &mut Rng, g: &mut Grid, p: &Params) {
    let (cw, ch) = ((g.w - 1) / 2, (g.h - 1) / 2);
    let mut visited = vec![false; cw * ch];
    let mut stack = vec![(0usize, 0usize)];
    visited[0] = true;
    g.carve(1, 1);
    while let Some(&(cx, cy)) = stack.last() {
        let mut options: Vec<(usize, usize)> = Vec::with_capacity(4);
        if cx > 0 && !visited[cy * cw + cx - 1] {
            options.push((cx - 1, cy));
        }
        if cx + 1 < cw && !visited[cy * cw + cx + 1] {
            options.push((cx + 1, cy));
        }
        if cy > 0 && !visited[(cy - 1) * cw + cx] {
            options.push((cx, cy - 1));
        }
        if cy + 1 < ch && !visited[(cy + 1) * cw + cx] {
            options.push((cx, cy + 1));
        }
        if options.is_empty() {
            stack.pop();
            continue;
        }
        let (nx, ny) = options[rng.below(options.len() as u32) as usize];
        visited[ny * cw + nx] = true;
        // the cell between the two, then the new cell
        g.carve(cx + nx + 1, cy + ny + 1);
        g.carve(2 * nx + 1, 2 * ny + 1);
        stack.push((nx, ny));
    }
    if p.braid > 0 {
        for y in 1..g.h - 1 {
            for x in 1..g.w - 1 {
                if !g.at(x, y) || g.open_sides(x, y) != 1 || rng.below(100) >= p.braid {
                    continue;
                }
                // a wall beside a dead end with open ground behind it (and not the map's border)
                let mut walls: Vec<(usize, usize)> = Vec::with_capacity(3);
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let (wx, wy) = ((x as i64 + dx) as usize, (y as i64 + dy) as usize);
                    let (bx, by) = (x as i64 + 2 * dx, y as i64 + 2 * dy);
                    if wx == 0 || wy == 0 || wx + 1 >= g.w || wy + 1 >= g.h || g.at(wx, wy) || bx < 1 || by < 1 || bx as usize + 1 >= g.w || by as usize + 1 >= g.h {
                        continue;
                    }
                    if g.at(bx as usize, by as usize) {
                        walls.push((wx, wy));
                    }
                }
                if !walls.is_empty() {
                    let (wx, wy) = walls[rng.below(walls.len() as u32) as usize];
                    g.carve(wx, wy);
                }
            }
        }
    }
}

fn make_walk(rng: &mut Rng, g: &mut Grid, p: &Params) {
    let interior = (g.w - 2) * (g.h - 2);
    let target = (interior * p.fill as usize / 100).clamp(1, interior);
    let (mut x, mut y) = (g.w / 2, g.h / 2);
    g.carve(x, y);
    let mut carved = 1usize;
    let mut steps = 0usize;
    while carved < target && steps < target * 60 {
        steps += 1;
        match rng.below(4) {
            0 if x > 1 => x -= 1,
            1 if x + 2 < g.w => x += 1,
            2 if y > 1 => y -= 1,
            3 if y + 2 < g.h => y += 1,
            _ => continue,
        }
        if !g.at(x, y) {
            g.carve(x, y);
            carved += 1;
        }
    }
}

/// Steps from `from` to every open cell it can walk to (u32::MAX for the rest).
fn distances(g: &Grid, from: (usize, usize)) -> Vec<u32> {
    let mut d = vec![u32::MAX; g.w * g.h];
    let mut queue = std::collections::VecDeque::new();
    d[from.1 * g.w + from.0] = 0;
    queue.push_back(from);
    while let Some((x, y)) = queue.pop_front() {
        let here = d[y * g.w + x];
        let mut try_cell = |nx: usize, ny: usize| {
            let i = ny * g.w + nx;
            if g.open[i] && d[i] == u32::MAX {
                d[i] = here + 1;
                queue.push_back((nx, ny));
            }
        };
        if x > 0 {
            try_cell(x - 1, y);
        }
        if x + 1 < g.w {
            try_cell(x + 1, y);
        }
        if y > 0 {
            try_cell(x, y - 1);
        }
        if y + 1 < g.h {
            try_cell(x, y + 1);
        }
    }
    d
}

/// Make a level. `p.w` and `p.h` are already checked by the caller (and odd for a maze).
pub(crate) fn generate(p: &Params) -> Level {
    let mut rng = Rng::new(p.seed ^ 0x6D61_7067_656E_0001);
    let mut g = Grid::new(p.w, p.h);
    let mut rooms = Vec::new();
    match p.mode {
        Mode::Rooms => rooms = make_rooms(&mut rng, &mut g, p),
        Mode::Caves => make_caves(&mut rng, &mut g, p),
        Mode::Maze => make_maze(&mut rng, &mut g, p),
        Mode::Walk => make_walk(&mut rng, &mut g, p),
    }
    let cells: Vec<usize> = (0..g.w * g.h).filter(|i| g.open[*i]).collect();
    let start = match p.mode {
        Mode::Rooms => rooms[0].centre(),
        Mode::Maze => (1, 1),
        _ => {
            let c = cells[rng.below(cells.len() as u32) as usize];
            (c % g.w, c / g.w)
        }
    };
    let dist = distances(&g, start);
    let mut exit_cell = start.1 * g.w + start.0;
    let mut reachable = 0usize;
    for &c in &cells {
        if dist[c] != u32::MAX {
            reachable += 1;
            if dist[c] > dist[exit_cell] {
                exit_cell = c;
            }
        }
    }
    let exit = (exit_cell % g.w, exit_cell / g.w);
    // spread-out points: each next one is the cell farthest from every point chosen so far, at least `clear` steps from the start
    let sq = |a: (usize, usize), b: (usize, usize)| -> u64 {
        let (dx, dy) = (a.0 as i64 - b.0 as i64, a.1 as i64 - b.1 as i64);
        (dx * dx + dy * dy) as u64
    };
    let mut nearest: Vec<u64> = cells.iter().map(|c| sq((c % g.w, c / g.w), start).min(sq((c % g.w, c / g.w), exit))).collect();
    let mut spawns = Vec::new();
    for _ in 0..p.points {
        let mut best: Option<usize> = None;
        for (k, &c) in cells.iter().enumerate() {
            if dist[c] == u32::MAX || dist[c] < p.clear || nearest[k] == 0 {
                continue;
            }
            if best.map_or(true, |b| nearest[k] > nearest[b]) {
                best = Some(k);
            }
        }
        let Some(k) = best else { break };
        let at = (cells[k] % g.w, cells[k] / g.w);
        spawns.push(at);
        for (j, &c) in cells.iter().enumerate() {
            nearest[j] = nearest[j].min(sq((c % g.w, c / g.w), at));
        }
    }
    Level { w: g.w, h: g.h, open: g.open, rooms, start, exit, distance: dist[exit_cell], reachable, spawns }
}

fn int_arg(a: &Args, key: &str, default: i64) -> Result<i64> {
    match a.0.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => v.as_i64().ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}` must be a whole number")).at(key)),
    }
}

fn ranged(a: &Args, key: &str, default: i64, lo: i64, hi: i64) -> Result<i64> {
    let v = int_arg(a, key, default)?;
    if v < lo || v > hi {
        return Err(EcsError::new("bad_value", format!("`{key}` is {v}, it must be from {lo} to {hi}")).at(key));
    }
    Ok(v)
}

impl Engine {
    pub(crate) fn generate_map_tool(&mut self, a: &Args) -> Result<Json> {
        let mode = match a.opt_str("mode")?.unwrap_or("rooms") {
            "rooms" => Mode::Rooms,
            "caves" => Mode::Caves,
            "maze" => Mode::Maze,
            "walk" => Mode::Walk,
            other => return Err(EcsError::new("bad_mode", format!("unknown mode `{other}`")).hint("modes: \"rooms\" (default), \"caves\", \"maze\", \"walk\"").at("mode")),
        };
        // the Tilemap to write into, and where it stands
        let target: Option<(u64, Tilemap, [f32; 3])> = match a.opt_u64("uid")? {
            None => None,
            Some(uid) => {
                let e = self.sim.world.entity(uid).ok_or_else(|| EcsError::new("no_such_entity", format!("no entity {uid}")).at("uid"))?;
                let map = *self.sim.world.get::<Tilemap>(e).ok_or_else(|| EcsError::new("no_tilemap", format!("entity {uid} has no Tilemap")).hint("add a Tilemap (size and tile), or leave out `uid` to only get the map back").at("uid"))?;
                let pos = self.sim.world.get::<Transform>(e).map(|t| t.pos).unwrap_or([0.0; 3]);
                Some((uid, map, pos))
            }
        };
        let (dw, dh) = match &target {
            Some((_, m, _)) if m.size[0] >= 8 && m.size[1] >= 8 => (m.size[0] as i64, m.size[1] as i64),
            _ => (48, 32),
        };
        let mut w = ranged(a, "width", dw, 8, 512)? as usize;
        let mut h = ranged(a, "height", dh, 8, 512)? as usize;
        if mode == Mode::Maze {
            // cells sit on odd columns and rows: an even size loses its last one
            w -= 1 - w % 2;
            h -= 1 - h % 2;
        }
        let min_room = ranged(a, "min_room", 4, 3, 64)? as usize;
        let max_room = ranged(a, "max_room", 10i64.max(min_room as i64), min_room as i64, 64)? as usize;
        if mode == Mode::Rooms && min_room + 2 > w.min(h) {
            return Err(EcsError::new("bad_size", format!("a room of {min_room} cells does not fit a map of {w} x {h}")).hint("make the map bigger or `min_room` smaller").at("min_room"));
        }
        let loops = match a.0.get("loops") {
            None | Some(Json::Null) => None,
            Some(_) => Some(ranged(a, "loops", 0, 0, 64)? as usize),
        };
        let params = Params {
            mode,
            w,
            h,
            seed: a.opt_u64("seed")?.unwrap_or(1),
            min_room,
            max_room,
            loops,
            fill: ranged(a, "fill", if mode == Mode::Walk { 40 } else { 45 }, 0, 100)? as u32,
            smooth: ranged(a, "smooth", 5, 0, 12)? as u32,
            braid: ranged(a, "braid", 0, 0, 100)? as u32,
            points: ranged(a, "points", 8, 0, 256)? as usize,
            clear: ranged(a, "clear", 6, 0, 100_000)? as u32,
        };
        let (floor, wall) = (int_arg(a, "floor", 0)?, int_arg(a, "wall", 1)?);
        let level = generate(&params);
        let rows: Vec<String> = level.open.chunks(level.w).map(|r| r.iter().map(|o| if *o { floor } else { wall }.to_string()).collect::<Vec<_>>().join(" ")).collect();
        let data = rows.join("\n");
        let cell = |c: (usize, usize)| Json::arr([Json::from(c.0), Json::from(c.1)]);
        let mut out = Json::obj([
            ("mode", Json::str(mode.name())),
            ("seed", Json::Int(params.seed as i64)),
            ("size", Json::arr([Json::from(level.w), Json::from(level.h)])),
            ("floor_cells", Json::from(level.floor_cells())),
            ("connected", Json::Bool(level.reachable == level.floor_cells())),
            ("start", cell(level.start)),
            ("exit", cell(level.exit)),
            ("distance", Json::from(level.distance)),
            ("spawns", Json::arr(level.spawns.iter().map(|c| cell(*c)))),
            ("rooms", Json::arr(level.rooms.iter().take(256).map(|r| Json::obj([("x", Json::from(r.x)), ("y", Json::from(r.y)), ("w", Json::from(r.w)), ("h", Json::from(r.h))])))),
            ("hash", hex(level.hash())),
        ]);
        match target {
            None => out.set("data", Json::str(data)),
            Some((uid, map, pos)) => {
                self.single(Command::Set { uid, component: "Tilemap".into(), value: Json::obj([("size", Json::arr([Json::from(level.w), Json::from(level.h)])), ("data", Json::str(data))]) })?;
                // where a cell's centre is in the world (a map that is not turned or scaled): the map is centred on the entity, the top row first
                let world = |c: (usize, usize)| {
                    let (w, h, t) = (level.w as f64, level.h as f64, map.tile as f64);
                    Json::arr([Json::Float(pos[0] as f64 + (c.0 as f64 + 0.5 - w / 2.0) * t), Json::Float(pos[1] as f64 + (h / 2.0 - c.1 as f64 - 0.5) * t)])
                };
                out.set("written", Json::Bool(true));
                out.set("start_world", world(level.start));
                out.set("exit_world", world(level.exit));
                out.set("spawns_world", Json::arr(level.spawns.iter().map(|c| world(*c))));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(mode: Mode, w: usize, h: usize, seed: u64) -> Params {
        Params { mode, w, h, seed, min_room: 4, max_room: 10, loops: None, fill: if mode == Mode::Walk { 40 } else { 45 }, smooth: 5, braid: 0, points: 8, clear: 6 }
    }

    fn border_is_wall(l: &Level) -> bool {
        (0..l.w).all(|x| !l.open[x] && !l.open[(l.h - 1) * l.w + x]) && (0..l.h).all(|y| !l.open[y * l.w] && !l.open[y * l.w + l.w - 1])
    }

    #[test]
    fn every_mode_makes_one_connected_level_inside_a_wall_of_rock() {
        for mode in [Mode::Rooms, Mode::Caves, Mode::Maze, Mode::Walk] {
            for (w, h) in [(48usize, 32usize), (9, 9), (31, 17), (80, 60)] {
                for seed in 1..=12u64 {
                    let l = generate(&params(mode, w, h, seed));
                    assert!(border_is_wall(&l), "{mode:?} {w}x{h} seed {seed}: the border must stay rock");
                    assert!(l.floor_cells() >= 9, "{mode:?} {w}x{h} seed {seed}: only {} open cells", l.floor_cells());
                    assert_eq!(l.reachable, l.floor_cells(), "{mode:?} {w}x{h} seed {seed}: some open cells cannot be reached from the start");
                    assert!(l.open[l.start.1 * l.w + l.start.0] && l.open[l.exit.1 * l.w + l.exit.0]);
                }
            }
        }
    }

    #[test]
    fn a_seed_is_a_level_and_other_seeds_are_other_levels() {
        for mode in [Mode::Rooms, Mode::Caves, Mode::Maze, Mode::Walk] {
            let a = generate(&params(mode, 48, 32, 7));
            let b = generate(&params(mode, 48, 32, 7));
            let c = generate(&params(mode, 48, 32, 8));
            assert_eq!((a.hash(), a.start, a.exit, &a.spawns), (b.hash(), b.start, b.exit, &b.spawns), "{mode:?}");
            assert_ne!(a.hash(), c.hash(), "{mode:?}: two seeds gave the same level");
        }
    }

    #[test]
    fn rooms_do_not_overlap_are_open_and_the_first_one_holds_the_start() {
        for seed in 1..=20u64 {
            let l = generate(&params(Mode::Rooms, 64, 40, seed));
            assert!(l.rooms.len() >= 3, "seed {seed}: {} rooms", l.rooms.len());
            for (i, r) in l.rooms.iter().enumerate() {
                assert!(r.w >= 4 && r.h >= 4 && r.w <= 10 && r.h <= 10, "{r:?}");
                assert!(r.x >= 1 && r.y >= 1 && r.x + r.w < l.w && r.y + r.h < l.h);
                for y in r.y..r.y + r.h {
                    for x in r.x..r.x + r.w {
                        assert!(l.open[y * l.w + x]);
                    }
                }
                for o in &l.rooms[i + 1..] {
                    let apart = r.x + r.w <= o.x || o.x + o.w <= r.x || r.y + r.h <= o.y || o.y + o.h <= r.y;
                    assert!(apart, "seed {seed}: rooms {r:?} and {o:?} overlap");
                }
            }
            let r = l.rooms[0];
            assert!(l.start.0 >= r.x && l.start.0 < r.x + r.w && l.start.1 >= r.y && l.start.1 < r.y + r.h);
        }
    }

    #[test]
    fn loops_add_corridors() {
        let none = Params { loops: Some(0), ..params(Mode::Rooms, 64, 40, 3) };
        let many = Params { loops: Some(12), ..none.clone() };
        assert!(generate(&many).floor_cells() > generate(&none).floor_cells());
    }

    #[test]
    fn a_maze_is_a_tree_unless_braided() {
        for seed in 1..=10u64 {
            let l = generate(&params(Mode::Maze, 31, 21, seed));
            let cells = ((l.w - 1) / 2) * ((l.h - 1) / 2);
            // every cell of the grid, and one passage for each but the first
            assert_eq!(l.floor_cells(), 2 * cells - 1, "seed {seed}");
            // no 2 x 2 open block: it would be a room, not a passage
            for y in 0..l.h - 1 {
                for x in 0..l.w - 1 {
                    let block = l.open[y * l.w + x] && l.open[y * l.w + x + 1] && l.open[(y + 1) * l.w + x] && l.open[(y + 1) * l.w + x + 1];
                    assert!(!block, "seed {seed}: open block at {x},{y}");
                }
            }
            let braided = generate(&Params { braid: 100, ..params(Mode::Maze, 31, 21, seed) });
            assert!(braided.floor_cells() > l.floor_cells(), "seed {seed}: braiding opened nothing");
            assert_eq!(braided.reachable, braided.floor_cells());
        }
    }

    #[test]
    fn caves_are_mostly_open_and_walks_reach_their_fill() {
        let c = generate(&params(Mode::Caves, 64, 40, 5));
        let share = c.floor_cells() * 100 / (64 * 40);
        assert!((15..=75).contains(&share), "{share}% open");
        let w = generate(&Params { fill: 30, ..params(Mode::Walk, 64, 40, 5) });
        assert_eq!(w.floor_cells(), 62 * 38 * 30 / 100, "a walk stops at the share it was asked for");
        let all_rock = generate(&Params { fill: 100, smooth: 0, ..params(Mode::Caves, 20, 20, 1) });
        assert!(all_rock.floor_cells() >= 9, "even a map of rock gets a place to stand");
    }

    #[test]
    fn exit_is_the_farthest_cell_and_spawns_are_spread_out_and_away_from_the_start() {
        for mode in [Mode::Rooms, Mode::Caves, Mode::Maze, Mode::Walk] {
            let l = generate(&params(mode, 48, 32, 4));
            let g = Grid { w: l.w, h: l.h, open: l.open.clone() };
            let d = distances(&g, l.start);
            assert_eq!(d[l.exit.1 * l.w + l.exit.0], l.distance);
            assert!(d.iter().filter(|x| **x != u32::MAX).all(|x| *x <= l.distance), "{mode:?}: a cell is farther than the exit");
            assert!(l.spawns.len() >= 3, "{mode:?}: {} spawns", l.spawns.len());
            for (i, s) in l.spawns.iter().enumerate() {
                assert!(l.open[s.1 * l.w + s.0]);
                assert!(d[s.1 * l.w + s.0] >= 6, "{mode:?}: spawn {s:?} is too near the start");
                assert!(*s != l.start && *s != l.exit);
                assert!(!l.spawns[..i].contains(s), "{mode:?}: a spawn point twice");
            }
        }
        // without points there are none
        assert!(generate(&Params { points: 0, ..params(Mode::Rooms, 48, 32, 4) }).spawns.is_empty());
    }

    fn j(s: &str) -> Json {
        lunaris_json::parse(s).unwrap()
    }

    #[test]
    fn the_tool_writes_the_tilemap_and_says_where_the_cells_are() {
        let mut e = crate::tests::engine();
        e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[10,20,0]},"Tilemap":{"size":[1,1],"tile":2.0}}}"#)).unwrap();
        let r = e.call("generate_map", &j(r#"{"uid":1,"mode":"rooms","seed":3,"width":40,"height":24}"#)).unwrap();
        assert_eq!(r.get("written").unwrap().as_bool(), Some(true));
        assert_eq!(r.get("connected").unwrap().as_bool(), Some(true));
        assert_eq!(r.get("size").unwrap().to_string_lossy(), "[40,24]");
        let map = e.call("get", &j(r#"{"uid":1,"components":["Tilemap"]}"#)).unwrap();
        let t = map.get("components").unwrap().get("Tilemap").unwrap();
        assert_eq!(t.get("size").unwrap().to_string_lossy(), "[40,24]");
        let rows: Vec<Vec<i64>> = t.get("data").unwrap().as_str().unwrap().lines().map(|l| l.split_whitespace().map(|n| n.parse().unwrap()).collect()).collect();
        assert_eq!((rows.len(), rows[0].len()), (24, 40));
        let floor = rows.iter().flatten().filter(|n| **n == 0).count() as u64;
        assert_eq!(floor, r.get("floor_cells").unwrap().as_u64().unwrap());
        assert!(rows.iter().flatten().all(|n| *n == 0 || *n == 1));
        // the start is open ground, and its world position is the cell's centre on a map centred on the entity (tiles 2 wide, top row first)
        let start = r.get("start").unwrap().as_array().unwrap();
        let (sx, sy) = (start[0].as_u64().unwrap() as usize, start[1].as_u64().unwrap() as usize);
        assert_eq!(rows[sy][sx], 0);
        let at = r.get("start_world").unwrap().as_array().unwrap();
        assert_eq!(at[0].as_f64().unwrap(), 10.0 + (sx as f64 + 0.5 - 20.0) * 2.0);
        assert_eq!(at[1].as_f64().unwrap(), 20.0 + (12.0 - sy as f64 - 0.5) * 2.0);
        assert_eq!(r.get("spawns_world").unwrap().as_array().unwrap().len(), r.get("spawns").unwrap().as_array().unwrap().len());
        // the same seed again gives the same level, another seed another; the answer's hash names it
        let again = e.call("generate_map", &j(r#"{"uid":1,"mode":"rooms","seed":3,"width":40,"height":24}"#)).unwrap();
        assert_eq!(again.get("hash").unwrap().as_str(), r.get("hash").unwrap().as_str());
        let other = e.call("generate_map", &j(r#"{"mode":"rooms","seed":4,"width":40,"height":24}"#)).unwrap();
        assert_ne!(other.get("hash").unwrap().as_str(), r.get("hash").unwrap().as_str());
        // without a uid nothing is written and the grid comes back, with the tile numbers asked for
        let plain = e.call("generate_map", &j(r#"{"mode":"maze","width":12,"height":10,"floor":-1,"wall":7}"#)).unwrap();
        assert_eq!(plain.get("size").unwrap().to_string_lossy(), "[11,9]", "a maze has an odd size");
        let data = plain.get("data").unwrap().as_str().unwrap();
        assert!(data.split_whitespace().all(|n| n == "-1" || n == "7") && data.lines().count() == 9);
        assert!(plain.get("written").is_none());
        // the Tilemap of the entity is only taken as the size when it is big enough to mean it
        let sized = e.call("generate_map", &j(r#"{"uid":1,"mode":"walk"}"#)).unwrap();
        assert_eq!(sized.get("size").unwrap().to_string_lossy(), "[40,24]");
        // undo puts the old map back
        e.call("undo", &j("{}")).unwrap();
        e.call("undo", &j("{}")).unwrap();
        e.call("undo", &j("{}")).unwrap();
        let back = e.call("get", &j(r#"{"uid":1,"components":["Tilemap"]}"#)).unwrap();
        assert_eq!(back.get("components").unwrap().get("Tilemap").unwrap().get("size").unwrap().to_string_lossy(), "[1,1]");
    }

    #[test]
    fn the_tool_refuses_what_it_cannot_make() {
        let mut e = crate::tests::engine();
        e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{}}}"#)).unwrap();
        assert_eq!(e.call("generate_map", &j(r#"{"mode":"x"}"#)).unwrap_err().code, "bad_mode");
        assert_eq!(e.call("generate_map", &j(r#"{"uid":99}"#)).unwrap_err().code, "no_such_entity");
        assert_eq!(e.call("generate_map", &j(r#"{"uid":2}"#)).unwrap_err().code, "no_tilemap");
        assert_eq!(e.call("generate_map", &j(r#"{"width":3}"#)).unwrap_err().code, "bad_value");
        assert_eq!(e.call("generate_map", &j(r#"{"width":"a"}"#)).unwrap_err().code, "type_mismatch");
        assert_eq!(e.call("generate_map", &j(r#"{"min_room":9,"max_room":4}"#)).unwrap_err().code, "bad_value");
        assert_eq!(e.call("generate_map", &j(r#"{"width":8,"height":8,"min_room":8}"#)).unwrap_err().code, "bad_size");
    }

    #[test]
    fn autotile_can_draw_the_edges_of_a_generated_cave() {
        let mut e = crate::tests::engine();
        e.call("spawn", &j(r#"{"uid":1,"components":{"Tilemap":{"size":[1,1]}}}"#)).unwrap();
        let r = e.call("generate_map", &j(r#"{"uid":1,"mode":"caves","seed":9,"width":40,"height":30}"#)).unwrap();
        let rock = 40 * 30 - r.get("floor_cells").unwrap().as_u64().unwrap();
        let t = e.call("autotile", &j(r#"{"uid":1,"base":100,"edge":"solid"}"#)).unwrap();
        assert_eq!(t.get("tiles_set").unwrap().as_u64(), Some(rock), "every rock cell becomes an edge tile");
    }

    #[test]
    fn the_levels_never_change_between_builds() {
        // integer-only generators: these numbers are the same on every machine, and change only when an algorithm does
        let pinned = [(Mode::Rooms, "2a7c7d5cda157f60"), (Mode::Caves, "2dca5c52a88aa81f"), (Mode::Maze, "df87da2e0a17817e"), (Mode::Walk, "b2ba5d84f072acbf")];
        for (mode, hash) in pinned {
            assert_eq!(format!("{:016x}", generate(&params(mode, 48, 32, 1)).hash()), hash, "{mode:?}");
        }
    }

    /// `cargo test -p lunaris-api show_levels -- --ignored --nocapture` draws one level of each kind as text (# rock, . floor, S start, E exit, o spawn).
    #[test]
    #[ignore]
    fn show_levels() {
        for mode in [Mode::Rooms, Mode::Caves, Mode::Maze, Mode::Walk] {
            let l = generate(&params(mode, 48, 32, 1));
            println!("{} ({} open, exit {} steps away, {} rooms)", mode.name(), l.floor_cells(), l.distance, l.rooms.len());
            for y in 0..l.h {
                let row: String = (0..l.w)
                    .map(|x| match (x, y) {
                        c if c == l.start => 'S',
                        c if c == l.exit => 'E',
                        c if l.spawns.contains(&c) => 'o',
                        _ if l.open[y * l.w + x] => '.',
                        _ => '#',
                    })
                    .collect();
                println!("{row}");
            }
        }
    }
}
