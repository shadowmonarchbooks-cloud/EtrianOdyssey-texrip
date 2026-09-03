from __future__ import annotations

import json as _json
from pathlib import Path as _Path

from .version import __version__, DISPLAY_VERSION, LEGACY_REFERENCE_VERSION

# 0.13 freezes the existing parser implementation while hardening destructive
# workspace boundaries through compatibility overlays. Keeping the legacy
# modules intact makes their behavior a stable reference for the Rust rewrite.
from .workspace_overhaul import install as _install_workspace_overhaul

_install_workspace_overhaul()
del _install_workspace_overhaul

from .extraction_budget import install as _install_extraction_budget, reset_budget as _reset_extraction_budget
from . import workspace as _workspace

_install_extraction_budget()
_legacy_reset_generated_workspace = _workspace.reset_generated_workspace


def _budgeted_reset_generated_workspace(root):
    _reset_extraction_budget()
    return _legacy_reset_generated_workspace(root)


_workspace.reset_generated_workspace = _budgeted_reset_generated_workspace
del _install_extraction_budget

from .hash_evidence_overhaul import install as _install_hash_evidence_overhaul

_install_hash_evidence_overhaul()
del _install_hash_evidence_overhaul

from .materials_overhaul import (
    install as _install_materials_overhaul,
    sanitize_persistent_material_reports as _sanitize_persistent_material_reports,
)

_install_materials_overhaul()
_legacy_cleanup_streamlined_workspace = _workspace.cleanup_streamlined_workspace


def _material_safe_cleanup_streamlined_workspace(root):
    _sanitize_persistent_material_reports(root)
    return _legacy_cleanup_streamlined_workspace(root)


_workspace.cleanup_streamlined_workspace = _material_safe_cleanup_streamlined_workspace
del _install_materials_overhaul

# Keep code-facing version metadata centralized even when frozen legacy call
# sites still pass their historical 0.12.0 literal.
_legacy_save_manifest = _workspace.save_manifest
_legacy_write_pack_metadata = _workspace._write_pack_metadata


def _versioned_save_manifest(
    workspace, title_id, assets, source_rom='', version=__version__,
    game_profile=None, product_code='',
):
    effective = __version__ if not version or version == LEGACY_REFERENCE_VERSION else version
    return _legacy_save_manifest(
        workspace, title_id, assets, source_rom, effective, game_profile, product_code
    )


def _versioned_write_pack_metadata(
    pack_root, title_id, version=__version__, game_name='', textures=None,
):
    effective = __version__ if not version or version == LEGACY_REFERENCE_VERSION else version
    return _legacy_write_pack_metadata(pack_root, title_id, effective, game_name, textures)


_workspace.save_manifest = _versioned_save_manifest
_workspace._write_pack_metadata = _versioned_write_pack_metadata


def _stamp_generated_metadata(root) -> None:
    root = _Path(root)
    for base in (root / '.eouhd' / 'reports', root / '.eouhd' / 'diagnostics'):
        if not base.exists():
            continue
        for path in base.rglob('*.json'):
            try:
                data = _json.loads(path.read_text(encoding='utf-8'))
            except Exception:
                continue
            if not isinstance(data, dict):
                continue
            data['version'] = __version__
            data.setdefault('legacy_reference_version', LEGACY_REFERENCE_VERSION)
            path.write_text(_json.dumps(data, indent=2, ensure_ascii=False), encoding='utf-8')


_current_cleanup_streamlined_workspace = _workspace.cleanup_streamlined_workspace


def _versioned_cleanup_streamlined_workspace(root):
    result = _current_cleanup_streamlined_workspace(root)
    _stamp_generated_metadata(root)
    return result


_workspace.cleanup_streamlined_workspace = _versioned_cleanup_streamlined_workspace
