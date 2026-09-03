use crate::EncodedTexture;
use encoding_rs::SHIFT_JIS;
use eo_core::{TextureDimensions, TextureFormat};
use std::collections::BTreeMap;
use thiserror::Error;

const BCH_MIN_SIZE: usize = 0x38;
const SECTION_TEXTURES: usize = 3;
const MAX_TEXTURES: u32 = 4096;
const MAX_COMMAND_WORDS: u32 = 0x4000;
const MAX_STRING_BYTES: usize = 512;
const MAX_EMBEDDED_BCH: usize = 16;

const TEXTURE_UNIT_REGS: [(u16, u16, u16); 3] = [
    (0x0082, 0x008e, 0x0085),
    (0x0092, 0x0096, 0x0095),
    (0x009a, 0x009e, 0x009d),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BchHeader {
    pub backward_compat: u8,
    pub forward_compat: u8,
    pub version: u16,
    pub content_addr: u32,
    pub strings_addr: u32,
    pub commands_addr: u32,
    pub data_addr: u32,
    pub data_ext_addr: u32,
    pub reloc_addr: u32,
    pub content_len: u32,
    pub strings_len: u32,
    pub commands_len: u32,
    pub data_len: u32,
    pub data_ext_len: u32,
    pub reloc_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BchTexture {
    pub encoded: EncodedTexture,
    pub name: String,
    pub descriptor_offset: u64,
    pub texture_unit_descriptor: u8,
    pub raw_data_offset: u32,
    pub data_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BchContainer {
    pub header: BchHeader,
    pub textures: Vec<BchTexture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BchWrapperKind {
    Atbc,
    Bam2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedBch {
    pub offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BchWrapper {
    pub kind: BchWrapperKind,
    pub payloads: Vec<EmbeddedBch>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BchError {
    #[error("BCH header is invalid")]
    InvalidHeader,
    #[error("BCH section is outside the source")]
    InvalidSection,
    #[error("BCH wrapper header is invalid")]
    InvalidWrapper,
    #[error("BCH payload index is invalid")]
    InvalidPayloadIndex,
}

pub fn parse_bch(data: &[u8]) -> Result<BchContainer, BchError> {
    let header = parse_header(data)?;
    let (pointers, dict_offset) = pointer_table_entries(data, &header, SECTION_TEXTURES)?;
    let dict_names = parse_dict_names(data, &header, dict_offset);
    let mut textures = Vec::new();

    for (index, descriptor) in pointers.into_iter().enumerate() {
        if descriptor
            .checked_add(32)
            .is_none_or(|end| end > data.len())
        {
            continue;
        }
        let name_raw = read_u32(data, descriptor + 28).unwrap_or(0);
        let name = resolve_string(data, &header, name_raw)
            .or_else(|| dict_names.get(index).cloned().flatten())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("bch_tex_{index:04}"));

        let mut texture_info = None;
        for unit in 0..TEXTURE_UNIT_REGS.len() {
            let raw_command = read_u32(data, descriptor + unit * 8).unwrap_or(0);
            let word_count = read_u32(data, descriptor + unit * 8 + 4).unwrap_or(0);
            let Some(registers) = command_block(data, &header, raw_command, word_count) else {
                continue;
            };
            if let Some(info) = texture_info_from_registers(&registers, unit) {
                texture_info = Some(info);
                break;
            }
        }
        let Some(info) = texture_info else {
            continue;
        };

        let dimensions = match TextureDimensions::new(info.width, info.height) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let format = match TextureFormat::try_from(info.format) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let size = match usize::try_from(dimensions.encoded_base_size(format)) {
            Ok(value) if value > 0 => value,
            _ => continue,
        };
        let data_offset = [
            usize::try_from(header.data_addr)
                .ok()
                .and_then(|base| base.checked_add(info.raw_data_offset as usize)),
            Some(info.raw_data_offset as usize),
        ]
        .into_iter()
        .flatten()
        .find(|offset| {
            offset
                .checked_add(size)
                .is_some_and(|end| end <= data.len())
        });
        let Some(data_offset) = data_offset else {
            continue;
        };
        let Some(payload) = data
            .get(data_offset..data_offset + size)
            .map(<[u8]>::to_vec)
        else {
            continue;
        };

        textures.push(BchTexture {
            encoded: EncodedTexture {
                dimensions,
                format,
                mip_count: 1,
                payload,
            },
            name,
            descriptor_offset: descriptor as u64,
            texture_unit_descriptor: info.unit as u8,
            raw_data_offset: info.raw_data_offset,
            data_offset: data_offset as u64,
        });
    }

    Ok(BchContainer { header, textures })
}

pub fn parse_bch_wrapper(data: &[u8]) -> Result<BchWrapper, BchError> {
    let kind = match data.get(..4) {
        Some(b"ATBC") => BchWrapperKind::Atbc,
        Some(b"BAM2") => BchWrapperKind::Bam2,
        _ => return Err(BchError::InvalidWrapper),
    };
    let mut payloads = Vec::new();
    let mut search = 0usize;
    while search + 4 <= data.len() && payloads.len() < MAX_EMBEDDED_BCH {
        let Some(relative) = find_magic(&data[search..], b"BCH\0") else {
            break;
        };
        let offset = search + relative;
        search = offset.saturating_add(4);
        if parse_header(&data[offset..]).is_ok() {
            payloads.push(EmbeddedBch {
                offset: offset as u64,
            });
        }
    }
    Ok(BchWrapper { kind, payloads })
}

impl BchWrapper {
    pub fn parse_payload(&self, data: &[u8], index: usize) -> Result<BchContainer, BchError> {
        let payload = self
            .payloads
            .get(index)
            .ok_or(BchError::InvalidPayloadIndex)?;
        let start = usize::try_from(payload.offset).map_err(|_| BchError::InvalidPayloadIndex)?;
        parse_bch(data.get(start..).ok_or(BchError::InvalidPayloadIndex)?)
    }
}

pub fn parse_header(data: &[u8]) -> Result<BchHeader, BchError> {
    if data.len() < BCH_MIN_SIZE || data.get(..4) != Some(b"BCH\0") {
        return Err(BchError::InvalidHeader);
    }
    let backward_compat = data[4];
    let forward_compat = data[5];
    let version = read_u16(data, 6).ok_or(BchError::InvalidHeader)?;
    let content_addr = read_u32(data, 0x08).ok_or(BchError::InvalidHeader)?;
    let strings_addr = read_u32(data, 0x0c).ok_or(BchError::InvalidHeader)?;
    let commands_addr = read_u32(data, 0x10).ok_or(BchError::InvalidHeader)?;
    let data_addr = read_u32(data, 0x14).ok_or(BchError::InvalidHeader)?;

    let mut cursor = 0x18usize;
    let mut data_ext_addr = 0u32;
    let mut data_ext_len = 0u32;
    if backward_compat > 0x20 {
        data_ext_addr = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;
        cursor += 4;
    }
    let reloc_addr = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;
    cursor += 4;
    let content_len = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;
    cursor += 4;
    let strings_len = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;
    cursor += 4;
    let commands_len = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;
    cursor += 4;
    let data_len = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;
    cursor += 4;
    if backward_compat > 0x20 {
        data_ext_len = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;
        cursor += 4;
    }
    let reloc_len = read_u32(data, cursor).ok_or(BchError::InvalidHeader)?;

    for address in [content_addr, strings_addr, commands_addr, data_addr, reloc_addr] {
        if address != 0 && address as usize >= data.len() {
            return Err(BchError::InvalidSection);
        }
    }
    if data_ext_addr != 0 && data_ext_addr as usize >= data.len() {
        return Err(BchError::InvalidSection);
    }
    if reloc_addr != 0
        && reloc_len != 0
        && (reloc_addr as usize)
            .checked_add(reloc_len as usize)
            .is_none_or(|end| end > data.len())
    {
        return Err(BchError::InvalidSection);
    }

    Ok(BchHeader {
        backward_compat,
        forward_compat,
        version,
        content_addr,
        strings_addr,
        commands_addr,
        data_addr,
        data_ext_addr,
        reloc_addr,
        content_len,
        strings_len,
        commands_len,
        data_len,
        data_ext_len,
        reloc_len,
    })
}

fn section(data: &[u8], header: &BchHeader, index: usize) -> Option<(u32, u32, u32)> {
    let base = header.content_addr as usize;
    let offset = base.checked_add(index.checked_mul(12)?)?;
    Some((
        read_u32(data, offset)?,
        read_u32(data, offset + 4)?,
        read_u32(data, offset + 8)?,
    ))
}

fn resolve_main_offset(raw: u32, header: &BchHeader, data_len: usize) -> Option<usize> {
    if raw == 0 {
        return None;
    }
    let content = header.content_addr as usize;
    let raw = raw as usize;
    let upper = if header.strings_addr > header.content_addr {
        (header.strings_addr as usize).min(data_len)
    } else {
        data_len
    };
    let candidates = [content.checked_add(raw), Some(raw)];
    for value in candidates.into_iter().flatten() {
        if content <= value && value < upper {
            return Some(value);
        }
    }
    candidates
        .into_iter()
        .flatten()
        .find(|value| *value < data_len)
}

fn pointer_table_entries(
    data: &[u8],
    header: &BchHeader,
    section_index: usize,
) -> Result<(Vec<usize>, u32), BchError> {
    let Some((pointer_offset, count, dict_offset)) = section(data, header, section_index) else {
        return Ok((Vec::new(), 0));
    };
    if pointer_offset == 0 || count == 0 || count > MAX_TEXTURES {
        return Ok((Vec::new(), dict_offset));
    }
    let Some(table) = resolve_main_offset(pointer_offset, header, data.len()) else {
        return Ok((Vec::new(), dict_offset));
    };
    let table_end = table
        .checked_add(count as usize * 4)
        .ok_or(BchError::InvalidSection)?;
    if table_end > data.len() {
        return Ok((Vec::new(), dict_offset));
    }

    let mut entries = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let raw = read_u32(data, table + index * 4).unwrap_or(0);
        if let Some(offset) = resolve_main_offset(raw, header, data.len()) {
            entries.push(offset);
        }
    }
    Ok((entries, dict_offset))
}

fn parse_dict_names(data: &[u8], header: &BchHeader, dict_offset: u32) -> Vec<Option<String>> {
    let Some(base) = resolve_main_offset(dict_offset, header, data.len()) else {
        return Vec::new();
    };
    if base.checked_add(8).is_none_or(|end| end > data.len()) {
        return Vec::new();
    }
    let count = read_u32(data, base + 4).unwrap_or(0);
    if count == 0 || count > MAX_TEXTURES {
        return Vec::new();
    }
    let mut names = Vec::new();
    let entry_start = base + 8;
    for index in 1..=count as usize {
        let Some(offset) = entry_start.checked_add(index * 16) else {
            break;
        };
        if offset.checked_add(16).is_none_or(|end| end > data.len()) {
            break;
        }
        let raw = read_u32(data, offset + 8).unwrap_or(0);
        names.push(resolve_string(data, header, raw));
    }
    names
}

fn resolve_string(data: &[u8], header: &BchHeader, raw: u32) -> Option<String> {
    let strings_start = header.strings_addr as usize;
    let strings_end = if header.strings_len == 0 {
        data.len()
    } else {
        strings_start
            .checked_add(header.strings_len as usize)
            .unwrap_or(data.len())
            .min(data.len())
    };
    let candidates = [
        strings_start.checked_add(raw as usize),
        Some(raw as usize),
    ];
    for position in candidates.into_iter().flatten() {
        if position < strings_start || position >= strings_end {
            continue;
        }
        let end_limit = strings_end.min(position.saturating_add(MAX_STRING_BYTES));
        let raw_string = data.get(position..end_limit)?;
        let Some(end) = raw_string.iter().position(|byte| *byte == 0) else {
            continue;
        };
        let bytes = &raw_string[..end];
        if bytes.is_empty() {
            continue;
        }
        let text = if let Ok(value) = std::str::from_utf8(bytes) {
            value.to_owned()
        } else {
            let (decoded, _) = SHIFT_JIS.decode_without_bom_handling(bytes);
            decoded.into_owned()
        };
        if !text.is_empty()
            && text
                .chars()
                .all(|character| !character.is_control() || matches!(character, '\t' | '\r' | '\n'))
        {
            return Some(text);
        }
    }
    None
}

fn command_block(
    data: &[u8],
    header: &BchHeader,
    raw_pointer: u32,
    word_count: u32,
) -> Option<BTreeMap<u16, u32>> {
    if word_count == 0 || word_count > MAX_COMMAND_WORDS {
        return None;
    }
    let relative = (header.commands_addr as usize).checked_add(raw_pointer as usize);
    for start in [relative, Some(raw_pointer as usize)].into_iter().flatten() {
        let byte_count = (word_count as usize).checked_mul(4)?;
        if start
            .checked_add(byte_count)
            .is_none_or(|end| end > data.len())
        {
            continue;
        }
        let registers = parse_gpu_commands(data, start, word_count);
        if !registers.is_empty() {
            return Some(registers);
        }
    }
    None
}

fn parse_gpu_commands(data: &[u8], start: usize, word_count: u32) -> BTreeMap<u16, u32> {
    let mut registers = BTreeMap::new();
    let Some(end) = start.checked_add(word_count as usize * 4) else {
        return registers;
    };
    let end = end.min(data.len());
    let mut position = start;
    while position + 8 <= end {
        let Some(parameter) = read_u32(data, position) else {
            break;
        };
        let Some(command) = read_u32(data, position + 4) else {
            break;
        };
        let register = (command & 0xffff) as u16;
        let extra = ((command >> 20) & 0xff) as usize;
        let consecutive = command & 0x8000_0000 != 0;
        position += 8;
        registers.insert(register, parameter);
        for index in 0..extra {
            if position + 4 > end {
                break;
            }
            let Some(value) = read_u32(data, position) else {
                break;
            };
            position += 4;
            let target = if consecutive {
                register.wrapping_add(index as u16 + 1)
            } else {
                register
            };
            registers.insert(target, value);
        }
        if position & 7 != 0 {
            position = position.saturating_add(4);
        }
    }
    registers
}

#[derive(Clone, Copy)]
struct TextureInfo {
    width: u32,
    height: u32,
    format: u8,
    raw_data_offset: u32,
    unit: usize,
}

fn texture_info_from_registers(
    registers: &BTreeMap<u16, u32>,
    unit: usize,
) -> Option<TextureInfo> {
    let (dimension_register, type_register, address_register) = *TEXTURE_UNIT_REGS.get(unit)?;
    let dimensions = *registers.get(&dimension_register)?;
    let raw_data_offset = *registers.get(&address_register)?;
    let width = (dimensions >> 16) & 0x7ff;
    let height = dimensions & 0x7ff;
    let format = (registers.get(&type_register).copied().unwrap_or(0) & 0x0f) as u8;
    if !(4..=4096).contains(&width)
        || !(4..=4096).contains(&height)
        || TextureFormat::try_from(format).is_err()
    {
        return None;
    }
    Some(TextureInfo {
        width,
        height,
        format,
        raw_data_offset,
        unit,
    })
}

fn find_magic(data: &[u8], magic: &[u8; 4]) -> Option<usize> {
    data.windows(4).position(|window| window == magic)
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NativePicaDecoder, TextureDecoder};

    fn synthetic_bch() -> Vec<u8> {
        let mut data = vec![0u8; 0x300];
        data[0..4].copy_from_slice(b"BCH\0");
        data[4] = 0x20;
        data[5] = 0;
        data[6..8].copy_from_slice(&1u16.to_le_bytes());
        data[0x08..0x0c].copy_from_slice(&0x40u32.to_le_bytes());
        data[0x0c..0x10].copy_from_slice(&0x100u32.to_le_bytes());
        data[0x10..0x14].copy_from_slice(&0x140u32.to_le_bytes());
        data[0x14..0x18].copy_from_slice(&0x200u32.to_le_bytes());
        data[0x18..0x1c].copy_from_slice(&0u32.to_le_bytes());
        data[0x1c..0x20].copy_from_slice(&0xc0u32.to_le_bytes());
        data[0x20..0x24].copy_from_slice(&0x40u32.to_le_bytes());
        data[0x24..0x28].copy_from_slice(&0xc0u32.to_le_bytes());
        data[0x28..0x2c].copy_from_slice(&0x100u32.to_le_bytes());
        data[0x2c..0x30].copy_from_slice(&0u32.to_le_bytes());

        let section = 0x40 + SECTION_TEXTURES * 12;
        data[section..section + 4].copy_from_slice(&0x40u32.to_le_bytes());
        data[section + 4..section + 8].copy_from_slice(&1u32.to_le_bytes());
        data[section + 8..section + 12].copy_from_slice(&0u32.to_le_bytes());
        data[0x80..0x84].copy_from_slice(&0x60u32.to_le_bytes());

        let descriptor = 0xa0usize;
        data[descriptor..descriptor + 4].copy_from_slice(&0u32.to_le_bytes());
        data[descriptor + 4..descriptor + 8].copy_from_slice(&6u32.to_le_bytes());
        data[descriptor + 28..descriptor + 32].copy_from_slice(&0u32.to_le_bytes());
        data[0x100..0x104].copy_from_slice(b"tex\0");

        let commands = [
            (8u32 << 16) | 8,
            0x0082,
            TextureFormat::Rgba8 as u32,
            0x008e,
            0,
            0x0085,
        ];
        for (index, word) in commands.into_iter().enumerate() {
            let offset = 0x140 + index * 4;
            data[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        data[0x200..0x204].copy_from_slice(&[4, 3, 2, 1]);
        data
    }

    #[test]
    fn version_aware_header_and_texture_commands_emit_base_level() {
        let data = synthetic_bch();
        let parsed = parse_bch(&data).unwrap();
        assert_eq!(parsed.header.backward_compat, 0x20);
        assert_eq!(parsed.textures.len(), 1);
        let texture = &parsed.textures[0];
        assert_eq!(texture.name, "tex");
        assert_eq!(texture.texture_unit_descriptor, 0);
        assert_eq!(texture.data_offset, 0x200);
        assert_eq!(texture.encoded.payload.len(), 256);
        let decoded = NativePicaDecoder
            .decode_base_level(&texture.encoded)
            .unwrap();
        assert_eq!(&decoded.rgba8[..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn embedded_bch_is_found_without_fixed_wrapper_offset() {
        let bch = synthetic_bch();
        let offset = 0x91usize;
        let mut wrapper = vec![0u8; offset + bch.len()];
        wrapper[..4].copy_from_slice(b"BAM2");
        wrapper[offset..].copy_from_slice(&bch);
        let parsed = parse_bch_wrapper(&wrapper).unwrap();
        assert_eq!(parsed.kind, BchWrapperKind::Bam2);
        assert_eq!(parsed.payloads, vec![EmbeddedBch { offset: offset as u64 }]);
        assert_eq!(parsed.parse_payload(&wrapper, 0).unwrap().textures.len(), 1);
    }

    #[test]
    fn false_bch_magic_is_not_accepted_without_valid_header() {
        let mut wrapper = vec![0u8; 0x100];
        wrapper[..4].copy_from_slice(b"ATBC");
        wrapper[0x40..0x44].copy_from_slice(b"BCH\0");
        assert!(parse_bch_wrapper(&wrapper).unwrap().payloads.is_empty());
    }

    #[test]
    fn non_aligned_dimensions_use_padded_base_size() {
        let mut data = synthetic_bch();
        let commands = [
            (7u32 << 16) | 7,
            0x0082,
            TextureFormat::A8 as u32,
            0x008e,
            0,
            0x0085,
        ];
        for (index, word) in commands.into_iter().enumerate() {
            let offset = 0x140 + index * 4;
            data[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        let parsed = parse_bch(&data).unwrap();
        assert_eq!(parsed.textures[0].encoded.payload.len(), 64);
        assert_eq!(parsed.textures[0].encoded.dimensions.storage_width, 8);
        assert_eq!(parsed.textures[0].encoded.dimensions.storage_height, 8);
    }
}
