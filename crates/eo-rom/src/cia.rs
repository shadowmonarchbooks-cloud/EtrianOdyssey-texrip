use crate::bytes::{ByteRange, ByteReader};
use crate::RomError;
use eo_core::TitleId;
use serde::{Deserialize, Serialize};

const CIA_MIN_HEADER_SIZE: u64 = 0x2020;
const CIA_SECTION_ALIGNMENT: u64 = 0x40;
const CONTENT_INDEX_OFFSET: u64 = 0x20;
const CONTENT_INDEX_SIZE: u64 = 0x2000;
const TMD_CONTENT_RECORDS_FROM_SIGNATURE_END: u64 = 0x9C4;
const TMD_CONTENT_RECORD_SIZE: u64 = 0x30;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiaSection {
    pub offset: u64,
    pub size: u64,
}

impl CiaSection {
    pub fn range(self) -> ByteRange {
        ByteRange {
            offset: self.offset,
            size: self.size,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiaContent {
    pub id: u32,
    pub index: u16,
    pub content_type: u16,
    pub size: u64,
    pub encrypted: bool,
    pub included: bool,
    pub offset: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiaHeader {
    pub header_size: u32,
    pub archive_type: u16,
    pub version: u16,
    pub certificate_chain_size: u32,
    pub ticket_size: u32,
    pub tmd_size: u32,
    pub meta_size: u32,
    pub content_size: u64,
    pub certificate_chain: CiaSection,
    pub ticket: CiaSection,
    pub tmd: CiaSection,
    pub content: CiaSection,
    pub meta: Option<CiaSection>,
}

pub struct CiaImage<'a> {
    data: &'a [u8],
    pub header: CiaHeader,
    pub title_id: Option<TitleId>,
    pub contents: Vec<CiaContent>,
    pub content_alignment: u64,
}

impl<'a> CiaImage<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, RomError> {
        let reader = ByteReader::new(data);
        if reader.len() < CIA_MIN_HEADER_SIZE {
            return Err(RomError::InvalidHeader);
        }

        let header_size = reader.u32_le(0)?;
        if u64::from(header_size) < CIA_MIN_HEADER_SIZE {
            return Err(RomError::InvalidHeader);
        }
        ByteRange::new(0, u64::from(header_size), reader.len())?;
        let archive_type = reader.u16_le(0x04)?;
        let version = reader.u16_le(0x06)?;
        let certificate_chain_size = reader.u32_le(0x08)?;
        let ticket_size = reader.u32_le(0x0C)?;
        let tmd_size = reader.u32_le(0x10)?;
        let meta_size = reader.u32_le(0x14)?;
        let content_size = reader.u64_le(0x18)?;
        let content_index = reader.bytes(CONTENT_INDEX_OFFSET, CONTENT_INDEX_SIZE)?;

        let certificate_chain_offset = align_up(u64::from(header_size), CIA_SECTION_ALIGNMENT)?;
        let ticket_offset = align_up(
            certificate_chain_offset
                .checked_add(u64::from(certificate_chain_size))
                .ok_or(RomError::InvalidOffset)?,
            CIA_SECTION_ALIGNMENT,
        )?;
        let tmd_offset = align_up(
            ticket_offset
                .checked_add(u64::from(ticket_size))
                .ok_or(RomError::InvalidOffset)?,
            CIA_SECTION_ALIGNMENT,
        )?;
        let content_offset = align_up(
            tmd_offset
                .checked_add(u64::from(tmd_size))
                .ok_or(RomError::InvalidOffset)?,
            CIA_SECTION_ALIGNMENT,
        )?;
        let meta_offset = align_up(
            content_offset
                .checked_add(content_size)
                .ok_or(RomError::InvalidOffset)?,
            CIA_SECTION_ALIGNMENT,
        )?;

        let certificate_chain = checked_section(
            certificate_chain_offset,
            u64::from(certificate_chain_size),
            reader.len(),
        )?;
        let ticket = checked_section(ticket_offset, u64::from(ticket_size), reader.len())?;
        let tmd = checked_section(tmd_offset, u64::from(tmd_size), reader.len())?;
        let content = checked_section(content_offset, content_size, reader.len())?;
        let meta = if meta_size == 0 {
            None
        } else {
            Some(checked_section(
                meta_offset,
                u64::from(meta_size),
                reader.len(),
            )?)
        };

        let tmd_bytes = reader.slice(tmd.range())?;
        let (title_id, mut contents) = parse_tmd(tmd_bytes, content_index)?;
        let content_alignment = detect_content_alignment(&contents, content_size)?;
        assign_content_offsets(
            &mut contents,
            content_offset,
            content_size,
            content_alignment,
            reader.len(),
        )?;

        Ok(Self {
            data,
            header: CiaHeader {
                header_size,
                archive_type,
                version,
                certificate_chain_size,
                ticket_size,
                tmd_size,
                meta_size,
                content_size,
                certificate_chain,
                ticket,
                tmd,
                content,
                meta,
            },
            title_id,
            contents,
            content_alignment,
        })
    }

    pub fn main_content(&self) -> Result<&'a [u8], RomError> {
        let main = self
            .contents
            .iter()
            .find(|content| content.index == 0 && content.included)
            .ok_or_else(|| RomError::MissingEntry("CIA main content".to_owned()))?;
        if main.encrypted {
            return Err(RomError::EncryptedInput);
        }
        let offset = main.offset.ok_or_else(|| {
            RomError::Malformed("included CIA main content has no offset".to_owned())
        })?;
        ByteReader::new(self.data).bytes(offset, main.size)
    }
}

