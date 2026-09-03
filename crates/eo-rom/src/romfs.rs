use crate::bytes::{ByteRange, ByteReader};
use crate::RomError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const IVFC_HEADER_SIZE: u64 = 0x60;
const ROMFS_INFO_HEADER_SIZE: u32 = 0x28;
const IVFC_ROMFS_ID: u32 = 0x0001_0000;
const NONE: u32 = 0xFFFF_FFFF;
const MAX_METADATA_NODES: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RomFsLayout {
    pub master_hash_size: u32,
    pub level3_offset: u64,
    pub level3_size: u64,
    pub level3_block_size: u64,
    pub directory_hash_offset: u32,
    pub directory_hash_size: u32,
    pub directory_metadata_offset: u32,
    pub directory_metadata_size: u32,
    pub file_hash_offset: u32,
    pub file_hash_size: u32,
    pub file_metadata_offset: u32,
    pub file_metadata_size: u32,
    pub file_data_offset: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RomFsEntry {
    pub virtual_path: String,
    pub size: u64,
    pub data_offset: u64,
}

#[derive(Clone, Debug)]
struct DirectoryNode {
    sibling: u32,
    child: u32,
    first_file: u32,
    name: String,
}

#[derive(Clone, Debug)]
struct FileNode {
    sibling: u32,
    data_offset: u64,
    size: u64,
    name: String,
}

pub struct RomFsImage<'a> {
    data: &'a [u8],
    pub layout: RomFsLayout,
}

