import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.cgfx_materials import (
    atbc_info,
    bindings_by_texture,
    find_cgfx_payloads,
    parse_cgfx_materials,
)
from eouhd.strict_scan import has_strict_texture_signature


def _self_ptr(buf: bytearray, field: int, target: int) -> None:
    struct.pack_into('<I', buf, field, target - field)


def _put_cstr(buf: bytearray, offset: int, value: str) -> None:
    raw = value.encode('ascii') + b'\0'
    buf[offset:offset + len(raw)] = raw


def make_cgfx() -> bytes:
    size = 0x1400
    b = bytearray(size)
    b[0:4] = b'CGFX'
    b[4:6] = b'\xff\xfe'
    struct.pack_into('<H', b, 6, 0x14)
    struct.pack_into('<I', b, 0x0C, size)

    # Minimal CMDL identity used by the structural scanner.
    cmdl = 0x100
    struct.pack_into('<I', b, cmdl, 0x40000012)
    b[cmdl + 4:cmdl + 8] = b'CMDL'
    struct.pack_into('<I', b, cmdl + 8, 0x09000000)
    _self_ptr(b, cmdl + 0x0C, 0x1200)
    _put_cstr(b, 0x1200, 'synthetic_model')

    # One embedded ImageTexture TXOB called body_tex.
    tx = 0x300
    struct.pack_into('<I', b, tx, 0x20000011)
    b[tx + 4:tx + 8] = b'TXOB'
    struct.pack_into('<I', b, tx + 8, 0x05000000)
    _self_ptr(b, tx + 0x0C, 0x1220)
    _put_cstr(b, 0x1220, 'body_tex')
    struct.pack_into('<II', b, tx + 0x18, 8, 8)  # height, width
    struct.pack_into('<I', b, tx + 0x28, 1)      # mip levels
    struct.pack_into('<I', b, tx + 0x34, 12)     # ETC1
    image = 0x400
    _self_ptr(b, tx + 0x38, image)
    struct.pack_into('<III', b, image, 8, 8, 32)
    _self_ptr(b, image + 0x0C, 0x500)
    b[0x500:0x520] = bytes(range(32))

    # One MTOB that binds Texture0 to a ReferenceTexture named body_tex.
    mt = 0x600
    struct.pack_into('<I', b, mt, 0x08000000)
    b[mt + 4:mt + 8] = b'MTOB'
    struct.pack_into('<I', b, mt + 8, 0x06000003)
    _self_ptr(b, mt + 0x0C, 0x1240)
    _put_cstr(b, 0x1240, 'body')

    texinfo = 0x900
    _self_ptr(b, mt + 0x274, texinfo)
    struct.pack_into('<I', b, texinfo, 0x80000000)
    ref = 0x980
    _self_ptr(b, texinfo + 0x08, ref)
    struct.pack_into('<I', b, ref, 0x20000004)
    b[ref + 4:ref + 8] = b'TXOB'
    struct.pack_into('<I', b, ref + 8, 0x05000000)
    _self_ptr(b, ref + 0x18, 0x1260)
    _put_cstr(b, 0x1260, 'body_tex')

    # Fragment shader: stage0 Replace(Texture0.Red), stages1-5 Previous.Alpha.
    frag = 0xA00
    _self_ptr(b, mt + 0x288, frag)
    base = frag + 0x2C
    # stage 0: SrcAlpha input0=Texture0(3), operand alpha bits use Red(2)
    struct.pack_into('<H', b, base + 0x06, 3)
    struct.pack_into('<I', b, base + 0x0C, 2 << 12)
    struct.pack_into('<H', b, base + 0x12, 0)  # Replace
    for stage in range(1, 6):
        off = base + stage * 0x1C
        struct.pack_into('<H', b, off + 0x06, 15)  # Previous
        struct.pack_into('<I', b, off + 0x0C, 0)
        struct.pack_into('<H', b, off + 0x12, 0)
    struct.pack_into('<I', b, base + 6 * 0x1C, 0x00004061)  # enabled, Greater, ref=64
    return bytes(b)


def test_atbc_probe_accepts_truncated_declared_cgfx():
    cgfx = make_cgfx()
    bam = b'ATBC' + bytes(0x17C) + cgfx
    probe = bam[:0x1000]
    assert find_cgfx_payloads(probe) == []
    assert find_cgfx_payloads(probe, allow_truncated=True) == [(0x180, len(cgfx))]
    assert has_strict_texture_signature(probe, '.bam')
    assert atbc_info(bam)['cgfx_covers_remainder'] is True


def test_cgfx_mtob_reference_texture_and_alpha_channel():
    report = parse_cgfx_materials(make_cgfx())
    assert report['model_count'] == 1
    assert report['models'][0]['name'] == 'synthetic_model'
    assert [(t['name'], t['width'], t['height'], t['format']) for t in report['textures']] == [
        ('body_tex', 8, 8, 12)
    ]
    assert len(report['materials']) == 1
    material = report['materials'][0]
    assert material['name'] == 'body'
    assert material['texture_slots'][0]['texture_name'] == 'body_tex'
    assert material['alpha_texture_uses'] == [{
        'stage': 0,
        'combiner': 'Replace',
        'input': 0,
        'slot': 0,
        'source': 'Texture0',
        'operand': 'Red',
        'operand_id': 2,
    }]
    assert material['alpha_test']['enabled'] is True
    bindings = bindings_by_texture(report)
    assert bindings['body_tex'][0]['material_format'] == 'CGFX/MTOB'
    assert bindings['body_tex'][0]['alpha_uses'][0]['operand'] == 'Red'


def test_existing_workspace_can_rehydrate_cgfx_bindings(tmp_path):
    from eouhd.materials import rehydrate_material_bindings
    cgfx = make_cgfx()
    source = tmp_path / '03_hpx_unpacked' / 'enemy001.bam'
    source.parent.mkdir(parents=True)
    source.write_bytes(b'ATBC' + bytes(0x17C) + cgfx)
    assets = [{
        'asset_id': 'mon_TEST',
        'parser_used': 'cgfx_struct',
        'source': str(source),
        'container_offset': 0x180,
        'texture_name': 'body_tex',
        'material_bindings': [],
    }]
    result = rehydrate_material_bindings(tmp_path, assets)
    assert result['models_reparsed'] == 1
    assert result['bindings_added'] == 1
    assert assets[0]['material_bindings'][0]['material_format'] == 'CGFX/MTOB'
    assert assets[0]['material_bindings'][0]['alpha_uses'][0]['operand'] == 'Red'
