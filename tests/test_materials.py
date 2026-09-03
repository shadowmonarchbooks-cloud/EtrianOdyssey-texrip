import json
import tempfile
from pathlib import Path
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.materials import alpha_plane_from_operand, build_3d_material_workspace
from eouhd.workspace import ensure_workspace


def _binding(slot: int, alpha_uses: list[dict], stages: list[dict]) -> dict:
    return {
        'material_index': 0,
        'material_name': 'enemy_body',
        'slot': slot,
        'enabled': True,
        'alpha_uses': alpha_uses,
        'alpha_stages': stages,
        'alpha_test': {'enabled': True, 'function': 6, 'reference': 64, 'raw': 16481},
    }


def test_alpha_operand_extracts_real_channel_and_inverse():
    rgba = np.zeros((2, 2, 4), dtype=np.uint8)
    rgba[:, :, 0] = [[0, 64], [128, 255]]
    rgba[:, :, 3] = [[10, 20], [30, 40]]
    assert np.array_equal(alpha_plane_from_operand(rgba, 2), rgba[:, :, 0])
    assert np.array_equal(alpha_plane_from_operand(rgba, 3), 255 - rgba[:, :, 0])
    assert np.array_equal(alpha_plane_from_operand(rgba, 0), rgba[:, :, 3])


def test_only_material_referenced_alpha_is_exported():
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        ensure_workspace(root)
        source = str(root / '03_hpx_unpacked' / 'enemy001.bam')

        color = np.zeros((8, 16, 4), dtype=np.uint8)
        color[:, :, :3] = [190, 20, 10]
        color[:, :, 3] = 255
        mask = np.zeros((4, 8, 4), dtype=np.uint8)
        red = np.tile(np.arange(8, dtype=np.uint8) * 32, (4, 1))
        mask[:, :, 0] = red
        mask[:, :, 1] = 17
        mask[:, :, 2] = 99
        mask[:, :, 3] = 255
        random_gray = np.zeros((8, 8, 4), dtype=np.uint8)
        random_gray[:, :, :3] = np.arange(64, dtype=np.uint8).reshape(8, 8)[:, :, None] * 4
        random_gray[:, :, 3] = 255

        paths = {}
        for aid, arr in [('mon_COLOR', color), ('mon_MASK', mask), ('mis_GRAY', random_gray)]:
            cat = 'monsters' if aid.startswith('mon') else 'misc'
            p = root / '04_originals' / cat / f'{aid}.png'
            p.parent.mkdir(parents=True, exist_ok=True)
            Image.fromarray(arr, 'RGBA').save(p)
            paths[aid] = str(p.relative_to(root)).replace('\\', '/')

        stage = [{
            'stage': 0,
            'combiner_id': 0,
            'combiner': 'Replace',
            'inputs': [{
                'input': 0, 'source_id': 4, 'source': 'Texture1',
                'operand_id': 2, 'operand': 'Red',
            }],
        }]
        use = [{
            'stage': 0, 'combiner': 'Replace', 'input': 0, 'slot': 1,
            'source': 'Texture1', 'operand': 'Red', 'operand_id': 2,
        }]
        assets = [
            {
                'asset_id': 'mon_COLOR', 'source': source, 'container_offset': 16,
                'original': paths['mon_COLOR'], 'master': paths['mon_COLOR'],
                'texture_name': 'body', 'width': 16, 'height': 8, 'format': 12,
                'material_bindings': [_binding(0, [], stage)],
            },
            {
                'asset_id': 'mon_MASK', 'source': source, 'container_offset': 16,
                'original': paths['mon_MASK'], 'master': paths['mon_MASK'],
                'texture_name': 'mask', 'width': 8, 'height': 4, 'format': 12,
                'material_bindings': [_binding(1, use, stage)],
            },
            {
                'asset_id': 'mis_GRAY', 'source': str(root / 'ui.bin'),
                'original': paths['mis_GRAY'], 'master': paths['mis_GRAY'],
                'texture_name': 'unrelated_gray', 'width': 8, 'height': 8, 'format': 12,
                'material_bindings': [],
            },
        ]
        report = build_3d_material_workspace(root, assets)
        assert report['materials_found'] == 1
        assert report['explicit_texture_alpha_channels'] == 1
        assert report['heuristic_grayscale_masks_generated'] == 0
        assert report['resolved_material_alphas'] == 1
        material = report['materials'][0]
        assert material['alpha_texture_channels'][0]['operand'] == 'Red'
        alpha_path = root / material['alpha_texture_channels'][0]['alpha_plane']
        with Image.open(alpha_path) as im:
            assert np.array_equal(np.asarray(im), red)
        # The unrelated opaque grayscale texture must not produce any alpha file.
        material_root = root / '.eouhd' / 'work' / '10_3d_materials'
        all_names = [p.name for p in material_root.rglob('*.png')]
        assert not any('mis_GRAY' in name for name in all_names)


def test_no_material_bindings_means_no_generated_alpha():
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        ensure_workspace(root)
        p = root / '04_originals/misc/gray.png'
        p.parent.mkdir(parents=True, exist_ok=True)
        Image.new('RGBA', (8, 8), (120, 120, 120, 255)).save(p)
        assets = [{
            'asset_id': 'gray', 'source': 'ui.bin', 'original': '04_originals/misc/gray.png',
            'master': '04_originals/misc/gray.png', 'texture_name': 'gray',
            'width': 8, 'height': 8, 'format': 12, 'material_bindings': [],
        }]
        report = build_3d_material_workspace(root, assets)
        assert report['materials_found'] == 0
        assert report['explicit_texture_alpha_channels'] == 0
        material_root = root / '.eouhd' / 'work' / '10_3d_materials'
        assert list(material_root.rglob('*.png')) == []


def test_etc1_alpha_operand_is_recorded_as_constant_not_fake_alpha_png():
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        ensure_workspace(root)
        p = root / '04_originals' / 'monsters' / 'mon_ETC1.png'
        p.parent.mkdir(parents=True, exist_ok=True)
        rgba = np.zeros((8, 8, 4), dtype=np.uint8)
        rgba[:, :, :3] = 120
        rgba[:, :, 3] = 255
        Image.fromarray(rgba, 'RGBA').save(p)
        stage = [{
            'stage': 0, 'combiner_id': 0, 'combiner': 'Replace',
            'inputs': [{'input': 0, 'source_id': 3, 'source': 'Texture0', 'operand_id': 0, 'operand': 'Alpha'}],
        }]
        use = [{'stage': 0, 'combiner': 'Replace', 'input': 0, 'slot': 0, 'source': 'Texture0', 'operand': 'Alpha', 'operand_id': 0}]
        assets = [{
            'asset_id': 'mon_ETC1', 'source': str(root / 'enemy.bam'), 'container_offset': 0x180,
            'original': str(p.relative_to(root)).replace('\\', '/'),
            'master': str(p.relative_to(root)).replace('\\', '/'),
            'texture_name': 'body', 'width': 8, 'height': 8, 'format': 12,
            'material_bindings': [_binding(0, use, stage)],
        }]
        report = build_3d_material_workspace(root, assets)
        assert report['explicit_texture_alpha_channels'] == 0
        assert report['constant_texture_alpha_inputs'] == 1
        material = report['materials'][0]
        assert material['alpha_texture_channels'] == []
        assert material['constant_texture_alpha_inputs'][0]['constant_value'] == 255
        folder = root / '.eouhd' / 'work' / '10_3d_materials'
        assert not list(folder.rglob('alpha_stage*.png'))
