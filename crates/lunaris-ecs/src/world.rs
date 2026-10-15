//! The world: entities with stable uids, archetype tables of plain-data components, runtime component registration.
use crate::column::Column;
use crate::error::{EcsError, Result};
use crate::schema::*;
use crate::strings::StringTable;
use lunaris_json::Json;
use std::any::TypeId;
use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::ptr;
use std::sync::Arc;

const DEAD: u32 = u32::MAX;
const MAX_FIELDS: usize = 64;
const MAX_COMPONENT_BYTES: usize = 64 * 1024;

/// A contiguous range of rows in one archetype: the unit of parallel work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub arch: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entity {
    pub index: u32,
    pub generation: u32,
}

impl Entity {
    pub fn to_bits(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }

    pub fn from_bits(b: u64) -> Entity {
        Entity { index: b as u32, generation: (b >> 32) as u32 }
    }
}

#[derive(Default)]
struct UidHasher(u64);

impl Hasher for UidHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type UidMap = HashMap<u64, Entity, BuildHasherDefault<UidHasher>>;

#[derive(Clone, Copy, Debug)]
struct EntityMeta {
    generation: u32,
    arch: u32,
    row: u32,
    uid: u64,
}

#[derive(Clone)]
pub struct Archetype {
    pub(crate) components: Vec<u32>,
    pub(crate) columns: Vec<Column>,
    pub(crate) entities: Vec<Entity>,
    add_edges: BTreeMap<u32, u32>,
    remove_edges: BTreeMap<u32, u32>,
    /// Indices into `components`, ordered by component name (canonical order for hashing and scenes).
    name_order: Vec<usize>,
}

impl Archetype {
    #[inline]
    pub(crate) fn col_index(&self, cid: u32) -> Option<usize> {
        self.components.binary_search(&cid).ok()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Base pointer of the column holding `cid` (rows are contiguous, `size` bytes apart), if this archetype has it.
    pub fn column_base(&self, cid: ComponentId) -> Option<*mut u8> {
        self.col_index(cid.0).map(|i| self.columns[i].base())
    }

    pub fn component_ids(&self) -> impl Iterator<Item = ComponentId> + '_ {
        self.components.iter().map(|&c| ComponentId(c))
    }

    fn remove_row(&mut self, row: usize) -> Option<Entity> {
        for c in &mut self.columns {
            c.swap_remove(row);
        }
        let last = self.entities.len() - 1;
        self.entities.swap_remove(row);
        if row != last {
            Some(self.entities[row])
        } else {
            None
        }
    }
}

#[derive(Clone, Default)]
struct Registry {
    infos: Vec<ComponentInfo>,
    by_name: BTreeMap<String, u32>,
    by_type: HashMap<TypeId, u32>,
}

#[derive(Clone)]
pub struct World {
    /// The components that exist. Shared (copy on write) so that cloning a world, which rollback netcode does on every tick, does not copy
    /// the names and fields of the ~75 components: they change only when a component is registered.
    registry: Arc<Registry>,
    meta: Vec<EntityMeta>,
    free: Vec<u32>,
    archetypes: Vec<Archetype>,
    arch_index: BTreeMap<Vec<u32>, u32>,
    uids: UidMap,
    next_uid: u64,
    strings: StringTable,
    live: usize,
    /// Named global numbers (score, timers, input state). Part of the state: hashed, snapshotted, saved in scenes.
    resources: BTreeMap<String, f64>,
}

impl Default for World {
    fn default() -> Self {
        World::new()
    }
}

