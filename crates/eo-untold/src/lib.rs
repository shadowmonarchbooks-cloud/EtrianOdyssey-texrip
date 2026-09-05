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
const MATERIAL_REFERENCE_DISPLAY_LIMIT: usize = 20;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanIssue {
    pub source: String,
    pub stage: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingMaterialReferenceIssue {
    source: String,
    missing_stage: String,
    container_offset: u64,
    reference_label: String,
    names: Vec<String>,
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
    depth: u16,
}

#[derive(Debug)]
struct ScanState {
    budget: ExtractionBudget,
    usage: ExtractionUsage,
    summary: ParitySummary,
    issues: Vec<ScanIssue>,
    pending_material_reference_issues: Vec<PendingMaterialReferenceIssue>,
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
            pending_material_reference_issues: Vec::new(),
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
/// Archive expansion mirrors the frozen pipeline's stage order: recursively
/// expand HPI/HPB first, then recursively expand FARC over the RomFS+HPX roots,
/// then recursively expand EPL over the RomFS+HPX+FARC roots. This intentionally
/// does not revisit earlier archive families discovered by a later stage.
/// Proprietary bytes remain in memory and never enter the structural fingerprint.
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
                depth: 0,
            }),
            Err(error) => state.issue(&entry.virtual_path, "romfs_read", error),
        }
    }

    scan_staged_file_sets(files, &mut state);
    let mut assets = dedupe_assets(state.assets);
    bind_external_texture_names(&mut assets, &state.bindings_by_name);
    state.issues.extend(finalize_material_reference_issues(
        &assets,
        std::mem::take(&mut state.pending_material_reference_issues),
    ));
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

fn scan_staged_file_sets(romfs: Vec<VirtualFile>, state: &mut ScanState) {
    let hpx = expand_hpx_stage(&romfs, state);

    let mut farc_sources = Vec::with_capacity(romfs.len() + hpx.len());
    farc_sources.extend(romfs.iter().cloned());
    farc_sources.extend(hpx.iter().cloned());
    let farc = expand_single_archive_stage(&farc_sources, ArchiveFlavor::Farc, state);

    let mut epl_sources = Vec::with_capacity(romfs.len() + hpx.len() + farc.len());
    epl_sources.extend(romfs.iter().cloned());
    epl_sources.extend(hpx.iter().cloned());
    epl_sources.extend(farc.iter().cloned());
    let epl = expand_single_archive_stage(&epl_sources, ArchiveFlavor::Epl, state);

    for file in romfs
        .iter()
        .chain(hpx.iter())
        .chain(farc.iter())
        .chain(epl.iter())
    {
        inventory_file(file, state);
        if matches!(
            extension(&file.path).map(str::to_ascii_lowercase).as_deref(),
            Some("hpi") | Some("hpb")
        ) {
            continue;
        }
        if strict_texture_signature(&file.path, prefix(&file.data, ROMFS_PROBE_BYTES)) {
            state.summary.strict_candidate_files += 1;
            scan_payload(&file.path, &file.data, state);
        }
    }
}

fn expand_hpx_stage(seed: &[VirtualFile], state: &mut ScanState) -> Vec<VirtualFile> {
    let mut searchable = seed.to_vec();
    let mut output = Vec::new();
    let mut processed = BTreeSet::<String>::new();

    loop {
        let mut by_path = BTreeMap::<String, usize>::new();
        for (index, file) in searchable.iter().enumerate() {
            by_path.entry(path_key(&file.path)).or_insert(index);
        }

        let mut pairs = Vec::<(usize, usize)>::new();
        for (index, file) in searchable.iter().enumerate() {
            if !has_extension(&file.path, "hpi") {
                continue;
            }
            let key = path_key(&file.path);
            if processed.contains(&key) {
                continue;
            }
            let partner_key = path_key(&replace_extension(&file.path, "hpb"));
            let Some(partner_index) = by_path.get(&partner_key).copied() else {
                continue;
            };
            processed.insert(key);
            pairs.push((index, partner_index));
        }
        if pairs.is_empty() {
            break;
        }

        let mut added = Vec::new();
        for (hpi_index, hpb_index) in pairs {
            let hpi = searchable[hpi_index].clone();
            let hpb = searchable[hpb_index].clone();
            added.extend(expand_hpi_pair(&hpi, &hpb, state));
        }
        output.extend(added.iter().cloned());
        searchable.extend(added);
    }

    output
}

