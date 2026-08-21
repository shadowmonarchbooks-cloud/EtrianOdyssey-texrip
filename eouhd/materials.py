from __future__ import annotations

"""Material-aware 3D texture/alpha export for EOU CGFX/MTOB and BCH/H3D models.

v0.12 intentionally does *not* infer alpha from image appearance. A texture is
considered a 3D material texture only when model material metadata references it,
and a channel is exported as alpha only when the PICA200/CGFX alpha combiner
actually uses that texture slot/channel.
"""

from pathlib import Path
import csv
import hashlib
import json
import re
import shutil
from typing import Any

import numpy as np
from PIL import Image

from .bch_materials import parse_bch_materials, bindings_by_texture as bch_bindings_by_texture
from .cgfx_materials import parse_cgfx_materials, bindings_by_texture as cgfx_bindings_by_texture, find_cgfx_payloads


_ONE_MINUS = {1, 3, 5, 7}
_CHANNEL_BY_OPERAND = {
    0: 3,  # Alpha
    1: 3,  # OneMinusAlpha
    2: 0,  # Red
    3: 0,  # OneMinusRed
    4: 1,  # Green
    5: 1,  # OneMinusGreen
    6: 2,  # Blue
    7: 2,  # OneMinusBlue
}

# Formats whose encoded texture payload actually stores an alpha component.
# For RGB/L/ETC1 formats, sampling `.a` returns a constant 1.0 in the material
# pipeline; that is material state, not an alpha image worth exporting.
_STORED_ALPHA_FORMATS = {0x0, 0x2, 0x4, 0x5, 0x8, 0x9, 0xB, 0xD}

def _format_stores_alpha(fmt: int) -> bool:
    return int(fmt) in _STORED_ALPHA_FORMATS


def _safe(value: str, fallback: str = 'unnamed') -> str:
    cleaned = re.sub(r'[^A-Za-z0-9._-]+', '_', str(value or '')).strip('._-')
    return cleaned[:96] or fallback


def _source_group(source: str, container_offset: int = 0) -> str:
    stem = _safe(Path(source).stem, 'model')
    token = hashlib.sha1(f'{source}|{container_offset}'.encode('utf-8', errors='replace')).hexdigest()[:10]
    return f'{stem}_{token}'


def _load_rgba(path: Path) -> np.ndarray:
    with Image.open(path) as im:
        return np.asarray(im.convert('RGBA'), dtype=np.uint8).copy()


def alpha_plane_from_operand(rgba: np.ndarray, operand_id: int) -> np.ndarray:
    """Return the exact channel selected by a PICA alpha operand.

    PICA alpha operands can consume A/R/G/B or their one-minus forms. This is
    why an opaque ETC1 texture can still be the true alpha mask: a material may
    explicitly consume Texture1.Red rather than Texture1.Alpha.
    """
    if operand_id not in _CHANNEL_BY_OPERAND:
        raise ValueError(f'Unsupported PICA alpha operand {operand_id}')
    arr = np.asarray(rgba, dtype=np.uint8)
    if arr.ndim != 3 or arr.shape[2] != 4:
        raise ValueError('RGBA image expected')
    plane = arr[:, :, _CHANNEL_BY_OPERAND[operand_id]].copy()
    if operand_id in _ONE_MINUS:
        plane = 255 - plane
    return plane


def _binding_identity(binding: dict) -> str:
    return json.dumps(binding, sort_keys=True, ensure_ascii=False, separators=(',', ':'))


def merge_material_bindings(target: dict, source: dict) -> None:
    """Merge material provenance when hash-deduping identical runtime textures."""
    current = target.setdefault('material_bindings', [])
    seen = {_binding_identity(x) for x in current if isinstance(x, dict)}
    for binding in source.get('material_bindings', []) or []:
        if not isinstance(binding, dict):
            continue
        key = _binding_identity(binding)
        if key not in seen:
            current.append(binding)
            seen.add(key)
    if current:
        target['is_3d_material_texture'] = True


def _enrich_binding(binding: dict, asset: dict) -> dict:
    out = dict(binding)
    out.setdefault('source', asset.get('source', ''))
    out.setdefault('container_offset', int(asset.get('container_offset', 0) or 0))
    out.setdefault('texture_name', asset.get('texture_name', ''))
    return out


