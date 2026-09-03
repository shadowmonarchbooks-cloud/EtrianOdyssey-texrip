use crate::{MaterialRecord, ModelError, ModelInspector, ModelInventory, TextureReference};
use encoding_rs::SHIFT_JIS;
use eo_core::TextureRole;
use std::collections::BTreeMap;

const BCH_MIN_SIZE: usize = 0x38;
const SECTION_MODELS: usize = 0;
const MAX_MODELS: u32 = 1024;
const MAX_MATERIALS: u32 = 2048;
const MAX_COMMAND_WORDS: u32 = 0x4000;
const MAX_STRING_BYTES: usize = 512;
const MAX_EMBEDDED_BCH: usize = 16;
const GPUREG_TEXUNIT_CONFIG: u16 = 0x0080;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BchModelInspector;

#[derive(Clone, Copy, Debug)]
struct BchHeader {
    backward_compat: u8,
    content_addr: u32,
    strings_addr: u32,
    commands_addr: u32,
    content_len: u32,
    strings_len: u32,
}

impl ModelInspector for BchModelInspector {
    fn probe(&self, data: &[u8]) -> bool {
        locate_bch_payload(data).is_some()
    }

    fn inspect(&self, data: &[u8]) -> Result<ModelInventory, ModelError> {
        let payload = locate_bch_payload(data).ok_or(ModelError::InvalidHeader)?;
        inspect_bch_payload(payload)
    }
}

fn inspect_bch_payload(data: &[u8]) -> Result<ModelInventory, ModelError> {
    let header = parse_header(data)?;
    let model_offsets = pointer_table_entries(data, &header, SECTION_MODELS, MAX_MODELS)?;
    let mut model_names = Vec::new();
    let mut materials = Vec::new();

    for (model_index, model_start) in model_offsets.into_iter().enumerate() {
        let model_name = model_name(data, &header, model_start)
            .unwrap_or_else(|| format!("model_{model_index:03}"));
        model_names.push(model_name.clone());

        let Some((table, count, record_size)) = model_material_table(data, &header, model_start)
        else {
            continue;
        };

        for local_index in 0..count as usize {
            let Some(material_start) = table.checked_add(local_index.saturating_mul(record_size)) else {
                continue;
            };
            if material_start
                .checked_add(record_size)
                .is_none_or(|end| end > data.len())
            {
                continue;
            }

            let names_offset = if header.backward_compat < 0x21 {
                0x48usize
            } else {
                0x1cusize
            };
            let name = read_u32(data, material_start + names_offset + 12)
                .and_then(|raw| resolve_string(data, &header, raw))
                .filter(|value| !value.is_empty())
                .or_else(|| Some(format!("{model_name}_material_{local_index:03}")));

            let enabled = material_texture_enablement(data, &header, material_start);
            let mut textures = Vec::new();
            for (slot, is_enabled) in enabled.into_iter().enumerate() {
                let Some(field) = material_start
                    .checked_add(names_offset)
                    .and_then(|base| base.checked_add(slot * 4))
                else {
                    continue;
                };
                let Some(texture_name) = read_u32(data, field)
                    .and_then(|raw| resolve_string(data, &header, raw))
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Ok(slot_u8) = u8::try_from(slot) else {
                    continue;
                };
                textures.push(TextureReference {
                    slot: slot_u8,
                    internal_name: texture_name,
                    role: TextureRole::Unknown,
                    enabled: is_enabled,
                });
            }

            materials.push(MaterialRecord {
                index: materials.len() as u32,
                name,
                textures,
            });
        }
    }

    let model_name = match model_names.as_slice() {
        [] => None,
        [only] => Some(only.clone()),
        many if many.windows(2).all(|pair| pair[0] == pair[1]) => Some(many[0].clone()),
        _ => None,
    };

    Ok(ModelInventory {
        model_name,
        materials,
    })
}

