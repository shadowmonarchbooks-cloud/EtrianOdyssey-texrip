from pathlib import Path
import struct

import pytest

from eouhd.hpx import HPXError, find_hpi_pairs, unpack_hpi_pair


def test_find_hpi_pair_is_case_insensitive(tmp_path: Path):
    (tmp_path / 'MORI1R.HPI').write_bytes(b'HPIH' + b'\0' * 32)
    (tmp_path / 'MORI1R.HPB').write_bytes(b'')
    assert [p.name for p in find_hpi_pairs(tmp_path)] == ['MORI1R.HPI']


def _write_uncompressed_pair(root: Path, filename: str, payload: bytes, declared_size: int | None = None) -> Path:
    index = root / 'test.hpi'
    binary = root / 'test.hpb'
    name = filename.encode('cp932') + b'\0'
    header = bytearray(0x18 + 16 + len(name))
    header[:4] = b'HPIH'
    struct.pack_into('<H', header, 0x12, 0)
    struct.pack_into('<H', header, 0x14, 1)
    struct.pack_into('<IIII', header, 0x18, 0, 0, declared_size if declared_size is not None else len(payload), 0)
    header[0x28:0x28 + len(name)] = name
    index.write_bytes(header)
    binary.write_bytes(payload)
    return index


def test_unpack_hpi_skips_windows_absolute_member_on_all_hosts(tmp_path: Path):
    index = _write_uncompressed_pair(tmp_path, r'C:\escape.bin', b'data')
    out = tmp_path / 'out'
    assert unpack_hpi_pair(index, out) == []
    assert not (tmp_path / 'escape.bin').exists()
    assert not any(out.rglob('*')) if out.exists() else True


def test_unpack_hpi_rejects_truncated_uncompressed_member(tmp_path: Path):
    index = _write_uncompressed_pair(tmp_path, 'safe/file.bin', b'abcd', declared_size=8)
    with pytest.raises(HPXError, match='exceeds archive bounds'):
        unpack_hpi_pair(index, tmp_path / 'out')