fn two_mut<T>(v: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert_ne!(a, b);
    if a < b {
        let (l, r) = v.split_at_mut(b);
        (&mut l[a], &mut r[0])
    } else {
        let (l, r) = v.split_at_mut(a);
        (&mut r[0], &mut l[b])
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

impl World {
    pub fn new() -> World {
        let mut w = World {
            registry: Arc::new(Registry::default()),
            meta: Vec::new(),
            free: Vec::new(),
            archetypes: Vec::new(),
            arch_index: BTreeMap::new(),
            uids: UidMap::default(),
            next_uid: 1,
            strings: StringTable::default(),
            live: 0,
            resources: BTreeMap::new(),
        };
        w.get_or_create_archetype(Vec::new());
        w
    }

    // ------------------------------------------------------------------------------------------ resources
    /// The value of a global, or `None` when it was never set.
    pub fn resource(&self, name: &str) -> Option<f64> {
        self.resources.get(name).copied()
    }

    pub fn resources(&self) -> &BTreeMap<String, f64> {
        &self.resources
    }

    /// Set (`Some`) or remove (`None`) a global; returns the previous value. The value must be finite.
    pub fn set_resource(&mut self, name: &str, value: Option<f64>) -> Result<Option<f64>> {
        if !valid_resource_name(name) {
            return Err(EcsError::new("bad_resource_name", format!("`{name}` is not a valid resource name")).hint("letters, digits and underscores in dot-separated parts, up to 64 characters, for example `score` or `input.space`"));
        }
        Ok(match value {
            Some(v) if !v.is_finite() => return Err(EcsError::new("bad_resource_value", format!("resource `{name}` must be a finite number"))),
            Some(v) => self.resources.insert(name.to_string(), v),
            None => self.resources.remove(name),
        })
    }

    // ------------------------------------------------------------------------------------------ registry
    fn finish_registration(&mut self, mut info: ComponentInfo, type_id: Option<TypeId>) -> Result<ComponentId> {
        if info.align > 8 {
            return Err(EcsError::new("unsupported_layout", format!("component {} needs alignment {}; the maximum is 8", info.name, info.align)));
        }
        let id = ComponentId(self.registry.infos.len() as u32);
        info.id = id;
        let registry = Arc::make_mut(&mut self.registry);
        registry.by_name.insert(info.name.clone(), id.0);
        if let Some(t) = type_id {
            registry.by_type.insert(t, id.0);
        }
        registry.infos.push(info);
        Ok(id)
    }

    pub fn register_native<T: Component>(&mut self) -> Result<ComponentId> {
        if let Some(&id) = self.registry.by_type.get(&TypeId::of::<T>()) {
            return Ok(ComponentId(id));
        }
        let spec = T::spec();
        if !valid_identifier(spec.name) {
            return Err(EcsError::new("bad_name", format!("`{}` is not a valid component name", spec.name)));
        }
        if let Some(&existing) = self.registry.by_name.get(spec.name) {
            return Err(EcsError::new("component_conflict", format!("a component named {} already exists (id {existing})", spec.name))
                .hint("choose a different name, or remove the dynamic definition"));
        }
        let fields: Vec<Field> = spec.fields.iter().map(|(n, ty, count, offset)| Field { name: n.to_string(), ty: *ty, count: *count, offset: *offset }).collect();
        let info = ComponentInfo {
            id: ComponentId(0),
            name: spec.name.to_string(),
            size: spec.size,
            align: spec.align,
            fields,
            doc: spec.doc.split_whitespace().collect::<Vec<_>>().join(" "),
            native: true,
            default_bytes: spec.default_bytes,
        };
        self.finish_registration(info, Some(TypeId::of::<T>()))
    }

    /// Define a component at runtime (from a script, a scene file, or an AI). Re-defining the same schema is a no-op.
    pub fn register_dynamic(&mut self, name: &str, doc: &str, specs: &[FieldSpec]) -> Result<ComponentId> {
        if !valid_identifier(name) || name.len() > 64 {
            return Err(EcsError::new("bad_name", format!("`{name}` is not a valid component name")).hint("use letters, digits and underscores, starting with a letter or underscore (max 64)"));
        }
        if specs.len() > MAX_FIELDS {
            return Err(EcsError::new("too_many_fields", format!("a component can have at most {MAX_FIELDS} fields")));
        }
        for (i, s) in specs.iter().enumerate() {
            if !valid_identifier(&s.name) {
                return Err(EcsError::new("bad_name", format!("`{}` is not a valid field name", s.name)).at(format!("{name}.{}", s.name)));
            }
            if specs[..i].iter().any(|o| o.name == s.name) {
                return Err(EcsError::new("duplicate_field", format!("field `{}` appears twice", s.name)).at(format!("{name}.{}", s.name)));
            }
            if s.count == 0 || s.count > 4096 {
                return Err(EcsError::new("bad_count", "array length must be between 1 and 4096").at(format!("{name}.{}", s.name)));
            }
        }
        let (fields, size, align) = compute_layout(specs);
        if size > MAX_COMPONENT_BYTES {
            return Err(EcsError::new("too_large", format!("component {name} would be {size} bytes; the maximum is {MAX_COMPONENT_BYTES}")));
        }
        if let Some(&existing) = self.registry.by_name.get(name) {
            let info = &self.registry.infos[existing as usize];
            let same = !info.native && info.fields.len() == fields.len() && info.fields.iter().zip(&fields).all(|(a, b)| a.name == b.name && a.ty == b.ty && a.count == b.count);
            return if same {
                Ok(info.id)
            } else {
                Err(EcsError::new("component_conflict", format!("component {name} already exists with a different schema")).hint("pick a new name; changing a schema in place is not supported"))
            };
        }
        let mut info = ComponentInfo { id: ComponentId(0), name: name.to_string(), size, align, fields, doc: doc.to_string(), native: false, default_bytes: Vec::new() };
        // default bytes: zero, then the declared defaults
        let mut buf = vec![0u64; size.div_ceil(8).max(1)];
        let defaults: Vec<(String, Json)> = specs.iter().filter_map(|s| s.default.clone().map(|d| (s.name.clone(), d))).collect();
        if !defaults.is_empty() {
            let mut scratch = StringTable::default();
            // SAFETY: buf holds at least `size` bytes, 8-aligned.
            unsafe { info.merge_json(&Json::Obj(defaults), buf.as_mut_ptr().cast::<u8>(), &mut scratch) }.map_err(|e| EcsError { hint: Some("check the `default` values of the field definitions".into()), ..e })?;
            if scratch.len() > 1 {
                return Err(EcsError::new("unsupported_default", "string defaults are not supported yet; set strings when adding the component"));
            }
        }
        // SAFETY: reading `size` bytes of a zero-initialised u64 buffer.
        info.default_bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), size) }.to_vec();
        self.finish_registration(info, None)
    }

    pub fn component_id(&self, name: &str) -> Option<ComponentId> {
        self.registry.by_name.get(name).map(|&i| ComponentId(i))
    }

    /// Like `component_id` but with an error that lists what exists and suggests the closest name.
    pub fn require_component(&self, name: &str) -> Result<ComponentId> {
        if let Some(id) = self.component_id(name) {
            return Ok(id);
        }
        let mut names: Vec<&String> = self.registry.by_name.keys().collect();
        names.sort_by_key(|n| levenshtein(n, name));
        let suggestion = names.first().filter(|n| levenshtein(n, name) <= 3.max(name.len() / 3)).map(|n| format!("did you mean `{n}`? "));
        let all: Vec<&str> = self.registry.by_name.keys().map(|s| s.as_str()).collect();
        Err(EcsError::new("unknown_component", format!("no component named `{name}`"))
            .hint(format!("{}known components: {}", suggestion.unwrap_or_default(), all.join(", ")))
            .at(name))
    }

    pub fn info(&self, id: ComponentId) -> &ComponentInfo {
        &self.registry.infos[id.0 as usize]
    }

    pub fn infos(&self) -> &[ComponentInfo] {
        &self.registry.infos
    }

    pub fn strings(&self) -> &StringTable {
        &self.strings
    }

    pub fn strings_mut(&mut self) -> &mut StringTable {
        &mut self.strings
    }

    // ------------------------------------------------------------------------------------------ archetypes
    fn get_or_create_archetype(&mut self, comps: Vec<u32>) -> u32 {
        if let Some(&i) = self.arch_index.get(&comps) {
            return i;
        }
        let columns = comps
            .iter()
            .map(|&c| {
                let info = &self.registry.infos[c as usize];
                Column::new(info.size, info.align.max(1))
            })
            .collect();
        let mut name_order: Vec<usize> = (0..comps.len()).collect();
        name_order.sort_by(|&a, &b| self.registry.infos[comps[a] as usize].name.cmp(&self.registry.infos[comps[b] as usize].name));
        let id = self.archetypes.len() as u32;
        self.archetypes.push(Archetype { components: comps.clone(), columns, entities: Vec::new(), add_edges: BTreeMap::new(), remove_edges: BTreeMap::new(), name_order });
        self.arch_index.insert(comps, id);
        id
    }

    fn arch_after_add(&mut self, from: u32, cid: u32) -> u32 {
        if let Some(&t) = self.archetypes[from as usize].add_edges.get(&cid) {
            return t;
        }
        let mut comps = self.archetypes[from as usize].components.clone();
        let pos = comps.binary_search(&cid).unwrap_err();
        comps.insert(pos, cid);
        let t = self.get_or_create_archetype(comps);
        self.archetypes[from as usize].add_edges.insert(cid, t);
        t
    }

    fn arch_after_remove(&mut self, from: u32, cid: u32) -> u32 {
        if let Some(&t) = self.archetypes[from as usize].remove_edges.get(&cid) {
            return t;
        }
        let mut comps = self.archetypes[from as usize].components.clone();
        let pos = comps.binary_search(&cid).expect("component present");
        comps.remove(pos);
        let t = self.get_or_create_archetype(comps);
        self.archetypes[from as usize].remove_edges.insert(cid, t);
        t
    }

    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    pub fn archetypes(&self) -> &[Archetype] {
        &self.archetypes
    }

    // ------------------------------------------------------------------------------------------ entities
    fn alloc_entity(&mut self, uid: u64) -> Entity {
        self.alloc_entity_in(uid, 0)
    }

    /// Create the entity record in archetype `arch` (the caller must then push one value into every column of it).
    fn alloc_entity_in(&mut self, uid: u64, arch: u32) -> Entity {
        let e = if let Some(index) = self.free.pop() {
            let generation = self.meta[index as usize].generation;
            Entity { index, generation }
        } else {
            self.meta.push(EntityMeta { generation: 0, arch: DEAD, row: 0, uid: 0 });
            Entity { index: (self.meta.len() - 1) as u32, generation: 0 }
        };
        let a = &mut self.archetypes[arch as usize];
        let row = a.entities.len() as u32;
        a.entities.push(e);
        self.meta[e.index as usize] = EntityMeta { generation: e.generation, arch, row, uid };
        self.uids.insert(uid, e);
        self.live += 1;
        e
    }

    pub fn spawn(&mut self) -> Entity {
        let uid = self.next_uid;
        self.next_uid += 1;
        self.alloc_entity(uid)
    }

    pub fn spawn_uid(&mut self, uid: u64) -> Result<Entity> {
        if uid == 0 {
            return Err(EcsError::new("bad_uid", "uid 0 is reserved for \"no entity\"").hint("omit the uid to get the next free one"));
        }
        if self.uids.contains_key(&uid) {
            return Err(EcsError::new("duplicate_uid", format!("an entity with uid {uid} already exists")));
        }
        self.next_uid = self.next_uid.max(uid + 1);
        Ok(self.alloc_entity(uid))
    }

    /// Create an entity that already has all the given components, directly in its final archetype.
    ///
    /// # Safety
    /// Each pointer must reference a valid value of its component (`info.size` bytes); component ids must be distinct.
    pub unsafe fn spawn_raw(&mut self, uid: Option<u64>, items: &[(ComponentId, *const u8)]) -> Result<Entity> {
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by_key(|&i| items[i].0 .0);
        if order.windows(2).any(|w| items[w[0]].0 == items[w[1]].0) {
            return Err(EcsError::new("duplicate_component", "a component appears twice in the spawn"));
        }
        let uid = match uid {
            Some(0) => return Err(EcsError::new("bad_uid", "uid 0 is reserved for \"no entity\"").hint("omit the uid to get the next free one")),
            Some(u) => {
                if self.uids.contains_key(&u) {
                    return Err(EcsError::new("duplicate_uid", format!("an entity with uid {u} already exists")));
                }
                self.next_uid = self.next_uid.max(u + 1);
                u
            }
            None => {
                let u = self.next_uid;
                self.next_uid += 1;
                u
            }
        };
        let comps: Vec<u32> = order.iter().map(|&i| items[i].0 .0).collect();
        let arch = self.get_or_create_archetype(comps);
        let e = self.alloc_entity_in(uid, arch);
        let a = &mut self.archetypes[arch as usize];
        for (k, &i) in order.iter().enumerate() {
            let size = a.columns[k].elem_size();
            // SAFETY: caller contract; zero-sized components copy nothing.
            unsafe { a.columns[k].push_copy(if size == 0 { ptr::NonNull::<u8>::dangling().as_ptr() } else { items[i].1 }) };
        }
        Ok(e)
    }

    /// Create an entity with components given as JSON (missing fields take defaults). Validates everything first, so
    /// a failure leaves the world untouched.
    pub fn spawn_json(&mut self, uid: Option<u64>, comps: &[(ComponentId, &Json)]) -> Result<Entity> {
        let mut bufs: Vec<Vec<u64>> = Vec::with_capacity(comps.len());
        for (cid, json) in comps {
            let info = &self.registry.infos[cid.0 as usize];
            let mut buf = vec![0u64; info.size.div_ceil(8).max(1)];
            let bp = buf.as_mut_ptr().cast::<u8>();
            // SAFETY: buf is 8-aligned with at least `size` bytes; default_bytes has exactly `size` bytes.
            unsafe {
                ptr::copy_nonoverlapping(info.default_bytes.as_ptr(), bp, info.size);
                info.merge_json(json, bp, &mut self.strings)?;
            }
            bufs.push(buf);
        }
        let items: Vec<(ComponentId, *const u8)> = comps.iter().zip(&bufs).map(|((c, _), b)| (*c, b.as_ptr().cast::<u8>())).collect();
        // SAFETY: every pointer references a fully initialised buffer for its component.
        unsafe { self.spawn_raw(uid, &items) }
    }

    pub fn spawn_with<A: Component>(&mut self, a: A) -> Result<Entity> {
        let ca = self.register_native::<A>()?;
        // SAFETY: pointers reference live values of the registered types.
        unsafe { self.spawn_raw(None, &[(ca, (&a as *const A).cast())]) }
    }

    pub fn spawn_with2<A: Component, B: Component>(&mut self, a: A, b: B) -> Result<Entity> {
        let (ca, cb) = (self.register_native::<A>()?, self.register_native::<B>()?);
        // SAFETY: pointers reference live values of the registered types.
        unsafe { self.spawn_raw(None, &[(ca, (&a as *const A).cast()), (cb, (&b as *const B).cast())]) }
    }

    pub fn spawn_with3<A: Component, B: Component, C: Component>(&mut self, a: A, b: B, c: C) -> Result<Entity> {
        let (ca, cb, cc) = (self.register_native::<A>()?, self.register_native::<B>()?, self.register_native::<C>()?);
        // SAFETY: pointers reference live values of the registered types.
        unsafe { self.spawn_raw(None, &[(ca, (&a as *const A).cast()), (cb, (&b as *const B).cast()), (cc, (&c as *const C).cast())]) }
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        self.meta.get(e.index as usize).is_some_and(|m| m.arch != DEAD && m.generation == e.generation)
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    pub fn next_uid(&self) -> u64 {
        self.next_uid
    }

    pub fn set_next_uid(&mut self, uid: u64) {
        self.next_uid = self.next_uid.max(uid);
    }

    pub fn uid_of(&self, e: Entity) -> Option<u64> {
        self.is_alive(e).then(|| self.meta[e.index as usize].uid)
    }

    /// Where an entity's components live: (archetype index, row). `None` for a dead handle.
    pub fn locate(&self, e: Entity) -> Option<(usize, usize)> {
        self.is_alive(e).then(|| {
            let m = self.meta[e.index as usize];
            (m.arch as usize, m.row as usize)
        })
    }

    pub fn entity(&self, uid: u64) -> Option<Entity> {
        self.uids.get(&uid).copied()
    }

    pub fn require_entity(&self, uid: u64) -> Result<Entity> {
        self.entity(uid).ok_or_else(|| EcsError::new("no_such_entity", format!("no entity with uid {uid}")).hint("use the `query` tool to list entities and their uids"))
    }

    pub fn despawn(&mut self, e: Entity) -> bool {
        if !self.is_alive(e) {
            return false;
        }
        let m = self.meta[e.index as usize];
        let moved = self.archetypes[m.arch as usize].remove_row(m.row as usize);
        if let Some(moved) = moved {
            self.meta[moved.index as usize].row = m.row;
        }
        self.uids.remove(&m.uid);
        let slot = &mut self.meta[e.index as usize];
        slot.arch = DEAD;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(e.index);
        self.live -= 1;
        true
    }

    /// Entities in ascending uid order (the canonical order used by scenes and hashes).
    pub fn entities_by_uid(&self) -> Vec<Entity> {
        let mut v: Vec<(u64, Entity)> = Vec::with_capacity(self.live);
        for (i, m) in self.meta.iter().enumerate() {
            if m.arch != DEAD {
                v.push((m.uid, Entity { index: i as u32, generation: m.generation }));
            }
        }
        v.sort_unstable_by_key(|x| x.0);
        v.into_iter().map(|x| x.1).collect()
    }

    // ------------------------------------------------------------------------------------------ components (raw)
    pub fn has(&self, e: Entity, cid: ComponentId) -> bool {
        self.is_alive(e) && self.archetypes[self.meta[e.index as usize].arch as usize].col_index(cid.0).is_some()
    }

    /// Pointer to the component bytes of `e`, if present.
    pub fn component_ptr(&self, e: Entity, cid: ComponentId) -> Option<*mut u8> {
        if !self.is_alive(e) {
            return None;
        }
        let m = self.meta[e.index as usize];
        let a = &self.archetypes[m.arch as usize];
        a.col_index(cid.0).map(|ci| a.columns[ci].ptr_at(m.row as usize))
    }

    /// Add `cid` to `e` with the given bytes (or its default), or overwrite it when already present.
    ///
    /// # Safety
    /// When `src` is `Some`, it must point to `info.size` bytes of a valid value of the component.
    pub unsafe fn insert_raw(&mut self, e: Entity, cid: ComponentId, src: Option<*const u8>) -> Result<()> {
        if !self.is_alive(e) {
            return Err(EcsError::new("dead_entity", "that entity no longer exists"));
        }
        let size = self.registry.infos[cid.0 as usize].size;
        let m = self.meta[e.index as usize];
        if let Some(p) = self.component_ptr(e, cid) {
            if let Some(s) = src {
                // SAFETY: both point to `size` valid bytes; the source is caller-provided and distinct from the column row.
                unsafe { ptr::copy(s, p, size) };
            }
            return Ok(());
        }
        let to = self.arch_after_add(m.arch, cid.0);
        // the default bytes live in the registry; copy them out so the borrow ends before the move
        let default_bytes: Vec<u8>;
        let data: *const u8 = match src {
            Some(s) => s,
            None => {
                default_bytes = self.registry.infos[cid.0 as usize].default_bytes.clone();
                default_bytes.as_ptr()
            }
        };
        let (from_a, to_a) = two_mut(&mut self.archetypes, m.arch as usize, to as usize);
        for (k, &c) in to_a.components.iter().enumerate() {
            if c == cid.0 {
                // SAFETY: data points to `size` bytes (caller contract or default bytes of exactly this size).
                unsafe { to_a.columns[k].push_copy(if size == 0 { ptr::NonNull::<u8>::dangling().as_ptr() } else { data }) };
            } else {
                let fi = from_a.col_index(c).expect("source has every other component");
                // SAFETY: the row exists in the source column.
                unsafe { to_a.columns[k].push_copy(from_a.columns[fi].ptr_at(m.row as usize)) };
            }
        }
        to_a.entities.push(e);
        let new_row = to_a.entities.len() as u32 - 1;
        let moved = from_a.remove_row(m.row as usize);
        if let Some(moved) = moved {
            self.meta[moved.index as usize].row = m.row;
        }
        let slot = &mut self.meta[e.index as usize];
        slot.arch = to;
        slot.row = new_row;
        Ok(())
    }

    pub fn remove(&mut self, e: Entity, cid: ComponentId) -> Result<bool> {
        if !self.is_alive(e) {
            return Err(EcsError::new("dead_entity", "that entity no longer exists"));
        }
        let m = self.meta[e.index as usize];
        if self.archetypes[m.arch as usize].col_index(cid.0).is_none() {
            return Ok(false);
        }
        let to = self.arch_after_remove(m.arch, cid.0);
        let (from_a, to_a) = two_mut(&mut self.archetypes, m.arch as usize, to as usize);
        for (k, &c) in to_a.components.iter().enumerate() {
            let fi = from_a.col_index(c).expect("source has every remaining component");
            // SAFETY: the row exists in the source column.
            unsafe { to_a.columns[k].push_copy(from_a.columns[fi].ptr_at(m.row as usize)) };
        }
        to_a.entities.push(e);
        let new_row = to_a.entities.len() as u32 - 1;
        let moved = from_a.remove_row(m.row as usize);
        if let Some(moved) = moved {
            self.meta[moved.index as usize].row = m.row;
        }
        let slot = &mut self.meta[e.index as usize];
        slot.arch = to;
        slot.row = new_row;
        Ok(true)
    }

    // ------------------------------------------------------------------------------------------ components (JSON)
    /// Add the component with defaults merged with `json`, or merge `json` into the existing value.
    pub fn insert_json(&mut self, e: Entity, cid: ComponentId, json: &Json) -> Result<()> {
        if !self.is_alive(e) {
            return Err(EcsError::new("dead_entity", "that entity no longer exists"));
        }
        let existing = self.component_ptr(e, cid);
        // `info` borrows the registry and `merge_json` the string table: different fields, so no copy of the component's description
        // is needed (that copy was most of the cost of loading a scene)
        let info = &self.registry.infos[cid.0 as usize];
        if let Some(p) = existing {
            // SAFETY: p points to the live component row.
            return unsafe { info.merge_json(json, p, &mut self.strings) };
        }
        let mut buf = vec![0u64; info.size.div_ceil(8).max(1)];
        let bp = buf.as_mut_ptr().cast::<u8>();
        // SAFETY: buf is 8-aligned with at least `size` bytes; default_bytes has exactly `size` bytes.
        unsafe {
            ptr::copy_nonoverlapping(info.default_bytes.as_ptr(), bp, info.size);
            info.merge_json(json, bp, &mut self.strings)?;
            self.insert_raw(e, cid, Some(bp))
        }
    }

    /// Merge `json` into an existing component.
    pub fn set_json(&mut self, e: Entity, cid: ComponentId, json: &Json) -> Result<()> {
        if !self.has(e, cid) {
            let name = self.registry.infos[cid.0 as usize].name.clone();
            return Err(EcsError::new("missing_component", format!("entity has no {name} component")).hint(format!("use add_component to add {name} first")).at(name));
        }
        self.insert_json(e, cid, json)
    }

    pub fn get_json(&self, e: Entity, cid: ComponentId) -> Option<Json> {
        let p = self.component_ptr(e, cid)?;
        // SAFETY: p points to the live component row.
        Some(unsafe { self.registry.infos[cid.0 as usize].read_json(p, &self.strings) })
    }

    /// Components of `e`, in name order.
    pub fn components_of(&self, e: Entity) -> Vec<ComponentId> {
        if !self.is_alive(e) {
            return Vec::new();
        }
        let a = &self.archetypes[self.meta[e.index as usize].arch as usize];
        a.name_order.iter().map(|&i| ComponentId(a.components[i])).collect()
    }

    // ------------------------------------------------------------------------------------------ components (typed)
    pub fn insert<T: Component>(&mut self, e: Entity, value: T) -> Result<()> {
        let cid = self.register_native::<T>()?;
        // SAFETY: &value points to size_of::<T>() valid bytes, and the registered size equals size_of::<T>().
        unsafe { self.insert_raw(e, cid, Some((&value as *const T).cast::<u8>())) }
    }

    pub fn get<T: Component>(&self, e: Entity) -> Option<&T> {
        let cid = *self.registry.by_type.get(&TypeId::of::<T>())?;
        // SAFETY: the column stores T values at T's alignment (registered from T itself).
        self.component_ptr(e, ComponentId(cid)).map(|p| unsafe { &*p.cast::<T>() })
    }

    pub fn get_mut<T: Component>(&mut self, e: Entity) -> Option<&mut T> {
        let cid = *self.registry.by_type.get(&TypeId::of::<T>())?;
        // SAFETY: as in `get`; &mut self guarantees exclusive access.
        self.component_ptr(e, ComponentId(cid)).map(|p| unsafe { &mut *p.cast::<T>() })
    }

    pub fn remove_typed<T: Component>(&mut self, e: Entity) -> Result<bool> {
        let cid = self.register_native::<T>()?;
        self.remove(e, cid)
    }

    // ------------------------------------------------------------------------------------------ queries
    /// Archetypes that contain every `with` component and none of the `without` ones.
    pub fn matching_archetypes(&self, with: &[ComponentId], without: &[ComponentId]) -> Vec<usize> {
        self.archetypes
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.entities.is_empty() && with.iter().all(|c| a.col_index(c.0).is_some()) && without.iter().all(|c| a.col_index(c.0).is_none()))
            .map(|(i, _)| i)
            .collect()
    }

    /// Run `f` for every entity that has all the terms of `Q`.
    pub fn for_each<Q: Query, F>(&mut self, mut f: F) -> Result<()>
    where
        F: for<'w> FnMut(Entity, Q::Item<'w>),
    {
        self.query_impl::<Q, _>(|e, _uid, item| f(e, item))
    }

    /// Like `for_each`, but also passes the entity's stable uid (useful for order-independent per-entity randomness).
    pub fn for_each_uid<Q: Query, F>(&mut self, f: F) -> Result<()>
    where
        F: for<'w> FnMut(Entity, u64, Q::Item<'w>),
    {
        self.query_impl::<Q, _>(f)
    }

    fn query_impl<Q: Query, F>(&mut self, mut f: F) -> Result<()>
    where
        F: for<'w> FnMut(Entity, u64, Q::Item<'w>),
    {
        let terms = Q::terms(self)?;
        for i in 0..terms.len() {
            for j in i + 1..terms.len() {
                if terms[i].0 == terms[j].0 && (terms[i].1 || terms[j].1) {
                    return Err(EcsError::new("aliasing_query", "a query cannot fetch the same component twice when one access is mutable"));
                }
            }
        }
        let ids: Vec<u32> = terms.iter().map(|t| (t.0).0).collect();
        let mut bases: Vec<*mut u8> = vec![ptr::null_mut(); ids.len()];
        for a in &self.archetypes {
            if a.entities.is_empty() {
                continue;
            }
            let mut ok = true;
            for (k, id) in ids.iter().enumerate() {
                match a.col_index(*id) {
                    Some(ci) => bases[k] = a.columns[ci].base(),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            for row in 0..a.entities.len() {
                // SAFETY: bases point at columns of this archetype with at least `entities.len()` rows; aliasing was checked above.
                let item = unsafe { Q::fetch(&bases, row) };
                let e = a.entities[row];
                f(e, self.meta[e.index as usize].uid, item);
            }
        }
        Ok(())
    }

    /// Split the rows of every archetype matching the query into chunks of at most `chunk_rows` rows. Chunks never overlap, so
    /// they can be processed on different threads; the list is in archetype order, so it is the same on every run.
    pub fn chunks(&self, with: &[ComponentId], without: &[ComponentId], chunk_rows: usize) -> Vec<Chunk> {
        let step = chunk_rows.max(1);
        let mut out = Vec::new();
        for ai in self.matching_archetypes(with, without) {
            let n = self.archetypes[ai].entities.len();
            let mut start = 0;
            while start < n {
                let end = (start + step).min(n);
                out.push(Chunk { arch: ai, start, end });
                start = end;
            }
        }
        out
    }

    /// Like `for_each_uid`, but spreads chunks of rows over `threads` threads. The closure must not depend on the order
    /// in which entities are visited (every Lunaris system is already written that way).
    pub fn par_for_each_uid<Q: Query, F>(&mut self, threads: usize, f: F) -> Result<()>
    where
        F: for<'w> Fn(Entity, u64, Q::Item<'w>) + Sync,
    {
        let terms = Q::terms(self)?;
        for i in 0..terms.len() {
            for j in i + 1..terms.len() {
                if terms[i].0 == terms[j].0 && (terms[i].1 || terms[j].1) {
                    return Err(EcsError::new("aliasing_query", "a query cannot fetch the same component twice when one access is mutable"));
                }
            }
        }
        let ids: Vec<ComponentId> = terms.iter().map(|t| t.0).collect();
        let total: usize = self.archetypes.iter().map(|a| a.entities.len()).sum();
        if threads <= 1 || total < 4096 {
            return self.query_impl::<Q, _>(|e, uid, item| f(e, uid, item));
        }
        let chunks = self.chunks(&ids, &[], (total / (threads * 4)).clamp(1024, 65_536));
        // column bases per archetype, as integers so they can cross threads (they are only turned back into pointers inside)
        let bases: Vec<Vec<usize>> = self
            .archetypes
            .iter()
            .map(|a| ids.iter().map(|&c| a.column_base(c).map(|p| p as usize).unwrap_or(0)).collect())
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let (archetypes, meta) = (&self.archetypes, &self.meta);
        std::thread::scope(|scope| {
            for _ in 0..threads.min(chunks.len()) {
                scope.spawn(|| {
                    let mut ptrs: Vec<*mut u8> = vec![ptr::null_mut(); ids.len()];
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(c) = chunks.get(k) else { break };
                        for (slot, b) in ptrs.iter_mut().zip(&bases[c.arch]) {
                            *slot = *b as *mut u8;
                        }
                        let a = &archetypes[c.arch];
                        for row in c.start..c.end {
                            // SAFETY: chunks are disjoint row ranges, aliasing between terms was checked above, and the
                            // column memory outlives the scope (the world is borrowed for the whole call).
                            let item = unsafe { Q::fetch(&ptrs, row) };
                            let e = a.entities[row];
                            f(e, meta[e.index as usize].uid, item);
                        }
                    }
                });
            }
        });
        Ok(())
    }

    // ------------------------------------------------------------------------------------------ hashing
    /// Hash of the entire logical state: entities by uid, components by name, values field by field.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        h.u64(self.live as u64);
        h.u64(self.next_uid);
        // only when present, so worlds without resources keep the hashes they always had
        if !self.resources.is_empty() {
            h.u64(self.resources.len() as u64);
            for (k, v) in &self.resources {
                h.bytes(k.as_bytes());
                h.u64(v.to_bits());
            }
        }
        for e in self.entities_by_uid() {
            let m = self.meta[e.index as usize];
            h.u64(m.uid);
            let a = &self.archetypes[m.arch as usize];
            h.u64(a.components.len() as u64);
            for &oi in &a.name_order {
                let info = &self.registry.infos[a.components[oi] as usize];
                h.bytes(info.name.as_bytes());
                // SAFETY: the row exists and holds a value of this component.
                unsafe { info.hash_into(a.columns[oi].ptr_at(m.row as usize), &self.strings, &mut h) };
            }
        }
        h.finish()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// typed queries
