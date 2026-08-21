import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.eou_stex import (
    parse_eou_stex,
    DT_UNSIGNED_BYTE,
    PF_ETC1,
    FMT_ETC1,
)
from eouhd.strict_scan import has_strict_texture_signature, _embedded_bch_offsets


def _minimal_bch(data_addr: int = 0) -> bytes:
    """Version-aware valid BCH header; data_addr=0 models external-texture BCH."""
    b = bytearray(0x100)
    b[:4] = b'BCH\0'
    b[4] = 0x20
    b[5] = 0x20
    struct.pack_into('<H', b, 6, 1)
    # content, strings, commands, data. A zero data section is legal for a
    # model whose textures are supplied externally.
    struct.pack_into('<IIII', b, 0x08, 0x40, 0x80, 0xA0, data_addr)
    struct.pack_into('<I', b, 0x18, 0xE0)  # relocation
    struct.pack_into('<IIIII', b, 0x1C, 0x40, 0x20, 0x40, 0, 0x20)
    return bytes(b)


def test_eo2u_atbc_bam2_with_bch_is_strict_candidate_without_cgfx():
    blob = b'ATBC' + b'\0' * 0x7C + _minimal_bch(0)
    assert b'CGFX' not in blob
    assert has_strict_texture_signature(blob, '.bam2') is True
    assert _embedded_bch_offsets(blob) == [0x80]


def test_stex_declared_size_may_exceed_available_when_base_level_is_complete():
    # 64x64 ETC1 = 2048 bytes. EO effect STEX files can report a much larger
    # NumImageBytes but physically store only the usable base-level payload.
    width = height = 64
    base = 64 * 64 // 2
    hdr = bytearray(0x80)
    hdr[:4] = b'STEX'
    struct.pack_into('<I', hdr, 0x0C, width)
    struct.pack_into('<I', hdr, 0x10, height)
    struct.pack_into('<I', hdr, 0x14, DT_UNSIGNED_BYTE)
    struct.pack_into('<I', hdr, 0x18, PF_ETC1)
    struct.pack_into('<I', hdr, 0x1C, base * 4 + 123)  # intentionally beyond EOF
    struct.pack_into('<I', hdr, 0x20, 0x80)
    payload = bytes((i * 13) & 0xFF for i in range(base))
    st = parse_eou_stex(bytes(hdr) + payload)
    assert st.pica_format == FMT_ETC1
    assert len(st.raw) == base
    assert st.data_size_declared > len(st.raw)


def test_streamlined_cleanup_keeps_small_diagnostics(tmp_path):
    from eouhd.workspace import ensure_workspace, cleanup_streamlined_workspace
    dirs = ensure_workspace(tmp_path)
    sample = dirs['diagnostics'] / 'sample.bin'
    sample.write_bytes(b'diagnostic')
    transient = dirs['work'] / 'large.tmp'
    transient.parent.mkdir(parents=True, exist_ok=True)
    transient.write_bytes(b'x' * 1024)
    cleanup_streamlined_workspace(tmp_path)
    assert sample.read_bytes() == b'diagnostic'
    assert not dirs['work'].exists()
