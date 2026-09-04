use crate::cityhash::cityhash64_hex;
use eo_textures::EncodedTexture;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParityAsset {
    pub candidate_hash: String,
    pub verified_hashes: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub format: i32,
    pub mip: u8,
    pub parser_used: String,
    pub category: String,
    pub material_binding_count: u64,
    #[serde(skip)]
    pub(crate) internal_name: String,
    #[serde(skip)]
    pub(crate) binding_keys: BTreeSet<String>,
}

impl ParityAsset {
    pub(crate) fn from_encoded(
        source: &str,
        internal_name: &str,
        parser_used: &str,
        encoded: &EncodedTexture,
        binding_keys: BTreeSet<String>,
    ) -> Self {
        let payload = encoded
            .runtime_hash_payload()
            .expect("container adapters only publish validated base-level textures");
        Self {
            candidate_hash: cityhash64_hex(payload),
            verified_hashes: Vec::new(),
            width: encoded.dimensions.visible_width,
            height: encoded.dimensions.visible_height,
            format: encoded.format as u8 as i32,
            mip: 0,
            parser_used: parser_used.to_owned(),
            category: category_for(&format!("{source}/{internal_name}")),
            material_binding_count: binding_keys.len() as u64,
            internal_name: internal_name.to_owned(),
            binding_keys,
        }
    }

    pub(crate) fn merge_bindings(&mut self, keys: impl IntoIterator<Item = String>) {
        self.binding_keys.extend(keys);
        self.material_binding_count = self.binding_keys.len() as u64;
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(
        candidate_hash: &str,
        width: u32,
        height: u32,
        format: i32,
        parser_used: &str,
        category: &str,
        material_binding_count: u64,
    ) -> Self {
        let binding_keys = (0..material_binding_count)
            .map(|index| format!("binding-{index}"))
            .collect();
        Self {
            candidate_hash: candidate_hash.to_owned(),
            verified_hashes: Vec::new(),
            width,
            height,
            format,
            mip: 0,
            parser_used: parser_used.to_owned(),
            category: category.to_owned(),
            material_binding_count,
            internal_name: String::new(),
            binding_keys,
        }
    }
}

pub(crate) fn dedupe_assets(assets: Vec<ParityAsset>) -> Vec<ParityAsset> {
    let mut output = Vec::<ParityAsset>::new();
    let mut by_key = BTreeMap::<(String, i32, u32, u32), usize>::new();
    for asset in assets {
        let key = (
            asset.candidate_hash.clone(),
            asset.format,
            asset.width,
            asset.height,
        );
        if let Some(index) = by_key.get(&key).copied() {
            let target = &mut output[index];
            target.merge_bindings(asset.binding_keys);
            for hash in asset.verified_hashes {
                if !target.verified_hashes.contains(&hash) {
                    target.verified_hashes.push(hash);
                }
            }
            target.verified_hashes.sort();
            target.verified_hashes.dedup();
        } else {
            by_key.insert(key, output.len());
            output.push(asset);
        }
    }
    output
}

pub(crate) fn bind_external_texture_names(
    assets: &mut [ParityAsset],
    bindings_by_name: &BTreeMap<String, BTreeSet<String>>,
) {
    let mut assets_by_name = BTreeMap::<String, Vec<usize>>::new();
    for (index, asset) in assets.iter().enumerate() {
        if !asset.internal_name.is_empty() {
            assets_by_name
                .entry(asset.internal_name.clone())
                .or_default()
                .push(index);
        }
    }
    for (name, binding_keys) in bindings_by_name {
        let Some(indices) = assets_by_name.get(name) else {
            continue;
        };
        if indices.len() != 1 {
            continue;
        }
        assets[indices[0]].merge_bindings(binding_keys.iter().cloned());
    }
}

fn category_for(source: &str) -> String {
    const RULES: &[(&str, &[&str])] = &[
        (
            "characters",
            &["face", "portrait", "chara", "character", "npc", "pc_", "event/ch", "bust"],
        ),
        ("monsters", &["enemy", "monster", "ene", "foe", "boss"]),
        (
            "ui",
            &["ui", "menu", "window", "frame", "cursor", "button", "layout"],
        ),
        ("icons", &["icon", "item", "skill", "equip", "status"]),
        ("maps", &["map", "floor", "atlas", "compass"]),
        (
            "dungeon",
            &["dungeon", "mori", "labyrinth", "field", "wall", "ground", "floor", "bg3d"],
        ),
        (
            "backgrounds",
            &["background", "back", "bg/", "eventbg", "town", "shop"],
        ),
        ("effects", &["effect", "eff", "particle", "magic"]),
        ("fonts", &["font", "glyph", "letter"]),
    ];
    let normalized = source.replace('\\', "/").to_ascii_lowercase();
    RULES
        .iter()
        .find_map(|(category, needles)| {
            needles
                .iter()
                .any(|needle| normalized.contains(needle))
                .then(|| (*category).to_owned())
        })
        .unwrap_or_else(|| "misc".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_core::{TextureDimensions, TextureFormat};

    #[test]
    fn candidate_hash_uses_exact_encoded_level_zero() {
        let texture = EncodedTexture {
            dimensions: TextureDimensions::new(8, 8).unwrap(),
            format: TextureFormat::A8,
            mip_count: 1,
            payload: vec![0x11; 64],
        };
        let asset = ParityAsset::from_encoded(
            "ui/test.stex",
            "",
            "eou_stex_strict",
            &texture,
            BTreeSet::new(),
        );
        assert_eq!(asset.candidate_hash, "7ABCF0A736B8A12E");
        assert_eq!(asset.category, "ui");
    }

    #[test]
    fn dedupe_matches_legacy_hash_format_dimension_identity() {
        let mut a = ParityAsset::test_fixture("A", 8, 8, 0, "first", "ui", 1);
        a.internal_name = "same".to_owned();
        let mut b = ParityAsset::test_fixture("A", 8, 8, 0, "second", "misc", 2);
        b.internal_name = "same".to_owned();
        let deduped = dedupe_assets(vec![a, b]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].parser_used, "first");
        assert_eq!(deduped[0].category, "ui");
        assert_eq!(deduped[0].material_binding_count, 2);
    }
}
