from __future__ import annotations

"""0.13 material-workspace compatibility and persistence policy."""

from copy import deepcopy
from pathlib import Path
import csv
import json

from . import materials as _materials

_ORIG_REHYDRATE = _materials.rehydrate_material_bindings
_ORIG_BUILD = _materials.build_3d_material_workspace

_TRANSIENT_PREFIXES = (
    '.eouhd/work/',
    '.eouhd\\work\\',
)


def _is_transient_path(value: object) -> bool:
    text = str(value or '')
    normalized = text.replace('\\', '/')
    return normalized.startswith('.eouhd/work/')


def _mark_removed(row: dict, key: str, status_key: str, status: str) -> None:
    value = row.get(key)
    if value and _is_transient_path(value):
        row[status_key] = status
        row[key] = ''


def sanitize_persistent_material_reports(workspace: Path) -> None:
    """Remove references to material images that streamlined cleanup will delete."""
    workspace = Path(workspace)
    report_dir = workspace / '.eouhd' / 'reports'
    report_path = report_dir / '3d_material_report.json'
    if not report_path.is_file():
        return
    try:
        report = json.loads(report_path.read_text(encoding='utf-8'))
    except Exception:
        return

    for material in report.get('materials', []) or []:
        for slot in material.get('texture_slots', []) or []:
            _mark_removed(
                slot, 'material_copy', 'material_copy_status',
                'transient_removed_after_successful_streamlined_cleanup'
            )
        for alpha in material.get('alpha_texture_channels', []) or []:
            _mark_removed(
                alpha, 'alpha_plane', 'alpha_plane_status',
                'transient_exact_channel_removed_after_successful_streamlined_cleanup'
            )
        _mark_removed(
            material, 'resolved_material_alpha', 'resolved_material_alpha_status',
            'transient_diagnostic_removed_after_successful_streamlined_cleanup'
        )
        _mark_removed(
            material, 'rgba_preview', 'rgba_preview_status',
            'transient_diagnostic_removed_after_successful_streamlined_cleanup'
        )
        _mark_removed(
            material, 'checker_preview', 'checker_preview_status',
            'transient_diagnostic_removed_after_successful_streamlined_cleanup'
        )
        if material.get('alpha_resolution_status') == 'resolved':
            material['resolved_material_alpha_kind'] = 'diagnostic_shader_reconstruction'
            material['resolved_material_alpha_exact_rendering'] = False
        material['diagnostic_alpha_note'] = (
            'Resolved material alpha is a diagnostic scalar reconstruction. It does not model full UV transforms, '
            'texture filtering/wrapping, or other GPU state; differing texture dimensions may have been resized '
            'with nearest-neighbor sampling.'
        )

    report['transient_material_artifacts_retained'] = False
    report['resolved_alpha_semantics'] = 'diagnostic_shader_reconstruction_not_exact_rendering'
    report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8')

    # Keep the retained CSV truthful as well: these artifacts are intentionally
    # absent after the successful streamlined cleanup.
    csv_path = report_dir / '3d_materials.csv'
    if csv_path.is_file():
        try:
            with csv_path.open('r', newline='', encoding='utf-8-sig') as handle:
                reader = csv.DictReader(handle)
                fieldnames = list(reader.fieldnames or [])
                rows = list(reader)
            for row in rows:
                for key in ('resolved_material_alpha', 'rgba_preview'):
                    if _is_transient_path(row.get(key)):
                        row[key] = ''
            with csv_path.open('w', newline='', encoding='utf-8-sig') as handle:
                writer = csv.DictWriter(handle, fieldnames=fieldnames)
                writer.writeheader()
                writer.writerows(rows)
        except Exception:
            pass


def _prepare_assets_for_streamlined_rebuild(workspace: Path, assets: list[dict]) -> None:
    """Use persistent masters when cleaned manifests no longer retain originals."""
    search_roots = [
        workspace / '.eouhd' / 'work' / '03_hpx_unpacked',
        workspace / '.eouhd' / 'work' / '03b_farc_unpacked',
        workspace / '.eouhd' / 'work' / '03c_epl_unpacked',
        workspace / '.eouhd' / 'work' / '02_romfs_selected',
        workspace / '03_hpx_unpacked',
        workspace / '03b_farc_unpacked',
        workspace / '02_romfs_selected',
    ]
    for asset in assets:
        if not asset.get('original'):
            master = workspace / str(asset.get('master') or '')
            if master.is_file():
                asset['original'] = str(master.relative_to(workspace)).replace('\\', '/')

        if asset.get('material_bindings'):
            continue
        source = Path(str(asset.get('source') or ''))
        if not source.name or source.is_file():
            continue
        matches: list[Path] = []
        for root in search_roots:
            if root.exists():
                matches.extend(p for p in root.rglob(source.name) if p.is_file())
        unique = list(dict.fromkeys(str(p.resolve()) for p in matches))
        if len(unique) == 1:
            asset['source'] = unique[0]


def rebuild_3d_material_workspace(workspace: Path) -> dict:
    workspace = Path(workspace)
    manifest_path = workspace / '.eouhd' / 'manifest.json'
    if not manifest_path.is_file():
        manifest_path = workspace / 'manifest.json'
    manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    assets = manifest.get('assets', [])
    _prepare_assets_for_streamlined_rebuild(workspace, assets)

    needs_rehydrate = [
        asset for asset in assets
        if str(asset.get('parser_used', '')) in {'bch_struct', 'cgfx_struct'}
        and not asset.get('material_bindings')
    ]
    if needs_rehydrate:
        rehydrated = _ORIG_REHYDRATE(workspace, assets)
        rehydrated['mode'] = 'source_reparse_when_available'
    else:
        rehydrated = {
            'models_reparsed': 0,
            'bindings_added': 0,
            'parse_errors': [],
            'mode': 'manifest_bindings',
        }

    report = _ORIG_BUILD(workspace, assets)
    report['rehydration'] = rehydrated
    report['resolved_alpha_semantics'] = 'diagnostic_shader_reconstruction_not_exact_rendering'
    manifest['assets'] = assets
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8')
    (workspace / '.eouhd' / 'reports' / '3d_material_report.json').write_text(
        json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8'
    )
    return report


def install() -> None:
    _materials.rebuild_3d_material_workspace = rebuild_3d_material_workspace
