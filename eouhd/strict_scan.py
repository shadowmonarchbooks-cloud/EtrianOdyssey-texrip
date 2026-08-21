from __future__ import annotations

"""Strict texture/model discovery for supported Etrian Odyssey 3DS titles.

The goal is to prefer deterministic container metadata over binary heuristics.
Heuristic discoveries can be inspected in quarantine, but they are not allowed into
the temporary decode workspace automatically; 0.12 retains only the deduplicated Azahar master/deployment packs after success.
"""

from dataclasses import dataclass, asdict
from pathlib import Path
import csv
import hashlib
import json
from collections import Counter
from typing import Callable, Iterable

from .eou_stex import is_stex, parse_eou_stex, format_name
from .forge_bridge import forge_import_path
from .bch_materials import (
    parse_bch_materials, bindings_by_texture as bch_bindings_by_texture, extract_bch_texture_infos, parse_bch_header, BCHMaterialError,
)
from .cgfx_materials import (
    find_cgfx_payloads, atbc_info, extract_cgfx_texture_infos,
    parse_cgfx_materials, bindings_by_texture as cgfx_bindings_by_texture,
)


KNOWN_DIRECT_MAGICS = (b'BCH\x00', b'CGFX', b'ATBC', b'CTPK', b'ctxb', b'CTXB', b'cmb ')
MODEL_EXTS = {'.bam', '.bam2', '.bch', '.bcres', '.bcmdl', '.cmb', '.model', '.bin'}
TEXTURE_EXTS = {'.stex', '.ctpk', '.ctxb', '.bch', '.bcres', '.bcmdl', '.cmb'}
SKIP_EXTS = {
    '.bcstm', '.bcwav', '.bcsnd', '.bcsar', '.wav', '.ogg', '.mp3', '.aac',
    '.moflex', '.mp4', '.mbm', '.tbl', '.bf', '.sav', '.txt', '.xml', '.lua',
}


@dataclass
class ScanIssue:
    source: str
    kind: str
    message: str


def _sha1(data: bytes) -> str:
    return hashlib.sha1(data).hexdigest()


def _looks_like_bch_at(data: bytes, off: int) -> bool:
    """Validate a BCH payload using the version-aware BCH header parser.

    EO2U BAM2 files can contain model-only BCH payloads whose texture pixels live
    in separate STEX files.  Those BCH files may legitimately have no local raw
    texture-data section, so the old `data_addr > 0` requirement incorrectly
    rejected them before material parsing ever ran.
    """
    if off < 0 or off + 0x38 > len(data) or data[off:off+4] != b'BCH\x00':
        return False
    try:
        parse_bch_header(data[off:])
        return True
    except Exception:
        return False


def _embedded_bch_offsets(data: bytes, limit: int = 8) -> list[int]:
    """Find genuine BCH payloads for the retained H3D fallback path.

    EOU1's supplied enemy-model sample is ATBC/CGFX, not BCH. Some related 3DS
    assets/titles may still wrap BCH, so this fallback searches for validated BCH
    magic without assuming a particular outer wrapper length.
    """
    offsets = []
    start = 0
    while len(offsets) < limit:
        idx = data.find(b'BCH\x00', start)
        if idx < 0:
            break
        if _looks_like_bch_at(data, idx):
            offsets.append(idx)
        start = idx + 4
    return offsets


def has_strict_texture_signature(data: bytes, suffix: str = '') -> bool:
    suffix = suffix.lower()
    if is_stex(data):
        return True
    if data[:4] in KNOWN_DIRECT_MAGICS:
        if data.startswith(b'ATBC'):
            # EOU1: ATBC -> CGFX.  EO2U: ATBC/BAM2 -> BCH/H3D.  Do not
            # short-circuit the second family merely because no CGFX exists.
            return bool(find_cgfx_payloads(data, allow_truncated=True)) or b'BCH\x00' in data
        return True
    if suffix in MODEL_EXTS | TEXTURE_EXTS and b'BCH\x00' in data[:0x20000]:
        return True
    # Wrapped BCH files are not guaranteed to carry a useful extension.
    if b'BCH\x00' in data[:0x10000]:
        return True
    return False


