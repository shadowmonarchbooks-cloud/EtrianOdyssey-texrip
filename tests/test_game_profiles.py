import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.profiles import detect_game_profile, UnsupportedGameError
from eouhd.strict_scan import has_strict_texture_signature, _embedded_bch_offsets


def _minimal_bch() -> bytes:
    import struct
    b = bytearray(0x100)
    b[:4] = b'BCH\0'
    # Sufficient for strict wrapper validation: content/strings/commands/data.
    struct.pack_into('<IIII', b, 0x08, 0x20, 0x28, 0x30, 0x40)
    return bytes(b)


def test_detects_eou1_usa_by_title_id():
    profile = detect_game_profile('00040000000EC700', 'CTR-P-BSKE')
    assert profile.id == 'eou1'


def test_detects_eo2u_regions_by_title_id():
    assert detect_game_profile('0004000000120500', '').id == 'eo2u'
    assert detect_game_profile('000400000015F200', '').id == 'eo2u'
    assert detect_game_profile('000400000016E900', '').id == 'eo2u'


def test_detects_eo2u_unknown_region_by_product_family():
    assert detect_game_profile('0004000000000001', 'CTR-P-BM9K').id == 'eo2u'


def test_rejects_unrelated_3ds_title():
    with pytest.raises(UnsupportedGameError):
        detect_game_profile('00040000000BD300', 'CTR-P-ASJE')


def test_eo2u_bam2_wrapped_bch_is_a_strict_candidate():
    wrapper = b'BAM2' + b'\0' * 0x7C + _minimal_bch()
    assert _embedded_bch_offsets(wrapper) == [0x80]
    assert has_strict_texture_signature(wrapper, '.bam')
