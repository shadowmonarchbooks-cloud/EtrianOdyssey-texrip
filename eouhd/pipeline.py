from __future__ import annotations

from pathlib import Path
import json
import shutil
import re
from typing import Callable

from .forge_bridge import extract_romfs_selected
from .hpx import find_hpi_pairs, unpack_hpi_pair
from .farc import find_farc_files, unpack_farc
from .epl import find_epl_files, unpack_epl
from .strict_scan import decode_strict_file, has_strict_texture_signature, inventory_files, write_scan_report, ScanIssue
from .workspace import (
    ensure_workspace, reset_generated_workspace, collect_protected_master_sources,
    add_decoded_texture, dedupe_assets, save_manifest, sync_azahar_master_pack,
    cleanup_streamlined_workspace, make_contact_sheets, build_azahar_pack,
)
from .materials import build_3d_material_workspace, merge_material_bindings
from .profiles import detect_game_profile, profile_summary


def _unpack_all_hpx(dirs: dict[str, Path], emit: Callable[[str], None]) -> tuple[int, int, list[str]]:
    """Unpack top-level and nested HPI/HPB pairs exactly once."""
    processed: set[str] = set()
    pair_count = 0
    unpacked_count = 0
    errors: list[str] = []

    while True:
        candidates = list(find_hpi_pairs(dirs['romfs'])) + list(find_hpi_pairs(dirs['hpx']))
        todo = [p for p in candidates if str(p.resolve()) not in processed]
        if not todo:
            break
        for hpi in todo:
            processed.add(str(hpi.resolve()))
            pair_count += 1
            try:
                if dirs['romfs'] in hpi.parents:
                    rel = hpi.relative_to(dirs['romfs']).with_suffix('')
                    out = dirs['hpx'] / rel
                else:
                    # Put recursively discovered archives under a separate subtree,
                    # preventing an archive from unpacking over its own input files.
                    rel = hpi.relative_to(dirs['hpx']).with_suffix('')
                    out = dirs['hpx'] / '_nested' / rel
                written = unpack_hpi_pair(hpi, out)
                unpacked_count += len(written)
                emit(f'  {hpi.name}: {len(written)} files')
            except Exception as e:
                msg = f'{hpi}: {e}'
                errors.append(msg)
                emit(f'  WARNING: {msg}')
    return pair_count, unpacked_count, errors



def _unpack_all_farc(dirs: dict[str, Path], emit: Callable[[str], None]) -> tuple[int, int, list[str], list[dict]]:
    """Recursively unpack PMD-style FARC archives used by EOU 3D resources.

    Enemy/environment model resources can sit behind this archive layer, so BCH
    discovery must run *after* FARC extraction. Unknown/hash-only members are still
    emitted with deterministic names because model discovery is content-based.
    """
    processed: set[str] = set()
    archive_count = 0
    unpacked_count = 0
    errors: list[str] = []
    reports: list[dict] = []

    while True:
        candidates: list[Path] = []
        for root in (dirs['romfs'], dirs['hpx'], dirs['farc']):
            candidates.extend(list(find_farc_files(root)))
        todo = [p for p in candidates if str(p.resolve()) not in processed]
        if not todo:
            break
        for source in todo:
            processed.add(str(source.resolve()))
            archive_count += 1
            try:
                if dirs['romfs'] in source.parents:
                    rel = source.relative_to(dirs['romfs'])
                    out = dirs['farc'] / 'romfs' / rel.parent / f'{rel.stem}_farc'
                elif dirs['hpx'] in source.parents:
                    rel = source.relative_to(dirs['hpx'])
                    out = dirs['farc'] / 'hpx' / rel.parent / f'{rel.stem}_farc'
                else:
                    rel = source.relative_to(dirs['farc'])
                    out = dirs['farc'] / '_nested' / rel.parent / f'{rel.stem}_farc'
                written, metadata = unpack_farc(source, out)
                unpacked_count += len(written)
                reports.append({
                    'source': str(source),
                    'output': str(out),
                    **metadata,
                    'files_written': len(written),
                })
                emit(f'  FARC {source.name}: {len(written)} files')
            except Exception as exc:
                msg = f'{source}: {exc}'
                errors.append(msg)
                emit(f'  WARNING: {msg}')
    return archive_count, unpacked_count, errors, reports