def _decode_eou_stex(data: bytes, forge_root: Path) -> list[dict]:
    st = parse_eou_stex(data)
    with forge_import_path(forge_root):
        decoder = __import__('textures.decoder', fromlist=['decode_texture_fast'])
        rgba = decoder.decode_texture_fast(st.raw, st.width, st.height, st.pica_format)
    if rgba is None:
        raise RuntimeError('Texture Forge decoder returned no pixels')
    return [{
        'width': st.width, 'height': st.height,
        'format': st.pica_format, 'format_name': format_name(st.pica_format),
        'raw': st.raw, 'rgba': rgba,
        'parser_used': 'eou_stex_strict', 'confidence': 'exact',
        'name': st.name or 'stex_texture',
        'data_type': st.data_type, 'pixel_format': st.pixel_format,
        'data_offset': st.data_offset, 'declared_size': st.data_size_declared,
        'trailing_bytes': st.trailing_bytes,
    }]


def _decode_bch_payload(payload: bytes, forge_root: Path, label: str) -> tuple[list[dict], dict]:
    """Decode structurally declared BCH textures and attach real H3D material bindings.

    The BCH fallback deliberately separates two jobs:
      * Our local BCH descriptor parser locates encoded 3D texture payloads.
      * Our BCH material parser reads Model -> Material -> H3DMaterialParams and
        the PICA material command streams, while Texture Forge is used only for
        pixel-format decoding.
        tells us which texture slot/channel is actually consumed by alpha.

    No visual/grayscale guessing is used to decide whether a 3D texture is an
    alpha source.
    """
    material_report: dict = {}
    material_bindings: dict[str, list[dict]] = {}
    material_error = ''
    try:
        material_report = parse_bch_materials(payload)
        material_bindings = bch_bindings_by_texture(material_report)
    except Exception as exc:
        # Texture extraction should still proceed even if a previously unseen BCH
        # material revision cannot yet be described. The failure is carried into
        # the texture records/reports instead of silently inventing mask metadata.
        material_error = str(exc)

    # Descriptor discovery is local so it uses the same corrected
    # PICA command semantics as the material parser. Texture Forge is retained
    # only for the mature pixel-format decoder.
    infos = extract_bch_texture_infos(payload)
    with forge_import_path(forge_root):
        decoder = __import__('textures.decoder', fromlist=['decode_texture_fast', 'get_format_name'])
        out = []
        for texture_index, t in enumerate(infos):
            w = int(t.get('width', 0)); h = int(t.get('height', 0)); fmt = int(t.get('format', -1))
            off = int(t.get('data_offset', -1)); size = int(t.get('data_size', 0))
            if w <= 0 or h <= 0 or off < 0 or size <= 0 or off + size > len(payload):
                continue
            raw = payload[off:off + size]
            rgba = decoder.decode_texture_fast(raw, w, h, fmt)
            if rgba is None:
                continue
            name = t.get('name', '') or f'{label}_bch_{len(out):03d}'
            record = {
                'width': w, 'height': h, 'format': fmt,
                'format_name': decoder.get_format_name(fmt),
                'raw': raw, 'rgba': rgba,
                'parser_used': 'bch_struct', 'confidence': 'structural',
                'name': name,
                'data_offset': off,
                'texture_index': texture_index,
                'material_bindings': material_bindings.get(str(name), []),
                'is_3d_material_texture': bool(material_bindings.get(str(name))),
                'bch_material_count': len(material_report.get('materials', [])) if material_report else 0,
            }
            if material_report.get('header'):
                record['bch_backward_compat'] = material_report['header'].get('backward_compat')
                record['bch_version'] = material_report['header'].get('version')
            if material_error:
                record['bch_material_parse_error'] = material_error
            out.append(record)

        decoded_names = {str(item.get('name') or '') for item in out if str(item.get('name') or '')}
        referenced_names = set(material_bindings)
        missing_names = sorted(referenced_names - decoded_names)
        diagnostic = {
            'label': label,
            'model_count': int(material_report.get('model_count', 0)) if material_report else 0,
            'materials_found': len(material_report.get('materials', [])) if material_report else 0,
            'material_params_count': int(material_report.get('material_params_count', 0)) if material_report else 0,
            'texture_descriptors_found': len(infos),
            'decoded_textures': len(out),
            'material_texture_references': len(referenced_names),
            'missing_material_texture_names': missing_names,
            # Retain the exact parsed bindings so the pipeline can resolve a
            # material name to a separate STEX/external texture after every
            # archive has been decoded. This is metadata only, never a guess.
            'material_bindings_by_texture': material_bindings,
            'material_parse_error': material_error,
        }
        return out, diagnostic



