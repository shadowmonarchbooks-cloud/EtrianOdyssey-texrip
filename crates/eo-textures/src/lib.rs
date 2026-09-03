//! Native PICA200 texture storage and decoding.
//!
//! 0.40 owns the raw hardware texture contract: padded 8x8 storage, Morton tile
//! traversal, exact mip byte ranges and format conversion to tightly packed RGBA8.
//! Container-specific transforms (for example a format choosing to present an
//! image vertically flipped) do not belong in this raw codec.

pub mod swizzle;
mod uncompressed;

use eo_core::{TextureDimensions, TextureFormat};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use uncompressed::decode_uncompressed;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncodedTexture {
    pub dimensions: TextureDimensions,
    pub format: TextureFormat,
    /// Total stored levels including level 0.
    pub mip_count: u8,
    /// Level 0 followed by progressively smaller mip levels. Container adapters
    /// must normalize other physical layouts before constructing this value.
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MipLevelLayout {
    pub level: u8,
    pub dimensions: TextureDimensions,
    pub offset: u64,
    pub size: u64,
}

impl EncodedTexture {
    pub fn max_mip_count(&self) -> u8 {
        max_mip_count(
            self.dimensions.visible_width,
            self.dimensions.visible_height,
        )
    }

    pub fn mip_layouts(&self) -> Result<Vec<MipLevelLayout>, TextureError> {
        let max = self.max_mip_count();
        if self.mip_count == 0 || self.mip_count > max {
            return Err(TextureError::InvalidMipCount {
                requested: self.mip_count,
                max,
            });
        }

        let mut layouts = Vec::with_capacity(usize::from(self.mip_count));
        let mut offset = 0u64;
        for level in 0..self.mip_count {
            let width = mip_extent(self.dimensions.visible_width, level);
            let height = mip_extent(self.dimensions.visible_height, level);
            let dimensions = TextureDimensions::new(width, height)
                .map_err(|error| TextureError::InvalidData(error.to_string()))?;
            let size = dimensions.encoded_base_size(self.format);
            layouts.push(MipLevelLayout {
                level,
                dimensions,
                offset,
                size,
            });
            offset = offset
                .checked_add(size)
                .ok_or(TextureError::EncodedSizeOverflow)?;
        }
        Ok(layouts)
    }

    pub fn expected_payload_size(&self) -> Result<u64, TextureError> {
        Ok(self
            .mip_layouts()?
            .last()
            .map_or(0, |layout| layout.offset + layout.size))
    }

    pub fn validate_base_level(&self) -> Result<(), TextureError> {
        let expected = self.dimensions.encoded_base_size(self.format);
        let actual = self.payload.len() as u64;
        if actual < expected {
            return Err(TextureError::TruncatedPayload { expected, actual });
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), TextureError> {
        let expected = self.expected_payload_size()?;
        let actual = self.payload.len() as u64;
        if actual < expected {
            return Err(TextureError::TruncatedPayload { expected, actual });
        }
        Ok(())
    }

    pub fn level_layout(&self, level: u8) -> Result<MipLevelLayout, TextureError> {
        self.mip_layouts()?
            .into_iter()
            .find(|layout| layout.level == level)
            .ok_or(TextureError::InvalidMipLevel {
                requested: level,
                count: self.mip_count,
            })
    }

    pub fn level_payload(&self, level: u8) -> Result<&[u8], TextureError> {
        let layout = self.level_layout(level)?;
        let end = layout
            .offset
            .checked_add(layout.size)
            .ok_or(TextureError::EncodedSizeOverflow)?;
        let start = usize::try_from(layout.offset).map_err(|_| TextureError::EncodedSizeOverflow)?;
        let end = usize::try_from(end).map_err(|_| TextureError::EncodedSizeOverflow)?;
        self.payload
            .get(start..end)
            .ok_or(TextureError::TruncatedPayload {
                expected: layout.offset + layout.size,
                actual: self.payload.len() as u64,
            })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecodedTexture {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8 pixels in row-major display order.
    pub rgba8: Vec<u8>,
}

impl DecodedTexture {
    pub fn validate(&self) -> Result<(), TextureError> {
        let expected = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(TextureError::DecodedSizeOverflow)?;
        if self.rgba8.len() as u64 != expected {
            return Err(TextureError::InvalidDecodedLength {
                expected,
                actual: self.rgba8.len() as u64,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TextureError {
    #[error("encoded texture payload is truncated: expected at least {expected} bytes, got {actual}")]
    TruncatedPayload { expected: u64, actual: u64 },
    #[error("encoded texture size overflow")]
    EncodedSizeOverflow,
    #[error("decoded texture size overflow")]
    DecodedSizeOverflow,
    #[error("invalid mip count {requested}; maximum for this texture is {max}")]
    InvalidMipCount { requested: u8, max: u8 },
    #[error("mip level {requested} is outside stored level count {count}")]
    InvalidMipLevel { requested: u8, count: u8 },
    #[error("decoded RGBA length mismatch: expected {expected} bytes, got {actual}")]
    InvalidDecodedLength { expected: u64, actual: u64 },
    #[error("texture format is not implemented by this decoder: {0:?}")]
    UnsupportedFormat(TextureFormat),
    #[error("texture decoder rejected malformed data: {0}")]
    InvalidData(String),
}

pub trait TextureDecoder {
    fn decode_base_level(&self, texture: &EncodedTexture) -> Result<DecodedTexture, TextureError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativePicaDecoder;

impl TextureDecoder for NativePicaDecoder {
    fn decode_base_level(&self, texture: &EncodedTexture) -> Result<DecodedTexture, TextureError> {
        texture.validate_base_level()?;
        let payload = texture.level_payload(0)?;
        match texture.format {
            TextureFormat::Etc1 | TextureFormat::Etc1A4 => {
                Err(TextureError::UnsupportedFormat(texture.format))
            }
            _ => decode_uncompressed(texture.dimensions, texture.format, payload),
        }
    }
}

pub fn max_mip_count(width: u32, height: u32) -> u8 {
    let mut extent = width.max(height).max(1);
    let mut count = 1u8;
    while extent > 1 {
        extent >>= 1;
        count += 1;
    }
    count
}

fn mip_extent(base: u32, level: u8) -> u32 {
    (base >> u32::from(level)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_level_validation_uses_padded_storage_size() {
        let dims = TextureDimensions::new(13, 17).unwrap();
        let texture = EncodedTexture {
            dimensions: dims,
            format: TextureFormat::Etc1,
            mip_count: 1,
            payload: vec![0; 191],
        };
        assert_eq!(
            texture.validate_base_level(),
            Err(TextureError::TruncatedPayload {
                expected: 192,
                actual: 191,
            })
        );
    }

    #[test]
    fn mip_layout_keeps_sub_8_levels_in_8x8_storage_tiles() {
        let dims = TextureDimensions::new(13, 17).unwrap();
        let texture = EncodedTexture {
            dimensions: dims,
            format: TextureFormat::Rgba8,
            mip_count: 5,
            payload: vec![0; 2560],
        };
        let layouts = texture.mip_layouts().unwrap();
        assert_eq!(texture.max_mip_count(), 5);
        assert_eq!(layouts[0].dimensions.visible_width, 13);
        assert_eq!(layouts[0].dimensions.visible_height, 17);
        assert_eq!(layouts[0].size, 1536);
        assert_eq!(layouts[1].dimensions.visible_width, 6);
        assert_eq!(layouts[1].dimensions.visible_height, 8);
        assert_eq!(layouts[1].size, 256);
        assert_eq!(layouts[4].dimensions.visible_width, 1);
        assert_eq!(layouts[4].dimensions.visible_height, 1);
        assert_eq!(layouts[4].size, 256);
        assert_eq!(layouts[4].offset, 2304);
        assert_eq!(texture.expected_payload_size().unwrap(), 2560);
    }

    #[test]
    fn exact_level_slice_excludes_following_mips_and_trailing_alignment() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut payload = vec![0xCC; 300];
        payload[..256].fill(0x11);
        let texture = EncodedTexture {
            dimensions: dims,
            format: TextureFormat::Rgba8,
            mip_count: 1,
            payload,
        };
        let level = texture.level_payload(0).unwrap();
        assert_eq!(level.len(), 256);
        assert!(level.iter().all(|byte| *byte == 0x11));
    }

    #[test]
    fn invalid_mip_counts_are_rejected() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let zero = EncodedTexture {
            dimensions: dims,
            format: TextureFormat::A8,
            mip_count: 0,
            payload: Vec::new(),
        };
        assert_eq!(
            zero.mip_layouts(),
            Err(TextureError::InvalidMipCount {
                requested: 0,
                max: 4
            })
        );
        let too_many = EncodedTexture {
            dimensions: dims,
            format: TextureFormat::A8,
            mip_count: 5,
            payload: vec![0; 320],
        };
        assert!(matches!(
            too_many.validate(),
            Err(TextureError::InvalidMipCount { .. })
        ));
    }

    #[test]
    fn native_decoder_handles_uncompressed_base_level() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut payload = vec![0u8; 256];
        payload[0..4].copy_from_slice(&[0x44, 0x33, 0x22, 0x11]);
        let texture = EncodedTexture {
            dimensions: dims,
            format: TextureFormat::Rgba8,
            mip_count: 1,
            payload,
        };
        let decoded = NativePicaDecoder.decode_base_level(&texture).unwrap();
        assert_eq!(&decoded.rgba8[..4], &[0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn decoded_rgba_length_is_exact() {
        let image = DecodedTexture {
            width: 2,
            height: 2,
            rgba8: vec![0; 16],
        };
        assert_eq!(image.validate(), Ok(()));
    }
}
