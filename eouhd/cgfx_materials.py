from __future__ import annotations

"""CGFX/BCMDL + EOU ATBC material/texture inspection.

EOU1 enemy .BAM files are ATBC wrappers.  In the samples supplied from the
actual game, a complete CGFX starts at 0x180 and its declared file size exactly
covers the remainder of the BAM.  CGFX materials are MTOB objects whose texture
mappers point to ReferenceTexture TXOB objects, while actual image pixels live
in ImageTexture TXOB objects.

This module intentionally reads those structural relationships rather than
inferring masks from decoded PNG appearance.
"""

import struct
from typing import Any

from .bch_materials import (
    SOURCE_NAMES, ALPHA_OPERAND_NAMES, COMBINER_NAMES, COMBINER_ARITY,
)


class CGFXError(ValueError):
    pass


_FORMAT_BPP = {
    0x0: 32, 0x1: 24, 0x2: 16, 0x3: 16, 0x4: 16, 0x5: 16, 0x6: 16,
    0x7: 8, 0x8: 8, 0x9: 8, 0xA: 4, 0xB: 4, 0xC: 4, 0xD: 8,
}

def _base_level_size(width: int, height: int, fmt: int) -> int:
    bpp = _FORMAT_BPP.get(fmt, 0)
    if not bpp:
        return 0
    # PICA texture data is tiled in 8x8 blocks.
    pw = (width + 7) // 8 * 8
    ph = (height + 7) // 8 * 8
    return (pw * ph * bpp + 7) // 8


def _u16(data: bytes, off: int) -> int:
    if off < 0 or off + 2 > len(data):
        raise CGFXError(f'u16 out of range at 0x{off:X}')
    return struct.unpack_from('<H', data, off)[0]


def _u32(data: bytes, off: int) -> int:
    if off < 0 or off + 4 > len(data):
        raise CGFXError(f'u32 out of range at 0x{off:X}')
    return struct.unpack_from('<I', data, off)[0]


def _read_self_string(data: bytes, field: int, max_len: int = 512) -> str:
    raw = _u32(data, field)
    if raw == 0:
        return ''
    pos = field + raw
    if pos < 0 or pos >= len(data):
        return ''
    end = data.find(b'\0', pos, min(len(data), pos + max_len))
    if end <= pos:
        return ''
    try:
        return data[pos:end].decode('utf-8')
    except UnicodeDecodeError:
        try:
            return data[pos:end].decode('shift_jis')
        except UnicodeDecodeError:
            return data[pos:end].decode('ascii', errors='replace')


def find_cgfx_payloads(data: bytes, limit: int = 16, allow_truncated: bool = False) -> list[tuple[int, int]]:
    """Return validated ``(offset, declared_size)`` CGFX payloads.

    This recognizes direct .bcmdl/.bcres CGFX files as well as the complete
    CGFX embedded after EOU's ATBC BAM wrapper. When *allow_truncated* is true,
    only the CGFX header must be present; this is used for bounded file probes
    so multi-megabyte ATBC files are not accidentally skipped before the full
    file is read. Full decoding always calls this with the default ``False``.
    """
    out: list[tuple[int, int]] = []
    pos = 0
    while len(out) < limit:
        off = data.find(b'CGFX', pos)
        if off < 0:
            break
        pos = off + 4
        if off + 0x14 > len(data):
            continue
        # CGFX BOM should be FF FE for little-endian and header size 0x14.
        if data[off + 4:off + 6] != b'\xff\xfe':
            continue
        header_size = _u16(data, off + 6)
        declared = _u32(data, off + 0x0C)
        if header_size < 0x14 or declared < 0x20:
            continue
        if off + declared > len(data) and not allow_truncated:
            continue
        out.append((off, declared))
    return out


def atbc_info(data: bytes) -> dict[str, Any] | None:
    if not data.startswith(b'ATBC'):
        return None
    payloads = find_cgfx_payloads(data)
    if not payloads:
        return {'magic': 'ATBC', 'cgfx_offset': None, 'cgfx_size': 0}
    off, size = payloads[0]
    return {
        'magic': 'ATBC',
        'cgfx_offset': off,
        'cgfx_size': size,
        'wrapper_size': off,
        'cgfx_covers_remainder': off + size == len(data),
    }


