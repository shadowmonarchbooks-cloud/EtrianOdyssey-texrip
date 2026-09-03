from __future__ import annotations

"""0.13 runtime-hash evidence policy.

Exact decoded-RGBA matches may verify a runtime hash automatically. Perceptual
matches against an upscaled/repainted pack are useful evidence, but they are not
proof of identity and therefore remain explicit candidates until confirmed.
"""

from pathlib import Path
import json

from . import workspace as _workspace


def _append_unique(items: list, value) -> None:
    if value not in items:
        items.append(value)


def import_runtime_dump(workspace: Path, dump_dir: Path) -> dict:
    workspace = Path(workspace)
    dump_dir = Path(dump_dir)
    manifest = _workspace.load_manifest(workspace)
    assets = manifest['assets']

    direct: dict[str, list[dict]] = {}
    for asset in assets:
        direct.setdefault(asset.get('rgba_sha256'), []).append(asset)
        direct.setdefault(asset.get('rgba_flip_sha256'), []).append(asset)

    matched_exact = 0
    visual_candidates = 0
    ambiguous = 0
    unmatched: list[str] = []
    candidate_hash_matches = 0
    candidate_hash_mismatches = 0
    evidence: list[dict] = []

    entries = _workspace._pack_hash_images(dump_dir)
    for runtime_hash, png, _meta in entries:
        try:
            direct_hash, flipped_hash = _workspace.rgba_hashes(png)
        except Exception:
            unmatched.append(png.name)
            continue

        matches: list[dict] = []
        for key in (direct_hash, flipped_hash):
            matches.extend(direct.get(key, []))
        unique = {id(item): item for item in matches}

        target = None
        method = ''
        score = 0.0
        runner_up = 0.0
        status = ''
        if len(unique) == 1:
            target = next(iter(unique.values()))
            method = 'exact_rgba'
            status = 'verified'
            matched_exact += 1
        elif len(unique) > 1:
            ambiguous += 1
            continue
        else:
            target, score, runner_up = _workspace._visual_match(png, workspace, assets)
            if target is None:
                unmatched.append(png.name)
                continue
            method = 'visual_hd_downsample'
            status = 'candidate'
            visual_candidates += 1

        normalized_hash = str(runtime_hash).upper().zfill(16)
        if status == 'verified':
            verified = target.setdefault('verified_hashes', [])
            _append_unique(verified, normalized_hash)
            # If a previously proposed candidate is now exact, promote it and
            # remove the redundant pending entry.
            pending = target.setdefault('runtime_hash_candidates', [])
            target['runtime_hash_candidates'] = [
                item for item in pending
                if str(item.get('hash', '')).upper().zfill(16) != normalized_hash
            ]
            if normalized_hash == str(target.get('candidate_hash', '')).upper().zfill(16):
                candidate_hash_matches += 1
            else:
                candidate_hash_mismatches += 1
        else:
            pending = target.setdefault('runtime_hash_candidates', [])
            existing = next((
                item for item in pending
                if str(item.get('hash', '')).upper().zfill(16) == normalized_hash
            ), None)
            candidate = {
                'hash': normalized_hash,
                'method': method,
                'file': png.name,
                'score': round(score, 6),
                'runner_up_score': round(runner_up, 6),
                'status': 'candidate',
            }
            if existing is None:
                pending.append(candidate)
            else:
                existing.update(candidate)

        ev = target.setdefault('hash_evidence', [])
        ev_row = {
            'hash': normalized_hash,
            'method': method,
            'file': png.name,
            'status': status,
            'score': round(score, 6) if method != 'exact_rgba' else 0,
            'runner_up_score': round(runner_up, 6) if method != 'exact_rgba' else 0,
        }
        ev.append(ev_row)
        evidence.append({
            'asset_id': target['asset_id'],
            **ev_row,
            'file': str(png),
        })

    manifest['assets'] = assets
    protected_sources = _workspace.collect_protected_master_sources(workspace)
    for asset in assets:
        asset.setdefault('title_id', manifest.get('title_id', ''))
    if assets:
        _workspace.sync_azahar_master_pack(
            workspace, assets, protected_sources=protected_sources, use_candidates=True
        )
    _workspace.existing_manifest_path(workspace).write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8'
    )

    report = {
        'hash_images_found': len(entries),
        'matched_exact': matched_exact,
        'verified_total': matched_exact,
        'visual_candidates': visual_candidates,
        # Compatibility field retained for callers that display the old metric.
        'matched_visual_hd': visual_candidates,
        'matched_total': matched_exact,
        'candidate_hash_matches_on_exact': candidate_hash_matches,
        'candidate_hash_mismatches_on_exact': candidate_hash_mismatches,
        'candidate_hash_validation': (
            'PASS' if candidate_hash_matches > 0 and candidate_hash_mismatches == 0
            else ('FAIL' if candidate_hash_mismatches > 0 else 'NO_EXACT_SAMPLES')
        ),
        'ambiguous': ambiguous,
        'unmatched': len(unmatched),
        'unmatched_files': unmatched,
        'evidence': evidence,
        'policy': 'exact_rgba=verified; visual_hd_downsample=candidate_until_confirmed',
    }
    report_dir = workspace / _workspace.METADATA_DIR / 'reports'
    report_dir.mkdir(parents=True, exist_ok=True)
    (report_dir / 'runtime_import.json').write_text(
        json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8'
    )
    return report


def confirm_runtime_hash_candidate(workspace: Path, asset_id: str, runtime_hash: str) -> dict:
    """Explicitly promote one perceptual candidate to verified evidence."""
    workspace = Path(workspace)
    normalized_hash = str(runtime_hash).upper().zfill(16)
    manifest = _workspace.load_manifest(workspace)
    assets = manifest.get('assets', [])
    target = next((a for a in assets if str(a.get('asset_id')) == str(asset_id)), None)
    if target is None:
        raise KeyError(f'Unknown asset_id: {asset_id}')

    pending = target.get('runtime_hash_candidates') or []
    candidate = next((
        item for item in pending
        if str(item.get('hash', '')).upper().zfill(16) == normalized_hash
    ), None)
    if candidate is None:
        raise ValueError(f'No pending runtime-hash candidate {normalized_hash} for {asset_id}')

    verified = target.setdefault('verified_hashes', [])
    _append_unique(verified, normalized_hash)
    target['runtime_hash_candidates'] = [item for item in pending if item is not candidate]
    target.setdefault('hash_evidence', []).append({
        'hash': normalized_hash,
        'method': 'user_confirmed_visual_candidate',
        'file': candidate.get('file', ''),
        'status': 'verified',
        'score': candidate.get('score', 0),
        'runner_up_score': candidate.get('runner_up_score', 0),
    })

    protected_sources = _workspace.collect_protected_master_sources(workspace)
    for asset in assets:
        asset.setdefault('title_id', manifest.get('title_id', ''))
    _workspace.sync_azahar_master_pack(
        workspace, assets, protected_sources=protected_sources, use_candidates=True
    )
    manifest['assets'] = assets
    _workspace.existing_manifest_path(workspace).write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8'
    )
    return {
        'asset_id': asset_id,
        'hash': normalized_hash,
        'status': 'verified',
        'method': 'user_confirmed_visual_candidate',
    }


def install() -> None:
    _workspace.import_runtime_dump = import_runtime_dump
    _workspace.confirm_runtime_hash_candidate = confirm_runtime_hash_candidate
