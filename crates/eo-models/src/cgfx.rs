use crate::{MaterialRecord, ModelError, ModelInspector, ModelInventory, TextureReference};
use encoding_rs::SHIFT_JIS;
use eo_core::TextureRole;

const CGFX_HEADER_MIN: usize = 0x14;
const CGFX_DECLARED_MIN: u32 = 0x20;
const MTOB_TYPE: u32 = 0x0800_0000;
const MTOB_MIN: usize = 0x28c;
const TEXINFO_TYPE: u32 = 0x8000_0000;
const REFERENCE_TXOB_TYPE: u32 = 0x2000_0004;
const MAX_SELF_STRING_BYTES: usize = 512;
const MAX_EMBEDDED_CGFX: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CgfxModelInspector;

impl ModelInspector for CgfxModelInspector {
    fn probe(&self, data: &[u8]) -> bool {
        locate_cgfx_payload(data).is_some()
    }

    fn inspect(&self, data: &[u8]) -> Result<ModelInventory, ModelError> {
        let payload = locate_cgfx_payload(data).ok_or(ModelError::InvalidHeader)?;
        inspect_cgfx_payload(payload)
    }
}

fn inspect_cgfx_payload(data: &[u8]) -> Result<ModelInventory, ModelError> {
    let declared_size = validate_cgfx_header(data)?;
    let payload = data
        .get(..declared_size)
        .ok_or(ModelError::InvalidOffset)?;
    let model_name = first_cmdl_name(payload);
    let mut materials = Vec::new();
    let mut search = 0usize;

    while search + 4 <= payload.len() {
        let Some(relative) = find_magic(&payload[search..], b"MTOB") else {
            break;
        };
        let signature = search + relative;
        search = signature.saturating_add(4);
        let Some(object) = signature.checked_sub(4) else {
            continue;
        };
        if object
            .checked_add(MTOB_MIN)
            .is_none_or(|end| end > payload.len())
        {
            continue;
        }
        if read_u32(payload, object) != Some(MTOB_TYPE) {
            continue;
        }

        let name = read_self_string(payload, object + 0x0c).filter(|value| !value.is_empty());
        let mut textures = Vec::new();
        for slot in 0..3usize {
            let Some(field) = object
                .checked_add(0x274)
                .and_then(|base| base.checked_add(slot * 4))
            else {
                continue;
            };
            let Some(texture_name) = reference_texture_name(payload, field) else {
                continue;
            };
            let Ok(slot) = u8::try_from(slot) else {
                continue;
            };
            textures.push(TextureReference {
                slot,
                internal_name: texture_name,
                role: TextureRole::Unknown,
                enabled: true,
            });
        }

        materials.push(MaterialRecord {
            index: materials.len() as u32,
            name,
            textures,
        });
    }

    Ok(ModelInventory {
        model_name,
        materials,
    })
}

fn locate_cgfx_payload(data: &[u8]) -> Option<&[u8]> {
    if let Ok(declared_size) = validate_cgfx_header(data) {
        return data.get(..declared_size);
    }
    if data.get(..4) != Some(b"ATBC") {
        return None;
    }

    let mut search = 0usize;
    let mut found = 0usize;
    while search + 4 <= data.len() && found < MAX_EMBEDDED_CGFX {
        let relative = find_magic(&data[search..], b"CGFX")?;
        let offset = search + relative;
        search = offset.saturating_add(4);
        found += 1;
        let candidate = data.get(offset..)?;
        let Ok(declared_size) = validate_cgfx_header(candidate) else {
            continue;
        };
        if let Some(payload) = candidate.get(..declared_size) {
            return Some(payload);
        }
    }
    None
}

fn validate_cgfx_header(data: &[u8]) -> Result<usize, ModelError> {
    if data.len() < CGFX_HEADER_MIN || data.get(..4) != Some(b"CGFX") {
        return Err(ModelError::InvalidHeader);
    }
    if data.get(4..6) != Some(&[0xff, 0xfe]) {
        return Err(ModelError::InvalidHeader);
    }
    let header_size = read_u16(data, 6).ok_or(ModelError::InvalidHeader)?;
    let declared_size = read_u32(data, 0x0c).ok_or(ModelError::InvalidHeader)?;
    if usize::from(header_size) < CGFX_HEADER_MIN || declared_size < CGFX_DECLARED_MIN {
        return Err(ModelError::InvalidHeader);
    }
    let declared_size = usize::try_from(declared_size).map_err(|_| ModelError::InvalidOffset)?;
    if declared_size > data.len() {
        return Err(ModelError::InvalidOffset);
    }
    Ok(declared_size)
}

fn first_cmdl_name(data: &[u8]) -> Option<String> {
    let signature = find_magic(data, b"CMDL")?;
    let object = signature.checked_sub(4)?;
    read_self_string(data, object.checked_add(0x0c)?)
        .filter(|value| !value.is_empty())
}

