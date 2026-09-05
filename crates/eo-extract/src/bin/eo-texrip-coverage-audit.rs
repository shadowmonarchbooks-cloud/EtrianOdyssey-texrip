use eo_archives::{
    ArchiveParser, EplParser, ExtractionBudget, ExtractionUsage, FarcParser, HpiHpbParser,
};
use eo_rom::{NativeRom, RomReader};
use eo_textures::{
    bch::{parse_bch, parse_header as parse_bch_header, BchHeader},
    cgfx::parse_cgfx,
    stex::{is_stex, parse_stex},
};
use eo_untold::inventory_reader;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

const AUDIT_PROBE_BYTES: usize = 4 * 1024 * 1024;
const CGFX_TEXTURE_DICT_INDEX: usize = 1;
const BCH_TEXTURE_SECTION_INDEX: usize = 3;
const CGFX_IMAGE_TEXTURE: u32 = 0x2000_0011;
const CGFX_CUBE_TEXTURE: u32 = 0x2000_0009;
const CGFX_REFERENCE_TEXTURE: u32 = 0x2000_0004;
const CGFX_PROCEDURAL_TEXTURE: u32 = 0x2000_0002;
const CGFX_SHADOW_TEXTURE: u32 = 0x2000_0021;

#[derive(Clone, Debug)]
struct VirtualFile {
    path: String,
    data: Vec<u8>,
    depth: u16,
    extractor_selected: bool,
}

#[derive(Clone, Debug, Serialize)]
struct AuditIssue {
    source: String,
    stage: String,
    message: String,
}

#[derive(Clone, Debug, Default, Serialize)]
struct CoverageTotals {
    romfs_entries: u64,
    romfs_files_read: u64,
    expanded_files: u64,
    fixed_point_archive_expansions: u64,
    cross_family_archive_candidates: u64,
    stex_files: u64,
    stex_parsed: u64,
    cgfx_payloads: u64,
    cgfx_top_level_textures_declared: u64,
    cgfx_image_textures_declared: u64,
    cgfx_image_textures_parsed: u64,
    cgfx_cube_textures_declared: u64,
    cgfx_reference_textures_declared: u64,
    cgfx_procedural_textures_declared: u64,
    cgfx_shadow_textures_declared: u64,
    cgfx_unknown_texture_objects: u64,
    bch_payloads: u64,
    bch_texture_entries_declared: u64,
    bch_texture_pointers_resolved: u64,
    bch_texture_entries_parsed: u64,
    bch_cube_texture_entries: u64,
    bch_cube_faces_declared: u64,
    unsupported_texture_containers: u64,
}

#[derive(Clone, Debug, Serialize)]
struct ExtractorSnapshot {
    textures_after_dedup: usize,
    decoded_before_dedup: u64,
    strict_candidate_files: u64,
    model_payloads: u64,
    cgfx_payloads: u64,
    bch_payloads: u64,
    texture_descriptors_found: u64,
    decoded_3d_textures: u64,
    extraction_issues: usize,
}

#[derive(Clone, Debug, Serialize)]
struct CoverageAuditReport {
    schema: String,
    profile_id: String,
    game_id: String,
    title_id: Option<String>,
    product_code: Option<String>,
    audit_probe_bytes: usize,
    extractor: ExtractorSnapshot,
    coverage: CoverageTotals,
    audit_issues: Vec<AuditIssue>,
    coverage_complete: bool,
}

#[derive(Clone, Debug, Default)]
struct CgfxTextureObjects {
    types: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default)]
