//! Privacy-safe format reconnaissance for planned Etrian Odyssey profiles.
//!
//! Reconnaissance deliberately reports aggregate metadata only. It never emits
//! RomFS paths, proprietary bytes, payload offsets, member names, or content
//! hashes. The goal is to identify which already-known parser families are
//! actually present before 0.70 broadens native extraction beyond Untold.

use eo_archives::{ArchiveParser, EplParser, ExtractionBudget};
use eo_core::GameId;
use eo_profiles::{detect_verified_profile, ProfileStatus};
use eo_rom::{RomError, RomReader};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const RECON_SCHEMA: &str = "eo-texrip-universal-eo-recon-v2";
pub const RECON_PROBE_BYTES: usize = 0x4_0000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveReconSummary {
    pub hpi_files: u64,
    pub hpb_files: u64,
    pub hpi_hpb_pairs: u64,
    pub hpi_index_errors: u64,
    pub hpi_members: u64,
    pub hpi_members_marked_compressed: u64,
    pub hpi_member_extensions: BTreeMap<String, u64>,
    pub epl_files_inspected: u64,
    pub epl_inspect_errors: u64,
    pub epl_members: u64,
    pub epl_member_extensions: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniversalEoReconReport {
    pub schema: String,
    pub profile_id: String,
    pub game_id: GameId,
    pub profile_status: ProfileStatus,
    pub title_id: Option<String>,
    pub product_code: Option<String>,
    pub romfs_files: u64,
    pub romfs_bytes_total: u64,
    pub largest_file_bytes: u64,
    pub files_probed: u64,
    pub probe_read_errors: u64,
    pub probe_bytes_per_file: usize,
    pub extensions: BTreeMap<String, u64>,
    pub leading_magics: BTreeMap<String, u64>,
    pub embedded_magics: BTreeMap<String, u64>,
    pub archives: ArchiveReconSummary,
    pub privacy: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReconError {
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

pub fn recon_reader<R: RomReader>(reader: &R) -> Result<UniversalEoReconReport, ReconError> {
    let hint = reader.identity_hint()?;
    let profile = detect_verified_profile(hint.title_id, hint.product_code.as_deref()).ok_or_else(|| {
        ReconError::UnsupportedGame {
            title_id: hint.title_id.map(|value| value.to_string()),
            product_code: hint.product_code.clone(),
        }
    })?;

    if !matches!(
        profile.game_id,
        GameId::EtrianOdysseyIv | GameId::EtrianOdysseyV | GameId::EtrianOdysseyNexus
    ) {
        return Err(ReconError::UnsupportedGame {
            title_id: hint.title_id.map(|value| value.to_string()),
            product_code: hint.product_code,
        });
    }

    let entries = reader.entries()?;
    let mut extensions = BTreeMap::new();
    let mut leading_magics = BTreeMap::new();
    let mut embedded_magics = BTreeMap::new();
    let mut archives = ArchiveReconSummary::default();
    let mut romfs_bytes_total = 0u64;
    let mut largest_file_bytes = 0u64;
    let mut files_probed = 0u64;
    let mut probe_read_errors = 0u64;

    let mut hpi_keys = BTreeSet::new();
    let mut hpb_keys = BTreeSet::new();
    for entry in &entries {
        match extension_bucket(&entry.virtual_path).as_str() {
            "hpi" => {
                archives.hpi_files += 1;
                hpi_keys.insert(archive_pair_key(&entry.virtual_path));
            }
            "hpb" => {
                archives.hpb_files += 1;
                hpb_keys.insert(archive_pair_key(&entry.virtual_path));
            }
            _ => {}
        }
    }
    archives.hpi_hpb_pairs = hpi_keys.intersection(&hpb_keys).count() as u64;

    let budget = ExtractionBudget::default();
    for entry in &entries {
        romfs_bytes_total = romfs_bytes_total.saturating_add(entry.size);
        largest_file_bytes = largest_file_bytes.max(entry.size);
        let extension = extension_bucket(&entry.virtual_path);
        increment(&mut extensions, extension.clone());

        if extension == "hpi" && entry.size != 0 {
            match reader.read_entry(&entry.virtual_path) {
                Ok(data) => match inspect_hpi_index(&data) {
                    Ok(index) => {
                        archives.hpi_members = archives.hpi_members.saturating_add(index.members);
                        archives.hpi_members_marked_compressed = archives
                            .hpi_members_marked_compressed
                            .saturating_add(index.members_marked_compressed);
                        merge_counts(
                            &mut archives.hpi_member_extensions,
                            &index.member_extensions,
                        );
                    }
                    Err(()) => {
                        archives.hpi_index_errors = archives.hpi_index_errors.saturating_add(1)
                    }
                },
                Err(_) => archives.hpi_index_errors = archives.hpi_index_errors.saturating_add(1),
            }
        }

        if extension == "epl" && entry.size != 0 {
            match reader.read_entry(&entry.virtual_path) {
                Ok(data) => {
                    let parser = EplParser;
                    if !parser.probe(&data) {
                        archives.epl_inspect_errors = archives.epl_inspect_errors.saturating_add(1);
                    } else {
                        match parser.inspect(&data, budget) {
                            Ok(inventory) => {
                                archives.epl_files_inspected =
                                    archives.epl_files_inspected.saturating_add(1);
                                archives.epl_members = archives
                                    .epl_members
                                    .saturating_add(inventory.members.len() as u64);
                                for member in inventory.members {
                                    let bucket = member
                                        .name
                                        .as_deref()
                                        .map(extension_bucket)
                                        .unwrap_or_else(|| "<unnamed>".to_owned());
                                    increment(&mut archives.epl_member_extensions, bucket);
                                }
                            }
                            Err(_) => {
                                archives.epl_inspect_errors =
                                    archives.epl_inspect_errors.saturating_add(1)
                            }
                        }
                    }
                }
                Err(_) => {
                    archives.epl_inspect_errors = archives.epl_inspect_errors.saturating_add(1)
                }
            }
        }

        if entry.size == 0 {
            continue;
        }
        let probe_len = usize::try_from(entry.size.min(RECON_PROBE_BYTES as u64))
            .unwrap_or(RECON_PROBE_BYTES);
        let probe = match reader.read_entry_prefix(&entry.virtual_path, probe_len) {
            Ok(value) => value,
            Err(_) => {
                probe_read_errors = probe_read_errors.saturating_add(1);
                continue;
            }
        };
        files_probed = files_probed.saturating_add(1);

        if let Some(magic) = leading_magic(&probe) {
            increment(&mut leading_magics, magic.to_owned());
        }
        for (name, magic) in known_four_byte_magics() {
            if find_magic_after_start(&probe, magic) {
                increment(&mut embedded_magics, normalize_magic_name(name).to_owned());
            }
        }
    }

    Ok(UniversalEoReconReport {
        schema: RECON_SCHEMA.to_owned(),
        profile_id: profile.profile_id.to_owned(),
        game_id: profile.game_id,
        profile_status: profile.status,
        title_id: hint.title_id.map(|value| value.to_string()),
        product_code: hint.product_code,
        romfs_files: entries.len() as u64,
        romfs_bytes_total,
        largest_file_bytes,
        files_probed,
        probe_read_errors,
        probe_bytes_per_file: RECON_PROBE_BYTES,
        extensions,
        leading_magics,
        embedded_magics,
        archives,
        privacy: "Aggregate counts only; no RomFS paths, archive member names, proprietary bytes, payload offsets, or content hashes are emitted."
            .to_owned(),
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HpiIndexSummary {
    members: u64,
    members_marked_compressed: u64,
    member_extensions: BTreeMap<String, u64>,
}

fn inspect_hpi_index(data: &[u8]) -> Result<HpiIndexSummary, ()> {
    const HEADER: usize = 0x18;
    const ENTRY: usize = 16;
    if data.len() < HEADER || data.get(..4) != Some(b"HPIH") {
        return Err(());
    }
    let unknown_count = usize::from(read_u16_le(data, 0x12)?);
    let file_count = usize::from(read_u16_le(data, 0x14)?);
    let unknown_bytes = unknown_count.checked_mul(4).ok_or(())?;
    let file_table = HEADER.checked_add(unknown_bytes).ok_or(())?;
    let table_size = file_count.checked_mul(ENTRY).ok_or(())?;
    let names_base = file_table.checked_add(table_size).ok_or(())?;
    if names_base > data.len() || file_table.checked_add(table_size).ok_or(())? > data.len() {
        return Err(());
    }
    let names = &data[names_base..];
    let mut summary = HpiIndexSummary::default();

    for index in 0..file_count {
        let entry_offset = file_table
            .checked_add(index.checked_mul(ENTRY).ok_or(())?)
            .ok_or(())?;
        let name_offset = usize::try_from(read_u32_le(data, entry_offset)?).map_err(|_| ())?;
        let marked_compressed = read_u32_le(data, entry_offset + 12)? != 0;
        if marked_compressed {
            summary.members_marked_compressed =
                summary.members_marked_compressed.saturating_add(1);
        }
        let bucket = if let Some(tail) = names.get(name_offset..) {
            let end = tail.iter().position(|byte| *byte == 0).unwrap_or(tail.len());
            extension_bucket_bytes(&tail[..end])
        } else {
            "<invalid-name>".to_owned()
        };
        increment(&mut summary.member_extensions, bucket);
        summary.members = summary.members.saturating_add(1);
    }
    Ok(summary)
}

fn read_u16_le(data: &[u8], offset: usize) -> Result<u16, ()> {
    let bytes = data.get(offset..offset.checked_add(2).ok_or(())?).ok_or(())?;
    Ok(u16::from_le_bytes(bytes.try_into().map_err(|_| ())?))
}

fn read_u32_le(data: &[u8], offset: usize) -> Result<u32, ()> {
    let bytes = data.get(offset..offset.checked_add(4).ok_or(())?).ok_or(())?;
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| ())?))
}

fn extension_bucket_bytes(path: &[u8]) -> String {
    let leaf = path
        .rsplit(|byte| matches!(*byte, b'/' | b'\\'))
        .next()
        .unwrap_or(path);
    let Some(dot) = leaf.iter().rposition(|byte| *byte == b'.') else {
        return "<none>".to_owned();
    };
    if dot == 0 || dot + 1 >= leaf.len() {
        return "<none>".to_owned();
    }
    let extension = &leaf[dot + 1..];
    if extension.iter().all(u8::is_ascii_alphanumeric) {
        extension
            .iter()
            .map(u8::to_ascii_lowercase)
            .map(char::from)
            .collect()
    } else {
        "<non-ascii>".to_owned()
    }
}

fn increment(map: &mut BTreeMap<String, u64>, key: String) {
    *map.entry(key).or_default() += 1;
}

fn merge_counts(target: &mut BTreeMap<String, u64>, source: &BTreeMap<String, u64>) {
    for (key, count) in source {
        *target.entry(key.clone()).or_default() += count;
    }
}

fn archive_pair_key(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let without_extension = normalized.rsplit_once('.').map_or(normalized.as_str(), |(stem, _)| stem);
    without_extension.to_ascii_lowercase()
}

fn extension_bucket(path: &str) -> String {
    let leaf = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match leaf.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            extension.to_ascii_lowercase()
        }
        _ => "<none>".to_owned(),
    }
}

fn known_four_byte_magics() -> [(&'static str, &'static [u8]); 10] {
    [
        ("stex", b"STEX"),
        ("cgfx", b"CGFX"),
        ("bch", b"BCH\0"),
        ("atbc", b"ATBC"),
        ("ctpk", b"CTPK"),
        ("ctxb", b"CTXB"),
        ("ctxb_lower", b"ctxb"),
        ("cmb", b"cmb "),
        ("farc", b"FARC"),
        ("sir0", b"SIR0"),
    ]
}

fn normalize_magic_name(name: &str) -> &str {
    if name == "ctxb_lower" {
        "ctxb"
    } else {
        name
    }
}

fn leading_magic(data: &[u8]) -> Option<&'static str> {
    for (name, magic) in known_four_byte_magics() {
        if data.starts_with(magic) {
            return Some(normalize_magic_name(name));
        }
    }
    if data.starts_with(b"EPL") {
        return Some("epl");
    }
    None
}