def _unpack_all_epl(dirs: dict[str, Path], emit: Callable[[str], None]) -> tuple[int, int, int, list[dict], list[dict]]:
    """Recursively unpack conservative Atlus EPL general-resource packages.

    EOU/EO2U effect resources can sit behind EPL containers.  We do not guess
    member formats: the EPL table supplies exact payload bounds, and only members
    with known texture/model signatures are later admitted by strict_scan.
    """
    processed: set[str] = set()
    archive_count = 0
    unpacked_count = 0
    known_texture_members = 0
    errors: list[dict] = []
    reports: list[dict] = []

    while True:
        candidates: list[Path] = []
        for root in (dirs['romfs'], dirs['hpx'], dirs['farc'], dirs['epl']):
            candidates.extend(list(find_epl_files(root)))
        todo = [p for p in candidates if str(p.resolve()) not in processed]
        if not todo:
            break
        for source in todo:
            processed.add(str(source.resolve()))
            archive_count += 1
            try:
                if dirs['romfs'] in source.parents:
                    rel = source.relative_to(dirs['romfs'])
                    out = dirs['epl'] / 'romfs' / rel.parent / f'{rel.stem}_epl'
                elif dirs['hpx'] in source.parents:
                    rel = source.relative_to(dirs['hpx'])
                    out = dirs['epl'] / 'hpx' / rel.parent / f'{rel.stem}_epl'
                elif dirs['farc'] in source.parents:
                    rel = source.relative_to(dirs['farc'])
                    out = dirs['epl'] / 'farc' / rel.parent / f'{rel.stem}_epl'
                else:
                    rel = source.relative_to(dirs['epl'])
                    out = dirs['epl'] / '_nested' / rel.parent / f'{rel.stem}_epl'
                written, metadata = unpack_epl(source, out)
                unpacked_count += len(written)
                known_texture_members += int(metadata.get('known_texture_members', 0))
                members = metadata.pop('members', [])
                reports.append({
                    'source': str(source),
                    'output': str(out),
                    **metadata,
                    'files_written': len(written),
                    'member_samples': members[:20],
                })
                emit(f"  EPL {source.name}: {len(written)} members; {metadata.get('known_texture_members', 0)} known texture/model payload(s)")
            except Exception as exc:
                errors.append({'source': str(source), 'error': str(exc)})
                emit(f'  WARNING: EPL {source}: {exc}')
    return archive_count, unpacked_count, known_texture_members, errors, reports


def _strict_candidates(roots: list[Path]) -> list[Path]:
    out: list[Path] = []
    seen: set[str] = set()
    for root in roots:
        if not root.exists():
            continue
        for p in root.rglob('*'):
            if not p.is_file() or p.suffix.lower() in {'.hpi', '.hpb'}:
                continue
            key = str(p.resolve())
            if key in seen:
                continue
            seen.add(key)
            try:
                with p.open('rb') as f:
                    probe = f.read(0x100000)
            except OSError:
                continue
            if has_strict_texture_signature(probe, p.suffix):
                out.append(p)
    return out