struct BchDeclaredTextures {
    declared: u32,
    resolved: u32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("coverage audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let source = args.next().map(PathBuf::from).ok_or(
        "usage: eo-texrip-coverage-audit <decrypted-rom> [coverage-report.json]",
    )?;
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_report_path(&source));
    if args.next().is_some() {
        return Err("too many arguments".into());
    }

    let bytes = fs::read(&source)?;
    let rom = NativeRom::detect(&bytes)?;
    let budget = ExtractionBudget::default();
    let inventory = inventory_reader(&rom, budget)?;

    let entries = rom.entries()?;
    let mut issues = Vec::new();
    let mut coverage = CoverageTotals {
        romfs_entries: entries.len() as u64,
        ..CoverageTotals::default()
    };
    let mut files = Vec::new();

    for entry in &entries {
        if entry.size == 0 {
            continue;
        }
        let probe_len = usize::try_from(entry.size.min(AUDIT_PROBE_BYTES as u64))
            .unwrap_or(AUDIT_PROBE_BYTES);
        let probe = match rom.read_entry_prefix(&entry.virtual_path, probe_len) {
            Ok(value) => value,
            Err(error) => {
                issue(
                    &mut issues,
                    &entry.virtual_path,
                    "romfs_probe",
                    error.to_string(),
                );
                continue;
            }
        };
        let extractor_selected = extractor_candidate_path(&entry.virtual_path)
            || extractor_romfs_probe_candidate(&probe);
        let audit_selected = extractor_selected || contains_magic(&probe, b"CGFX");
        if !audit_selected {
            continue;
        }
        if entry.size > budget.max_archive_bytes {
            issue(
                &mut issues,
                &entry.virtual_path,
                "romfs_budget",
                format!(
                    "audit candidate size {} exceeds read ceiling {}",
                    entry.size, budget.max_archive_bytes
                ),
            );
            continue;
        }
        match rom.read_entry(&entry.virtual_path) {
            Ok(data) => {
                coverage.romfs_files_read += 1;
                files.push(VirtualFile {
                    path: normalize_path(&entry.virtual_path),
                    data,
                    depth: 0,
                    extractor_selected,
                });
            }
            Err(error) => issue(
                &mut issues,
                &entry.virtual_path,
                "romfs_read",
                error.to_string(),
            ),
        }
    }

    expand_archives_fixed_point(
        &mut files,
        budget,
        &mut coverage,
        &mut issues,
    );
    coverage.expanded_files = files.len().saturating_sub(coverage.romfs_files_read as usize) as u64;

    let archive_sources = archive_source_paths(&files);
    for file in &files {
        if archive_sources.contains(&path_key(&file.path))
            || matches!(extension(&file.path).as_deref(), Some("hpi") | Some("hpb"))
        {
            continue;
        }
        audit_payload(file, &mut coverage, &mut issues);
    }

    if coverage.cgfx_payloads > inventory.cgfx_payloads {
        issue(
            &mut issues,
            "ROM",
            "cgfx_payload_coverage",
            format!(
                "independent audit found {} valid CGFX payloads, extractor inventoried {}",
                coverage.cgfx_payloads, inventory.cgfx_payloads
            ),
        );
    }
    if coverage.bch_payloads > inventory.bch_payloads {
        issue(
            &mut issues,
            "ROM",
            "bch_payload_coverage",
            format!(
                "independent audit found {} valid BCH payloads, extractor inventoried {}",
                coverage.bch_payloads, inventory.bch_payloads
            ),
        );
    }

    let coverage_complete = issues.is_empty();
    let report = CoverageAuditReport {
        schema: "eo-texrip-untold-coverage-audit-v1".to_owned(),
        profile_id: inventory.profile_id.clone(),
        game_id: format!("{:?}", inventory.game_id),
        title_id: inventory.title_id.clone(),
        product_code: inventory.product_code.clone(),
        audit_probe_bytes: AUDIT_PROBE_BYTES,
        extractor: ExtractorSnapshot {
            textures_after_dedup: inventory.assets.len(),
            decoded_before_dedup: inventory.summary.decoded_before_dedup,
            strict_candidate_files: inventory.summary.strict_candidate_files,
            model_payloads: inventory.model_payloads,
            cgfx_payloads: inventory.cgfx_payloads,
            bch_payloads: inventory.bch_payloads,
            texture_descriptors_found: inventory.texture_descriptors_found,
            decoded_3d_textures: inventory.decoded_3d_textures,
            extraction_issues: inventory.issues.len(),
        },
        coverage,
        audit_issues: issues,
        coverage_complete,
    };

    let mut json = serde_json::to_vec_pretty(&report)?;
    json.push(b'\n');
    fs::write(&output, json)?;
    println!("coverage report: {}", output.display());
    println!(
        "coverage complete: {} ({} audit issue(s))",
        report.coverage_complete,
        report.audit_issues.len()
    );
    Ok(())
}

