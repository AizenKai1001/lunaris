//! Component schemas. A component is a flat record of scalars and fixed arrays. The same description serves Rust types,
//! components defined at runtime (by a script or an AI), JSON conversion, hashing and documentation.
use crate::error::{EcsError, Result};
use crate::strings::StringTable;
use lunaris_json::Json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    F32,
    F64,
    I32,
    U32,
    I64,
    U64,
    Bool,
    /// Interned string, stored as a u32 symbol.
    Str,
    /// Reference to another entity by its stable uid (0 = none).
    Uid,
}

impl FieldType {
    pub fn size(self) -> usize {
        match self {
            FieldType::F32 | FieldType::I32 | FieldType::U32 | FieldType::Str => 4,
            FieldType::F64 | FieldType::I64 | FieldType::U64 | FieldType::Uid => 8,
            FieldType::Bool => 1,
        }
    }

    pub fn align(self) -> usize {
        self.size()
    }

    pub fn name(self) -> &'static str {
        match self {
            FieldType::F32 => "f32",
            FieldType::F64 => "f64",
            FieldType::I32 => "i32",
            FieldType::U32 => "u32",
            FieldType::I64 => "i64",
            FieldType::U64 => "u64",
            FieldType::Bool => "bool",
            FieldType::Str => "str",
            FieldType::Uid => "uid",
        }
    }

    pub fn parse(s: &str) -> Option<FieldType> {
        Some(match s {
            "f32" => FieldType::F32,
            "f64" => FieldType::F64,
            "i32" => FieldType::I32,
            "u32" => FieldType::U32,
            "i64" => FieldType::I64,
            "u64" => FieldType::U64,
            "bool" => FieldType::Bool,
            "str" => FieldType::Str,
            "uid" => FieldType::Uid,
            _ => return None,
        })
    }

    pub const ALL: [&'static str; 9] = ["f32", "f64", "i32", "u32", "i64", "u64", "bool", "str", "uid"];
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: FieldType,
    /// 1 = scalar, n > 1 = fixed-size array of n elements.
    pub count: u32,
    pub offset: usize,
}

impl Field {
    pub fn byte_len(&self) -> usize {
        self.ty.size() * self.count as usize
    }
}

#[derive(Clone, Debug)]
pub struct ComponentInfo {
    pub id: ComponentId,
    pub name: String,
    pub size: usize,
    pub align: usize,
    pub fields: Vec<Field>,
    pub doc: String,
    /// True for components backed by a Rust type (their layout is the Rust layout).
    pub native: bool,
    /// Bytes of a new component with every field at its default value.
    pub default_bytes: Vec<u8>,
}

/// A field description before a layout is known: (name, type, count, default).
#[derive(Clone, Debug)]
pub struct FieldSpec {
    pub name: String,
    pub ty: FieldType,
    pub count: u32,
    pub default: Option<Json>,
}

impl FieldSpec {
    pub fn new(name: &str, ty: FieldType, count: u32) -> Self {
        FieldSpec { name: name.to_string(), ty, count, default: None }
    }
}

impl ComponentInfo {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn schema_json(&self) -> Json {
        let fields = self.fields.iter().map(|f| {
            let mut o = Json::obj([("name", Json::str(f.name.clone())), ("type", Json::str(f.ty.name()))]);
            if f.count > 1 {
                o.set("count", Json::from(f.count as i64));
            }
            o
        });
        Json::obj([
            ("name", Json::str(self.name.clone())),
            ("doc", Json::str(self.doc.clone())),
            ("native", Json::Bool(self.native)),
            ("size_bytes", Json::from(self.size)),
            ("fields", Json::arr(fields)),
        ])
    }
}

