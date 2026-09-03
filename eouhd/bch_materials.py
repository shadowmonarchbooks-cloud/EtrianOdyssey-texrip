from __future__ import annotations

"""Material-aware BCH/H3D inspection for the Etrian Odyssey Untold-series 3DS games.

Unlike the image heuristics used in v0.4, this module reads relationships from
BCH metadata itself:

* content-table material and texture sections
* BCH relocation entries that identify string and GPU-command pointers
* material texture-unit enable commands
* PICA200 texture-environment alpha combiner commands

This lets the extractor answer the useful question for a 3D asset: which actual
texture slot supplies alpha, and which channel of that texture is consumed.
"""

from dataclasses import dataclass, asdict
import struct
from typing import Iterable


# H3D/BCH content-table sections.
SECTION_MODELS = 0
SECTION_MATERIAL_PARAMS = 1
SECTION_TEXTURES = 3

# PICA200 registers used by H3D material command streams.
GPUREG_TEXUNIT_CONFIG = 0x0080
GPUREG_TEXENV_SOURCE = (0x00C0, 0x00C8, 0x00D0, 0x00D8, 0x00F0, 0x00F8)
GPUREG_TEXENV_OPERAND = (0x00C1, 0x00C9, 0x00D1, 0x00D9, 0x00F1, 0x00F9)
GPUREG_TEXENV_COMBINER = (0x00C2, 0x00CA, 0x00D2, 0x00DA, 0x00F2, 0x00FA)
GPUREG_FRAGOP_ALPHA_TEST = 0x0104

SOURCE_NAMES = {
    0: 'PrimaryColor',
    1: 'FragmentPrimaryColor',
    2: 'FragmentSecondaryColor',
    3: 'Texture0',
    4: 'Texture1',
    5: 'Texture2',
    6: 'Texture3',
    13: 'PreviousBuffer',
    14: 'Constant',
    15: 'Previous',
}

ALPHA_OPERAND_NAMES = {
    0: 'Alpha',
    1: 'OneMinusAlpha',
    2: 'Red',
    3: 'OneMinusRed',
    4: 'Green',
    5: 'OneMinusGreen',
    6: 'Blue',
    7: 'OneMinusBlue',
}

COMBINER_NAMES = {
    0: 'Replace',
    1: 'Modulate',
    2: 'Add',
    3: 'AddSigned',
    4: 'Interpolate',
    5: 'Subtract',
    6: 'DotProduct3Rgb',
    7: 'DotProduct3Rgba',
    8: 'MultAdd',
    9: 'AddMult',
}

# Number of source operands consumed by each combiner mode.
COMBINER_ARITY = {
    0: 1,
    1: 2,
    2: 2,
    3: 2,
    4: 3,
    5: 2,
    6: 2,
    7: 2,
    8: 3,
    9: 3,
}


@dataclass(frozen=True)
class BCHHeader:
    backward_compat: int
    forward_compat: int
    version: int
    content_addr: int
    strings_addr: int
    commands_addr: int
    data_addr: int
    data_ext_addr: int
    reloc_addr: int
    content_len: int
    strings_len: int
    commands_len: int
    data_len: int
    data_ext_len: int
    reloc_len: int


@dataclass(frozen=True)
class Relocation:
    flags: int
    encoded_offset: int
    location: int


class BCHMaterialError(ValueError):
    pass


def _u32(data: bytes, off: int) -> int:
    if off < 0 or off + 4 > len(data):
        raise BCHMaterialError(f'u32 out of range at 0x{off:X}')
    return struct.unpack_from('<I', data, off)[0]