fn default_report_path(source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "eo-rom".to_owned());
    source.with_file_name(format!("{stem}-coverage-audit.json"))
}

fn expand_archives_fixed_point(
    files: &mut Vec<VirtualFile>,
    budget: ExtractionBudget,
    coverage: &mut CoverageTotals,
    issues: &mut Vec<AuditIssue>,
) {
    let mut usage = ExtractionUsage::default();
    let mut processed_hpi = BTreeSet::new();
    let mut processed_farc = BTreeSet::new();
    let mut processed_epl = BTreeSet::new();

    loop {
        let before = files.len();
        expand_hpi_pass(
            files,
            budget,
            &mut usage,
            &mut processed_hpi,
            coverage,
            issues,
        );
        expand_single_pass(
            files,
            ArchiveFlavor::Farc,
            budget,
            &mut usage,
            &mut processed_farc,
            coverage,
            issues,
        );
        expand_single_pass(
            files,
            ArchiveFlavor::Epl,
            budget,
            &mut usage,
            &mut processed_epl,
            coverage,
            issues,
        );
        if files.len() == before {
            break;
        }
    }
}

fn expand_hpi_pass(
    files: &mut Vec<VirtualFile>,
    budget: ExtractionBudget,
    usage: &mut ExtractionUsage,
    processed: &mut BTreeSet<String>,
    coverage: &mut CoverageTotals,
    issues: &mut Vec<AuditIssue>,
) {
    let mut by_path = BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        by_path.entry(path_key(&file.path)).or_insert(index);
    }
    let mut pairs = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if extension(&file.path).as_deref() != Some("hpi") {
            continue;
        }
        let key = path_key(&file.path);
        if processed.contains(&key) {
            continue;
        }
        let partner = path_key(&replace_extension(&file.path, "hpb"));
        if let Some(partner_index) = by_path.get(&partner).copied() {
            processed.insert(key);
            pairs.push((index, partner_index));
        }
    }

    let parser = HpiHpbParser;
    let mut added = Vec::new();
    for (hpi_index, hpb_index) in pairs {
        let hpi = &files[hpi_index];
        let hpb = &files[hpb_index];
        if has_archive_ancestor(&hpi.path, "farc") || has_archive_ancestor(&hpi.path, "epl") {
            coverage.cross_family_archive_candidates += 1;
            issue(
                issues,
                &hpi.path,
                "cross_family_archive",
                "HPI/HPB pair was discovered inside FARC/EPL output; the production staged extractor does not revisit HPI after those stages",
            );
        }
        let inventory = match parser.inspect(&hpi.data, &hpb.data, budget) {
            Ok(value) => value,
            Err(error) => {
                issue(issues, &hpi.path, "hpi_hpb_inspect", error.to_string());
                continue;
            }
        };
        let depth = hpi.depth.max(hpb.depth);
        if let Err(error) = usage.charge_inventory(depth, &inventory, budget) {
            issue(issues, &hpi.path, "archive_budget", error.to_string());
            continue;
        }
        coverage.fixed_point_archive_expansions += 1;
        for member in &inventory.members {
            match parser.read_member(&hpi.data, &hpb.data, member, budget) {
                Ok(data) => {
                    let name = member
                        .name
                        .as_deref()
                        .map(safe_relative_name)
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| format!("member_{:05}.bin", member.index));
                    added.push(VirtualFile {
                        path: format!("{}/{}", hpi.path, name),
                        data,
                        depth: depth.saturating_add(1),
                        extractor_selected: true,
                    });
                }
                Err(error) => issue(issues, &hpi.path, "hpi_hpb_member", error.to_string()),
            }
        }
    }
    files.extend(added);
}