impl<'a> RomFsImage<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, RomError> {
        let reader = ByteReader::new(data);
        if reader.len() < IVFC_HEADER_SIZE || reader.bytes(0, 4)? != b"IVFC" {
            return Err(RomError::InvalidHeader);
        }
        if reader.u32_le(4)? != IVFC_ROMFS_ID {
            return Err(RomError::Malformed(
                "IVFC container is not a Nintendo 3DS RomFS".to_owned(),
            ));
        }

        let master_hash_size = reader.u32_le(0x08)?;
        let level3_size = reader.u64_le(0x44)?;
        let level3_block_log2 = reader.u32_le(0x4C)?;
        let level3_block_size = 1u64
            .checked_shl(level3_block_log2)
            .ok_or_else(|| RomError::Malformed("invalid RomFS Level-3 block size".to_owned()))?;
        if level3_block_size == 0 || level3_block_size > (1u64 << 30) {
            return Err(RomError::Malformed(
                "implausible RomFS Level-3 block size".to_owned(),
            ));
        }

        let hashes_end = IVFC_HEADER_SIZE
            .checked_add(u64::from(master_hash_size))
            .ok_or(RomError::InvalidOffset)?;
        let level3_offset = align_up(hashes_end, level3_block_size)?;
        let level3_range = ByteRange::new(level3_offset, level3_size, reader.len())?;
        let level3 = reader.slice(level3_range)?;
        let level3_reader = ByteReader::new(level3);
        if level3_reader.u32_le(0)? != ROMFS_INFO_HEADER_SIZE {
            return Err(RomError::Malformed(
                "invalid RomFS Level-3 info-header length".to_owned(),
            ));
        }

        let layout = RomFsLayout {
            master_hash_size,
            level3_offset,
            level3_size,
            level3_block_size,
            directory_hash_offset: level3_reader.u32_le(0x04)?,
            directory_hash_size: level3_reader.u32_le(0x08)?,
            directory_metadata_offset: level3_reader.u32_le(0x0C)?,
            directory_metadata_size: level3_reader.u32_le(0x10)?,
            file_hash_offset: level3_reader.u32_le(0x14)?,
            file_hash_size: level3_reader.u32_le(0x18)?,
            file_metadata_offset: level3_reader.u32_le(0x1C)?,
            file_metadata_size: level3_reader.u32_le(0x20)?,
            file_data_offset: level3_reader.u32_le(0x24)?,
        };
        validate_level3_layout(&layout, level3_reader.len())?;

        Ok(Self { data, layout })
    }

    pub fn entries(&self) -> Result<Vec<RomFsEntry>, RomError> {
        let mut entries = Vec::new();
        let mut seen_dirs = BTreeSet::new();
        let mut seen_files = BTreeSet::new();
        self.walk_directory(
            0,
            "",
            &mut entries,
            &mut seen_dirs,
            &mut seen_files,
        )?;
        Ok(entries)
    }

    pub fn read_entry(&self, entry: &RomFsEntry) -> Result<&'a [u8], RomError> {
        ByteReader::new(self.data).slice(ByteRange::new(
            entry.data_offset,
            entry.size,
            self.data.len() as u64,
        )?)
    }

    fn walk_directory(
        &self,
        offset: u32,
        parent_path: &str,
        entries: &mut Vec<RomFsEntry>,
        seen_dirs: &mut BTreeSet<u32>,
        seen_files: &mut BTreeSet<u32>,
    ) -> Result<(), RomError> {
        if seen_dirs.len() >= MAX_METADATA_NODES || !seen_dirs.insert(offset) {
            return Err(RomError::Malformed(
                "RomFS directory metadata cycle or node limit exceeded".to_owned(),
            ));
        }

        let dir = self.directory_node(offset)?;
        let current_path = if offset == 0 {
            String::new()
        } else {
            join_path(parent_path, &dir.name)
        };

        let mut file_offset = dir.first_file;
        while file_offset != NONE {
            if seen_files.len() >= MAX_METADATA_NODES || !seen_files.insert(file_offset) {
                return Err(RomError::Malformed(
                    "RomFS file metadata cycle or node limit exceeded".to_owned(),
                ));
            }
            let file = self.file_node(file_offset)?;
            let virtual_path = join_path(&current_path, &file.name);
            let absolute_data_offset = self
                .layout
                .level3_offset
                .checked_add(u64::from(self.layout.file_data_offset))
                .and_then(|base| base.checked_add(file.data_offset))
                .ok_or(RomError::InvalidOffset)?;
            let level3_end = self
                .layout
                .level3_offset
                .checked_add(self.layout.level3_size)
                .ok_or(RomError::InvalidOffset)?;
            let file_end = absolute_data_offset
                .checked_add(file.size)
                .ok_or(RomError::InvalidOffset)?;
            if absolute_data_offset < self.layout.level3_offset || file_end > level3_end {
                return Err(RomError::InvalidOffset);
            }
            entries.push(RomFsEntry {
                virtual_path,
                size: file.size,
                data_offset: absolute_data_offset,
            });
            file_offset = file.sibling;
        }

        let mut child_offset = dir.child;
        while child_offset != NONE {
            let child = self.directory_node(child_offset)?;
            self.walk_directory(
                child_offset,
                &current_path,
                entries,
                seen_dirs,
                seen_files,
            )?;
            child_offset = child.sibling;
        }
        Ok(())
    }

    fn directory_node(&self, offset: u32) -> Result<DirectoryNode, RomError> {
        let table = self.directory_metadata_table()?;
        let reader = ByteReader::new(table);
        let base = u64::from(offset);
        if base
            .checked_add(0x18)
            .ok_or(RomError::InvalidOffset)?
            > reader.len()
        {
            return Err(RomError::InvalidOffset);
        }
        let name_len = reader.u32_le(base + 0x14)?;
        let name = decode_component(reader.bytes(base + 0x18, u64::from(name_len))?)?;
        Ok(DirectoryNode {
            sibling: reader.u32_le(base + 0x04)?,
            child: reader.u32_le(base + 0x08)?,
            first_file: reader.u32_le(base + 0x0C)?,
            name,
        })
    }

    fn file_node(&self, offset: u32) -> Result<FileNode, RomError> {
        let table = self.file_metadata_table()?;
        let reader = ByteReader::new(table);
        let base = u64::from(offset);
        if base
            .checked_add(0x20)
            .ok_or(RomError::InvalidOffset)?
            > reader.len()
        {
            return Err(RomError::InvalidOffset);
        }
        let name_len = reader.u32_le(base + 0x1C)?;
        let name = decode_component(reader.bytes(base + 0x20, u64::from(name_len))?)?;
        Ok(FileNode {
            sibling: reader.u32_le(base + 0x04)?,
            data_offset: reader.u64_le(base + 0x08)?,
            size: reader.u64_le(base + 0x10)?,
            name,
        })
    }

    fn level3(&self) -> Result<&'a [u8], RomError> {
        ByteReader::new(self.data).slice(ByteRange::new(
            self.layout.level3_offset,
            self.layout.level3_size,
            self.data.len() as u64,
        )?)
    }

    fn directory_metadata_table(&self) -> Result<&'a [u8], RomError> {
        let level3 = ByteReader::new(self.level3()?);
        level3.bytes(
            u64::from(self.layout.directory_metadata_offset),
            u64::from(self.layout.directory_metadata_size),
        )
    }

    fn file_metadata_table(&self) -> Result<&'a [u8], RomError> {
        let level3 = ByteReader::new(self.level3()?);
        level3.bytes(
            u64::from(self.layout.file_metadata_offset),
            u64::from(self.layout.file_metadata_size),
        )
    }
}

fn validate_level3_layout(layout: &RomFsLayout, level3_len: u64) -> Result<(), RomError> {
    let ranges = [
        (
            layout.directory_hash_offset,
            layout.directory_hash_size,
            "directory hash table",
        ),
        (
            layout.directory_metadata_offset,
            layout.directory_metadata_size,
            "directory metadata table",
        ),
        (
            layout.file_hash_offset,
            layout.file_hash_size,
            "file hash table",
        ),
        (
            layout.file_metadata_offset,
            layout.file_metadata_size,
            "file metadata table",
        ),
    ];
    for (offset, size, label) in ranges {
        ByteRange::new(u64::from(offset), u64::from(size), level3_len).map_err(|_| {
            RomError::Malformed(format!("RomFS {label} is outside Level 3"))
        })?;
    }
    if u64::from(layout.file_data_offset) > level3_len {
        return Err(RomError::Malformed(
            "RomFS file data begins outside Level 3".to_owned(),
        ));
    }
    Ok(())
}