def extract_cgfx_texture_infos(data: bytes) -> list[dict[str, Any]]:
    """Return structurally declared CGFX ImageTexture TXOB payloads."""
    if len(data) < 0x14 or data[:4] != b'CGFX':
        return []
    out: list[dict[str, Any]] = []
    pos = 0
    while True:
        sig = data.find(b'TXOB', pos)
        if sig < 0:
            break
        pos = sig + 4
        obj = sig - 4
        if obj < 0 or obj + 0x4C > len(data):
            continue
        # ImageTextureCtr type. ReferenceTexture is 0x20000004.
        if _u32(data, obj) != 0x20000011:
            continue
        try:
            name = _read_self_string(data, obj + 0x0C) or f'cgfx_tex_{len(out):04d}'
            height = _u32(data, obj + 0x18)
            width = _u32(data, obj + 0x1C)
            fmt = _u32(data, obj + 0x34)
            mip_count = _u32(data, obj + 0x28)
            image_rel = _u32(data, obj + 0x38)
            image_obj = obj + 0x38 + image_rel if image_rel else 0
            if not image_obj or image_obj + 0x20 > len(data):
                continue
            image_h = _u32(data, image_obj + 0x00)
            image_w = _u32(data, image_obj + 0x04)
            data_size = _u32(data, image_obj + 0x08)
            data_field = image_obj + 0x0C
            data_offset = data_field + _u32(data, data_field)
            if width <= 0 or height <= 0 or width > 4096 or height > 4096:
                continue
            if image_w not in (0, width) or image_h not in (0, height):
                continue
            base_size = _base_level_size(width, height, fmt)
            if fmt > 0x0D or data_size <= 0 or base_size <= 0:
                continue
            if data_offset < 0 or data_offset + data_size > len(data):
                continue
            if base_size > data_size:
                continue
            out.append({
                'index': len(out),
                'name': name,
                'width': width,
                'height': height,
                'format': fmt,
                'mip_count': max(1, mip_count),
                'data_offset': data_offset,
                # Decode/hash the base level only. PixelBasedImageCtr.DataSize
                # may include following mip levels, while Azahar requests each
                # uploaded mip level independently.
                'data_size': base_size,
                'storage_data_size': data_size,
                'txob_offset': obj,
                'image_object_offset': image_obj,
            })
        except Exception:
            continue
    return out


def _reference_texture_name(data: bytes, texinfo: int) -> str:
    if texinfo <= 0 or texinfo + 0x50 > len(data):
        return ''
    # TexInfo starts with type 0x80000000 and has self-relative TXOB at +8.
    if _u32(data, texinfo) != 0x80000000:
        return ''
    tx_field = texinfo + 0x08
    rel = _u32(data, tx_field)
    if rel == 0:
        return ''
    txob = tx_field + rel
    if txob < 0 or txob + 0x20 > len(data):
        return ''
    if _u32(data, txob) != 0x20000004 or data[txob + 4:txob + 8] != b'TXOB':
        return ''
    # ReferenceTexture adds LinkedTextureName immediately after TXOB base header.
    return _read_self_string(data, txob + 0x18)


def _decode_alpha_stages_from_fragment(data: bytes, fragment: int) -> tuple[list[dict], dict | None]:
    # FragmentShader: BufferColor 0x10 + FragmentLighting 0x18 + table pointer 4.
    combiner_base = fragment + 0x2C
    stage_size = 0x1C
    if fragment < 0 or combiner_base + 6 * stage_size + 8 > len(data):
        return [], None
    stages: list[dict] = []
    for stage in range(6):
        off = combiner_base + stage * stage_size
        src_alpha = _u16(data, off + 0x06)
        operands = _u32(data, off + 0x0C)
        alpha_mode = _u16(data, off + 0x12) & 0xF
        arity = COMBINER_ARITY.get(alpha_mode, 3)
        inputs: list[dict] = []
        for i in range(arity):
            source_id = (src_alpha >> (i * 4)) & 0xF
            operand_id = (operands >> (12 + i * 4)) & 0x7
            inputs.append({
                'input': i,
                'source_id': source_id,
                'source': SOURCE_NAMES.get(source_id, f'Unknown{source_id}'),
                'operand_id': operand_id,
                'operand': ALPHA_OPERAND_NAMES.get(operand_id, f'Unknown{operand_id}'),
            })
        stages.append({
            'stage': stage,
            'combiner_id': alpha_mode,
            'combiner': COMBINER_NAMES.get(alpha_mode, f'Unknown{alpha_mode}'),
            'inputs': inputs,
        })
    at = combiner_base + 6 * stage_size
    command1 = _u32(data, at)
    alpha_test = {
        'enabled': bool(command1 & 1),
        'function': (command1 >> 4) & 7,
        'reference': (command1 >> 8) & 0xFF,
        'raw': command1,
    }
    return stages, alpha_test