#[derive(Clone, Copy)]
enum ArchiveFlavor {
    Farc,
    Epl,
}

fn expand_single_pass(
    files: &mut Vec<VirtualFile>,
    flavor: ArchiveFlavor,
    budget: ExtractionBudget,
    usage: &mut ExtractionUsage,
    processed: &mut BTreeSet<String>,
    coverage: &mut CoverageTotals,
    issues: &mut Vec<AuditIssue>,
) {
    let mut targets = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let discovered = match flavor {
            ArchiveFlavor::Farc => file.data.get(..4) == Some(b"FARC"),
            ArchiveFlavor::Epl => extension(&file.path).as_deref() == Some("epl"),
        };
        let key = path_key(&file.path);
        if discovered && processed.insert(key) {
            targets.push(index);
        }
    }

    let mut added = Vec::new();
    for index in targets {
        let file = &files[index];
        if matches!(flavor, ArchiveFlavor::Farc) && has_archive_ancestor(&file.path, "epl") {
            coverage.cross_family_archive_candidates += 1;
            issue(
                issues,
                &file.path,
                "cross_family_archive",
                "FARC was discovered inside EPL output; the production staged extractor does not revisit FARC after the EPL stage",
            );
        }
        let inventory = match flavor {
            ArchiveFlavor::Farc => FarcParser.inspect(&file.data, budget),
            ArchiveFlavor::Epl => EplParser.inspect(&file.data, budget),
        };
        let inventory = match inventory {
            Ok(value) => value,
            Err(error) => {
                issue(issues, &file.path, "archive_inspect", error.to_string());
                continue;
            }
        };
        if let Err(error) = usage.charge_inventory(file.depth, &inventory, budget) {
            issue(issues, &file.path, "archive_budget", error.to_string());
            continue;
        }
        coverage.fixed_point_archive_expansions += 1;
        for member in &inventory.members {
            let result = match flavor {
                ArchiveFlavor::Farc => FarcParser.read_member(&file.data, member, budget),
                ArchiveFlavor::Epl => EplParser.read_member(&file.data, member, budget),
            };
            match result {
                Ok(data) => {
                    let raw_name = member
                        .name
                        .as_deref()
                        .map(safe_flat_name)
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| format!("member_{:05}", member.index));
                    let name = ensure_useful_suffix(raw_name, &data);
                    added.push(VirtualFile {
                        path: format!("{}/{}", file.path, name),
                        data,
                        depth: file.depth.saturating_add(1),
                        extractor_selected: true,
                    });
                }
                Err(error) => issue(issues, &file.path, "archive_member", error.to_string()),
            }
        }
    }
    files.extend(added);
}

fn archive_source_paths(files: &[VirtualFile]) -> BTreeSet<String> {
    files
        .iter()
        .filter(|file| {
            matches!(extension(&file.path).as_deref(), Some("hpi") | Some("hpb") | Some("epl"))
                || file.data.get(..4) == Some(b"FARC")
        })
        .map(|file| path_key(&file.path))
        .collect()
}