def rehydrate_material_bindings(workspace: Path, assets: list[dict]) -> dict:
    """Recover structural 3D material bindings for an existing workspace.

    v0.12 supports both EOU1's actual ATBC -> CGFX/MTOB path and the retained
    BCH/H3D path. This remains best-effort because older manifests may have
    discarded source provenance during deduplication; a fresh ROM extraction is
    authoritative.
    """
    cache: dict[tuple[str, int, str], dict[str, list[dict]]] = {}
    parsed_models = 0
    parse_errors: list[dict] = []
    bindings_added = 0

    search_roots = [
        workspace / '03_hpx_unpacked',
        workspace / '03b_farc_unpacked',
        workspace / '02_romfs_selected',
    ]

    for asset in assets:
        parser = str(asset.get('parser_used', ''))
        if parser not in {'bch_struct', 'cgfx_struct'}:
            continue
        if asset.get('material_bindings'):
            asset['material_bindings'] = [_enrich_binding(b, asset) for b in asset['material_bindings']]
            continue
        source = str(asset.get('source') or '')
        if not source:
            continue
        source_path = Path(source)
        if not source_path.is_file():
            matches = []
            for root in search_roots:
                if root.exists():
                    matches.extend(root.rglob(source_path.name))
            unique = list(dict.fromkeys(str(x.resolve()) for x in matches if x.is_file()))
            if len(unique) == 1:
                source_path = Path(unique[0])
            else:
                continue
        offset = int(asset.get('container_offset', 0) or 0)
        key = (str(source_path.resolve()), offset, parser)
        if key not in cache:
            try:
                data = source_path.read_bytes()
                if offset < 0 or offset >= len(data):
                    raise ValueError(f'container offset 0x{offset:X} outside file')
                if parser == 'cgfx_struct':
                    payload = data[offset:]
                    matches = [x for x in find_cgfx_payloads(data) if x[0] == offset]
                    if matches:
                        payload = data[offset:offset + matches[0][1]]
                    report = parse_cgfx_materials(payload)
                    raw_bindings = cgfx_bindings_by_texture(report)
                else:
                    report = parse_bch_materials(data[offset:])
                    raw_bindings = bch_bindings_by_texture(report)
                cache[key] = {
                    name: [
                        {**b, 'source': str(source_path), 'container_offset': offset, 'texture_name': name}
                        for b in rows
                    ]
                    for name, rows in raw_bindings.items()
                }
                parsed_models += 1
            except Exception as exc:
                cache[key] = {}
                parse_errors.append({
                    'source': str(source_path), 'container_offset': offset,
                    'parser_used': parser, 'error': str(exc),
                })
        name = str(asset.get('texture_name') or '')
        rows = cache[key].get(name, [])
        if rows:
            asset['material_bindings'] = rows
            asset['is_3d_material_texture'] = True
            bindings_added += len(rows)

    return {
        'models_reparsed': parsed_models,
        'bindings_added': bindings_added,
        'parse_errors': parse_errors,
    }

def _slot_asset_map(material_assets: list[tuple[dict, dict]]) -> dict[int, dict]:
    """Map material texture slot -> asset, preferring enabled bindings."""
    slots: dict[int, tuple[bool, dict]] = {}
    for asset, binding in material_assets:
        slot = int(binding.get('slot', -1))
        if slot not in (0, 1, 2):
            continue
        enabled = bool(binding.get('enabled'))
        if slot not in slots or (enabled and not slots[slot][0]):
            slots[slot] = (enabled, asset)
    return {slot: pair[1] for slot, pair in slots.items()}