/// Lay out fields like `#[repr(C)]`: in order, each at the next multiple of its alignment.
pub fn compute_layout(specs: &[FieldSpec]) -> (Vec<Field>, usize, usize) {
    let mut offset = 0usize;
    let mut align = 1usize;
    let mut fields = Vec::with_capacity(specs.len());
    for s in specs {
        let a = s.ty.align();
        offset = offset.div_ceil(a) * a;
        fields.push(Field { name: s.name.clone(), ty: s.ty, count: s.count, offset });
        offset += s.ty.size() * s.count as usize;
        align = align.max(a);
    }
    let size = offset.div_ceil(align) * align;
    (fields, if specs.is_empty() { 0 } else { size }, align)
}

pub fn valid_identifier(s: &str) -> bool {
    let mut c = s.chars();
    match c.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

// ---------------------------------------------------------------------------------------------------------------
// Rust-side description (used by the `component!` macro)
pub trait ScalarKind {
    const TYPE: FieldType;
}
macro_rules! scalar_kind {
    ($($t:ty => $v:ident),* $(,)?) => { $(impl ScalarKind for $t { const TYPE: FieldType = FieldType::$v; })* };
}
scalar_kind!(f32 => F32, f64 => F64, i32 => I32, u32 => U32, i64 => I64, u64 => U64, bool => Bool);

/// Interned-string handle stored in a component.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Sym(pub u32);
impl ScalarKind for Sym {
    const TYPE: FieldType = FieldType::Str;
}

/// Reference to another entity by stable uid (0 = none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Uid(pub u64);
impl ScalarKind for Uid {
    const TYPE: FieldType = FieldType::Uid;
}

/// What a struct field's Rust type maps to: a scalar or a fixed array of one scalar kind.
pub trait FieldKind {
    const TYPE: FieldType;
    const COUNT: u32;
}
impl<T: ScalarKind, const N: usize> FieldKind for [T; N] {
    const TYPE: FieldType = T::TYPE;
    const COUNT: u32 = N as u32;
}
macro_rules! field_kind_scalar {
    ($($t:ty),*) => { $(impl FieldKind for $t { const TYPE: FieldType = <$t as ScalarKind>::TYPE; const COUNT: u32 = 1; })* };
}
field_kind_scalar!(f32, f64, i32, u32, i64, u64, bool, Sym, Uid);

#[derive(Clone, Debug)]
pub struct NativeSpec {
    pub name: &'static str,
    pub size: usize,
    pub align: usize,
    /// (name, type, count, offset)
    pub fields: Vec<(&'static str, FieldType, u32, usize)>,
    pub default_bytes: Vec<u8>,
    pub doc: &'static str,
}

/// A Rust type that can be stored in the world. Plain data only: `Copy`, no pointers, no destructors.
pub trait Component: Copy + Default + 'static {
    fn spec() -> NativeSpec;
}

// ---------------------------------------------------------------------------------------------------------------
// JSON <-> bytes
/// Where a value sits (`Health.hp`, `Transform.pos[2]`). It is only turned into text when an error needs it, so the common case
/// (no error) allocates nothing.
struct Path<'a> {
    comp: &'a str,
    field: &'a str,
    index: Option<usize>,
}

impl std::fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.comp, self.field)?;
        match self.index {
            Some(i) => write!(f, "[{i}]"),
            None => Ok(()),
        }
    }
}

fn mismatch(path: &Path, expected: &str, got: &Json) -> EcsError {
    EcsError::new("type_mismatch", format!("expected {expected}, got {}", got.type_name())).at(path.to_string())
}

/// A value converted to the field's type, not yet stored.
enum Scalar<'a> {
    F32(f32),
    F64(f64),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Bool(bool),
    Str(&'a str),
}

