import json
import tempfile
from pathlib import Path

import pytest
from PIL import Image

from eouhd import workspace_overhaul
from eouhd.workspace import (
    DEPLOY_PACK_DIR,
    MASTER_PACK_DIR,
    cleanup_streamlined_workspace,
    collect_protected_master_sources,
    ensure_workspace,
    reset_generated_workspace,
    rgba_hashes,
    save_manifest,
    sync_azahar_master_pack,
)

TITLE = '00040000000EC700'
HASH = '1111111111111111'


def _asset(root: Path, color=(10, 20, 30, 255)) -> dict:
    dirs = ensure_workspace(root)
    orig = dirs['originals'] / 'monsters' / 'monkey.png'
    master = dirs['masters'] / 'monsters' / 'monkey.png'
    orig.parent.mkdir(parents=True, exist_ok=True)
    master.parent.mkdir(parents=True, exist_ok=True)
    Image.new('RGBA', (8, 8), color).save(orig)
    Image.open(orig).save(master)
    direct, flipped = rgba_hashes(orig)
    return {
        'asset_id': 'mon_MONKEY',
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
        'texture_name': 'monkey',
        'source': 'ENEMY/MONKEY.STEX',
        'material_bindings': [],
    }


def _master_title(root: Path) -> Path:
    return root / MASTER_PACK_DIR / 'load' / 'textures' / TITLE


def test_manifest_loss_does_not_destroy_live_upscaled_master() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        save_manifest(root, TITLE, [asset], version='0.12.0')
        master = _master_title(root) / 'monsters' / 'monkey.png'
        Image.new('RGBA', (32, 32), (90, 80, 70, 255)).save(master)

        (root / '.eouhd' / 'manifest.json').unlink()
        protected = collect_protected_master_sources(root)
        assert protected[f'hash:{HASH}'] == master

        fresh = _asset(root, color=(1, 2, 3, 255))
        sync_azahar_master_pack(root, [fresh], protected)
        with Image.open(master) as im:
            assert im.size == (32, 32)
            assert im.convert('RGBA').getpixel((0, 0)) == (90, 80, 70, 255)


def test_live_pack_json_rename_is_preserved_on_rerun() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        save_manifest(root, TITLE, [asset], version='0.12.0')

        title = _master_title(root)
        old = title / 'monsters' / 'monkey.png'
        renamed = title / 'monsters' / 'my-monkey-upscale.png'
        old.replace(renamed)
        Image.new('RGBA', (32, 32), (4, 5, 6, 255)).save(renamed)
        pack_path = title / 'pack.json'
        pack = json.loads(pack_path.read_text(encoding='utf-8'))
        pack['textures'][HASH] = renamed.name
        pack_path.write_text(json.dumps(pack, indent=2), encoding='utf-8')

        fresh = _asset(root, color=(7, 8, 9, 255))
        protected = collect_protected_master_sources(root)
        sync_azahar_master_pack(root, [fresh], protected)

        pack = json.loads((_master_title(root) / 'pack.json').read_text(encoding='utf-8'))
        assert pack['textures'][HASH] == renamed.name
        kept = _master_title(root) / 'monsters' / renamed.name
        assert kept.is_file()
        with Image.open(kept) as im:
            assert im.size == (32, 32)
            assert im.convert('RGBA').getpixel((0, 0)) == (4, 5, 6, 255)


def test_untracked_master_file_survives_transactional_refresh() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        extra = _master_title(root) / 'user-notes' / 'custom-reference.png'
        extra.parent.mkdir(parents=True, exist_ok=True)
        Image.new('RGBA', (3, 5), (11, 22, 33, 44)).save(extra)

        fresh = _asset(root, color=(2, 3, 4, 255))
        sync_azahar_master_pack(root, [fresh], collect_protected_master_sources(root))
        kept = _master_title(root) / 'user-notes' / 'custom-reference.png'
        assert kept.is_file()
        assert kept.read_bytes() == extra.read_bytes()


def test_validation_failure_leaves_previous_master_pack_intact(monkeypatch) -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        asset = _asset(root)
        sync_azahar_master_pack(root, [asset], {})
        master = _master_title(root) / 'monsters' / 'monkey.png'
        before = master.read_bytes()

        def fail_validation(_root: Path) -> None:
            raise RuntimeError('synthetic staged validation failure')

        monkeypatch.setattr(workspace_overhaul, '_validate_pack_root', fail_validation)
        fresh = _asset(root, color=(99, 98, 97, 255))
        with pytest.raises(RuntimeError, match='synthetic staged validation failure'):
            sync_azahar_master_pack(root, [fresh], collect_protected_master_sources(root))

        assert master.is_file()
        assert master.read_bytes() == before


def test_cleanup_requires_workspace_marker_before_deleting_legacy_trees() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        legacy = root / '04_originals'
        legacy.mkdir(parents=True)
        (legacy / 'keep.bin').write_bytes(b'keep')

        with pytest.raises(RuntimeError, match='workspace marker'):
            cleanup_streamlined_workspace(root)
        assert (legacy / 'keep.bin').is_file()

        ensure_workspace(root)
        cleanup_streamlined_workspace(root)
        assert not legacy.exists()


def test_reset_keeps_previous_deployment_until_replacement_succeeds() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        ensure_workspace(root)
        deploy_file = root / DEPLOY_PACK_DIR / 'known-good.txt'
        deploy_file.parent.mkdir(parents=True, exist_ok=True)
        deploy_file.write_text('known-good', encoding='utf-8')

        reset_generated_workspace(root)
        assert deploy_file.read_text(encoding='utf-8') == 'known-good'