fn locate_bch_payload(data: &[u8]) -> Option<&[u8]> {
    if parse_header(data).is_ok() {
        return Some(data);
    }
    if !matches!(data.get(..4), Some(b"ATBC") | Some(b"BAM2")) {
        return None;
    }

    let mut search = 0usize;
    let mut found = 0usize;
    while search + 4 <= data.len() && found < MAX_EMBEDDED_BCH {
        let relative = find_magic(&data[search..], b"BCH\0")?;
        let offset = search + relative;
        search = offset.saturating_add(4);
        found += 1;
        let candidate = data.get(offset..)?;
        if parse_header(candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn parse_header(data: &[u8]) -> Result<BchHeader, ModelError> {
    if data.len() < BCH_MIN_SIZE || data.get(..4) != Some(b"BCH\0") {
        return Err(ModelError::InvalidHeader);
    }
    let backward_compat = data[4];
    let content_addr = read_u32(data, 0x08).ok_or(ModelError::InvalidHeader)?;
    let strings_addr = read_u32(data, 0x0c).ok_or(ModelError::InvalidHeader)?;
    let commands_addr = read_u32(data, 0x10).ok_or(ModelError::InvalidHeader)?;
    let data_addr = read_u32(data, 0x14).ok_or(ModelError::InvalidHeader)?;

    let mut cursor = 0x18usize;
    let mut data_ext_addr = 0u32;
    let mut data_ext_len = 0u32;
    if backward_compat > 0x20 {
        data_ext_addr = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;
        cursor += 4;
    }
    let reloc_addr = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;
    cursor += 4;
    let content_len = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;
    cursor += 4;
    let strings_len = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;
    cursor += 4;
    let commands_len = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;
    cursor += 4;
    let data_len = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;
    cursor += 4;
    if backward_compat > 0x20 {
        data_ext_len = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;
        cursor += 4;
    }
    let reloc_len = read_u32(data, cursor).ok_or(ModelError::InvalidHeader)?;

    if content_addr == 0 || content_len < 12 {
        return Err(ModelError::InvalidHeader);
    }
    for (address, length) in [
        (content_addr, content_len),
        (strings_addr, strings_len),
        (commands_addr, commands_len),
        (data_addr, data_len),
        (data_ext_addr, data_ext_len),
        (reloc_addr, reloc_len),
    ] {
        if address == 0 {
            if length != 0 {
                return Err(ModelError::InvalidOffset);
            }
            continue;
        }
        let start = address as usize;
        if start >= data.len()
            || start
                .checked_add(length as usize)
                .is_none_or(|end| end > data.len())
        {
            return Err(ModelError::InvalidOffset);
        }
    }

    Ok(BchHeader {
        backward_compat,
        content_addr,
        strings_addr,
        commands_addr,
        content_len,
        strings_len,
    })
}

fn section(data: &[u8], header: &BchHeader, index: usize) -> Option<(u32, u32, u32)> {
    let start = (header.content_addr as usize).checked_add(index.checked_mul(12)?)?;
    let content_end = (header.content_addr as usize).checked_add(header.content_len as usize)?;
    if start.checked_add(12)? > content_end || content_end > data.len() {
        return None;
    }
    Some((
        read_u32(data, start)?,
        read_u32(data, start + 4)?,
        read_u32(data, start + 8)?,
    ))
}

fn resolve_main_offset(raw: u32, header: &BchHeader, data_len: usize) -> Option<usize> {
    if raw == 0 {
        return None;
    }
    let content = header.content_addr as usize;
    let raw = raw as usize;
    let upper = if header.strings_addr > header.content_addr {
        (header.strings_addr as usize).min(data_len)
    } else {
        data_len
    };
    for candidate in [content.checked_add(raw), Some(raw)].into_iter().flatten() {
        if content <= candidate && candidate < upper {
            return Some(candidate);
        }
    }
    [content.checked_add(raw), Some(raw)]
        .into_iter()
        .flatten()
        .find(|candidate| *candidate < data_len)
}

fn pointer_table_entries(
    data: &[u8],
    header: &BchHeader,
    section_index: usize,
    max_count: u32,
) -> Result<Vec<usize>, ModelError> {
    let Some((pointer_offset, count, _dict_offset)) = section(data, header, section_index) else {
        return Ok(Vec::new());
    };
    if pointer_offset == 0 || count == 0 {
        return Ok(Vec::new());
    }
    if count > max_count {
        return Err(ModelError::InvalidMaterial(format!(
            "section {section_index} count {count} exceeds {max_count}"
        )));
    }
    let Some(table) = resolve_main_offset(pointer_offset, header, data.len()) else {
        return Ok(Vec::new());
    };
    let table_end = table
        .checked_add(count as usize * 4)
        .ok_or(ModelError::InvalidOffset)?;
    if table_end > data.len() {
        return Err(ModelError::InvalidOffset);
    }

    let mut entries = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let raw = read_u32(data, table + index * 4).unwrap_or(0);
        if let Some(offset) = resolve_main_offset(raw, header, data.len()) {
            entries.push(offset);
        }
    }
    Ok(entries)
}

fn resolve_string(data: &[u8], header: &BchHeader, raw: u32) -> Option<String> {
    if header.strings_addr == 0 {
        return None;
    }
    let start = header.strings_addr as usize;
    let end = if header.strings_len == 0 {
        data.len()
    } else {
        start.checked_add(header.strings_len as usize)?.min(data.len())
    };
    for position in [start.checked_add(raw as usize), Some(raw as usize)]
        .into_iter()
        .flatten()
    {
        if position < start || position >= end {
            continue;
        }
        let limit = end.min(position.saturating_add(MAX_STRING_BYTES));
        let bytes = data.get(position..limit)?;
        let terminator = bytes.iter().position(|byte| *byte == 0)?;
        let raw_text = &bytes[..terminator];
        if raw_text.is_empty() {
            continue;
        }
        let text = if let Ok(value) = std::str::from_utf8(raw_text) {
            value.to_owned()
        } else {
            let (decoded, _) = SHIFT_JIS.decode_without_bom_handling(raw_text);
            decoded.into_owned()
        };
        if !text.is_empty()
            && text
                .chars()
                .all(|character| !character.is_control() || matches!(character, '\t' | '\r' | '\n'))
        {
            return Some(text);
        }
    }
    None
}

fn model_name(data: &[u8], header: &BchHeader, model_start: usize) -> Option<String> {
    let offsets: &[usize] = if header.backward_compat > 6 {
        &[0x84]
    } else {
        &[0x7c, 0x84]
    };
    offsets.iter().find_map(|relative| {
        model_start
            .checked_add(*relative)
            .and_then(|field| read_u32(data, field))
            .and_then(|raw| resolve_string(data, header, raw))
            .filter(|value| !value.is_empty())
    })
}

fn model_material_table(
    data: &[u8],
    header: &BchHeader,
    model_start: usize,
) -> Option<(usize, u32, usize)> {
    if model_start.checked_add(0x3c)? > data.len() {
        return None;
    }
    let raw_table = read_u32(data, model_start + 0x34)?;
    let count = read_u32(data, model_start + 0x38)?;
    if count == 0 || count > MAX_MATERIALS {
        return None;
    }
    let table = resolve_main_offset(raw_table, header, data.len())?;
    let record_size = if header.backward_compat < 0x21 {
        0x58usize
    } else {
        0x2cusize
    };
    table
        .checked_add(count as usize * record_size)
        .filter(|end| *end <= data.len())?;
    Some((table, count, record_size))
}

fn material_texture_enablement(
    data: &[u8],
    header: &BchHeader,
    material_start: usize,
) -> [bool; 3] {
    let Some(raw_pointer) = read_u32(data, material_start + 0x10) else {
        return [false; 3];
    };
    let Some(word_count) = read_u32(data, material_start + 0x14) else {
        return [false; 3];
    };
    let Some(registers) = command_block(data, header, raw_pointer, word_count) else {
        return [false; 3];
    };
    let value = registers.get(&GPUREG_TEXUNIT_CONFIG).copied().unwrap_or(0);
    [value & 0x001 != 0, value & 0x002 != 0, value & 0x004 != 0]
}

fn command_block(
    data: &[u8],
    header: &BchHeader,
    raw_pointer: u32,
    word_count: u32,
) -> Option<BTreeMap<u16, u32>> {
    if word_count == 0 || word_count > MAX_COMMAND_WORDS {
        return None;
    }
    let byte_count = (word_count as usize).checked_mul(4)?;
    for start in [
        (header.commands_addr as usize).checked_add(raw_pointer as usize),
        Some(raw_pointer as usize),
    ]
    .into_iter()
    .flatten()
    {
        if start
            .checked_add(byte_count)
            .is_none_or(|end| end > data.len())
        {
            continue;
        }
        let registers = parse_gpu_commands(data, start, word_count);
        if !registers.is_empty() {
            return Some(registers);
        }
    }
    None
}

fn parse_gpu_commands(data: &[u8], start: usize, word_count: u32) -> BTreeMap<u16, u32> {
    let mut registers = BTreeMap::new();
    let Some(end) = start.checked_add(word_count as usize * 4) else {
        return registers;
    };
    let end = end.min(data.len());
    let mut position = start;
    while position + 8 <= end {
        let Some(parameter) = read_u32(data, position) else {
            break;
        };
        let Some(command) = read_u32(data, position + 4) else {
            break;
        };
        let register = (command & 0xffff) as u16;
        let extra = ((command >> 20) & 0xff) as usize;
        let consecutive = command & 0x8000_0000 != 0;
        position += 8;
        registers.insert(register, parameter);
        for index in 0..extra {
            if position + 4 > end {
                break;
            }
            let Some(value) = read_u32(data, position) else {
                break;
            };
            position += 4;
            let target = if consecutive {
                register.wrapping_add(index as u16 + 1)
            } else {
                register
            };
            registers.insert(target, value);
        }
        if position & 7 != 0 {
            position = position.saturating_add(4);
        }
    }
    registers
}

fn find_magic(data: &[u8], magic: &[u8; 4]) -> Option<usize> {
    data.windows(4).position(|window| window == magic)
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

    fn synthetic_bch_model() -> Vec<u8> {
        let mut data = vec![0u8; 0x400];
        data[..4].copy_from_slice(b"BCH\0");
        data[4] = 0x21;
        data[6..8].copy_from_slice(&1u16.to_le_bytes());
        put_u32(&mut data, 0x08, 0x40);
        put_u32(&mut data, 0x0c, 0x300);
        put_u32(&mut data, 0x10, 0x380);
        put_u32(&mut data, 0x14, 0);
        put_u32(&mut data, 0x18, 0);
        put_u32(&mut data, 0x1c, 0);
        put_u32(&mut data, 0x20, 0x2c0);
        put_u32(&mut data, 0x24, 0x80);
        put_u32(&mut data, 0x28, 0x40);
        put_u32(&mut data, 0x2c, 0);
        put_u32(&mut data, 0x30, 0);
        put_u32(&mut data, 0x34, 0);

        put_u32(&mut data, 0x40, 0x40);
        put_u32(&mut data, 0x44, 1);
        put_u32(&mut data, 0x48, 0);
        put_u32(&mut data, 0x80, 0x60);

        let model = 0xa0usize;
        put_u32(&mut data, model + 0x34, 0x120);
        put_u32(&mut data, model + 0x38, 1);
        put_u32(&mut data, model + 0x84, 0);

        let material = 0x160usize;
        put_u32(&mut data, material + 0x10, 0);
        put_u32(&mut data, material + 0x14, 2);
        put_u32(&mut data, material + 0x1c, 12);
        put_u32(&mut data, material + 0x20, 0x80);
        put_u32(&mut data, material + 0x24, 0x80);
        put_u32(&mut data, material + 0x28, 21);

        data[0x300..0x30c].copy_from_slice(b"enemy_model\0");
        data[0x30c..0x315].copy_from_slice(b"body_tex\0");
        data[0x315..0x323].copy_from_slice(b"body_material\0");

        put_u32(&mut data, 0x380, 1);
        put_u32(&mut data, 0x384, GPUREG_TEXUNIT_CONFIG as u32);
        data
    }

    #[test]
    fn direct_bch_material_names_and_enable_bits_are_structural() {
        let data = synthetic_bch_model();
        let inventory = BchModelInspector.inspect(&data).unwrap();
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
    fn bam2_embedded_bch_is_found_without_fixed_offset() {
        let bch = synthetic_bch_model();
        let offset = 0x117usize;
        let mut wrapper = vec![0u8; offset + bch.len()];
        wrapper[..4].copy_from_slice(b"BAM2");
        wrapper[offset..].copy_from_slice(&bch);
        let inspector = BchModelInspector;
        assert!(inspector.probe(&wrapper));
        let inventory = inspector.inspect(&wrapper).unwrap();
        assert_eq!(inventory.materials[0].textures[0].internal_name, "body_tex");
    }

    #[test]
    fn disabled_texture_slot_is_preserved_as_disabled_metadata() {
        let mut data = synthetic_bch_model();
        put_u32(&mut data, 0x380, 0);
        let inventory = BchModelInspector.inspect(&data).unwrap();
        assert_eq!(inventory.materials[0].textures.len(), 1);
        assert!(!inventory.materials[0].textures[0].enabled);
    }

    #[test]
    fn false_bch_magic_inside_wrapper_is_not_claimed() {
        let mut wrapper = vec![0u8; 0x100];
        wrapper[..4].copy_from_slice(b"ATBC");
        wrapper[0x40..0x44].copy_from_slice(b"BCH\0");
        let inspector = BchModelInspector;
        assert!(!inspector.probe(&wrapper));
        assert_eq!(inspector.inspect(&wrapper), Err(ModelError::InvalidHeader));
    }

    #[test]
    fn oversized_material_count_is_not_allocated() {
        let mut data = synthetic_bch_model();
        put_u32(&mut data, 0xa0 + 0x38, MAX_MATERIALS + 1);
        let inventory = BchModelInspector.inspect(&data).unwrap();
        assert!(inventory.materials.is_empty());
    }
}