fn expand_hpi_pair(
    hpi: &VirtualFile,
    hpb: &VirtualFile,
    state: &mut ScanState,
) -> Vec<VirtualFile> {
    state.summary.hpx_pairs += 1;
    let parser = HpiHpbParser;
    let inventory = match parser.inspect(&hpi.data, &hpb.data, state.budget) {
        Ok(value) => value,
        Err(error) => {
            state.issue(&hpi.path, "hpi_hpb_inspect", error);
            return Vec::new();
        }
    };
    let archive_depth = hpi.depth.max(hpb.depth);
    if let Err(error) = state
        .usage
        .charge_inventory(archive_depth, &inventory, state.budget)
    {
        state.issue(&hpi.path, "archive_budget", error);
        return Vec::new();
    }

    let mut nested = Vec::new();
    let mut output_index = BTreeMap::<String, usize>::new();
    let mut writes = 0u64;
    for member in &inventory.members {
        let name = member
            .name
            .clone()
            .unwrap_or_else(|| format!("unnamed_{:05}.bin", member.index));
        let Some(path) = safe_child_path(&hpi.path, &name) else {
            continue;
        };
        match parser.read_member(&hpi.data, &hpb.data, member, state.budget) {
            Ok(data) => {
                writes += 1;
                let file = VirtualFile {
                    path: path.clone(),
                    data,
                    depth: archive_depth.saturating_add(1),
                };
                // Frozen unpack_hpi_pair() writes to a filesystem destination:
                // duplicate member names count as multiple successful writes, but
                // later writes replace the earlier bytes at that path. Preserve
                // the first position while replacing its final in-memory payload.
                if let Some(index) = output_index.get(&path).copied() {
                    nested[index] = file;
                } else {
                    output_index.insert(path, nested.len());
                    nested.push(file);
                }
            }
            Err(error) => state.issue(&hpi.path, "hpi_hpb_member", error),
        }
    }
    state.summary.hpx_files += writes;
    nested
}

#[derive(Clone, Copy)]
enum ArchiveFlavor {
    Farc,
    Epl,
}

fn expand_single_archive_stage(
    seed: &[VirtualFile],
    flavor: ArchiveFlavor,
    state: &mut ScanState,
) -> Vec<VirtualFile> {
    let mut searchable = seed.to_vec();
    let mut output = Vec::new();
    let mut processed = BTreeSet::<String>::new();
    let mut cursor = 0usize;

    while cursor < searchable.len() {
        let file = searchable[cursor].clone();
        cursor += 1;
        if !archive_discovered(flavor, &file) || !processed.insert(path_key(&file.path)) {
            continue;
        }
        let nested = expand_single_archive(file, flavor, state);
        output.extend(nested.iter().cloned());
        searchable.extend(nested);
    }

    output
}

fn archive_discovered(flavor: ArchiveFlavor, file: &VirtualFile) -> bool {
    match flavor {
        // Frozen find_farc_files() discovers by four-byte magic before parsing.
        ArchiveFlavor::Farc => file.data.get(..4) == Some(b"FARC"),
        // Frozen find_epl_files() is deliberately extension-only; malformed .epl
        // files still count as discovered archives and then surface a parse error.
        ArchiveFlavor::Epl => has_extension(&file.path, "epl"),
    }
}

fn expand_single_archive(
    file: VirtualFile,
    flavor: ArchiveFlavor,
    state: &mut ScanState,
) -> Vec<VirtualFile> {
    match flavor {
        ArchiveFlavor::Farc => state.summary.farc_archives += 1,
        ArchiveFlavor::Epl => state.summary.epl_archives += 1,
    }

    let inventory = match flavor {
        ArchiveFlavor::Farc => FarcParser.inspect(&file.data, state.budget),
        ArchiveFlavor::Epl => EplParser.inspect(&file.data, state.budget),
    };
    let inventory = match inventory {
        Ok(value) => value,
        Err(error) => {
            state.issue(&file.path, "archive_inspect", error);
            return Vec::new();
        }
    };
    if let Err(error) = state
        .usage
        .charge_inventory(file.depth, &inventory, state.budget)
    {
        state.issue(&file.path, "archive_budget", error);
        return Vec::new();
    }

    let mut nested = Vec::new();
    let mut used_names = BTreeSet::<String>::new();
    for member in &inventory.members {
        let result = match flavor {
            ArchiveFlavor::Farc => FarcParser.read_member(&file.data, member, state.budget),
            ArchiveFlavor::Epl => EplParser.read_member(&file.data, member, state.budget),
        };
        match result {
            Ok(data) => {
                let name = archive_output_name(
                    flavor,
                    member.index,
                    member.name.as_deref(),
                    &data,
                    &mut used_names,
                );
                nested.push(VirtualFile {
                    path: flat_child_path(&file.path, &name),
                    data,
                    depth: file.depth.saturating_add(1),
                });
            }
            Err(error) => state.issue(&file.path, "archive_member", error),
        }
    }

    match flavor {
        ArchiveFlavor::Farc => state.summary.farc_files += nested.len() as u64,
        ArchiveFlavor::Epl => state.summary.epl_files += nested.len() as u64,
    }
    nested
}

