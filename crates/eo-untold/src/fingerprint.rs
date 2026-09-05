use crate::{ParityAsset, ScanIssue, UntoldInventory};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const FINGERPRINT_SCHEMA: u32 = 1;
pub const FINGERPRINT_KIND: &str = "eo-texrip-structural-regression-fingerprint";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyStatement {
    pub contains_rom_bytes: bool,
    pub contains_decoded_pixels: bool,
    pub contains_source_paths: bool,
    pub contains_texture_or_model_names: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuralFingerprint {
    pub schema_version: u32,
    pub kind: String,
    pub game_id: String,
    pub title_id: String,
    pub product_code: String,
    pub asset_count: usize,
    pub asset_descriptor_sha256: String,
    pub candidate_hash_count: usize,
    pub verified_runtime_hash_count: usize,
    pub parser_counts: BTreeMap<String, u64>,
    pub format_counts: BTreeMap<String, u64>,
    pub dimension_counts: BTreeMap<String, u64>,
    pub category_counts: BTreeMap<String, u64>,
    pub material_bound_assets: usize,
    pub summary: BTreeMap<String, u64>,
    pub materials: BTreeMap<String, u64>,
    pub models: BTreeMap<String, u64>,
    pub privacy: PrivacyStatement,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FingerprintDifference {
    pub expected: Value,
    pub actual: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FingerprintComparison {
    #[serde(rename = "match")]
    pub matches: bool,
    pub differences: BTreeMap<String, FingerprintDifference>,
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalAssetDescriptor<'a> {
    // Keep fields in the same alphabetical order produced by Python's
    // json.dumps(sort_keys=True). All strings in this descriptor are ASCII.
    candidate_hash: &'a str,
    category: &'a str,
    format: i32,
    height: u32,
    material_binding_count: u64,
    mip: u8,
    parser_used: &'a str,
    verified_hashes: &'a [String],
    width: u32,
}

pub fn build_fingerprint(inventory: &UntoldInventory) -> StructuralFingerprint {
    let mut assets: Vec<&ParityAsset> = inventory.assets.iter().collect();
    assets.sort_by(|a, b| {
        (
            &a.candidate_hash,
            a.width,
            a.height,
            a.format,
            a.mip,
            &a.parser_used,
            &a.category,
            &a.verified_hashes,
        )
            .cmp(&(
                &b.candidate_hash,
                b.width,
                b.height,
                b.format,
                b.mip,
                &b.parser_used,
                &b.category,
                &b.verified_hashes,
            ))
    });

    let descriptors = assets
        .iter()
        .map(|asset| CanonicalAssetDescriptor {
            candidate_hash: &asset.candidate_hash,
            category: &asset.category,
            format: asset.format,
            height: asset.height,
            material_binding_count: asset.material_binding_count,
            mip: asset.mip,
            parser_used: &asset.parser_used,
            verified_hashes: &asset.verified_hashes,
            width: asset.width,
        })
        .collect::<Vec<_>>();
    let canonical = serde_json::to_string(&descriptors)
        .expect("serializing a parity descriptor cannot fail");
    let asset_descriptor_sha256 = format!("{:x}", Sha256::digest(canonical.as_bytes()));

    let mut parser_counts = BTreeMap::new();
    let mut format_counts = BTreeMap::new();
    let mut dimension_counts = BTreeMap::new();
    let mut category_counts = BTreeMap::new();
    for asset in &assets {
        increment(&mut parser_counts, asset.parser_used.clone());
        increment(&mut format_counts, asset.format.to_string());
        increment(
            &mut dimension_counts,
            format!("{}x{}", asset.width, asset.height),
        );
        increment(&mut category_counts, asset.category.clone());
    }

    // The frozen run_summary `issues` field is the count returned by
    // decode_strict_file. Archive extraction, native safety budgets, and probe
    // diagnostics are useful to expose to callers, but they are separate report
    // channels in the reference pipeline and must not inflate schema-1 parity.
    let mut summary = inventory.summary.as_fingerprint_map();
    summary.insert(
        "issues".to_owned(),
        frozen_decoder_issue_count(&inventory.issues),
    );

    StructuralFingerprint {
        schema_version: FINGERPRINT_SCHEMA,
        kind: FINGERPRINT_KIND.to_owned(),
        game_id: inventory.profile_id.clone(),
        title_id: inventory
            .title_id
            .clone()
            .unwrap_or_default()
            .to_ascii_uppercase(),
        product_code: inventory.product_code.clone().unwrap_or_default(),
        asset_count: assets.len(),
        asset_descriptor_sha256,
        candidate_hash_count: assets
            .iter()
            .filter(|asset| !asset.candidate_hash.is_empty())
            .count(),
        verified_runtime_hash_count: assets
            .iter()
            .map(|asset| asset.verified_hashes.len())
            .sum(),
        parser_counts,
        format_counts,
        dimension_counts,
        category_counts,
        material_bound_assets: assets
            .iter()
            .filter(|asset| asset.material_binding_count > 0)
            .count(),
        summary,
        materials: inventory.material_summary.as_fingerprint_map(),
        models: BTreeMap::from([
            ("payloads".to_owned(), inventory.model_payloads),
            ("cgfx_payloads".to_owned(), inventory.cgfx_payloads),
            ("bch_payloads".to_owned(), inventory.bch_payloads),
            (
                "bam2_bch_payloads".to_owned(),
                inventory.bam2_bch_payloads,
            ),
            ("models_found".to_owned(), inventory.summary.models_found),
            (
                "materials_found".to_owned(),
                inventory.summary.model_materials_found,
            ),
            (
                "texture_descriptors_found".to_owned(),
                inventory.texture_descriptors_found,
            ),
            (
                "decoded_3d_textures".to_owned(),
                inventory.decoded_3d_textures,
            ),
        ]),
        privacy: PrivacyStatement::default(),
    }
}

pub fn compare_fingerprints(
    expected: &StructuralFingerprint,
    actual: &StructuralFingerprint,
) -> FingerprintComparison {
    let expected = serde_json::to_value(expected).expect("fingerprint serialization cannot fail");
    let actual = serde_json::to_value(actual).expect("fingerprint serialization cannot fail");
    let keys = [
        "game_id",
        "title_id",
        "product_code",
        "asset_count",
        "asset_descriptor_sha256",
        "candidate_hash_count",
        "verified_runtime_hash_count",
        "parser_counts",
        "format_counts",
        "dimension_counts",
        "category_counts",
        "material_bound_assets",
        "summary",
        "materials",
        "models",
    ];
    let mut differences = BTreeMap::new();
    for key in keys {
        let expected_value = expected.get(key).cloned().unwrap_or(Value::Null);
        let actual_value = actual.get(key).cloned().unwrap_or(Value::Null);
        if expected_value != actual_value {
            differences.insert(
                key.to_owned(),
                FingerprintDifference {
                    expected: expected_value,
                    actual: actual_value,
                },
            );
        }
    }
    FingerprintComparison {
        matches: differences.is_empty(),
        differences,
    }
}

fn frozen_decoder_issue_count(issues: &[ScanIssue]) -> u64 {
    issues
        .iter()
        .filter(|issue| frozen_decoder_issue(issue))
        .count() as u64
}

fn frozen_decoder_issue(issue: &ScanIssue) -> bool {
    match issue.stage.as_str() {
        // Full-file read failures of non-archive candidates correspond to the
        // frozen decode_strict_file `read_error` path. HPI/HPB/FARC/EPL failures
        // are separately reported extraction diagnostics in the reference.
        "romfs_read" => !has_archive_extension(&issue.source),
        // push_asset records raw PICA decode diagnostics for all native texture
        // families. The frozen strict scanner counts this as a decoder issue for
        // STEX, but CGFX/BCH per-texture decode failures remain model diagnostics
        // unless they also cause a material-referenced texture to be missing.
        "texture_decode" => has_extension(&issue.source, "stex"),
        // Current native decoder/model stages map to the same failure families
        // emitted by the frozen strict decoder. Exact legacy labels are also
        // accepted so this filter remains stable as diagnostics are refined.
        "stex"
        | "stex_decode"
        | "stex_decode_error"
        | "cgfx"
        | "cgfx_model"
        | "cgfx_decode_error"
        | "cgfx_material_parse_error"
        | "cgfx_material_texture_missing"
        | "bch"
        | "bch_model"
        | "bch_decode_error"
        | "bch_material_parse_error"
        | "bch_material_texture_missing"
        | "container_decode_error" => true,
        _ => false,
    }
}

fn has_archive_extension(source: &str) -> bool {
    ["hpi", "hpb", "farc", "epl"]
        .into_iter()
        .any(|extension| has_extension(source, extension))
}

fn has_extension(source: &str, expected: &str) -> bool {
    let name = source.rsplit(['/', '\\']).next().unwrap_or(source);
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    extension.eq_ignore_ascii_case(expected)
}

fn increment(counts: &mut BTreeMap<String, u64>, key: String) {
    *counts.entry(key).or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MaterialParitySummary, ParitySummary, UntoldInventory};
    use eo_archives::ExtractionUsage;
    use eo_core::GameId;

    fn inventory_with_assets(assets: Vec<ParityAsset>) -> UntoldInventory {
        let material_summary = MaterialParitySummary {
            material_texture_bindings: 2,
            ..MaterialParitySummary::default()
        };
        UntoldInventory {
            profile_id: "eou1".to_owned(),
            game_id: GameId::EtrianOdysseyUntold,
            title_id: Some("00040000000ec700".to_owned()),
            product_code: Some("CTR-P-BSK-USA".to_owned()),
            romfs_files: 1,
            material_texture_bindings: material_summary.material_texture_bindings,
            material_summary,
            extraction_usage: ExtractionUsage::default(),
            summary: ParitySummary::default(),
            issues: Vec::new(),
            assets,
            model_payloads: 0,
            cgfx_payloads: 0,
            bch_payloads: 0,
            bam2_bch_payloads: 0,
            texture_descriptors_found: 0,
            decoded_3d_textures: 0,
        }
    }

    #[test]
    fn fingerprint_is_privacy_safe_and_order_independent() {
        let a = ParityAsset::test_fixture(
            "AAAAAAAAAAAAAAAA",
            8,
            8,
            8,
            "eou_stex_strict",
            "ui",
            2,
        );
        let b = ParityAsset::test_fixture(
            "BBBBBBBBBBBBBBBB",
            16,
            8,
            0,
            "cgfx_struct",
            "monsters",
            0,
        );
        let forward = build_fingerprint(&inventory_with_assets(vec![a.clone(), b.clone()]));
        let reverse = build_fingerprint(&inventory_with_assets(vec![b, a]));
        assert_eq!(
            forward.asset_descriptor_sha256,
            reverse.asset_descriptor_sha256
        );
        assert_eq!(forward.asset_count, 2);
        assert_eq!(forward.material_bound_assets, 1);
        assert_eq!(forward.title_id, "00040000000EC700");
        assert_eq!(forward.materials["material_texture_bindings"], 2);
        assert_eq!(forward.privacy, PrivacyStatement::default());
    }

    #[test]
    fn fingerprint_issues_exclude_archive_and_budget_diagnostics() {
        let mut inventory = inventory_with_assets(Vec::new());
        inventory.summary.issues = 6;
        inventory.issues = vec![
            ScanIssue {
                source: "data/pack.hpi".to_owned(),
                stage: "hpi_hpb_member".to_owned(),
                message: "diagnostic".to_owned(),
            },
            ScanIssue {
                source: "data/model.farc".to_owned(),
                stage: "archive_budget".to_owned(),
                message: "diagnostic".to_owned(),
            },
            ScanIssue {
                source: "models/enemy.bam".to_owned(),
                stage: "texture_decode".to_owned(),
                message: "per-texture diagnostic".to_owned(),
            },
            ScanIssue {
                source: "textures/broken.stex".to_owned(),
                stage: "texture_decode".to_owned(),
                message: "STEX decoder issue".to_owned(),
            },
            ScanIssue {
                source: "models/enemy.bam".to_owned(),
                stage: "cgfx_model".to_owned(),
                message: "decoder issue".to_owned(),
            },
            ScanIssue {
                source: "textures/ui.stex".to_owned(),
                stage: "stex".to_owned(),
                message: "decoder issue".to_owned(),
            },
        ];

        let fingerprint = build_fingerprint(&inventory);
        assert_eq!(fingerprint.summary["issues"], 3);
        assert_eq!(inventory.summary.issues, 6);
    }

    #[test]
    fn comparison_reports_only_structural_keys() {
        let mut expected = build_fingerprint(&inventory_with_assets(Vec::new()));
        let mut actual = expected.clone();
        actual.asset_count = 1;
        actual.privacy.contains_rom_bytes = true;
        let diff = compare_fingerprints(&expected, &actual);
        assert!(!diff.matches);
        assert_eq!(diff.differences.len(), 1);
        assert!(diff.differences.contains_key("asset_count"));
        expected.asset_count = 1;
        assert!(compare_fingerprints(&expected, &actual).matches);
    }
}