def _decode_cgfx_payload(payload: bytes, forge_root: Path, label: str) -> tuple[list[dict], dict]:
    """Decode CGFX ImageTexture TXOBs and attach MTOB material bindings."""
    report = parse_cgfx_materials(payload)
    bindings = cgfx_bindings_by_texture(report)
    infos = extract_cgfx_texture_infos(payload)
    decode_errors: list[dict] = []
    with forge_import_path(forge_root):
        decoder = __import__('textures.decoder', fromlist=['decode_texture_fast', 'get_format_name'])
        out = []
        for texture_index, t in enumerate(infos):
            w = int(t.get('width', 0)); h = int(t.get('height', 0)); fmt = int(t.get('format', -1))
            off = int(t.get('data_offset', -1)); size = int(t.get('data_size', 0))
            name = str(t.get('name') or f'{label}_cgfx_{len(out):03d}')
            if w <= 0 or h <= 0 or off < 0 or size <= 0 or off + size > len(payload):
                decode_errors.append({'texture_name': name, 'texture_index': texture_index, 'error': 'invalid encoded texture bounds'})
                continue
            raw = payload[off:off + size]
            try:
                rgba = decoder.decode_texture_fast(raw, w, h, fmt)
            except Exception as exc:
                decode_errors.append({'texture_name': name, 'texture_index': texture_index, 'error': str(exc)})
                continue
            if rgba is None:
                decode_errors.append({'texture_name': name, 'texture_index': texture_index, 'error': 'pixel decoder returned no image'})
                continue
            out.append({
                'width': w, 'height': h, 'format': fmt,
                'format_name': decoder.get_format_name(fmt),
                'raw': raw, 'rgba': rgba,
                'parser_used': 'cgfx_struct', 'confidence': 'structural',
                'name': name, 'data_offset': off, 'texture_index': texture_index,
                'material_bindings': bindings.get(name, []),
                'is_3d_material_texture': bool(bindings.get(name)),
                'cgfx_material_count': len(report.get('materials', [])),
            })
    decoded_names = {str(x.get('name') or '') for x in out}
    referenced = set(bindings)
    return out, {
        'format': 'cgfx',
        'label': label,
        'model_count': int(report.get('model_count', 0)),
        'materials_found': len(report.get('materials', [])),
        'texture_descriptors_found': len(infos),
        'decoded_textures': len(out),
        'material_texture_references': len(referenced),
        'missing_material_texture_names': sorted(referenced - decoded_names),
        'material_bindings_by_texture': bindings,
        'materials': report.get('materials', []),
        'mtob_candidates': int(report.get('mtob_candidates', 0)),
        'material_parse_errors': report.get('material_parse_errors', []),
        'texture_decode_errors': decode_errors,
    }

def _decode_generic_strict(data: bytes, source: Path, forge_root: Path, title_id: str) -> list[dict]:
    """Use Texture Forge only when the file itself has a recognized container magic.

    No scan_all fallback is enabled here.
    """
    with forge_import_path(forge_root):
        scanner = __import__('textures.scanner', fromlist=['extract_textures_with_confidence'])
        decoder = __import__('textures.decoder', fromlist=['decode_texture_fast', 'get_format_name'])
        texs, fp = scanner.extract_textures_with_confidence(
            data, str(source), scan_all=False, title_id=title_id
        )
        out = []
        for t in texs:
            parser = str(t.get('parser_used', fp.detected_type or 'unknown'))
            # BCH is handled structurally above, STEX by our exact Atlus parser.
            if parser.lower() in {'bch', 'stex'}:
                continue
            raw = t.get('data', b'')
            w = int(t.get('width', 0)); h = int(t.get('height', 0)); fmt = int(t.get('format', -1))
            if not raw or w <= 0 or h <= 0 or fmt < 0:
                continue
            rgba = decoder.decode_texture_fast(raw, w, h, fmt)
            if rgba is None:
                continue
            out.append({
                'width': w, 'height': h, 'format': fmt,
                'format_name': decoder.get_format_name(fmt),
                'raw': raw, 'rgba': rgba,
                'parser_used': f'forge_strict:{parser}', 'confidence': t.get('confidence', 'structural'),
                'name': t.get('name', '') or f'{source.stem}_{len(out):03d}',
            })
        return out


