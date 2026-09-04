//! Native EOU1/EO2U orchestration and structural parity inventory.
//!
//! 0.50 proved the individual ROM, archive, texture-container, and model parsers.
//! 0.60 composes those bounded pieces into an end-to-end Untold path and emits a
//! copyright-safe fingerprint compatible with the frozen Python reference.

mod asset;
pub mod cityhash;
pub mod fingerprint;
mod material;

pub use asset::ParityAsset;
pub use fingerprint::{
    build_fingerprint, compare_fingerprints, FingerprintComparison, FingerprintDifference,
    PrivacyStatement, StructuralFingerprint,
};
pub use material::MaterialParitySummary;

use asset::{bind_external_texture_names, dedupe_assets};
use eo_archives::{
    ArchiveParser, EplParser, ExtractionBudget, ExtractionUsage, FarcParser, HpiHpbParser,
};
use eo_core::GameId;
use eo_models::{BchModelInspector, CgfxModelInspector, ModelInspector, ModelInventory};
use eo_profiles::detect_verified_profile;
use eo_rom::{RomError, RomReader};
use eo_textures::{
    bch::{parse_bch, parse_header as parse_bch_header, BchContainer},
    cgfx::{parse_cgfx, CgfxContainer},
    stex::{is_stex, parse_stex},
    EncodedTexture, NativePicaDecoder, TextureDecoder,
};
use material::{summarize_materials, ParityMaterial};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const ROMFS_PROBE_BYTES: usize = 0x10_0000;
const INVENTORY_PROBE_BYTES: usize = 0x2_0000;
const WRAPPED_BCH_PROBE_BYTES: usize = 0x1_0000;
const MAX_EMBEDDED_CGFX: usize = 16;
const MAX_EMBEDDED_BCH: usize = 8;

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