fn archive_output_name(
    flavor: ArchiveFlavor,
    index: u64,
    original_name: Option<&str>,
    payload: &[u8],
    used_names: &mut BTreeSet<String>,
) -> String {
    match flavor {
        ArchiveFlavor::Farc => {
            let mut name = match original_name {
                Some(value) => farc_safe_component(value),
                None => format!("hash_00000000_{index:05}{}", farc_guess_suffix(payload)),
            };
            if original_name.is_some() && extension(&name).is_none() {
                name.push_str(farc_guess_suffix(payload));
            }
            unique_farc_name(name, used_names)
        }
        ArchiveFlavor::Epl => {
            let fallback = format!("member_{index:04}");
            let mut base = epl_safe_name(original_name.unwrap_or(""), &fallback);
            let suffix = epl_guess_suffix(payload, &base);
            let existing = extension(&base)
                .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
                .unwrap_or_default();
            if existing != suffix {
                base.push_str(&suffix);
            }
            format!("{index:04}_{base}")
        }
    }
}

fn farc_safe_component(value: &str) -> String {
    let replaced = value.replace(['\\', '/'], "_");
    let trimmed = replaced.trim().trim_matches('.');
    let mut mapped = String::new();
    for ch in trimmed.chars() {
        if ch <= '\u{1f}' || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
            mapped.push('_');
        } else {
            mapped.push(ch);
        }
    }

    let mut collapsed = String::new();
    let mut whitespace = false;
    for ch in mapped.chars() {
        if ch.is_whitespace() {
            if !whitespace && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            whitespace = true;
        } else {
            collapsed.push(ch);
            whitespace = false;
        }
    }
    let value = collapsed.trim().chars().take(180).collect::<String>();
    if value.is_empty() {
        "unnamed".to_owned()
    } else {
        value
    }
}

fn farc_guess_suffix(payload: &[u8]) -> &'static str {
    if payload.get(..4) == Some(b"BCH\0")
        || contains_magic(prefix(payload, WRAPPED_BCH_PROBE_BYTES), b"BCH\0")
    {
        ".bchbin"
    } else if payload.get(..4) == Some(b"STEX") {
        ".stex"
    } else if payload.get(..4) == Some(b"FARC") {
        ".farc"
    } else if payload.get(..4) == Some(b"SIR0") {
        ".sir0"
    } else if payload.get(..4) == Some(b"CGFX") {
        ".cgfx"
    } else if payload.get(..4) == Some(b"CTPK") {
        ".ctpk"
    } else {
        ".bin"
    }
}