def _bind_external_material_textures(assets: list[dict], model_diagnostics: list[dict]) -> dict:
    """Resolve model texture names to separately stored decoded assets.

    H3DMaterial texture slots are names. Usually the corresponding H3DTexture
    is inside the same BCH, but Atlus containers can keep resources separately.
    We only auto-bind when an exact texture name identifies exactly one decoded
    asset after CityHash deduplication. Ambiguous names remain unresolved.
    """
    by_name: dict[str, list[dict]] = {}
    for asset in assets:
        name = str(asset.get('texture_name') or '')
        if name:
            by_name.setdefault(name, []).append(asset)

    resolved = 0
    ambiguous: list[dict] = []
    missing: list[dict] = []
    for diagnostic in model_diagnostics:
        source = str(diagnostic.get('source') or '')
        offset = int(diagnostic.get('container_offset', 0) or 0)
        bindings_by_name = diagnostic.get('material_bindings_by_texture') or {}
        missing_names = set(str(x) for x in diagnostic.get('missing_material_texture_names') or [])
        for name in sorted(missing_names):
            candidates = by_name.get(name, [])
            if len(candidates) == 1:
                asset = candidates[0]
                temp = {
                    'material_bindings': [
                        {
                            **binding,
                            'source': source,
                            'container_offset': offset,
                            'texture_name': name,
                            'external_texture_binding': True,
                        }
                        for binding in bindings_by_name.get(name, [])
                        if isinstance(binding, dict)
                    ]
                }
                before = len(asset.get('material_bindings', []) or [])
                merge_material_bindings(asset, temp)
                after = len(asset.get('material_bindings', []) or [])
                added = max(0, after - before)
                if added:
                    asset['is_3d_material_texture'] = True
                    resolved += added
            elif len(candidates) > 1:
                ambiguous.append({
                    'source': source,
                    'container_offset': offset,
                    'texture_name': name,
                    'candidate_asset_ids': [str(a.get('asset_id') or '') for a in candidates],
                })
            else:
                missing.append({
                    'source': source,
                    'container_offset': offset,
                    'texture_name': name,
                })
    return {
        'external_bindings_resolved': resolved,
        'ambiguous_external_texture_names': ambiguous,
        'still_missing_external_texture_names': missing,
    }


