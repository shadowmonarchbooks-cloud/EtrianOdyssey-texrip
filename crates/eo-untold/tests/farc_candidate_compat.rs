use eo_archives::ExtractionBudget;
use eo_rom::{RomEntry, RomError, RomIdentityHint, RomImageKind, RomMetadata, RomReader};
use eo_untold::inventory_reader;
use std::collections::BTreeMap;

#[derive(Clone)]
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

fn farc_with_member(name: &str, payload: &[u8]) -> Vec<u8> {
    let mut data = vec![0u8; 0xc0 + payload.len()];
    data[0..4].copy_from_slice(b"FARC");
    data[0x20..0x24].copy_from_slice(&4u32.to_le_bytes());
    data[0x24..0x28].copy_from_slice(&0x40u32.to_le_bytes());
    data[0x28..0x2c].copy_from_slice(&0x80u32.to_le_bytes());
    data[0x2c..0x30].copy_from_slice(&0xc0u32.to_le_bytes());
    data[0x30..0x34].copy_from_slice(&(payload.len() as u32).to_le_bytes());

    let sir0 = 0x40usize;
    data[sir0..sir0 + 4].copy_from_slice(b"SIR0");
    data[sir0 + 4..sir0 + 8].copy_from_slice(&0x10u32.to_le_bytes());
    data[sir0 + 8..sir0 + 12].copy_from_slice(&0x70u32.to_le_bytes());
    data[sir0 + 0x10..sir0 + 0x14].copy_from_slice(&0x20u32.to_le_bytes());
    data[sir0 + 0x14..sir0 + 0x18].copy_from_slice(&1u32.to_le_bytes());
    data[sir0 + 0x18..sir0 + 0x1c].copy_from_slice(&0u32.to_le_bytes());
    data[sir0 + 0x20..sir0 + 0x24].copy_from_slice(&0x40u32.to_le_bytes());
    data[sir0 + 0x24..sir0 + 0x28].copy_from_slice(&0u32.to_le_bytes());
    data[sir0 + 0x28..sir0 + 0x2c].copy_from_slice(&(payload.len() as u32).to_le_bytes());

    for (index, unit) in format!("{name}\0").encode_utf16().enumerate() {
        let offset = sir0 + 0x40 + index * 2;
        data[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
    }
    data[0xc0..0xc0 + payload.len()].copy_from_slice(payload);
    data
}

#[test]
fn farc_archive_is_not_itself_a_strict_texture_candidate() {
    let payload = stex_a8(0x34);
    let rom = FakeRom {
        files: BTreeMap::from([(
            "data/models.farc".to_owned(),
            farc_with_member("nested.stex", &payload),
        )]),
    };

    let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
    assert_eq!(inventory.summary.farc_archives, 1);
    assert_eq!(inventory.summary.farc_files, 1);
    assert_eq!(inventory.summary.strict_candidate_files, 1);
    assert_eq!(inventory.summary.stex_files, 1);
    assert_eq!(inventory.summary.decoded_before_dedup, 1);
    assert_eq!(inventory.assets.len(), 1);
    assert!(inventory.issues.is_empty());
}
