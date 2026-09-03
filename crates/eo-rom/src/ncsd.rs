use crate::bytes::{ByteRange, ByteReader};
use crate::RomError;
use serde::{Deserialize, Serialize};

pub const NCSD_MEDIA_UNIT_SIZE: u64 = 0x200;
const NCSD_HEADER_MIN_SIZE: u64 = 0x160;
const PARTITION_TABLE_OFFSET: u64 = 0x120;
const PARTITION_COUNT: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NcsdPartition {
    pub index: u8,
    pub fs_type: u8,
    pub crypto_type: u8,
    pub offset_units: u32,
    pub size_units: u32,
    pub offset: u64,
    pub size: u64,
}

impl NcsdPartition {
    pub fn range(&self) -> ByteRange {
        ByteRange {
            offset: self.offset,
            size: self.size,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NcsdHeader {
    pub declared_size_units: u32,
    pub declared_size: u64,
    pub media_id: u64,
    pub partitions: Vec<NcsdPartition>,
}

impl NcsdHeader {
    pub fn parse(data: &[u8]) -> Result<Self, RomError> {
        let reader = ByteReader::new(data);
        if reader.len() < NCSD_HEADER_MIN_SIZE || reader.bytes(0x100, 4)? != b"NCSD" {
            return Err(RomError::InvalidHeader);
        }

        let declared_size_units = reader.u32_le(0x104)?;
        let declared_size = u64::from(declared_size_units)
            .checked_mul(NCSD_MEDIA_UNIT_SIZE)
            .ok_or(RomError::InvalidOffset)?;
        let media_id = reader.u64_le(0x108)?;
        let fs_types = reader.array::<8>(0x110)?;
        let crypto_types = reader.array::<8>(0x118)?;

        let mut partitions = Vec::new();
        for index in 0..PARTITION_COUNT {
            let entry = PARTITION_TABLE_OFFSET + (index as u64 * 8);
            let offset_units = reader.u32_le(entry)?;
            let size_units = reader.u32_le(entry + 4)?;
            if size_units == 0 {
                continue;
            }

            let range = ByteRange::from_units(
                offset_units,
                size_units,
                NCSD_MEDIA_UNIT_SIZE,
                reader.len(),
            )?;
            partitions.push(NcsdPartition {
                index: index as u8,
                fs_type: fs_types[index],
                crypto_type: crypto_types[index],
                offset_units,
                size_units,
                offset: range.offset,
                size: range.size,
            });
        }

        Ok(Self {
            declared_size_units,
            declared_size,
            media_id,
            partitions,
        })
    }

    pub fn partition(&self, index: u8) -> Option<&NcsdPartition> {
        self.partitions.iter().find(|part| part.index == index)
    }
}

pub struct NcsdImage<'a> {
    data: &'a [u8],
    pub header: NcsdHeader,
}

impl<'a> NcsdImage<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, RomError> {
        Ok(Self {
            data,
            header: NcsdHeader::parse(data)?,
        })
    }

    pub fn partition_bytes(&self, index: u8) -> Result<&'a [u8], RomError> {
        let partition = self
            .header
            .partition(index)
            .ok_or_else(|| RomError::MissingEntry(format!("NCSD partition {index}")))?;
        ByteReader::new(self.data).slice(partition.range())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut data = vec![0u8; 0x1000];
        data[0x100..0x104].copy_from_slice(b"NCSD");
        data[0x104..0x108].copy_from_slice(&8u32.to_le_bytes());
        data[0x108..0x110].copy_from_slice(&0x1122334455667788u64.to_le_bytes());
        data[0x110] = 1;
        data[0x118] = 2;
        data[0x120..0x124].copy_from_slice(&2u32.to_le_bytes());
        data[0x124..0x128].copy_from_slice(&3u32.to_le_bytes());
        data[0x400..0x404].copy_from_slice(b"NCCH");
        data
    }

    #[test]
    fn parses_partition_table_in_fixed_0x200_media_units() {
        let data = fixture();
        let image = NcsdImage::parse(&data).unwrap();
        assert_eq!(image.header.declared_size, 0x1000);
        assert_eq!(image.header.media_id, 0x1122334455667788);
        let part = image.header.partition(0).unwrap();
        assert_eq!(part.offset, 0x400);
        assert_eq!(part.size, 0x600);
        assert_eq!(part.fs_type, 1);
        assert_eq!(part.crypto_type, 2);
        assert_eq!(&image.partition_bytes(0).unwrap()[..4], b"NCCH");
    }

    #[test]
    fn trimmed_images_may_be_smaller_than_declared_media_size() {
        let mut data = fixture();
        data[0x104..0x108].copy_from_slice(&0x100u32.to_le_bytes());
        let header = NcsdHeader::parse(&data).unwrap();
        assert_eq!(header.declared_size, 0x20000);
    }

    #[test]
    fn rejects_partition_that_runs_past_actual_eof() {
        let mut data = fixture();
        data[0x124..0x128].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(NcsdHeader::parse(&data), Err(RomError::InvalidOffset));
    }
}