def parse_bch_header(data: bytes) -> BCHHeader:
    if len(data) < 0x38 or data[:4] != b'BCH\x00':
        raise BCHMaterialError('not a BCH payload')

    bc = data[4]
    fc = data[5]
    version = struct.unpack_from('<H', data, 6)[0]
    content = _u32(data, 0x08)
    strings = _u32(data, 0x0C)
    commands = _u32(data, 0x10)
    data_addr = _u32(data, 0x14)

    p = 0x18
    data_ext = 0
    data_ext_len = 0
    if bc > 0x20:
        data_ext = _u32(data, p)
        p += 4

    reloc = _u32(data, p); p += 4
    content_len = _u32(data, p); p += 4
    strings_len = _u32(data, p); p += 4
    commands_len = _u32(data, p); p += 4
    data_len = _u32(data, p); p += 4
    if bc > 0x20:
        data_ext_len = _u32(data, p)
        p += 4
    reloc_len = _u32(data, p)

    for label, value in (
        ('content', content), ('strings', strings), ('commands', commands),
        ('data', data_addr), ('reloc', reloc),
    ):
        if value and value >= len(data):
            raise BCHMaterialError(f'{label} section is outside BCH: 0x{value:X}')
    if reloc and reloc_len and reloc + reloc_len > len(data):
        raise BCHMaterialError('relocation table exceeds BCH length')

    return BCHHeader(
        bc, fc, version, content, strings, commands, data_addr, data_ext, reloc,
        content_len, strings_len, commands_len, data_len, data_ext_len, reloc_len,
    )


def _section(data: bytes, hdr: BCHHeader, index: int) -> tuple[int, int, int]:
    off = hdr.content_addr + index * 12
    if off + 12 > len(data):
        return 0, 0, 0
    return _u32(data, off), _u32(data, off + 4), _u32(data, off + 8)


def _resolve_main_offset(raw: int, hdr: BCHHeader, data_len: int) -> int | None:
    """Resolve an on-disk main-section pointer conservatively.

    H3D BCH stores these as main-section-relative offsets before relocation.
    Some already-relocated samples exist in tooling workflows, so absolute
    addresses are accepted as a fallback.
    """
    if raw == 0:
        return None
    candidates = (hdr.content_addr + raw, raw)
    upper = hdr.strings_addr if hdr.strings_addr > hdr.content_addr else data_len
    for value in candidates:
        if hdr.content_addr <= value < min(upper, data_len):
            return value
    for value in candidates:
        if 0 <= value < data_len:
            return value
    return None


def _resolve_string(data: bytes, hdr: BCHHeader, raw: int) -> str:
    # Offset 0 is a valid reference to the first string in the BCH string table.
    # Null-vs-offset-zero is disambiguated by the surrounding descriptor or relocation.
    candidates = (hdr.strings_addr + raw, raw)
    strings_end = min(
        len(data),
        hdr.strings_addr + hdr.strings_len if hdr.strings_len else len(data),
    )
    for pos in candidates:
        if not (hdr.strings_addr <= pos < strings_end):
            continue
        end = data.find(b'\x00', pos, min(strings_end, pos + 512))
        if end <= pos:
            continue
        blob = data[pos:end]
        try:
            text = blob.decode('utf-8')
        except UnicodeDecodeError:
            try:
                text = blob.decode('shift_jis')
            except UnicodeDecodeError:
                continue
        if text and all(ord(ch) >= 0x20 or ch in '\t\r\n' for ch in text):
            return text
    return ''


def parse_relocations(data: bytes, hdr: BCHHeader) -> list[Relocation]:
    out: list[Relocation] = []
    if not hdr.reloc_addr or not hdr.reloc_len:
        return out
    end = min(len(data), hdr.reloc_addr + hdr.reloc_len)
    for pos in range(hdr.reloc_addr, end - 3, 4):
        value = _u32(data, pos)
        flags = value >> 25
        encoded = value & 0x01FFFFFF
        if flags == 1:
            location = hdr.content_addr + encoded
        else:
            location = hdr.content_addr + encoded * 4
        if 0 <= location <= len(data) - 4:
            out.append(Relocation(flags, encoded, location))
    return out


