use crate::EncodedTexture;
use encoding_rs::SHIFT_JIS;
use eo_core::{TextureDimensions, TextureFormat};
use thiserror::Error;

const CTPK_HEADER_SIZE: usize = 0x20;
const CTPK_ENTRY_SIZE: usize = 0x20;
const MAX_TEXTURES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtpkTextureType {
    CubeMap,
    OneDimensional,
    TwoDimensional,
    Unknown(u8),
}

impl CtpkTextureType {
    const fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::CubeMap,
            1 => Self::OneDimensional,
            2 => Self::TwoDimensional,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtpkTexture {
    pub index: u16,
    pub name: Option<String>,
    pub declared_size: u32,
    pub data_offset: u32,
    pub format_raw: u32,
    pub width: u16,
    pub height: u16,
    pub mip_level: u8,
    pub texture_type: CtpkTextureType,
    pub bitmap_size_offset: u32,
    pub encoded: Option<EncodedTexture>,
    pub trailing_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtpkContainer {
    pub version: u16,
    pub texture_data_offset: u32,
    pub texture_data_size: u32,
    pub hash_section_offset: u32,
    pub conversion_info_offset: u32,
    pub textures: Vec<CtpkTexture>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CtpkError {
    #[error("not a structurally valid CTPK container")]
    InvalidHeader,
    #[error("CTPK texture count {0} exceeds structural limit")]
    TooManyTextures(u16),
    #[error("CTPK section offset/size is outside the container")]
    InvalidSection,
    #[error("CTPK texture entry {index} has invalid dimensions {width}x{height}")]
    InvalidDimensions {
        index: u16,
        width: u16,
        height: u16,
    },
    #[error("CTPK texture entry {index} uses unsupported format value 0x{format:x}")]
    UnsupportedFormat { index: u16, format: u32 },
    #[error("CTPK texture entry {index} data extent is outside the texture-data section")]
    InvalidTextureExtent { index: u16 },
    #[error("CTPK texture entry {index} base level is truncated: expected {expected} bytes, found {actual}")]
    TruncatedBaseLevel {
        index: u16,
        expected: u64,
        actual: u64,
    },
    #[error("CTPK texture entry {index} file-path offset is invalid")]
    InvalidNameOffset { index: u16 },
}

pub fn is_ctpk(data: &[u8]) -> bool {
    data.len() >= CTPK_HEADER_SIZE && data.get(..4) == Some(b"CTPK")
}

pub fn parse_ctpk(data: &[u8]) -> Result<CtpkContainer, CtpkError> {
    if !is_ctpk(data) {
        return Err(CtpkError::InvalidHeader);
    }

    let version = read_u16(data, 0x04)?;
    let texture_count = read_u16(data, 0x06)?;
    let texture_data_offset = read_u32(data, 0x08)?;
    let texture_data_size = read_u32(data, 0x0c)?;
    let hash_section_offset = read_u32(data, 0x10)?;
    let conversion_info_offset = read_u32(data, 0x14)?;

    if usize::from(texture_count) > MAX_TEXTURES {
        return Err(CtpkError::TooManyTextures(texture_count));
    }
    let table_size = usize::from(texture_count)
        .checked_mul(CTPK_ENTRY_SIZE)
        .ok_or(CtpkError::InvalidHeader)?;
    let table_end = CTPK_HEADER_SIZE
        .checked_add(table_size)
        .ok_or(CtpkError::InvalidHeader)?;
    if table_end > data.len() {
        return Err(CtpkError::InvalidHeader);
    }

    let texture_section_start = usize::try_from(texture_data_offset)
        .map_err(|_| CtpkError::InvalidSection)?;
    let texture_section_size = usize::try_from(texture_data_size)
        .map_err(|_| CtpkError::InvalidSection)?;
    let texture_section_end = texture_section_start
        .checked_add(texture_section_size)
        .ok_or(CtpkError::InvalidSection)?;
    if texture_section_start < table_end || texture_section_end > data.len() {
        return Err(CtpkError::InvalidSection);
    }
    for optional_offset in [hash_section_offset, conversion_info_offset] {
        if optional_offset != 0
            && usize::try_from(optional_offset)
                .ok()
                .is_none_or(|offset| offset > data.len())
        {
            return Err(CtpkError::InvalidSection);
        }
    }

    let mut textures = Vec::with_capacity(usize::from(texture_count));
    for raw_index in 0..texture_count {
        let index = raw_index;
        let entry_offset = CTPK_HEADER_SIZE + usize::from(index) * CTPK_ENTRY_SIZE;
        let path_offset = read_u32(data, entry_offset)?;
        let declared_size = read_u32(data, entry_offset + 0x04)?;
        let relative_data_offset = read_u32(data, entry_offset + 0x08)?;
        let format_raw = read_u32(data, entry_offset + 0x0c)?;
        let width = read_u16(data, entry_offset + 0x10)?;
        let height = read_u16(data, entry_offset + 0x12)?;
        let mip_level = *data.get(entry_offset + 0x14).ok_or(CtpkError::InvalidHeader)?;
        let texture_type_raw = *data.get(entry_offset + 0x15).ok_or(CtpkError::InvalidHeader)?;
        let bitmap_size_offset = read_u32(data, entry_offset + 0x18)?;
        let texture_type = CtpkTextureType::from_raw(texture_type_raw);

        let name = parse_name(data, index, path_offset)?;
        let relative_start = usize::try_from(relative_data_offset)
            .map_err(|_| CtpkError::InvalidTextureExtent { index })?;
        let declared_size_usize = usize::try_from(declared_size)
            .map_err(|_| CtpkError::InvalidTextureExtent { index })?;
        let relative_end = relative_start
            .checked_add(declared_size_usize)
            .ok_or(CtpkError::InvalidTextureExtent { index })?;
        if relative_end > texture_section_size {
            return Err(CtpkError::InvalidTextureExtent { index });
        }
        let absolute_start = texture_section_start
            .checked_add(relative_start)
            .ok_or(CtpkError::InvalidTextureExtent { index })?;
        let absolute_end = texture_section_start
            .checked_add(relative_end)
            .ok_or(CtpkError::InvalidTextureExtent { index })?;
        let texture_bytes = data
            .get(absolute_start..absolute_end)
            .ok_or(CtpkError::InvalidTextureExtent { index })?;

        let (encoded, trailing_bytes) = if texture_type == CtpkTextureType::TwoDimensional {
            let dimensions = TextureDimensions::new(u32::from(width), u32::from(height)).map_err(|_| {
                CtpkError::InvalidDimensions {
                    index,
                    width,
                    height,
                }
            })?;
            let format_u8 = u8::try_from(format_raw)
                .map_err(|_| CtpkError::UnsupportedFormat { index, format: format_raw })?;
            let format = TextureFormat::try_from(format_u8)
                .map_err(|_| CtpkError::UnsupportedFormat { index, format: format_raw })?;
            let base_size = dimensions.encoded_base_size(format);
            let actual = texture_bytes.len() as u64;
            if actual < base_size {
                return Err(CtpkError::TruncatedBaseLevel {
                    index,
                    expected: base_size,
                    actual,
                });
            }
            let base_size_usize = usize::try_from(base_size)
                .map_err(|_| CtpkError::InvalidTextureExtent { index })?;
            (
                Some(EncodedTexture {
                    dimensions,
                    format,
                    // CTPK's raw mip-level field is retained above. Until the
                    // per-mip bitmap-size array is normalized, only the exact
                    // base-level span is exposed to the shared decoder/hash path.
                    mip_count: 1,
                    payload: texture_bytes[..base_size_usize].to_vec(),
                }),
                actual - base_size,
            )
        } else {
            (None, texture_bytes.len() as u64)
        };

        textures.push(CtpkTexture {
            index,
            name,
            declared_size,
            data_offset: relative_data_offset,
            format_raw,
            width,
            height,
            mip_level,
            texture_type,
            bitmap_size_offset,
            encoded,
            trailing_bytes,
        });
    }

    Ok(CtpkContainer {
        version,
        texture_data_offset,
        texture_data_size,
        hash_section_offset,
        conversion_info_offset,
        textures,
    })
}

fn parse_name(data: &[u8], index: u16, path_offset: u32) -> Result<Option<String>, CtpkError> {
    if path_offset == 0 {
        return Ok(None);
    }
    let start = usize::try_from(path_offset).map_err(|_| CtpkError::InvalidNameOffset { index })?;
    let tail = data
        .get(start..)
        .ok_or(CtpkError::InvalidNameOffset { index })?;
    let end = tail.iter().position(|byte| *byte == 0).unwrap_or(tail.len());
    if end == tail.len() {
        return Err(CtpkError::InvalidNameOffset { index });
    }
    let (decoded, _) = SHIFT_JIS.decode_without_bom_handling(&tail[..end]);
    let value = decoded.trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, CtpkError> {
    data.get(offset..offset + 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(CtpkError::InvalidHeader)
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, CtpkError> {
    data.get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(CtpkError::InvalidHeader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NativePicaDecoder, TextureDecoder};

    fn synthetic_ctpk(
        format: TextureFormat,
        width: u16,
        height: u16,
        texture_type: u8,
        declared_size: usize,
    ) -> Vec<u8> {
        let name = b"ui/test_texture.bin\0";
        let name_offset = 0x40usize;
        let texture_offset = 0x80usize;
        let total = texture_offset + declared_size;
        let mut data = vec![0u8; total.max(name_offset + name.len())];
        data[0..4].copy_from_slice(b"CTPK");
        data[0x04..0x06].copy_from_slice(&1u16.to_le_bytes());
        data[0x06..0x08].copy_from_slice(&1u16.to_le_bytes());
        data[0x08..0x0c].copy_from_slice(&(texture_offset as u32).to_le_bytes());
        data[0x0c..0x10].copy_from_slice(&(declared_size as u32).to_le_bytes());
        data[0x20..0x24].copy_from_slice(&(name_offset as u32).to_le_bytes());
        data[0x24..0x28].copy_from_slice(&(declared_size as u32).to_le_bytes());
        data[0x28..0x2c].copy_from_slice(&0u32.to_le_bytes());
        data[0x2c..0x30].copy_from_slice(&(format as u32).to_le_bytes());
        data[0x30..0x32].copy_from_slice(&width.to_le_bytes());
        data[0x32..0x34].copy_from_slice(&height.to_le_bytes());
        data[0x34] = 1;
        data[0x35] = texture_type;
        data[name_offset..name_offset + name.len()].copy_from_slice(name);
        data
    }

    #[test]
    fn parses_exact_rgba8_base_level() {
        let mut data = synthetic_ctpk(TextureFormat::Rgba8, 8, 8, 2, 256);
        data[0x80..0x84].copy_from_slice(&[4, 3, 2, 1]);
        let parsed = parse_ctpk(&data).unwrap();
        assert_eq!(parsed.textures.len(), 1);
        let texture = &parsed.textures[0];
        assert_eq!(texture.name.as_deref(), Some("ui/test_texture.bin"));
        assert_eq!(texture.texture_type, CtpkTextureType::TwoDimensional);
        assert_eq!(texture.trailing_bytes, 0);
        let encoded = texture.encoded.as_ref().unwrap();
        assert_eq!(encoded.payload.len(), 256);
        let decoded = NativePicaDecoder.decode_base_level(encoded).unwrap();
        assert_eq!(&decoded.rgba8[..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn base_level_excludes_declared_trailing_mip_bytes() {
        let mut data = synthetic_ctpk(TextureFormat::A8, 8, 8, 2, 96);
        data[0x80..0xc0].fill(0x11);
        data[0xc0..0xe0].fill(0xcc);
        let parsed = parse_ctpk(&data).unwrap();
        let texture = &parsed.textures[0];
        assert_eq!(texture.encoded.as_ref().unwrap().payload, vec![0x11; 64]);
        assert_eq!(texture.trailing_bytes, 32);
    }

    #[test]
    fn all_core_pica_format_values_are_accepted_for_2d_entries() {
        for format in TextureFormat::ALL {
            let dims = TextureDimensions::new(8, 8).unwrap();
            let size = dims.encoded_base_size(format) as usize;
            let data = synthetic_ctpk(format, 8, 8, 2, size);
            let parsed = parse_ctpk(&data).unwrap();
            assert_eq!(parsed.textures[0].encoded.as_ref().unwrap().format, format);
        }
    }

    #[test]
    fn cube_entries_are_inventory_visible_but_not_flattened_to_2d() {
        let data = synthetic_ctpk(TextureFormat::Rgba8, 8, 8, 0, 256);
        let parsed = parse_ctpk(&data).unwrap();
        let texture = &parsed.textures[0];
        assert_eq!(texture.texture_type, CtpkTextureType::CubeMap);
        assert!(texture.encoded.is_none());
        assert_eq!(texture.trailing_bytes, 256);
    }

    #[test]
    fn texture_extent_must_stay_inside_declared_data_section() {
        let mut data = synthetic_ctpk(TextureFormat::Rgba8, 8, 8, 2, 256);
        data[0x28..0x2c].copy_from_slice(&128u32.to_le_bytes());
        assert_eq!(
            parse_ctpk(&data),
            Err(CtpkError::InvalidTextureExtent { index: 0 })
        );
    }

    #[test]
    fn short_base_level_is_rejected_before_decode() {
        let data = synthetic_ctpk(TextureFormat::Rgba8, 8, 8, 2, 255);
        assert_eq!(
            parse_ctpk(&data),
            Err(CtpkError::TruncatedBaseLevel {
                index: 0,
                expected: 256,
                actual: 255,
            })
        );
    }

    #[test]
    fn invalid_magic_is_not_accepted_as_ctpk() {
        let data = vec![0u8; 0x80];
        assert_eq!(parse_ctpk(&data), Err(CtpkError::InvalidHeader));
    }
}
