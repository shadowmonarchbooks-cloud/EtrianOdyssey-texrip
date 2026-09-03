//! Azahar output planning and validation.
//!
//! Azahar is an export target, not the internal asset identity model. Only verified
//! runtime hashes are eligible for automatic pack mappings.

use eo_core::{RuntimeHash, TextureAsset, TitleId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AzaharPackPlan {
    pub title_id: TitleId,
    pub use_new_hash: bool,
    pub textures: BTreeMap<RuntimeHash, String>,
}

impl AzaharPackPlan {
    pub fn new(title_id: TitleId) -> Self {
        Self {
            title_id,
            use_new_hash: true,
            textures: BTreeMap::new(),
        }
    }

    pub fn add_asset(
        &mut self,
        asset: &TextureAsset,
        relative_png_path: impl Into<String>,
    ) -> Result<usize, AzaharError> {
        let path = normalize_relative_png_path(relative_png_path.into())?;
        let mut added = 0;
        for hash in asset.verified_runtime_hashes() {
            match self.textures.get(&hash) {
                Some(existing) if existing != &path => {
                    return Err(AzaharError::ConflictingHash {
                        hash,
                        first_path: existing.clone(),
                        second_path: path,
                    });
                }
                Some(_) => {}
                None => {
                    self.textures.insert(hash, path.clone());
                    added += 1;
                }
            }
        }
        Ok(added)
    }
}

fn normalize_relative_png_path(path: String) -> Result<String, AzaharError> {
    let path = path.replace('\\', "/");
    let unsafe_path = path.is_empty()
        || path.starts_with('/')
        || path.split('/').any(|part| part.is_empty() || part == "..")
        || !path.to_ascii_lowercase().ends_with(".png");
    if unsafe_path {
        return Err(AzaharError::InvalidRelativePath(path));
    }
    Ok(path)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AzaharError {
    #[error("invalid Azahar relative PNG path: {0}")]
    InvalidRelativePath(String),
    #[error("runtime hash {hash} maps to both {first_path} and {second_path}")]
    ConflictingHash {
        hash: RuntimeHash,
        first_path: String,
        second_path: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_core::{
        AssetClassification, AssetId, EvidenceConfidence, GameId, RuntimeHashEvidence,
        SourceLocator, TextureDimensions, TextureFormat, UserMetadata,
    };

    fn asset(confidence: EvidenceConfidence) -> TextureAsset {
        TextureAsset {
            id: AssetId::new("tex:1").unwrap(),
            game_id: GameId::EtrianOdysseyUntold,
            dimensions: TextureDimensions::new(8, 8).unwrap(),
            format: TextureFormat::Etc1,
            mip_level: 0,
            internal_name: None,
            source: SourceLocator::default(),
            classification: AssetClassification::default(),
            runtime_hashes: vec![RuntimeHashEvidence {
                hash: "ABCDEF".parse().unwrap(),
                confidence,
                method: "test".to_owned(),
            }],
            user: UserMetadata::default(),
        }
    }

    #[test]
    fn candidates_do_not_enter_pack_automatically() {
        let mut plan = AzaharPackPlan::new("00040000000EC700".parse().unwrap());
        let added = plan
            .add_asset(&asset(EvidenceConfidence::Candidate), "monsters/body.png")
            .unwrap();
        assert_eq!(added, 0);
        assert!(plan.textures.is_empty());
    }

    #[test]
    fn runtime_verified_hashes_enter_pack() {
        let mut plan = AzaharPackPlan::new("00040000000EC700".parse().unwrap());
        let added = plan
            .add_asset(
                &asset(EvidenceConfidence::RuntimeVerified),
                "monsters/body.png",
            )
            .unwrap();
        assert_eq!(added, 1);
        assert_eq!(plan.textures.len(), 1);
    }
}
