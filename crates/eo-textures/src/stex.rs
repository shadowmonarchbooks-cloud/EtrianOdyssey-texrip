use crate::EncodedTexture;
use eo_core::{TextureDimensions, TextureFormat};
use thiserror::Error;

const STEX_HEADER_MIN: usize = 0x24;
const NORMAL_IMAGE_OFFSET: u32 = 0x80;
const LEGACY_IMAGE_OFFSET: u32 = 0x20;
const NAME_OFFSET: usize = 0x28;

const DT_UNSIGNED_BYTE: u32 = 0x1401;
const DT_UNSIGNED_BYTE_44_DMP: u32 = 0x6760;
const DT_UNSIGNED_4BITS_DMP: u32 = 0x6761;
const DT_UNSIGNED_SHORT_4444: u32 = 0x8033;
const DT_UNSIGNED_SHORT_5551: u32 = 0x8034;
const DT_UNSIGNED_SHORT_565: u32 = 0x8363;

const PF_RGBA: u32 = 0x6752;
const PF_RGB: u32 = 0x6754;
const PF_ALPHA: u32 = 0x6756;
const PF_LUMINANCE: u32 = 0x6757;
const PF_LUMINANCE_ALPHA: u32 = 0x6758;
const PF_ETC1: u32 = 0x675a;
const PF_ETC1A4: u32 = 0x675b;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StexTexture {
    pub encoded: EncodedTexture,
    pub data_type: u32,
    pub pixel_format: u32,
    pub declared_size: u32,
    pub data_offset: u32,
    pub name: Option<String>,
    pub trailing_bytes: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StexError {
    #[error("not an STEX texture")]
    InvalidHeader,
    #[error("STEX dimensions are invalid: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("unsupported STEX format pair data_type=0x{data_type:04x}, pixel_format=0x{pixel_format:04x}")]
    UnsupportedFormatPair { data_type: u32, pixel_format: u32 },
    #[error("STEX data offset is invalid: 0x{0:x}")]
    InvalidDataOffset(u32),
    #[error("STEX base level is truncated: expected {expected} bytes, found {actual}")]
    TruncatedBaseLevel { expected: u64, actual: u64 },
}

pub fn is_stex(data: &[u8]) -> bool {
    data.len() >= STEX_HEADER_MIN && data.get(..4) == Some(b"STEX")
}

pub fn parse_stex(data: &[u8]) -> Result<StexTexture, StexError> {
    if !is_stex(data) {
        return Err(StexError::InvalidHeader);
    }

    let width = read_u32(data, 0x0c)?;
    let height = read_u32(data, 0x10)?;
    let data_type = read_u32(data, 0x14)?;
    let pixel_format = read_u32(data, 0x18)?;
    let declared_size = read_u32(data, 0x1c)?;
    let image_offset = read_u32(data, 0x20)?;
    let dimensions = TextureDimensions::new(width, height)
        .map_err(|_| StexError::InvalidDimensions { width, height })?;
    let format = format_pair(data_type, pixel_format).ok_or(StexError::UnsupportedFormatPair {
        data_type,
        pixel_format,
    })?;

    let data_len = data.len() as u64;
    let declared_end = u64::from(image_offset).checked_add(u64::from(declared_size));
    let data_offset = if image_offset == NORMAL_IMAGE_OFFSET
        || (image_offset > 0 && declared_end == Some(data_len))
        || (image_offset >= STEX_HEADER_MIN as u32
            && u64::from(image_offset) < data_len
            && declared_size > 0
            && declared_end.is_some_and(|end| end <= data_len))
    {
        image_offset
    } else {
        LEGACY_IMAGE_OFFSET
    };
    if u64::from(data_offset) >= data_len {
        return Err(StexError::InvalidDataOffset(data_offset));
    }

    let base_size = dimensions.encoded_base_size(format);
    let available = data_len - u64::from(data_offset);
    if available < base_size {
        return Err(StexError::TruncatedBaseLevel {
            expected: base_size,
            actual: available,
        });
    }
    let start = data_offset as usize;
    let end = start
        .checked_add(base_size as usize)
        .ok_or(StexError::TruncatedBaseLevel {
            expected: base_size,
            actual: available,
        })?;
    let payload = data
        .get(start..end)
        .ok_or(StexError::TruncatedBaseLevel {
            expected: base_size,
            actual: available,
        })?
        .to_vec();
    let name = if start > NAME_OFFSET {
        let raw = &data[NAME_OFFSET..start];
        let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
        let text = String::from_utf8_lossy(&raw[..end]).trim().to_owned();
        (!text.is_empty()).then_some(text)
    } else {
        None
    };

    Ok(StexTexture {
        encoded: EncodedTexture {
            dimensions,
            format,
            mip_count: 1,
            payload,
        },
        data_type,
        pixel_format,
        declared_size,
        data_offset,
        name,
        trailing_bytes: available - base_size,
    })
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, StexError> {
    data.get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(StexError::InvalidHeader)
}

const fn format_pair(data_type: u32, pixel_format: u32) -> Option<TextureFormat> {
    match (data_type, pixel_format) {
        (DT_UNSIGNED_SHORT_4444, PF_RGBA) => Some(TextureFormat::Rgba4),
        (DT_UNSIGNED_SHORT_5551, PF_RGBA) => Some(TextureFormat::Rgba5551),
        (DT_UNSIGNED_BYTE, PF_RGBA) => Some(TextureFormat::Rgba8),
        (DT_UNSIGNED_SHORT_565, PF_RGB) => Some(TextureFormat::Rgb565),
        (DT_UNSIGNED_BYTE, PF_RGB) => Some(TextureFormat::Rgb8),
        (DT_UNSIGNED_BYTE, PF_ETC1) => Some(TextureFormat::Etc1),
        (DT_UNSIGNED_BYTE, PF_ETC1A4) => Some(TextureFormat::Etc1A4),
        (DT_UNSIGNED_BYTE, PF_ALPHA) => Some(TextureFormat::A8),
        (DT_UNSIGNED_4BITS_DMP, PF_ALPHA) => Some(TextureFormat::A4),
        (DT_UNSIGNED_BYTE, PF_LUMINANCE) => Some(TextureFormat::L8),
        (DT_UNSIGNED_4BITS_DMP, PF_LUMINANCE) => Some(TextureFormat::L4),
        (DT_UNSIGNED_BYTE, PF_LUMINANCE_ALPHA) => Some(TextureFormat::La8),
        (DT_UNSIGNED_BYTE_44_DMP, PF_LUMINANCE_ALPHA) => Some(TextureFormat::La4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NativePicaDecoder, TextureDecoder};

    fn synthetic_stex(
        width: u32,
        height: u32,
        data_type: u32,
        pixel_format: u32,
        image_offset: u32,
        declared_size: u32,
        payload_size: usize,
    ) -> Vec<u8> {
        let total = usize::try_from(image_offset).unwrap().saturating_add(payload_size);
        let mut data = vec![0u8; total.max(STEX_HEADER_MIN)];
        data[0..4].copy_from_slice(b"STEX");
        data[0x0c..0x10].copy_from_slice(&width.to_le_bytes());
        data[0x10..0x14].copy_from_slice(&height.to_le_bytes());
        data[0x14..0x18].copy_from_slice(&data_type.to_le_bytes());
        data[0x18..0x1c].copy_from_slice(&pixel_format.to_le_bytes());
        data[0x1c..0x20].copy_from_slice(&declared_size.to_le_bytes());
        data[0x20..0x24].copy_from_slice(&image_offset.to_le_bytes());
        data
    }

    #[test]
    fn format_pair_distinguishes_rgba4444_from_rgba8() {
        let data = synthetic_stex(8, 8, DT_UNSIGNED_SHORT_4444, PF_RGBA, 0x80, 128, 128);
        let parsed = parse_stex(&data).unwrap();
        assert_eq!(parsed.encoded.format, TextureFormat::Rgba4);
        assert_eq!(parsed.encoded.payload.len(), 128);
    }

    #[test]
    fn non_aligned_dimensions_use_padded_pica_base_span() {
        let data = synthetic_stex(7, 7, DT_UNSIGNED_BYTE, PF_RGBA, 0x80, 256, 256);
        let parsed = parse_stex(&data).unwrap();
        assert_eq!(parsed.encoded.dimensions.storage_width, 8);
        assert_eq!(parsed.encoded.dimensions.storage_height, 8);
        assert_eq!(parsed.encoded.payload.len(), 256);
    }

    #[test]
    fn declared_size_may_overshoot_when_base_level_is_physically_present() {
        let data = synthetic_stex(8, 8, DT_UNSIGNED_BYTE, PF_ALPHA, 0x80, 0x1000, 64);
        let parsed = parse_stex(&data).unwrap();
        assert_eq!(parsed.encoded.payload.len(), 64);
        assert_eq!(parsed.declared_size, 0x1000);
    }

    #[test]
    fn exact_base_span_excludes_trailing_mip_or_alignment_bytes() {
        let mut data = synthetic_stex(8, 8, DT_UNSIGNED_BYTE, PF_ALPHA, 0x80, 96, 96);
        data[0x80..0xc0].fill(0x11);
        data[0xc0..0xe0].fill(0xcc);
        let parsed = parse_stex(&data).unwrap();
        assert_eq!(parsed.encoded.payload, vec![0x11; 64]);
        assert_eq!(parsed.trailing_bytes, 32);
    }

    #[test]
    fn parser_output_decodes_directly_through_native_pica_engine() {
        let mut data = synthetic_stex(8, 8, DT_UNSIGNED_BYTE, PF_RGBA, 0x80, 256, 256);
        data[0x80..0x84].copy_from_slice(&[4, 3, 2, 1]);
        let parsed = parse_stex(&data).unwrap();
        let decoded = NativePicaDecoder.decode_base_level(&parsed.encoded).unwrap();
        assert_eq!(&decoded.rgba8[..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn unsupported_format_pair_is_not_guessed() {
        let data = synthetic_stex(8, 8, DT_UNSIGNED_SHORT_565, PF_RGBA, 0x80, 128, 128);
        assert!(matches!(
            parse_stex(&data),
            Err(StexError::UnsupportedFormatPair { .. })
        ));
    }
}
