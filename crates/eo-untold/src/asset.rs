use crate::cityhash::cityhash64_hex;
use eo_textures::{EncodedTexture, NativePicaDecoder, TextureDecoder};
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
    /// Virtual ROM/archive source retained only for local extraction output.
    #[serde(skip)]
    pub source: String,
    /// Primary container-provided texture name retained only for local extraction output.
    #[serde(skip)]
    pub internal_name: String,
    /// All container-provided names that resolve to this deduped pixel payload.
    #[serde(skip)]
    internal_names: BTreeSet<String>,
    /// Tightly packed RGBA8 pixels retained only in memory for native export.
    #[serde(skip)]
    pub rgba8: Vec<u8>,
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
        let decoded = NativePicaDecoder
            .decode_base_level(encoded)
            .expect("scan validates texture decoding before publishing an asset");
        let mut internal_names = BTreeSet::new();
        if !internal_name.is_empty() {
            internal_names.insert(internal_name.to_owned());
        }
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
            source: source.to_owned(),
            internal_name: internal_name.to_owned(),
            internal_names,
            rgba8: decoded.rgba8,
            binding_keys,
        }
    }

    pub(crate) fn merge_bindings(&mut self, keys: impl IntoIterator<Item = String>) {
        self.binding_keys.extend(keys);
        self.material_binding_count = self.binding_keys.len() as u64;
    }

    pub(crate) fn merge_internal_names(&mut self, names: impl IntoIterator<Item = String>) {
        self.internal_names.extend(names.into_iter().filter(|name| !name.is_empty()));
    }

    pub(crate) fn internal_names(&self) -> impl Iterator<Item = &str> {
        self.internal_names.iter().map(String::as_str)
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
            source: String::new(),
            internal_name: String::new(),
            internal_names: BTreeSet::new(),
            rgba8: Vec::new(),
            binding_keys,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_internal_name(&mut self, name: &str) {
        self.internal_name = name.to_owned();
        self.internal_names.clear();
        if !name.is_empty() {
            self.internal_names.insert(name.to_owned());
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
            target.merge_internal_names(asset.internal_names);
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
        for name in asset.internal_names() {
            assets_by_name
                .entry(name.to_owned())
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
            &[
                "face",
                "portrait",
                "chara",
                "character",
                "npc",
                "pc_",
                "event/ch",
                "bust",
            ],
        ),
        ("monsters", &["enemy", "monster", "ene", "foe", "boss"]),
        (
            "ui",
            &[
                "ui", "menu", "window", "frame", "cursor", "button", "layout",
            ],
        ),
        ("icons", &["icon", "item", "skill", "equip", "status"]),
        ("maps", &["map", "floor", "atlas", "compass"]),
        (
            "dungeon",
            &[
                "dungeon",
                "mori",
                "labyrinth",
                "field",
                "wall",
                "ground",
                "floor",
                "bg3d",
            ],
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
        assert_eq!(asset.source, "ui/test.stex");
        assert_eq!(asset.rgba8.len(), 8 * 8 * 4);
    }

    #[test]
    fn dedupe_matches_legacy_hash_format_dimension_identity() {
        let mut a = ParityAsset::test_fixture("A", 8, 8, 0, "first", "ui", 1);
        a.set_internal_name("same");
        let mut b = ParityAsset::test_fixture("A", 8, 8, 0, "second", "misc", 2);
        b.set_internal_name("same");
        let deduped = dedupe_assets(vec![a, b]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].parser_used, "first");
        assert_eq!(deduped[0].category, "ui");
        assert_eq!(deduped[0].material_binding_count, 2);
    }

    #[test]
    fn dedupe_preserves_distinct_names_for_identical_pixels() {
        let mut a = ParityAsset::test_fixture("A", 8, 8, 0, "first", "ui", 0);
        a.set_internal_name("day_sky");
        let mut b = ParityAsset::test_fixture("A", 8, 8, 0, "second", "misc", 0);
        b.set_internal_name("night_sky");

        let deduped = dedupe_assets(vec![a, b]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(
            deduped[0].internal_names().collect::<Vec<_>>(),
            vec!["day_sky", "night_sky"]
        );
    }

    #[test]
    fn external_binding_can_resolve_through_deduped_alias() {
        let mut a = ParityAsset::test_fixture("A", 8, 8, 0, "first", "ui", 0);
        a.set_internal_name("day_sky");
        let mut b = ParityAsset::test_fixture("A", 8, 8, 0, "second", "misc", 0);
        b.set_internal_name("night_sky");
        let mut assets = dedupe_assets(vec![a, b]);
        let bindings = BTreeMap::from([(
            "night_sky".to_owned(),
            BTreeSet::from(["material-slot".to_owned()]),
        )]);

        bind_external_texture_names(&mut assets, &bindings);
        assert_eq!(assets[0].material_binding_count, 1);
        assert!(assets[0].binding_keys.contains("material-slot"));
    }
}
