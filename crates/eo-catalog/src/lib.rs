//! Rescan-safe catalog behavior.
//!
//! Extraction may discover better structural metadata over time. User decisions
//! and verified runtime evidence are different: they must survive a rescan unless
//! the user explicitly changes them.

use eo_core::{
    AssetId, EvidenceConfidence, RuntimeHash, RuntimeHashEvidence, TextureAsset, UserMetadata,
};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("asset is not present in the catalog: {0}")]
    MissingAsset(AssetId),
}

#[derive(Clone, Debug, Default)]
pub struct AssetCatalog {
    assets: BTreeMap<AssetId, TextureAsset>,
}

impl AssetCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn get(&self, id: &AssetId) -> Option<&TextureAsset> {
        self.assets.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TextureAsset> {
        self.assets.values()
    }

    /// Insert a newly discovered asset or merge a rescan of an existing stable ID.
    ///
    /// Extraction-owned fields come from `incoming`. User-owned metadata,
    /// user-overridden classification, and the strongest runtime-hash evidence
    /// are retained from the existing catalog row.
    pub fn upsert_extracted(&mut self, mut incoming: TextureAsset) -> &TextureAsset {
        if let Some(existing) = self.assets.get(&incoming.id) {
            incoming.user = existing.user.clone();
            if existing.classification.user_override {
                incoming.classification = existing.classification.clone();
            }
            incoming.runtime_hashes = merge_hash_evidence(
                existing.runtime_hashes.iter().cloned(),
                incoming.runtime_hashes.into_iter(),
            );
        }
        let id = incoming.id.clone();
        self.assets.insert(id.clone(), incoming);
        self.assets.get(&id).expect("asset was just inserted")
    }

    pub fn set_user_metadata(
        &mut self,
        id: &AssetId,
        metadata: UserMetadata,
    ) -> Result<(), CatalogError> {
        let asset = self
            .assets
            .get_mut(id)
            .ok_or_else(|| CatalogError::MissingAsset(id.clone()))?;
        asset.user = metadata;
        Ok(())
    }
}

fn merge_hash_evidence(
    existing: impl Iterator<Item = RuntimeHashEvidence>,
    incoming: impl Iterator<Item = RuntimeHashEvidence>,
) -> Vec<RuntimeHashEvidence> {
    let mut by_hash: BTreeMap<RuntimeHash, RuntimeHashEvidence> = BTreeMap::new();
    for evidence in existing.chain(incoming) {
        match by_hash.get(&evidence.hash) {
            Some(current) if current.confidence >= evidence.confidence => {}
            _ => {
                by_hash.insert(evidence.hash, evidence);
            }
        }
    }
    by_hash.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_core::{
        AssetClassification, GameId, SourceLocator, TextureDimensions, TextureFormat,
        TextureRole,
    };
    use std::collections::BTreeSet;

    fn asset(id: &str, confidence: EvidenceConfidence) -> TextureAsset {
        TextureAsset {
            id: AssetId::new(id).unwrap(),
            game_id: GameId::EtrianOdysseyUntold,
            dimensions: TextureDimensions::new(16, 16).unwrap(),
            format: TextureFormat::Etc1,
            mip_level: 0,
            internal_name: Some("body".to_owned()),
            source: SourceLocator::default(),
            classification: AssetClassification {
                category: "monsters".to_owned(),
                role: TextureRole::Color,
                confidence: EvidenceConfidence::Structural,
                reason: "model binding".to_owned(),
                user_override: false,
            },
            runtime_hashes: vec![RuntimeHashEvidence {
                hash: "1111111111111111".parse().unwrap(),
                confidence,
                method: "fixture".to_owned(),
            }],
            user: UserMetadata::default(),
        }
    }

    #[test]
    fn rescan_preserves_user_owned_metadata_and_category_override() {
        let mut catalog = AssetCatalog::new();
        let mut first = asset("tex:0001", EvidenceConfidence::RuntimeVerified);
        first.user.friendly_name = Some("my-monkey".to_owned());
        first.user.category_override = Some("monsters/boss".to_owned());
        first.user.tags = BTreeSet::from(["favorite".to_owned()]);
        first.classification.category = "monsters/boss".to_owned();
        first.classification.user_override = true;
        catalog.upsert_extracted(first);

        let mut rescanned = asset("tex:0001", EvidenceConfidence::Candidate);
        rescanned.internal_name = Some("better_internal_name".to_owned());
        rescanned.classification.category = "misc".to_owned();
        let merged = catalog.upsert_extracted(rescanned);

        assert_eq!(merged.internal_name.as_deref(), Some("better_internal_name"));
        assert_eq!(merged.user.friendly_name.as_deref(), Some("my-monkey"));
        assert_eq!(merged.user.category_override.as_deref(), Some("monsters/boss"));
        assert!(merged.user.tags.contains("favorite"));
        assert_eq!(merged.classification.category, "monsters/boss");
        assert!(merged.classification.user_override);
    }

    #[test]
    fn rescan_never_downgrades_verified_hash_to_candidate() {
        let mut catalog = AssetCatalog::new();
        catalog.upsert_extracted(asset(
            "tex:0001",
            EvidenceConfidence::RuntimeVerified,
        ));
        let merged = catalog.upsert_extracted(asset(
            "tex:0001",
            EvidenceConfidence::Candidate,
        ));
        assert_eq!(merged.runtime_hashes.len(), 1);
        assert_eq!(
            merged.runtime_hashes[0].confidence,
            EvidenceConfidence::RuntimeVerified
        );
    }
}
