import json
import tempfile
from pathlib import Path

from PIL import Image

from eouhd.workspace import (
    MASTER_PACK_DIR,
    DEPLOY_PACK_DIR,
    cleanup_streamlined_workspace,
    collect_protected_master_sources,
    ensure_workspace,
    rgba_hashes,
    save_manifest,
    sync_azahar_master_pack,
    build_azahar_pack,
)

TITLE = '00040000000EC700'
HASH = 'B48BE5A931380CE8'
FILENAME = 'en-flower01-t01.png'


def _asset(root: Path, color=(1, 2, 3, 255)) -> dict:
    dirs = ensure_workspace(root)
    orig = dirs['originals'] / 'monsters' / 'mon_test.png'
    master = dirs['masters'] / 'monsters' / 'mon_test.png'
    orig.parent.mkdir(parents=True, exist_ok=True)
    master.parent.mkdir(parents=True, exist_ok=True)
    Image.new('RGBA', (8, 8), color).save(orig)
    Image.open(orig).save(master)
    direct, flipped = rgba_hashes(orig)
    return {
        'asset_id': 'mon_TEST',
        'title_id': TITLE,
        'category': 'monsters',
        'width': 8,
        'height': 8,
        'format': 0,
        'candidate_hash': HASH,
        'verified_hashes': [],
        'mip': 0,
        'original': str(orig.relative_to(root)).replace('\\', '/'),
        'master': str(master.relative_to(root)).replace('\\', '/'),
        'rgba_sha256': direct,
        'rgba_flip_sha256': flipped,
        'target_scale': 4,
        'source': 'EN001A.BAM/en_flower01_t01',
    }


def test_streamlined_master_and_deployment_are_categorized() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        save_manifest(root, TITLE, [asset], version='0.9.0')
        out = build_azahar_pack(root)

        master_png = root / MASTER_PACK_DIR / 'load' / 'textures' / TITLE / 'monsters' / FILENAME
        deploy_png = root / DEPLOY_PACK_DIR / 'load' / 'textures' / TITLE / 'monsters' / FILENAME
        assert master_png.is_file()
        assert deploy_png.is_file()
        assert master_png.read_bytes() == deploy_png.read_bytes()
        assert out == root / DEPLOY_PACK_DIR / 'load' / 'textures' / TITLE
        assert (out / 'pack.json').is_file()
        pack = json.loads((out / 'pack.json').read_text())
        assert pack['textures'][HASH] == FILENAME


def test_cleanup_leaves_only_packs_and_lightweight_metadata() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        save_manifest(root, TITLE, [asset], version='0.9.0')
        build_azahar_pack(root)
        cleanup_streamlined_workspace(root)

        assert (root / MASTER_PACK_DIR).is_dir()
        assert (root / DEPLOY_PACK_DIR).is_dir()
        assert (root / '.eouhd' / 'manifest.json').is_file()
        assert not (root / '.eouhd' / 'work').exists()
        assert not (root / '04_originals').exists()
        assert not (root / '05_hd_masters').exists()


def test_rerun_preserves_upscaled_master() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        save_manifest(root, TITLE, [asset], version='0.9.0')

        master_png = root / MASTER_PACK_DIR / 'load' / 'textures' / TITLE / 'monsters' / FILENAME
        Image.new('RGBA', (32, 32), (9, 8, 7, 255)).save(master_png)
        protected = collect_protected_master_sources(root)
        assert FILENAME in protected

        # Simulate a new extraction producing the original 8x8 source again.
        fresh = _asset(root, color=(4, 5, 6, 255))
        sync_azahar_master_pack(root, [fresh], protected)
        with Image.open(master_png) as im:
            assert im.size == (32, 32)
            assert im.convert('RGBA').getpixel((0, 0)) == (9, 8, 7, 255)
