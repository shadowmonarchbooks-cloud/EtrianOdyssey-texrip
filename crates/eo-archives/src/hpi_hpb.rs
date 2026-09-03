use crate::bytes::{ByteRange, ByteReader};
use crate::{
    enforce_archive_budget, enforce_inventory_budget, ArchiveError, ArchiveInventory, ArchiveKind,
    ArchiveMember, ExtractionBudget,
};
use encoding_rs::SHIFT_JIS;

const HPI_HEADER_SIZE: u64 = 0x18;
const HPI_ENTRY_SIZE: u64 = 16;
const ACMP_MIN_HEADER_SIZE: u64 = 0x20;
const REVERSE_LZ_HISTORY_SIZE: usize = 0x8000;

#[derive(Clone, Copy, Debug, Default)]
pub struct HpiHpbParser;

#[derive(Clone, Debug)]
struct HpiEntry {
    index: u64,
    filename: String,
    file_offset: u64,
    compressed_size: u64,
    declared_decompressed_size: u64,
}

impl HpiHpbParser {
    pub fn probe_index(&self, hpi: &[u8]) -> bool {
        hpi.len() >= HPI_HEADER_SIZE as usize && hpi.get(..4) == Some(b"HPIH")
    }

    pub fn inspect(
        &self,
        hpi: &[u8],
        hpb: &[u8],
        budget: ExtractionBudget,
    ) -> Result<ArchiveInventory, ArchiveError> {
        enforce_archive_budget(hpi.len() as u64, budget)?;
        enforce_archive_budget(hpb.len() as u64, budget)?;
        let entries = parse_hpi(hpi, budget)?;
        let hpb_reader = ByteReader::new(hpb);
        let mut members = Vec::with_capacity(entries.len());

        for entry in entries {
            if entry.declared_decompressed_size > budget.max_member_bytes {
                return Err(ArchiveError::BudgetExceeded(format!(
                    "member {} declared output {} exceeds {}",
                    entry.index, entry.declared_decompressed_size, budget.max_member_bytes
                )));
            }

            let (stored_size, expanded_size) = if entry.declared_decompressed_size == 0 {
                ByteRange::new(entry.file_offset, entry.compressed_size, hpb_reader.len())?;
                (entry.compressed_size, entry.compressed_size)
            } else {
                let block = inspect_compressed_block(hpb, entry.file_offset, budget)?;
                (block.total_size, block.decompressed_size)
            };

            members.push(ArchiveMember {
                index: entry.index,
                name: Some(entry.filename),
                offset: entry.file_offset,
                stored_size,
                expanded_size: Some(expanded_size),
            });
        }

        enforce_inventory_budget(&members, budget)?;
        Ok(ArchiveInventory {
            kind: ArchiveKind::HpiHpb,
            members,
        })
    }

    pub fn read_member(
        &self,
        hpi: &[u8],
        hpb: &[u8],
        member: &ArchiveMember,
        budget: ExtractionBudget,
    ) -> Result<Vec<u8>, ArchiveError> {
        let inventory = self.inspect(hpi, hpb, budget)?;
        let canonical = inventory
            .members
            .iter()
            .find(|candidate| candidate.index == member.index)
            .ok_or(ArchiveError::MissingMember(member.index))?;
        if canonical != member {
            return Err(ArchiveError::MissingMember(member.index));
        }

        let entries = parse_hpi(hpi, budget)?;
        let entry = entries
            .iter()
            .find(|entry| entry.index == member.index)
            .ok_or(ArchiveError::MissingMember(member.index))?;
        if entry.declared_decompressed_size == 0 {
            return Ok(ByteReader::new(hpb)
                .bytes(canonical.offset, canonical.stored_size)?
                .to_vec());
        }

        let block = inspect_compressed_block(hpb, canonical.offset, budget)?;
        let bytes = ByteReader::new(hpb).bytes(canonical.offset, block.total_size)?;
        decompress_reverse_lz(bytes, budget)
    }
}

#[derive(Clone, Copy, Debug)]
struct CompressedBlockLayout {
    total_size: u64,
    decompressed_size: u64,
}