def _pointer_table_entries(
    data: bytes,
    hdr: BCHHeader,
    section_index: int,
    max_count: int = 4096,
) -> tuple[list[int], int]:
    ptr_off, count, dict_off = _section(data, hdr, section_index)
    if not (0 < count <= max_count) or ptr_off == 0:
        return [], dict_off
    table = _resolve_main_offset(ptr_off, hdr, len(data))
    if table is None or table + count * 4 > len(data):
        return [], dict_off
    out: list[int] = []
    for i in range(count):
        raw = _u32(data, table + i * 4)
        resolved = _resolve_main_offset(raw, hdr, len(data))
        if resolved is not None:
            out.append(resolved)
    return out, dict_off


def _parse_dict_names(data: bytes, hdr: BCHHeader, dict_off: int, max_entries: int = 4096) -> list[str]:
    if not dict_off:
        return []
    base = _resolve_main_offset(dict_off, hdr, len(data))
    if base is None or base + 8 > len(data):
        return []
    count = _u32(data, base + 4)
    if not (0 < count <= max_entries):
        return []
    result: list[str] = []
    entry_start = base + 8
    for i in range(1, count + 1):
        off = entry_start + i * 16
        if off + 16 > len(data):
            break
        name = _resolve_string(data, hdr, _u32(data, off + 8))
        result.append(name)
    return result


def texture_descriptors(data: bytes, hdr: BCHHeader) -> list[dict]:
    ptrs, dict_off = _pointer_table_entries(data, hdr, SECTION_TEXTURES, max_count=2048)
    dict_names = _parse_dict_names(data, hdr, dict_off, 2048)
    out = []
    for index, desc in enumerate(ptrs):
        if desc + 32 > len(data):
            continue
        name = _resolve_string(data, hdr, _u32(data, desc + 28))
        if not name and index < len(dict_names):
            name = dict_names[index]
        out.append({'index': index, 'descriptor_offset': desc, 'name': name})
    return out


def _parse_gpu_commands(data: bytes, start: int, word_count: int) -> dict[int, int]:
    regs: dict[int, int] = {}
    if word_count <= 0:
        return regs
    end = min(len(data), start + word_count * 4)
    pos = start
    while pos + 8 <= end:
        param = _u32(data, pos)
        header = _u32(data, pos + 4)
        reg_id = header & 0xFFFF
        extra = (header >> 20) & 0xFF
        # PICA200 command bit 31 enables consecutive-register writes.
        # This matches Nintendo/3dbrew documentation and SPICA's reader.
        consecutive = bool(header & 0x80000000)
        pos += 8
        regs[reg_id] = param
        for i in range(extra):
            if pos + 4 > end:
                break
            value = _u32(data, pos)
            pos += 4
            regs[reg_id + 1 + i if consecutive else reg_id] = value
        if pos & 7:
            pos += 4
    return regs



_FORMAT_BPP = {
    0x0: 32,  # RGBA8
    0x1: 24,  # RGB8
    0x2: 16,  # RGBA5551
    0x3: 16,  # RGB565
    0x4: 16,  # RGBA4
    0x5: 16,  # LA8
    0x6: 16,  # HILO8
    0x7: 8,   # L8
    0x8: 8,   # A8
    0x9: 8,   # LA4
    0xA: 4,   # L4
    0xB: 4,   # A4
    0xC: 4,   # ETC1
    0xD: 8,   # ETC1A4
}

_TEXTURE_UNIT_REGS = (
    (0x0082, 0x008E, 0x0085),
    (0x0092, 0x0096, 0x0095),
    (0x009A, 0x009E, 0x009D),
)


def _texture_size(width: int, height: int, fmt: int) -> int:
    """Return the encoded PICA base-level size, including 8x8 tile padding."""
    bpp = _FORMAT_BPP.get(fmt, 0)
    if not bpp or width <= 0 or height <= 0:
        return 0
    storage_width = (width + 7) // 8 * 8
    storage_height = (height + 7) // 8 * 8
    return (storage_width * storage_height * bpp + 7) // 8