fn find_magic_after_start(data: &[u8], magic: &[u8]) -> bool {
    if data.len() <= magic.len() {
        return false;
    }
    data[1..].windows(magic.len()).any(|window| window == magic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eo_core::TitleId;
    use eo_rom::{RomEntry, RomIdentityHint, RomImageKind, RomMetadata};

    #[derive(Clone)]
    struct FixtureReader {
        hint: RomIdentityHint,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl FixtureReader {
        fn eo4(files: BTreeMap<String, Vec<u8>>) -> Self {
            Self {
                hint: RomIdentityHint {
                    title_id: Some("00040000000BD300".parse::<TitleId>().unwrap()),
                    product_code: Some("CTR-P-ASJE".to_owned()),
                },
                files,
            }
        }
    }

    impl RomReader for FixtureReader {
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

        fn read_entry_prefix(
            &self,
            virtual_path: &str,
            max_bytes: usize,
        ) -> Result<Vec<u8>, RomError> {
            let data = self
                .files
                .get(virtual_path)
                .ok_or_else(|| RomError::MissingEntry(virtual_path.to_owned()))?;
            Ok(data[..data.len().min(max_bytes)].to_vec())
        }
    }

    fn synthetic_hpi(names: &[&[u8]]) -> Vec<u8> {
        let table_size = names.len() * 16;
        let names_base = 0x18 + table_size;
        let names_len = names.iter().map(|name| name.len() + 1).sum::<usize>();
        let mut data = vec![0u8; names_base + names_len];
        data[0..4].copy_from_slice(b"HPIH");
        data[0x14..0x16].copy_from_slice(&(names.len() as u16).to_le_bytes());
        let mut name_cursor = 0usize;
        for (index, name) in names.iter().enumerate() {
            let offset = 0x18 + index * 16;
            data[offset..offset + 4].copy_from_slice(&(name_cursor as u32).to_le_bytes());
            data[offset + 8..offset + 12].copy_from_slice(&16u32.to_le_bytes());
            if index == 1 {
                data[offset + 12..offset + 16].copy_from_slice(&32u32.to_le_bytes());
            }
            let start = names_base + name_cursor;
            data[start..start + name.len()].copy_from_slice(name);
            name_cursor += name.len() + 1;
        }
        data
    }

    #[test]
    fn universal_eo_recon_reports_only_aggregate_format_evidence() {
        let mut files = BTreeMap::new();
        files.insert("UI/IMAGE.STEX".to_owned(), b"STEXfixture".to_vec());
        let mut wrapped = vec![0xAA; 32];
        wrapped.extend_from_slice(b"CGFXfixture");
        files.insert("MODEL/WRAPPED.BIN".to_owned(), wrapped);
        files.insert("PACK/DATA.CTPK".to_owned(), b"CTPKfixture".to_vec());
        files.insert("README".to_owned(), Vec::new());
        files.insert(
            "ROOT/DATA.HPI".to_owned(),
            synthetic_hpi(&[b"MODEL/FOO.BAM", b"TEX/BAR.STEX"]),
        );
        files.insert("ROOT/DATA.HPB".to_owned(), vec![0u8; 64]);

        let report = recon_reader(&FixtureReader::eo4(files)).unwrap();
        assert_eq!(report.schema, RECON_SCHEMA);
        assert_eq!(report.profile_id, "eo4");
        assert_eq!(report.game_id, GameId::EtrianOdysseyIv);
        assert_eq!(report.profile_status, ProfileStatus::PlannedResearch);
        assert_eq!(report.romfs_files, 6);
        assert_eq!(report.extensions.get("stex"), Some(&1));
        assert_eq!(report.extensions.get("bin"), Some(&1));
        assert_eq!(report.extensions.get("ctpk"), Some(&1));
        assert_eq!(report.extensions.get("<none>"), Some(&1));
        assert_eq!(report.leading_magics.get("stex"), Some(&1));
        assert_eq!(report.leading_magics.get("ctpk"), Some(&1));
        assert_eq!(report.embedded_magics.get("cgfx"), Some(&1));
        assert_eq!(report.archives.hpi_hpb_pairs, 1);
        assert_eq!(report.archives.hpi_members, 2);
        assert_eq!(report.archives.hpi_members_marked_compressed, 1);
        assert_eq!(report.archives.hpi_member_extensions.get("bam"), Some(&1));
        assert_eq!(report.archives.hpi_member_extensions.get("stex"), Some(&1));
        assert_eq!(report.probe_read_errors, 0);
    }

    #[test]
    fn hpi_index_rejects_truncated_tables_without_emitting_names() {
        let mut hpi = synthetic_hpi(&[b"TEX/ONE.STEX"]);
        hpi.truncate(0x1c);
        assert_eq!(inspect_hpi_index(&hpi), Err(()));
    }

    #[test]
    fn untold_identity_is_not_accepted_as_universal_eo_recon_target() {
        let reader = FixtureReader {
            hint: RomIdentityHint {
                title_id: Some("00040000000EC700".parse::<TitleId>().unwrap()),
                product_code: Some("CTR-P-BSKE".to_owned()),
            },
            files: BTreeMap::new(),
        };
        assert!(matches!(
            recon_reader(&reader),
            Err(ReconError::UnsupportedGame { .. })
        ));
    }
}
