use eo_archives::ExtractionBudget;
use eo_rom::{RomEntry, RomError, RomIdentityHint, RomImageKind, RomMetadata, RomReader};
use eo_untold::inventory_reader;
use std::collections::BTreeMap;

struct FakeRom {
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
        Ok(RomIdentityHint {
            title_id: Some("00040000000EC700".parse().unwrap()),
            product_code: Some("CTR-P-BSK-USA".to_owned()),
        })
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

fn truncated_atbc_cgfx_probe() -> Vec<u8> {
    let mut data = vec![0u8; 0x80];
    data[..4].copy_from_slice(b"ATBC");
    let cgfx = 0x20usize;
    data[cgfx..cgfx + 4].copy_from_slice(b"CGFX");
    data[cgfx + 4..cgfx + 6].copy_from_slice(&[0xff, 0xfe]);
    data[cgfx + 6..cgfx + 8].copy_from_slice(&0x14u16.to_le_bytes());
    data[cgfx + 0x0c..cgfx + 0x10].copy_from_slice(&0x200u32.to_le_bytes());
    data
}

fn truncated_wrapped_cgfx_probe() -> Vec<u8> {
    let mut data = truncated_atbc_cgfx_probe();
    data[..4].fill(0);
    data
}

#[test]
fn truncated_atbc_cgfx_header_is_inventory_and_strict_candidate_evidence() {
    let rom = FakeRom {
        files: BTreeMap::from([(
            "models/enemy.bam".to_owned(),
            truncated_atbc_cgfx_probe(),
        )]),
    };

    let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
    assert_eq!(inventory.summary.strict_candidate_files, 1);
    assert_eq!(inventory.summary.atbc_files, 1);
    assert_eq!(inventory.summary.cgfx_files, 1);
    assert_eq!(inventory.model_payloads, 0);
    assert!(inventory.assets.is_empty());
    assert!(inventory.issues.is_empty());
}

#[test]
fn extension_selected_wrapped_cgfx_is_strict_candidate_evidence() {
    let rom = FakeRom {
        files: BTreeMap::from([(
            "models/wrapped.bcmdl".to_owned(),
            truncated_wrapped_cgfx_probe(),
        )]),
    };

    let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
    assert_eq!(inventory.summary.strict_candidate_files, 1);
    assert_eq!(inventory.summary.atbc_files, 0);
    assert_eq!(inventory.summary.cgfx_files, 1);
    assert_eq!(inventory.model_payloads, 0);
    assert!(inventory.assets.is_empty());
    assert!(inventory.issues.is_empty());
}