def _source_plane(
    workspace: Path,
    source: dict,
    slots: dict[int, dict],
    previous: np.ndarray | None,
) -> tuple[np.ndarray | None, str]:
    source_id = int(source.get('source_id', -1))
    operand_id = int(source.get('operand_id', -1))
    if 3 <= source_id <= 5:
        slot = source_id - 3
        asset = slots.get(slot)
        if not asset:
            return None, f'Texture{slot} is not bound to a decoded asset'
        path = workspace / str(asset.get('original', ''))
        if not path.is_file():
            return None, f'Texture{slot} original PNG is missing'
        try:
            return alpha_plane_from_operand(_load_rgba(path), operand_id), ''
        except Exception as exc:
            return None, str(exc)
    if source_id == 15:  # Previous
        if previous is None:
            return None, 'Previous alpha is not available yet'
        # Operand selection applies to a source color in hardware. Previous alpha
        # is already scalar here; Alpha/OneMinusAlpha can be represented exactly.
        if operand_id == 0:
            return previous.copy(), ''
        if operand_id == 1:
            return 255 - previous, ''
        return None, f'Previous.{source.get("operand")} requires RGB state not reconstructed by this exporter'
    return None, f'{source.get("source", "unknown source")} is not a texture/Previous alpha source'


def _resize_like(a: np.ndarray, shape: tuple[int, int]) -> np.ndarray:
    if a.shape[:2] == shape:
        return a
    im = Image.fromarray(a, mode='L').resize((shape[1], shape[0]), Image.Resampling.NEAREST)
    return np.asarray(im, dtype=np.uint8)


def _combine(mode: str, values: list[np.ndarray]) -> np.ndarray | None:
    if not values:
        return None
    shape = values[0].shape[:2]
    vals = [_resize_like(v, shape).astype(np.float32) / 255.0 for v in values]
    if mode == 'Replace' and len(vals) >= 1:
        out = vals[0]
    elif mode == 'Modulate' and len(vals) >= 2:
        out = vals[0] * vals[1]
    elif mode == 'Add' and len(vals) >= 2:
        out = vals[0] + vals[1]
    elif mode == 'AddSigned' and len(vals) >= 2:
        out = vals[0] + vals[1] - 0.5
    elif mode == 'Subtract' and len(vals) >= 2:
        out = vals[0] - vals[1]
    elif mode == 'Interpolate' and len(vals) >= 3:
        out = vals[0] * vals[2] + vals[1] * (1.0 - vals[2])
    elif mode == 'MultAdd' and len(vals) >= 3:
        out = vals[0] * vals[1] + vals[2]
    elif mode == 'AddMult' and len(vals) >= 3:
        out = (vals[0] + vals[1]) * vals[2]
    else:
        # Dot product modes or unknown modes need RGB source state; refusing to
        # fabricate an alpha plane is safer than pretending we evaluated them.
        return None
    return np.clip(np.rint(out * 255.0), 0, 255).astype(np.uint8)


def _resolve_final_alpha(
    workspace: Path,
    stages: list[dict],
    slots: dict[int, dict],
) -> tuple[np.ndarray | None, list[str]]:
    previous: np.ndarray | None = None
    reasons: list[str] = []
    used_texture = False
    for stage in sorted(stages, key=lambda s: int(s.get('stage', 0))):
        values: list[np.ndarray] = []
        stage_ok = True
        for inp in stage.get('inputs', []):
            plane, reason = _source_plane(workspace, inp, slots, previous)
            if plane is None:
                stage_ok = False
                reasons.append(f"stage {stage.get('stage')}: {reason}")
                break
            if 3 <= int(inp.get('source_id', -1)) <= 5:
                used_texture = True
            values.append(plane)
        if not stage_ok:
            return None, reasons
        combined = _combine(str(stage.get('combiner', '')), values)
        if combined is None:
            reasons.append(f"stage {stage.get('stage')}: combiner {stage.get('combiner')} is not scalar-resolvable")
            return None, reasons
        previous = combined
    if not used_texture:
        return None, ['alpha pipeline contains no texture source']
    return previous, reasons