fn audit_payload(file: &VirtualFile, coverage: &mut CoverageTotals, issues: &mut Vec<AuditIssue>) {
    let data = &file.data;
    if is_stex(data) {
        coverage.stex_files += 1;
        match parse_stex(data) {
            Ok(_) => coverage.stex_parsed += 1,
            Err(error) => issue(issues, &file.path, "stex_declared_but_unparsed", error.to_string()),
        }
        return;
    }

    if matches!(
        data.get(..4),
        Some(b"CTPK") | Some(b"CTXB") | Some(b"ctxb") | Some(b"cmb ")
    ) {
        coverage.unsupported_texture_containers += 1;
        issue(
            issues,
            &file.path,
            "unsupported_texture_container",
            "known texture-capable legacy container is present and is not covered by native extraction",
        );
        return;
    }

    let cgfx_offsets = valid_cgfx_payloads(data);
    if !file.extractor_selected && !cgfx_offsets.is_empty() {
        issue(
            issues,
            &file.path,
            "extractor_probe_gap",
            "valid embedded CGFX was found by the broader audit probe in a RomFS file the production selector would not read",
        );
    }
    for (offset, size) in cgfx_offsets {
        let Some(payload) = data.get(offset..offset + size) else {
            continue;
        };
        coverage.cgfx_payloads += 1;
        match cgfx_top_level_texture_objects(payload) {
            Ok(objects) => {
                coverage.cgfx_top_level_textures_declared += objects.types.len() as u64;
                for object_type in objects.types {
                    match object_type {
                        CGFX_IMAGE_TEXTURE => coverage.cgfx_image_textures_declared += 1,
                        CGFX_CUBE_TEXTURE => {
                            coverage.cgfx_cube_textures_declared += 1;
                            issue(
                                issues,
                                &file.path,
                                "cgfx_cube_texture_unextracted",
                                format!(
                                    "CGFX payload at 0x{offset:X} declares a cube TXOB (type 0x{CGFX_CUBE_TEXTURE:08X})"
                                ),
                            );
                        }
                        CGFX_REFERENCE_TEXTURE => coverage.cgfx_reference_textures_declared += 1,
                        CGFX_PROCEDURAL_TEXTURE => coverage.cgfx_procedural_textures_declared += 1,
                        CGFX_SHADOW_TEXTURE => coverage.cgfx_shadow_textures_declared += 1,
                        other => {
                            coverage.cgfx_unknown_texture_objects += 1;
                            issue(
                                issues,
                                &file.path,
                                "cgfx_unknown_texture_object",
                                format!(
                                    "CGFX payload at 0x{offset:X} declares unknown TXOB type 0x{other:08X}"
                                ),
                            );
                        }
                    }
                }
            }
            Err(error) => issue(
                issues,
                &file.path,
                "cgfx_texture_dictionary",
                format!("CGFX payload at 0x{offset:X}: {error}"),
            ),
        }
        match parse_cgfx(payload) {
            Ok(container) => {
                coverage.cgfx_image_textures_parsed += container.textures.len() as u64;
                let declared = cgfx_top_level_texture_objects(payload)
                    .ok()
                    .map(|objects| {
                        objects
                            .types
                            .iter()
                            .filter(|kind| **kind == CGFX_IMAGE_TEXTURE)
                            .count()
                    })
                    .unwrap_or(0);
                if container.textures.len() != declared {
                    issue(
                        issues,
                        &file.path,
                        "cgfx_image_descriptor_gap",
                        format!(
                            "CGFX payload at 0x{offset:X} declares {declared} image TXOB(s), but {} parsed successfully",
                            container.textures.len()
                        ),
                    );
                }
            }
            Err(error) => issue(
                issues,
                &file.path,
                "cgfx_parse",
                format!("CGFX payload at 0x{offset:X}: {error}"),
            ),
        }
    }

    for offset in valid_bch_offsets(data) {
        let Some(payload) = data.get(offset..) else {
            continue;
        };
        coverage.bch_payloads += 1;
        let header = match parse_bch_header(payload) {
            Ok(value) => value,
            Err(error) => {
                issue(
                    issues,
                    &file.path,
                    "bch_header",
                    format!("BCH payload at 0x{offset:X}: {error}"),
                );
                continue;
            }
        };
        match bch_declared_textures(payload, &header) {
            Ok(declared) => {
                coverage.bch_texture_entries_declared += u64::from(declared.declared);
                coverage.bch_texture_pointers_resolved += u64::from(declared.resolved);
                if declared.declared != declared.resolved {
                    issue(
                        issues,
                        &file.path,
                        "bch_texture_pointer_gap",
                        format!(
                            "BCH payload at 0x{offset:X} declares {} texture entries, but only {} descriptor pointers resolve",
                            declared.declared, declared.resolved
                        ),
                    );
                }
            }
            Err(error) => issue(
                issues,
                &file.path,
                "bch_texture_table",
                format!("BCH payload at 0x{offset:X}: {error}"),
            ),
        }
        match parse_bch(payload) {
            Ok(container) => {
                coverage.bch_texture_entries_parsed += container.textures.len() as u64;
                if let Ok(declared) = bch_declared_textures(payload, &header) {
                    if container.textures.len() != declared.declared as usize {
                        issue(
                            issues,
                            &file.path,
                            "bch_texture_descriptor_gap",
                            format!(
                                "BCH payload at 0x{offset:X} declares {} texture entries, but {} parsed successfully",
                                declared.declared,
                                container.textures.len()
                            ),
                        );
                    }
                }
                for texture in &container.textures {
                    let faces = bch_cube_face_count(payload, &header, texture.descriptor_offset as usize);
                    if faces > 1 {
                        coverage.bch_cube_texture_entries += 1;
                        coverage.bch_cube_faces_declared += u64::from(faces);
                        issue(
                            issues,
                            &file.path,
                            "bch_cube_texture_unextracted",
                            format!(
                                "BCH payload at 0x{offset:X}, texture {} declares {faces} cube-map faces; production extraction exports only the first face",
                                texture.name
                            ),
                        );
                    }
                }
            }
            Err(error) => issue(
                issues,
                &file.path,
                "bch_parse",
                format!("BCH payload at 0x{offset:X}: {error}"),
            ),
        }
    }
}