fn checked_section(offset: u64, size: u64, source_len: u64) -> Result<CiaSection, RomError> {
    ByteRange::new(offset, size, source_len)?;
    Ok(CiaSection { offset, size })
}

fn parse_tmd(
    data: &[u8],
    content_index: &[u8],
) -> Result<(Option<TitleId>, Vec<CiaContent>), RomError> {
    let reader = ByteReader::new(data);
    let signature_type = reader.u32_be(0)?;
    let signature_block_size = signature_block_size(signature_type).ok_or_else(|| {
        RomError::Malformed(format!("unsupported TMD signature type 0x{signature_type:08X}"))
    })?;
    let header_end = signature_block_size
        .checked_add(TMD_CONTENT_RECORDS_FROM_SIGNATURE_END)
        .ok_or(RomError::InvalidOffset)?;
    if header_end > reader.len() {
        return Err(RomError::InvalidOffset);
    }

    let raw_title_id = reader.u64_be(signature_block_size + 0x4C)?;
    let title_id = if raw_title_id == 0 {
        None
    } else {
        Some(
            format!("{raw_title_id:016X}")
                .parse::<TitleId>()
                .map_err(|_| RomError::Malformed("invalid TMD Title ID".to_owned()))?,
        )
    };
    let content_count = usize::from(reader.u16_be(signature_block_size + 0x9E)?);
    let records_end = header_end
        .checked_add(
            (content_count as u64)
                .checked_mul(TMD_CONTENT_RECORD_SIZE)
                .ok_or(RomError::InvalidOffset)?,
        )
        .ok_or(RomError::InvalidOffset)?;
    if records_end > reader.len() {
        return Err(RomError::InvalidOffset);
    }

    let mut contents = Vec::with_capacity(content_count);
    for record_index in 0..content_count {
        let base = header_end + record_index as u64 * TMD_CONTENT_RECORD_SIZE;
        let id = reader.u32_be(base)?;
        let index = reader.u16_be(base + 0x04)?;
        let content_type = reader.u16_be(base + 0x06)?;
        let size = reader.u64_be(base + 0x08)?;
        contents.push(CiaContent {
            id,
            index,
            content_type,
            size,
            encrypted: content_type & 0x0001 != 0,
            included: content_index_contains(content_index, index)?,
            offset: None,
        });
    }
    Ok((title_id, contents))
}

fn content_index_contains(index_table: &[u8], index: u16) -> Result<bool, RomError> {
    let byte_index = usize::from(index / 8);
    let byte = *index_table.get(byte_index).ok_or(RomError::InvalidOffset)?;
    Ok(byte & (0x80 >> (index & 7)) != 0)
}

fn detect_content_alignment(contents: &[CiaContent], declared_size: u64) -> Result<u64, RomError> {
    for alignment in [0x40u64, 0x10, 1] {
        if content_total(contents, alignment)? == declared_size {
            return Ok(alignment);
        }
    }
    Err(RomError::Malformed(
        "CIA content sizes do not match the declared content section".to_owned(),
    ))
}

