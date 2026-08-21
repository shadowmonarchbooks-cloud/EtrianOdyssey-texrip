from pathlib import Path
from eouhd.hpx import find_hpi_pairs


def test_find_hpi_pair_is_case_insensitive(tmp_path: Path):
    (tmp_path / 'MORI1R.HPI').write_bytes(b'HPIH' + b'\0' * 32)
    (tmp_path / 'MORI1R.HPB').write_bytes(b'')
    assert [p.name for p in find_hpi_pairs(tmp_path)] == ['MORI1R.HPI']
