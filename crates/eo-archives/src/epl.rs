use crate::bytes::{ByteRange, ByteReader};
use crate::{
    enforce_archive_budget, enforce_inventory_budget, ArchiveError, ArchiveInventory, ArchiveKind,
    ArchiveMember, ArchiveParser, ExtractionBudget,
};
use encoding_rs::SHIFT_JIS;

const EPL_HEADER_END: u64 = 0x8c;
const EPL_RECORD_SIZE: u64 = 0xc0;
const EPL_DESCRIPTOR_MIN_SIZE: u64 = 0x28;
const EPL_NAME_OFFSET: u64 = 0x9c;
const EPL_NAME_SIZE: u64 = 36;
const MAX_STRUCTURAL_MEMBERS: u64 = 10_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct EplParser;

impl ArchiveParser for EplParser {
    fn kind(&self) -> ArchiveKind {
        ArchiveKind::Epl
    }

    fn probe(&self, data: &[u8]) -> bool {
        let reader = ByteReader::new(data);
        if reader.len() < EPL_HEADER_END {
            return false;
        }
        let Ok(file_count) = reader.i32_le(0x80) else {
            return false;
        };
        let Ok(data_start) = reader.i32_le(0x88) else {
            return false;
        };
        if file_count <= 0
            || u64::try_from(file_count)
                .ok()
                .is_none_or(|count| count > MAX_STRUCTURAL_MEMBERS)
        {
            return false;
        }
        if data_start < EPL_HEADER_END as i32 {
            return false;
        }
        let Ok(data_start) = u64::try_from(data_start) else {
            return false;
        };
        let count = file_count as u64;
        count
            .checked_mul(EPL_RECORD_SIZE)
            .and_then(|size| data_start.checked_add(size))
            .is_some_and(|end| end <= reader.len())
    }