def decode_strict_file(path: Path, forge_root: Path, title_id: str = '') -> tuple[list[dict], list[ScanIssue], list[dict]]:
    issues: list[ScanIssue] = []
    model_diagnostics: list[dict] = []
    try:
        with path.open('rb') as f:
            probe = f.read(0x100000)
        if len(probe) < 4:
            return [], issues, model_diagnostics
        strict_sig = has_strict_texture_signature(probe, path.suffix)
        if path.suffix.lower() in SKIP_EXTS and not strict_sig:
            return [], issues, model_diagnostics
        if not strict_sig:
            return [], issues, model_diagnostics
        data = path.read_bytes()
    except Exception as e:
        return [], [ScanIssue(str(path), 'read_error', str(e))], model_diagnostics

    if is_stex(data):
        try:
            return _decode_eou_stex(data, forge_root), issues, model_diagnostics
        except Exception as e:
            issues.append(ScanIssue(str(path), 'stex_decode_error', str(e)))
            return [], issues, model_diagnostics

    out: list[dict] = []

    # EOU1 BAM files are ATBC wrappers around a complete CGFX payload; direct
    # .bcmdl/.bcres files are the same CGFX family. Parse these before BCH.
    for n, (off, size) in enumerate(find_cgfx_payloads(data)):
        try:
            decoded, diagnostic = _decode_cgfx_payload(data[off:off + size], forge_root, f'{path.stem}_{n}')
            diagnostic = {**diagnostic, 'source': str(path), 'container_offset': off, 'container_size': size}
            if data.startswith(b'ATBC'):
                diagnostic['atbc'] = atbc_info(data)
            model_diagnostics.append(diagnostic)
            parse_errors = diagnostic.get('material_parse_errors') or []
            if parse_errors:
                issues.append(ScanIssue(str(path), 'cgfx_material_parse_error',
                    f'offset 0x{off:X}: {len(parse_errors)} MTOB material record(s) could not be parsed'))
            missing = diagnostic.get('missing_material_texture_names') or []
            if missing:
                issues.append(ScanIssue(str(path), 'cgfx_material_texture_missing',
                    f'offset 0x{off:X}: {len(missing)} MTOB texture reference(s) were not decoded: ' + ', '.join(map(str, missing[:20]))))
            for t in decoded:
                t['container_offset'] = off
                t['cgfx_model_count'] = diagnostic.get('model_count', 0)
                t['cgfx_missing_material_texture_names'] = missing
            out.extend(decoded)
        except Exception as e:
            issues.append(ScanIssue(str(path), 'cgfx_decode_error', f'offset 0x{off:X}: {e}'))

    # Retain BCH support for games/assets that genuinely use H3D BCH.
    for n, off in enumerate(_embedded_bch_offsets(data)):
        try:
            decoded, diagnostic = _decode_bch_payload(data[off:], forge_root, f'{path.stem}_{n}')
            diagnostic = {**diagnostic, 'format': 'bch', 'source': str(path), 'container_offset': off}
            model_diagnostics.append(diagnostic)
            material_error = str(diagnostic.get('material_parse_error') or '')
            if material_error:
                issues.append(ScanIssue(str(path), 'bch_material_parse_error', f'offset 0x{off:X}: {material_error}'))
            missing = diagnostic.get('missing_material_texture_names') or []
            if missing:
                issues.append(ScanIssue(str(path), 'bch_material_texture_missing',
                    f'offset 0x{off:X}: {len(missing)} material-referenced texture(s) were not decoded: ' + ', '.join(map(str, missing[:20]))))
            for t in decoded:
                t['container_offset'] = off
                t['bch_model_count'] = diagnostic.get('model_count', 0)
                t['bch_missing_material_texture_names'] = missing
            out.extend(decoded)
        except Exception as e:
            issues.append(ScanIssue(str(path), 'bch_decode_error', f'offset 0x{off:X}: {e}'))

    # Generic strict containers only when neither structural 3D path produced output.
    if not out and data[:4] in KNOWN_DIRECT_MAGICS and not data.startswith((b'BCH\x00', b'CGFX', b'ATBC')):
        try:
            out.extend(_decode_generic_strict(data, path, forge_root, title_id))
        except Exception as e:
            issues.append(ScanIssue(str(path), 'container_decode_error', str(e)))

    return out, issues, model_diagnostics


