from __future__ import annotations

"""Safety wrappers for the frozen 0.13 Python workspace implementation.

The 0.12 workspace module remains the behavioral reference, but its destructive
pack refreshes depended too heavily on the previous manifest.  This module wraps
those public entry points so the editable Azahar master pack is itself recoverable
state and directory promotion is transactional.
"""

from pathlib import Path
import json
import shutil
from typing import Any

from . import workspace as _legacy

WORKSPACE_MARKER = 'workspace.json'
WORKSPACE_KIND = 'eo-texrip-workspace'
WORKSPACE_SCHEMA = 1

_ORIG_ENSURE = _legacy.ensure_workspace
_ORIG_COLLECT = _legacy.collect_protected_master_sources
_ORIG_RESET = _legacy.reset_generated_workspace
_ORIG_CLEANUP = _legacy.cleanup_streamlined_workspace


def _marker_path(root: Path) -> Path:
    return Path(root) / _legacy.METADATA_DIR / WORKSPACE_MARKER


def _write_workspace_marker(root: Path) -> Path:
    marker = _marker_path(root)
    marker.parent.mkdir(parents=True, exist_ok=True)
    data = {
        'kind': WORKSPACE_KIND,
        'schema_version': WORKSPACE_SCHEMA,
        'master_pack': _legacy.MASTER_PACK_DIR,
        'deployment_pack': _legacy.DEPLOY_PACK_DIR,
    }
    marker.write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
    return marker


def _has_workspace_marker(root: Path) -> bool:
    marker = _marker_path(root)
    if not marker.is_file():
        return False
    try:
        data = json.loads(marker.read_text(encoding='utf-8'))
    except Exception:
        return False
    return data.get('kind') == WORKSPACE_KIND and int(data.get('schema_version', 0) or 0) >= 1


def ensure_workspace(root: Path) -> dict[str, Path]:
    dirs = _ORIG_ENSURE(Path(root))
    _write_workspace_marker(Path(root))
    return dirs


def _live_master_state(root: Path) -> dict[str, Any]:
    """Read recoverable state directly from azahar_pack_master.

    A missing/corrupt manifest must not make the editable pack disposable.  The
    live pack.json mapping is sufficient to reconnect runtime hashes to renamed
    PNGs, while the full file inventory lets transactional rebuilds retain files
    that are not represented in a stale manifest.
    """
    root = Path(root)
    pack_root = root / _legacy.MASTER_PACK_DIR
    state: dict[str, Any] = {
        'pack_root': pack_root,
        'title_roots': [],
        'hash_to_file': {},
        'files': [],
    }
    if not pack_root.is_dir():
        return state

    load_textures = pack_root / 'load' / 'textures'
    if load_textures.is_dir():
        state['title_roots'] = [p for p in load_textures.iterdir() if p.is_dir()]

    all_files = [p for p in pack_root.rglob('*') if p.is_file()]
    state['files'] = all_files

    for title_root in state['title_roots']:
        pack_json = title_root / 'pack.json'
        if not pack_json.is_file():
            continue
        try:
            pack = json.loads(pack_json.read_text(encoding='utf-8'))
        except Exception:
            continue
        mappings = pack.get('textures')
        if not isinstance(mappings, dict):
            continue

        by_name: dict[str, list[Path]] = {}
        for p in title_root.rglob('*'):
            if p.is_file():
                by_name.setdefault(p.name, []).append(p)
        for raw_hash, raw_value in mappings.items():
            try:
                hh = f'{int(str(raw_hash), 16):016X}'
            except Exception:
                continue
            values = raw_value if isinstance(raw_value, list) else [raw_value]
            resolved: list[Path] = []
            for value in values:
                if not isinstance(value, str):
                    continue
                direct = title_root / value
                if direct.is_file():
                    resolved.append(direct)
                    continue
                matches = by_name.get(Path(value).name, [])
                if len(matches) == 1:
                    resolved.append(matches[0])
            unique = list(dict.fromkeys(resolved))
            if len(unique) == 1:
                state['hash_to_file'][hh] = unique[0]
    return state