fn unique_farc_name(name: String, used_names: &mut BTreeSet<String>) -> String {
    if used_names.insert(name.clone()) {
        return name;
    }
    let (stem, suffix) = split_filename_suffix(&name);
    for number in 2u32.. {
        let candidate = format!("{stem}_{number}{suffix}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("u32 filename suffix space exhausted")
}

fn epl_safe_name(value: &str, fallback: &str) -> String {
    let normalized = value.replace('\\', "/");
    let leaf = normalized.rsplit('/').next().unwrap_or("").trim();
    let mut mapped = String::with_capacity(leaf.len());
    for ch in leaf.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '@' | '+' | '-') {
            mapped.push(ch);
        } else {
            mapped.push('_');
        }
    }
    let value = mapped
        .trim_matches(|ch| matches!(ch, '.' | '_'))
        .chars()
        .take(120)
        .collect::<String>();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

fn epl_guess_suffix(payload: &[u8], original_name: &str) -> String {
    let suffix = if payload.get(..4) == Some(b"STEX") {
        Some(".stex")
    } else if payload.get(..4) == Some(b"CGFX") {
        Some(".cgfx")
    } else if payload.get(..4) == Some(b"BCH\0") {
        Some(".bch")
    } else if payload.get(..4) == Some(b"ATBC") {
        Some(".bam")
    } else if payload.get(..4) == Some(b"CTPK") {
        Some(".ctpk")
    } else if matches!(payload.get(..4), Some(b"CTXB") | Some(b"ctxb")) {
        Some(".ctxb")
    } else if payload.get(..4) == Some(b"FARC") {
        Some(".farc")
    } else if payload.starts_with(b"EPL") {
        Some(".epl")
    } else {
        None
    };
    if let Some(suffix) = suffix {
        return suffix.to_owned();
    }
    if let Some(ext) = extension(original_name) {
        let suffix = format!(".{}", ext.to_ascii_lowercase());
        if suffix.len() <= 12 {
            return suffix;
        }
    }
    ".bin".to_owned()
}

fn split_filename_suffix(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    }
}

