use crate::bytes::{ByteRange, ByteReader};
use crate::{enforce_archive_budget, enforce_inventory_budget, ArchiveError, ArchiveInventory, ArchiveKind, ArchiveMember, ArchiveParser, ExtractionBudget};

const FARC_HEADER_SIZE: u64 = 0x34;
const SIR0_HEADER_SIZE: u64 = 12;
const FARC_ENTRY_SIZE: u64 = 12;
const MAX_UTF16_NAME_BYTES: u64 = 0x1000;

#[derive(Clone, Copy, Debug, Default)]
pub struct FarcParser;

impl ArchiveParser for FarcParser {
    fn kind(&self) -> ArchiveKind {
        ArchiveKind::Farc
    }

    fn probe(&self, data: &[u8]) -> bool {
        data.len() >= FARC_HEADER_SIZE as usize && data.get(..4) == Some(b"FARC")
    }

    fn inspect(
        &self,
        data: &[u8],
        budget: ExtractionBudget,
    ) -> Result<ArchiveInventory, ArchiveError> {
        enforce_archive_budget(data.len() as u64, budget)?;
        if !self.probe(data) {
            return Err(ArchiveError::InvalidHeader);
        }

        let reader = ByteReader::new(data);
        let fat_type = reader.u32_le(0x20)?;
        if !matches!(fat_type, 4 | 5) {
            return Err(ArchiveError::UnsupportedRevision(format!(
                "FARC SIR0 type {fat_type}; expected 4 or 5"
            )));
        }

        let sir0_offset = u64::from(reader.u32_le(0x24)?);
        let sir0_length = u64::from(reader.u32_le(0x28)?);
        let all_data_offset = u64::from(reader.u32_le(0x2c)?);
        if sir0_offset == 0 || sir0_length < SIR0_HEADER_SIZE {
            return Err(ArchiveError::InvalidHeader);
        }
        let sir0_range = ByteRange::new(sir0_offset, sir0_length, reader.len())?;
        if all_data_offset > reader.len() {
            return Err(ArchiveError::InvalidOffset);
        }
        let sir0 = ByteReader::new(reader.bytes(sir0_range.offset, sir0_range.size)?);
        if sir0.bytes(0, 4)? != b"SIR0" {
            return Err(ArchiveError::InvalidHeader);
        }

        let header_offset = u64::from(sir0.u32_le(4)?);
        let pointer_offset = u64::from(sir0.u32_le(8)?);
        ByteRange::new(header_offset, 12, sir0.len())?;
        if pointer_offset < header_offset || pointer_offset > sir0.len() {
            return Err(ArchiveError::InvalidHeader);
        }

        let entry_table_offset = u64::from(sir0.u32_le(header_offset)?);
        let file_count = u64::from(sir0.u32_le(header_offset + 4)?);
        let filename_mode = sir0.u32_le(header_offset + 8)?;
        if !matches!(filename_mode, 0 | 1) {
            return Err(ArchiveError::UnsupportedRevision(format!(
                "FARC filename mode {filename_mode}"
            )));
        }
        if file_count > budget.max_members {
            return Err(ArchiveError::BudgetExceeded(format!(
                "member count {file_count} exceeds {}",
                budget.max_members
            )));
        }
        let table_size = file_count
            .checked_mul(FARC_ENTRY_SIZE)
            .ok_or(ArchiveError::InvalidOffset)?;
        ByteRange::new(entry_table_offset, table_size, sir0.len())?;

        let mut members = Vec::with_capacity(
            usize::try_from(file_count).map_err(|_| ArchiveError::InvalidOffset)?,
        );
        for index in 0..file_count {
            let entry_offset = entry_table_offset
                .checked_add(
                    index
                        .checked_mul(FARC_ENTRY_SIZE)
                        .ok_or(ArchiveError::InvalidOffset)?,
                )
                .ok_or(ArchiveError::InvalidOffset)?;
            let name_or_hash = sir0.u32_le(entry_offset)?;
            let relative_data_offset = u64::from(sir0.u32_le(entry_offset + 4)?);
            let size = u64::from(sir0.u32_le(entry_offset + 8)?);
            let absolute_offset = all_data_offset
                .checked_add(relative_data_offset)
                .ok_or(ArchiveError::InvalidOffset)?;
            ByteRange::new(absolute_offset, size, reader.len())?;

            let name = if filename_mode == 0 {
                Some(read_utf16z(
                    sir0.bytes(0, sir0.len())?,
                    u64::from(name_or_hash),
                )?)
            } else {
                None
            };
            members.push(ArchiveMember {
                index,
                name,
                offset: absolute_offset,
                stored_size: size,
                expanded_size: Some(size),
            });
        }

        enforce_inventory_budget(&members, budget)?;
        Ok(ArchiveInventory {
            kind: ArchiveKind::Farc,
            members,
        })
    }