def inventory_files(roots: Iterable[Path], report_dir: Path) -> dict:
    report_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    ext_counts = Counter(); magic_counts = Counter(); bch_wrapped = 0; bam_bch = 0; stex_count = 0; farc_count = 0; atbc_count = 0; cgfx_count = 0; epl_count = 0; ep_count = 0; ctpk_count = 0; embedded_stex_count = 0
    seen = set()
    for root in roots:
        if not root.exists():
            continue
        for p in root.rglob('*'):
            if not p.is_file():
                continue
            rp = str(p.resolve())
            if rp in seen:
                continue
            seen.add(rp)
            ext = p.suffix.lower() or '(none)'; ext_counts[ext] += 1
            try:
                with p.open('rb') as f:
                    head = f.read(0x20000)
            except Exception:
                continue
            magic = head[:4]
            magic_label = ''.join(chr(x) if 32 <= x < 127 else '.' for x in magic)
            magic_counts[magic_label] += 1
            has_bch = b'BCH\x00' in head
            is_s = head.startswith(b'STEX')
            is_farc = head.startswith(b'FARC')
            is_atbc = head.startswith(b'ATBC')
            is_ctpk = head.startswith(b'CTPK')
            has_embedded_stex = (not is_s) and (b'STEX' in head)
            has_cgfx = bool(find_cgfx_payloads(head, allow_truncated=True))
            if has_bch and not head.startswith(b'BCH\x00'): bch_wrapped += 1
            if has_bch and ext in {'.bam', '.bam2'}: bam_bch += 1
            if is_s: stex_count += 1
            if is_farc: farc_count += 1
            if is_atbc: atbc_count += 1
            if is_ctpk or ext == '.ctpk': ctpk_count += 1
            if ext == '.epl': epl_count += 1
            if ext == '.ep': ep_count += 1
            if has_embedded_stex: embedded_stex_count += 1
            if has_cgfx: cgfx_count += 1
            rows.append({
                'path': str(p), 'extension': ext, 'magic_ascii': magic_label,
                'size': p.stat().st_size, 'stex': int(is_s), 'farc': int(is_farc), 'atbc': int(is_atbc), 'cgfx': int(has_cgfx),
                'ctpk': int(is_ctpk), 'embedded_stex': int(has_embedded_stex),
                'embedded_bch': int(has_bch), 'strict_candidate': int(has_strict_texture_signature(head, p.suffix)),
            })
    csv_path = report_dir/'file_inventory.csv'
    with csv_path.open('w', newline='', encoding='utf-8-sig') as f:
        w = csv.DictWriter(f, fieldnames=['path','extension','magic_ascii','size','stex','farc','atbc','cgfx','ctpk','embedded_stex','embedded_bch','strict_candidate'])
        w.writeheader(); w.writerows(rows)
    summary = {
        'files': len(rows), 'stex_files': stex_count, 'farc_files': farc_count, 'atbc_files': atbc_count, 'cgfx_files': cgfx_count,
        'epl_files': epl_count, 'ep_files': ep_count, 'ctpk_files': ctpk_count, 'embedded_stex_files': embedded_stex_count,
        'wrapped_bch_files': bch_wrapped, 'bam_bch_files': bam_bch,
        'extensions': dict(ext_counts.most_common()), 'magics': dict(magic_counts.most_common(50)),
    }
    (report_dir/'file_inventory_summary.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    return summary


def write_scan_report(report_dir: Path, issues: list[ScanIssue], stats: dict) -> None:
    report_dir.mkdir(parents=True, exist_ok=True)
    out = dict(stats)
    out['issues'] = [asdict(x) for x in issues]
    (report_dir/'strict_scan.json').write_text(json.dumps(out, indent=2, ensure_ascii=False), encoding='utf-8')
