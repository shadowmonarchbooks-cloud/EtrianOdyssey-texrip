import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.bch_materials import parse_bch_materials, bindings_by_texture, extract_bch_texture_infos


def _cmd(param: int, reg: int) -> bytes:
    return struct.pack('<II', param, reg)


def build_synthetic_bch() -> bytes:
    """Build an EOU-era (<0x21) BCH with one model and one real model material.

    The important distinction is intentional: content section 1 contains the
    H3DMaterialParams object, while Texture0Name/Texture1Name live in the
    H3DMaterial record inside H3DModel.Materials.
    """
    content = 0x40
    strings = 0x300
    commands = 0x380
    data_addr = 0x480
    reloc = 0x500
    size = 0x600
    b = bytearray(size)
    b[:4] = b'BCH\0'
    b[4] = 0x20
    b[5] = 0x20
    struct.pack_into('<H', b, 6, 1)
    struct.pack_into('<IIII', b, 0x08, content, strings, commands, data_addr)
    # bc <= 0x20: relocation immediately follows data_addr.
    struct.pack_into('<I', b, 0x18, reloc)
    struct.pack_into(
        '<IIII', b, 0x1C,
        strings - content, commands - strings, data_addr - commands, reloc - data_addr,
    )

    # Content sections: Models(0), MaterialParams(1), Textures(3).
    model_table_rel = 0xA0
    params_table_rel = 0xA4
    tex_table_rel = 0xA8
    struct.pack_into('<III', b, content + 0 * 12, model_table_rel, 1, 0)
    struct.pack_into('<III', b, content + 1 * 12, params_table_rel, 1, 0)
    struct.pack_into('<III', b, content + 3 * 12, tex_table_rel, 2, 0)

    model_rel = 0xC0
    params_rel = 0x160
    tex0_rel = 0x1B0
    tex1_rel = 0x1D0
    material_table_rel = 0x200

    struct.pack_into('<I', b, content + model_table_rel, model_rel)
    struct.pack_into('<I', b, content + params_table_rel, params_rel)
    struct.pack_into('<II', b, content + tex_table_rel, tex0_rel, tex1_rel)

    model = content + model_rel
    params = content + params_rel
    tex0 = content + tex0_rel
    tex1 = content + tex1_rel
    material = content + material_table_rel

    body_off = 0x10
    mask_off = 0x20
    mat_name_off = 0x30
    model_name_off = 0x40
    b[strings + body_off:strings + body_off + 5] = b'body\0'
    b[strings + mask_off:strings + mask_off + 5] = b'mask\0'
    b[strings + mat_name_off:strings + mat_name_off + 9] = b'body_mat\0'
    b[strings + model_name_off:strings + model_name_off + 8] = b'enemy01\0'

    # Global H3D texture descriptors. Use bit31 consecutive writes for
    # DIM/PARAM/LOD/ADDR so the test also guards the real 3D-ripping path.
    def texture_gpu_block(width: int, height: int, raw_addr: int, fmt: int) -> bytes:
        dim = (width << 16) | height
        consecutive_header = 0x80000000 | (3 << 20) | 0x0082
        # 5 words become 6 after required 8-byte command padding, then TYPE.
        return b''.join([
            struct.pack('<IIIII', dim, consecutive_header, 0, 0, raw_addr),
            struct.pack('<I', 0),
            _cmd(fmt, 0x008E),
        ])

    tex0_cmd_rel = 0x80
    tex1_cmd_rel = 0xA0
    tex0_cmd = texture_gpu_block(8, 8, 0x00, 0x0C)
    tex1_cmd = texture_gpu_block(8, 8, 0x20, 0x0C)
    b[commands + tex0_cmd_rel:commands + tex0_cmd_rel + len(tex0_cmd)] = tex0_cmd
    b[commands + tex1_cmd_rel:commands + tex1_cmd_rel + len(tex1_cmd)] = tex1_cmd
    struct.pack_into('<II', b, tex0 + 0, tex0_cmd_rel, len(tex0_cmd) // 4)
    struct.pack_into('<II', b, tex1 + 0, tex1_cmd_rel, len(tex1_cmd) // 4)
    struct.pack_into('<I', b, tex0 + 28, body_off)
    struct.pack_into('<I', b, tex1 + 28, mask_off)
    # Two tiny ETC1 payloads in the raw-data section.
    b[data_addr:data_addr + 0x20] = bytes(range(0x20))
    b[data_addr + 0x20:data_addr + 0x40] = bytes(reversed(range(0x20)))

    # H3DModel leading layout: material H3DDict at +0x34/+0x38/+0x3c.
    struct.pack_into('<I', b, model + 0x34, material_table_rel)
    struct.pack_into('<I', b, model + 0x38, 1)
    struct.pack_into('<I', b, model + 0x3C, 0)
    struct.pack_into('<I', b, model + 0x84, model_name_off)

    # H3DMaterial (<0x21 is 0x58 bytes):
    # +00 H3DMaterialParams pointer
    # +10 TextureCommands pointer, +14 word count
    # +18..+47 inline legacy texture mapper block
    # +48/+4c/+50 Texture0/1/2Name, +54 material name.
    struct.pack_into('<I', b, material + 0x00, params_rel)
    texture_cmd_rel = 0x00
    texture_commands = _cmd(0x00000003, 0x0080)  # Texture0 + Texture1 enabled.
    b[commands + texture_cmd_rel:commands + texture_cmd_rel + len(texture_commands)] = texture_commands
    struct.pack_into('<II', b, material + 0x10, texture_cmd_rel, len(texture_commands) // 4)
    struct.pack_into('<IIII', b, material + 0x48, body_off, mask_off, 0, mat_name_off)

    # H3DMaterialParams owns the fragment command pointer. Put it at a
    # synthetic location inside the params object and identify it through the
    # BCH command-pointer relocation entry (flag 2).
    fragment_cmd_rel = 0x40
    source_param = 4 << 16   # alpha source input0 = Texture1
    operand_param = 2 << 12  # alpha operand input0 = Red
    combiner_param = 0       # Replace
    fragment_commands = b''.join([
        _cmd(source_param, 0x00C0),
        _cmd(operand_param, 0x00C1),
        _cmd(combiner_param, 0x00C2),
        _cmd(1 | (6 << 4) | (0x40 << 8), 0x0104),
    ])
    b[commands + fragment_cmd_rel:commands + fragment_cmd_rel + len(fragment_commands)] = fragment_commands
    struct.pack_into('<II', b, params + 0x20, fragment_cmd_rel, len(fragment_commands) // 4)

    rels = [
        (2 << 25) | (((params + 0x20) - content) // 4),
    ]
    struct.pack_into('<I', b, 0x2C, len(rels) * 4)
    for i, value in enumerate(rels):
        struct.pack_into('<I', b, reloc + i * 4, value)
    return bytes(b)


def test_material_parser_identifies_texture1_red_as_alpha():
    report = parse_bch_materials(build_synthetic_bch())
    assert report['model_count'] == 1
    assert len(report['materials']) == 1
    material = report['materials'][0]
    assert material['model_name'] == 'enemy01'
    assert material['name'] == 'body_mat'
    assert [x['texture_name'] for x in material['texture_slots']] == ['body', 'mask']
    assert material['enabled_texture_slots'] == [0, 1]
    assert material['alpha_texture_uses'] == [{
        'stage': 0,
        'combiner': 'Replace',
        'input': 0,
        'slot': 1,
        'source': 'Texture1',
        'operand': 'Red',
        'operand_id': 2,
    }]
    by_tex = bindings_by_texture(report)
    assert by_tex['body'][0]['alpha_uses'] == []
    assert by_tex['mask'][0]['model_name'] == 'enemy01'
    assert by_tex['mask'][0]['alpha_uses'][0]['operand'] == 'Red'
    assert material['alpha_test']['enabled'] is True
    assert material['alpha_test']['reference'] == 0x40


def test_pica_bit31_writes_extra_parameters_to_consecutive_registers():
    from eouhd.bch_materials import _parse_gpu_commands

    # PICA command: first parameter at TEXENV0_SOURCE and two extra parameters
    # at the following OPERAND and COMBINER registers. Bit 31 means the target
    # register increments for each extra parameter.
    source = 4 << 16
    operand = 2 << 12
    combiner = 0
    header = 0x80000000 | (2 << 20) | 0x00C0
    # 5 words total after 8-byte padding: source/header/operand/combiner/pad.
    raw = struct.pack('<IIIII', source, header, operand, combiner, 0)
    regs = _parse_gpu_commands(raw, 0, 4)

    assert regs[0x00C0] == source
    assert regs[0x00C1] == operand
    assert regs[0x00C2] == combiner


def test_local_bch_texture_descriptor_parser_handles_consecutive_commands():
    infos = extract_bch_texture_infos(build_synthetic_bch())
    assert [(x['name'], x['width'], x['height'], x['format']) for x in infos] == [
        ('body', 8, 8, 0x0C),
        ('mask', 8, 8, 0x0C),
    ]
    assert infos[0]['data_offset'] == 0x480
    assert infos[1]['data_offset'] == 0x4A0
