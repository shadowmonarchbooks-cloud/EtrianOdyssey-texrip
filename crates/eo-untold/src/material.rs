use crate::ParityAsset;
use eo_core::TextureFormat;
use eo_models::AlphaStage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialParitySummary {
    pub materials_found: u64,
    pub material_texture_bindings: u64,
    pub explicit_texture_alpha_channels: u64,
    pub constant_texture_alpha_inputs: u64,
    pub resolved_material_alphas: u64,
    pub unresolved_material_alphas: u64,
}

impl MaterialParitySummary {
    pub(crate) fn as_fingerprint_map(&self) -> BTreeMap<String, u64> {
        BTreeMap::from([
            ("materials_found".to_owned(), self.materials_found),
            (
                "material_texture_bindings".to_owned(),
                self.material_texture_bindings,
            ),
            (
                "explicit_texture_alpha_channels".to_owned(),
                self.explicit_texture_alpha_channels,
            ),
            (
                "constant_texture_alpha_inputs".to_owned(),
                self.constant_texture_alpha_inputs,
            ),
            (
                "resolved_material_alphas".to_owned(),
                self.resolved_material_alphas,
            ),
            (
                "unresolved_material_alphas".to_owned(),
                self.unresolved_material_alphas,
            ),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MaterialSlot {
    pub binding_key: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParityMaterial {
    pub slots: BTreeMap<u8, MaterialSlot>,
    pub alpha_stages: Vec<AlphaStage>,
}

impl ParityMaterial {
    pub(crate) fn insert_slot(&mut self, slot: u8, binding_key: String, enabled: bool) {
        match self.slots.get(&slot) {
            Some(existing) if existing.enabled || !enabled => {}
            _ => {
                self.slots.insert(
                    slot,
                    MaterialSlot {
                        binding_key,
                        enabled,
                    },
                );
            }
        }
    }
}

pub(crate) fn summarize_materials(
    materials: &[ParityMaterial],
    assets: &[ParityAsset],
) -> MaterialParitySummary {
    let mut asset_by_binding = BTreeMap::<String, usize>::new();
    for (index, asset) in assets.iter().enumerate() {
        for key in &asset.binding_keys {
            // The frozen material workspace keeps the first resolved asset for a
            // slot unless a later enabled binding replaces a disabled one. Asset
            // binding keys already encode enabled state, so first-wins preserves
            // the frozen extraction/dedup order for duplicate local names.
            asset_by_binding.entry(key.clone()).or_insert(index);
        }
    }

    let mut summary = MaterialParitySummary::default();
    for material in materials {
        let mut slots = BTreeMap::<u8, &ParityAsset>::new();
        for (slot, record) in &material.slots {
            let Some(index) = asset_by_binding.get(&record.binding_key).copied() else {
                continue;
            };
            slots.insert(*slot, &assets[index]);
        }
        if slots.is_empty() {
            // The frozen material report is asset-driven: a material with no
            // decoded/resolved texture binding is not emitted as a material row.
            continue;
        }

        summary.materials_found += 1;
        summary.material_texture_bindings += slots.len() as u64;
        let alpha_rows = count_alpha_rows(material, &slots, &mut summary);
        if alpha_pipeline_resolves(&material.alpha_stages, &slots) {
            summary.resolved_material_alphas += 1;
        } else if alpha_rows > 0 {
            summary.unresolved_material_alphas += 1;
        }
    }
    summary
}

fn count_alpha_rows(
    material: &ParityMaterial,
    slots: &BTreeMap<u8, &ParityAsset>,
    summary: &mut MaterialParitySummary,
) -> u64 {
    let mut seen = BTreeSet::<(u8, u8, u8, u8)>::new();
    let mut rows = 0u64;
    for stage in &material.alpha_stages {
        for input in &stage.inputs {
            if !(3..=5).contains(&input.source_id) {
                continue;
            }
            let slot = input.source_id - 3;
            let Some(asset) = slots.get(&slot) else {
                continue;
            };
            let key = (stage.stage, input.input, slot, input.operand_id);
            if !seen.insert(key) {
                continue;
            }
            if matches!(input.operand_id, 0 | 1) && !format_stores_alpha(asset.format) {
                summary.constant_texture_alpha_inputs += 1;
                rows += 1;
            } else if input.operand_id <= 7 {
                summary.explicit_texture_alpha_channels += 1;
                rows += 1;
            }
        }
    }
    rows
}

fn alpha_pipeline_resolves(stages: &[AlphaStage], slots: &BTreeMap<u8, &ParityAsset>) -> bool {
    let mut ordered = stages.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|stage| stage.stage);
    let mut previous_available = false;
    let mut used_texture = false;

    for stage in ordered {
        for input in &stage.inputs {
            match input.source_id {
                3..=5 => {
                    let slot = input.source_id - 3;
                    if input.operand_id > 7 || !slots.contains_key(&slot) {
                        return false;
                    }
                    used_texture = true;
                }
                15 => {
                    if !previous_available || !matches!(input.operand_id, 0 | 1) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        if !scalar_combiner_supported(stage.combiner_id, stage.inputs.len()) {
            return false;
        }
        previous_available = true;
    }

    previous_available && used_texture
}

fn scalar_combiner_supported(mode: u8, inputs: usize) -> bool {
    let required = match mode {
        0 => 1,
        1 | 2 | 3 | 5 => 2,
        4 | 8 | 9 => 3,
        // Dot-product modes need RGB state. Unknown modes remain unresolved.
        6 | 7 => return false,
        _ => return false,
    };
    inputs >= required
}

fn format_stores_alpha(format: i32) -> bool {
    u8::try_from(format)
        .ok()
        .and_then(|value| TextureFormat::try_from(value).ok())
        .is_some_and(TextureFormat::stores_alpha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_models::AlphaInput;

    fn asset(format: i32, key: &str) -> ParityAsset {
        let mut asset = ParityAsset::test_fixture("AAAAAAAAAAAAAAAA", 8, 8, format, "test", "misc", 0);
        asset.binding_keys.insert(key.to_owned());
        asset.material_binding_count = 1;
        asset
    }

    fn material(slot: u8, key: &str, operand: u8, combiner: u8) -> ParityMaterial {
        let mut material = ParityMaterial {
            slots: BTreeMap::new(),
            alpha_stages: vec![AlphaStage {
                stage: 0,
                combiner_id: combiner,
                inputs: vec![AlphaInput {
                    input: 0,
                    source_id: slot + 3,
                    operand_id: operand,
                }],
            }],
        };
        material.insert_slot(slot, key.to_owned(), true);
        material
    }

    #[test]
    fn stored_alpha_operand_is_an_explicit_channel_and_resolves() {
        let summary = summarize_materials(&[material(0, "m0", 0, 0)], &[asset(8, "m0")]);
        assert_eq!(summary.materials_found, 1);
        assert_eq!(summary.material_texture_bindings, 1);
        assert_eq!(summary.explicit_texture_alpha_channels, 1);
        assert_eq!(summary.constant_texture_alpha_inputs, 0);
        assert_eq!(summary.resolved_material_alphas, 1);
        assert_eq!(summary.unresolved_material_alphas, 0);
    }

    #[test]
    fn rgb_texture_alpha_operand_is_a_hardware_constant() {
        let summary = summarize_materials(&[material(0, "m0", 0, 0)], &[asset(1, "m0")]);
        assert_eq!(summary.explicit_texture_alpha_channels, 0);
        assert_eq!(summary.constant_texture_alpha_inputs, 1);
        assert_eq!(summary.resolved_material_alphas, 1);
    }

    #[test]
    fn rgb_channel_operand_remains_explicit_even_without_stored_alpha() {
        let summary = summarize_materials(&[material(0, "m0", 2, 0)], &[asset(1, "m0")]);
        assert_eq!(summary.explicit_texture_alpha_channels, 1);
        assert_eq!(summary.constant_texture_alpha_inputs, 0);
        assert_eq!(summary.resolved_material_alphas, 1);
    }

    #[test]
    fn dot_product_alpha_stage_is_reported_unresolved() {
        let mut material = material(0, "m0", 2, 6);
        material.alpha_stages[0].inputs.push(AlphaInput {
            input: 1,
            source_id: 3,
            operand_id: 2,
        });
        let summary = summarize_materials(&[material], &[asset(1, "m0")]);
        assert_eq!(summary.explicit_texture_alpha_channels, 2);
        assert_eq!(summary.resolved_material_alphas, 0);
        assert_eq!(summary.unresolved_material_alphas, 1);
    }

    #[test]
    fn missing_alpha_slot_without_exportable_channel_is_not_labeled_unresolved() {
        let mut material = material(1, "missing", 0, 0);
        material.insert_slot(0, "present".to_owned(), true);
        let summary = summarize_materials(&[material], &[asset(8, "present")]);
        assert_eq!(summary.materials_found, 1);
        assert_eq!(summary.explicit_texture_alpha_channels, 0);
        assert_eq!(summary.constant_texture_alpha_inputs, 0);
        assert_eq!(summary.resolved_material_alphas, 0);
        assert_eq!(summary.unresolved_material_alphas, 0);
    }
}