pub trait QueryTerm {
    type Item<'w>;
    const MUTABLE: bool;
    fn component(world: &mut World) -> Result<ComponentId>;
    /// # Safety
    /// `base` must be the column base of this component and `row` in bounds; the caller guarantees aliasing rules.
    unsafe fn fetch<'w>(base: *mut u8, row: usize) -> Self::Item<'w>;
}

impl<T: Component> QueryTerm for &T {
    type Item<'w> = &'w T;
    const MUTABLE: bool = false;
    fn component(world: &mut World) -> Result<ComponentId> {
        world.register_native::<T>()
    }
    unsafe fn fetch<'w>(base: *mut u8, row: usize) -> &'w T {
        // SAFETY: guaranteed by the caller.
        unsafe { &*base.cast::<T>().add(row) }
    }
}

impl<T: Component> QueryTerm for &mut T {
    type Item<'w> = &'w mut T;
    const MUTABLE: bool = true;
    fn component(world: &mut World) -> Result<ComponentId> {
        world.register_native::<T>()
    }
    unsafe fn fetch<'w>(base: *mut u8, row: usize) -> &'w mut T {
        // SAFETY: guaranteed by the caller.
        unsafe { &mut *base.cast::<T>().add(row) }
    }
}

pub trait Query {
    type Item<'w>;
    fn terms(world: &mut World) -> Result<Vec<(ComponentId, bool)>>;
    /// # Safety
    /// `bases[i]` must be the column base for term `i` and `row` in bounds.
    unsafe fn fetch<'w>(bases: &[*mut u8], row: usize) -> Self::Item<'w>;
}

macro_rules! impl_query {
    ($($T:ident $i:tt),+) => {
        impl<$($T: QueryTerm),+> Query for ($($T,)+) {
            type Item<'w> = ($($T::Item<'w>,)+);
            fn terms(world: &mut World) -> Result<Vec<(ComponentId, bool)>> {
                Ok(vec![$(($T::component(world)?, $T::MUTABLE)),+])
            }
            unsafe fn fetch<'w>(bases: &[*mut u8], row: usize) -> Self::Item<'w> {
                // SAFETY: forwarded from the caller.
                unsafe { ($($T::fetch(bases[$i], row),)+) }
            }
        }
    };
}
impl_query!(A 0);
impl_query!(A 0, B 1);
impl_query!(A 0, B 1, C 2);
impl_query!(A 0, B 1, C 2, D 3);
impl_query!(A 0, B 1, C 2, D 3, E 4);
impl_query!(A 0, B 1, C 2, D 3, E 4, F 5);

/// Resource names: dot-separated parts of letters, digits and underscores, at most 64 characters in all.
pub fn valid_resource_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}
