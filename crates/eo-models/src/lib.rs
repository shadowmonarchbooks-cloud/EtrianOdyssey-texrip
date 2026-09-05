//! Model and material inspection contracts for EO-TexRip.
//!
//! 0.50 begins the native implementations behind these contracts. Inspectors
//! expose only structural texture bindings; semantic roles remain `Unknown`
//! unless the container metadata proves a stronger classification.

mod bch;
mod cgfx;

pub use bch::BchModelInspector;
pub use cgfx::CgfxModelInspector;

use eo_core::TextureRole;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextureReference {
    pub slot: u8,
    pub internal_name: String,
    pub role: TextureRole,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphaInput {
    pub input: u8,
    pub source_id: u8,
    pub operand_id: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlphaStage {
    pub stage: u8,
    pub combiner_id: u8,
    pub inputs: Vec<AlphaInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialRecord {
    /// Payload-global material index, matching the frozen report.
    pub index: u32,
    pub model_index: u32,
    pub model_material_index: u32,
    pub name: Option<String>,
    pub textures: Vec<TextureReference>,
    /// PICA texture-environment alpha stages in hardware order.
    pub alpha_stages: Vec<AlphaStage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInventory {
    /// Exact structural model count reported by the container format.
    pub model_count: u32,
    /// Convenience name when the payload has one unambiguous model identity.
    pub model_name: Option<String>,
    pub materials: Vec<MaterialRecord>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("model header is invalid")]
    InvalidHeader,
    #[error("model offset or length is outside the source")]
    InvalidOffset,
    #[error("model revision is unsupported: {0}")]
    UnsupportedRevision(String),
    #[error("material metadata is malformed: {0}")]
    InvalidMaterial(String),
}

pub trait ModelInspector {
    fn probe(&self, data: &[u8]) -> bool;
    fn inspect(&self, data: &[u8]) -> Result<ModelInventory, ModelError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_slots_are_explicit_structural_metadata() {
        let material = MaterialRecord {
            index: 0,
            model_index: 0,
            model_material_index: 0,
            name: Some("body".to_owned()),
            textures: vec![TextureReference {
                slot: 1,
                internal_name: "body_mask".to_owned(),
                role: TextureRole::Mask,
                enabled: true,
            }],
            alpha_stages: Vec::new(),
        };
        assert_eq!(material.textures[0].slot, 1);
        assert_eq!(material.textures[0].role, TextureRole::Mask);
    }
}
