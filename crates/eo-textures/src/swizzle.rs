use crate::TextureError;

pub const TILE_WIDTH: u32 = 8;
pub const TILE_HEIGHT: u32 = 8;
pub const TILE_PIXELS: u64 = 64;

/// Convert a sequential PICA texture pixel index to storage-space coordinates.
///
/// Tiles are row-major. Pixels inside each 8x8 tile use Morton/Z-order with
/// alternating X/Y bits: x0, y0, x1, y1, x2, y2.
pub fn storage_coords(index: u64, storage_width: u32) -> Result<(u32, u32), TextureError> {
    if storage_width == 0 || storage_width % TILE_WIDTH != 0 {
        return Err(TextureError::InvalidData(
            "PICA storage width must be a non-zero multiple of 8".to_owned(),
        ));
    }

    let tiles_per_row = u64::from(storage_width / TILE_WIDTH);
    let tile_index = index / TILE_PIXELS;
    let point = (index % TILE_PIXELS) as u8;
    let tile_x = tile_index % tiles_per_row;
    let tile_y = tile_index / tiles_per_row;
    let (local_x, local_y) = morton_xy(point);

    let x = tile_x
        .checked_mul(u64::from(TILE_WIDTH))
        .and_then(|base| base.checked_add(u64::from(local_x)))
        .ok_or(TextureError::DecodedSizeOverflow)?;
    let y = tile_y
        .checked_mul(u64::from(TILE_HEIGHT))
        .and_then(|base| base.checked_add(u64::from(local_y)))
        .ok_or(TextureError::DecodedSizeOverflow)?;
    Ok((
        u32::try_from(x).map_err(|_| TextureError::DecodedSizeOverflow)?,
        u32::try_from(y).map_err(|_| TextureError::DecodedSizeOverflow)?,
    ))
}

pub const fn morton_xy(point: u8) -> (u8, u8) {
    let x = (point & 0x01) | ((point >> 1) & 0x02) | ((point >> 2) & 0x04);
    let y = ((point >> 1) & 0x01) | ((point >> 2) & 0x02) | ((point >> 3) & 0x04);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morton_prefix_matches_documented_3ds_tile_order() {
        let expected = [
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 1),
            (2, 0),
            (3, 0),
            (2, 1),
            (3, 1),
            (0, 2),
            (1, 2),
            (0, 3),
            (1, 3),
            (2, 2),
            (3, 2),
            (2, 3),
            (3, 3),
            (4, 0),
        ];
        for (index, expected) in expected.into_iter().enumerate() {
            assert_eq!(storage_coords(index as u64, 8).unwrap(), expected);
        }
    }

    #[test]
    fn tiles_advance_row_major() {
        assert_eq!(storage_coords(64, 16).unwrap(), (8, 0));
        assert_eq!(storage_coords(128, 16).unwrap(), (0, 8));
    }
}