    fn inspect(
        &self,
        data: &[u8],
        budget: ExtractionBudget,
    ) -> Result<ArchiveInventory, ArchiveError> {
        enforce_archive_budget(data.len() as u64, budget)?;
        let reader = ByteReader::new(data);
        if reader.len() < EPL_HEADER_END {
            return Err(ArchiveError::InvalidHeader);
        }

        let file_count_i32 = reader.i32_le(0x80)?;
        let data_start_i32 = reader.i32_le(0x88)?;
        if file_count_i32 <= 0 {
            return Err(ArchiveError::InvalidHeader);
        }
        let file_count = u64::try_from(file_count_i32).map_err(|_| ArchiveError::InvalidHeader)?;
        if file_count > MAX_STRUCTURAL_MEMBERS {
            return Err(ArchiveError::InvalidHeader);
        }
        if file_count > budget.max_members {
            return Err(ArchiveError::BudgetExceeded(format!(
                "member count {file_count} exceeds {}",
                budget.max_members
            )));
        }
        if data_start_i32 < EPL_HEADER_END as i32 {
            return Err(ArchiveError::InvalidOffset);
        }
        let data_start = u64::try_from(data_start_i32).map_err(|_| ArchiveError::InvalidOffset)?;
        let table_size = file_count
            .checked_mul(EPL_RECORD_SIZE)
            .ok_or(ArchiveError::InvalidOffset)?;
        ByteRange::new(data_start, table_size, reader.len())?;

        let mut members = Vec::with_capacity(
            usize::try_from(file_count).map_err(|_| ArchiveError::InvalidOffset)?,
        );
        for index in 0..file_count {
            let record_offset = data_start
                .checked_add(
                    index
                        .checked_mul(EPL_RECORD_SIZE)
                        .ok_or(ArchiveError::InvalidOffset)?,
                )
                .ok_or(ArchiveError::InvalidOffset)?;
            let descriptor_i32 = reader.i32_le(record_offset + 0x90)?;
            if descriptor_i32 < 0 {
                return Err(ArchiveError::InvalidOffset);
            }
            let descriptor =
                u64::try_from(descriptor_i32).map_err(|_| ArchiveError::InvalidOffset)?;
            ByteRange::new(descriptor, EPL_DESCRIPTOR_MIN_SIZE, reader.len())?;

            let relative_i32 = reader.i32_le(descriptor + 0x20)?;
            let size_i32 = reader.i32_le(descriptor + 0x24)?;
            if relative_i32 < 0 || size_i32 < 0 {
                return Err(ArchiveError::InvalidOffset);
            }
            let relative = u64::try_from(relative_i32).map_err(|_| ArchiveError::InvalidOffset)?;
            let size = u64::try_from(size_i32).map_err(|_| ArchiveError::InvalidOffset)?;
            let payload_offset = descriptor
                .checked_add(relative)
                .ok_or(ArchiveError::InvalidOffset)?;
            ByteRange::new(payload_offset, size, reader.len())?;

            let raw_name = reader.bytes(record_offset + EPL_NAME_OFFSET, EPL_NAME_SIZE)?;
            let name = decode_cstring(raw_name);
            members.push(ArchiveMember {
                index,
                name,
                offset: payload_offset,
                stored_size: size,
                expanded_size: Some(size),
            });
        }

        enforce_inventory_budget(&members, budget)?;
        Ok(ArchiveInventory {
            kind: ArchiveKind::Epl,
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
        Ok(ByteReader::new(data)
            .bytes(canonical.offset, canonical.stored_size)?
            .to_vec())
    }
}

fn decode_cstring(raw: &[u8]) -> Option<String> {
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    if end == 0 {
        return None;
    }
    let (decoded, _) = SHIFT_JIS.decode_without_bom_handling(&raw[..end]);
    Some(decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_epl() -> Vec<u8> {
        let mut data = vec![0u8; 0x220];
        data[0x80..0x84].copy_from_slice(&1i32.to_le_bytes());
        data[0x84..0x88].copy_from_slice(&0i32.to_le_bytes());
        data[0x88..0x8c].copy_from_slice(&0x90i32.to_le_bytes());

        let record = 0x90usize;
        data[record + 0x90..record + 0x94].copy_from_slice(&0x180i32.to_le_bytes());
        data[record + 0x9c..record + 0xa4].copy_from_slice(b"fx.stex\0");

        let descriptor = 0x180usize;
        data[descriptor + 0x20..descriptor + 0x24].copy_from_slice(&0x30i32.to_le_bytes());
        data[descriptor + 0x24..descriptor + 0x28].copy_from_slice(&4i32.to_le_bytes());
        data[0x1b0..0x1b4].copy_from_slice(b"STEX");
        data
    }

    #[test]
    fn structurally_probes_and_reads_epl_member() {
        let data = synthetic_epl();
        let parser = EplParser;
        assert!(parser.probe(&data));
        let inventory = parser.inspect(&data, ExtractionBudget::default()).unwrap();
        assert_eq!(inventory.kind, ArchiveKind::Epl);
        assert_eq!(inventory.members.len(), 1);
        let member = &inventory.members[0];
        assert_eq!(member.name.as_deref(), Some("fx.stex"));
        assert_eq!(member.offset, 0x1b0);
        assert_eq!(
            parser
                .read_member(&data, member, ExtractionBudget::default())
                .unwrap(),
            b"STEX"
        );
    }

    #[test]
    fn epl_names_use_shift_jis_like_the_legacy_parser() {
        let (encoded, _, _) = SHIFT_JIS.encode("テスト.stex");
        assert_eq!(decode_cstring(&encoded).as_deref(), Some("テスト.stex"));
    }

    #[test]
    fn rejects_negative_member_size() {
        let mut data = synthetic_epl();
        data[0x1a4..0x1a8].copy_from_slice(&(-1i32).to_le_bytes());
        assert_eq!(
            EplParser.inspect(&data, ExtractionBudget::default()),
            Err(ArchiveError::InvalidOffset)
        );
    }

    #[test]
    fn member_count_respects_budget_before_allocation() {
        let data = synthetic_epl();
        let budget = ExtractionBudget {
            max_members: 0,
            ..ExtractionBudget::default()
        };
        assert!(matches!(
            EplParser.inspect(&data, budget),
            Err(ArchiveError::BudgetExceeded(_))
        ));
    }
}
