//! Lunaris ECS: archetype storage of plain-data components, stable entity uids, runtime component schemas, text scenes.
//!
//! Design in one paragraph: components are flat records (scalars and fixed arrays) described by a schema, so the same
//! machinery handles Rust types, components defined at runtime by a script or an AI, JSON conversion, hashing and
//! documentation. Entities are addressed by a stable `uid` outside the engine and by a generational `Entity` handle inside.
mod column;
mod error;
mod macros;
mod scene;
mod schema;
mod strings;
mod world;

pub use column::Column;
pub use error::{EcsError, Result};
pub use scene::{load_scene_json, parse_component_def, to_scene_json, SCENE_VERSION};
pub use schema::{compute_layout, valid_identifier, Component, ComponentId, ComponentInfo, Field, FieldKind, FieldSpec, FieldType, NativeSpec, ScalarKind, StateHasher, Sym, Uid};
pub use strings::StringTable;
pub use world::{valid_resource_name, Archetype, Bundle, Chunk, Entity, Query, QueryTerm, Terms, World, MAX_BUNDLE, MAX_TERMS};

#[cfg(test)]
mod tests;