def collect_protected_master_sources(root: Path) -> dict[str, Path]:
    """Protect manifest-known edits plus all recoverable live pack mappings.

    When no valid manifest exists, mapped live master images are conservatively
    treated as user-owned.  This prefers retaining a stale original over deleting
    a possible upscale/retouch, which is the only safe default for the editable
    source-of-truth tree.
    """
    root = Path(root)
    protected = dict(_ORIG_COLLECT(root))
    live = _live_master_state(root)
    for hh, image in live['hash_to_file'].items():
        protected.setdefault(f'hash:{hh}', image)
        protected.setdefault(image.name, image)
    return protected


def _validate_pack_root(pack_root: Path) -> None:
    load_root = Path(pack_root) / 'load' / 'textures'
    if not load_root.is_dir():
        raise RuntimeError(f'Pack staging root has no load/textures directory: {pack_root}')
    pack_jsons = list(load_root.glob('*/pack.json'))
    if len(pack_jsons) != 1:
        raise RuntimeError(f'Expected exactly one staged pack.json, found {len(pack_jsons)}')
    pack_json = pack_jsons[0]
    title_root = pack_json.parent
    try:
        data = json.loads(pack_json.read_text(encoding='utf-8'))
    except Exception as exc:
        raise RuntimeError(f'Invalid staged pack.json: {exc}') from exc
    mappings = data.get('textures')
    if not isinstance(mappings, dict):
        raise RuntimeError('Staged pack.json textures mapping is missing or invalid')

    by_name: dict[str, list[Path]] = {}
    for p in title_root.rglob('*'):
        if p.is_file() and p.name != 'pack.json':
            by_name.setdefault(p.name, []).append(p)
    for raw_hash, raw_value in mappings.items():
        try:
            int(str(raw_hash), 16)
        except ValueError as exc:
            raise RuntimeError(f'Invalid runtime hash in staged pack.json: {raw_hash!r}') from exc
        values = raw_value if isinstance(raw_value, list) else [raw_value]
        if not values:
            raise RuntimeError(f'Runtime hash {raw_hash} has no mapped texture')
        for value in values:
            if not isinstance(value, str) or not value:
                raise RuntimeError(f'Runtime hash {raw_hash} has an invalid mapped filename')
            direct = title_root / value
            if direct.is_file():
                continue
            matches = by_name.get(Path(value).name, [])
            if len(matches) != 1:
                raise RuntimeError(
                    f'Runtime hash {raw_hash} maps to {value!r}, which resolves to {len(matches)} files'
                )


def _promote_directory(staged: Path, final: Path) -> None:
    """Promote one validated directory while retaining rollback until success."""
    staged = Path(staged)
    final = Path(final)
    backup = final.parent / f'.{final.name}.rollback'
    if backup.exists():
        shutil.rmtree(backup)

    had_final = final.exists()
    if had_final:
        final.replace(backup)
    try:
        staged.replace(final)
    except Exception:
        if final.exists():
            shutil.rmtree(final)
        if had_final and backup.exists():
            backup.replace(final)
        raise
    else:
        if backup.exists():
            shutil.rmtree(backup)


def _preferred_live_file(asset: dict, live: dict[str, Any]) -> Path | None:
    matches: list[Path] = []
    for hh in _legacy._asset_hashes(asset, True):
        p = live['hash_to_file'].get(hh)
        if p is not None and p.is_file():
            matches.append(p)
    unique = list(dict.fromkeys(matches))
    return unique[0] if len(unique) == 1 else None


def _copy_unclaimed_live_files(live: dict[str, Any], old_pack_root: Path, staged_root: Path) -> None:
    """Retain user files not claimed by the regenerated catalog.

    Existing files win only when the staged path does not already exist.  This
    prevents an untracked stale file from overwriting a newly generated asset,
    while still making manifest loss non-destructive.
    """
    if not old_pack_root.is_dir():
        return
    for source in live.get('files', []):
        if not source.is_file():
            continue
        try:
            rel = source.relative_to(old_pack_root)
        except ValueError:
            continue
        if rel.name in {'pack.json', 'INSTALL_TO_AZAHAR.txt'}:
            continue
        dest = staged_root / rel
        if dest.exists():
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, dest)


