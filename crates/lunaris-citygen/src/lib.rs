//! A procedural city: streets, lots and buildings made from rules and hashes of a seed, written as glTF cells that the engine streams like any other model.
//! The buildings are real geometry (recessed windows with frames and sills, balconies, cornices, parapets, roof equipment, shopfronts) wearing photographed PBR materials at their true scale.

pub mod gltf;
pub mod materials;
pub mod mesh;
pub mod png;
pub mod rng;
pub mod scene;
pub mod facade;
pub mod building;
pub mod survival;
pub mod weather;
pub mod city;
pub mod street;
pub mod props;
pub mod interior;
pub mod simplify;
pub mod plan;
pub mod lod;
pub mod streetscape;
pub mod runtime;
pub mod export;
pub mod polar;
pub mod serverside;
pub mod shell;