def _texture_info_from_regs(regs: dict[int, int], unit: int) -> dict | None:
    dim_reg, type_reg, addr_reg = _TEXTURE_UNIT_REGS[unit]
    if dim_reg not in regs or addr_reg not in regs:
        return None
    dim = regs[dim_reg]
    width = (dim >> 16) & 0x7FF
    height = dim & 0x7FF
    fmt = regs.get(type_reg, 0) & 0xF
    if not (4 <= width <= 4096 and 4 <= height <= 4096 and fmt in _FORMAT_BPP):
        return None
    return {
        'width': width,
        'height': height,
        'format': fmt,
        'raw_data_offset': regs[addr_reg],
        'unit': unit,
    }


def extract_bch_texture_infos(data: bytes) -> list[dict]:
    """Read H3D texture descriptors using our corrected PICA command parser.

    This intentionally replaces Texture Forge's BCH command interpretation for
    EOU model assets.  The third-party decoder remains useful for turning raw
    PICA texture bytes into RGBA, but descriptor discovery is kept here so the
    same verified command semantics are used for both textures and materials.
    """
    hdr = parse_bch_header(data)
    ptrs, dict_off = _pointer_table_entries(data, hdr, SECTION_TEXTURES, max_count=4096)
    dict_names = _parse_dict_names(data, hdr, dict_off, 4096)
    out: list[dict] = []

    for index, desc in enumerate(ptrs):
        if desc + 32 > len(data):
            continue
        name = _resolve_string(data, hdr, _u32(data, desc + 28))
        if not name and index < len(dict_names):
            name = dict_names[index]
        if not name:
            name = f'bch_tex_{index:04d}'

        info = None
        for unit in range(3):
            raw_cmd = _u32(data, desc + unit * 8)
            word_count = _u32(data, desc + unit * 8 + 4)
            regs = _command_block_at(data, hdr, raw_cmd, word_count)
            candidate = _texture_info_from_regs(regs, unit) if regs else None
            if candidate:
                info = candidate
                break
        if not info:
            continue

        width = int(info['width'])
        height = int(info['height'])
        fmt = int(info['format'])
        raw_offset = int(info['raw_data_offset'])
        size = _texture_size(width, height, fmt)
        if size <= 0:
            continue

        candidates = (hdr.data_addr + raw_offset, raw_offset)
        abs_offset = None
        for value in candidates:
            if 0 <= value and value + size <= len(data):
                abs_offset = value
                break
        if abs_offset is None:
            continue

        out.append({
            'index': index,
            'width': width,
            'height': height,
            'format': fmt,
            'data_offset': abs_offset,
            'raw_data_offset': raw_offset,
            'data_size': size,
            'mip_count': 1,
            'name': name,
            'texture_unit_descriptor': int(info['unit']),
            'descriptor_offset': desc,
        })
    return out

def _command_blocks_for_material(
    data: bytes,
    hdr: BCHHeader,
    relocs: Iterable[Relocation],
    start: int,
    end: int,
) -> list[dict]:
    blocks: list[dict] = []
    seen: set[tuple[int, int]] = set()
    for reloc in relocs:
        if reloc.flags != 2 or not (start <= reloc.location < end):
            continue
        ptr_loc = reloc.location
        if ptr_loc + 8 > len(data):
            continue
        raw_ptr = _u32(data, ptr_loc)
        count = _u32(data, ptr_loc + 4)
        if not (0 < count <= 0x4000):
            continue
        cmd_start = hdr.commands_addr + raw_ptr
        if not (hdr.commands_addr <= cmd_start < len(data)):
            # Accept an already relocated pointer as a fallback.
            cmd_start = raw_ptr
        if not (0 <= cmd_start <= len(data) - 8) or cmd_start + count * 4 > len(data):
            continue
        key = (cmd_start, count)
        if key in seen:
            continue
        seen.add(key)
        regs = _parse_gpu_commands(data, cmd_start, count)
        if not regs:
            continue
        kind = 'other'
        if GPUREG_TEXUNIT_CONFIG in regs:
            kind = 'texture_units'
        if any(reg in regs for reg in GPUREG_TEXENV_SOURCE):
            kind = 'fragment_shader'
        blocks.append({
            'pointer_location': ptr_loc,
            'command_offset': cmd_start,
            'word_count': count,
            'kind': kind,
            'registers': regs,
        })
    return blocks