fn cgfx_top_level_texture_objects(data: &[u8]) -> Result<CgfxTextureObjects, String> {
    if data.len() < 0x14 || data.get(..4) != Some(b"CGFX") {
        return Err("invalid CGFX header".to_owned());
    }
    let header_size = read_u16(data, 6).ok_or("missing CGFX header size")? as usize;
    let data_start = header_size;
    if data.get(data_start..data_start + 4) != Some(b"DATA") {
        return Err(format!("DATA block not found at header size 0x{header_size:X}"));
    }
    let descriptor = data_start
        .checked_add(8 + CGFX_TEXTURE_DICT_INDEX * 8)
        .ok_or("texture dictionary descriptor overflow")?;
    let outer_count = read_u32(data, descriptor).ok_or("missing texture dictionary count")?;
    let raw_dict = read_u32(data, descriptor + 4).ok_or("missing texture dictionary offset")?;
    if outer_count == 0 {
        return Ok(CgfxTextureObjects::default());
    }
    if raw_dict == 0 {
        return Err(format!("texture dictionary has {outer_count} entries but null offset"));
    }
    let dict = descriptor
        .checked_add(4)
        .and_then(|field| field.checked_add(raw_dict as usize))
        .ok_or("texture dictionary offset overflow")?;
    if data.get(dict..dict + 4) != Some(b"DICT") {
        return Err("texture dictionary does not point to DICT".to_owned());
    }
    let count = read_u32(data, dict + 8).ok_or("missing DICT entry count")?;
    if count != outer_count {
        return Err(format!(
            "DATA declares {outer_count} textures but DICT declares {count}"
        ));
    }
    let mut types = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let entry = dict
            .checked_add(0x1c + index * 0x10)
            .ok_or("DICT entry offset overflow")?;
        let data_field = entry.checked_add(0x0c).ok_or("DICT data field overflow")?;
        let raw = read_u32(data, data_field).ok_or("truncated DICT entry")?;
        if raw == 0 {
            return Err(format!("texture DICT entry {index} has null object pointer"));
        }
        let object = data_field
            .checked_add(raw as usize)
            .ok_or("texture object pointer overflow")?;
        let object_type = read_u32(data, object)
            .ok_or_else(|| format!("texture DICT entry {index} points outside CGFX"))?;
        if data.get(object + 4..object + 8) != Some(b"TXOB") {
            return Err(format!("texture DICT entry {index} does not point to TXOB"));
        }
        types.push(object_type);
    }
    Ok(CgfxTextureObjects { types })
}