def _retain_failure_diagnostics(
    dirs: dict[str, Path],
    profile_id: str,
    issues: list[ScanIssue],
    candidates: list[Path],
    model_diagnostics: list[dict],
    emit: Callable[[str], None],
    epl_errors: list[dict] | None = None,
    epl_candidates: list[Path] | None = None,
    epl_known_texture_members: int = 0,
    max_files: int = 6,
    max_total_bytes: int = 32 * 1024 * 1024,
) -> list[dict]:
    """Retain only tiny, failure-focused binary samples across workspace cleanup.

    0.8/0.9 deliberately removed the huge transient extraction tree.  That is
    correct for an upscaling workspace, but it made unseen container variants
    impossible to debug.  0.12 keeps a capped sample only when a relevant decode
    path still fails.
    """
    diag_root = dirs['diagnostics']
    diag_root.mkdir(parents=True, exist_ok=True)
    entries: list[dict] = []
    copied_sources: set[str] = set()
    total = 0

    def add(source: str | Path, reason: str) -> None:
        nonlocal total
        if len(entries) >= max_files:
            return
        p = Path(source)
        try:
            resolved = str(p.resolve())
            size = p.stat().st_size
        except OSError:
            return
        if resolved in copied_sources or size <= 0:
            return
        if size > max_total_bytes or total + size > max_total_bytes:
            return
        safe = re.sub(r'[^A-Za-z0-9._-]+', '_', p.name)[:100] or 'sample.bin'
        dest = diag_root / f'{len(entries)+1:02d}_{safe}'
        shutil.copy2(p, dest)
        copied_sources.add(resolved)
        total += size
        entries.append({
            'file': dest.name,
            'source': str(p),
            'reason': reason,
            'size': size,
        })

    # Preserve a few STEX files only if they still fail after the relaxed
    # UntoldUnpack-compatible size handling.
    for issue in issues:
        if issue.kind == 'stex_decode_error':
            add(issue.source, 'STEX decode failure')
            if len([x for x in entries if x['reason'] == 'STEX decode failure']) >= 3:
                break

    # Preserve BAM/BAM2 samples that actually generated BCH parsing failures.
    for issue in issues:
        if issue.kind not in {'bch_decode_error', 'bch_material_parse_error'}:
            continue
        if Path(issue.source).suffix.lower() in {'.bam', '.bam2'}:
            add(issue.source, issue.kind)

    # If EO2U still has ATBC/BAM2+BCH files but not one of them reached the BCH
    # diagnostics list, keep one representative sample. This is the exact 0.10
    # failure mode and costs only one model file rather than the entire RomFS.
    if profile_id == 'eo2u':
        parsed_bam2 = {
            str(Path(str(row.get('source') or '')).resolve())
            for row in model_diagnostics
            if row.get('format') == 'bch' and Path(str(row.get('source') or '')).suffix.lower() == '.bam2'
        }
        if not parsed_bam2:
            for candidate in candidates:
                if candidate.suffix.lower() != '.bam2':
                    continue
                try:
                    with candidate.open('rb') as f:
                        probe = f.read(0x100000)
                except OSError:
                    continue
                if probe.startswith(b'ATBC') and b'BCH\x00' in probe:
                    add(candidate, 'EO2U ATBC/BAM2 contains BCH but no BCH payload was parsed')
                    break

    # EPL is now a real extraction layer. Preserve failed packages, or one
    # representative package if EPL exists but exposes zero recognized texture/model
    # payloads. This avoids deleting the evidence for another EPL revision.
    for err in (epl_errors or [])[:2]:
        add(err.get('source', ''), 'EPL parse failure: ' + str(err.get('error', '')))
    if epl_candidates and epl_known_texture_members == 0:
        for candidate in epl_candidates:
            add(candidate, 'EPL parsed/discovered but no known texture/model member was exposed')
            break

    index = {
        'version': '0.12.0',
        'purpose': 'Small failure/investigation samples retained so transient extraction can still be cleaned.',
        'max_files': max_files,
        'max_total_bytes': max_total_bytes,
        'retained_files': len(entries),
        'retained_bytes': total,
        'samples': entries,
    }
    (diag_root / 'index.json').write_text(json.dumps(index, indent=2, ensure_ascii=False), encoding='utf-8')
    if entries:
        emit(f'  Retained {len(entries)} small diagnostic sample(s) under .eouhd/diagnostics ({total / 1024 / 1024:.1f} MiB).')
    return entries

