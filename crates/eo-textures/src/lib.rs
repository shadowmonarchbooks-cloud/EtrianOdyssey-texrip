//! PICA200 texture decoding contracts.
//!
//! 0.20 defines payload validation and decoder boundaries only. The native tiled
//! codec implementation lands in the texture-engine milestone.

use eo_core::{TextureDimensions, TextureFormat};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncodedTexture {
    pub dimensions: TextureDimensions,
    pub format: TextureFormat,
    pub mip_count: u8,
    pub payload: Vec<u8>,
}

impl EncodedTexture {
    pub fn validate_base_level(&self) -> Result<(), TextureError> {
        let expected = self.dimensions.encoded_base_size(self.format);
        let actual = self.payload.len() as u64;
        if actual < expected {
            return Err(TextureError::TruncatedPayload { expected, actual });
        }
        Ok(())
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
    #[error("decoded texture size overflow")]
    DecodedSizeOverflow,
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
    fn decoded_rgba_length_is_exact() {
        let image = DecodedTexture {
            width: 2,
            height: 2,
            rgba8: vec![0; 16],
        };
        assert_eq!(image.validate(), Ok(()));
    }
}
