from pathlib import Path
import struct

from eouhd.farc import parse_farc, unpack_farc, find_farc_files
from eouhd.pipeline import _strict_candidates


def _make_hash_farc(payload: bytes, name_hash: int = 0x12345678) -> bytes:
    sir0_offset = 0x40
    sir0_length = 0x50
    data_offset = 0x90
    sir0 = bytearray(sir0_length)
    sir0[:4] = b'SIR0'
    struct.pack_into('<II', sir0, 4, 0x10, 0x40)
    # format-specific FARC FAT header
    struct.pack_into('<III', sir0, 0x10, 0x1C, 1, 1)
    struct.pack_into('<III', sir0, 0x1C, name_hash, 0, len(payload))
    out = bytearray(data_offset + len(payload))
    out[:4] = b'FARC'
    struct.pack_into('<IIIII', out, 0x20, 5, sir0_offset, sir0_length, data_offset, len(payload))
    out[sir0_offset:sir0_offset+sir0_length] = sir0
    out[data_offset:] = payload
    return bytes(out)


def _make_named_farc(payload: bytes, filename: str = 'enemy_test.bam') -> bytes:
    sir0_offset = 0x40
    sir0_length = 0x80
    data_offset = 0xC0
    sir0 = bytearray(sir0_length)
    sir0[:4] = b'SIR0'
    struct.pack_into('<II', sir0, 4, 0x10, 0x70)
    # type 0 entries store a UTF-16LE filename pointer in the first u32
    struct.pack_into('<III', sir0, 0x10, 0x1C, 1, 0)
    name_off = 0x30
    struct.pack_into('<III', sir0, 0x1C, name_off, 0, len(payload))
    encoded = filename.encode('utf-16le') + b'\0\0'
    sir0[name_off:name_off+len(encoded)] = encoded
    out = bytearray(data_offset + len(payload))
    out[:4] = b'FARC'
    struct.pack_into('<IIIII', out, 0x20, 4, sir0_offset, sir0_length, data_offset, len(payload))
    out[sir0_offset:sir0_offset+sir0_length] = sir0
    out[data_offset:] = payload
    return bytes(out)


def _bam2_like_payload() -> bytes:
    # Enough BCH metadata to pass strict embedded-BCH signature validation.
    bch = bytearray(0x300)
    bch[:4] = b'BCH\x00'
    struct.pack_into('<IIII', bch, 0x08, 0x40, 0x80, 0x100, 0x200)
    return b'BAM2' + b'\0' * 0x3C + bytes(bch)


def test_parse_hash_indexed_farc():
    payload = _bam2_like_payload()
    data = _make_hash_farc(payload)
    entries, meta = parse_farc(data)
    assert meta['file_count'] == 1
    assert meta['filename_mode'] == 1
    assert entries[0].name is None
    assert entries[0].name_hash == 0x12345678
    assert data[entries[0].absolute_offset:entries[0].absolute_offset+entries[0].data_length] == payload


def test_unpack_farc_exposes_embedded_bch_candidate(tmp_path: Path):
    source = tmp_path / 'monster_graphic.bin'
    source.write_bytes(_make_hash_farc(_bam2_like_payload()))
    extracted = tmp_path / 'out'
    written, meta = unpack_farc(source, extracted)
    assert meta['file_count'] == 1
    assert len(written) == 1
    assert written[0].read_bytes().startswith(b'BAM2')
    assert written[0].suffix == '.bchbin'
    assert written[0] in _strict_candidates([extracted])


def test_named_farc_preserves_member_name(tmp_path: Path):
    source = tmp_path / 'named.farc'
    source.write_bytes(_make_named_farc(b'BCH\x00' + b'\0' * 0x300, 'enemy_001.bam'))
    written, meta = unpack_farc(source, tmp_path / 'named_out')
    assert meta['filename_mode'] == 0
    assert written[0].name == 'enemy_001.bam'


def test_find_farc_uses_magic_not_extension(tmp_path: Path):
    source = tmp_path / 'unknown.bin'
    source.write_bytes(_make_hash_farc(b'hello'))
    assert list(find_farc_files(tmp_path)) == [source]


def test_romfs_prefilter_keeps_farc_magic():
    from eouhd.forge_bridge import _romfs_candidate
    assert _romfs_candidate('/romfs/monster_graphic.bin', b'FARC' + b'\0' * 64)
