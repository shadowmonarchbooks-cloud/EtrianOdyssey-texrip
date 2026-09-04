//! Native EOU1/EO2U orchestration and parity inventory.
//!
//! 0.50 proved the individual ROM, archive, texture-container, and model parsers.
//! 0.60 composes those bounded pieces into the first end-to-end Untold path. The
//! inventory deliberately mirrors the copyright-safe summary fields emitted by
//! the frozen Python reference, while keeping semantic classification out of this
//! layer until structural evidence supports it.

use eo_archives::{
    ArchiveParser, EplParser, ExtractionBudget, ExtractionUsage, FarcParser, HpiHpbParser,
};
use eo_core::GameId;
use eo_models::{BchModelInspector, CgfxModelInspector, ModelInspector};
use eo_profiles::detect_verified_profile;
use eo_rom::{RomError, RomReader};
use eo_textures::{
    bch::{parse_bch, parse_bch_wrapper},
    cgfx::{is_cgfx, parse_atbc, parse_cgfx},
    stex::{is_stex, parse_stex},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanIssue {
    pub source: String,
    pub stage: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParitySummary {
    pub strict_candidate_files: u64,
    pub decoded_before_dedup: u64,
    pub issues: u64,
    pub hpx_pairs: u64,
    pub hpx_files: u64,
    pub farc_archives: u64,
    pub farc_files: u64,
    pub epl_archives: u64,
    pub epl_files: u64,
    pub models_found: u64,
    pub model_materials_found: u64,
    pub stex_files: u64,
    pub atbc_files: u64,
    pub cgfx_files: u64,
    pub wrapped_bch_files: u64,
    pub bam_bch_files: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UntoldInventory {
    pub profile_id: String,
    pub game_id: GameId,
    pub title_id: Option<String>,
    pub product_code: Option<String>,
    pub romfs_files: u64,
    pub material_texture_bindings: u64,
    pub extraction_usage: ExtractionUsage,
    pub summary: ParitySummary,
    pub issues: Vec<ScanIssue>,
}

impl UntoldInventory {
    pub fn legacy_summary_projection(&self) -> ParitySummary {
        let mut summary = self.summary.clone();
        summary.issues = self.issues.len() as u64;
        summary
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UntoldError {
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("ROM identity is not a verified EOU1/EO2U profile: title_id={title_id:?}, product_code={product_code:?}")]
    UnsupportedGame {
        title_id: Option<String>,
        product_code: Option<String>,
    },
}

#[derive(Clone, Debug)]
struct VirtualFile {
    path: String,
    data: Vec<u8>,
}

#[derive(Debug)]
struct ScanState {
    budget: ExtractionBudget,
    usage: ExtractionUsage,
    summary: ParitySummary,
    material_texture_bindings: u64,
    issues: Vec<ScanIssue>,
}

impl ScanState {
    fn new(budget: ExtractionBudget) -> Self {
        Self {
            budget,
            usage: ExtractionUsage::default(),
            summary: ParitySummary::default(),
            material_texture_bindings: 0,
            issues: Vec::new(),
        }
    }

    fn issue(&mut self, source: &str, stage: &str, error: impl ToString) {
        self.issues.push(ScanIssue {
            source: source.to_owned(),
            stage: stage.to_owned(),
            message: error.to_string(),
        });
    }
}

/// Inspect a verified EOU1/EO2U ROM using only native Rust readers/parsers.
///
/// This first 0.60 path recursively expands paired HPI/HPB, FARC, and EPL data
/// in memory under the shared extraction budget. It does not write extracted
/// proprietary bytes to disk and does not broaden file-format guesses.
pub fn inventory_reader<R: RomReader>(
    reader: &R,
    budget: ExtractionBudget,
) -> Result<UntoldInventory, UntoldError> {
    let hint = reader.identity_hint()?;
    let profile = detect_verified_profile(hint.title_id, hint.product_code.as_deref()).ok_or_else(|| {
        UntoldError::UnsupportedGame {
            title_id: hint.title_id.map(|value| value.to_string()),
            product_code: hint.product_code.clone(),
        }
    })?;
    if !matches!(
        profile.game_id,
        GameId::EtrianOdysseyUntold | GameId::EtrianOdyssey2Untold
    ) {
        return Err(UntoldError::UnsupportedGame {
            title_id: hint.title_id.map(|value| value.to_string()),
            product_code: hint.product_code,
        });
    }

    let entries = reader.entries()?;
    let mut state = ScanState::new(budget);
    let mut files = Vec::new();
    for entry in &entries {
        if !candidate_path(&entry.virtual_path) {
            continue;
        }
        if entry.size > budget.max_archive_bytes {
            state.issue(
                &entry.virtual_path,
                "romfs_budget",
                format!(
                    "candidate file size {} exceeds archive read ceiling {}",
                    entry.size, budget.max_archive_bytes
                ),
            );
            continue;
        }
        match reader.read_entry(&entry.virtual_path) {
            Ok(data) => files.push(VirtualFile {
                path: normalize_virtual_path(&entry.virtual_path),
                data,
            }),
            Err(error) => state.issue(&entry.virtual_path, "romfs_read", error),
        }
    }

    scan_file_set(files, 0, &mut state);
    state.summary.issues = state.issues.len() as u64;

    Ok(UntoldInventory {
        profile_id: profile.profile_id.to_owned(),
        game_id: profile.game_id,
        title_id: hint.title_id.map(|value| value.to_string()),
        product_code: hint.product_code,
        romfs_files: entries.len() as u64,
        material_texture_bindings: state.material_texture_bindings,
        extraction_usage: state.usage,
        summary: state.summary,
        issues: state.issues,
    })
}

fn scan_file_set(files: Vec<VirtualFile>, depth: u16, state: &mut ScanState) {
    let mut by_path = BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        by_path.entry(path_key(&file.path)).or_insert(index);
    }

    let mut consumed = BTreeSet::new();
    for (index, file) in files.iter().enumerate() {
        if !has_extension(&file.path, "hpi") || consumed.contains(&index) {
            continue;
        }
        let partner_key = path_key(&replace_extension(&file.path, "hpb"));
        let Some(partner_index) = by_path.get(&partner_key).copied() else {
            continue;
        };
        consumed.insert(index);
        consumed.insert(partner_index);
        scan_hpi_pair(file, &files[partner_index], depth, state);
    }

    for (index, file) in files.into_iter().enumerate() {
        if consumed.contains(&index) || has_extension(&file.path, "hpb") {
            continue;
        }
        scan_single_file(file, depth, state);
    }
}

fn scan_hpi_pair(hpi: &VirtualFile, hpb: &VirtualFile, depth: u16, state: &mut ScanState) {
    state.summary.hpx_pairs += 1;
    let parser = HpiHpbParser;
    let inventory = match parser.inspect(&hpi.data, &hpb.data, state.budget) {
        Ok(value) => value,
        Err(error) => {
            state.issue(&hpi.path, "hpi_hpb_inspect", error);
            return;
        }
    };
    if let Err(error) = state.usage.charge_inventory(depth, &inventory, state.budget) {
        state.issue(&hpi.path, "archive_budget", error);
        return;
    }
    state.summary.hpx_files += inventory.members.len() as u64;

    let mut nested = Vec::new();
    for member in &inventory.members {
        match parser.read_member(&hpi.data, &hpb.data, member, state.budget) {
            Ok(data) => nested.push(VirtualFile {
                path: child_path(
                    &hpi.path,
                    member.name.as_deref().unwrap_or("unnamed_member.bin"),
                ),
                data,
            }),
            Err(error) => state.issue(&hpi.path, "hpi_hpb_member", error),
        }
    }
    if !nested.is_empty() {
        scan_file_set(nested, depth.saturating_add(1), state);
    }
}

fn scan_single_file(file: VirtualFile, depth: u16, state: &mut ScanState) {
    let farc = FarcParser;
    if farc.probe(&file.data) {
        state.summary.farc_archives += 1;
        scan_single_archive(file, depth, state, ArchiveFlavor::Farc);
        return;
    }

    let epl = EplParser;
    if epl.probe(&file.data) {
        state.summary.epl_archives += 1;
        scan_single_archive(file, depth, state, ArchiveFlavor::Epl);
        return;
    }

    scan_payload(&file.path, &file.data, state);
}

#[derive(Clone, Copy)]
enum ArchiveFlavor {
    Farc,
    Epl,
}

fn scan_single_archive(file: VirtualFile, depth: u16, state: &mut ScanState, flavor: ArchiveFlavor) {
    let inventory = match flavor {
        ArchiveFlavor::Farc => FarcParser.inspect(&file.data, state.budget),
        ArchiveFlavor::Epl => EplParser.inspect(&file.data, state.budget),
    };
    let inventory = match inventory {
        Ok(value) => value,
        Err(error) => {
            state.issue(&file.path, "archive_inspect", error);
            return;
        }
    };
    if let Err(error) = state.usage.charge_inventory(depth, &inventory, state.budget) {
        state.issue(&file.path, "archive_budget", error);
        return;
    }

    match flavor {
        ArchiveFlavor::Farc => state.summary.farc_files += inventory.members.len() as u64,
        ArchiveFlavor::Epl => state.summary.epl_files += inventory.members.len() as u64,
    }

    let mut nested = Vec::new();
    for member in &inventory.members {
        let result = match flavor {
            ArchiveFlavor::Farc => FarcParser.read_member(&file.data, member, state.budget),
            ArchiveFlavor::Epl => EplParser.read_member(&file.data, member, state.budget),
        };
        match result {
            Ok(data) => nested.push(VirtualFile {
                path: child_path(
                    &file.path,
                    member.name.as_deref().unwrap_or("unnamed_member.bin"),
                ),
                data,
            }),
            Err(error) => state.issue(&file.path, "archive_member", error),
        }
    }
    if !nested.is_empty() {
        scan_file_set(nested, depth.saturating_add(1), state);
    }
}

fn scan_payload(path: &str, data: &[u8], state: &mut ScanState) {
    let ext = extension(path).map(str::to_ascii_lowercase);
    let mut strict_candidate = false;

    if is_stex(data) {
        strict_candidate = true;
        state.summary.stex_files += 1;
        match parse_stex(data) {
            Ok(_) => state.summary.decoded_before_dedup += 1,
            Err(error) => state.issue(path, "stex", error),
        }
    }

    if is_cgfx(data) {
        strict_candidate = true;
        state.summary.cgfx_files += 1;
        match parse_cgfx(data) {
            Ok(container) => state.summary.decoded_before_dedup += container.textures.len() as u64,
            Err(error) => state.issue(path, "cgfx", error),
        }
    }

    if data.get(..4) == Some(b"ATBC") {
        state.summary.atbc_files += 1;
        match parse_atbc(data) {
            Ok(wrapper) => {
                if !wrapper.cgfx_payloads.is_empty() {
                    strict_candidate = true;
                }
                for payload in wrapper.cgfx_payloads {
                    let start = payload.offset as usize;
                    let end = start.saturating_add(payload.size as usize);
                    let Some(bytes) = data.get(start..end) else {
                        state.issue(path, "atbc_cgfx_bounds", "embedded CGFX extent is invalid");
                        continue;
                    };
                    match parse_cgfx(bytes) {
                        Ok(container) => {
                            state.summary.decoded_before_dedup += container.textures.len() as u64
                        }
                        Err(error) => state.issue(path, "atbc_cgfx", error),
                    }
                }
            }
            Err(error) => state.issue(path, "atbc", error),
        }
    }

    if matches!(data.get(..4), Some(b"ATBC") | Some(b"BAM2")) {
        match parse_bch_wrapper(data) {
            Ok(wrapper) => {
                if !wrapper.payloads.is_empty() {
                    strict_candidate = true;
                    state.summary.wrapped_bch_files += 1;
                    if matches!(ext.as_deref(), Some("bam") | Some("bam2")) {
                        state.summary.bam_bch_files += 1;
                    }
                }
                for index in 0..wrapper.payloads.len() {
                    match wrapper.parse_payload(data, index) {
                        Ok(container) => {
                            state.summary.decoded_before_dedup += container.textures.len() as u64
                        }
                        Err(error) => state.issue(path, "wrapped_bch", error),
                    }
                }
            }
            Err(error) => state.issue(path, "bch_wrapper", error),
        }
    } else if data.get(..4) == Some(b"BCH\0") {
        strict_candidate = true;
        match parse_bch(data) {
            Ok(container) => state.summary.decoded_before_dedup += container.textures.len() as u64,
            Err(error) => state.issue(path, "bch", error),
        }
    }

    let cgfx_inspector = CgfxModelInspector;
    if cgfx_inspector.probe(data) {
        strict_candidate = true;
        match cgfx_inspector.inspect(data) {
            Ok(inventory) => {
                state.summary.models_found += 1;
                state.summary.model_materials_found += inventory.materials.len() as u64;
                state.material_texture_bindings += inventory
                    .materials
                    .iter()
                    .map(|material| material.textures.len() as u64)
                    .sum::<u64>();
            }
            Err(error) => state.issue(path, "cgfx_model", error),
        }
    }

    let bch_inspector = BchModelInspector;
    if bch_inspector.probe(data) {
        strict_candidate = true;
        match bch_inspector.inspect(data) {
            Ok(inventory) => {
                state.summary.models_found += 1;
                state.summary.model_materials_found += inventory.materials.len() as u64;
                state.material_texture_bindings += inventory
                    .materials
                    .iter()
                    .map(|material| material.textures.len() as u64)
                    .sum::<u64>();
            }
            Err(error) => state.issue(path, "bch_model", error),
        }
    }

    if strict_candidate {
        state.summary.strict_candidate_files += 1;
    }
}

fn candidate_path(path: &str) -> bool {
    let Some(ext) = extension(path).map(str::to_ascii_lowercase) else {
        return false;
    };
    matches!(
        ext.as_str(),
        "hpi"
            | "hpb"
            | "stex"
            | "bch"
            | "bcres"
            | "bcmdl"
            | "cmb"
            | "ctpk"
            | "ctxb"
            | "bam"
            | "bam2"
            | "atbc"
            | "farc"
            | "epl"
            | "model"
            | "bin"
    )
}

fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.trim();
    if ext.is_empty() {
        None
    } else {
        Some(ext)
    }
}

fn has_extension(path: &str, expected: &str) -> bool {
    extension(path).is_some_and(|ext| ext.eq_ignore_ascii_case(expected))
}

fn path_key(path: &str) -> String {
    normalize_virtual_path(path).to_ascii_lowercase()
}

fn normalize_virtual_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn replace_extension(path: &str, new_extension: &str) -> String {
    let normalized = normalize_virtual_path(path);
    match normalized.rfind('.') {
        Some(index) => format!("{}.{}", &normalized[..index], new_extension),
        None => format!("{normalized}.{new_extension}"),
    }
}

fn child_path(parent: &str, child: &str) -> String {
    let parent = normalize_virtual_path(parent);
    let child = normalize_virtual_path(child).trim_start_matches('/').to_owned();
    format!("{parent}/{child}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_rom::{RomEntry, RomIdentityHint, RomImageKind, RomMetadata};

    #[derive(Clone)]
    struct FakeRom {
        hint: RomIdentityHint,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl RomReader for FakeRom {
        fn metadata(&self) -> Result<RomMetadata, RomError> {
            Ok(RomMetadata {
                kind: RomImageKind::ExtractedRomFs,
                game: None,
                decrypted: true,
            })
        }

        fn identity_hint(&self) -> Result<RomIdentityHint, RomError> {
            Ok(self.hint.clone())
        }

        fn entries(&self) -> Result<Vec<RomEntry>, RomError> {
            Ok(self
                .files
                .iter()
                .map(|(path, data)| RomEntry {
                    virtual_path: path.clone(),
                    size: data.len() as u64,
                })
                .collect())
        }

        fn read_entry(&self, virtual_path: &str) -> Result<Vec<u8>, RomError> {
            self.files
                .get(virtual_path)
                .cloned()
                .ok_or_else(|| RomError::MissingEntry(virtual_path.to_owned()))
        }
    }

    fn eou1_hint() -> RomIdentityHint {
        RomIdentityHint {
            title_id: Some("00040000000EC700".parse().unwrap()),
            product_code: Some("CTR-P-BSK-USA".to_owned()),
        }
    }

    fn stex_a8() -> Vec<u8> {
        let mut data = vec![0u8; 0xc0];
        data[..4].copy_from_slice(b"STEX");
        data[0x0c..0x10].copy_from_slice(&8u32.to_le_bytes());
        data[0x10..0x14].copy_from_slice(&8u32.to_le_bytes());
        data[0x14..0x18].copy_from_slice(&0x1401u32.to_le_bytes());
        data[0x18..0x1c].copy_from_slice(&0x6756u32.to_le_bytes());
        data[0x1c..0x20].copy_from_slice(&64u32.to_le_bytes());
        data[0x20..0x24].copy_from_slice(&0x80u32.to_le_bytes());
        data
    }

    fn hpi_for_uncompressed(name: &str, payload_size: usize) -> Vec<u8> {
        let name = name.as_bytes();
        let mut hpi = vec![0u8; 0x28 + name.len() + 1];
        hpi[..4].copy_from_slice(b"HPIH");
        hpi[0x12..0x14].copy_from_slice(&0u16.to_le_bytes());
        hpi[0x14..0x16].copy_from_slice(&1u16.to_le_bytes());
        hpi[0x18..0x1c].copy_from_slice(&0u32.to_le_bytes());
        hpi[0x1c..0x20].copy_from_slice(&0u32.to_le_bytes());
        hpi[0x20..0x24].copy_from_slice(&(payload_size as u32).to_le_bytes());
        hpi[0x24..0x28].copy_from_slice(&0u32.to_le_bytes());
        hpi[0x28..0x28 + name.len()].copy_from_slice(name);
        hpi
    }

    #[test]
    fn direct_stex_projects_reference_summary_fields() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("tex/ui.stex".to_owned(), stex_a8())]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.profile_id, "eou1");
        assert_eq!(inventory.romfs_files, 1);
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 1);
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn hpi_hpb_members_are_scanned_recursively_without_disk_extraction() {
        let payload = stex_a8();
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([
                (
                    "DATA/PACK.HPI".to_owned(),
                    hpi_for_uncompressed("nested.stex", payload.len()),
                ),
                ("data/pack.hpb".to_owned(), payload),
            ]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.hpx_pairs, 1);
        assert_eq!(inventory.summary.hpx_files, 1);
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 1);
        assert_eq!(inventory.extraction_usage.members, 1);
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn unknown_identity_is_not_forced_through_untold_pipeline() {
        let rom = FakeRom {
            hint: RomIdentityHint {
                title_id: Some("0004000000000001".parse().unwrap()),
                product_code: Some("CTR-P-UNKNOWN".to_owned()),
            },
            files: BTreeMap::new(),
        };
        assert!(matches!(
            inventory_reader(&rom, ExtractionBudget::default()),
            Err(UntoldError::UnsupportedGame { .. })
        ));
    }

    #[test]
    fn malformed_stex_remains_a_counted_candidate_and_reported_issue() {
        let mut malformed = stex_a8();
        malformed.truncate(0x90);
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("bad.stex".to_owned(), malformed)]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 0);
        assert_eq!(inventory.summary.issues, 1);
    }

    #[test]
    fn oversized_candidate_is_reported_before_rom_reader_allocation() {
        struct OversizedRom;
        impl RomReader for OversizedRom {
            fn metadata(&self) -> Result<RomMetadata, RomError> {
                unreachable!()
            }
            fn identity_hint(&self) -> Result<RomIdentityHint, RomError> {
                Ok(eou1_hint())
            }
            fn entries(&self) -> Result<Vec<RomEntry>, RomError> {
                Ok(vec![RomEntry {
                    virtual_path: "huge.bin".to_owned(),
                    size: 100,
                }])
            }
            fn read_entry(&self, _virtual_path: &str) -> Result<Vec<u8>, RomError> {
                panic!("oversized candidate must be rejected before read_entry")
            }
        }

        let budget = ExtractionBudget {
            max_archive_bytes: 99,
            ..ExtractionBudget::default()
        };
        let inventory = inventory_reader(&OversizedRom, budget).unwrap();
        assert_eq!(inventory.summary.issues, 1);
        assert_eq!(inventory.issues[0].stage, "romfs_budget");
    }
}
