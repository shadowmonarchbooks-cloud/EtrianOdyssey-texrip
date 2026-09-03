use crate::bytes::{ByteRange, ByteReader};
use crate::RomError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const EXEFS_HEADER_SIZE: u64 = 0x200;
const EXEFS_FILE_COUNT: usize = 10;
const EXEFS_FILE_HEADER_SIZE: u64 = 0x10;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExeFsEntry {
    pub name: String,
    pub offset: u64,
    pub size: u64,
}

impl ExeFsEntry {
    pub fn range(&self) -> ByteRange {
        ByteRange {
            offset: self.offset,
            size: self.size,
        }
    }
}

pub struct ExeFsImage<'a> {
    data: &'a [u8],
    entries: Vec<ExeFsEntry>,
}

impl<'a> ExeFsImage<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, RomError> {
        let reader = ByteReader::new(data);
        if reader.len() < EXEFS_HEADER_SIZE {
            return Err(RomError::InvalidHeader);
        }

        let mut entries = Vec::new();
        let mut names = BTreeSet::new();
        let mut ranges = Vec::new();
        for index in 0..EXEFS_FILE_COUNT {
            let base = index as u64 * EXEFS_FILE_HEADER_SIZE;
            let raw_name = reader.bytes(base, 8)?;
            if raw_name.iter().all(|byte| *byte == 0) {
                continue;
            }
            let name = decode_name(raw_name)?;
            if !names.insert(name.clone()) {
                return Err(RomError::Malformed(format!(
                    "duplicate ExeFS entry name: {name}"
                )));
            }

            let relative_offset = u64::from(reader.u32_le(base + 0x08)?);
            let size = u64::from(reader.u32_le(base + 0x0C)?);
            let offset = EXEFS_HEADER_SIZE
                .checked_add(relative_offset)
                .ok_or(RomError::InvalidOffset)?;
            let range = ByteRange::new(offset, size, reader.len())?;
            for previous in &ranges {
                if overlaps(range, *previous) {
                    return Err(RomError::Malformed(format!(
                        "overlapping ExeFS entry: {name}"
                    )));
                }
            }
            ranges.push(range);
            entries.push(ExeFsEntry { name, offset, size });
        }

        Ok(Self { data, entries })
    }

    pub fn entries(&self) -> &[ExeFsEntry] {
        &self.entries
    }

    pub fn read_entry(&self, name: &str) -> Result<&'a [u8], RomError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| RomError::MissingEntry(format!("ExeFS/{name}")))?;
        ByteReader::new(self.data).slice(entry.range())
    }
}

fn decode_name(bytes: &[u8]) -> Result<String, RomError> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    if end == 0 {
        return Err(RomError::Malformed("empty ExeFS entry name".to_owned()));
    }
    let name = std::str::from_utf8(&bytes[..end])
        .map_err(|_| RomError::Malformed("ExeFS entry name is not UTF-8/ASCII".to_owned()))?
        .to_owned();
    if name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
    {
        return Err(RomError::UnsafePath(name));
    }
    Ok(name)
}

fn overlaps(first: ByteRange, second: ByteRange) -> bool {
    let first_end = first.offset + first.size;
    let second_end = second.offset + second.size;
    first.offset < second_end && second.offset < first_end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut data = vec![0u8; 0x300];
        data[0..5].copy_from_slice(b".code");
        data[0x08..0x0C].copy_from_slice(&0u32.to_le_bytes());
        data[0x0C..0x10].copy_from_slice(&4u32.to_le_bytes());
        data[0x10..0x14].copy_from_slice(b"icon");
        data[0x18..0x1C].copy_from_slice(&0x20u32.to_le_bytes());
        data[0x1C..0x20].copy_from_slice(&4u32.to_le_bytes());
        data[0x200..0x204].copy_from_slice(b"CODE");
        data[0x220..0x224].copy_from_slice(b"ICON");
        data
    }

    #[test]
    fn parses_and_reads_exefs_entries() {
        let data = fixture();
        let exefs = ExeFsImage::parse(&data).unwrap();
        assert_eq!(exefs.entries().len(), 2);
        assert_eq!(exefs.entries()[0].name, ".code");
        assert_eq!(exefs.read_entry(".code").unwrap(), b"CODE");
        assert_eq!(exefs.read_entry("icon").unwrap(), b"ICON");
    }

    #[test]
    fn rejects_entry_past_eof() {
        let mut data = fixture();
        data[0x0C..0x10].copy_from_slice(&0x200u32.to_le_bytes());
        assert!(matches!(ExeFsImage::parse(&data), Err(RomError::InvalidOffset)));
    }

    #[test]
    fn rejects_unsafe_entry_name() {
        let mut data = fixture();
        data[0..8].fill(0);
        data[0..7].copy_from_slice(b"bad/one");
        assert!(matches!(ExeFsImage::parse(&data), Err(RomError::UnsafePath(_))));
    }
}