def _decode_alpha_stages(regs: dict[int, int]) -> list[dict]:
    stages: list[dict] = []
    for stage in range(6):
        src_reg = GPUREG_TEXENV_SOURCE[stage]
        operand_reg = GPUREG_TEXENV_OPERAND[stage]
        combiner_reg = GPUREG_TEXENV_COMBINER[stage]
        if src_reg not in regs and combiner_reg not in regs:
            continue
        src = regs.get(src_reg, 0)
        operand = regs.get(operand_reg, 0)
        comb = regs.get(combiner_reg, 0)
        alpha_mode = (comb >> 16) & 0xF
        arity = COMBINER_ARITY.get(alpha_mode, 3)
        inputs = []
        for i in range(arity):
            source_id = (src >> (16 + i * 4)) & 0xF
            operand_id = (operand >> (12 + i * 4)) & 0x7
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
    return stages


def _alpha_texture_uses(stages: list[dict]) -> list[dict]:
    uses: list[dict] = []
    for stage in stages:
        for inp in stage['inputs']:
            source_id = int(inp['source_id'])
            if 3 <= source_id <= 5:
                uses.append({
                    'stage': stage['stage'],
                    'combiner': stage['combiner'],
                    'input': inp['input'],
                    'slot': source_id - 3,
                    'source': inp['source'],
                    'operand': inp['operand'],
                    'operand_id': inp['operand_id'],
                })
    return uses


def _command_block_at(data: bytes, hdr: BCHHeader, raw_ptr: int, word_count: int) -> dict[int, int]:
    """Parse one commands-section pointer/length pair from an H3D object."""
    if not (0 < word_count <= 0x4000):
        return {}
    candidates = (hdr.commands_addr + raw_ptr, raw_ptr)
    for cmd_start in candidates:
        if not (hdr.commands_addr <= cmd_start < len(data)) and cmd_start != raw_ptr:
            continue
        if not (0 <= cmd_start <= len(data) - 8):
            continue
        if cmd_start + word_count * 4 > len(data):
            continue
        regs = _parse_gpu_commands(data, cmd_start, word_count)
        if regs:
            return regs
    return {}


def _main_object_end(start: int, object_starts: list[int], hdr: BCHHeader, data_len: int) -> int:
    later = [value for value in object_starts if value > start]
    main_candidates = [
        value for value in (hdr.strings_addr, hdr.commands_addr, hdr.data_addr)
        if value > start
    ]
    candidates = [*later, *main_candidates, data_len]
    return min(candidates) if candidates else data_len


def _model_name(data: bytes, hdr: BCHHeader, model_start: int, fallback: str) -> str:
    """Read H3DModel._Name from the stable pre-0x21/0x21+ header layout."""
    # Ohana3DS and SPICA agree on the leading H3DModel layout. For BCH
    # compatibility >= 7, the extra mesh-layer/submesh-culling list fields put
    # _Name at +0x84. Older BCH revisions use a shorter block.
    offsets = [0x84] if hdr.backward_compat > 6 else [0x7C, 0x84]
    for rel in offsets:
        if model_start + rel + 4 > len(data):
            continue
        text = _resolve_string(data, hdr, _u32(data, model_start + rel))
        if text:
            return text
    return fallback


