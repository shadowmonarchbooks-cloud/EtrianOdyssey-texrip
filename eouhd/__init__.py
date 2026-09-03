__version__ = '0.12.0'

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
