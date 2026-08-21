import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.epl import parse_epl, unpack_epl
from eouhd.pipeline import _strict_candidates
from eouhd.eou_stex import DT_UNSIGNED_BYTE, PF_ETC1


def make_stex(width=8, height=8):
    payload_size = width * height // 2
    hdr = bytearray(0x80)
    hdr[:4] = b'STEX'
    struct.pack_into('<I', hdr, 0x0C, width)
    struct.pack_into('<I', hdr, 0x10, height)
    struct.pack_into('<I', hdr, 0x14, DT_UNSIGNED_BYTE)
    struct.pack_into('<I', hdr, 0x18, PF_ETC1)
    struct.pack_into('<I', hdr, 0x1C, payload_size)
    struct.pack_into('<I', hdr, 0x20, 0x80)
    hdr[0x28:0x31] = b'fx_tex\0\0\0'
    return bytes(hdr) + bytes(range(payload_size))


def make_epl(member: bytes, name: str = 'effect_tex') -> bytes:
    data_start = 0x90
    record = data_start
    table_offset = 0x160
    payload_offset = 0x190
    total = payload_offset + len(member)
    b = bytearray(total)
    # Public AtlusLibSharp EPL layout.
    struct.pack_into('<iii', b, 0x80, 1, 0, data_start)
    struct.pack_into('<i', b, record + 0x90, table_offset)
    raw_name = name.encode('ascii')[:35] + b'\0'
    b[record + 0x9C: record + 0x9C + len(raw_name)] = raw_name
    struct.pack_into('<ii', b, table_offset + 0x20, payload_offset - table_offset, len(member))
    b[payload_offset:payload_offset + len(member)] = member
    return bytes(b)


def test_parse_epl_resource_table():
    payload = make_stex()
    entries, meta = parse_epl(make_epl(payload))
    assert meta['file_count'] == 1
    assert entries[0].name == 'effect_tex'
    assert entries[0].data_size == len(payload)
    assert entries[0].magic_ascii == 'STEX'


def test_unpack_epl_exposes_stex_to_strict_pipeline(tmp_path: Path):
    source = tmp_path / 'effect.epl'
    source.write_bytes(make_epl(make_stex()))
    out = tmp_path / 'unpacked'
    written, meta = unpack_epl(source, out)
    assert len(written) == 1
    assert meta['known_texture_members'] == 1
    assert written[0].suffix.lower() == '.stex'
    assert written[0] in _strict_candidates([out])