def _model_material_table(data: bytes, hdr: BCHHeader, model_start: int) -> tuple[int | None, int, int]:
    """Return (table, count, record_size) for H3DModel.Materials.

    H3DModel begins with flags/bone-scaling/silhouette count (4 bytes), a
    Matrix3x4 (48 bytes), then the inline H3DDict<H3DMaterial>. Ohana3DS
    documents the material list pointer/count at +0x34/+0x38. H3DMaterial is
    [Inline], so its records are contiguous: 0x58 bytes before compatibility
    0x21 and 0x2c bytes from 0x21 onward.
    """
    if model_start + 0x3C > len(data):
        return None, 0, 0
    raw_table = _u32(data, model_start + 0x34)
    count = _u32(data, model_start + 0x38)
    if not (0 < count <= 2048):
        return None, 0, 0
    table = _resolve_main_offset(raw_table, hdr, len(data))
    record_size = 0x58 if hdr.backward_compat < 0x21 else 0x2C
    if table is None or table + count * record_size > len(data):
        return None, 0, record_size
    return table, count, record_size


def _material_texture_names(data: bytes, hdr: BCHHeader, material_start: int) -> tuple[list[dict], str]:
    """Read Texture0Name/1Name/2Name and H3DMaterial.Name from a model material."""
    names_off = 0x48 if hdr.backward_compat < 0x21 else 0x1C
    slots: list[dict] = []
    for slot in range(3):
        field = material_start + names_off + slot * 4
        if field + 4 > len(data):
            break
        raw = _u32(data, field)
        name = _resolve_string(data, hdr, raw)
        if name:
            slots.append({
                'slot': slot,
                'texture_name': name,
                'string_pointer_location': field,
            })
    material_name = ''
    name_field = material_start + names_off + 12
    if name_field + 4 <= len(data):
        material_name = _resolve_string(data, hdr, _u32(data, name_field))
    return slots, material_name


def _fragment_registers_for_params(
    data: bytes,
    hdr: BCHHeader,
    relocs: list[Relocation],
    params_start: int | None,
    params_starts: list[int],
) -> tuple[dict[int, int], list[dict]]:
    if params_start is None:
        return {}, []
    params_end = _main_object_end(params_start, params_starts, hdr, len(data))
    blocks = _command_blocks_for_material(data, hdr, relocs, params_start, params_end)
    fragment: dict[int, int] = {}
    for block in blocks:
        regs = block['registers']
        if any(
            reg in regs
            for reg in (*GPUREG_TEXENV_SOURCE, *GPUREG_TEXENV_OPERAND,
                        *GPUREG_TEXENV_COMBINER, GPUREG_FRAGOP_ALPHA_TEST)
        ):
            fragment.update(regs)
    return fragment, blocks


