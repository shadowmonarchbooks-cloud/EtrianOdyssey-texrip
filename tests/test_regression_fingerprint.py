import json
import tempfile
from pathlib import Path

from PIL import Image

from eouhd.regression import build_workspace_fingerprint, compare_fingerprints
from eouhd.workspace import ensure_workspace, rgba_hashes, save_manifest

TITLE = '00040000000EC700'


def _make_workspace(root: Path) -> None:
    dirs = ensure_workspace(root)
    orig = dirs['originals'] / 'monsters' / 'asset.png'
    master = dirs['masters'] / 'monsters' / 'asset.png'
    orig.parent.mkdir(parents=True, exist_ok=True)
    master.parent.mkdir(parents=True, exist_ok=True)
    Image.new('RGBA', (8, 16), (1, 2, 3, 255)).save(orig)
    Image.open(orig).save(master)
    direct, flipped = rgba_hashes(orig)
    asset = {
        'asset_id': 'mon_TEST',
        'title_id': TITLE,
        'category': 'monsters',
        'width': 8,
        'height': 16,
        'format': 12,
        'candidate_hash': 'AAAAAAAAAAAAAAAA',
        'verified_hashes': ['BBBBBBBBBBBBBBBB'],
        'mip': 0,
        'original': str(orig.relative_to(root)).replace('\\', '/'),
        'master': str(master.relative_to(root)).replace('\\', '/'),
        'rgba_sha256': direct,
        'rgba_flip_sha256': flipped,
        'parser_used': 'eou_stex_strict',
        'texture_name': 'copyrighted_name_should_not_appear',
        'source': '/private/legal/rom/path/that_must_not_appear.stex',
        'material_bindings': [],
    }
    save_manifest(
        root, TITLE, [asset], version='0.13.0',
        game_profile={'id': 'eou1', 'display_name': 'EOU'}, product_code='CTR-P-AEKJ'
    )


def test_fingerprint_is_deterministic_and_omits_paths_and_names() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        _make_workspace(root)
        first = build_workspace_fingerprint(root)
        second = build_workspace_fingerprint(root)
        assert first == second
        assert first['asset_count'] == 1
        assert first['asset_descriptor_sha256'] == second['asset_descriptor_sha256']
        encoded = json.dumps(first)
        assert 'copyrighted_name_should_not_appear' not in encoded
        assert '/private/legal/rom/path' not in encoded
        assert first['privacy']['contains_rom_bytes'] is False
        assert first['privacy']['contains_decoded_pixels'] is False
        assert first['privacy']['contains_source_paths'] is False


def test_fingerprint_compare_reports_structural_drift() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        _make_workspace(root)
        expected = build_workspace_fingerprint(root)
        actual = json.loads(json.dumps(expected))
        actual['asset_count'] = 2
        result = compare_fingerprints(expected, actual)
        assert result['match'] is False
        assert result['differences']['asset_count'] == {'expected': 1, 'actual': 2}
