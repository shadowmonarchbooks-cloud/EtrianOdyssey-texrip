use crate::swizzle::storage_coords;
use crate::{DecodedTexture, TextureError};
use eo_core::{TextureDimensions, TextureFormat};

pub fn decode_uncompressed(
    dimensions: TextureDimensions,
    format: TextureFormat,
    payload: &[u8],
) -> Result<DecodedTexture, TextureError> {
    if matches!(format, TextureFormat::Etc1 | TextureFormat::Etc1A4) {
        return Err(TextureError::UnsupportedFormat(format));
    }

    let expected = dimensions.encoded_base_size(format);
    if payload.len() as u64 < expected {
        return Err(TextureError::TruncatedPayload {
            expected,
            actual: payload.len() as u64,
        });
    }

    let output_len = u64::from(dimensions.visible_width)
        .checked_mul(u64::from(dimensions.visible_height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(TextureError::DecodedSizeOverflow)?;
    let mut rgba8 = vec![
        0u8;
        usize::try_from(output_len).map_err(|_| TextureError::DecodedSizeOverflow)?
    ];
    let storage_pixels = u64::from(dimensions.storage_width)
        .checked_mul(u64::from(dimensions.storage_height))
        .ok_or(TextureError::DecodedSizeOverflow)?;

    for index in 0..storage_pixels {
        let (x, y) = storage_coords(index, dimensions.storage_width)?;
        let pixel = decode_pixel(format, payload, index)?;
        if x >= dimensions.visible_width || y >= dimensions.visible_height {
            continue;
        }
        let output_pixel = u64::from(y)
            .checked_mul(u64::from(dimensions.visible_width))
            .and_then(|row| row.checked_add(u64::from(x)))
            .and_then(|pixel_index| pixel_index.checked_mul(4))
            .ok_or(TextureError::DecodedSizeOverflow)?;
        let output_pixel =
            usize::try_from(output_pixel).map_err(|_| TextureError::DecodedSizeOverflow)?;
        rgba8[output_pixel..output_pixel + 4].copy_from_slice(&pixel);
    }

    let decoded = DecodedTexture {
        width: dimensions.visible_width,
        height: dimensions.visible_height,
        rgba8,
    };
    decoded.validate()?;
    Ok(decoded)
}

fn decode_pixel(
    format: TextureFormat,
    payload: &[u8],
    pixel_index: u64,
) -> Result<[u8; 4], TextureError> {
    match format {
        TextureFormat::Rgba8 => {
            let data = pixel_bytes(payload, pixel_index, 4)?;
            Ok([data[3], data[2], data[1], data[0]])
        }
        TextureFormat::Rgb8 => {
            let data = pixel_bytes(payload, pixel_index, 3)?;
            Ok([data[2], data[1], data[0], 0xFF])
        }
        TextureFormat::Rgba5551 => {
            let value = u16::from_le_bytes(pixel_bytes(payload, pixel_index, 2)?.try_into().unwrap());
            let red = expand5(((value >> 11) & 0x1F) as u8);
            let green = expand5(((value >> 6) & 0x1F) as u8);
            let blue = expand5(((value >> 1) & 0x1F) as u8);
            let alpha = if value & 1 != 0 { 0xFF } else { 0 };
            Ok([red, green, blue, alpha])
        }
        TextureFormat::Rgb565 => {
            let value = u16::from_le_bytes(pixel_bytes(payload, pixel_index, 2)?.try_into().unwrap());
            Ok([
                expand5(((value >> 11) & 0x1F) as u8),
                expand6(((value >> 5) & 0x3F) as u8),
                expand5((value & 0x1F) as u8),
                0xFF,
            ])
        }
        TextureFormat::Rgba4 => {
            let value = u16::from_le_bytes(pixel_bytes(payload, pixel_index, 2)?.try_into().unwrap());
            Ok([
                expand4(((value >> 12) & 0x0F) as u8),
                expand4(((value >> 8) & 0x0F) as u8),
                expand4(((value >> 4) & 0x0F) as u8),
                expand4((value & 0x0F) as u8),
            ])
        }
        TextureFormat::La8 => {
            let data = pixel_bytes(payload, pixel_index, 2)?;
            Ok([data[1], data[1], data[1], data[0]])
        }
        TextureFormat::Hilo8 => {
            let data = pixel_bytes(payload, pixel_index, 2)?;
            Ok([data[1], data[0], 0xFF, 0xFF])
        }
        TextureFormat::L8 => {
            let value = pixel_bytes(payload, pixel_index, 1)?[0];
            Ok([value, value, value, 0xFF])
        }
        TextureFormat::A8 => {
            let alpha = pixel_bytes(payload, pixel_index, 1)?[0];
            Ok([0xFF, 0xFF, 0xFF, alpha])
        }
        TextureFormat::La4 => {
            let value = pixel_bytes(payload, pixel_index, 1)?[0];
            let luminance = expand4(value >> 4);
            let alpha = expand4(value & 0x0F);
            Ok([luminance, luminance, luminance, alpha])
        }
        TextureFormat::L4 => {
            let value = packed_nibble(payload, pixel_index)?;
            let luminance = expand4(value);
            Ok([luminance, luminance, luminance, 0xFF])
        }
        TextureFormat::A4 => {
            let alpha = expand4(packed_nibble(payload, pixel_index)?);
            Ok([0xFF, 0xFF, 0xFF, alpha])
        }
        TextureFormat::Etc1 | TextureFormat::Etc1A4 => Err(TextureError::UnsupportedFormat(format)),
    }
}

fn pixel_bytes(
    payload: &[u8],
    pixel_index: u64,
    bytes_per_pixel: u64,
) -> Result<&[u8], TextureError> {
    let offset = pixel_index
        .checked_mul(bytes_per_pixel)
        .ok_or(TextureError::DecodedSizeOverflow)?;
    let end = offset
        .checked_add(bytes_per_pixel)
        .ok_or(TextureError::DecodedSizeOverflow)?;
    let offset = usize::try_from(offset).map_err(|_| TextureError::DecodedSizeOverflow)?;
    let end = usize::try_from(end).map_err(|_| TextureError::DecodedSizeOverflow)?;
    payload
        .get(offset..end)
        .ok_or_else(|| TextureError::InvalidData("pixel extends beyond encoded payload".to_owned()))
}

fn packed_nibble(payload: &[u8], pixel_index: u64) -> Result<u8, TextureError> {
    let byte_index = usize::try_from(pixel_index / 2)
        .map_err(|_| TextureError::DecodedSizeOverflow)?;
    let value = *payload
        .get(byte_index)
        .ok_or_else(|| TextureError::InvalidData("nibble extends beyond encoded payload".to_owned()))?;
    let shift = ((pixel_index & 1) * 4) as u32;
    Ok((value >> shift) & 0x0F)
}

const fn expand4(value: u8) -> u8 {
    (value << 4) | value
}

const fn expand5(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

const fn expand6(value: u8) -> u8 {
    (value << 2) | (value >> 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &DecodedTexture, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * image.width + x) * 4) as usize;
        image.rgba8[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn rgba8_uses_morton_order_and_abgr_memory_bytes() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut payload = vec![0u8; dims.encoded_base_size(TextureFormat::Rgba8) as usize];
        payload[0..4].copy_from_slice(&[4, 3, 2, 1]);
        payload[4..8].copy_from_slice(&[8, 7, 6, 5]);
        payload[8..12].copy_from_slice(&[12, 11, 10, 9]);
        let image = decode_uncompressed(dims, TextureFormat::Rgba8, &payload).unwrap();
        assert_eq!(pixel(&image, 0, 0), [1, 2, 3, 4]);
        assert_eq!(pixel(&image, 1, 0), [5, 6, 7, 8]);
        assert_eq!(pixel(&image, 0, 1), [9, 10, 11, 12]);
    }

    #[test]
    fn padded_storage_pixels_are_consumed_but_cropped_from_output() {
        let dims = TextureDimensions::new(7, 7).unwrap();
        let mut payload = vec![0u8; dims.encoded_base_size(TextureFormat::A8) as usize];
        payload[0] = 0x11;
        payload[63] = 0xEE;
        let image = decode_uncompressed(dims, TextureFormat::A8, &payload).unwrap();
        assert_eq!((image.width, image.height), (7, 7));
        assert_eq!(image.rgba8.len(), 7 * 7 * 4);
        assert_eq!(pixel(&image, 0, 0), [0xFF, 0xFF, 0xFF, 0x11]);
    }

    #[test]
    fn packed_4bit_formats_use_low_nibble_first() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut payload = vec![0u8; dims.encoded_base_size(TextureFormat::L4) as usize];
        payload[0] = 0xA3;
        let image = decode_uncompressed(dims, TextureFormat::L4, &payload).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0x33, 0x33, 0x33, 0xFF]);
        assert_eq!(pixel(&image, 1, 0), [0xAA, 0xAA, 0xAA, 0xFF]);
    }

    #[test]
    fn packed_16bit_channel_order_matches_pica_layout() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut rgba5551 = vec![0u8; dims.encoded_base_size(TextureFormat::Rgba5551) as usize];
        let value = (31u16 << 11) | (16u16 << 6) | (1u16 << 1) | 1;
        rgba5551[0..2].copy_from_slice(&value.to_le_bytes());
        let image = decode_uncompressed(dims, TextureFormat::Rgba5551, &rgba5551).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0xFF, 0x84, 0x08, 0xFF]);

        let mut rgba4 = vec![0u8; dims.encoded_base_size(TextureFormat::Rgba4) as usize];
        rgba4[0..2].copy_from_slice(&0x1234u16.to_le_bytes());
        let image = decode_uncompressed(dims, TextureFormat::Rgba4, &rgba4).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn la_and_hilo_formats_preserve_documented_byte_roles() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut la8 = vec![0u8; dims.encoded_base_size(TextureFormat::La8) as usize];
        la8[0..2].copy_from_slice(&[0x40, 0x80]);
        let image = decode_uncompressed(dims, TextureFormat::La8, &la8).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0x80, 0x80, 0x80, 0x40]);

        let mut hilo = vec![0u8; dims.encoded_base_size(TextureFormat::Hilo8) as usize];
        hilo[0..2].copy_from_slice(&[0x22, 0x99]);
        let image = decode_uncompressed(dims, TextureFormat::Hilo8, &hilo).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0x99, 0x22, 0xFF, 0xFF]);
    }
}