fn parse_hpi(hpi: &[u8], budget: ExtractionBudget) -> Result<Vec<HpiEntry>, ArchiveError> {
    let reader = ByteReader::new(hpi);
    if reader.len() < HPI_HEADER_SIZE || reader.bytes(0, 4)? != b"HPIH" {
        return Err(ArchiveError::InvalidHeader);
    }

    let unknown_count = u64::from(reader.u16_le(0x12)?);
    let file_count = u64::from(reader.u16_le(0x14)?);
    if file_count > budget.max_members {
        return Err(ArchiveError::BudgetExceeded(format!(
            "member count {file_count} exceeds {}",
            budget.max_members
        )));
    }

    let unknown_bytes = unknown_count
        .checked_mul(4)
        .ok_or(ArchiveError::InvalidOffset)?;
    let file_table = HPI_HEADER_SIZE
        .checked_add(unknown_bytes)
        .ok_or(ArchiveError::InvalidOffset)?;
    let table_size = file_count
        .checked_mul(HPI_ENTRY_SIZE)
        .ok_or(ArchiveError::InvalidOffset)?;
    let names_base = file_table
        .checked_add(table_size)
        .ok_or(ArchiveError::InvalidOffset)?;
    ByteRange::new(file_table, table_size, reader.len())?;
    if names_base > reader.len() {
        return Err(ArchiveError::InvalidOffset);
    }
    let names = reader.bytes(names_base, reader.len() - names_base)?;

    let mut entries = Vec::with_capacity(
        usize::try_from(file_count).map_err(|_| ArchiveError::InvalidOffset)?,
    );
    for index in 0..file_count {
        let offset = file_table
            .checked_add(
                index
                    .checked_mul(HPI_ENTRY_SIZE)
                    .ok_or(ArchiveError::InvalidOffset)?,
            )
            .ok_or(ArchiveError::InvalidOffset)?;
        let filename_offset = u64::from(reader.u32_le(offset)?);
        let filename = if filename_offset >= names.len() as u64 {
            format!("unnamed_{index:05}.bin")
        } else {
            decode_hpi_name(names, filename_offset)?
        };
        entries.push(HpiEntry {
            index,
            filename,
            file_offset: u64::from(reader.u32_le(offset + 4)?),
            compressed_size: u64::from(reader.u32_le(offset + 8)?),
            declared_decompressed_size: u64::from(reader.u32_le(offset + 12)?),
        });
    }
    Ok(entries)
}

fn decode_hpi_name(names: &[u8], offset: u64) -> Result<String, ArchiveError> {
    let start = usize::try_from(offset).map_err(|_| ArchiveError::InvalidOffset)?;
    let tail = names.get(start..).ok_or(ArchiveError::InvalidOffset)?;
    let end = tail.iter().position(|byte| *byte == 0).unwrap_or(tail.len());
    let (decoded, _) = SHIFT_JIS.decode_without_bom_handling(&tail[..end]);
    Ok(decoded.into_owned())
}

fn inspect_compressed_block(
    hpb: &[u8],
    offset: u64,
    budget: ExtractionBudget,
) -> Result<CompressedBlockLayout, ArchiveError> {
    let reader = ByteReader::new(hpb);
    ByteRange::new(offset, ACMP_MIN_HEADER_SIZE, reader.len())?;
    let compressed_size = u64::from(reader.u32_le(offset + 0x04)?);
    let header_size = u64::from(reader.u32_le(offset + 0x08)?);
    let decompressed_size = u64::from(reader.u32_le(offset + 0x10)?);
    if header_size < ACMP_MIN_HEADER_SIZE {
        return Err(ArchiveError::InvalidHeader);
    }
    if decompressed_size > budget.max_member_bytes {
        return Err(ArchiveError::BudgetExceeded(format!(
            "decompressed member size {decompressed_size} exceeds {}",
            budget.max_member_bytes
        )));
    }
    let total_size = header_size
        .checked_add(compressed_size)
        .ok_or(ArchiveError::InvalidOffset)?;
    ByteRange::new(offset, total_size, reader.len())?;
    if compressed_size < 8 {
        return Err(ArchiveError::TruncatedMember);
    }
    Ok(CompressedBlockLayout {
        total_size,
        decompressed_size,
    })
}