fn bch_declared_textures(data: &[u8], header: &BchHeader) -> Result<BchDeclaredTextures, String> {
    let section = (header.content_addr as usize)
        .checked_add(BCH_TEXTURE_SECTION_INDEX * 12)
        .ok_or("texture section offset overflow")?;
    let pointer_offset = read_u32(data, section).ok_or("missing BCH texture pointer table")?;
    let count = read_u32(data, section + 4).ok_or("missing BCH texture count")?;
    if count == 0 {
        return Ok(BchDeclaredTextures::default());
    }
    if pointer_offset == 0 {
        return Err(format!("BCH declares {count} textures but pointer table is null"));
    }
    let table = resolve_bch_main_offset(pointer_offset, header, data.len())
        .ok_or("BCH texture pointer table does not resolve")?;
    let end = table
        .checked_add(count as usize * 4)
        .ok_or("BCH texture pointer table overflow")?;
    if end > data.len() {
        return Err("BCH texture pointer table is truncated".to_owned());
    }
    let mut resolved = 0u32;
    for index in 0..count as usize {
        let raw = read_u32(data, table + index * 4).unwrap_or(0);
        if resolve_bch_main_offset(raw, header, data.len()).is_some() {
            resolved += 1;
        }
    }
    Ok(BchDeclaredTextures {
        declared: count,
        resolved,
    })
}

fn bch_cube_face_count(data: &[u8], header: &BchHeader, descriptor: usize) -> u32 {
    let raw_command = read_u32(data, descriptor).unwrap_or(0);
    let word_count = read_u32(data, descriptor + 4).unwrap_or(0);
    let Some(registers) = bch_command_registers(data, header, raw_command, word_count) else {
        return 1;
    };
    let extra_faces = (0x0086u16..=0x008au16)
        .filter(|register| registers.get(register).copied().unwrap_or(0) != 0)
        .count() as u32;
    1 + extra_faces
}

fn bch_command_registers(
    data: &[u8],
    header: &BchHeader,
    raw_pointer: u32,
    word_count: u32,
) -> Option<BTreeMap<u16, u32>> {
    if word_count == 0 || word_count > 0x4000 {
        return None;
    }
    for start in [
        (header.commands_addr as usize).checked_add(raw_pointer as usize),
        Some(raw_pointer as usize),
    ]
    .into_iter()
    .flatten()
    {
        let bytes = (word_count as usize).checked_mul(4)?;
        if start.checked_add(bytes).is_none_or(|end| end > data.len()) {
            continue;
        }
        let mut registers = BTreeMap::new();
        let end = start + bytes;
        let mut position = start;
        while position + 8 <= end {
            let parameter = read_u32(data, position)?;
            let command = read_u32(data, position + 4)?;
            let register = (command & 0xffff) as u16;
            let extra = ((command >> 20) & 0xff) as usize;
            let consecutive = command & 0x8000_0000 != 0;
            position += 8;
            registers.insert(register, parameter);
            for index in 0..extra {
                if position + 4 > end {
                    break;
                }
                let value = read_u32(data, position)?;
                position += 4;
                let target = if consecutive {
                    register.wrapping_add(index as u16 + 1)
                } else {
                    register
                };
                registers.insert(target, value);
            }
            if position & 7 != 0 {
                position = position.saturating_add(4);
            }
        }
        if !registers.is_empty() {
            return Some(registers);
        }
    }
    None
}

fn resolve_bch_main_offset(raw: u32, header: &BchHeader, data_len: usize) -> Option<usize> {
    if raw == 0 {
        return None;
    }
    let content = header.content_addr as usize;
    let raw = raw as usize;
    let upper = if header.strings_addr > header.content_addr {
        (header.strings_addr as usize).min(data_len)
    } else {
        data_len
    };
    for value in [content.checked_add(raw), Some(raw)].into_iter().flatten() {
        if content <= value && value < upper {
            return Some(value);
        }
    }
    [content.checked_add(raw), Some(raw)]
        .into_iter()
        .flatten()
        .find(|value| *value < data_len)
}