/// The one set of rules for what JSON may be stored in a field of type `ty` (used for checking and for writing).
fn convert<'a>(ty: FieldType, v: &'a Json, path: &Path) -> Result<Scalar<'a>> {
    Ok(match ty {
        FieldType::F32 => {
            let f = v.as_f64().ok_or_else(|| mismatch(path, "a number", v))? as f32;
            if !f.is_finite() {
                return Err(EcsError::new("out_of_range", "number does not fit in f32").at(path.to_string()));
            }
            Scalar::F32(f)
        }
        FieldType::F64 => {
            let f = v.as_f64().ok_or_else(|| mismatch(path, "a number", v))?;
            if !f.is_finite() {
                return Err(EcsError::new("out_of_range", "number is not finite").at(path.to_string()));
            }
            Scalar::F64(f)
        }
        FieldType::I32 => {
            let i = v.as_i64().ok_or_else(|| mismatch(path, "an integer", v))?;
            Scalar::I32(i32::try_from(i).map_err(|_| EcsError::new("out_of_range", format!("{i} does not fit in i32")).at(path.to_string()))?)
        }
        FieldType::U32 => {
            let i = v.as_i64().ok_or_else(|| mismatch(path, "an integer", v))?;
            Scalar::U32(u32::try_from(i).map_err(|_| EcsError::new("out_of_range", format!("{i} does not fit in u32")).at(path.to_string()))?)
        }
        FieldType::I64 => Scalar::I64(v.as_i64().ok_or_else(|| mismatch(path, "an integer", v))?),
        FieldType::U64 | FieldType::Uid => Scalar::U64(v.as_u64().ok_or_else(|| mismatch(path, "a non-negative integer", v))?),
        FieldType::Bool => Scalar::Bool(v.as_bool().ok_or_else(|| mismatch(path, "true or false", v))?),
        FieldType::Str => Scalar::Str(v.as_str().ok_or_else(|| mismatch(path, "a string", v))?),
    })
}

/// Write one scalar into `dst`.
///
/// # Safety
/// `dst` must point to writable memory aligned for the field type.
unsafe fn write_scalar(ty: FieldType, v: &Json, dst: *mut u8, strings: &mut StringTable, path: &Path) -> Result<()> {
    let value = convert(ty, v, path)?;
    // SAFETY: caller contract; each arm writes exactly the type's size.
    unsafe {
        match value {
            Scalar::F32(x) => dst.cast::<f32>().write(x),
            Scalar::F64(x) => dst.cast::<f64>().write(x),
            Scalar::I32(x) => dst.cast::<i32>().write(x),
            Scalar::U32(x) => dst.cast::<u32>().write(x),
            Scalar::I64(x) => dst.cast::<i64>().write(x),
            Scalar::U64(x) => dst.cast::<u64>().write(x),
            Scalar::Bool(x) => dst.write(x as u8),
            Scalar::Str(x) => dst.cast::<u32>().write(strings.intern(x)),
        }
    }
    Ok(())
}

/// Read one scalar from `src`.
///
/// # Safety
/// `src` must point to a valid, aligned value of the field type.
unsafe fn read_scalar(ty: FieldType, src: *const u8, strings: &StringTable) -> Json {
    unsafe {
        match ty {
            FieldType::F32 => Json::Float(lunaris_json::f32_value(src.cast::<f32>().read())),
            FieldType::F64 => Json::Float(src.cast::<f64>().read()),
            FieldType::I32 => Json::Int(src.cast::<i32>().read() as i64),
            FieldType::U32 => Json::Int(src.cast::<u32>().read() as i64),
            FieldType::I64 => Json::Int(src.cast::<i64>().read()),
            FieldType::U64 | FieldType::Uid => {
                let u = src.cast::<u64>().read();
                if u <= i64::MAX as u64 {
                    Json::Int(u as i64)
                } else {
                    Json::Float(u as f64)
                }
            }
            FieldType::Bool => Json::Bool(src.read() != 0),
            FieldType::Str => Json::Str(strings.get(src.cast::<u32>().read()).to_string()),
        }
    }
}

