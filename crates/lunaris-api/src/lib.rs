//! Lunaris API: one tool set (the `Engine`), exposed over JSON-RPC 2.0 and the Model Context Protocol.
mod audio;
mod autotile;
mod mapgen;
mod tilecol;
mod tiled;
mod base64;
mod debug_draw;
mod export;
mod profile;
mod buses;
mod check;
mod engine;
mod gpu;
mod render_tools;
mod saves;
mod scatter;
mod terrain_layers;
mod rpc;
mod tools;
mod typing;
mod verify;
mod water;

pub use audio::{acoustics_for, room_settings, AcousticsConfig, Director};
pub use engine::{Engine, VERSION};
pub use gpu::{GpuBackend, GpuFactory, QUALITIES};
pub use rpc::{handle_message, safe_handle, serve, serve_with_limit, MAX_REQUEST_BYTES};
pub use tools::{tool_defs, tool_names, ToolDef};
pub use verify::{evaluate, run_spec};

#[cfg(test)]
mod audio_tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod layers_tests;
#[cfg(test)]
mod vehicle_tests;
#[cfg(test)]
mod mesh_tests;
#[cfg(test)]
mod saves_tests;
#[cfg(test)]
mod script_tests;
#[cfg(test)]
mod terrain_tests;
#[cfg(test)]
mod terrain_layers_tests;
#[cfg(test)]
mod water_tests;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod draw_order_tests;
#[cfg(test)]
mod language_tests;
#[cfg(test)]
mod menus_tests;
#[cfg(test)]
mod ui_kit3_tests;
#[cfg(test)]
mod scene_paths_tests;
#[cfg(test)]
mod tilecol_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod fuzz_tests;