def _checker_preview(rgba: Image.Image, cell: int = 12) -> Image.Image:
    rgba = rgba.convert('RGBA')
    y, x = np.indices((rgba.height, rgba.width))
    board = np.where(((x // cell + y // cell) & 1)[..., None], 165, 220).astype(np.uint8)
    rgb = np.repeat(board, 3, axis=2)
    bg = Image.fromarray(rgb, mode='RGB').convert('RGBA')
    return Image.alpha_composite(bg, rgba).convert('RGB')


def _write_material(
    workspace: Path,
    out_root: Path,
    source: str,
    container_offset: int,
    material_index: int,
    material_name: str,
    material_assets: list[tuple[dict, dict]],
) -> dict:
    group_id = _source_group(source, container_offset)
    representative = material_assets[0][1]
    model_index = int(representative.get('model_index', -1))
    model_name = str(representative.get('model_name') or 'model')
    model_id = f'{model_index:03d}_{_safe(model_name, "model")}' if model_index >= 0 else _safe(model_name, 'model')
    material_id = f'{material_index:03d}_{_safe(material_name, "material")}'
    folder = out_root / group_id / model_id / material_id
    folder.mkdir(parents=True, exist_ok=True)

    slots = _slot_asset_map(material_assets)
    slot_rows: list[dict] = []
    alpha_rows: list[dict] = []
    constant_alpha_rows: list[dict] = []
    seen_alpha: set[tuple[int, int, int, int]] = set()

    # Use any binding for common material-level state. All slot bindings from the
    # same material carry the same stages/alpha-test metadata.
    stages = representative.get('alpha_stages', []) or []
    alpha_test = representative.get('alpha_test')

    for slot in sorted(slots):
        asset = slots[slot]
        original = workspace / str(asset.get('original', ''))
        copy_rel = ''
        if original.is_file():
            copy_path = folder / f'texture{slot}_{asset["asset_id"]}_{_safe(asset.get("texture_name", "texture"))}.png'
            shutil.copy2(original, copy_path)
            copy_rel = str(copy_path.relative_to(workspace)).replace('\\', '/')
        slot_rows.append({
            'slot': slot,
            'asset_id': asset.get('asset_id'),
            'texture_name': asset.get('texture_name'),
            'width': asset.get('width'),
            'height': asset.get('height'),
            'format': asset.get('format'),
            'format_name': asset.get('format_name'),
            'candidate_hash': asset.get('candidate_hash'),
            'original': asset.get('original'),
            'master': asset.get('master'),
            'material_copy': copy_rel,
        })

    # Export only channels explicitly named by the material alpha pipeline.
    for asset, binding in material_assets:
        slot = int(binding.get('slot', -1))
        if slot not in slots or slots[slot].get('asset_id') != asset.get('asset_id'):
            continue
        original = workspace / str(asset.get('original', ''))
        if not original.is_file():
            continue
        rgba = _load_rgba(original)
        for use in binding.get('alpha_uses', []) or []:
            key = (int(use.get('stage', -1)), int(use.get('input', -1)), slot, int(use.get('operand_id', -1)))
            if key in seen_alpha:
                continue
            seen_alpha.add(key)
            # Texture.Alpha on an RGB/L/ETC1 format is a hardware constant,
            # not stored alpha data. Record the exact constant but do not create
            # another misleading solid-white/black PNG. RGB channel operands
            # remain real extractable mask data regardless of texture format.
            if key[3] in (0, 1) and not _format_stores_alpha(int(asset.get('format', -1))):
                constant_alpha_rows.append({
                    'stage': key[0], 'input': key[1], 'slot': slot,
                    'asset_id': asset.get('asset_id'),
                    'texture_name': asset.get('texture_name'),
                    'source': use.get('source'), 'operand': use.get('operand'),
                    'combiner': use.get('combiner'),
                    'constant_value': 255 if key[3] == 0 else 0,
                    'reason': 'texture format stores no alpha component; PICA sampling returns constant alpha',
                })
                continue
            try:
                plane = alpha_plane_from_operand(rgba, key[3])
            except Exception:
                continue
            filename = (
                f'alpha_stage{key[0]}_input{key[1]}_texture{slot}_'
                f'{_safe(str(use.get("operand", "channel"))).lower()}.png'
            )
            target = folder / filename
            Image.fromarray(plane, mode='L').save(target)
            alpha_rows.append({
                'stage': key[0],
                'input': key[1],
                'slot': slot,
                'asset_id': asset.get('asset_id'),
                'texture_name': asset.get('texture_name'),
                'source': use.get('source'),
                'operand': use.get('operand'),
                'combiner': use.get('combiner'),
                'alpha_plane': str(target.relative_to(workspace)).replace('\\', '/'),
            })

    final_alpha, unresolved = _resolve_final_alpha(workspace, stages, slots)
    resolved_alpha_rel = ''
    preview_rel = ''
    checker_rel = ''
    if final_alpha is not None:
        resolved_path = folder / 'resolved_material_alpha.png'
        Image.fromarray(final_alpha, mode='L').save(resolved_path)
        resolved_alpha_rel = str(resolved_path.relative_to(workspace)).replace('\\', '/')
        # Texture0 is conventionally the diffuse/base texture. This preview is
        # diagnostic only and never replaces either runtime texture in Azahar.
        color = slots.get(0)
        if color:
            color_path = workspace / str(color.get('original', ''))
            if color_path.is_file():
                with Image.open(color_path) as im:
                    rgba_im = im.convert('RGBA')
                alpha_im = Image.fromarray(final_alpha, mode='L')
                if alpha_im.size != rgba_im.size:
                    alpha_im = alpha_im.resize(rgba_im.size, Image.Resampling.NEAREST)
                rgba_im.putalpha(alpha_im)
                preview = folder / 'rgba_material_preview.png'
                checker = folder / 'checker_material_preview.jpg'
                rgba_im.save(preview)
                _checker_preview(rgba_im).save(checker, quality=92)
                preview_rel = str(preview.relative_to(workspace)).replace('\\', '/')
                checker_rel = str(checker.relative_to(workspace)).replace('\\', '/')

    payload = {
        'source': source,
        'container_offset': container_offset,
        'model_index': model_index,
        'model_name': model_name,
        'material_index': material_index,
        'material_name': material_name,
        'texture_slots': slot_rows,
        'alpha_texture_channels': alpha_rows,
        'constant_texture_alpha_inputs': constant_alpha_rows,
        'alpha_stages': stages,
        'alpha_test': alpha_test,
        'resolved_material_alpha': resolved_alpha_rel,
        'rgba_preview': preview_rel,
        'checker_preview': checker_rel,
        'alpha_resolution_status': 'resolved' if final_alpha is not None else ('unresolved' if (alpha_rows or constant_alpha_rows) else 'no_texture_alpha'),
        'unresolved_reasons': unresolved,
        'note': 'Alpha channels are exported only from explicit PICA200 material alpha-combiner references. No grayscale/image-content heuristic is used.',
    }
    (folder / 'material.json').write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding='utf-8')
    return payload


def build_3d_material_workspace(workspace: Path, assets: list[dict]) -> dict:
    workspace = Path(workspace)
    out_root = workspace / '.eouhd' / 'work' / '10_3d_materials'
    if out_root.exists():
        shutil.rmtree(out_root)
    out_root.mkdir(parents=True, exist_ok=True)

    # Remove misleading v0.4 post-decode heuristic metadata from the active
    # manifest. The original RGBA PNG itself still preserves genuine embedded
    # alpha; we simply stop fabricating a sidecar for every texture.
    stale_keys = (
        'alpha_analysis', 'alpha_source', 'alpha_master', 'alpha_sidecar_kind',
        'material_pairs',
    )
    for asset in assets:
        for key in stale_keys:
            asset.pop(key, None)

    materials: dict[tuple[str, int, int, str, int, str], list[tuple[dict, dict]]] = {}
    for asset in assets:
        bindings = asset.get('material_bindings', []) or []
        if not bindings:
            asset['is_3d_material_texture'] = False
            continue
        asset['is_3d_material_texture'] = True
        normalized = []
        for raw_binding in bindings:
            if not isinstance(raw_binding, dict):
                continue
            binding = _enrich_binding(raw_binding, asset)
            normalized.append(binding)
            source = str(binding.get('source') or asset.get('source') or '')
            offset = int(binding.get('container_offset', asset.get('container_offset', 0)) or 0)
            model_index = int(binding.get('model_index', -1))
            model_name = str(binding.get('model_name') or 'model')
            index = int(binding.get('material_index', -1))
            name = str(binding.get('material_name') or f'material_{index:03d}')
            materials.setdefault((source, offset, model_index, model_name, index, name), []).append((asset, binding))
        asset['material_bindings'] = normalized

    material_payloads = []
    for (source, offset, _model_index, _model_name, index, name), rows in sorted(
        materials.items(), key=lambda x: (x[0][0], x[0][1], x[0][2], x[0][3], x[0][4], x[0][5])
    ):
        material_payloads.append(_write_material(workspace, out_root, source, offset, index, name, rows))

    report_dir = workspace / '.eouhd' / 'reports'
    report_dir.mkdir(parents=True, exist_ok=True)
    alpha_channels = sum(len(m['alpha_texture_channels']) for m in material_payloads)
    constant_alpha_inputs = sum(len(m.get('constant_texture_alpha_inputs', [])) for m in material_payloads)
    material_textures = sum(len(m['texture_slots']) for m in material_payloads)
    resolved = sum(m['alpha_resolution_status'] == 'resolved' for m in material_payloads)
    unresolved = sum(m['alpha_resolution_status'] == 'unresolved' for m in material_payloads)
    no_texture_alpha = sum(m['alpha_resolution_status'] == 'no_texture_alpha' for m in material_payloads)
    report = {
        'version': '0.12.0',
        'materials_found': len(material_payloads),
        'material_texture_bindings': material_textures,
        'assets_referenced_by_3d_materials': sum(bool(a.get('material_bindings')) for a in assets),
        'explicit_texture_alpha_channels': alpha_channels,
        'constant_texture_alpha_inputs': constant_alpha_inputs,
        'resolved_material_alphas': resolved,
        'unresolved_material_alphas': unresolved,
        'materials_without_texture_alpha': no_texture_alpha,
        'heuristic_grayscale_masks_generated': 0,
        'materials': material_payloads,
    }
    (report_dir / '3d_material_report.json').write_text(
        json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8'
    )

    fields = [
        'source', 'container_offset', 'model_index', 'model_name', 'material_index', 'material_name',
        'alpha_resolution_status', 'texture_slot_count', 'alpha_channel_count',
        'resolved_material_alpha', 'rgba_preview',
    ]
    with (report_dir / '3d_materials.csv').open('w', newline='', encoding='utf-8-sig') as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for m in material_payloads:
            writer.writerow({
                'source': m['source'],
                'container_offset': m['container_offset'],
                'model_index': m.get('model_index'),
                'model_name': m.get('model_name'),
                'material_index': m['material_index'],
                'material_name': m['material_name'],
                'alpha_resolution_status': m['alpha_resolution_status'],
                'texture_slot_count': len(m['texture_slots']),
                'alpha_channel_count': len(m['alpha_texture_channels']),
                'resolved_material_alpha': m['resolved_material_alpha'],
                'rgba_preview': m['rgba_preview'],
            })

    with (report_dir / 'unresolved_3d_materials.csv').open('w', newline='', encoding='utf-8-sig') as f:
        fields2 = ['source', 'container_offset', 'model_index', 'model_name', 'material_index', 'material_name', 'reasons']
        writer = csv.DictWriter(f, fieldnames=fields2)
        writer.writeheader()
        for m in material_payloads:
            if m['alpha_resolution_status'] != 'unresolved':
                continue
            writer.writerow({
                'source': m['source'], 'container_offset': m['container_offset'],
                'model_index': m.get('model_index'), 'model_name': m.get('model_name'),
                'material_index': m['material_index'], 'material_name': m['material_name'],
                'reasons': ' | '.join(m.get('unresolved_reasons') or []),
            })
    return report


def rebuild_3d_material_workspace(workspace: Path) -> dict:
    workspace = Path(workspace)
    manifest_path = workspace / '.eouhd' / 'manifest.json'
    if not manifest_path.is_file():
        manifest_path = workspace / 'manifest.json'
    manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    assets = manifest.get('assets', [])
    rehydrated = rehydrate_material_bindings(workspace, assets)
    report = build_3d_material_workspace(workspace, assets)
    report['rehydration'] = rehydrated
    manifest['assets'] = assets
    manifest['extractor_version'] = '0.12.0'
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8')
    (workspace / '.eouhd' / 'reports' / '3d_material_report.json').write_text(
        json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8'
    )
    return report
