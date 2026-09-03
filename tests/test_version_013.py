import json
import tempfile
from pathlib import Path

from PIL import Image

import eouhd
from eouhd.app import APP_TITLE
from eouhd.version import DISPLAY_VERSION, LEGACY_REFERENCE_VERSION
from eouhd.workspace import (
    cleanup_streamlined_workspace,
    ensure_workspace,
    rgba_hashes,
    save_manifest,
    sync_azahar_master_pack,
)

TITLE = '00040000000EC700'


def _asset(root: Path) -> dict:
    dirs = ensure_workspace(root)
    orig = dirs['originals'] / 'ui' / 'test.png'
    master = dirs['masters'] / 'ui' / 'test.png'
    orig.parent.mkdir(parents=True, exist_ok=True)
    master.parent.mkdir(parents=True, exist_ok=True)
    Image.new('RGBA', (8, 8), (1, 2, 3, 255)).save(orig)
    Image.open(orig).save(master)
    direct, flipped = rgba_hashes(orig)
    return {
        'asset_id': 'ui_TEST', 'title_id': TITLE, 'category': 'ui',
        'width': 8, 'height': 8, 'format': 0, 'candidate_hash': 'AAAAAAAAAAAAAAAA',
        'verified_hashes': [], 'mip': 0,
        'original': str(orig.relative_to(root)).replace('\\', '/'),
        'master': str(master.relative_to(root)).replace('\\', '/'),
        'rgba_sha256': direct, 'rgba_flip_sha256': flipped,
        'texture_name': 'test', 'source': 'UI/TEST.STEX', 'material_bindings': [],
    }


def test_canonical_application_version_is_013() -> None:
    assert eouhd.__version__ == '0.13.0'
    assert DISPLAY_VERSION == '0.13'
    assert LEGACY_REFERENCE_VERSION == '0.12.0'
    assert APP_TITLE.endswith('0.13')


def test_legacy_version_literals_are_promoted_on_persistent_metadata() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        save_manifest(root, TITLE, [asset], version='0.12.0')

        manifest = json.loads((root / '.eouhd' / 'manifest.json').read_text(encoding='utf-8'))
        assert manifest['extractor_version'] == '0.13.0'

        pack = json.loads((root / 'azahar_pack_master' / 'load' / 'textures' / TITLE / 'pack.json').read_text(encoding='utf-8'))
        assert pack['version'] == '0.13.0'

        reports = root / '.eouhd' / 'reports'
        reports.mkdir(parents=True, exist_ok=True)
        (reports / 'synthetic.json').write_text(json.dumps({'version': '0.12.0'}), encoding='utf-8')
        cleanup_streamlined_workspace(root)
        stamped = json.loads((reports / 'synthetic.json').read_text(encoding='utf-8'))
        assert stamped['version'] == '0.13.0'
        assert stamped['legacy_reference_version'] == '0.12.0'
