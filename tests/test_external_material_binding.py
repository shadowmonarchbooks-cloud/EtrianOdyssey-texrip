import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.pipeline import _bind_external_material_textures


def _binding():
    return {
        'material_index': 0,
        'material_name': 'enemy_body',
        'model_index': 0,
        'model_name': 'enemy01',
        'model_material_index': 0,
        'slot': 1,
        'enabled': True,
        'alpha_uses': [{
            'stage': 0, 'combiner': 'Replace', 'input': 0, 'slot': 1,
            'source': 'Texture1', 'operand': 'Red', 'operand_id': 2,
        }],
        'alpha_stages': [],
        'alpha_test': None,
    }


def test_unique_external_stex_name_gets_exact_material_binding():
    assets = [
        {'asset_id': 'stex_mask', 'texture_name': 'enemy_mask', 'material_bindings': []},
        {'asset_id': 'unrelated', 'texture_name': 'ui_tex', 'material_bindings': []},
    ]
    diagnostics = [{
        'source': '/archive/enemy01.bam',
        'container_offset': 32,
        'missing_material_texture_names': ['enemy_mask'],
        'material_bindings_by_texture': {'enemy_mask': [_binding()]},
    }]
    report = _bind_external_material_textures(assets, diagnostics)
    assert report['external_bindings_resolved'] == 1
    assert report['ambiguous_external_texture_names'] == []
    assert report['still_missing_external_texture_names'] == []
    bound = assets[0]['material_bindings'][0]
    assert bound['source'] == '/archive/enemy01.bam'
    assert bound['container_offset'] == 32
    assert bound['external_texture_binding'] is True
    assert bound['alpha_uses'][0]['operand'] == 'Red'


def test_ambiguous_external_name_is_not_guessed():
    assets = [
        {'asset_id': 'a', 'texture_name': 'shared_mask', 'material_bindings': []},
        {'asset_id': 'b', 'texture_name': 'shared_mask', 'material_bindings': []},
    ]
    diagnostics = [{
        'source': '/archive/enemy01.bam',
        'container_offset': 0,
        'missing_material_texture_names': ['shared_mask'],
        'material_bindings_by_texture': {'shared_mask': [_binding()]},
    }]
    report = _bind_external_material_textures(assets, diagnostics)
    assert report['external_bindings_resolved'] == 0
    assert len(report['ambiguous_external_texture_names']) == 1
    assert assets[0]['material_bindings'] == []
    assert assets[1]['material_bindings'] == []