def _alpha_texture_uses(stages: list[dict]) -> list[dict]:
    uses: list[dict] = []
    for stage in stages:
        for inp in stage.get('inputs', []):
            sid = int(inp.get('source_id', -1))
            if 3 <= sid <= 5:
                uses.append({
                    'stage': stage.get('stage'),
                    'combiner': stage.get('combiner'),
                    'input': inp.get('input'),
                    'slot': sid - 3,
                    'source': inp.get('source'),
                    'operand': inp.get('operand'),
                    'operand_id': inp.get('operand_id'),
                })
    return uses


def _first_cmdl_name(data: bytes) -> str:
    sig = data.find(b'CMDL')
    if sig < 4:
        return 'cgfx_model'
    obj = sig - 4
    try:
        return _read_self_string(data, obj + 0x0C) or 'cgfx_model'
    except Exception:
        return 'cgfx_model'


def parse_cgfx_materials(data: bytes) -> dict[str, Any]:
    """Parse CGFX MTOB texture-slot bindings and fragment alpha pipeline."""
    if len(data) < 0x14 or data[:4] != b'CGFX':
        raise CGFXError('not a CGFX payload')

    textures = extract_cgfx_texture_infos(data)
    texture_names = {str(t.get('name') or '') for t in textures if t.get('name')}
    model_name = _first_cmdl_name(data)
    materials: list[dict] = []
    material_parse_errors: list[dict] = []
    mtob_candidates = 0
    pos = 0
    while True:
        sig = data.find(b'MTOB', pos)
        if sig < 0:
            break
        pos = sig + 4
        obj = sig - 4
        if obj < 0 or obj + 0x28C > len(data):
            continue
        if _u32(data, obj) != 0x08000000:
            continue
        mtob_candidates += 1
        try:
            name = _read_self_string(data, obj + 0x0C) or f'material_{len(materials):03d}'
            slots: list[dict] = []
            for slot in range(3):
                field = obj + 0x274 + slot * 4
                rel = _u32(data, field)
                if rel == 0:
                    continue
                texinfo = field + rel
                tex_name = _reference_texture_name(data, texinfo)
                if not tex_name:
                    continue
                slots.append({
                    'slot': slot,
                    'texture_name': tex_name,
                    'enabled': True,
                    'known_cgfx_texture': tex_name in texture_names,
                    'texinfo_offset': texinfo,
                })
            frag_field = obj + 0x288
            frag_rel = _u32(data, frag_field)
            fragment = frag_field + frag_rel if frag_rel else 0
            stages, alpha_test = _decode_alpha_stages_from_fragment(data, fragment) if fragment else ([], None)
            uses = _alpha_texture_uses(stages)
            materials.append({
                'index': len(materials),
                'model_index': 0,
                'model_name': model_name,
                'model_material_index': len(materials),
                'name': name,
                'record_offset': obj,
                'revision': _u32(data, obj + 0x08),
                'texture_slots': slots,
                'enabled_texture_slots': [int(x['slot']) for x in slots],
                'alpha_stages': stages,
                'alpha_texture_uses': uses,
                'alpha_test': alpha_test,
                'fragment_shader_offset': fragment or None,
            })
        except Exception as exc:
            material_parse_errors.append({'offset': obj, 'error': str(exc)})
            continue

    return {
        'format': 'cgfx',
        'model_count': 1 if data.find(b'CMDL') >= 0 else 0,
        'models': [{'index': 0, 'name': model_name}] if data.find(b'CMDL') >= 0 else [],
        'materials': materials,
        'mtob_candidates': mtob_candidates,
        'material_parse_errors': material_parse_errors,
        'textures': textures,
    }


def bindings_by_texture(material_report: dict) -> dict[str, list[dict]]:
    by_name: dict[str, list[dict]] = {}
    for material in material_report.get('materials', []):
        uses_by_slot: dict[int, list[dict]] = {}
        for use in material.get('alpha_texture_uses', []):
            uses_by_slot.setdefault(int(use['slot']), []).append(use)
        for slot in material.get('texture_slots', []):
            name = str(slot.get('texture_name') or '')
            if not name:
                continue
            idx = int(slot.get('slot', 0))
            by_name.setdefault(name, []).append({
                'material_index': material.get('index'),
                'material_name': material.get('name'),
                'model_index': material.get('model_index'),
                'model_name': material.get('model_name'),
                'model_material_index': material.get('model_material_index'),
                'slot': idx,
                'enabled': bool(slot.get('enabled')),
                'alpha_uses': uses_by_slot.get(idx, []),
                'alpha_stages': material.get('alpha_stages', []),
                'alpha_test': material.get('alpha_test'),
                'material_format': 'CGFX/MTOB',
            })
    return by_name