def parse_bch_materials(data: bytes) -> dict:
    """Parse the *actual model materials* and their PICA alpha pipeline.

    The global H3D content-table material section contains H3DMaterialParams,
    not the per-model H3DMaterial objects that own Texture0Name/1Name/2Name.
    Consequently we walk:

        H3D Models -> model.Materials -> texture names / texture commands
                   -> referenced H3DMaterialParams -> fragment commands

    This is the relationship the game uses for 3D model rendering and avoids
    the v0.4 mistake of inferring alpha from decoded image appearance.
    """
    hdr = parse_bch_header(data)
    relocs = parse_relocations(data, hdr)
    tex_descs = texture_descriptors(data, hdr)
    known_texture_names = {
        str(texture.get('name') or '')
        for texture in tex_descs
        if str(texture.get('name') or '')
    }

    model_ptrs, model_dict_off = _pointer_table_entries(data, hdr, SECTION_MODELS, max_count=1024)
    model_dict_names = _parse_dict_names(data, hdr, model_dict_off, 1024)
    params_ptrs, _params_dict_off = _pointer_table_entries(
        data, hdr, SECTION_MATERIAL_PARAMS, max_count=4096
    )
    params_starts = sorted(set(params_ptrs))

    materials: list[dict] = []
    models: list[dict] = []

    for model_index, model_start in enumerate(model_ptrs):
        fallback_model_name = (
            model_dict_names[model_index]
            if model_index < len(model_dict_names) and model_dict_names[model_index]
            else f'model_{model_index:03d}'
        )
        model_name = _model_name(data, hdr, model_start, fallback_model_name)
        table, count, record_size = _model_material_table(data, hdr, model_start)
        model_row = {
            'index': model_index,
            'name': model_name,
            'record_offset': model_start,
            'material_table_offset': table,
            'material_count': count,
            'material_record_size': record_size,
        }
        models.append(model_row)
        if table is None or count <= 0:
            continue

        for local_index in range(count):
            material_start = table + local_index * record_size
            if material_start + record_size > len(data):
                continue

            # H3DMaterial begins with a pointer to its H3DMaterialParams.
            raw_params = _u32(data, material_start)
            params_start = _resolve_main_offset(raw_params, hdr, len(data))
            if params_start is not None and params_starts:
                # Prefer an exact top-level parameter object. If the pointer was
                # already relocated or uses an unexpected encoding, match only
                # when it resolves to a known object; otherwise preserve it as
                # unresolved rather than scanning an unrelated main object.
                if params_start not in params_starts:
                    alt = raw_params if raw_params in params_starts else None
                    params_start = alt

            slots, material_name = _material_texture_names(data, hdr, material_start)
            if not material_name:
                material_name = f'{model_name}_material_{local_index:03d}'

            # The H3DMaterial's own TextureCommands govern enabled Texture0/1/2.
            raw_tex_cmd = _u32(data, material_start + 0x10)
            tex_cmd_count = _u32(data, material_start + 0x14)
            texture_regs = _command_block_at(data, hdr, raw_tex_cmd, tex_cmd_count)
            texunit_param = texture_regs.get(GPUREG_TEXUNIT_CONFIG, 0)
            enabled = [
                bool(texunit_param & 0x001),
                bool(texunit_param & 0x002),
                bool(texunit_param & 0x004),
            ]
            for slot in slots:
                slot_index = int(slot['slot'])
                slot['enabled'] = enabled[slot_index]
                slot['known_bch_texture'] = slot['texture_name'] in known_texture_names

            fragment_regs, params_blocks = _fragment_registers_for_params(
                data, hdr, relocs, params_start, params_starts
            )
            stages = _decode_alpha_stages(fragment_regs)
            uses = _alpha_texture_uses(stages)

            alpha_test_param = fragment_regs.get(GPUREG_FRAGOP_ALPHA_TEST)
            alpha_test = None
            if alpha_test_param is not None:
                alpha_test = {
                    'enabled': bool(alpha_test_param & 1),
                    'function': (alpha_test_param >> 4) & 7,
                    'reference': (alpha_test_param >> 8) & 0xFF,
                    'raw': alpha_test_param,
                }

            materials.append({
                'index': len(materials),
                'model_index': model_index,
                'model_name': model_name,
                'model_material_index': local_index,
                'name': material_name,
                'record_offset': material_start,
                'record_end': material_start + record_size,
                'material_params_offset': params_start,
                'texture_slots': slots,
                'enabled_texture_slots': [i for i, value in enumerate(enabled) if value],
                'alpha_stages': stages,
                'alpha_texture_uses': uses,
                'alpha_test': alpha_test,
                'texture_command_word_count': tex_cmd_count,
                'parameter_command_blocks': [
                    {k: v for k, v in block.items() if k != 'registers'}
                    for block in params_blocks
                ],
            })

    return {
        'header': asdict(hdr),
        'textures': tex_descs,
        'models': models,
        'materials': materials,
        'model_count': len(models),
        'material_params_count': len(params_ptrs),
        'relocation_count': len(relocs),
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
            slot_index = int(slot.get('slot', 0))
            by_name.setdefault(name, []).append({
                'material_index': material.get('index'),
                'material_name': material.get('name'),
                'model_index': material.get('model_index'),
                'model_name': material.get('model_name'),
                'model_material_index': material.get('model_material_index'),
                'slot': slot_index,
                'enabled': bool(slot.get('enabled')),
                'alpha_uses': uses_by_slot.get(slot_index, []),
                'alpha_stages': material.get('alpha_stages', []),
                'alpha_test': material.get('alpha_test'),
            })
    return by_name
