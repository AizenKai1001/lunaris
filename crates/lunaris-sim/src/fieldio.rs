//! Reading and writing one field of an entity by its path (`Transform.pos[1]`, `Health.value`), for the parts of the engine that are driven by text:
//! state machines, tweens and conditions. The same paths as in graph systems.
use lunaris_ecs::{ComponentId, Entity, FieldType, World};

/// Where a field lives inside its component.
#[derive(Clone, Debug)]
pub struct Slot {
    pub cid: ComponentId,
    pub offset: usize,
    pub ty: FieldType,
    pub label: String,
}

/// Work out a path such as `Transform.pos[1]`. The error says what is wrong in words an author can act on.
pub fn slot(world: &World, path: &str) -> Result<Slot, String> {
    let (comp, rest) = path.split_once('.').ok_or_else(|| format!("`{path}` is not a field path (write Component.field or Component.field[index])"))?;
    let cid = world.component_id(comp).ok_or_else(|| format!("there is no component `{comp}` (in `{path}`)"))?;
    let (fname, idx) = match rest.split_once('[') {
        Some((f, i)) => (f, Some(i.strip_suffix(']').and_then(|n| n.parse::<usize>().ok()).ok_or_else(|| format!("bad array index in `{path}`"))?)),
        None => (rest, None),
    };
    let info = world.info(cid);
    let field = info.field(fname).ok_or_else(|| format!("{comp} has no field `{fname}` (its fields: {})", info.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")))?;
    let elem = match (field.count, idx) {
        (1, None) | (1, Some(0)) => 0,
        (1, Some(_)) => return Err(format!("{comp}.{fname} is not an array")),
        (n, Some(i)) if (i as u32) < n => i,
        (n, Some(i)) => return Err(format!("index {i} is out of range for {comp}.{fname} (length {n})")),
        (n, None) => return Err(format!("{comp}.{fname} is an array of {n}: pick an element, e.g. {comp}.{fname}[0]")),
    };
    Ok(Slot { cid, offset: field.offset + elem * field.ty.size(), ty: field.ty, label: path.to_string() })
}

/// The number in the field (booleans are 0 and 1); `None` when the entity does not have the component or the field is a string.
pub fn read(world: &World, e: Entity, s: &Slot) -> Option<f64> {
    let base = world.component_ptr(e, s.cid)?;
    // SAFETY: `base` points at this component's bytes and `offset` and `ty` come from its layout.
    unsafe {
        let p = base.add(s.offset);
        Some(match s.ty {
            FieldType::F32 => p.cast::<f32>().read() as f64,
            FieldType::F64 => p.cast::<f64>().read(),
            FieldType::I32 => p.cast::<i32>().read() as f64,
            FieldType::U32 => p.cast::<u32>().read() as f64,
            FieldType::I64 => p.cast::<i64>().read() as f64,
            FieldType::U64 | FieldType::Uid => p.cast::<u64>().read() as f64,
            FieldType::Bool => (p.read() != 0) as u8 as f64,
            FieldType::Str => return None,
        })
    }
}

/// Store a number in the field. `false` when the entity lacks the component, the value is not finite (or does not fit) or the field is a string.
pub fn write(world: &mut World, e: Entity, s: &Slot, v: f64) -> bool {
    if !v.is_finite() {
        return false;
    }
    let Some(base) = world.component_ptr(e, s.cid) else { return false };
    // SAFETY: as in `read`; the world is borrowed mutably, so nothing else points at the field.
    unsafe {
        let p = base.add(s.offset);
        match s.ty {
            FieldType::F32 => {
                let y = v as f32;
                if !y.is_finite() {
                    return false;
                }
                p.cast::<f32>().write(y)
            }
            FieldType::F64 => p.cast::<f64>().write(v),
            FieldType::I32 => p.cast::<i32>().write(v as i32),
            FieldType::U32 => p.cast::<u32>().write(v as u32),
            FieldType::I64 => p.cast::<i64>().write(v as i64),
            FieldType::U64 | FieldType::Uid => p.cast::<u64>().write(v as u64),
            FieldType::Bool => p.write((v != 0.0) as u8),
            FieldType::Str => return false,
        }
    }
    true
}
