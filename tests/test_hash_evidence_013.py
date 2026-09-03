import json
import tempfile
from pathlib import Path

from PIL import Image

from eouhd.workspace import (
    confirm_runtime_hash_candidate,
    ensure_workspace,
    import_runtime_dump,
    rgba_hashes,
)

TITLE = '00040000000EC700'
RUNTIME_HASH = '1234567890ABCDEF'


def _workspace_with_asset(root: Path, color=(10, 20, 30, 255)) -> None:
    ensure_workspace(root)
    orig = root / '04_originals' / 'ui' / 'ui_TEST.png'
    master = root / '05_hd_masters' / 'ui' / 'ui_TEST.png'
    orig.parent.mkdir(parents=True, exist_ok=True)
    master.parent.mkdir(parents=True, exist_ok=True)
    image = Image.new('RGBA', (8, 8), color)
    image.save(orig)
    image.save(master)
    direct, flipped = rgba_hashes(orig)
    manifest = {
        'title_id': TITLE,
        'assets': [{
            'asset_id': 'ui_TEST',
            'title_id': TITLE,
            'category': 'ui',
            'width': 8,
            'height': 8,
            'format': 0,
            'candidate_hash': 'AAAAAAAAAAAAAAAA',
            'verified_hashes': [],
            'original': str(orig.relative_to(root)).replace('\\', '/'),
            'master': str(master.relative_to(root)).replace('\\', '/'),
            'rgba_sha256': direct,
            'rgba_flip_sha256': flipped,
            'texture_name': 'ui-test',
            'source': 'UI/TEST.STEX',
        }],
    }
    (root / 'manifest.json').write_text(json.dumps(manifest), encoding='utf-8')


def test_visual_hd_match_stays_candidate_until_confirmed() -> None:
    with tempfile.TemporaryDirectory() as td:
        td = Path(td)
        root = td / 'ws'
        _workspace_with_asset(root)

        pack = td / 'pack'
        pack.mkdir()
        Image.new('RGBA', (32, 32), (10, 20, 30, 255)).save(pack / 'upscaled.png')
        (pack / 'pack.json').write_text(json.dumps({
            'textures': {RUNTIME_HASH: 'upscaled.png'}
        }), encoding='utf-8')

        report = import_runtime_dump(root, pack)
        assert report['matched_exact'] == 0
        assert report['visual_candidates'] == 1
        assert report['verified_total'] == 0
        assert report['matched_total'] == 0

        manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
        asset = manifest['assets'][0]
        assert RUNTIME_HASH not in asset['verified_hashes']
        assert asset['runtime_hash_candidates'][0]['hash'] == RUNTIME_HASH
        assert asset['runtime_hash_candidates'][0]['status'] == 'candidate'

        result = confirm_runtime_hash_candidate(root, 'ui_TEST', RUNTIME_HASH)
        assert result['status'] == 'verified'
        manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
        asset = manifest['assets'][0]
        assert RUNTIME_HASH in asset['verified_hashes']
        assert asset['runtime_hash_candidates'] == []


def test_exact_rgba_match_is_still_verified_automatically() -> None:
    with tempfile.TemporaryDirectory() as td:
        td = Path(td)
        root = td / 'ws'
        _workspace_with_asset(root)

        pack = td / 'pack'
        pack.mkdir()
        Image.new('RGBA', (8, 8), (10, 20, 30, 255)).save(pack / 'exact.png')
        (pack / 'pack.json').write_text(json.dumps({
            'textures': {RUNTIME_HASH: 'exact.png'}
        }), encoding='utf-8')

        report = import_runtime_dump(root, pack)
        assert report['matched_exact'] == 1
        assert report['verified_total'] == 1
        assert report['visual_candidates'] == 0

        manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
        assert RUNTIME_HASH in manifest['assets'][0]['verified_hashes']