def sync_azahar_master_pack(
    workspace: Path,
    assets: list[dict],
    protected_sources: dict[str, Path] | None = None,
    use_candidates: bool = True,
) -> Path:
    workspace = Path(workspace)
    ensure_workspace(workspace)
    protected_sources = protected_sources or collect_protected_master_sources(workspace)
    if not assets:
        raise ValueError('No assets available for master-pack creation.')
    title_id = str(assets[0].get('title_id') or '').upper().zfill(16)
    if not title_id.strip('0'):
        raise ValueError('Missing Title ID for master-pack creation.')

    _legacy.assign_friendly_pack_names(assets)
    final_root = workspace / _legacy.MASTER_PACK_DIR
    temp_root = workspace / _legacy.METADATA_DIR / '_master_build'
    if temp_root.exists():
        shutil.rmtree(temp_root)

    live = _live_master_state(workspace)
    game_name = str((assets[0].get('game') if assets else '') or '')
    title_root = _legacy._write_pack_metadata(temp_root, title_id, '0.12.0', game_name)
    mappings: dict[str, str] = {}

    for asset in assets:
        hashes = _legacy._asset_hashes(asset, use_candidates)
        if not hashes:
            continue
        source = workspace / str(asset.get('master') or '')
        if not source.is_file():
            source = workspace / str(asset.get('original') or '')
        if not source.is_file():
            continue

        live_file = _preferred_live_file(asset, live)
        if live_file is not None:
            # Respect intentional pack.json rename. Basename is Azahar's lookup
            # identity; category is still derived from the current asset model.
            asset['pack_filename'] = live_file.name
        filename = _legacy.friendly_pack_filename(asset)
        category = _legacy._safe_category(asset.get('category', 'misc'))
        dest = title_root / category / filename
        dest.parent.mkdir(parents=True, exist_ok=True)

        protected = _legacy._protected_source_for_asset(asset, protected_sources)
        if protected is None and live_file is not None:
            protected = live_file
        shutil.copy2(protected or source, dest)

        rel = f'{_legacy.MASTER_PACK_DIR}/' + str(dest.relative_to(temp_root)).replace('\\', '/')
        asset['master_files'] = [rel]
        asset['master'] = rel
        asset['pack_filename'] = filename
        asset['pack_hashes'] = hashes
        asset.pop('original', None)
        for hh in hashes:
            mappings[hh] = filename

    # Preserve still-live mappings that are not represented by the fresh scan.
    for hh, source in live['hash_to_file'].items():
        if hh in mappings:
            continue
        existing_name = source.name
        rel = source.relative_to(final_root) if final_root in source.parents else None
        if rel is not None:
            dest = temp_root / rel
            if not dest.exists():
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, dest)
        mappings[hh] = existing_name

    _copy_unclaimed_live_files(live, final_root, temp_root)
    _legacy._write_pack_texture_map(title_root, mappings)
    (temp_root / 'INSTALL_TO_AZAHAR.txt').write_text(
        'EO-TexRip 0.13 legacy master pack\n'
        '================================\n\n'
        'This directory is user-owned editable source data.\n'
        'Runtime hashes are mapped through pack.json; intentional renames recorded there are preserved on rerun.\n',
        encoding='utf-8',
    )
    _validate_pack_root(temp_root)
    _promote_directory(temp_root, final_root)
    return final_root