fn reference_texture_name(data: &[u8], mapper_field: usize) -> Option<String> {
    let relative = usize::try_from(read_u32(data, mapper_field)?).ok()?;
    if relative == 0 {
        return None;
    }
    let texinfo = mapper_field.checked_add(relative)?;
    if read_u32(data, texinfo)? != TEXINFO_TYPE {
        return None;
    }

    let tx_field = texinfo.checked_add(0x08)?;
    let tx_relative = usize::try_from(read_u32(data, tx_field)?).ok()?;
    if tx_relative == 0 {
        return None;
    }
    let txob = tx_field.checked_add(tx_relative)?;
    if read_u32(data, txob)? != REFERENCE_TXOB_TYPE
        || data.get(txob.checked_add(4)?..txob.checked_add(8)?) != Some(b"TXOB")
    {
        return None;
    }
    read_self_string(data, txob.checked_add(0x18)?)
        .filter(|value| !value.is_empty())
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
    data.get(offset..offset.checked_add(2)?)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.checked_add(4)?)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_self_string(data: &mut [u8], field: usize, start: usize, value: &[u8]) {
        put_u32(data, field, (start - field) as u32);
        data[start..start + value.len()].copy_from_slice(value);
        data[start + value.len()] = 0;
    }

    fn synthetic_cgfx() -> Vec<u8> {
        let mut data = vec![0u8; 0x580];
        data[0..4].copy_from_slice(b"CGFX");
        data[4..6].copy_from_slice(&[0xff, 0xfe]);
        data[6..8].copy_from_slice(&0x14u16.to_le_bytes());
        let declared_size = data.len() as u32;
        put_u32(&mut data, 0x0c, declared_size);

        let cmdl = 0x40usize;
        data[cmdl + 4..cmdl + 8].copy_from_slice(b"CMDL");
        put_self_string(&mut data, cmdl + 0x0c, 0x90, b"enemy_model");

        let material = 0x100usize;
        put_u32(&mut data, material, MTOB_TYPE);
        data[material + 4..material + 8].copy_from_slice(b"MTOB");
        put_self_string(&mut data, material + 0x0c, 0x3a0, b"body_material");

        let mapper_field = material + 0x274;
        let texinfo = 0x400usize;
        put_u32(&mut data, mapper_field, (texinfo - mapper_field) as u32);
        put_u32(&mut data, texinfo, TEXINFO_TYPE);

        let tx_field = texinfo + 0x08;
        let txob = 0x440usize;
        put_u32(&mut data, tx_field, (txob - tx_field) as u32);
        put_u32(&mut data, txob, REFERENCE_TXOB_TYPE);
        data[txob + 4..txob + 8].copy_from_slice(b"TXOB");
        put_self_string(&mut data, txob + 0x18, 0x500, b"body_tex");
        data
    }

    #[test]
    fn direct_cgfx_material_bindings_are_structural() {
        let data = synthetic_cgfx();
        let inventory = CgfxModelInspector.inspect(&data).unwrap();
        assert_eq!(inventory.model_name.as_deref(), Some("enemy_model"));
        assert_eq!(inventory.materials.len(), 1);
        let material = &inventory.materials[0];
        assert_eq!(material.name.as_deref(), Some("body_material"));
        assert_eq!(material.textures.len(), 1);
        assert_eq!(material.textures[0].slot, 0);
        assert_eq!(material.textures[0].internal_name, "body_tex");
        assert_eq!(material.textures[0].role, TextureRole::Unknown);
        assert!(material.textures[0].enabled);
    }

    #[test]
    fn embedded_atbc_cgfx_is_inspected_without_fixed_wrapper_offset() {
        let cgfx = synthetic_cgfx();
        let offset = 0x123usize;
        let mut atbc = vec![0u8; offset + cgfx.len()];
        atbc[..4].copy_from_slice(b"ATBC");
        atbc[offset..].copy_from_slice(&cgfx);
        let inspector = CgfxModelInspector;
        assert!(inspector.probe(&atbc));
        let inventory = inspector.inspect(&atbc).unwrap();
        assert_eq!(inventory.materials[0].textures[0].internal_name, "body_tex");
    }

    #[test]
    fn false_mtob_magic_is_not_treated_as_a_material() {
        let mut data = synthetic_cgfx();
        let false_object = 0x60usize;
        data[false_object + 4..false_object + 8].copy_from_slice(b"MTOB");
        let inventory = CgfxModelInspector.inspect(&data).unwrap();
        assert_eq!(inventory.materials.len(), 1);
    }

    #[test]
    fn malformed_reference_texture_pointer_is_ignored_not_guessed() {
        let mut data = synthetic_cgfx();
        let material = 0x100usize;
        let mapper_field = material + 0x274;
        put_u32(&mut data, mapper_field, u32::MAX);
        let inventory = CgfxModelInspector.inspect(&data).unwrap();
        assert_eq!(inventory.materials.len(), 1);
        assert!(inventory.materials[0].textures.is_empty());
    }

    #[test]
    fn truncated_declared_cgfx_is_rejected() {
        let mut data = synthetic_cgfx();
        put_u32(&mut data, 0x0c, data.len() as u32 + 1);
        assert_eq!(CgfxModelInspector.inspect(&data), Err(ModelError::InvalidHeader));
    }

    #[test]
    fn atbc_without_a_structural_cgfx_is_not_claimed() {
        let mut data = vec![0u8; 0x80];
        data[..4].copy_from_slice(b"ATBC");
        data[0x20..0x24].copy_from_slice(b"CGFX");
        let inspector = CgfxModelInspector;
        assert!(!inspector.probe(&data));
        assert_eq!(inspector.inspect(&data), Err(ModelError::InvalidHeader));
    }
}