impl ParitySummary {
    pub(crate) fn as_fingerprint_map(&self) -> BTreeMap<String, u64> {
        BTreeMap::from([
            ("strict_candidate_files".to_owned(), self.strict_candidate_files),
            ("decoded_before_dedup".to_owned(), self.decoded_before_dedup),
            ("issues".to_owned(), self.issues),
            ("hpx_pairs".to_owned(), self.hpx_pairs),
            ("hpx_files".to_owned(), self.hpx_files),
            ("farc_archives".to_owned(), self.farc_archives),
            ("farc_files".to_owned(), self.farc_files),
            ("epl_archives".to_owned(), self.epl_archives),
            ("epl_files".to_owned(), self.epl_files),
            ("models_found".to_owned(), self.models_found),
            ("model_materials_found".to_owned(), self.model_materials_found),
            ("stex_files".to_owned(), self.stex_files),
            ("atbc_files".to_owned(), self.atbc_files),
            ("cgfx_files".to_owned(), self.cgfx_files),
            ("wrapped_bch_files".to_owned(), self.wrapped_bch_files),
            ("bam_bch_files".to_owned(), self.bam_bch_files),
        ])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UntoldInventory {
    pub profile_id: String,
    pub game_id: GameId,
    pub title_id: Option<String>,
    pub product_code: Option<String>,
    pub romfs_files: u64,
    /// Compatibility mirror of `material_summary.material_texture_bindings`.
    pub material_texture_bindings: u64,
    pub material_summary: MaterialParitySummary,
    pub extraction_usage: ExtractionUsage,
    pub summary: ParitySummary,
    pub issues: Vec<ScanIssue>,
    pub assets: Vec<ParityAsset>,
    pub model_payloads: u64,
    pub cgfx_payloads: u64,
    pub bch_payloads: u64,
    pub bam2_bch_payloads: u64,
    pub texture_descriptors_found: u64,
    pub decoded_3d_textures: u64,
}

impl UntoldInventory {
    pub fn legacy_summary_projection(&self) -> ParitySummary {
        let mut summary = self.summary.clone();
        summary.issues = self.issues.len() as u64;
        summary
    }

    pub fn structural_fingerprint(&self) -> StructuralFingerprint {
        build_fingerprint(self)
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
    issues: Vec<ScanIssue>,
    assets: Vec<ParityAsset>,
    materials: Vec<ParityMaterial>,
    bindings_by_name: BTreeMap<String, BTreeSet<String>>,
    model_payloads: u64,
    cgfx_payloads: u64,
    bch_payloads: u64,
    bam2_bch_payloads: u64,
    texture_descriptors_found: u64,
    decoded_3d_textures: u64,
}

impl ScanState {
    fn new(budget: ExtractionBudget) -> Self {
        Self {
            budget,
            usage: ExtractionUsage::default(),
            summary: ParitySummary::default(),
            issues: Vec::new(),
            assets: Vec::new(),
            materials: Vec::new(),
            bindings_by_name: BTreeMap::new(),
            model_payloads: 0,
            cgfx_payloads: 0,
            bch_payloads: 0,
            bam2_bch_payloads: 0,
            texture_descriptors_found: 0,
            decoded_3d_textures: 0,
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
/// HPI/HPB, FARC, and EPL are recursively expanded in memory under the shared
/// extraction budget. No extracted proprietary bytes, paths, or model/texture
/// names are written into the structural fingerprint.
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
        let selected = if candidate_path(&entry.virtual_path) {
            true
        } else if entry.size == 0 {
            false
        } else {
            let probe_len = usize::try_from(entry.size.min(ROMFS_PROBE_BYTES as u64))
                .unwrap_or(ROMFS_PROBE_BYTES);
            match reader.read_entry_prefix(&entry.virtual_path, probe_len) {
                Ok(probe) => romfs_probe_candidate(&probe),
                Err(error) => {
                    state.issue(&entry.virtual_path, "romfs_probe", error);
                    continue;
                }
            }
        };
        if !selected {
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
    let mut assets = dedupe_assets(state.assets);
    bind_external_texture_names(&mut assets, &state.bindings_by_name);
    let material_summary = summarize_materials(&state.materials, &assets);
    let material_texture_bindings = material_summary.material_texture_bindings;
    state.summary.issues = state.issues.len() as u64;

    Ok(UntoldInventory {
        profile_id: profile.profile_id.to_owned(),
        game_id: profile.game_id,
        title_id: hint.title_id.map(|value| value.to_string()),
        product_code: hint.product_code,
        romfs_files: entries.len() as u64,
        material_texture_bindings,
        material_summary,
        extraction_usage: state.usage,
        summary: state.summary,
        issues: state.issues,
        assets,
        model_payloads: state.model_payloads,
        cgfx_payloads: state.cgfx_payloads,
        bch_payloads: state.bch_payloads,
        bam2_bch_payloads: state.bam2_bch_payloads,
        texture_descriptors_found: state.texture_descriptors_found,
        decoded_3d_textures: state.decoded_3d_textures,
    })
}

fn scan_file_set(files: Vec<VirtualFile>, depth: u16, state: &mut ScanState) {
    for file in &files {
        inventory_file(file, state);
        if !matches!(extension(&file.path).map(str::to_ascii_lowercase).as_deref(), Some("hpi") | Some("hpb"))
            && strict_texture_signature(&file.path, prefix(&file.data, ROMFS_PROBE_BYTES))
        {
            state.summary.strict_candidate_files += 1;
        }
    }

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

fn inventory_file(file: &VirtualFile, state: &mut ScanState) {
    let probe = prefix(&file.data, INVENTORY_PROBE_BYTES);
    let ext = extension(&file.path).map(str::to_ascii_lowercase);
    let has_bch = contains_magic(probe, b"BCH\0");

    if probe.get(..4) == Some(b"STEX") {
        state.summary.stex_files += 1;
    }
    if probe.get(..4) == Some(b"ATBC") {
        state.summary.atbc_files += 1;
    }
    if has_cgfx_probe(probe) {
        state.summary.cgfx_files += 1;
    }
    if has_bch && probe.get(..4) != Some(b"BCH\0") {
        state.summary.wrapped_bch_files += 1;
    }
    if has_bch && matches!(ext.as_deref(), Some("bam") | Some("bam2")) {
        state.summary.bam_bch_files += 1;
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
    if is_stex(data) {
        match parse_stex(data) {
            Ok(texture) => push_asset(
                path,
                texture.name.as_deref().unwrap_or(""),
                "eou_stex_strict",
                &texture.encoded,
                BTreeSet::new(),
                false,
                state,
            ),
            Err(error) => state.issue(path, "stex", error),
        }
        return;
    }

    let before_assets = state.assets.len();
    for (offset, size) in cgfx_payload_extents(data) {
        let Some(end) = offset.checked_add(size) else {
            state.issue(path, "cgfx_bounds", "embedded CGFX extent overflow");
            continue;
        };
        let Some(bytes) = data.get(offset..end) else {
            state.issue(path, "cgfx_bounds", "embedded CGFX extent is invalid");
            continue;
        };
        scan_cgfx_payload(path, bytes, offset as u64, state);
    }

    let bam2 = matches!(
        extension(path).map(str::to_ascii_lowercase).as_deref(),
        Some("bam") | Some("bam2")
    );
    for offset in embedded_bch_offsets(data) {
        let Some(bytes) = data.get(offset..) else {
            state.issue(path, "bch_bounds", "embedded BCH offset is invalid");
            continue;
        };
        scan_bch_payload(path, bytes, offset as u64, bam2, state);
    }

    if state.assets.len() == before_assets
        && matches!(
            data.get(..4),
            Some(b"CTPK") | Some(b"CTXB") | Some(b"ctxb") | Some(b"cmb ")
        )
    {
        state.issue(
            path,
            "unsupported_parity_container",
            "legacy reference recognizes this container but native parity support is not implemented yet",
        );
    }
}

fn scan_cgfx_payload(path: &str, data: &[u8], container_offset: u64, state: &mut ScanState) {
    let container = match parse_cgfx(data) {
        Ok(value) => value,
        Err(error) => {
            state.issue(path, "cgfx", error);
            return;
        }
    };
    state.model_payloads += 1;
    state.cgfx_payloads += 1;
    state.texture_descriptors_found += container.textures.len() as u64;

    let local_bindings = inspect_model(
        path,
        "cgfx",
        container_offset,
        CgfxModelInspector.inspect(data),
        state,
    );
    add_cgfx_assets(path, &container, &local_bindings, state);
}

fn scan_bch_payload(
    path: &str,
    data: &[u8],
    container_offset: u64,
    bam2: bool,
    state: &mut ScanState,
) {
    let container = match parse_bch(data) {
        Ok(value) => value,
        Err(error) => {
            state.issue(path, "bch", error);
            return;
        }
    };
    state.model_payloads += 1;
    state.bch_payloads += 1;
    if bam2 {
        state.bam2_bch_payloads += 1;
    }
    state.texture_descriptors_found += container.textures.len() as u64;

    let local_bindings = inspect_model(
        path,
        "bch",
        container_offset,
        BchModelInspector.inspect(data),
        state,
    );
    add_bch_assets(path, &container, &local_bindings, state);
}

fn inspect_model(
    path: &str,
    format: &str,
    container_offset: u64,
    result: Result<ModelInventory, eo_models::ModelError>,
    state: &mut ScanState,
) -> BTreeMap<String, BTreeSet<String>> {
    let inventory = match result {
        Ok(value) => value,
        Err(error) => {
            state.issue(path, &format!("{format}_model"), error);
            return BTreeMap::new();
        }
    };
    state.summary.models_found += u64::from(inventory.model_count);
    state.summary.model_materials_found += inventory.materials.len() as u64;

    let model_name = inventory.model_name.as_deref().unwrap_or("");
    let mut local = BTreeMap::<String, BTreeSet<String>>::new();
    for material in inventory.materials {
        let material_name = material.name.as_deref().unwrap_or("");
        let mut parity_material = ParityMaterial {
            slots: BTreeMap::new(),
            alpha_stages: material.alpha_stages.clone(),
        };
        for texture in material.textures {
            let key = format!(
                "{path}|{format}|{container_offset}|{}|{model_name}|{}|{material_name}|{}|{}|{}",
                material.model_index,
                material.model_material_index,
                texture.slot,
                texture.enabled,
                texture.internal_name
            );
            parity_material.insert_slot(texture.slot, key.clone(), texture.enabled);
            local
                .entry(texture.internal_name.clone())
                .or_default()
                .insert(key.clone());
            state
                .bindings_by_name
                .entry(texture.internal_name)
                .or_default()
                .insert(key);
        }
        state.materials.push(parity_material);
    }
    local
}

fn add_cgfx_assets(
    path: &str,
    container: &CgfxContainer,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    state: &mut ScanState,
) {
    for texture in &container.textures {
        push_asset(
            path,
            &texture.name,
            "cgfx_struct",
            &texture.encoded,
            bindings.get(&texture.name).cloned().unwrap_or_default(),
            true,
            state,
        );
    }
}

fn add_bch_assets(
    path: &str,
    container: &BchContainer,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    state: &mut ScanState,
) {
    for texture in &container.textures {
        push_asset(
            path,
            &texture.name,
            "bch_struct",
            &texture.encoded,
            bindings.get(&texture.name).cloned().unwrap_or_default(),
            true,
            state,
        );
    }
}

fn push_asset(
    path: &str,
    internal_name: &str,
    parser_used: &str,
    encoded: &EncodedTexture,
    binding_keys: BTreeSet<String>,
    model_texture: bool,
    state: &mut ScanState,
) {
    let decoder = NativePicaDecoder;
    if let Err(error) = decoder.decode_base_level(encoded) {
        state.issue(path, "texture_decode", error);
        return;
    }
    state.summary.decoded_before_dedup += 1;
    if model_texture {
        state.decoded_3d_textures += 1;
    }
    state.assets.push(ParityAsset::from_encoded(
        path,
        internal_name,
        parser_used,
        encoded,
        binding_keys,
    ));
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
            | "farc"
            | "epl"
    )
}

fn romfs_probe_candidate(probe: &[u8]) -> bool {
    if matches!(
        probe.get(..4),
        Some(b"STEX")
            | Some(b"BCH\0")
            | Some(b"CGFX")
            | Some(b"ATBC")
            | Some(b"CTPK")
            | Some(b"CTXB")
            | Some(b"ctxb")
            | Some(b"cmb ")
            | Some(b"FARC")
    ) {
        return true;
    }
    contains_magic(probe, b"BCH\0") || has_cgfx_probe(prefix(probe, INVENTORY_PROBE_BYTES))
}

fn strict_texture_signature(path: &str, probe: &[u8]) -> bool {
    if is_stex(probe) {
        return true;
    }
    match probe.get(..4) {
        Some(b"ATBC") => {
            return has_cgfx_probe(probe) || contains_magic(probe, b"BCH\0");
        }
        Some(b"BCH\0")
        | Some(b"CGFX")
        | Some(b"CTPK")
        | Some(b"CTXB")
        | Some(b"ctxb")
        | Some(b"cmb ") => return true,
        _ => {}
    }

    let ext = extension(path).map(str::to_ascii_lowercase);
    let extension_qualified = matches!(
        ext.as_deref(),
        Some("bam")
            | Some("bam2")
            | Some("bch")
            | Some("bcres")
            | Some("bcmdl")
            | Some("cmb")
            | Some("model")
            | Some("bin")
            | Some("stex")
            | Some("ctpk")
            | Some("ctxb")
    );
    if extension_qualified
        && contains_magic(prefix(probe, INVENTORY_PROBE_BYTES), b"BCH\0")
    {
        return true;
    }
    contains_magic(prefix(probe, WRAPPED_BCH_PROBE_BYTES), b"BCH\0")
}

fn cgfx_payload_extents(data: &[u8]) -> Vec<(usize, usize)> {
    let mut payloads = Vec::new();
    let mut search = 0usize;
    while search + 4 <= data.len() && payloads.len() < MAX_EMBEDDED_CGFX {
        let Some(relative) = find_magic(&data[search..], b"CGFX") else {
            break;
        };
        let offset = search + relative;
        search = offset.saturating_add(4);
        let Some(size) = cgfx_declared_size(data, offset, false) else {
            continue;
        };
        payloads.push((offset, size));
    }
    payloads
}

fn has_cgfx_probe(data: &[u8]) -> bool {
    let mut search = 0usize;
    let mut found = 0usize;
    while search + 4 <= data.len() && found < MAX_EMBEDDED_CGFX {
        let Some(relative) = find_magic(&data[search..], b"CGFX") else {
            break;
        };
        let offset = search + relative;
        search = offset.saturating_add(4);
        found += 1;
        if cgfx_declared_size(data, offset, true).is_some() {
            return true;
        }
    }
    false
}

fn cgfx_declared_size(data: &[u8], offset: usize, allow_truncated: bool) -> Option<usize> {
    let header_end = offset.checked_add(0x14)?;
    if header_end > data.len() || data.get(offset..offset + 4) != Some(b"CGFX") {
        return None;
    }
    if data.get(offset + 4..offset + 6) != Some(&[0xff, 0xfe]) {
        return None;
    }
    let header_size = read_u16_le(data, offset + 6)?;
    let declared = usize::try_from(read_u32_le(data, offset + 0x0c)?).ok()?;
    if header_size < 0x14 || declared < 0x20 {
        return None;
    }
    if !allow_truncated && offset.checked_add(declared)? > data.len() {
        return None;
    }
    Some(declared)
}

fn embedded_bch_offsets(data: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut search = 0usize;
    while search + 4 <= data.len() && offsets.len() < MAX_EMBEDDED_BCH {
        let Some(relative) = find_magic(&data[search..], b"BCH\0") else {
            break;
        };
        let offset = search + relative;
        search = offset.saturating_add(4);
        if parse_bch_header(&data[offset..]).is_ok() {
            offsets.push(offset);
        }
    }
    offsets
}

fn prefix(data: &[u8], limit: usize) -> &[u8] {
    &data[..data.len().min(limit)]
}

fn find_magic(data: &[u8], magic: &[u8]) -> Option<usize> {
    if magic.is_empty() {
        return None;
    }
    data.windows(magic.len()).position(|window| window == magic)
}

fn contains_magic(data: &[u8], magic: &[u8]) -> bool {
    find_magic(data, magic).is_some()
}

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset.checked_add(2)?)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.checked_add(4)?)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
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
    fn direct_stex_projects_reference_summary_and_asset_fields() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("tex/ui.stex".to_owned(), stex_a8(0x11))]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.profile_id, "eou1");
        assert_eq!(inventory.romfs_files, 1);
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 1);
        assert_eq!(inventory.assets.len(), 1);
        assert_eq!(inventory.assets[0].candidate_hash, "7ABCF0A736B8A12E");
        assert_eq!(inventory.assets[0].parser_used, "eou_stex_strict");
        assert_eq!(inventory.assets[0].category, "ui");
        assert_eq!(inventory.material_summary, MaterialParitySummary::default());
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn extensionless_stex_is_selected_by_romfs_probe() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("tex/opaque_resource".to_owned(), stex_a8(0x12))]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.assets.len(), 1);
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn unrelated_bin_is_not_selected_by_extension_alone() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("misc/random.bin".to_owned(), vec![0u8; 64])]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.strict_candidate_files, 0);
        assert!(inventory.assets.is_empty());
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn duplicate_encoded_assets_use_legacy_dedupe_identity() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([
                ("a_ui/a.stex".to_owned(), stex_a8(0x22)),
                ("z_misc/b.stex".to_owned(), stex_a8(0x22)),
            ]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.decoded_before_dedup, 2);
        assert_eq!(inventory.assets.len(), 1);
        assert_eq!(inventory.assets[0].category, "ui");
    }

    #[test]
    fn hpi_hpb_members_are_scanned_recursively_without_disk_extraction() {
        let payload = stex_a8(0x33);
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
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 1);
        assert_eq!(inventory.extraction_usage.members, 1);
        assert_eq!(inventory.assets.len(), 1);
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
        let mut malformed = stex_a8(0);
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
        assert!(inventory.assets.is_empty());
    }

    #[test]
    fn residual_legacy_container_is_visible_as_a_parity_gap() {
        let mut ctpk = vec![0u8; 0x40];
        ctpk[..4].copy_from_slice(b"CTPK");
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("legacy.ctpk".to_owned(), ctpk)]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.issues, 1);
        assert_eq!(inventory.issues[0].stage, "unsupported_parity_container");
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
                    virtual_path: "huge.stex".to_owned(),
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

    #[test]
    fn fingerprint_contains_no_paths_or_names() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("secret/path/ui.stex".to_owned(), stex_a8(0x44))]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        let text = serde_json::to_string(&inventory.structural_fingerprint()).unwrap();
        assert!(!text.contains("secret/path"));
        assert!(!text.contains("ui.stex"));
        assert!(text.contains("eo-texrip-structural-regression-fingerprint"));
    }
}