fn valid_cgfx_payloads(data: &[u8]) -> Vec<(usize, usize)> {
    let mut output = Vec::new();
    let mut search = 0usize;
    while search + 4 <= data.len() {
        let Some(relative) = find_magic(&data[search..], b"CGFX") else {
            break;
        };
        let offset = search + relative;
        search = offset.saturating_add(4);
        let Some(header_size) = read_u16(data, offset + 6) else {
            continue;
        };
        let Some(size) = read_u32(data, offset + 0x0c).map(|value| value as usize) else {
            continue;
        };
        if header_size < 0x14 || size < 0x20 {
            continue;
        }
        if data.get(offset + 4..offset + 6) != Some(&[0xff, 0xfe]) {
            continue;
        }
        if offset.checked_add(size).is_some_and(|end| end <= data.len()) {
            output.push((offset, size));
        }
    }
    output
}

fn valid_bch_offsets(data: &[u8]) -> Vec<usize> {
    let mut output = Vec::new();
    let mut search = 0usize;
    while search + 4 <= data.len() {
        let Some(relative) = find_magic(&data[search..], b"BCH\0") else {
            break;
        };
        let offset = search + relative;
        search = offset.saturating_add(4);
        if parse_bch_header(&data[offset..]).is_ok() {
            output.push(offset);
        }
    }
    output
}

fn extractor_candidate_path(path: &str) -> bool {
    matches!(
        extension(path).as_deref(),
        Some("hpi")
            | Some("hpb")
            | Some("stex")
            | Some("bch")
            | Some("bcres")
            | Some("bcmdl")
            | Some("cmb")
            | Some("ctpk")
            | Some("ctxb")
            | Some("bam")
            | Some("bam2")
            | Some("farc")
            | Some("epl")
    )
}

fn extractor_romfs_probe_candidate(probe: &[u8]) -> bool {
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

fn ensure_useful_suffix(mut name: String, data: &[u8]) -> String {
    if extension(&name).is_some() {
        return name;
    }
    let suffix = match data.get(..4) {
        Some(b"STEX") => ".stex",
        Some(b"CGFX") => ".cgfx",
        Some(b"BCH\0") => ".bch",
        Some(b"ATBC") => ".bam",
        Some(b"BAM2") => ".bam2",
        Some(b"CTPK") => ".ctpk",
        Some(b"CTXB") | Some(b"ctxb") => ".ctxb",
        Some(b"FARC") => ".farc",
        _ if data.starts_with(b"EPL") => ".epl",
        _ => ".bin",
    };
    name.push_str(suffix);
    name
}

fn safe_relative_name(value: &str) -> String {
    value
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .map(safe_component)
        .collect::<Vec<_>>()
        .join("/")
}

fn safe_flat_name(value: &str) -> String {
    safe_component(&value.replace(['\\', '/'], "_"))
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim_matches(['.', ' ', '_'])
        .chars()
        .take(180)
        .collect()
}

fn has_archive_ancestor(path: &str, extension: &str) -> bool {
    path.to_ascii_lowercase()
        .contains(&format!(".{extension}/"))
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn path_key(path: &str) -> String {
    normalize_path(path).to_ascii_lowercase()
}

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, ext)| ext.trim().to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
}

fn replace_extension(path: &str, replacement: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, _)) => format!("{stem}.{replacement}"),
        None => format!("{path}.{replacement}"),
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

fn find_magic(data: &[u8], magic: &[u8]) -> Option<usize> {
    if magic.is_empty() {
        return None;
    }
    data.windows(magic.len()).position(|window| window == magic)
}

fn contains_magic(data: &[u8], magic: &[u8]) -> bool {
    find_magic(data, magic).is_some()
}

fn issue(issues: &mut Vec<AuditIssue>, source: &str, stage: &str, message: impl Into<String>) {
    issues.push(AuditIssue {
        source: source.to_owned(),
        stage: stage.to_owned(),
        message: message.into(),
    });
}
