use eo_archives::ExtractionBudget;
use eo_rom::{
    RomEntry, RomError, RomIdentityHint, RomImageKind, RomMetadata, RomReader,
};
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

#[test]
fn canonical_asset_descriptor_sha_matches_frozen_python_schema_one() {
    // Frozen Python canonical descriptor JSON for these two assets hashes to
    // 6650c2dbe340e7578e9a61d3d182df9398584815e327a4d479b02e8de2f34d5a.
    // This pins key ordering, compact separators, asset sorting, parser labels,
    // category rules, and the CityHash64 candidate values in one cross-language
    // compatibility vector.
    let rom = FakeRom {
        files: BTreeMap::from([
            ("ui/a.stex".to_owned(), stex_a8(0x11)),
            ("enemy/b.stex".to_owned(), stex_a8(0x22)),
        ]),
    };
    let inventory = inventory_reader(&rom, ExtractionBudget::default()).unwrap();
    let fingerprint = inventory.structural_fingerprint();

    assert_eq!(fingerprint.asset_count, 2);
    assert_eq!(fingerprint.candidate_hash_count, 2);
    assert_eq!(
        fingerprint.asset_descriptor_sha256,
        "6650c2dbe340e7578e9a61d3d182df9398584815e327a4d479b02e8de2f34d5a"
    );
    assert_eq!(fingerprint.category_counts.get("ui"), Some(&1));
    assert_eq!(fingerprint.category_counts.get("monsters"), Some(&1));
}
