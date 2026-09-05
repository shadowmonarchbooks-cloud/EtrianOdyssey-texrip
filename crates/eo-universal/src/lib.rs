//! Evidence-backed native inventory for Etrian Odyssey IV, V, and Nexus.
//!
//! This crate deliberately leaves the completed 0.60 Untold parity path alone.
//! It reuses the same bounded archive and texture parsers, but stages only the
//! archive members justified by the 0.70 reconnaissance matrix so the much larger
//! EO V/Nexus HPI archives do not require retaining every member in memory.

use eo_archives::{
    ArchiveMember, ArchiveParser, EplParser, ExtractionBudget, FarcParser, HpiHpbParser,
};
use eo_core::GameId;
use eo_profiles::detect_verified_profile;
use eo_rom::{RomError, RomReader};
use eo_textures::{
    bch::{parse_bch, parse_header as parse_bch_header},
    cgfx::parse_cgfx,
    ctpk::{is_ctpk, parse_ctpk, CtpkTextureType},
    stex::{is_stex, parse_stex},
    EncodedTexture, NativePicaDecoder, TextureDecoder,
};
use eo_untold::cityhash::cityhash64_hex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

pub const UNIVERSAL_INVENTORY_SCHEMA: &str = "eo-texrip-universal-native-inventory-v1";
const ROMFS_PROBE_BYTES: usize = 0x10_0000;
const MAX_EMBEDDED_CONTAINERS: usize = 64;
const HPI_HEADER_SIZE: usize = 0x18;
const HPI_ENTRY_SIZE: usize = 16;
const ACMP_MIN_HEADER_SIZE: usize = 0x20;
const REVERSE_LZ_HISTORY_SIZE: usize = 0x8000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniversalIssue {
    #[serde(skip_serializing)]
    pub source: String,
    pub stage: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniversalSummary {
    pub romfs_files: u64,
    pub direct_candidate_files: u64,
    pub hpi_hpb_pairs: u64,
    pub hpi_members_total: u64,
    pub hpi_members_selected: u64,
    pub hpi_members_read: u64,
    pub hpi_members_marked_compressed: u64,
    pub farc_archives: u64,
    pub farc_members: u64,
    pub epl_archives: u64,
    pub epl_members: u64,
    pub stex_files: u64,
    pub cgfx_payloads: u64,
    pub cgfx_textures: u64,
    pub bch_payloads: u64,
    pub bch_textures: u64,
    pub standard_ctpk_files: u64,
    pub standard_ctpk_textures: u64,
    pub standard_ctpk_2d_textures: u64,
    pub standard_ctpk_non_2d_textures: u64,
    pub unclassified_ctpk_extension_payloads: u64,
    pub unsupported_bcfnt_candidates: u64,
    pub unsupported_tmx_candidates: u64,
    pub unsupported_ttd_candidates: u64,
    pub unsupported_tgd_candidates: u64,
    pub decoded_before_dedup: u64,
    pub textures_after_dedup: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniversalAsset {
    pub candidate_hash: String,
    pub width: u32,
    pub height: u32,
    pub format: i32,
    pub parser_used: String,
    pub category: String,
    #[serde(skip_serializing)]
    pub source: String,
    #[serde(skip_serializing)]
    pub internal_name: String,
    #[serde(skip_serializing)]
    pub rgba8: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniversalInventory {
    pub profile_id: String,
    pub game_id: GameId,
    pub title_id: Option<String>,
    pub product_code: Option<String>,
    pub summary: UniversalSummary,
    pub issues: Vec<UniversalIssue>,
    pub assets: Vec<UniversalAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniversalInventoryReport {
    pub schema: String,
    pub profile_id: String,
    pub game_id: GameId,
    pub title_id: Option<String>,
    pub product_code: Option<String>,
    pub summary: UniversalSummary,
    pub issues_by_stage: BTreeMap<String, u64>,
    pub privacy: String,
}

impl UniversalInventory {
    pub fn privacy_safe_report(&self) -> UniversalInventoryReport {
        let mut issues_by_stage = BTreeMap::new();
        for issue in &self.issues {
            *issues_by_stage.entry(issue.stage.clone()).or_default() += 1;
        }
        UniversalInventoryReport {
            schema: UNIVERSAL_INVENTORY_SCHEMA.to_owned(),
            profile_id: self.profile_id.clone(),
            game_id: self.game_id,
            title_id: self.title_id.clone(),
            product_code: self.product_code.clone(),
            summary: self.summary.clone(),
            issues_by_stage,
            privacy: "Aggregate parser counts and issue stages only; no RomFS paths, archive member names, payload bytes, payload offsets, texture names, or content hashes are emitted."
                .to_owned(),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UniversalError {
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error(
        "ROM identity is not a verified 0.70 Universal EO target: title_id={title_id:?}, product_code={product_code:?}"
    )]
    UnsupportedGame {
        title_id: Option<String>,
        product_code: Option<String>,
    },
}

#[derive(Clone, Debug)]
struct VirtualFile {
    path: String,
    data: Vec<u8>,
    depth: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArchiveFlavor {
    Farc,
    Epl,
}

#[derive(Debug)]
struct ScanState {
    budget: ExtractionBudget,
    summary: UniversalSummary,
    issues: Vec<UniversalIssue>,
    assets: Vec<UniversalAsset>,
}

impl ScanState {
    fn new(budget: ExtractionBudget, romfs_files: u64) -> Self {
        Self {
            budget,
            summary: UniversalSummary {
                romfs_files,
                ..UniversalSummary::default()
            },
            issues: Vec::new(),
            assets: Vec::new(),
        }
    }

    fn issue(&mut self, source: &str, stage: &str, message: impl ToString) {
        self.issues.push(UniversalIssue {
            source: source.to_owned(),
            stage: stage.to_owned(),
            message: message.to_string(),
        });
    }
}

/// Build a native, parser-backed inventory for EO IV, EO V, or Nexus.
///
/// The privacy-safe report derived from the returned inventory omits proprietary
/// paths/names/bytes. The in-memory assets retain pixels and local provenance so
/// the next 0.70 slice can route them through the shared exporter.
pub fn inventory_reader<R: RomReader>(
    reader: &R,
    budget: ExtractionBudget,
) -> Result<UniversalInventory, UniversalError> {
    let hint = reader.identity_hint()?;
    let profile = detect_verified_profile(hint.title_id, hint.product_code.as_deref()).ok_or_else(|| {
        UniversalError::UnsupportedGame {
            title_id: hint.title_id.map(|value| value.to_string()),
            product_code: hint.product_code.clone(),
        }
    })?;
    if !matches!(
        profile.game_id,
        GameId::EtrianOdysseyIv | GameId::EtrianOdysseyV | GameId::EtrianOdysseyNexus
    ) {
        return Err(UniversalError::UnsupportedGame {
            title_id: hint.title_id.map(|value| value.to_string()),
            product_code: hint.product_code,
        });
    }

    let entries = reader.entries()?;
    let mut state = ScanState::new(budget, entries.len() as u64);
    let mut archive_queue = VecDeque::<(ArchiveFlavor, VirtualFile)>::new();
    let mut hpi_entries = BTreeMap::<String, String>::new();
    let mut hpb_entries = BTreeMap::<String, String>::new();

    for entry in &entries {
        let extension = extension(&entry.virtual_path).map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("hpi") => {
                hpi_entries.insert(pair_key(&entry.virtual_path), entry.virtual_path.clone());
                continue;
            }
            Some("hpb") => {
                hpb_entries.insert(pair_key(&entry.virtual_path), entry.virtual_path.clone());
                continue;
            }
            _ => {}
        }

        if !direct_candidate_path(&entry.virtual_path) && entry.size != 0 {
            let probe_len = usize::try_from(entry.size.min(ROMFS_PROBE_BYTES as u64))
                .unwrap_or(ROMFS_PROBE_BYTES);
            let probe = match reader.read_entry_prefix(&entry.virtual_path, probe_len) {
                Ok(value) => value,
                Err(error) => {
                    state.issue(&entry.virtual_path, "romfs_probe", error);
                    continue;
                }
            };
            if !known_payload_probe(&probe) {
                continue;
            }
        } else if !direct_candidate_path(&entry.virtual_path) {
            continue;
        }

        state.summary.direct_candidate_files = state.summary.direct_candidate_files.saturating_add(1);
        if entry.size > budget.max_archive_bytes {
            state.issue(
                &entry.virtual_path,
                "romfs_budget",
                format!("candidate file size {} exceeds {}", entry.size, budget.max_archive_bytes),
            );
            continue;
        }
        let data = match reader.read_entry(&entry.virtual_path) {
            Ok(value) => value,
            Err(error) => {
                state.issue(&entry.virtual_path, "romfs_read", error);
                continue;
            }
        };
        dispatch_file(
            VirtualFile {
                path: normalize_path(&entry.virtual_path),
                data,
                depth: 0,
            },
            &mut archive_queue,
            &mut state,
        );
    }

    for (key, hpi_path) in hpi_entries {
        let Some(hpb_path) = hpb_entries.get(&key) else {
            state.issue(&hpi_path, "hpi_pair", "matching HPB file was not found");
            continue;
        };
        state.summary.hpi_hpb_pairs = state.summary.hpi_hpb_pairs.saturating_add(1);
        let hpi = match reader.read_entry(&hpi_path) {
            Ok(value) => value,
            Err(error) => {
                state.issue(&hpi_path, "hpi_read", error);
                continue;
            }
        };
        let hpb = match reader.read_entry(hpb_path) {
            Ok(value) => value,
            Err(error) => {
                state.issue(hpb_path, "hpb_read", error);
                continue;
            }
        };
        expand_hpi_pair(&hpi_path, &hpi, &hpb, &mut archive_queue, &mut state);
    }

    while let Some((flavor, file)) = archive_queue.pop_front() {
        if file.depth > budget.max_depth {
            state.issue(&file.path, "archive_depth", "archive nesting exceeds configured limit");
            continue;
        }
        expand_archive(file, flavor, &mut archive_queue, &mut state);
    }

    state.assets = dedupe_assets(std::mem::take(&mut state.assets));
    state.summary.textures_after_dedup = state.assets.len() as u64;

    Ok(UniversalInventory {
        profile_id: profile.profile_id.to_owned(),
        game_id: profile.game_id,
        title_id: hint.title_id.map(|value| value.to_string()),
        product_code: hint.product_code,
        summary: state.summary,
        issues: state.issues,
        assets: state.assets,
    })
}

fn dispatch_file(
    file: VirtualFile,
    archive_queue: &mut VecDeque<(ArchiveFlavor, VirtualFile)>,
    state: &mut ScanState,
) {
    if file.data.get(..4) == Some(b"FARC") {
        archive_queue.push_back((ArchiveFlavor::Farc, file));
        return;
    }
    if has_extension(&file.path, "epl") || EplParser.probe(&file.data) {
        archive_queue.push_back((ArchiveFlavor::Epl, file));
        return;
    }
    scan_payload(&file.path, &file.data, state);
}

fn expand_archive(
    file: VirtualFile,
    flavor: ArchiveFlavor,
    archive_queue: &mut VecDeque<(ArchiveFlavor, VirtualFile)>,
    state: &mut ScanState,
) {
    let inventory = match flavor {
        ArchiveFlavor::Farc => {
            state.summary.farc_archives = state.summary.farc_archives.saturating_add(1);
            FarcParser.inspect(&file.data, state.budget)
        }
        ArchiveFlavor::Epl => {
            state.summary.epl_archives = state.summary.epl_archives.saturating_add(1);
            EplParser.inspect(&file.data, state.budget)
        }
    };
    let inventory = match inventory {
        Ok(value) => value,
        Err(error) => {
            state.issue(&file.path, "archive_inspect", error);
            return;
        }
    };
    match flavor {
        ArchiveFlavor::Farc => {
            state.summary.farc_members = state
                .summary
                .farc_members
                .saturating_add(inventory.members.len() as u64)
        }
        ArchiveFlavor::Epl => {
            state.summary.epl_members = state
                .summary
                .epl_members
                .saturating_add(inventory.members.len() as u64)
        }
    }

    for member in &inventory.members {
        if member.expanded_size.unwrap_or(member.stored_size) > state.budget.max_member_bytes {
            state.issue(&file.path, "archive_member_budget", "archive member exceeds configured limit");
            continue;
        }
        let data = match flavor {
            ArchiveFlavor::Farc => FarcParser.read_member(&file.data, member, state.budget),
            ArchiveFlavor::Epl => EplParser.read_member(&file.data, member, state.budget),
        };
        let data = match data {
            Ok(value) => value,
            Err(error) => {
                state.issue(&file.path, "archive_member", error);
                continue;
            }
        };
        let name = member
            .name
            .as_deref()
            .map(safe_component)
            .unwrap_or_else(|| format!("member_{:05}", member.index));
        dispatch_file(
            VirtualFile {
                path: format!("{}::{name}", file.path),
                data,
                depth: file.depth.saturating_add(1),
            },
            archive_queue,
            state,
        );
    }
}

fn expand_hpi_pair(
    source: &str,
    hpi: &[u8],
    hpb: &[u8],
    archive_queue: &mut VecDeque<(ArchiveFlavor, VirtualFile)>,
    state: &mut ScanState,
) {
    let parser = HpiHpbParser;
    let inventory = match parser.inspect(hpi, hpb, state.budget) {
        Ok(value) => value,
        Err(error) => {
            state.issue(source, "hpi_hpb_inspect", error);
            return;
        }
    };
    state.summary.hpi_members_total = state
        .summary
        .hpi_members_total
        .saturating_add(inventory.members.len() as u64);
    let compressed_flags = match hpi_compressed_flags(hpi) {
        Ok(value) => value,
        Err(message) => {
            state.issue(source, "hpi_index_flags", message);
            return;
        }
    };

    for member in &inventory.members {
        let name = member.name.as_deref().unwrap_or("");
        if !relevant_hpi_member(name) {
            continue;
        }
        state.summary.hpi_members_selected = state.summary.hpi_members_selected.saturating_add(1);
        let index = match usize::try_from(member.index) {
            Ok(value) => value,
            Err(_) => {
                state.issue(source, "hpi_member_index", "member index does not fit address space");
                continue;
            }
        };
        let compressed = compressed_flags.get(index).copied().unwrap_or(false);
        if compressed {
            state.summary.hpi_members_marked_compressed = state
                .summary
                .hpi_members_marked_compressed
                .saturating_add(1);
        }
        let data = match read_hpi_member_fast(hpb, member, compressed, state.budget) {
            Ok(value) => value,
            Err(message) => {
                state.issue(source, "hpi_member", message);
                continue;
            }
        };
        state.summary.hpi_members_read = state.summary.hpi_members_read.saturating_add(1);
        dispatch_file(
            VirtualFile {
                path: format!("{}::{}", normalize_path(source), safe_component(name)),
                data,
                depth: 1,
            },
            archive_queue,
            state,
        );
    }
}

fn scan_payload(path: &str, data: &[u8], state: &mut ScanState) {
    let ext = extension(path).map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("bcfnt") => state.summary.unsupported_bcfnt_candidates =
            state.summary.unsupported_bcfnt_candidates.saturating_add(1),
        Some("tmx") => state.summary.unsupported_tmx_candidates =
            state.summary.unsupported_tmx_candidates.saturating_add(1),
        Some("ttd") => state.summary.unsupported_ttd_candidates =
            state.summary.unsupported_ttd_candidates.saturating_add(1),
        Some("tgd") => state.summary.unsupported_tgd_candidates =
            state.summary.unsupported_tgd_candidates.saturating_add(1),
        _ => {}
    }

    if is_stex(data) {
        state.summary.stex_files = state.summary.stex_files.saturating_add(1);
        match parse_stex(data) {
            Ok(texture) => push_asset(
                path,
                texture.name.as_deref().unwrap_or(""),
                "stex_struct",
                &texture.encoded,
                state,
            ),
            Err(error) => state.issue(path, "stex", error),
        }
        return;
    }

    if is_ctpk(data) {
        scan_standard_ctpk(path, data, state);
        return;
    }

    let mut found_structural = false;
    let mut seen_cgfx = BTreeSet::new();
    for offset in magic_offsets(data, b"CGFX", MAX_EMBEDDED_CONTAINERS) {
        if !seen_cgfx.insert(offset) {
            continue;
        }
        let Some(payload) = data.get(offset..) else {
            continue;
        };
        let container = match parse_cgfx(payload) {
            Ok(value) => value,
            Err(_) => continue,
        };
        found_structural = true;
        state.summary.cgfx_payloads = state.summary.cgfx_payloads.saturating_add(1);
        state.summary.cgfx_textures = state
            .summary
            .cgfx_textures
            .saturating_add(container.textures.len() as u64);
        for texture in container.textures {
            push_asset(path, &texture.name, "cgfx_struct", &texture.encoded, state);
        }
    }

    let mut seen_bch = BTreeSet::new();
    for offset in magic_offsets(data, b"BCH\0", MAX_EMBEDDED_CONTAINERS) {
        if !seen_bch.insert(offset) {
            continue;
        }
        let Some(payload) = data.get(offset..) else {
            continue;
        };
        if parse_bch_header(payload).is_err() {
            continue;
        }
        let container = match parse_bch(payload) {
            Ok(value) => value,
            Err(error) => {
                state.issue(path, "bch", error);
                continue;
            }
        };
        found_structural = true;
        state.summary.bch_payloads = state.summary.bch_payloads.saturating_add(1);
        state.summary.bch_textures = state
            .summary
            .bch_textures
            .saturating_add(container.textures.len() as u64);
        for texture in container.textures {
            push_asset(path, &texture.name, "bch_struct", &texture.encoded, state);
        }
    }

    if ext.as_deref() == Some("ctpk") && !found_structural {
        state.summary.unclassified_ctpk_extension_payloads = state
            .summary
            .unclassified_ctpk_extension_payloads
            .saturating_add(1);
        state.issue(
            path,
            "unclassified_ctpk_extension_payload",
            "file/member uses .ctpk extension but payload is not structurally recognized as standard CTPK, CGFX, or BCH",
        );
    }

    if matches!(data.get(..4), Some(b"CTXB") | Some(b"ctxb") | Some(b"cmb ")) {
        state.issue(
            path,
            "unsupported_known_texture_container",
            "recognized texture/container magic does not yet have a 0.70 native adapter",
        );
    }
}

fn scan_standard_ctpk(path: &str, data: &[u8], state: &mut ScanState) {
    state.summary.standard_ctpk_files = state.summary.standard_ctpk_files.saturating_add(1);
    let container = match parse_ctpk(data) {
        Ok(value) => value,
        Err(error) => {
            state.issue(path, "ctpk", error);
            return;
        }
    };
    state.summary.standard_ctpk_textures = state
        .summary
        .standard_ctpk_textures
        .saturating_add(container.textures.len() as u64);
    for texture in container.textures {
        match texture.texture_type {
            CtpkTextureType::TwoDimensional => {
                state.summary.standard_ctpk_2d_textures = state
                    .summary
                    .standard_ctpk_2d_textures
                    .saturating_add(1);
                if let Some(encoded) = texture.encoded.as_ref() {
                    push_asset(
                        path,
                        texture.name.as_deref().unwrap_or(""),
                        "ctpk_struct",
                        encoded,
                        state,
                    );
                }
            }
            _ => {
                state.summary.standard_ctpk_non_2d_textures = state
                    .summary
                    .standard_ctpk_non_2d_textures
                    .saturating_add(1);
                state.issue(
                    path,
                    "ctpk_non_2d_texture",
                    "standard CTPK contains a non-2D texture object that is inventoried but not flattened",
                );
            }
        }
    }
}

fn push_asset(
    source: &str,
    internal_name: &str,
    parser_used: &str,
    encoded: &EncodedTexture,
    state: &mut ScanState,
) {
    let decoder = NativePicaDecoder;
    let decoded = match decoder.decode_base_level(encoded) {
        Ok(value) => value,
        Err(error) => {
            state.issue(source, "texture_decode", error);
            return;
        }
    };
    let payload = match encoded.runtime_hash_payload() {
        Ok(value) => value,
        Err(error) => {
            state.issue(source, "runtime_hash_payload", error);
            return;
        }
    };
    state.summary.decoded_before_dedup = state.summary.decoded_before_dedup.saturating_add(1);
    state.assets.push(UniversalAsset {
        candidate_hash: cityhash64_hex(payload),
        width: encoded.dimensions.visible_width,
        height: encoded.dimensions.visible_height,
        format: encoded.format as u8 as i32,
        parser_used: parser_used.to_owned(),
        category: category_for(&format!("{source}/{internal_name}")),
        source: source.to_owned(),
        internal_name: internal_name.to_owned(),
        rgba8: decoded.rgba8,
    });
}

fn dedupe_assets(assets: Vec<UniversalAsset>) -> Vec<UniversalAsset> {
    let mut output = Vec::new();
    let mut seen = BTreeMap::<(String, i32, u32, u32), usize>::new();
    for asset in assets {
        let key = (
            asset.candidate_hash.clone(),
            asset.format,
            asset.width,
            asset.height,
        );
        if seen.contains_key(&key) {
            continue;
        }
        seen.insert(key, output.len());
        output.push(asset);
    }
    output
}

fn direct_candidate_path(path: &str) -> bool {
    matches!(
        extension(path).map(str::to_ascii_lowercase).as_deref(),
        Some("stex")
            | Some("bch")
            | Some("bcres")
            | Some("bcmdl")
            | Some("bam")
            | Some("bam2")
            | Some("farc")
            | Some("epl")
            | Some("ep")
            | Some("ctpk")
            | Some("ctxb")
            | Some("cmb")
            | Some("bin")
            | Some("bcfnt")
            | Some("tmx")
            | Some("ttd")
            | Some("tgd")
    )
}

fn relevant_hpi_member(name: &str) -> bool {
    matches!(
        extension(name).map(str::to_ascii_lowercase).as_deref(),
        Some("stex")
            | Some("bch")
            | Some("bcres")
            | Some("bcmdl")
            | Some("bam")
            | Some("bam2")
            | Some("farc")
            | Some("epl")
            | Some("ctpk")
            | Some("ctxb")
            | Some("cmb")
            | Some("bcfnt")
            | Some("tmx")
            | Some("ttd")
            | Some("tgd")
    )
}

fn known_payload_probe(data: &[u8]) -> bool {
    matches!(
        data.get(..4),
        Some(b"STEX")
            | Some(b"CGFX")
            | Some(b"BCH\0")
            | Some(b"ATBC")
            | Some(b"BAM2")
            | Some(b"CTPK")
            | Some(b"CTXB")
            | Some(b"ctxb")
            | Some(b"cmb ")
            | Some(b"FARC")
    ) || data.windows(4).any(|window| matches!(window, b"CGFX" | b"BCH\0"))
}

fn hpi_compressed_flags(hpi: &[u8]) -> Result<Vec<bool>, String> {
    if hpi.len() < HPI_HEADER_SIZE || hpi.get(..4) != Some(b"HPIH") {
        return Err("HPI header is invalid".to_owned());
    }
    let unknown_count = usize::from(read_u16(hpi, 0x12).ok_or("HPI header is truncated")?);
    let file_count = usize::from(read_u16(hpi, 0x14).ok_or("HPI header is truncated")?);
    let file_table = HPI_HEADER_SIZE
        .checked_add(unknown_count.checked_mul(4).ok_or("HPI table overflow")?)
        .ok_or("HPI table overflow")?;
    let table_end = file_table
        .checked_add(file_count.checked_mul(HPI_ENTRY_SIZE).ok_or("HPI table overflow")?)
        .ok_or("HPI table overflow")?;
    if table_end > hpi.len() {
        return Err("HPI entry table is truncated".to_owned());
    }
    let mut flags = Vec::with_capacity(file_count);
    for index in 0..file_count {
        let offset = file_table + index * HPI_ENTRY_SIZE;
        flags.push(read_u32(hpi, offset + 12).ok_or("HPI entry is truncated")? != 0);
    }
    Ok(flags)
}

fn read_hpi_member_fast(
    hpb: &[u8],
    member: &ArchiveMember,
    compressed: bool,
    budget: ExtractionBudget,
) -> Result<Vec<u8>, String> {
    let start = usize::try_from(member.offset).map_err(|_| "HPB member offset overflow")?;
    let stored = usize::try_from(member.stored_size).map_err(|_| "HPB member size overflow")?;
    let end = start.checked_add(stored).ok_or("HPB member extent overflow")?;
    let bytes = hpb.get(start..end).ok_or("HPB member extent is outside the file")?;
    if !compressed {
        if member.stored_size > budget.max_member_bytes {
            return Err("HPB member exceeds configured size limit".to_owned());
        }
        return Ok(bytes.to_vec());
    }
    decompress_reverse_lz(bytes, budget)
}

fn decompress_reverse_lz(block: &[u8], budget: ExtractionBudget) -> Result<Vec<u8>, String> {
    if block.len() < ACMP_MIN_HEADER_SIZE {
        return Err("compressed HPB member is shorter than ACMP header".to_owned());
    }
    let compressed_size = usize::try_from(read_u32(block, 0x04).ok_or("ACMP header truncated")?)
        .map_err(|_| "ACMP compressed size overflow")?;
    let header_size = usize::try_from(read_u32(block, 0x08).ok_or("ACMP header truncated")?)
        .map_err(|_| "ACMP header size overflow")?;
    let decompressed_size = usize::try_from(read_u32(block, 0x10).ok_or("ACMP header truncated")?)
        .map_err(|_| "ACMP decompressed size overflow")?;
    if header_size < ACMP_MIN_HEADER_SIZE {
        return Err("ACMP header size is invalid".to_owned());
    }
    if decompressed_size as u64 > budget.max_member_bytes {
        return Err("ACMP output exceeds configured member limit".to_owned());
    }
    let total = header_size
        .checked_add(compressed_size)
        .ok_or("ACMP extent overflow")?;
    if total > block.len() || compressed_size < 8 {
        return Err("ACMP compressed payload is truncated".to_owned());
    }
    let compressed = &block[header_size..total];
    let trailer_offset = compressed.len() - 8;
    let packed = u32::from_le_bytes(
        compressed[trailer_offset..trailer_offset + 4]
            .try_into()
            .map_err(|_| "ACMP trailer truncated")?,
    );
    let decompressed_increase = usize::try_from(u32::from_le_bytes(
        compressed[trailer_offset + 4..trailer_offset + 8]
            .try_into()
            .map_err(|_| "ACMP trailer truncated")?,
    ))
    .map_err(|_| "ACMP trailer size overflow")?;
    let trailer_size = usize::from((packed >> 24) as u8);
    let trailer_compressed_size = usize::try_from(packed & 0x00ff_ffff)
        .map_err(|_| "ACMP trailer compressed size overflow")?;
    if trailer_size == 0 || trailer_size > compressed.len() {
        return Err("ACMP trailer size is invalid".to_owned());
    }
    let target = trailer_compressed_size
        .checked_add(decompressed_increase)
        .ok_or("ACMP target overflow")?;
    if target > decompressed_size {
        return Err("ACMP target exceeds declared output".to_owned());
    }

    let mut output = vec![0xaa; decompressed_size];
    let mut history = [0u8; REVERSE_LZ_HISTORY_SIZE];
    let mut history_index = 0usize;
    let mut written = 0usize;
    let mut input_offset = compressed.len() - trailer_size;
    let mut output_offset = output.len();

    fn read_back(compressed: &[u8], input_offset: &mut usize) -> Result<u8, String> {
        if *input_offset == 0 {
            return Err("ACMP reverse-LZ input exhausted".to_owned());
        }
        *input_offset -= 1;
        compressed
            .get(*input_offset)
            .copied()
            .ok_or_else(|| "ACMP reverse-LZ input is truncated".to_owned())
    }

    fn write_back(
        output: &mut [u8],
        history: &mut [u8; REVERSE_LZ_HISTORY_SIZE],
        output_offset: &mut usize,
        history_index: &mut usize,
        written: &mut usize,
        value: u8,
    ) -> Result<(), String> {
        if *output_offset == 0 {
            return Err("ACMP reverse-LZ output overflow".to_owned());
        }
        *output_offset -= 1;
        output[*output_offset] = value;
        history[*history_index] = value;
        *history_index = (*history_index + 1) & (REVERSE_LZ_HISTORY_SIZE - 1);
        *written += 1;
        Ok(())
    }

    while written < target && input_offset > 0 {
        let flags = read_back(compressed, &mut input_offset)?;
        for bit in (0..8).rev() {
            if written >= target {
                break;
            }
            if (flags >> bit) & 1 != 0 {
                let first = read_back(compressed, &mut input_offset)?;
                let count = usize::from(first >> 4) + 3;
                let second = read_back(compressed, &mut input_offset)?;
                let distance = ((usize::from(first & 0x0f) << 8) | usize::from(second)) + 3;
                for _ in 0..count {
                    let source = history_index.wrapping_sub(distance) & (REVERSE_LZ_HISTORY_SIZE - 1);
                    let value = history[source];
                    write_back(
                        &mut output,
                        &mut history,
                        &mut output_offset,
                        &mut history_index,
                        &mut written,
                        value,
                    )?;
                }
            } else {
                let value = read_back(compressed, &mut input_offset)?;
                write_back(
                    &mut output,
                    &mut history,
                    &mut output_offset,
                    &mut history_index,
                    &mut written,
                    value,
                )?;
            }
        }
    }
    while written < output.len() {
        let value = read_back(compressed, &mut input_offset)?;
        write_back(
            &mut output,
            &mut history,
            &mut output_offset,
            &mut history_index,
            &mut written,
            value,
        )?;
    }
    Ok(output)
}

fn magic_offsets(data: &[u8], magic: &[u8], limit: usize) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut cursor = 0usize;
    while cursor + magic.len() <= data.len() && offsets.len() < limit {
        let Some(relative) = data[cursor..]
            .windows(magic.len())
            .position(|window| window == magic)
        else {
            break;
        };
        let offset = cursor + relative;
        offsets.push(offset);
        cursor = offset.saturating_add(magic.len());
    }
    offsets
}

fn pair_key(path: &str) -> String {
    let normalized = normalize_path(path);
    normalized
        .rsplit_once('.')
        .map_or(normalized.clone(), |(stem, _)| stem.to_owned())
        .to_ascii_lowercase()
}

fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit(['/', '\\', ':']).find(|part| !part.is_empty())?;
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.trim();
    (!ext.is_empty()).then_some(ext)
}

fn has_extension(path: &str, expected: &str) -> bool {
    extension(path).is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn safe_component(value: &str) -> String {
    let leaf = value.replace('\\', "/");
    let leaf = leaf.rsplit('/').next().unwrap_or("").trim();
    let mut out = String::new();
    for ch in leaf.chars().take(96) {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "member".to_owned()
    } else {
        out
    }
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset.checked_add(2)?)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.checked_add(4)?)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
}

fn category_for(source: &str) -> String {
    const RULES: &[(&str, &[&str])] = &[
        ("characters", &["face", "portrait", "chara", "character", "npc", "pc_", "bust"]),
        ("monsters", &["enemy", "monster", "ene", "foe", "boss"]),
        ("ui", &["ui", "menu", "window", "frame", "cursor", "button", "layout"]),
        ("icons", &["icon", "item", "skill", "equip", "status"]),
        ("maps", &["map", "floor", "atlas", "compass"]),
        ("dungeon", &["dungeon", "mori", "labyrinth", "field", "wall", "ground", "bg3d"]),
        ("backgrounds", &["background", "back", "bg/", "eventbg", "town", "shop"]),
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
    use eo_core::TitleId;
    use eo_rom::{RomEntry, RomIdentityHint, RomImageKind, RomMetadata};

    #[derive(Clone)]
    struct FixtureRom {
        hint: RomIdentityHint,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl RomReader for FixtureRom {
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

    fn eo4_hint() -> RomIdentityHint {
        RomIdentityHint {
            title_id: Some("00040000000BD300".parse::<TitleId>().unwrap()),
            product_code: Some("CTR-P-ASJE".to_owned()),
        }
    }

    fn stex_a8(fill: u8) -> Vec<u8> {
        let mut data = vec![0u8; 0xc0];
        data[..4].copy_from_slice(b"STEX");
        data[0x0c..0x10].copy_from_slice(&8u32.to_le_bytes());
        data[0x10..0x14].copy_from_slice(&8u32.to_le_bytes());
        data[0x14..0x18].copy_from_slice(&0x1401u32.to_le_bytes());
        data[0x18..0x1c].copy_from_slice(&0x6756u32.to_le_bytes());
        data[0x1c..0x20].copy_from_slice(&64u32.to_le_bytes());
        data[0x20..0x24].copy_from_slice(&0x80u32.to_le_bytes());
        data[0x80..0xc0].fill(fill);
        data
    }

    #[test]
    fn eo4_direct_stex_is_native_inventory_evidence() {
        let rom = FixtureRom {
            hint: eo4_hint(),
            files: BTreeMap::from([("ui/direct.stex".to_owned(), stex_a8(0x11))]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.profile_id, "eo4");
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 1);
        assert_eq!(inventory.summary.textures_after_dedup, 1);
        assert!(inventory.issues.is_empty());
        let report = inventory.privacy_safe_report();
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("ui/direct.stex"));
        assert!(!json.contains("7ABCF0A736B8A12E"));
    }

    #[test]
    fn extension_only_ctpk_is_visible_but_not_misparsed() {
        let rom = FixtureRom {
            hint: eo4_hint(),
            files: BTreeMap::from([("mystery.ctpk".to_owned(), vec![0x55; 64])]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.standard_ctpk_files, 0);
        assert_eq!(inventory.summary.unclassified_ctpk_extension_payloads, 1);
        assert_eq!(inventory.issues[0].stage, "unclassified_ctpk_extension_payload");
    }

    #[test]
    fn untold_identity_stays_outside_universal_gate() {
        let rom = FixtureRom {
            hint: RomIdentityHint {
                title_id: Some("00040000000EC700".parse::<TitleId>().unwrap()),
                product_code: Some("CTR-P-BSKE".to_owned()),
            },
            files: BTreeMap::new(),
        };
        assert!(matches!(
            inventory_reader(&rom, ExtractionBudget::default()),
            Err(UniversalError::UnsupportedGame { .. })
        ));
    }
}
