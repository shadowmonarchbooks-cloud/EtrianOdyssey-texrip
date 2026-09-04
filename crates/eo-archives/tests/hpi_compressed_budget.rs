use eo_archives::{ExtractionBudget, HpiHpbParser};

fn hpi_for_member(name: &[u8], compressed_size: u32, decompressed_size: u32) -> Vec<u8> {
    let mut hpi = vec![0u8; 0x28 + name.len() + 1];
    hpi[0..4].copy_from_slice(b"HPIH");
    hpi[0x12..0x14].copy_from_slice(&0u16.to_le_bytes());
    hpi[0x14..0x16].copy_from_slice(&1u16.to_le_bytes());
    hpi[0x18..0x1c].copy_from_slice(&0u32.to_le_bytes());
    hpi[0x1c..0x20].copy_from_slice(&0u32.to_le_bytes());
    hpi[0x20..0x24].copy_from_slice(&compressed_size.to_le_bytes());
    hpi[0x24..0x28].copy_from_slice(&decompressed_size.to_le_bytes());
    hpi[0x28..0x28 + name.len()].copy_from_slice(name);
    hpi
}

fn literal_acmp(payload: &[u8]) -> Vec<u8> {
    assert!(!payload.is_empty() && payload.len() <= 8);
    let compressed_size = payload.len() + 1 + 8;
    let mut block = vec![0u8; 0x20 + compressed_size];
    block[0..4].copy_from_slice(b"ACMP");
    block[0x04..0x08].copy_from_slice(&(compressed_size as u32).to_le_bytes());
    block[0x08..0x0c].copy_from_slice(&0x20u32.to_le_bytes());
    block[0x10..0x14].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    block[0x20..0x20 + payload.len()].copy_from_slice(payload);
    block[0x20 + payload.len()] = 0;
    let trailer = 0x20 + payload.len() + 1;
    let packed = (8u32 << 24) | payload.len() as u32;
    block[trailer..trailer + 4].copy_from_slice(&packed.to_le_bytes());
    block[trailer + 4..trailer + 8].copy_from_slice(&0u32.to_le_bytes());
    block
}

#[test]
fn compressed_source_span_may_exceed_output_member_budget() {
    let expected = vec![1, 2, 3, 4];
    let hpb = literal_acmp(&expected);
    assert!(hpb.len() > expected.len());
    let hpi = hpi_for_member(b"compressed.bin", hpb.len() as u32, u32::MAX);
    let budget = ExtractionBudget {
        max_member_bytes: expected.len() as u64,
        ..ExtractionBudget::default()
    };

    let parser = HpiHpbParser;
    let inventory = parser.inspect(&hpi, &hpb, budget).unwrap();
    assert_eq!(inventory.members.len(), 1);
    assert_eq!(inventory.members[0].expanded_size, Some(expected.len() as u64));
    assert!(inventory.members[0].stored_size > budget.max_member_bytes);
    assert_eq!(
        parser
            .read_member(&hpi, &hpb, &inventory.members[0], budget)
            .unwrap(),
        expected
    );
}
