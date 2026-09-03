use crate::bytes::{ByteRange, ByteReader};
use crate::{RomError, RomImageKind};
use serde::{Deserialize, Serialize};

const NCCH_HEADER_SIZE: u64 = 0x200;
const BASE_MEDIA_UNIT: u64 = 0x200;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NcchRegion {
    pub offset_units: u32,
    pub size_units: u32,
    pub offset: u64,
    pub size: u64,
}

impl NcchRegion {
    fn parse(
        reader: ByteReader<'_>,
        offset_field: u64,
        size_field: u64,
        unit_size: u64,
    ) -> Result<Option<Self>, RomError> {
        let offset_units = reader.u32_le(offset_field)?;
        let size_units = reader.u32_le(size_field)?;
        if size_units == 0 {
            return Ok(None);
        }
        let range = ByteRange::from_units(offset_units, size_units, unit_size, reader.len())?;
        Ok(Some(Self {
            offset_units,
            size_units,
            offset: range.offset,
            size: range.size,
        }))
    }

    pub fn range(&self) -> ByteRange {
        ByteRange {
            offset: self.offset,
            size: self.size,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NcchHeader {
    pub declared_content_units: u32,
    pub content_unit_size: u64,
    pub declared_content_size: u64,
    pub partition_id: u64,
    pub maker_code: String,
    pub format_version: u16,
    pub program_id: u64,
    pub product_code: String,
    pub extended_header_size: u32,
    pub flags: [u8; 8],
    pub executable: bool,
    pub no_crypto: bool,
    pub no_mount_romfs: bool,
    pub plain_region: Option<NcchRegion>,
    pub logo_region: Option<NcchRegion>,
    pub exefs: Option<NcchRegion>,
    pub romfs: Option<NcchRegion>,
}

impl NcchHeader {
    pub fn parse(data: &[u8]) -> Result<Self, RomError> {
        let reader = ByteReader::new(data);
        if reader.len() < NCCH_HEADER_SIZE || reader.bytes(0x100, 4)? != b"NCCH" {
            return Err(RomError::InvalidHeader);
        }

        let flags = reader.array::<8>(0x188)?;
        let content_unit_size = BASE_MEDIA_UNIT
            .checked_shl(u32::from(flags[6]))
            .ok_or(RomError::InvalidHeader)?;
        if content_unit_size > (1u64 << 32) {
            return Err(RomError::InvalidHeader);
        }

        let declared_content_units = reader.u32_le(0x104)?;
        let declared_content_size = u64::from(declared_content_units)
            .checked_mul(content_unit_size)
            .ok_or(RomError::InvalidOffset)?;
        let extended_header_size = reader.u32_le(0x180)?;
        if extended_header_size != 0 {
            ByteRange::new(0x200, u64::from(extended_header_size), reader.len())?;
        }

        Ok(Self {
            declared_content_units,
            content_unit_size,
            declared_content_size,
            partition_id: reader.u64_le(0x108)?,
            maker_code: decode_ascii_field(reader.bytes(0x110, 2)?),
            format_version: reader.u16_le(0x112)?,
            program_id: reader.u64_le(0x118)?,
            product_code: decode_ascii_field(reader.bytes(0x150, 0x10)?),
            extended_header_size,
            executable: flags[5] & 0x02 != 0,
            no_crypto: flags[7] & 0x04 != 0,
            no_mount_romfs: flags[7] & 0x02 != 0,
            plain_region: NcchRegion::parse(reader, 0x190, 0x194, content_unit_size)?,
            logo_region: NcchRegion::parse(reader, 0x198, 0x19C, content_unit_size)?,
            exefs: NcchRegion::parse(reader, 0x1A0, 0x1A4, content_unit_size)?,
            romfs: NcchRegion::parse(reader, 0x1B0, 0x1B4, content_unit_size)?,
            flags,
        })
    }

    pub fn image_kind(&self) -> RomImageKind {
        if self.executable {
            RomImageKind::Cxi
        } else {
            RomImageKind::Ncch
        }
    }
}

fn decode_ascii_field(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0 || *byte == 0xFF)
        .unwrap_or(bytes.len());
    bytes[..end]
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '\u{FFFD}'
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

pub struct NcchImage<'a> {
    data: &'a [u8],
    pub header: NcchHeader,
}

impl<'a> NcchImage<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, RomError> {
        Ok(Self {
            data,
            header: NcchHeader::parse(data)?,
        })
    }

