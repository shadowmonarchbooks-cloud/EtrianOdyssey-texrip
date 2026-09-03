import json
import tempfile
from pathlib import Path

from PIL import Image

from eouhd.materials import rebuild_3d_material_workspace
from eouhd.workspace import cleanup_streamlined_workspace, ensure_workspace

TITLE = '00040000000EC700'


def test_cleanup_sanitizes_transient_material_report_paths() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        ensure_workspace(root)
        report_dir = root / '.eouhd' / 'reports'
        report_dir.mkdir(parents=True, exist_ok=True)
        transient = root / '.eouhd' / 'work' / '10_3d_materials' / 'model' / 'material'
        transient.mkdir(parents=True, exist_ok=True)
        (transient / 'resolved_material_alpha.png').write_bytes(b'png')

        report = {
            'materials': [{
                'alpha_resolution_status': 'resolved',
                'texture_slots': [{
                    'material_copy': '.eouhd/work/10_3d_materials/model/material/texture0.png'
                }],
                'alpha_texture_channels': [{
                    'alpha_plane': '.eouhd/work/10_3d_materials/model/material/alpha.png'
                }],
                'resolved_material_alpha': '.eouhd/work/10_3d_materials/model/material/resolved_material_alpha.png',
                'rgba_preview': '.eouhd/work/10_3d_materials/model/material/rgba_material_preview.png',
                'checker_preview': '.eouhd/work/10_3d_materials/model/material/checker_material_preview.jpg',
            }],
        }
        (report_dir / '3d_material_report.json').write_text(json.dumps(report), encoding='utf-8')

        cleanup_streamlined_workspace(root)
        saved = json.loads((report_dir / '3d_material_report.json').read_text(encoding='utf-8'))
        material = saved['materials'][0]
        assert material['texture_slots'][0]['material_copy'] == ''
        assert material['alpha_texture_channels'][0]['alpha_plane'] == ''
        assert material['resolved_material_alpha'] == ''
        assert material['rgba_preview'] == ''
        assert material['checker_preview'] == ''
        assert material['resolved_material_alpha_kind'] == 'diagnostic_shader_reconstruction'
        assert material['resolved_material_alpha_exact_rendering'] is False
        assert saved['transient_material_artifacts_retained'] is False
        assert not (root / '.eouhd' / 'work').exists()


def test_rebuild_uses_persistent_master_when_original_tree_is_gone() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        ensure_workspace(root)
        master = root / 'azahar_pack_master' / 'load' / 'textures' / TITLE / 'monsters' / 'monkey.png'
        master.parent.mkdir(parents=True, exist_ok=True)
        Image.new('RGBA', (8, 8), (50, 60, 70, 128)).save(master)

        stages = [{
            'stage': 0,
            'combiner_id': 0,
            'combiner': 'Replace',
            'inputs': [{
                'input': 0,
                'source_id': 3,
                'source': 'Texture0',
                'operand_id': 0,
                'operand': 'Alpha',
            }],
        }]
        use = [{
            'stage': 0,
            'combiner': 'Replace',
            'input': 0,
            'slot': 0,
            'source': 'Texture0',
            'operand': 'Alpha',
            'operand_id': 0,
        }]
        manifest = {
            'title_id': TITLE,
            'assets': [{
                'asset_id': 'mon_MONKEY',
                'title_id': TITLE,
                'category': 'monsters',
                'width': 8,
                'height': 8,
                'format': 0,
                'candidate_hash': 'AAAAAAAAAAAAAAAA',
                'verified_hashes': [],
                'master': str(master.relative_to(root)).replace('\\', '/'),
                'texture_name': 'monkey',
                'source': 'deleted_source.bch',
                'parser_used': 'bch_struct',
                'material_bindings': [{
                    'material_index': 0,
                    'material_name': 'monkey_material',
                    'model_index': 0,
                    'model_name': 'monkey_model',
                    'slot': 0,
                    'enabled': True,
                    'alpha_uses': use,
                    'alpha_stages': stages,
                    'alpha_test': None,
                    'source': 'deleted_source.bch',
                    'container_offset': 0,
                    'texture_name': 'monkey',
                }],
            }],
        }
        manifest_path = root / '.eouhd' / 'manifest.json'
        manifest_path.write_text(json.dumps(manifest), encoding='utf-8')

        report = rebuild_3d_material_workspace(root)
        assert report['rehydration']['mode'] == 'manifest_bindings'
        assert report['materials_found'] == 1
        assert report['resolved_material_alphas'] == 1
        assert report['resolved_alpha_semantics'] == 'diagnostic_shader_reconstruction_not_exact_rendering'
        resolved = root / report['materials'][0]['resolved_material_alpha']
        assert resolved.is_file()
