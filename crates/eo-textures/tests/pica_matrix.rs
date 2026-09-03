use eo_core::{TextureDimensions, TextureFormat};
use eo_textures::{EncodedTexture, NativePicaDecoder, TextureDecoder, TextureError};

const DIMENSIONS: [(u32, u32); 7] = [
    (4, 4),
    (7, 7),
    (8, 8),
    (9, 9),
    (13, 17),
    (64, 64),
    (257, 129),
];

#[test]
fn every_pica_format_decodes_across_alignment_boundaries() {
    let decoder = NativePicaDecoder;
    for format in TextureFormat::ALL {
        for (width, height) in DIMENSIONS {
            let dimensions = TextureDimensions::new(width, height).unwrap();
            let size = dimensions.encoded_base_size(format);
            let texture = EncodedTexture {
                dimensions,
                format,
                mip_count: 1,
                payload: vec![0; size as usize],
            };
            let decoded = decoder.decode_base_level(&texture).unwrap_or_else(|error| {
                panic!("failed to decode {format:?} at {width}x{height}: {error}")
            });
            assert_eq!((decoded.width, decoded.height), (width, height));
            assert_eq!(decoded.rgba8.len() as u64, u64::from(width) * u64::from(height) * 4);
        }
    }
}

#[test]
fn every_format_rejects_one_byte_short_padded_payload() {
    let dimensions = TextureDimensions::new(13, 17).unwrap();
    for format in TextureFormat::ALL {
        let expected = dimensions.encoded_base_size(format);
        let texture = EncodedTexture {
            dimensions,
            format,
            mip_count: 1,
            payload: vec![0; (expected - 1) as usize],
        };
        assert_eq!(
            texture.validate_base_level(),
            Err(TextureError::TruncatedPayload {
                expected,
                actual: expected - 1,
            }),
            "format {format:?} did not enforce padded storage size"
        );
    }
}

#[test]
fn multi_mip_payloads_keep_each_level_tile_padded_and_independently_decodable() {
    let dimensions = TextureDimensions::new(257, 129).unwrap();
    let seed = EncodedTexture {
        dimensions,
        format: TextureFormat::Etc1A4,
        mip_count: 9,
        payload: Vec::new(),
    };
    let layouts = seed.mip_layouts().unwrap();
    assert_eq!(layouts.len(), 9);
    assert_eq!((layouts[0].dimensions.storage_width, layouts[0].dimensions.storage_height), (264, 136));
    assert_eq!((layouts[8].dimensions.visible_width, layouts[8].dimensions.visible_height), (1, 1));
    assert_eq!((layouts[8].dimensions.storage_width, layouts[8].dimensions.storage_height), (8, 8));

    let total = layouts.last().unwrap().offset + layouts.last().unwrap().size;
    let texture = EncodedTexture {
        payload: vec![0; total as usize],
        ..seed
    };
    texture.validate().unwrap();
    let decoder = NativePicaDecoder;
    for layout in layouts {
        let decoded = decoder.decode_level(&texture, layout.level).unwrap();
        assert_eq!(decoded.width, layout.dimensions.visible_width);
        assert_eq!(decoded.height, layout.dimensions.visible_height);
        assert_eq!(texture.level_payload(layout.level).unwrap().len() as u64, layout.size);
    }
}

#[test]
fn runtime_hash_span_is_exact_base_level_for_compressed_texture_with_mips() {
    let dimensions = TextureDimensions::new(13, 17).unwrap();
    let seed = EncodedTexture {
        dimensions,
        format: TextureFormat::Etc1,
        mip_count: 5,
        payload: Vec::new(),
    };
    let layouts = seed.mip_layouts().unwrap();
    let total = layouts.last().unwrap().offset + layouts.last().unwrap().size;
    let mut payload = vec![0xCC; total as usize + 37];
    payload[..layouts[0].size as usize].fill(0x5A);
    let texture = EncodedTexture { payload, ..seed };

    let hash_span = texture.runtime_hash_payload().unwrap();
    assert_eq!(hash_span.len() as u64, layouts[0].size);
    assert!(hash_span.iter().all(|byte| *byte == 0x5A));
    assert_ne!(hash_span.len(), texture.payload.len());
}