    pub fn exefs_bytes(&self) -> Result<Option<&'a [u8]>, RomError> {
        self.cleartext_region(self.header.exefs.as_ref())
    }

    pub fn romfs_bytes(&self) -> Result<Option<&'a [u8]>, RomError> {
        if self.header.no_mount_romfs {
            return Ok(None);
        }
        self.cleartext_region(self.header.romfs.as_ref())
    }

    fn cleartext_region(&self, region: Option<&NcchRegion>) -> Result<Option<&'a [u8]>, RomError> {
        let Some(region) = region else {
            return Ok(None);
        };
        if !self.header.no_crypto {
            return Err(RomError::EncryptedInput);
        }
        Ok(Some(ByteReader::new(self.data).slice(region.range())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(no_crypto: bool) -> Vec<u8> {
        let mut data = vec![0u8; 0x1200];
        data[0x100..0x104].copy_from_slice(b"NCCH");
        data[0x104..0x108].copy_from_slice(&9u32.to_le_bytes());
        data[0x108..0x110].copy_from_slice(&0x8877665544332211u64.to_le_bytes());
        data[0x110..0x112].copy_from_slice(b"01");
        data[0x112..0x114].copy_from_slice(&2u16.to_le_bytes());
        data[0x118..0x120].copy_from_slice(&0x00040000000EC700u64.to_le_bytes());
        data[0x150..0x15D].copy_from_slice(b"CTR-P-BSK-USA");
        data[0x180..0x184].copy_from_slice(&0x400u32.to_le_bytes());
        data[0x18D] = 0x02;
        data[0x18E] = 0;
        data[0x18F] = if no_crypto { 0x04 } else { 0 };
        data[0x1A0..0x1A4].copy_from_slice(&4u32.to_le_bytes());
        data[0x1A4..0x1A8].copy_from_slice(&1u32.to_le_bytes());
        data[0x1B0..0x1B4].copy_from_slice(&5u32.to_le_bytes());
        data[0x1B4..0x1B8].copy_from_slice(&2u32.to_le_bytes());
        data[0x800..0x804].copy_from_slice(b"EXFS");
        data[0xA00..0xA04].copy_from_slice(b"IVFC");
        data
    }

    #[test]
    fn parses_cxi_metadata_and_regions() {
        let data = fixture(true);
        let image = NcchImage::parse(&data).unwrap();
        assert_eq!(image.header.image_kind(), RomImageKind::Cxi);
        assert_eq!(image.header.content_unit_size, 0x200);
        assert_eq!(image.header.product_code, "CTR-P-BSK-USA");
        assert_eq!(image.header.extended_header_size, 0x400);
        assert_eq!(image.header.exefs.as_ref().unwrap().offset, 0x800);
        assert_eq!(image.header.romfs.as_ref().unwrap().offset, 0xA00);
        assert_eq!(&image.exefs_bytes().unwrap().unwrap()[..4], b"EXFS");
        assert_eq!(&image.romfs_bytes().unwrap().unwrap()[..4], b"IVFC");
    }

    #[test]
    fn encrypted_regions_are_not_exposed_as_cleartext() {
        let data = fixture(false);
        let image = NcchImage::parse(&data).unwrap();
        assert_eq!(image.romfs_bytes(), Err(RomError::EncryptedInput));
    }

    #[test]
    fn content_unit_shift_changes_region_offsets() {
        let mut data = fixture(true);
        data.resize(0x3000, 0);
        data[0x18E] = 2;
        data[0x1A0..0x1A4].copy_from_slice(&2u32.to_le_bytes());
        data[0x1A4..0x1A8].copy_from_slice(&1u32.to_le_bytes());
        let header = NcchHeader::parse(&data).unwrap();
        assert_eq!(header.content_unit_size, 0x800);
        assert_eq!(header.exefs.unwrap().offset, 0x1000);
    }

    #[test]
    fn rejects_region_that_runs_past_eof() {
        let mut data = fixture(true);
        data[0x1B4..0x1B8].copy_from_slice(&0x100u32.to_le_bytes());
        assert_eq!(NcchHeader::parse(&data), Err(RomError::InvalidOffset));
    }
}