def build_azahar_pack(workspace: Path, use_candidates: bool = True) -> Path:
    workspace = Path(workspace)
    ensure_workspace(workspace)
    manifest = _legacy.load_manifest(workspace)
    title_id = str(manifest['title_id']).upper().zfill(16)
    assets = manifest.get('assets', [])
    master_root = workspace / _legacy.MASTER_PACK_DIR
    if not master_root.exists():
        for asset in assets:
            asset.setdefault('title_id', title_id)
        sync_azahar_master_pack(workspace, assets, use_candidates=use_candidates)

    deploy_root = workspace / _legacy.DEPLOY_PACK_DIR
    temp_root = workspace / _legacy.METADATA_DIR / '_deploy_build'
    if temp_root.exists():
        shutil.rmtree(temp_root)
    game_name = str(manifest.get('game') or (manifest.get('game_profile') or {}).get('display_name') or '')
    title_root = _legacy._write_pack_metadata(temp_root, title_id, '0.12.0', game_name)

    mappings: dict[str, str] = {}
    for asset in assets:
        hashes = _legacy._asset_hashes(asset, use_candidates)
        if not hashes:
            continue
        filename = str(asset.get('pack_filename') or _legacy.friendly_pack_filename(asset))
        primary = workspace / str(asset.get('master') or '')
        if not primary.is_file():
            primary = _legacy._find_pack_file_by_name(master_root, filename) or Path()
        if not primary.is_file():
            for candidate_hash in _legacy._asset_hashes(asset, True):
                found = _legacy._find_pack_file_by_name(
                    master_root, _legacy.canonical_pack_filename(asset, candidate_hash)
                )
                if found is not None:
                    primary = found
                    break
        if not primary.is_file():
            continue
        category = _legacy._safe_category(asset.get('category', 'misc'))
        dest = title_root / category / filename
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(primary, dest)
        for hh in hashes:
            mappings[hh] = filename
        asset['pack_filename'] = filename
        asset['pack_hashes'] = hashes

    _legacy._write_pack_texture_map(title_root, mappings)
    (temp_root / 'INSTALL_TO_AZAHAR.txt').write_text(
        'EO-TexRip 0.13 legacy Azahar deployment pack\n'
        '============================================\n\n'
        'Generated from azahar_pack_master. Edit the master pack, not this deployment copy.\n',
        encoding='utf-8',
    )
    _validate_pack_root(temp_root)
    _promote_directory(temp_root, deploy_root)

    manifest['assets'] = assets
    _legacy.existing_manifest_path(workspace).write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8'
    )
    reports = workspace / _legacy.METADATA_DIR / 'reports'
    reports.mkdir(parents=True, exist_ok=True)
    (reports / 'azahar_pack_report.json').write_text(json.dumps({
        'title_id': title_id,
        'assets_exported': len({v for v in mappings.values()}),
        'hash_mappings_exported': len(mappings),
        'source_of_truth': _legacy.MASTER_PACK_DIR,
        'transactional_promotion': True,
        'mapping_mode': 'verified-first; candidate fallback' if use_candidates else 'verified-only',
    }, indent=2), encoding='utf-8')
    return deploy_root / 'load' / 'textures' / title_id


def reset_generated_workspace(root: Path) -> dict[str, Path]:
    """Reset only transient extraction state; retain prior known-good packs."""
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    ensure_workspace(root)
    meta = root / _legacy.METADATA_DIR
    for path in (meta / 'work', meta / 'contact_sheets'):
        if path.exists():
            shutil.rmtree(path)
    # Reports/diagnostics and the deployment pack are deliberately not removed
    # before the new run succeeds. Individual reports may be refreshed in place.
    return ensure_workspace(root)


def cleanup_streamlined_workspace(root: Path) -> None:
    root = Path(root)
    if not _has_workspace_marker(root):
        raise RuntimeError(
            'Refusing destructive workspace cleanup: EO-TexRip workspace marker is missing or invalid.'
        )
    _ORIG_CLEANUP(root)


def install() -> None:
    """Install 0.13 safety wrappers onto the legacy workspace module."""
    _legacy.ensure_workspace = ensure_workspace
    _legacy.collect_protected_master_sources = collect_protected_master_sources
    _legacy.sync_azahar_master_pack = sync_azahar_master_pack
    _legacy.build_azahar_pack = build_azahar_pack
    _legacy.reset_generated_workspace = reset_generated_workspace
    _legacy.cleanup_streamlined_workspace = cleanup_streamlined_workspace