fn align_up(value: u64, alignment: u64) -> Result<u64, RomError> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(RomError::InvalidOffset);
    }
    let mask = alignment - 1;
    value
        .checked_add(mask)
        .map(|sum| sum & !mask)
        .ok_or(RomError::InvalidOffset)
}

fn decode_component(bytes: &[u8]) -> Result<String, RomError> {
    if bytes.len() % 2 != 0 {
        return Err(RomError::Malformed(
            "RomFS UTF-16 name has odd byte length".to_owned(),
        ));
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let name = String::from_utf16(&units)
        .map_err(|_| RomError::Malformed("RomFS name is invalid UTF-16".to_owned()))?;
    if name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(RomError::UnsafePath(name));
    }
    Ok(name)
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}/{child}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_utf16(data: &mut [u8], offset: usize, value: &str) -> u32 {
        let encoded = value.encode_utf16().collect::<Vec<_>>();
        for (index, unit) in encoded.iter().enumerate() {
            let start = offset + index * 2;
            data[start..start + 2].copy_from_slice(&unit.to_le_bytes());
        }
        (encoded.len() * 2) as u32
    }

    fn fixture() -> Vec<u8> {
        let mut data = vec![0u8; 0x3000];
        data[0..4].copy_from_slice(b"IVFC");
        put_u32(&mut data, 4, IVFC_ROMFS_ID);
        put_u32(&mut data, 0x08, 0x20);
        put_u64(&mut data, 0x44, 0x1000);
        put_u32(&mut data, 0x4C, 12);

        let l3 = 0x1000usize;
        put_u32(&mut data, l3, 0x28);
        put_u32(&mut data, l3 + 0x04, 0x28);
        put_u32(&mut data, l3 + 0x08, 0x0C);
        put_u32(&mut data, l3 + 0x0C, 0x34);
        put_u32(&mut data, l3 + 0x10, 0x50);
        put_u32(&mut data, l3 + 0x14, 0x84);
        put_u32(&mut data, l3 + 0x18, 0x0C);
        put_u32(&mut data, l3 + 0x1C, 0x90);
        put_u32(&mut data, l3 + 0x20, 0x70);
        put_u32(&mut data, l3 + 0x24, 0x100);

        let dirs = l3 + 0x34;
        put_u32(&mut data, dirs, 0);
        put_u32(&mut data, dirs + 0x04, NONE);
        put_u32(&mut data, dirs + 0x08, 0x18);
        put_u32(&mut data, dirs + 0x0C, NONE);
        put_u32(&mut data, dirs + 0x10, NONE);
        put_u32(&mut data, dirs + 0x14, 0);

        let child = dirs + 0x18;
        put_u32(&mut data, child, 0);
        put_u32(&mut data, child + 0x04, NONE);
        put_u32(&mut data, child + 0x08, NONE);
        put_u32(&mut data, child + 0x0C, 0);
        put_u32(&mut data, child + 0x10, NONE);
        let dir_name_len = put_utf16(&mut data, child + 0x18, "data");
        put_u32(&mut data, child + 0x14, dir_name_len);

        let files = l3 + 0x90;
        put_u32(&mut data, files, 0x18);
        put_u32(&mut data, files + 0x04, NONE);
        put_u64(&mut data, files + 0x08, 0x20);
        put_u64(&mut data, files + 0x10, 4);
        put_u32(&mut data, files + 0x18, NONE);
        let file_name_len = put_utf16(&mut data, files + 0x20, "test.bin");
        put_u32(&mut data, files + 0x1C, file_name_len);

        data[l3 + 0x120..l3 + 0x124].copy_from_slice(b"EO3D");
        data
    }

    #[test]
    fn parses_level3_tree_and_reads_file_data() {
        let data = fixture();
        let romfs = RomFsImage::parse(&data).unwrap();
        assert_eq!(romfs.layout.level3_offset, 0x1000);
        let entries = romfs.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].virtual_path, "data/test.bin");
        assert_eq!(entries[0].size, 4);
        assert_eq!(romfs.read_entry(&entries[0]).unwrap(), b"EO3D");
    }

    #[test]
    fn rejects_bad_level3_extent() {
        let mut data = fixture();
        put_u64(&mut data, 0x44, 0x4000);
        assert!(matches!(RomFsImage::parse(&data), Err(RomError::InvalidOffset)));
    }

    #[test]
    fn rejects_metadata_cycles() {
        let mut data = fixture();
        let files = 0x1000 + 0x90;
        put_u32(&mut data, files + 0x04, 0);
        let romfs = RomFsImage::parse(&data).unwrap();
        assert!(matches!(romfs.entries(), Err(RomError::Malformed(_))));
    }

    #[test]
    fn rejects_path_separators_in_components() {
        assert!(matches!(
            decode_component(&[b'.', 0, b'.', 0]),
            Err(RomError::UnsafePath(_))
        ));
    }
}
