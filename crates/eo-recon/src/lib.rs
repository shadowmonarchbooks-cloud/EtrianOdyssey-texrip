//! Privacy-safe format reconnaissance for planned Etrian Odyssey profiles.
//!
//! Reconnaissance deliberately reports aggregate metadata only. It never emits
//! RomFS paths, proprietary bytes, payload offsets, or content hashes. The goal
//! is to identify which already-known parser families are actually present
//! before 0.70 broadens native extraction beyond the Untold titles.

use eo_core::GameId;
use eo_profiles::{detect_verified_profile, ProfileStatus};
use eo_rom::{RomError, RomReader};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const RECON_SCHEMA: &str = "eo-texrip-universal-eo-recon-v1";
pub const RECON_PROBE_BYTES: usize = 0x4_0000;

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
    let mut romfs_bytes_total = 0u64;
    let mut largest_file_bytes = 0u64;
    let mut files_probed = 0u64;
    let mut probe_read_errors = 0u64;

    for entry in &entries {
        romfs_bytes_total = romfs_bytes_total.saturating_add(entry.size);
        largest_file_bytes = largest_file_bytes.max(entry.size);
        increment(&mut extensions, extension_bucket(&entry.virtual_path));

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
                increment(&mut embedded_magics, name.to_owned());
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
        privacy: "Aggregate counts only; no RomFS paths, payload bytes, payload offsets, or content hashes are emitted."
            .to_owned(),
    })
}

fn increment(map: &mut BTreeMap<String, u64>, key: String) {
    *map.entry(key).or_default() += 1;
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

fn leading_magic(data: &[u8]) -> Option<&'static str> {
    for (name, magic) in known_four_byte_magics() {
        if data.starts_with(magic) {
            return Some(match name {
                "ctxb_lower" => "ctxb",
                other => other,
            });
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

    #[test]
    fn universal_eo_recon_reports_only_aggregate_format_evidence() {
        let mut files = BTreeMap::new();
        files.insert("UI/IMAGE.STEX".to_owned(), b"STEXfixture".to_vec());
        let mut wrapped = vec![0xAA; 32];
        wrapped.extend_from_slice(b"CGFXfixture");
        files.insert("MODEL/WRAPPED.BIN".to_owned(), wrapped);
        files.insert("PACK/DATA.CTPK".to_owned(), b"CTPKfixture".to_vec());
        files.insert("README".to_owned(), Vec::new());

        let report = recon_reader(&FixtureReader::eo4(files)).unwrap();
        assert_eq!(report.schema, RECON_SCHEMA);
        assert_eq!(report.profile_id, "eo4");
        assert_eq!(report.game_id, GameId::EtrianOdysseyIv);
        assert_eq!(report.profile_status, ProfileStatus::PlannedResearch);
        assert_eq!(report.romfs_files, 4);
        assert_eq!(report.extensions.get("stex"), Some(&1));
        assert_eq!(report.extensions.get("bin"), Some(&1));
        assert_eq!(report.extensions.get("ctpk"), Some(&1));
        assert_eq!(report.extensions.get("<none>"), Some(&1));
        assert_eq!(report.leading_magics.get("stex"), Some(&1));
        assert_eq!(report.leading_magics.get("ctpk"), Some(&1));
        assert_eq!(report.embedded_magics.get("cgfx"), Some(&1));
        assert_eq!(report.probe_read_errors, 0);
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