fn decompress_reverse_lz(
    block: &[u8],
    budget: ExtractionBudget,
) -> Result<Vec<u8>, ArchiveError> {
    let reader = ByteReader::new(block);
    if reader.len() < ACMP_MIN_HEADER_SIZE {
        return Err(ArchiveError::TruncatedMember);
    }
    let compressed_size = u64::from(reader.u32_le(0x04)?);
    let header_size = u64::from(reader.u32_le(0x08)?);
    let decompressed_size = u64::from(reader.u32_le(0x10)?);
    if header_size < ACMP_MIN_HEADER_SIZE {
        return Err(ArchiveError::InvalidHeader);
    }
    if decompressed_size > budget.max_member_bytes {
        return Err(ArchiveError::BudgetExceeded(format!(
            "decompressed member size {decompressed_size} exceeds {}",
            budget.max_member_bytes
        )));
    }
    let total_size = header_size
        .checked_add(compressed_size)
        .ok_or(ArchiveError::InvalidOffset)?;
    ByteRange::new(0, total_size, reader.len())?;
    let compressed = reader.bytes(header_size, compressed_size)?;
    if compressed.len() < 8 {
        return Err(ArchiveError::TruncatedMember);
    }

    let trailer_offset = compressed.len() - 8;
    let packed = u32::from_le_bytes(
        compressed[trailer_offset..trailer_offset + 4]
            .try_into()
            .map_err(|_| ArchiveError::TruncatedMember)?,
    );
    let decompressed_increase = u64::from(u32::from_le_bytes(
        compressed[trailer_offset + 4..trailer_offset + 8]
            .try_into()
            .map_err(|_| ArchiveError::TruncatedMember)?,
    ));
    let trailer_size = usize::from((packed >> 24) as u8);
    let trailer_compressed_size = u64::from(packed & 0x00ff_ffff);
    if trailer_size == 0 || trailer_size > compressed.len() {
        return Err(ArchiveError::InvalidHeader);
    }
    let target = trailer_compressed_size
        .checked_add(decompressed_increase)
        .ok_or(ArchiveError::InvalidOffset)?;
    if target > decompressed_size {
        return Err(ArchiveError::InvalidHeader);
    }

    let output_len = usize::try_from(decompressed_size)
        .map_err(|_| ArchiveError::BudgetExceeded("member output does not fit memory".to_owned()))?;
    let target = usize::try_from(target).map_err(|_| ArchiveError::InvalidOffset)?;
    let mut output = vec![0xaa; output_len];
    let mut history = [0u8; REVERSE_LZ_HISTORY_SIZE];
    let mut history_index = 0usize;
    let mut written = 0usize;
    let mut input_offset = compressed.len() - trailer_size;
    let mut output_offset = output_len;

    fn read_back(compressed: &[u8], input_offset: &mut usize) -> Result<u8, ArchiveError> {
        if *input_offset == 0 {
            return Err(ArchiveError::TruncatedMember);
        }
        *input_offset -= 1;
        compressed
            .get(*input_offset)
            .copied()
            .ok_or(ArchiveError::TruncatedMember)
    }

    fn write_back(
        output: &mut [u8],
        history: &mut [u8; REVERSE_LZ_HISTORY_SIZE],
        output_offset: &mut usize,
        history_index: &mut usize,
        written: &mut usize,
        value: u8,
    ) -> Result<(), ArchiveError> {
        if *output_offset == 0 {
            return Err(ArchiveError::InvalidHeader);
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
                let distance = (usize::from(first & 0x0f) << 8) | usize::from(second);
                let distance = distance + 3;
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

    while written < output_len {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn uncompressed_pair_inspects_and_reads_exact_member() {
        let hpi = hpi_for_member(b"plain.bin", 4, 0);
        let hpb = vec![1, 2, 3, 4];
        let parser = HpiHpbParser;
        assert!(parser.probe_index(&hpi));
        let inventory = parser.inspect(&hpi, &hpb, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.kind, ArchiveKind::HpiHpb);
        assert_eq!(inventory.members[0].name.as_deref(), Some("plain.bin"));
        assert_eq!(inventory.members[0].expanded_size, Some(4));
        assert_eq!(
            parser
                .read_member(
                    &hpi,
                    &hpb,
                    &inventory.members[0],
                    ExtractionBudget::default()
                )
                .unwrap(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn compressed_literal_block_round_trips_through_reverse_lz() {
        let hpb = literal_acmp(&[1, 2, 3, 4]);
        let hpi = hpi_for_member(b"compressed.bin", hpb.len() as u32, 4);
        let parser = HpiHpbParser;
        let inventory = parser.inspect(&hpi, &hpb, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.members[0].stored_size, hpb.len() as u64);
        assert_eq!(inventory.members[0].expanded_size, Some(4));
        assert_eq!(
            parser
                .read_member(
                    &hpi,
                    &hpb,
                    &inventory.members[0],
                    ExtractionBudget::default()
                )
                .unwrap(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn decompressed_size_is_budget_checked_before_output_allocation() {
        let hpb = literal_acmp(&[1, 2, 3, 4]);
        let hpi = hpi_for_member(b"compressed.bin", hpb.len() as u32, 4);
        let budget = ExtractionBudget {
            max_member_bytes: 3,
            ..ExtractionBudget::default()
        };
        assert!(matches!(
            HpiHpbParser.inspect(&hpi, &hpb, budget),
            Err(ArchiveError::BudgetExceeded(_))
        ));
    }

    #[test]
    fn truncated_compressed_block_is_rejected_during_inspection() {
        let mut hpb = literal_acmp(&[1, 2, 3, 4]);
        let hpi = hpi_for_member(b"compressed.bin", hpb.len() as u32, 4);
        hpb.pop();
        assert_eq!(
            HpiHpbParser.inspect(&hpi, &hpb, ExtractionBudget::default()),
            Err(ArchiveError::InvalidOffset)
        );
    }

    #[test]
    fn shift_jis_names_decode_without_filesystem_policy() {
        let (encoded, _, _) = SHIFT_JIS.encode("テスト.bin");
        let hpi = hpi_for_member(&encoded, 1, 0);
        let hpb = vec![0];
        let inventory = HpiHpbParser
            .inspect(&hpi, &hpb, ExtractionBudget::default())
            .unwrap();
        assert_eq!(inventory.members[0].name.as_deref(), Some("テスト.bin"));
    }
}
