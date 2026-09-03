use crate::EncodedTexture;
use encoding_rs::SHIFT_JIS;
use eo_core::{TextureDimensions, TextureFormat};
use thiserror::Error;

const CGFX_HEADER_MIN: usize = 0x14;
const CGFX_DECLARED_MIN: u32 = 0x20;
const TXOB_IMAGE_TYPE: u32 = 0x2000_0011;
const TXOB_OBJECT_MIN: usize = 0x4c;
const IMAGE_OBJECT_MIN: usize = 0x20;
const MAX_SELF_STRING_BYTES: usize = 512;
const MAX_EMBEDDED_CGFX: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CgfxTexture {
    pub encoded: EncodedTexture,
    pub name: String,
    pub declared_mip_count: u32,
    pub storage_data_size: u32,
    pub txob_offset: u64,
    pub image_object_offset: u64,
    pub data_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CgfxContainer {
    pub declared_size: u32,
    pub textures: Vec<CgfxTexture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedCgfx {
    pub offset: u64,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtbcContainer {
    pub cgfx_payloads: Vec<EmbeddedCgfx>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CgfxError {
    #[error("CGFX header is invalid")]
    InvalidHeader,
    #[error("CGFX declared extent exceeds the source")]
    TruncatedContainer,
    #[error("ATBC wrapper header is invalid")]
    InvalidAtbcHeader,
}

pub fn is_cgfx(data: &[u8]) -> bool {
    validate_cgfx_header(data).is_ok()
}

pub fn parse_cgfx(data: &[u8]) -> Result<CgfxContainer, CgfxError> {
    let declared_size = validate_cgfx_header(data)?;
    let end = declared_size as usize;
    let payload = data.get(..end).ok_or(CgfxError::TruncatedContainer)?;
    let mut textures = Vec::new();
    let mut search = 0usize;

    while search + 4 <= payload.len() {
        let Some(relative) = find_magic(&payload[search..], b"TXOB") else {
            break;
        };
        let signature = search + relative;
        search = signature.saturating_add(4);
        let Some(object) = signature.checked_sub(4) else {
            continue;
        };
        if object
            .checked_add(TXOB_OBJECT_MIN)
            .is_none_or(|end| end > payload.len())
        {
            continue;
        }
        if read_u32(payload, object) != Some(TXOB_IMAGE_TYPE) {
            continue;
        }
        if let Some(texture) = parse_image_texture(payload, object, textures.len()) {
            textures.push(texture);
        }
    }

    Ok(CgfxContainer {
        declared_size,
        textures,
    })
}

pub fn parse_atbc(data: &[u8]) -> Result<AtbcContainer, CgfxError> {
    if data.len() < 4 || data.get(..4) != Some(b"ATBC") {
        return Err(CgfxError::InvalidAtbcHeader);
    }

    let mut cgfx_payloads = Vec::new();
    let mut search = 0usize;
    while search + 4 <= data.len() && cgfx_payloads.len() < MAX_EMBEDDED_CGFX {
        let Some(relative) = find_magic(&data[search..], b"CGFX") else {
            break;
        };
        let offset = search + relative;
        search = offset.saturating_add(4);
        let candidate = &data[offset..];
        let Ok(declared_size) = validate_cgfx_header(candidate) else {
            continue;
        };
        cgfx_payloads.push(EmbeddedCgfx {
            offset: offset as u64,
            size: u64::from(declared_size),
        });
    }

    Ok(AtbcContainer { cgfx_payloads })
}

impl AtbcContainer {
    pub fn covers_remainder(&self, source_len: u64, index: usize) -> Option<bool> {
        self.cgfx_payloads.get(index).map(|payload| {
            payload
                .offset
                .checked_add(payload.size)
                .is_some_and(|end| end == source_len)
        })
    }

    pub fn parse_payload(
        &self,
        data: &[u8],
        index: usize,
    ) -> Result<CgfxContainer, CgfxError> {
        let payload = self
            .cgfx_payloads
            .get(index)
            .ok_or(CgfxError::InvalidAtbcHeader)?;
        let start = usize::try_from(payload.offset).map_err(|_| CgfxError::InvalidAtbcHeader)?;
        let size = usize::try_from(payload.size).map_err(|_| CgfxError::InvalidAtbcHeader)?;
        let end = start
            .checked_add(size)
            .ok_or(CgfxError::InvalidAtbcHeader)?;
        parse_cgfx(
            data.get(start..end)
                .ok_or(CgfxError::TruncatedContainer)?,
        )
    }
}

fn validate_cgfx_header(data: &[u8]) -> Result<u32, CgfxError> {
    if data.len() < CGFX_HEADER_MIN || data.get(..4) != Some(b"CGFX") {
        return Err(CgfxError::InvalidHeader);
    }
    if data.get(4..6) != Some(&[0xff, 0xfe]) {
        return Err(CgfxError::InvalidHeader);
    }
    let header_size = read_u16(data, 6).ok_or(CgfxError::InvalidHeader)?;
    let declared_size = read_u32(data, 0x0c).ok_or(CgfxError::InvalidHeader)?;
    if usize::from(header_size) < CGFX_HEADER_MIN || declared_size < CGFX_DECLARED_MIN {
        return Err(CgfxError::InvalidHeader);
    }
    if usize::try_from(declared_size)
        .ok()
        .is_none_or(|declared| declared > data.len())
    {
        return Err(CgfxError::TruncatedContainer);
    }
    Ok(declared_size)
}

fn parse_image_texture(data: &[u8], object: usize, index: usize) -> Option<CgfxTexture> {
    let height = read_u32(data, object + 0x18)?;
    let width = read_u32(data, object + 0x1c)?;
    let declared_mip_count = read_u32(data, object + 0x28)?;
    let format_id = read_u32(data, object + 0x34)?;
    let format = u8::try_from(format_id)
        .ok()
        .and_then(|value| TextureFormat::try_from(value).ok())?;
    let dimensions = TextureDimensions::new(width, height).ok()?;

    let image_field = object.checked_add(0x38)?;
    let image_relative = usize::try_from(read_u32(data, image_field)?).ok()?;
    if image_relative == 0 {
        return None;
    }
    let image_object = image_field.checked_add(image_relative)?;
    if image_object
        .checked_add(IMAGE_OBJECT_MIN)
        .is_none_or(|end| end > data.len())
    {
        return None;
    }

    let image_height = read_u32(data, image_object)?;
    let image_width = read_u32(data, image_object + 4)?;
    if !matches_dimension(image_width, width) || !matches_dimension(image_height, height) {
        return None;
    }
    let storage_data_size = read_u32(data, image_object + 8)?;
    if storage_data_size == 0 {
        return None;
    }
    let data_field = image_object.checked_add(0x0c)?;
    let data_relative = usize::try_from(read_u32(data, data_field)?).ok()?;
    let data_offset = data_field.checked_add(data_relative)?;
    let storage_end = data_offset.checked_add(storage_data_size as usize)?;
    if storage_end > data.len() {
        return None;
    }

    let base_size = usize::try_from(dimensions.encoded_base_size(format)).ok()?;
    if base_size == 0 || base_size > storage_data_size as usize {
        return None;
    }
    let base_end = data_offset.checked_add(base_size)?;
    let payload = data.get(data_offset..base_end)?.to_vec();
    let name = read_self_string(data, object + 0x0c)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("cgfx_tex_{index:04}"));

    Some(CgfxTexture {
        encoded: EncodedTexture {
            dimensions,
            format,
            mip_count: 1,
            payload,
        },
        name,
        declared_mip_count: declared_mip_count.max(1),
        storage_data_size,
        txob_offset: object as u64,
        image_object_offset: image_object as u64,
        data_offset: data_offset as u64,
    })
}

fn matches_dimension(stored: u32, expected: u32) -> bool {
    stored == 0 || stored == expected
}

fn read_self_string(data: &[u8], field: usize) -> Option<String> {
    let relative = usize::try_from(read_u32(data, field)?).ok()?;
    if relative == 0 {
        return None;
    }
    let start = field.checked_add(relative)?;
    if start >= data.len() {
        return None;
    }
    let limit = data.len().min(start.saturating_add(MAX_SELF_STRING_BYTES));
    let raw = data.get(start..limit)?;
    let end = raw.iter().position(|byte| *byte == 0)?;
    let bytes = &raw[..end];
    if let Ok(value) = std::str::from_utf8(bytes) {
        return Some(value.to_owned());
    }
    let (decoded, _) = SHIFT_JIS.decode_without_bom_handling(bytes);
    Some(decoded.into_owned())
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

    fn synthetic_cgfx() -> Vec<u8> {
        let mut data = vec![0u8; 0x240];
        data[0..4].copy_from_slice(b"CGFX");
        data[4..6].copy_from_slice(&[0xff, 0xfe]);
        data[6..8].copy_from_slice(&0x14u16.to_le_bytes());
        data[0x0c..0x10].copy_from_slice(&(data.len() as u32).to_le_bytes());

        let object = 0x40usize;
        data[object..object + 4].copy_from_slice(&TXOB_IMAGE_TYPE.to_le_bytes());
        data[object + 4..object + 8].copy_from_slice(b"TXOB");
        data[object + 0x0c..object + 0x10].copy_from_slice(&0x44u32.to_le_bytes());
        data[object + 0x18..object + 0x1c].copy_from_slice(&8u32.to_le_bytes());
        data[object + 0x1c..object + 0x20].copy_from_slice(&8u32.to_le_bytes());
        data[object + 0x28..object + 0x2c].copy_from_slice(&2u32.to_le_bytes());
        data[object + 0x34..object + 0x38]
            .copy_from_slice(&(TextureFormat::Rgba8 as u32).to_le_bytes());
        data[object + 0x38..object + 0x3c].copy_from_slice(&0x58u32.to_le_bytes());
        data[0x90..0x99].copy_from_slice(b"body_tex\0");

        let image = 0xd0usize;
        data[image..image + 4].copy_from_slice(&8u32.to_le_bytes());
        data[image + 4..image + 8].copy_from_slice(&8u32.to_le_bytes());
        data[image + 8..image + 0x0c].copy_from_slice(&0x120u32.to_le_bytes());
        data[image + 0x0c..image + 0x10].copy_from_slice(&0x24u32.to_le_bytes());
        let pixels = 0x100usize;
        data[pixels..pixels + 4].copy_from_slice(&[4, 3, 2, 1]);
        data
    }

    #[test]
    fn direct_cgfx_emits_exact_native_encoded_texture() {
        let data = synthetic_cgfx();
        let parsed = parse_cgfx(&data).unwrap();
        assert_eq!(parsed.declared_size as usize, data.len());
        assert_eq!(parsed.textures.len(), 1);
        let texture = &parsed.textures[0];
        assert_eq!(texture.name, "body_tex");
        assert_eq!(texture.declared_mip_count, 2);
        assert_eq!(texture.storage_data_size, 0x120);
        assert_eq!(texture.encoded.mip_count, 1);
        assert_eq!(texture.encoded.payload.len(), 256);
        let decoded = NativePicaDecoder
            .decode_base_level(&texture.encoded)
            .unwrap();
        assert_eq!(&decoded.rgba8[..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn embedded_atbc_cgfx_is_found_structurally_not_at_fixed_offset() {
        let cgfx = synthetic_cgfx();
        let offset = 0x123usize;
        let mut atbc = vec![0u8; offset + cgfx.len()];
        atbc[..4].copy_from_slice(b"ATBC");
        atbc[offset..].copy_from_slice(&cgfx);
        let wrapper = parse_atbc(&atbc).unwrap();
        assert_eq!(wrapper.cgfx_payloads.len(), 1);
        assert_eq!(wrapper.cgfx_payloads[0].offset, offset as u64);
        assert_eq!(wrapper.covers_remainder(atbc.len() as u64, 0), Some(true));
        assert_eq!(wrapper.parse_payload(&atbc, 0).unwrap().textures.len(), 1);
    }

    #[test]
    fn false_cgfx_magic_inside_atbc_is_ignored() {
        let mut atbc = vec![0u8; 0x80];
        atbc[..4].copy_from_slice(b"ATBC");
        atbc[0x20..0x24].copy_from_slice(b"CGFX");
        let wrapper = parse_atbc(&atbc).unwrap();
        assert!(wrapper.cgfx_payloads.is_empty());
    }

    #[test]
    fn truncated_declared_extent_is_rejected() {
        let mut data = synthetic_cgfx();
        let declared = data.len() as u32 + 1;
        data[0x0c..0x10].copy_from_slice(&declared.to_le_bytes());
        assert_eq!(parse_cgfx(&data), Err(CgfxError::TruncatedContainer));
    }

    #[test]
    fn non_image_reference_txob_is_not_misread_as_pixels() {
        let mut data = synthetic_cgfx();
        data[0x40..0x44].copy_from_slice(&0x2000_0004u32.to_le_bytes());
        assert!(parse_cgfx(&data).unwrap().textures.is_empty());
    }
}