fn content_total(contents: &[CiaContent], alignment: u64) -> Result<u64, RomError> {
    let mut total = 0u64;
    for content in contents.iter().filter(|content| content.included) {
        total = total
            .checked_add(align_up(content.size, alignment)?)
            .ok_or(RomError::InvalidOffset)?;
    }
    Ok(total)
}

fn assign_content_offsets(
    contents: &mut [CiaContent],
    content_offset: u64,
    declared_size: u64,
    alignment: u64,
    source_len: u64,
) -> Result<(), RomError> {
    let mut cursor = content_offset;
    for content in contents.iter_mut().filter(|content| content.included) {
        ByteRange::new(cursor, content.size, source_len)?;
        content.offset = Some(cursor);
        cursor = cursor
            .checked_add(align_up(content.size, alignment)?)
            .ok_or(RomError::InvalidOffset)?;
    }
    let expected_end = content_offset
        .checked_add(declared_size)
        .ok_or(RomError::InvalidOffset)?;
    if cursor != expected_end {
        return Err(RomError::Malformed(
            "CIA content layout does not consume the declared section".to_owned(),
        ));
    }
    Ok(())
}

fn signature_block_size(signature_type: u32) -> Option<u64> {
    match signature_type {
        0x0001_0000 | 0x0001_0003 => Some(0x240),
        0x0001_0001 | 0x0001_0004 => Some(0x140),
        0x0001_0002 | 0x0001_0005 => Some(0x80),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16_be(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn put_u32_le(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32_be(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn put_u64_le(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64_be(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
    }

    fn fixture(encrypted: bool, content_alignment: u64) -> Vec<u8> {
        let header_size = 0x2020u32;
        let tmd_size = 0xB34u32;
        let raw_content_size = 0x220u64;
        let stored_content_size = align_up(raw_content_size, content_alignment).unwrap();
        let tmd_offset = 0x2040usize;
        let content_offset = 0x2B80usize;
        let mut data = vec![0u8; content_offset + stored_content_size as usize];

        put_u32_le(&mut data, 0, header_size);
        put_u32_le(&mut data, 0x10, tmd_size);
        put_u64_le(&mut data, 0x18, stored_content_size);
        data[0x20] = 0x80;

        put_u32_be(&mut data, tmd_offset, 0x0001_0004);
        let sig_end = tmd_offset + 0x140;
        put_u64_be(&mut data, sig_end + 0x4C, 0x0004_0000_000E_C700);
        put_u16_be(&mut data, sig_end + 0x9E, 1);
        let record = sig_end + 0x9C4;
        put_u32_be(&mut data, record, 0x12345678);
        put_u16_be(&mut data, record + 0x04, 0);
        put_u16_be(&mut data, record + 0x06, if encrypted { 1 } else { 0 });
        put_u64_be(&mut data, record + 0x08, raw_content_size);
        data[content_offset + 0x100..content_offset + 0x104].copy_from_slice(b"NCCH");
        data
    }

    #[test]
    fn parses_cleartext_main_content_and_title_id() {
        let data = fixture(false, 0x40);
        let cia = CiaImage::parse(&data).unwrap();
        assert_eq!(cia.content_alignment, 0x40);
        assert_eq!(cia.title_id.unwrap().to_string(), "00040000000EC700");
        assert_eq!(cia.contents.len(), 1);
        assert_eq!(cia.contents[0].offset, Some(0x2B80));
        assert_eq!(&cia.main_content().unwrap()[0x100..0x104], b"NCCH");
    }

    #[test]
    fn accepts_makerom_style_16_byte_content_alignment() {
        let data = fixture(false, 0x10);
        let cia = CiaImage::parse(&data).unwrap();
        assert_eq!(cia.content_alignment, 0x10);
    }

    #[test]
    fn encrypted_main_content_is_not_exposed_as_cleartext() {
        let data = fixture(true, 0x40);
        let cia = CiaImage::parse(&data).unwrap();
        assert_eq!(cia.main_content(), Err(RomError::EncryptedInput));
    }

    #[test]
    fn rejects_tmd_content_extent_past_file() {
        let mut data = fixture(false, 0x40);
        data.truncate(data.len() - 1);
        assert!(matches!(CiaImage::parse(&data), Err(RomError::InvalidOffset)));
    }
}
