from __future__ import annotations

"""Copyright-safe structural fingerprints for local game validation.

Fingerprints contain counts, texture metadata, emulator/runtime hashes and an
aggregate SHA-256 over normalized asset descriptors. They never contain ROM
bytes, decoded texture pixels, source paths, or embedded texture/model names.
"""

from collections import Counter
from pathlib import Path
import hashlib
import json
from typing import Any

from . import workspace

FINGERPRINT_SCHEMA = 1


def _counter_dict(values) -> dict[str, int]:
    return dict(sorted(Counter(str(v) for v in values).items()))


def _asset_descriptor(asset: dict) -> dict[str, Any]:
    return {
        'candidate_hash': str(asset.get('candidate_hash') or '').upper(),
        'verified_hashes': sorted(str(h).upper() for h in (asset.get('verified_hashes') or [])),
        'width': int(asset.get('width', 0) or 0),
        'height': int(asset.get('height', 0) or 0),
        'format': int(asset.get('format', -1) if asset.get('format') is not None else -1),
        'mip': int(asset.get('mip', 0) or 0),
        'parser_used': str(asset.get('parser_used') or ''),
        'category': str(asset.get('category') or ''),
        'material_binding_count': len(asset.get('material_bindings') or []),
    }


def _load_json(path: Path) -> dict:
    if not path.is_file():
        return {}
    try:
        value = json.loads(path.read_text(encoding='utf-8'))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


def build_workspace_fingerprint(workspace_root: str | Path) -> dict:
    root = Path(workspace_root)
    manifest = workspace.load_manifest(root)
    assets = [a for a in manifest.get('assets', []) if isinstance(a, dict)]
    descriptors = sorted(
        (_asset_descriptor(asset) for asset in assets),
        key=lambda row: (
            row['candidate_hash'], row['width'], row['height'], row['format'],
            row['mip'], row['parser_used'], row['category'], row['verified_hashes'],
        ),
    )
    canonical = json.dumps(descriptors, sort_keys=True, separators=(',', ':'), ensure_ascii=True)
    asset_digest = hashlib.sha256(canonical.encode('ascii')).hexdigest()

    reports = root / workspace.METADATA_DIR / 'reports'
    run_summary = _load_json(reports / 'run_summary.json')
    material_report = _load_json(reports / '3d_material_report.json')
    model_inventory = _load_json(reports / '3d_model_inventory.json')

    fingerprint = {
        'schema_version': FINGERPRINT_SCHEMA,
        'kind': 'eo-texrip-structural-regression-fingerprint',
        'game_id': str(manifest.get('game_id') or (manifest.get('game_profile') or {}).get('id') or ''),
        'title_id': str(manifest.get('title_id') or '').upper(),
        'product_code': str(manifest.get('product_code') or ''),
        'asset_count': len(assets),
        'asset_descriptor_sha256': asset_digest,
        'candidate_hash_count': sum(bool(a.get('candidate_hash')) for a in assets),
        'verified_runtime_hash_count': sum(len(a.get('verified_hashes') or []) for a in assets),
        'parser_counts': _counter_dict(a.get('parser_used') or '' for a in assets),
        'format_counts': _counter_dict(a.get('format', -1) for a in assets),
        'dimension_counts': _counter_dict(
            f"{int(a.get('width', 0) or 0)}x{int(a.get('height', 0) or 0)}" for a in assets
        ),
        'category_counts': _counter_dict(a.get('category') or '' for a in assets),
        'material_bound_assets': sum(bool(a.get('material_bindings')) for a in assets),
        'summary': {
            key: run_summary.get(key)
            for key in (
                'strict_candidate_files', 'decoded_before_dedup', 'issues',
                'hpx_pairs', 'hpx_files', 'farc_archives', 'farc_files',
                'epl_archives', 'epl_files', 'models_found', 'model_materials_found',
                'stex_files', 'atbc_files', 'cgfx_files', 'wrapped_bch_files', 'bam_bch_files',
            )
            if key in run_summary
        },
        'materials': {
            key: material_report.get(key)
            for key in (
                'materials_found', 'material_texture_bindings',
                'explicit_texture_alpha_channels', 'constant_texture_alpha_inputs',
                'resolved_material_alphas', 'unresolved_material_alphas',
            )
            if key in material_report
        },
        'models': {
            key: model_inventory.get(key)
            for key in (
                'payloads', 'cgfx_payloads', 'bch_payloads', 'bam2_bch_payloads',
                'models_found', 'materials_found', 'texture_descriptors_found', 'decoded_3d_textures',
            )
            if key in model_inventory
        },
        'privacy': {
            'contains_rom_bytes': False,
            'contains_decoded_pixels': False,
            'contains_source_paths': False,
            'contains_texture_or_model_names': False,
        },
    }
    return fingerprint


def compare_fingerprints(expected: dict, actual: dict) -> dict:
    """Return a compact structural diff suitable for CI/local validation."""
    keys = (
        'game_id', 'title_id', 'product_code', 'asset_count',
        'asset_descriptor_sha256', 'candidate_hash_count', 'verified_runtime_hash_count',
        'parser_counts', 'format_counts', 'dimension_counts', 'category_counts',
        'material_bound_assets', 'summary', 'materials', 'models',
    )
    differences = {
        key: {'expected': expected.get(key), 'actual': actual.get(key)}
        for key in keys
        if expected.get(key) != actual.get(key)
    }
    return {
        'match': not differences,
        'differences': differences,
    }