    fn read_member(
        &self,
        data: &[u8],
        member: &ArchiveMember,
        budget: ExtractionBudget,
    ) -> Result<Vec<u8>, ArchiveError> {
        let inventory = self.inspect(data, budget)?;
        let canonical = inventory
            .members
            .iter()
            .find(|candidate| candidate.index == member.index)
            .ok_or(ArchiveError::MissingMember(member.index))?;
        if canonical != member {
            return Err(ArchiveError::MissingMember(member.index));
        }
        let reader = ByteReader::new(data);
        Ok(reader
            .bytes(canonical.offset, canonical.stored_size)?
            .to_vec())
    }
}

fn read_utf16z(data: &[u8], offset: u64) -> Result<String, ArchiveError> {
    let reader = ByteReader::new(data);
    if offset >= reader.len() {
        return Err(ArchiveError::InvalidOffset);
    }
    let limit = reader
        .len()
        .min(offset.saturating_add(MAX_UTF16_NAME_BYTES));
    let mut units = Vec::new();
    let mut position = offset;
    while position.checked_add(2).is_some_and(|end| end <= limit) {
        let pair = reader.bytes(position, 2)?;
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if unit == 0 {
            return String::from_utf16(&units)
                .map_err(|_| ArchiveError::InvalidName("invalid FARC UTF-16 name".to_owned()));
        }
        units.push(unit);
        position = position.checked_add(2).ok_or(ArchiveError::InvalidOffset)?;
    }
    Err(ArchiveError::InvalidName(
        "unterminated FARC UTF-16 name".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_farc(filename_mode: u32) -> Vec<u8> {
        let mut data = vec![0u8; 0x100];
        data[0..4].copy_from_slice(b"FARC");
        data[0x20..0x24].copy_from_slice(&4u32.to_le_bytes());
        data[0x24..0x28].copy_from_slice(&0x40u32.to_le_bytes());
        data[0x28..0x2c].copy_from_slice(&0x80u32.to_le_bytes());
        data[0x2c..0x30].copy_from_slice(&0xc0u32.to_le_bytes());
        data[0x30..0x34].copy_from_slice(&4u32.to_le_bytes());

        let sir0 = 0x40usize;
        data[sir0..sir0 + 4].copy_from_slice(b"SIR0");
        data[sir0 + 4..sir0 + 8].copy_from_slice(&0x10u32.to_le_bytes());
        data[sir0 + 8..sir0 + 12].copy_from_slice(&0x70u32.to_le_bytes());
        data[sir0 + 0x10..sir0 + 0x14].copy_from_slice(&0x20u32.to_le_bytes());
        data[sir0 + 0x14..sir0 + 0x18].copy_from_slice(&1u32.to_le_bytes());
        data[sir0 + 0x18..sir0 + 0x1c].copy_from_slice(&filename_mode.to_le_bytes());
        let name_or_hash = if filename_mode == 0 { 0x40u32 } else { 0x1234_5678 };
        data[sir0 + 0x20..sir0 + 0x24].copy_from_slice(&name_or_hash.to_le_bytes());
        data[sir0 + 0x24..sir0 + 0x28].copy_from_slice(&0u32.to_le_bytes());
        data[sir0 + 0x28..sir0 + 0x2c].copy_from_slice(&4u32.to_le_bytes());
        if filename_mode == 0 {
            for (i, unit) in "a.bin\0".encode_utf16().enumerate() {
                let offset = sir0 + 0x40 + i * 2;
                data[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
            }
        }
        data[0xc0..0xc4].copy_from_slice(&[1, 2, 3, 4]);
        data
    }

    #[test]
    fn parses_named_farc_and_reads_exact_member() {
        let data = synthetic_farc(0);
        let parser = FarcParser;
        let inventory = parser.inspect(&data, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.kind, ArchiveKind::Farc);
        assert_eq!(inventory.members.len(), 1);
        let member = &inventory.members[0];
        assert_eq!(member.name.as_deref(), Some("a.bin"));
        assert_eq!(member.offset, 0xc0);
        assert_eq!(parser.read_member(&data, member, ExtractionBudget::default()).unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn hashed_farc_names_remain_unknown_metadata() {
        let data = synthetic_farc(1);
        let inventory = FarcParser
            .inspect(&data, ExtractionBudget::default())
            .unwrap();
        assert_eq!(inventory.members[0].name, None);
    }

    #[test]
    fn rejects_member_extent_past_eof() {
        let mut data = synthetic_farc(0);
        let sir0 = 0x40usize;
        data[sir0 + 0x28..sir0 + 0x2c].copy_from_slice(&0x80u32.to_le_bytes());
        assert_eq!(
            FarcParser.inspect(&data, ExtractionBudget::default()),
            Err(ArchiveError::InvalidOffset)
        );
    }
}
