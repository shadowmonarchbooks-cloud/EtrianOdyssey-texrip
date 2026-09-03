use crate::swizzle::{storage_coords, TILE_PIXELS};
use crate::{DecodedTexture, TextureError};
use eo_core::{TextureDimensions, TextureFormat};

const ETC_PIXELS_PER_BLOCK: u64 = 16;
const ETC_BLOCKS_PER_TILE: u64 = 4;
const ETC_COLOR_BYTES: usize = 8;
const ETC_ALPHA_BYTES: usize = 8;

const SELECTOR_ORDER: [u8; 16] = [0, 4, 1, 5, 8, 12, 9, 13, 2, 6, 3, 7, 10, 14, 11, 15];

const MODIFIERS: [[i16; 4]; 8] = [
    [2, 8, -2, -8],
    [5, 17, -5, -17],
    [9, 29, -9, -29],
    [13, 42, -13, -42],
    [18, 60, -18, -60],
    [24, 80, -24, -80],
    [33, 106, -33, -106],
    [47, 183, -47, -183],
];

pub fn decode_etc1(
    dimensions: TextureDimensions,
    format: TextureFormat,
    payload: &[u8],
) -> Result<DecodedTexture, TextureError> {
    let has_alpha = match format {
        TextureFormat::Etc1 => false,
        TextureFormat::Etc1A4 => true,
        _ => return Err(TextureError::UnsupportedFormat(format)),
    };

    let expected = dimensions.encoded_base_size(format);
    if (payload.len() as u64) < expected {
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
    let tile_count = storage_pixels / TILE_PIXELS;
    let block_bytes = if has_alpha {
        ETC_ALPHA_BYTES + ETC_COLOR_BYTES
    } else {
        ETC_COLOR_BYTES
    };

    for tile_index in 0..tile_count {
        for block_in_tile in 0..ETC_BLOCKS_PER_TILE {
            let block_index = tile_index
                .checked_mul(ETC_BLOCKS_PER_TILE)
                .and_then(|index| index.checked_add(block_in_tile))
                .ok_or(TextureError::EncodedSizeOverflow)?;
            let block_offset = block_index
                .checked_mul(block_bytes as u64)
                .ok_or(TextureError::EncodedSizeOverflow)?;
            let block_offset =
                usize::try_from(block_offset).map_err(|_| TextureError::EncodedSizeOverflow)?;
            let block_end = block_offset
                .checked_add(block_bytes)
                .ok_or(TextureError::EncodedSizeOverflow)?;
            let encoded = payload.get(block_offset..block_end).ok_or(
                TextureError::TruncatedPayload {
                    expected,
                    actual: payload.len() as u64,
                },
            )?;

            let (alpha, color_bytes) = if has_alpha {
                let alpha = u64::from_le_bytes(
                    encoded[..ETC_ALPHA_BYTES]
                        .try_into()
                        .map_err(|_| TextureError::InvalidData("invalid ETC1A4 alpha block".to_owned()))?,
                );
                (Some(alpha), &encoded[ETC_ALPHA_BYTES..])
            } else {
                (None, encoded)
            };
            let color = EtcColorBlock::parse(color_bytes)?;

            for local_sequence in 0..ETC_PIXELS_PER_BLOCK {
                let selector_index = SELECTOR_ORDER[local_sequence as usize];
                let mut pixel = color.decode(selector_index)?;
                pixel[3] = alpha
                    .map(|bits| expand4(((bits >> (u64::from(selector_index) * 4)) & 0xF) as u8))
                    .unwrap_or(0xFF);

                let storage_index = tile_index
                    .checked_mul(TILE_PIXELS)
                    .and_then(|index| {
                        index.checked_add(block_in_tile * ETC_PIXELS_PER_BLOCK + local_sequence)
                    })
                    .ok_or(TextureError::DecodedSizeOverflow)?;
                let (x, y) = storage_coords(storage_index, dimensions.storage_width)?;
                if x >= dimensions.visible_width || y >= dimensions.visible_height {
                    continue;
                }
                let output_offset = u64::from(y)
                    .checked_mul(u64::from(dimensions.visible_width))
                    .and_then(|row| row.checked_add(u64::from(x)))
                    .and_then(|pixel_index| pixel_index.checked_mul(4))
                    .ok_or(TextureError::DecodedSizeOverflow)?;
                let output_offset = usize::try_from(output_offset)
                    .map_err(|_| TextureError::DecodedSizeOverflow)?;
                rgba8[output_offset..output_offset + 4].copy_from_slice(&pixel);
            }
        }
    }

    let decoded = DecodedTexture {
        width: dimensions.visible_width,
        height: dimensions.visible_height,
        rgba8,
    };
    decoded.validate()?;
    Ok(decoded)
}

#[derive(Clone, Copy)]
struct EtcColorBlock {
    selector_lsb: u16,
    selector_msb: u16,
    flags: u8,
    blue: u8,
    green: u8,
    red: u8,
}

impl EtcColorBlock {
    fn parse(data: &[u8]) -> Result<Self, TextureError> {
        if data.len() != ETC_COLOR_BYTES {
            return Err(TextureError::InvalidData(
                "ETC1 color block must be exactly 8 bytes".to_owned(),
            ));
        }
        Ok(Self {
            selector_lsb: u16::from_le_bytes([data[0], data[1]]),
            selector_msb: u16::from_le_bytes([data[2], data[3]]),
            flags: data[4],
            blue: data[5],
            green: data[6],
            red: data[7],
        })
    }

    fn decode(self, selector_index: u8) -> Result<[u8; 4], TextureError> {
        let (base0, base1) = self.base_colors()?;
        let flip = self.flags & 0x01 != 0;
        let second_subblock = if flip {
            selector_index & 0x02 != 0
        } else {
            selector_index & 0x08 != 0
        };
        let table = if second_subblock {
            (self.flags >> 2) & 0x07
        } else {
            (self.flags >> 5) & 0x07
        };
        let selector = (((self.selector_msb >> selector_index) & 1) << 1)
            | ((self.selector_lsb >> selector_index) & 1);
        let base = if second_subblock { base1 } else { base0 };
        let modifier = MODIFIERS[usize::from(table)][usize::from(selector)];
        Ok([
            add_modifier(base[0], modifier),
            add_modifier(base[1], modifier),
            add_modifier(base[2], modifier),
            0xFF,
        ])
    }

    fn base_colors(self) -> Result<([u8; 3], [u8; 3]), TextureError> {
        if self.flags & 0x02 == 0 {
            return Ok((
                [
                    expand4(self.red >> 4),
                    expand4(self.green >> 4),
                    expand4(self.blue >> 4),
                ],
                [
                    expand4(self.red & 0x0F),
                    expand4(self.green & 0x0F),
                    expand4(self.blue & 0x0F),
                ],
            ));
        }

        let base0 = [self.red >> 3, self.green >> 3, self.blue >> 3];
        let delta = [
            sign3(self.red & 0x07),
            sign3(self.green & 0x07),
            sign3(self.blue & 0x07),
        ];
        let mut base1 = [0u8; 3];
        for channel in 0..3 {
            let value = i16::from(base0[channel]) + delta[channel];
            if !(0..=31).contains(&value) {
                return Err(TextureError::InvalidData(
                    "ETC1 differential endpoint is outside 5-bit range".to_owned(),
                ));
            }
            base1[channel] = value as u8;
        }
        Ok((
            [expand5(base0[0]), expand5(base0[1]), expand5(base0[2])],
            [expand5(base1[0]), expand5(base1[1]), expand5(base1[2])],
        ))
    }
}

const fn sign3(value: u8) -> i16 {
    if value & 0x04 != 0 {
        i16::from(value) - 8
    } else {
        i16::from(value)
    }
}

fn add_modifier(value: u8, modifier: i16) -> u8 {
    (i16::from(value) + modifier).clamp(0, 255) as u8
}

const fn expand4(value: u8) -> u8 {
    (value << 4) | value
}

const fn expand5(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_block(
        lsb: u16,
        msb: u16,
        flags: u8,
        red: u8,
        green: u8,
        blue: u8,
    ) -> [u8; 8] {
        let mut block = [0u8; 8];
        block[0..2].copy_from_slice(&lsb.to_le_bytes());
        block[2..4].copy_from_slice(&msb.to_le_bytes());
        block[4] = flags;
        block[5] = blue;
        block[6] = green;
        block[7] = red;
        block
    }

    fn pixel(image: &DecodedTexture, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * image.width + x) * 4) as usize;
        image.rgba8[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn individual_mode_blocks_fill_8x8_quadrants_in_morton_group_order() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let colors = [0x11u8, 0x33, 0x55, 0x77];
        let mut payload = Vec::new();
        for red in colors {
            payload.extend_from_slice(&color_block(0, 0, 0, red * 0x11, 0, 0));
        }
        let image = decode_etc1(dims, TextureFormat::Etc1, &payload).unwrap();
        assert_eq!(pixel(&image, 0, 0)[0], 0x13);
        assert_eq!(pixel(&image, 4, 0)[0], 0x35);
        assert_eq!(pixel(&image, 0, 4)[0], 0x57);
        assert_eq!(pixel(&image, 4, 4)[0], 0x79);
    }

    #[test]
    fn selector_bits_follow_etc_column_major_indices() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut payload = vec![0u8; 32];
        let block = color_block(1 << 4, 0, 0, 0x88, 0x44, 0x22);
        payload[0..8].copy_from_slice(&block);
        let image = decode_etc1(dims, TextureFormat::Etc1, &payload).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0x8A, 0x46, 0x24, 0xFF]);
        assert_eq!(pixel(&image, 1, 0), [0x90, 0x4C, 0x2A, 0xFF]);
    }

    #[test]
    fn etc1a4_alpha_nibbles_use_the_same_selector_index_mapping() {
        let dims = TextureDimensions::new(8, 8).unwrap();
        let mut payload = vec![0u8; 64];
        let alpha = 0x1u64 | (0xAu64 << 16);
        payload[0..8].copy_from_slice(&alpha.to_le_bytes());
        payload[8..16].copy_from_slice(&color_block(0, 0, 0, 0x88, 0x44, 0x22));
        let image = decode_etc1(dims, TextureFormat::Etc1A4, &payload).unwrap();
        assert_eq!(pixel(&image, 0, 0)[3], 0x11);
        assert_eq!(pixel(&image, 1, 0)[3], 0xAA);
    }

    #[test]
    fn differential_mode_expands_5bit_endpoints_and_rejects_invalid_delta() {
        let valid = EtcColorBlock::parse(&color_block(0, 0, 0x02, 0xF8, 0x80, 0x08)).unwrap();
        assert_eq!(valid.decode(0).unwrap(), [0xFF, 0x86, 0x0A, 0xFF]);

        let invalid = EtcColorBlock::parse(&color_block(0, 0, 0x02, 0xFC, 0x80, 0x08)).unwrap();
        assert!(matches!(
            invalid.decode(0),
            Err(TextureError::InvalidData(_))
        ));
    }

    #[test]
    fn non_aligned_visible_dimensions_are_cropped_after_full_tile_decode() {
        let dims = TextureDimensions::new(9, 9).unwrap();
        let block = color_block(0, 0, 0, 0x88, 0x44, 0x22);
        let mut payload = Vec::new();
        for _ in 0..16 {
            payload.extend_from_slice(&block);
        }
        assert_eq!(payload.len() as u64, dims.encoded_base_size(TextureFormat::Etc1));
        let image = decode_etc1(dims, TextureFormat::Etc1, &payload).unwrap();
        assert_eq!((image.width, image.height), (9, 9));
        assert_eq!(image.rgba8.len(), 9 * 9 * 4);
        assert_eq!(pixel(&image, 8, 8), [0x8A, 0x46, 0x24, 0xFF]);
    }
}