def run_full_pipeline(rom: str | Path, workspace: str | Path, forge_root: str | Path,
                      log: Callable[[str], None] | None = None,
                      make_sheets: bool = False, build_pack: bool = True) -> dict:
    rom = Path(rom)
    workspace = Path(workspace)
    emit = log or (lambda _s: None)

    # Preserve edited/upscaled canonical master-pack images across reruns. 0.12
    # can also migrate genuinely edited legacy 0.7 HD masters on the first run.
    protected_sources = collect_protected_master_sources(workspace)
    if protected_sources:
        emit(f'Preserving {len(protected_sources)} manually edited/upscaled master texture(s).')
    dirs = reset_generated_workspace(workspace)

    emit('Stage 1/6 — identifying game and extracting archive/model/texture candidates from RomFS…')
    title_id, product_code, selected = extract_romfs_selected(rom, dirs['romfs'], forge_root)
    profile = detect_game_profile(title_id, product_code)
    emit(f'  Game: {profile_summary(profile)}')
    emit(f'  Title ID: {title_id}')
    emit(f'  Product code: {product_code}')
    emit(f'  Selected RomFS candidates: {len(selected)}')
    emit('  Expected archive families: ' + ', '.join(profile.archive_families))
    emit('  Expected 3D model families: ' + ', '.join(profile.model_families))

    emit('Stage 2/6 — recursively unpacking Atlus HPI/HPB, FARC, and EPL resource packages…')
    pair_count, unpacked_count, hpx_errors = _unpack_all_hpx(dirs, emit)
    emit(f'  HPI/HPB pairs: {pair_count}; unpacked files: {unpacked_count}')
    farc_count, farc_unpacked_count, farc_errors, farc_reports = _unpack_all_farc(dirs, emit)
    emit(f'  FARC archives: {farc_count}; unpacked files: {farc_unpacked_count}')
    (dirs['reports'] / 'farc_inventory.json').write_text(
        json.dumps({
            'version': '0.12.0',
            'archives_found': farc_count,
            'files_unpacked': farc_unpacked_count,
            'errors': farc_errors,
            'archives': farc_reports,
        }, indent=2, ensure_ascii=False), encoding='utf-8'
    )

    epl_candidates_before = []
    for _root in (dirs['romfs'], dirs['hpx'], dirs['farc']):
        epl_candidates_before.extend(list(find_epl_files(_root)))
    epl_count, epl_unpacked_count, epl_known_texture_members, epl_errors, epl_reports = _unpack_all_epl(dirs, emit)
    emit(f'  EPL archives: {epl_count}; unpacked members: {epl_unpacked_count}; known texture/model members: {epl_known_texture_members}')
    (dirs['reports'] / 'epl_inventory.json').write_text(
        json.dumps({
            'version': '0.12.0',
            'archives_found': epl_count,
            'files_unpacked': epl_unpacked_count,
            'known_texture_members': epl_known_texture_members,
            'errors': epl_errors,
            'archives': epl_reports,
        }, indent=2, ensure_ascii=False), encoding='utf-8'
    )

    emit(f'Stage 3/6 — inventorying STEX + EPL members + {profile.short_name} 3D model resources…')
    scan_roots = [dirs['romfs'], dirs['hpx'], dirs['farc'], dirs['epl']]
    inventory = inventory_files(scan_roots, dirs['reports'])
    candidates = _strict_candidates(scan_roots)
    emit(f"  STEX files seen: {inventory.get('stex_files', 0)}")
    emit(f"  EPL files seen after unpacking: {inventory.get('epl_files', 0)}; .EP resources: {inventory.get('ep_files', 0)}")
    emit(f"  CTPK files seen: {inventory.get('ctpk_files', 0)}; files with embedded STEX magic: {inventory.get('embedded_stex_files', 0)}")
    emit(f"  ATBC/BAM files seen: {inventory.get('atbc_files', 0)}")
    emit(f"  CGFX-bearing files seen: {inventory.get('cgfx_files', 0)}")
    emit(f"  Wrapped BCH files seen: {inventory.get('wrapped_bch_files', 0)}")
    emit(f"  BAM/BAM2 files containing BCH: {inventory.get('bam_bch_files', 0)}")
    emit(f'  Strict decode candidates: {len(candidates)}')

    emit('Stage 4/6 — decoding 3D textures and parsing CGFX/MTOB + BCH/H3D material bindings…')
    assets: list[dict] = []
    issues: list[ScanIssue] = []
    decoded_textures = 0
    parser_counts: dict[str, int] = {}
    model_diagnostics: list[dict] = []
    for i, p in enumerate(candidates, 1):
        texs, file_issues, file_model_diagnostics = decode_strict_file(p, Path(forge_root), title_id)
        issues.extend(file_issues)
        model_diagnostics.extend(file_model_diagnostics)
        for j, tex in enumerate(texs):
            parser = tex.get('parser_used', 'unknown')
            parser_counts[parser] = parser_counts.get(parser, 0) + 1
            assets.append(add_decoded_texture(
                workspace, title_id, p, tex, j,
                protected_masters=set(),
            ))
            decoded_textures += 1
        if i % 100 == 0 or i == len(candidates):
            emit(f'  Processed {i}/{len(candidates)} files; {decoded_textures} textures decoded…')

    assets = dedupe_assets(assets)
    for asset in assets:
        asset['game_id'] = profile.id
        asset['game'] = profile.display_name
        asset['product_code'] = product_code
    external_binding_report = _bind_external_material_textures(assets, model_diagnostics)

    # Low-level structural report, including EOU1 ATBC/CGFX and BCH/H3D.
    cgfx_rows = [row for row in model_diagnostics if row.get('format') == 'cgfx']
    bch_rows = [row for row in model_diagnostics if row.get('format') == 'bch']
    bam2_bch_rows = [
        row for row in bch_rows
        if Path(str(row.get('source') or '')).suffix.lower() in {'.bam', '.bam2'}
    ]
    inventory3d = {
        'version': '0.12.0',
        'game_id': profile.id,
        'game': profile.display_name,
        'product_code': product_code,
        'title_id': title_id,
        'payloads': len(model_diagnostics),
        'cgfx_payloads': len(cgfx_rows),
        'bch_payloads': len(bch_rows),
        'bam2_bch_payloads': len(bam2_bch_rows),
        'bam2_models_found': sum(int(row.get('model_count', 0)) for row in bam2_bch_rows),
        'bam2_materials_found': sum(int(row.get('materials_found', 0)) for row in bam2_bch_rows),
        'bam2_decoded_textures': sum(int(row.get('decoded_textures', 0)) for row in bam2_bch_rows),
        'models_found': sum(int(row.get('model_count', 0)) for row in model_diagnostics),
        'materials_found': sum(int(row.get('materials_found', 0)) for row in model_diagnostics),
        'texture_descriptors_found': sum(int(row.get('texture_descriptors_found', 0)) for row in model_diagnostics),
        'decoded_3d_textures': sum(int(row.get('decoded_textures', 0)) for row in model_diagnostics),
        'missing_material_texture_references_before_external_resolution': sum(len(row.get('missing_material_texture_names') or []) for row in model_diagnostics),
        'external_texture_binding': external_binding_report,
        'missing_material_texture_references_after_external_resolution': len(external_binding_report.get('still_missing_external_texture_names', [])) + len(external_binding_report.get('ambiguous_external_texture_names', [])),
        'payload_details': model_diagnostics,
    }
    (dirs['reports'] / '3d_model_inventory.json').write_text(
        json.dumps(inventory3d, indent=2, ensure_ascii=False), encoding='utf-8'
    )
    # Keep the old filename as a compatibility breadcrumb, but make it clear it
    # is no longer BCH-only.
    (dirs['reports'] / 'bch_3d_inventory.json').write_text(
        json.dumps(inventory3d, indent=2, ensure_ascii=False), encoding='utf-8'
    )

    emit('Stage 5/6 — exporting material-referenced 3D textures and exact alpha channels…')
    material_report = build_3d_material_workspace(workspace, assets)
    emit(f"  3D materials (CGFX/MTOB + BCH/H3D): {material_report.get('materials_found', 0)}")
    emit(f"  3D material-referenced assets: {material_report.get('assets_referenced_by_3d_materials', 0)}")
    emit(f"  Explicit stored texture-alpha/channel planes: {material_report.get('explicit_texture_alpha_channels', 0)}")
    emit(f"  Constant alpha inputs from RGB/ETC1 formats: {material_report.get('constant_texture_alpha_inputs', 0)}")
    emit(f"  Resolved material alpha pipelines: {material_report.get('resolved_material_alphas', 0)}")
    emit(f"  Unresolved material alpha pipelines: {material_report.get('unresolved_material_alphas', 0)}")

    emit('  Building unique categorized azahar_pack_master…')
    master_path = sync_azahar_master_pack(
        workspace, assets, protected_sources=protected_sources, use_candidates=True,
    )
    emit(f'  Editable master pack: {master_path}')
    save_manifest(workspace, title_id, assets, str(rom), version='0.12.0', game_profile=profile.to_dict(), product_code=product_code)
    scan_stats = {
        'version': '0.12.0',
        'game_id': profile.id,
        'game': profile.display_name,
        'product_code': product_code,
        'title_id': title_id,
        'strict_candidate_files': len(candidates),
        'inventory_epl_files': inventory.get('epl_files', 0),
        'inventory_ep_files': inventory.get('ep_files', 0),
        'inventory_ctpk_files': inventory.get('ctpk_files', 0),
        'inventory_embedded_stex_files': inventory.get('embedded_stex_files', 0),
        'decoded_before_dedup': decoded_textures,
        'unique_assets': len(assets),
        'parser_counts': parser_counts,
        'hpx_errors': hpx_errors,
        'farc_errors': farc_errors,
        'farc_archives': farc_count,
        'farc_files': farc_unpacked_count,
        'epl_archives': epl_count,
        'epl_files': epl_unpacked_count,
        'epl_known_texture_members': epl_known_texture_members,
        'epl_errors': epl_errors,
        'protected_masters': len(protected_sources),
        'models_found': sum(int(row.get('model_count', 0)) for row in model_diagnostics),
        'cgfx_models_found': sum(int(row.get('model_count', 0)) for row in model_diagnostics if row.get('format') == 'cgfx'),
        'bch_models_found': sum(int(row.get('model_count', 0)) for row in model_diagnostics if row.get('format') == 'bch'),
        'model_materials_found': sum(int(row.get('materials_found', 0)) for row in model_diagnostics),
        'cgfx_materials_found': sum(int(row.get('materials_found', 0)) for row in model_diagnostics if row.get('format') == 'cgfx'),
        'bch_model_materials_found': sum(int(row.get('materials_found', 0)) for row in model_diagnostics if row.get('format') == 'bch'),
        'bam2_bch_payloads': len(bam2_bch_rows),
        'bam2_models_found': sum(int(row.get('model_count', 0)) for row in bam2_bch_rows),
        'bam2_materials_found': sum(int(row.get('materials_found', 0)) for row in bam2_bch_rows),
        'bam2_decoded_textures': sum(int(row.get('decoded_textures', 0)) for row in bam2_bch_rows),
        'stex_decode_errors': sum(1 for issue in issues if issue.kind == 'stex_decode_error'),
        'missing_3d_material_texture_references': len(external_binding_report.get('still_missing_external_texture_names', [])) + len(external_binding_report.get('ambiguous_external_texture_names', [])),
        'external_3d_texture_bindings_resolved': external_binding_report.get('external_bindings_resolved', 0),
        'materials_found': material_report.get('materials_found', 0),
        'material_texture_bindings': material_report.get('material_texture_bindings', 0),
        'explicit_texture_alpha_channels': material_report.get('explicit_texture_alpha_channels', 0),
        'constant_texture_alpha_inputs': material_report.get('constant_texture_alpha_inputs', 0),
        'resolved_material_alphas': material_report.get('resolved_material_alphas', 0),
        'unresolved_material_alphas': material_report.get('unresolved_material_alphas', 0),
    }
    write_scan_report(dirs['reports'], issues, scan_stats)
    emit(f'  Unique assets: {len(assets)}')
    emit(f'  Decoder issues quarantined/reported: {len(issues)}')

    emit('Stage 6/6 — building streamlined upscaling workspace…')
    sheets = []
    if make_sheets:
        sheets = make_contact_sheets(workspace, hide_names=True)
        emit(f'  Contact sheets: {len(sheets)}')
    pack_path = None
    if build_pack:
        pack_path = build_azahar_pack(workspace, use_candidates=True)
        emit(f'  Deployment Azahar pack: {pack_path}')

    diagnostic_samples = _retain_failure_diagnostics(
        dirs, profile.id, issues, candidates, model_diagnostics, emit,
        epl_errors=epl_errors, epl_candidates=epl_candidates_before,
        epl_known_texture_members=epl_known_texture_members,
    )

    # Small top-level summary specifically useful when reporting extraction gaps.
    summary = {
        'game_id': profile.id,
        'game': profile.display_name,
        'game_short_name': profile.short_name,
        'product_code': product_code,
        'title_id': title_id,
        'assets': len(assets),
        'decoded_before_dedup': decoded_textures,
        'hpx_pairs': pair_count,
        'hpx_files': unpacked_count,
        'farc_archives': farc_count,
        'farc_files': farc_unpacked_count,
        'epl_archives': epl_count,
        'epl_files': epl_unpacked_count,
        'epl_known_texture_members': epl_known_texture_members,
        'epl_errors': len(epl_errors),
        'strict_candidate_files': len(candidates),
        'atbc_files': inventory.get('atbc_files', 0),
        'cgfx_files': inventory.get('cgfx_files', 0),
        'wrapped_bch_files': inventory.get('wrapped_bch_files', 0),
        'bam_bch_files': inventory.get('bam_bch_files', 0),
        'stex_files': inventory.get('stex_files', 0),
        'inventory_epl_files': inventory.get('epl_files', 0),
        'inventory_ep_files': inventory.get('ep_files', 0),
        'inventory_ctpk_files': inventory.get('ctpk_files', 0),
        'inventory_embedded_stex_files': inventory.get('embedded_stex_files', 0),
        'parser_counts': parser_counts,
        'issues': len(issues),
        'models_found': sum(int(row.get('model_count', 0)) for row in model_diagnostics),
        'cgfx_models_found': sum(int(row.get('model_count', 0)) for row in model_diagnostics if row.get('format') == 'cgfx'),
        'bch_models_found': sum(int(row.get('model_count', 0)) for row in model_diagnostics if row.get('format') == 'bch'),
        'model_materials_found': sum(int(row.get('materials_found', 0)) for row in model_diagnostics),
        'cgfx_materials_found': sum(int(row.get('materials_found', 0)) for row in model_diagnostics if row.get('format') == 'cgfx'),
        'bch_model_materials_found': sum(int(row.get('materials_found', 0)) for row in model_diagnostics if row.get('format') == 'bch'),
        'bam2_bch_payloads': len(bam2_bch_rows),
        'bam2_models_found': sum(int(row.get('model_count', 0)) for row in bam2_bch_rows),
        'bam2_materials_found': sum(int(row.get('materials_found', 0)) for row in bam2_bch_rows),
        'bam2_decoded_textures': sum(int(row.get('decoded_textures', 0)) for row in bam2_bch_rows),
        'stex_decode_errors': sum(1 for issue in issues if issue.kind == 'stex_decode_error'),
        'missing_3d_material_texture_references': len(external_binding_report.get('still_missing_external_texture_names', [])) + len(external_binding_report.get('ambiguous_external_texture_names', [])),
        'external_3d_texture_bindings_resolved': external_binding_report.get('external_bindings_resolved', 0),
        'materials_found': material_report.get('materials_found', 0),
        'assets_referenced_by_3d_materials': material_report.get('assets_referenced_by_3d_materials', 0),
        'explicit_texture_alpha_channels': material_report.get('explicit_texture_alpha_channels', 0),
        'constant_texture_alpha_inputs': material_report.get('constant_texture_alpha_inputs', 0),
        'resolved_material_alphas': material_report.get('resolved_material_alphas', 0),
        'unresolved_material_alphas': material_report.get('unresolved_material_alphas', 0),
        'heuristic_grayscale_masks_generated': 0,
        'contact_sheets': len(sheets),
        'diagnostic_samples': len(diagnostic_samples),
        'master_pack_path': str(master_path),
        'pack_path': str(pack_path) if pack_path else '',
    }
    (dirs['reports']/'run_summary.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    cleanup_streamlined_workspace(workspace)
    emit('  Removed transient extraction/original/master/material image trees; only the two packs and lightweight .eouhd metadata/diagnostics remain.')
    emit('Done.')
    return summary