impl ComponentInfo {
    /// Merge the fields present in `json` (an object) into the component bytes at `dst`; absent fields keep their value.
    ///
    /// # Safety
    /// `dst` must point to `self.size` writable bytes laid out as this component.
    pub unsafe fn merge_json(&self, json: &Json, dst: *mut u8, strings: &mut StringTable) -> Result<()> {
        let pairs = match json {
            Json::Obj(p) => p,
            other => {
                return Err(EcsError::new("type_mismatch", format!("component {} must be an object, got {}", self.name, other.type_name()))
                    .at(self.name.clone())
                    .hint(self.field_hint()))
            }
        };
        // validate everything first so a failure never leaves a half-written component
        for (k, v) in pairs {
            let f = self.field(k).ok_or_else(|| {
                EcsError::new("unknown_field", format!("component {} has no field `{k}`", self.name)).at(format!("{}.{k}", self.name)).hint(self.field_hint())
            })?;
            let path = Path { comp: &self.name, field: &f.name, index: None };
            if f.count == 1 {
                convert(f.ty, v, &path)?;
            } else {
                let arr = v.as_array().ok_or_else(|| mismatch(&path, &format!("an array of {} {}", f.count, f.ty.name()), v))?;
                if arr.len() != f.count as usize {
                    return Err(EcsError::new("wrong_length", format!("expected {} elements, got {}", f.count, arr.len())).at(path.to_string()));
                }
                for (i, e) in arr.iter().enumerate() {
                    convert(f.ty, e, &Path { index: Some(i), ..path })?;
                }
            }
        }
        for (k, v) in pairs {
            let f = self.field(k).expect("validated above");
            let path = Path { comp: &self.name, field: &f.name, index: None };
            // SAFETY: offsets come from the layout, which keeps every field inside `size` and aligned.
            unsafe {
                let base = dst.add(f.offset);
                if f.count == 1 {
                    write_scalar(f.ty, v, base, strings, &path)?;
                } else {
                    for (i, e) in v.as_array().unwrap().iter().enumerate() {
                        write_scalar(f.ty, e, base.add(i * f.ty.size()), strings, &path)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn field_hint(&self) -> String {
        let names: Vec<String> = self.fields.iter().map(|f| if f.count > 1 { format!("{}: {}[{}]", f.name, f.ty.name(), f.count) } else { format!("{}: {}", f.name, f.ty.name()) }).collect();
        format!("fields of {}: {}", self.name, if names.is_empty() { "(none)".to_string() } else { names.join(", ") })
    }

    /// Read the component bytes at `src` as a JSON object.
    ///
    /// # Safety
    /// `src` must point to `self.size` valid bytes laid out as this component.
    pub unsafe fn read_json(&self, src: *const u8, strings: &StringTable) -> Json {
        let mut pairs = Vec::with_capacity(self.fields.len());
        for f in &self.fields {
            // SAFETY: offsets come from the layout.
            let v = unsafe {
                let base = src.add(f.offset);
                if f.count == 1 {
                    read_scalar(f.ty, base, strings)
                } else {
                    Json::Arr((0..f.count as usize).map(|i| read_scalar(f.ty, base.add(i * f.ty.size()), strings)).collect())
                }
            };
            pairs.push((f.name.clone(), v));
        }
        Json::Obj(pairs)
    }

    /// Feed the field values (not the padding) into `h`.
    ///
    /// # Safety
    /// `src` must point to `self.size` valid bytes laid out as this component.
    pub unsafe fn hash_into(&self, src: *const u8, strings: &StringTable, h: &mut StateHasher) {
        for f in &self.fields {
            for i in 0..f.count as usize {
                // SAFETY: offsets come from the layout.
                unsafe {
                    let p = src.add(f.offset + i * f.ty.size());
                    match f.ty {
                        FieldType::F32 => h.u64(p.cast::<f32>().read().to_bits() as u64),
                        FieldType::F64 => h.u64(p.cast::<f64>().read().to_bits()),
                        FieldType::I32 => h.u64(p.cast::<i32>().read() as i64 as u64),
                        FieldType::U32 => h.u64(p.cast::<u32>().read() as u64),
                        FieldType::I64 => h.u64(p.cast::<i64>().read() as u64),
                        FieldType::U64 | FieldType::Uid => h.u64(p.cast::<u64>().read()),
                        FieldType::Bool => h.u64((p.read() != 0) as u64),
                        FieldType::Str => h.bytes(strings.get(p.cast::<u32>().read()).as_bytes()),
                    }
                }
            }
        }
    }
}

/// One step of hashing an entity's components, worked out once per archetype by `World::state_hash` instead of once per entity: a constant word (the number of
/// components, a component's name) or a run of values read from one column. Feeding the hasher with these gives exactly the words that `ComponentInfo::hash_into` and
/// `StateHasher::bytes` give, so the hash does not change.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HashOp {
    Word(u64),
    Run { col: usize, offset: usize, ty: FieldType, count: usize },
}

impl ComponentInfo {
    /// Append the steps that hash this component, which lives in column `col` of its archetype: its name as `StateHasher::bytes` feeds it, then its fields (neighbouring
    /// fields of one type that follow each other in memory become one run).
    pub(crate) fn push_hash_ops(&self, col: usize, ops: &mut Vec<HashOp>) {
        let name = self.name.as_bytes();
        ops.push(HashOp::Word(name.len() as u64));
        for chunk in name.chunks(8) {
            let mut w = [0u8; 8];
            w[..chunk.len()].copy_from_slice(chunk);
            ops.push(HashOp::Word(u64::from_le_bytes(w)));
        }
        for f in &self.fields {
            let count = f.count as usize;
            if let Some(HashOp::Run { col: c, offset, ty, count: n }) = ops.last_mut() {
                // (a string needs its text, so it is never merged)
                if *c == col && *ty == f.ty && *ty != FieldType::Str && *offset + *n * ty.size() == f.offset {
                    *n += count;
                    continue;
                }
            }
            ops.push(HashOp::Run { col, offset: f.offset, ty: f.ty, count });
        }
    }
}

/// Fast, portable 64-bit state hasher (integer mixing only, so it gives the same answer on every platform).
#[derive(Clone, Debug)]
pub struct StateHasher {
    h: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        StateHasher { h: 0x9E37_79B9_7F4A_7C15 }
    }
}

/// One step of the hash on a bare state: the loop that hashes a whole world keeps the state in a local (a register) instead of behind a `&mut StateHasher`, because
/// every step waits for the one before it and a round trip through memory in each would make the chain twice as long.
#[inline(always)]
pub(crate) fn mix_word(h: u64, x: u64) -> u64 {
    (h.rotate_left(23) ^ x).wrapping_mul(0x9FB2_1C65_1E98_DF25).wrapping_add(0x6A09_E667_F3BC_C909)
}

/// `StateHasher::bytes` on a bare state.
#[inline]
pub(crate) fn mix_bytes(mut h: u64, b: &[u8]) -> u64 {
    h = mix_word(h, b.len() as u64);
    for chunk in b.chunks(8) {
        let mut w = [0u8; 8];
        w[..chunk.len()].copy_from_slice(chunk);
        h = mix_word(h, u64::from_le_bytes(w));
    }
    h
}

impl StateHasher {
    #[inline]
    pub fn u64(&mut self, x: u64) {
        self.h = mix_word(self.h, x);
    }

    pub fn bytes(&mut self, b: &[u8]) {
        self.h = mix_bytes(self.h, b);
    }

    /// The state itself, to hash a long run in a local and put it back with `set_raw`.
    #[inline]
    pub(crate) fn raw(&self) -> u64 {
        self.h
    }

    #[inline]
    pub(crate) fn set_raw(&mut self, h: u64) {
        self.h = h;
    }

    pub fn finish(&self) -> u64 {
        let mut z = self.h;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}