fn flat_child_path(parent: &str, child: &str) -> String {
    let parent = normalize_virtual_path(parent);
    format!("{}/{}", parent.trim_end_matches('/'), child)
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

fn scan_payload(path: &str, data: &[u8], state: &mut ScanState) {
    if is_stex(data) {
        match parse_stex(data) {
            Ok(texture) => {
                push_asset(
                    path,
                    texture.name.as_deref().unwrap_or(""),
                    "eou_stex_strict",
                    &texture.encoded,
                    BTreeSet::new(),
                    false,
                    state,
                );
            }
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
    let decoded_names = add_cgfx_assets(path, &container, &local_bindings, state);
    report_missing_material_textures(
        path,
        "cgfx_material_texture_missing",
        container_offset,
        "MTOB",
        &local_bindings,
        &decoded_names,
        state,
    );
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
    let decoded_names = add_bch_assets(path, &container, &local_bindings, state);
    report_missing_material_textures(
        path,
        "bch_material_texture_missing",
        container_offset,
        "H3D material",
        &local_bindings,
        &decoded_names,
        state,
    );
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
            if texture.enabled {
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
) -> BTreeSet<String> {
    let mut decoded_names = BTreeSet::new();
    for texture in &container.textures {
        if push_asset(
            path,
            &texture.name,
            "cgfx_struct",
            &texture.encoded,
            bindings.get(&texture.name).cloned().unwrap_or_default(),
            true,
            state,
        ) {
            decoded_names.insert(texture.name.clone());
        }
    }
    decoded_names
}

fn add_bch_assets(
    path: &str,
    container: &BchContainer,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    state: &mut ScanState,
) -> BTreeSet<String> {
    let mut decoded_names = BTreeSet::new();
    for texture in &container.textures {
        if push_asset(
            path,
            &texture.name,
            "bch_struct",
            &texture.encoded,
            bindings.get(&texture.name).cloned().unwrap_or_default(),
            true,
            state,
        ) {
            decoded_names.insert(texture.name.clone());
        }
    }
    decoded_names
}

fn report_missing_material_textures(
    path: &str,
    stage: &str,
    container_offset: u64,
    reference_label: &str,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    decoded_names: &BTreeSet<String>,
    state: &mut ScanState,
) {
    let missing = bindings
        .keys()
        .filter(|name| !decoded_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }
    state
        .pending_material_reference_issues
        .push(PendingMaterialReferenceIssue {
            source: path.to_owned(),
            missing_stage: stage.to_owned(),
            container_offset,
            reference_label: reference_label.to_owned(),
            names: missing,
        });
}

fn finalize_material_reference_issues(
    assets: &[ParityAsset],
    pending: Vec<PendingMaterialReferenceIssue>,
) -> Vec<ScanIssue> {
    let mut decoded_by_name = BTreeMap::<&str, usize>::new();
    for asset in assets {
        for name in asset.internal_names() {
            *decoded_by_name.entry(name).or_default() += 1;
        }
    }

    let mut issues = Vec::new();
    for pending_issue in pending {
        let mut missing = Vec::new();
        let mut ambiguous = Vec::new();
        for name in pending_issue.names {
            match decoded_by_name.get(name.as_str()).copied().unwrap_or(0) {
                0 => missing.push(name),
                1 => {}
                _ => ambiguous.push(name),
            }
        }

        if !missing.is_empty() {
            issues.push(ScanIssue {
                source: pending_issue.source.clone(),
                stage: pending_issue.missing_stage.clone(),
                message: render_material_reference_issue(
                    pending_issue.container_offset,
                    &pending_issue.reference_label,
                    &missing,
                    "were not decoded anywhere in the scanned ROM",
                ),
            });
        }
        if !ambiguous.is_empty() {
            issues.push(ScanIssue {
                source: pending_issue.source,
                stage: pending_issue.missing_stage.replace("_missing", "_ambiguous"),
                message: render_material_reference_issue(
                    pending_issue.container_offset,
                    &pending_issue.reference_label,
                    &ambiguous,
                    "matched multiple decoded textures across the scanned ROM and could not be bound unambiguously",
                ),
            });
        }
    }
    issues
}

fn render_material_reference_issue(
    container_offset: u64,
    reference_label: &str,
    names: &[String],
    detail: &str,
) -> String {
    let displayed = names
        .iter()
        .take(MATERIAL_REFERENCE_DISPLAY_LIMIT)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let omitted = names.len().saturating_sub(MATERIAL_REFERENCE_DISPLAY_LIMIT);
    let suffix = if omitted == 0 {
        String::new()
    } else {
        format!(", ... (+{omitted} more)")
    };
    format!(
        "offset 0x{container_offset:X}: {} {reference_label} texture reference(s) {detail}: {displayed}{suffix}",
        names.len()
    )
}

fn push_asset(
    path: &str,
    internal_name: &str,
    parser_used: &str,
    encoded: &EncodedTexture,
    binding_keys: BTreeSet<String>,
    model_texture: bool,
    state: &mut ScanState,
) -> bool {
    let decoder = NativePicaDecoder;
    if let Err(error) = decoder.decode_base_level(encoded) {
        state.issue(path, "texture_decode", error);
        return false;
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
    true
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
    contains_magic(probe, b"BCH\0")
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

fn safe_child_path(parent: &str, child: &str) -> Option<String> {
    if child.is_empty() || child.contains('\0') {
        return None;
    }
    let normalized = child.replace('\\', "/");
    if normalized.starts_with('/') {
        return None;
    }

    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with([' ', '.'])
            || part.contains(':')
            || is_windows_device_name(part)
        {
            return None;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return None;
    }

    let parent = normalize_virtual_path(parent);
    Some(format!("{}/{}", parent.trim_end_matches('/'), parts.join("/")))
}

fn is_windows_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (bytes.starts_with(b"COM") || bytes.starts_with(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_core::TextureRole;
    use eo_models::{MaterialRecord, TextureReference};
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

    fn named_asset(name: &str, hash_index: u64) -> ParityAsset {
        let hash = format!("{hash_index:016X}");
        let mut asset =
            ParityAsset::test_fixture(&hash, 8, 8, 13, "test", "dungeon", 0);
        asset.set_internal_name(name);
        asset
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

    fn hpi_for_duplicate_uncompressed(
        name: &str,
        first_size: usize,
        second_offset: usize,
        second_size: usize,
    ) -> Vec<u8> {
        let name = name.as_bytes();
        let names_base = 0x18 + 2 * 16;
        let mut hpi = vec![0u8; names_base + name.len() + 1];
        hpi[..4].copy_from_slice(b"HPIH");
        hpi[0x12..0x14].copy_from_slice(&0u16.to_le_bytes());
        hpi[0x14..0x16].copy_from_slice(&2u16.to_le_bytes());

        hpi[0x18..0x1c].copy_from_slice(&0u32.to_le_bytes());
        hpi[0x1c..0x20].copy_from_slice(&0u32.to_le_bytes());
        hpi[0x20..0x24].copy_from_slice(&(first_size as u32).to_le_bytes());
        hpi[0x24..0x28].copy_from_slice(&0u32.to_le_bytes());

        hpi[0x28..0x2c].copy_from_slice(&0u32.to_le_bytes());
        hpi[0x2c..0x30].copy_from_slice(&(second_offset as u32).to_le_bytes());
        hpi[0x30..0x34].copy_from_slice(&(second_size as u32).to_le_bytes());
        hpi[0x34..0x38].copy_from_slice(&0u32.to_le_bytes());

        hpi[names_base..names_base + name.len()].copy_from_slice(name);
        hpi
    }

    fn farc_with_member(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 0xc0 + payload.len()];
        data[0..4].copy_from_slice(b"FARC");
        data[0x20..0x24].copy_from_slice(&4u32.to_le_bytes());
        data[0x24..0x28].copy_from_slice(&0x40u32.to_le_bytes());
        data[0x28..0x2c].copy_from_slice(&0x80u32.to_le_bytes());
        data[0x2c..0x30].copy_from_slice(&0xc0u32.to_le_bytes());

        let sir0 = 0x40usize;
        data[sir0..sir0 + 4].copy_from_slice(b"SIR0");
        data[sir0 + 4..sir0 + 8].copy_from_slice(&0x10u32.to_le_bytes());
        data[sir0 + 8..sir0 + 12].copy_from_slice(&0x70u32.to_le_bytes());
        data[sir0 + 0x10..sir0 + 0x14].copy_from_slice(&0x20u32.to_le_bytes());
        data[sir0 + 0x14..sir0 + 0x18].copy_from_slice(&1u32.to_le_bytes());
        data[sir0 + 0x18..sir0 + 0x1c].copy_from_slice(&0u32.to_le_bytes());
        data[sir0 + 0x20..sir0 + 0x24].copy_from_slice(&0x40u32.to_le_bytes());
        data[sir0 + 0x24..sir0 + 0x28].copy_from_slice(&0u32.to_le_bytes());
        data[sir0 + 0x28..sir0 + 0x2c]
            .copy_from_slice(&(payload.len() as u32).to_le_bytes());
        for (index, unit) in format!("{name}\0").encode_utf16().enumerate() {
            let offset = sir0 + 0x40 + index * 2;
            data[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        data[0xc0..].copy_from_slice(payload);
        data
    }

    fn epl_with_member(name: &str, payload: &[u8]) -> Vec<u8> {
        let payload_offset = 0x1b0usize;
        let mut data = vec![0u8; payload_offset + payload.len()];
        data[0x80..0x84].copy_from_slice(&1i32.to_le_bytes());
        data[0x84..0x88].copy_from_slice(&0i32.to_le_bytes());
        data[0x88..0x8c].copy_from_slice(&0x90i32.to_le_bytes());

        let record = 0x90usize;
        data[record + 0x90..record + 0x94].copy_from_slice(&0x180i32.to_le_bytes());
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(35);
        data[record + 0x9c..record + 0x9c + name_len]
            .copy_from_slice(&name_bytes[..name_len]);

        let descriptor = 0x180usize;
        data[descriptor + 0x20..descriptor + 0x24].copy_from_slice(&0x30i32.to_le_bytes());
        data[descriptor + 0x24..descriptor + 0x28]
            .copy_from_slice(&(payload.len() as i32).to_le_bytes());
        data[payload_offset..].copy_from_slice(payload);
        data
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
        // Frozen inventory_files scans both the raw RomFS HPB and the extracted HPX roots.
        assert_eq!(inventory.summary.stex_files, 2);
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 1);
        assert_eq!(inventory.extraction_usage.members, 1);
        assert_eq!(inventory.assets.len(), 1);
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn duplicate_hpx_member_paths_overwrite_but_count_each_write() {
        let first = stex_a8(0x21);
        let second = stex_a8(0x22);
        let second_offset = first.len();
        let mut hpb = first;
        hpb.extend_from_slice(&second);
        let hpi = hpi_for_duplicate_uncompressed(
            "same.stex",
            second_offset,
            second_offset,
            second.len(),
        );
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([
                ("data/dup.hpi".to_owned(), hpi),
                ("data/dup.hpb".to_owned(), hpb),
            ]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.hpx_pairs, 1);
        assert_eq!(inventory.summary.hpx_files, 2);
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.decoded_before_dedup, 1);
        assert_eq!(inventory.assets.len(), 1);
        assert_eq!(
            inventory.assets[0].candidate_hash,
            crate::cityhash::cityhash64_hex(&[0x22; 64])
        );
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn later_epl_stage_expands_epl_extracted_from_farc() {
        let inner_epl = epl_with_member("leaf.stex", &stex_a8(0x55));
        let outer_farc = farc_with_member("effects.epl", &inner_epl);
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("effects.farc".to_owned(), outer_farc)]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.farc_archives, 1);
        assert_eq!(inventory.summary.farc_files, 1);
        assert_eq!(inventory.summary.epl_archives, 1);
        assert_eq!(inventory.summary.epl_files, 1);
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.assets.len(), 1);
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn earlier_farc_stage_does_not_revisit_farc_extracted_from_epl() {
        let late_farc = farc_with_member("late.stex", &stex_a8(0x66));
        let outer_epl = epl_with_member("late.farc", &late_farc);
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("effects.epl".to_owned(), outer_epl)]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.epl_archives, 1);
        assert_eq!(inventory.summary.epl_files, 1);
        assert_eq!(inventory.summary.farc_archives, 0);
        assert_eq!(inventory.summary.farc_files, 0);
        assert_eq!(inventory.summary.strict_candidate_files, 0);
        assert_eq!(inventory.summary.stex_files, 0);
        assert!(inventory.assets.is_empty());
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn farc_member_names_are_flattened_and_scanned_like_frozen_unpacker() {
        let outer_farc = farc_with_member("../escape.stex", &stex_a8(0x77));
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("unsafe.farc".to_owned(), outer_farc)]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.farc_archives, 1);
        assert_eq!(inventory.summary.farc_files, 1);
        assert_eq!(inventory.summary.strict_candidate_files, 1);
        assert_eq!(inventory.summary.stex_files, 1);
        assert_eq!(inventory.assets.len(), 1);
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn malformed_epl_extension_is_discovered_before_parse_failure() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("broken.epl".to_owned(), vec![0u8; 64])]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.epl_archives, 1);
        assert_eq!(inventory.summary.epl_files, 0);
        assert_eq!(inventory.summary.strict_candidate_files, 0);
        assert_eq!(inventory.issues.len(), 1);
        assert_eq!(inventory.issues[0].stage, "archive_inspect");
    }

    #[test]
    fn malformed_farc_magic_is_discovered_before_parse_failure() {
        let rom = FakeRom {
            hint: eou1_hint(),
            files: BTreeMap::from([("broken.bin".to_owned(), b"FARCbad".to_vec())]),
        };
        let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.summary.farc_archives, 1);
        assert_eq!(inventory.summary.farc_files, 0);
        assert_eq!(inventory.summary.strict_candidate_files, 0);
        assert_eq!(inventory.issues.len(), 1);
        assert_eq!(inventory.issues[0].stage, "archive_inspect");
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
    fn disabled_model_texture_reference_is_not_required() {
        let inventory = ModelInventory {
            model_count: 1,
            model_name: Some("model".to_owned()),
            materials: vec![MaterialRecord {
                index: 0,
                model_index: 0,
                model_material_index: 0,
                name: Some("material".to_owned()),
                textures: vec![
                    TextureReference {
                        slot: 0,
                        internal_name: "live".to_owned(),
                        role: TextureRole::Unknown,
                        enabled: true,
                    },
                    TextureReference {
                        slot: 1,
                        internal_name: "stale-disabled".to_owned(),
                        role: TextureRole::Unknown,
                        enabled: false,
                    },
                ],
                alpha_stages: Vec::new(),
            }],
        };
        let mut state = ScanState::new(ExtractionBudget::default());

        let local = inspect_model("model.bch", "bch", 0, Ok(inventory), &mut state);

        assert!(local.contains_key("live"));
        assert!(!local.contains_key("stale-disabled"));
        assert!(state.bindings_by_name.contains_key("live"));
        assert!(!state.bindings_by_name.contains_key("stale-disabled"));
        assert_eq!(state.materials.len(), 1);
        assert_eq!(state.materials[0].slots.len(), 2);
        assert!(!state.materials[0].slots[&1].enabled);
    }

    #[test]
    fn unresolved_material_texture_references_are_preserved_structurally_per_payload() {
        let bindings = BTreeMap::from([
            (
                "decoded".to_owned(),
                BTreeSet::from(["binding-decoded".to_owned()]),
            ),
            (
                "missing-a".to_owned(),
                BTreeSet::from(["binding-a".to_owned()]),
            ),
            (
                "missing-b".to_owned(),
                BTreeSet::from(["binding-b".to_owned()]),
            ),
        ]);
        let decoded_names = BTreeSet::from(["decoded".to_owned()]);
        let mut state = ScanState::new(ExtractionBudget::default());

        report_missing_material_textures(
            "model.bam",
            "cgfx_material_texture_missing",
            0x123,
            "MTOB",
            &bindings,
            &decoded_names,
            &mut state,
        );

        assert!(state.issues.is_empty());
        assert_eq!(state.pending_material_reference_issues.len(), 1);
        let pending = &state.pending_material_reference_issues[0];
        assert_eq!(pending.source, "model.bam");
        assert_eq!(pending.missing_stage, "cgfx_material_texture_missing");
        assert_eq!(pending.container_offset, 0x123);
        assert_eq!(pending.reference_label, "MTOB");
        assert_eq!(pending.names, vec!["missing-a", "missing-b"]);
    }

    #[test]
    fn fully_decoded_material_texture_references_do_not_emit_pending_issue() {
        let bindings = BTreeMap::from([(
            "decoded".to_owned(),
            BTreeSet::from(["binding-decoded".to_owned()]),
        )]);
        let decoded_names = BTreeSet::from(["decoded".to_owned()]);
        let mut state = ScanState::new(ExtractionBudget::default());

        report_missing_material_textures(
            "model.bam2",
            "bch_material_texture_missing",
            0x40,
            "H3D material",
            &bindings,
            &decoded_names,
            &mut state,
        );

        assert!(state.issues.is_empty());
        assert!(state.pending_material_reference_issues.is_empty());
    }

    #[test]
    fn deduped_alias_resolves_material_reference_warning() {
        let mut first = ParityAsset::test_fixture("A", 8, 8, 13, "test", "dungeon", 0);
        first.set_internal_name("day-sky");
        let mut second = ParityAsset::test_fixture("A", 8, 8, 13, "test", "dungeon", 0);
        second.set_internal_name("night-sky");
        let assets = dedupe_assets(vec![first, second]);
        assert_eq!(assets.len(), 1);

        let pending = vec![PendingMaterialReferenceIssue {
            source: "model.bch".to_owned(),
            missing_stage: "bch_material_texture_missing".to_owned(),
            container_offset: 0,
            reference_label: "H3D material".to_owned(),
            names: vec!["night-sky".to_owned()],
        }];

        assert!(finalize_material_reference_issues(&assets, pending).is_empty());
    }

    #[test]
    fn material_reference_reconciliation_uses_full_name_set_before_rendering() {
        let names = (0..25)
            .map(|index| format!("tex{index:02}"))
            .collect::<Vec<_>>();
        let pending = vec![PendingMaterialReferenceIssue {
            source: "model.bam2".to_owned(),
            missing_stage: "bch_material_texture_missing".to_owned(),
            container_offset: 0x80,
            reference_label: "H3D material".to_owned(),
            names: names.clone(),
        }];
        let mut assets = names[..21]
            .iter()
            .enumerate()
            .map(|(index, name)| named_asset(name, index as u64 + 1))
            .collect::<Vec<_>>();
        assets.push(named_asset("tex21", 100));
        assets.push(named_asset("tex21", 101));

        let issues = finalize_material_reference_issues(&assets, pending);
        assert_eq!(issues.len(), 2);
        let missing = issues
            .iter()
            .find(|issue| issue.stage == "bch_material_texture_missing")
            .unwrap();
        assert!(missing.message.contains("3 H3D material texture reference(s)"));
        assert!(missing.message.contains("tex22, tex23, tex24"));
        assert!(!missing.message.contains("tex00"));
        let ambiguous = issues
            .iter()
            .find(|issue| issue.stage == "bch_material_texture_ambiguous")
            .unwrap();
        assert!(ambiguous.message.contains("1 H3D material texture reference(s)"));
        assert!(ambiguous.message.contains("tex21"));
    }

    #[test]
    fn material_reference_rendering_truncates_only_after_full_reconciliation() {
        let names = (0..25)
            .map(|index| format!("missing{index:02}"))
            .collect::<Vec<_>>();
        let pending = vec![PendingMaterialReferenceIssue {
            source: "model.bam2".to_owned(),
            missing_stage: "bch_material_texture_missing".to_owned(),
            container_offset: 0,
            reference_label: "H3D material".to_owned(),
            names,
        }];

        let issues = finalize_material_reference_issues(&[], pending);
        assert_eq!(issues.len(), 1);
        let message = &issues[0].message;
        assert!(message.contains("25 H3D material texture reference(s)"));
        assert!(message.contains("missing19"));
        assert!(!message.contains("missing20"));
        assert!(message.contains("... (+5 more)"));
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
