from __future__ import annotations

from dataclasses import dataclass, asdict
from pathlib import Path
import csv
import hashlib
import json
import re
import shutil
from typing import Iterable

from PIL import Image, ImageOps, ImageDraw, ImageFont
from .cityhash64 import cityhash64_hex
from .materials import merge_material_bindings

METADATA_DIR = '.eouhd'
MASTER_PACK_DIR = 'azahar_pack_master'
DEPLOY_PACK_DIR = 'azahar_pack'

AZAHAR_RE = re.compile(r'^tex1_(\d+)x(\d+)_([0-9A-Fa-f]{8,16})_(\d+)(?:_mip(\d+))?\.png$', re.I)

CATEGORY_RULES = [
    ('characters', ('face','portrait','chara','character','npc','pc_','event/ch','bust')),
    ('monsters', ('enemy','monster','ene','foe','boss')),
    ('ui', ('ui','menu','window','frame','cursor','button','layout')),
    ('icons', ('icon','item','skill','equip','status')),
    ('maps', ('map','floor','atlas','compass')),
    ('dungeon', ('dungeon','mori','labyrinth','field','wall','ground','floor','bg3d')),
    ('backgrounds', ('background','back','bg/','eventbg','town','shop')),
    ('effects', ('effect','eff','particle','magic')),
    ('fonts', ('font','glyph','letter')),
]


def category_for(source: str) -> str:
    s = source.replace('\\','/').lower()
    for cat, keys in CATEGORY_RULES:
        if any(k in s for k in keys):
            return cat
    return 'misc'


def parse_azahar_filename(name: str) -> dict | None:
    m = AZAHAR_RE.match(Path(name).name)
    if not m:
        return None
    return {
        'width': int(m.group(1)), 'height': int(m.group(2)),
        'hash': m.group(3).upper().zfill(16), 'format': int(m.group(4)),
        'mip': int(m.group(5) or 0),
    }


def rgba_hashes(path: Path) -> tuple[str, str]:
    with Image.open(path) as im:
        rgba = im.convert('RGBA')
        direct = hashlib.sha256(rgba.tobytes()).hexdigest()
        flipped = hashlib.sha256(ImageOps.flip(rgba).tobytes()).hexdigest()
    return direct, flipped


def target_scale(category: str, width: int, height: int) -> int:
    # Steam Deck-focused defaults. Keep tiny high-frequency effects lighter while
    # giving portraits/UI enough source resolution for clean downsampling.
    if category in {'characters','backgrounds'}:
        return 4
    if category in {'monsters','ui','icons','maps'}:
        return 4 if max(width, height) <= 512 else 2
    if category in {'dungeon','effects','fonts'}:
        return 2 if max(width, height) >= 256 else 4
    return 2 if max(width, height) >= 512 else 4


def ensure_workspace(root: Path) -> dict[str, Path]:
    """Create the 0.12 streamlined workspace layout.

    Extraction/decode data lives under .eouhd/work and is temporary.  The only
    large persistent trees are azahar_pack_master (the user's editable source of
    truth) and azahar_pack (the deployment copy).
    """
    root = Path(root)
    meta = root / METADATA_DIR
    work = meta / 'work'
    dirs = {
        'root': root,
        'meta': meta,
        'work': work,
        'forge': work / '01_forge_static',
        'romfs': work / '02_romfs_selected',
        'hpx': work / '03_hpx_unpacked',
        'farc': work / '03b_farc_unpacked',
        'epl': work / '03c_epl_unpacked',
        'originals': work / '04_originals',
        'masters': work / '05_hd_masters',
        'sheets': meta / 'contact_sheets',
        'pack_master': root / MASTER_PACK_DIR,
        'pack': root / DEPLOY_PACK_DIR,
        'reports': meta / 'reports',
        'diagnostics': meta / 'diagnostics',
        'quarantine': work / '09_quarantine',
        'materials': work / '10_3d_materials',
    }
    for key, path in dirs.items():
        if key in {'pack_master', 'pack'}:
            continue
        path.mkdir(parents=True, exist_ok=True)
    return dirs


def _manifest_candidates(root: Path) -> list[Path]:
    root = Path(root)
    return [root / METADATA_DIR / 'manifest.json', root / 'manifest.json']


def manifest_path(root: Path) -> Path:
    return Path(root) / METADATA_DIR / 'manifest.json'


def existing_manifest_path(root: Path) -> Path:
    for candidate in _manifest_candidates(root):
        if candidate.is_file():
            return candidate
    return manifest_path(root)


def _image_rgba_sha256(path: Path) -> tuple[str, tuple[int, int]]:
    with Image.open(path) as im:
        rgba = im.convert('RGBA')
        return hashlib.sha256(rgba.tobytes()).hexdigest(), rgba.size


def _asset_hashes(asset: dict, use_candidates: bool = True) -> list[str]:
    hashes = list(asset.get('verified_hashes') or [])
    if not hashes and use_candidates and asset.get('candidate_hash'):
        hashes = [asset['candidate_hash']]
    return [str(h).upper().zfill(16) for h in dict.fromkeys(hashes)]


def canonical_pack_filename(asset: dict, raw_hash: str) -> str:
    """Legacy/canonical Azahar filename used for migration and dump import."""
    return (
        f"tex1_{int(asset['width'])}x{int(asset['height'])}_"
        f"{str(raw_hash).upper().zfill(16)}_{int(asset['format'])}_mip{int(asset.get('mip', 0) or 0)}.png"
    )


def _friendly_slug(value: object, fallback: str = 'texture') -> str:
    """Return a Windows-safe, readable lowercase filename stem."""
    text = Path(str(value or '')).stem.strip()
    text = text.replace('\\', '-').replace('/', '-')
    text = re.sub(r'[_\s]+', '-', text)
    text = re.sub(r'[^A-Za-z0-9-]+', '-', text)
    text = re.sub(r'-{2,}', '-', text).strip(' .-_').lower()
    return text or fallback


def _binding_model_name(asset: dict) -> str:
    for binding in asset.get('material_bindings', []) or []:
        name = str(binding.get('model_name') or '').strip()
        if name:
            return name
    return ''


def _alpha_role_from_bindings(asset: dict) -> bool:
    """Conservatively identify a separate alpha/mask texture.

    Texture0.Alpha normally means the main RGBA texture simply contains alpha and
    must NOT become a separate `-alpha` file.  We only infer an auxiliary alpha
    texture when a non-primary slot/channel is explicitly consumed by the alpha
    combiner, or when the ROM itself calls the texture alpha/mask/opacity.
    """
    tex_name = str(asset.get('texture_name') or '').lower()
    if re.search(r'(^|[_\-.])(alpha|mask|opacity|trans|transparency)([_\-.]|$)', tex_name):
        return True
    for binding in asset.get('material_bindings', []) or []:
        slot = int(binding.get('slot', 0) or 0)
        for use in binding.get('alpha_uses', []) or []:
            operand = str(use.get('operand') or '')
            if slot > 0 and operand in {'Alpha', 'OneMinusAlpha', 'Red', 'OneMinusRed',
                                        'Green', 'OneMinusGreen', 'Blue', 'OneMinusBlue'}:
                return True
    return False


def _friendly_seed(asset: dict) -> str:
    """Choose the best ROM-derived human-readable identity for an asset."""
    texture_name = str(asset.get('texture_name') or '').strip()
    model_name = _binding_model_name(asset)
    source_stem = Path(str(asset.get('source') or '')).stem

    # Texture names are normally the most exact identity.  Collapse conventional
    # generic diffuse suffixes when they merely repeat a model name.
    seed = texture_name or model_name or source_stem or str(asset.get('asset_id') or 'texture')
    if texture_name and model_name:
        t = _friendly_slug(texture_name)
        m = _friendly_slug(model_name)
        # Common Atlus model suffixes such as _b are not useful in a texture name.
        mroot = re.sub(r'-(?:a|b|model|mdl)$', '', m)
        for root in dict.fromkeys([m, mroot]):
            if not root:
                continue
            if t == root or re.fullmatch(re.escape(root) + r'-(?:t|tex|texture|diff|diffuse|color|body)?0*1', t):
                seed = root
                break

    slug = _friendly_slug(seed)
    # Normalize explicit ROM mask words to the requested `-alpha` terminology.
    slug = re.sub(r'-(?:mask|opacity|transparency|trans)$', '-alpha', slug)
    if _alpha_role_from_bindings(asset) and not re.search(r'-(?:alpha|mask|opacity)(?:$|-)', slug):
        slug += '-alpha'
    return slug


def assign_friendly_pack_names(assets: list[dict]) -> None:
    """Assign deterministic, globally unique PNG basenames.

    Azahar's pack.json lookup uses the basename, so uniqueness is enforced across
    every category folder.  Collisions receive a short stable hash suffix.
    """
    used: dict[str, str] = {}
    # Stable ordering prevents names changing because extraction order changes.
    ordered = sorted(assets, key=lambda a: (
        _friendly_seed(a), str(a.get('candidate_hash') or ''), str(a.get('asset_id') or '')
    ))
    for asset in ordered:
        base = _friendly_seed(asset)
        owner = str(asset.get('candidate_hash') or asset.get('asset_id') or '')
        filename = f'{base}.png'
        key = filename.lower()
        if key in used and used[key] != owner:
            suffix = str(asset.get('candidate_hash') or asset.get('asset_id') or 'texture')[:8].lower()
            filename = f'{base}-{suffix}.png'
            n = 2
            while filename.lower() in used and used[filename.lower()] != owner:
                filename = f'{base}-{suffix}-{n}.png'; n += 1
        used[filename.lower()] = owner
        asset['pack_filename'] = filename


def friendly_pack_filename(asset: dict) -> str:
    return str(asset.get('pack_filename') or f'{_friendly_seed(asset)}.png')


def _find_pack_file_by_name(root: Path, filename: str) -> Path | None:
    if not root.exists():
        return None
    matches = [p for p in root.rglob(filename) if p.is_file()]
    return matches[0] if len(matches) == 1 else None


def collect_protected_master_sources(root: Path) -> dict[str, Path]:
    """Find edited/upscaled masters before refreshing a workspace.

    Supports 0.12 friendly names, 0.11 canonical tex1_* masters, and older
    original/master layouts. Returned aliases include asset id and runtime hash
    keys so a renamed 0.12 destination can still inherit a protected 0.11 edit.
    """
    root = Path(root)
    previous = None
    for candidate in _manifest_candidates(root):
        if candidate.is_file():
            try:
                previous = json.loads(candidate.read_text(encoding='utf-8'))
                break
            except Exception:
                pass
    if not isinstance(previous, dict):
        return {}

    protected: dict[str, Path] = {}
    pack_master = root / MASTER_PACK_DIR
    for asset in previous.get('assets', []):
        if not isinstance(asset, dict):
            continue
        baseline = str(asset.get('rgba_sha256') or '')
        expected_size = (int(asset.get('width', 0) or 0), int(asset.get('height', 0) or 0))
        candidates: list[Path] = []
        for rel in asset.get('master_files') or []:
            p = root / str(rel)
            if p.is_file(): candidates.append(p)
        if asset.get('master'):
            p = root / str(asset['master'])
            if p.is_file() and p not in candidates: candidates.append(p)
        # Legacy canonical lookup if manifest paths are stale/missing.
        for h in _asset_hashes(asset, True):
            p = _find_pack_file_by_name(pack_master, canonical_pack_filename(asset, h))
            if p is not None and p not in candidates: candidates.append(p)
        if asset.get('pack_filename'):
            p = _find_pack_file_by_name(pack_master, str(asset['pack_filename']))
            if p is not None and p not in candidates: candidates.append(p)

        edited_source = None
        for existing in candidates:
            try:
                digest, size = _image_rgba_sha256(existing)
                if size != expected_size or (baseline and digest != baseline):
                    edited_source = existing; break
            except Exception:
                edited_source = existing; break
        if edited_source is not None:
            aid = str(asset.get('asset_id') or '')
            if aid: protected[f'asset:{aid}'] = edited_source
            if asset.get('pack_filename'): protected[str(asset['pack_filename'])] = edited_source
            for h in _asset_hashes(asset, True):
                protected[f'hash:{h}'] = edited_source
                protected[canonical_pack_filename(asset, h)] = edited_source

        # Very old original/master pair: protect only when master differs.
        legacy_master_rel = asset.get('master')
        legacy_orig_rel = asset.get('original')
        if not legacy_master_rel or not legacy_orig_rel:
            continue
        legacy_master = root / str(legacy_master_rel); legacy_orig = root / str(legacy_orig_rel)
        if not legacy_master.is_file() or not legacy_orig.is_file():
            continue
        try:
            md, ms = _image_rgba_sha256(legacy_master); od, os = _image_rgba_sha256(legacy_orig)
            edited = ms != os or md != od
        except Exception:
            edited = True
        if edited:
            aid = str(asset.get('asset_id') or '')
            if aid: protected.setdefault(f'asset:{aid}', legacy_master)
            for h in _asset_hashes(asset, True):
                protected.setdefault(f'hash:{h}', legacy_master)
                protected.setdefault(canonical_pack_filename(asset, h), legacy_master)
    return protected


def _protected_source_for_asset(asset: dict, protected_sources: dict[str, Path]) -> Path | None:
    keys = [friendly_pack_filename(asset), f"asset:{asset.get('asset_id','')}"]
    keys.extend(f'hash:{h}' for h in _asset_hashes(asset, True))
    keys.extend(canonical_pack_filename(asset, h) for h in _asset_hashes(asset, True))
    for key in keys:
        p = protected_sources.get(key)
        if p is not None and p.is_file():
            return p
    return None


def detect_edited_masters(root: Path) -> set[str]:
    """Legacy compatibility: return edited asset IDs from original/master pairs."""
    root = Path(root)
    try:
        manifest = json.loads(existing_manifest_path(root).read_text(encoding='utf-8'))
    except Exception:
        return set()
    protected: set[str] = set()
    for asset in manifest.get('assets', []):
        aid = str(asset.get('asset_id') or '')
        orig_rel = asset.get('original')
        master_rel = asset.get('master')
        if not aid or not orig_rel or not master_rel:
            continue
        orig = root / str(orig_rel); master = root / str(master_rel)
        if not orig.is_file() or not master.is_file():
            continue
        try:
            od, os = _image_rgba_sha256(orig); md, ms = _image_rgba_sha256(master)
            if os != ms or od != md:
                protected.add(aid)
        except Exception:
            protected.add(aid)
    return protected


def reset_generated_workspace(root: Path) -> dict[str, Path]:
    """Reset temporary extraction state while preserving azahar_pack_master."""
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    meta = root / METADATA_DIR
    work = meta / 'work'
    reports = meta / 'reports'
    diagnostics = meta / 'diagnostics'
    sheets = meta / 'contact_sheets'
    for path in (work, reports, diagnostics, sheets, root / DEPLOY_PACK_DIR):
        if path.exists():
            shutil.rmtree(path)
    return ensure_workspace(root)


def cleanup_streamlined_workspace(root: Path) -> None:
    """Remove all bulky transient/legacy trees after a successful 0.12 run."""
    root = Path(root)
    transient = root / METADATA_DIR / 'work'
    if transient.exists():
        shutil.rmtree(transient)
    # Remove old pre-0.12 persistent extraction trees after the new master pack
    # has been safely populated.
    for name in (
        '01_forge_static', '02_romfs_selected', '03_hpx_unpacked', '03b_farc_unpacked',
        '04_originals', '04_alpha_sources', '05_hd_masters', '05_alpha_masters',
        '05_alpha_masters_v04_legacy', '06_contact_sheets', '07_azahar_pack',
        '08_reports', '09_quarantine', '10_material_sets', '10_3d_materials',
    ):
        path = root / name
        if path.exists():
            shutil.rmtree(path)
    legacy_manifest = root / 'manifest.json'
    if legacy_manifest.exists():
        legacy_manifest.unlink()

def load_forge_manifest(forge_project: Path) -> list[dict]:
    path = forge_project/'manifest.json'
    if not path.exists():
        return []
    return json.loads(path.read_text(encoding='utf-8')).get('textures', [])


def ingest_forge_project(forge_project: Path, workspace: Path, title_id: str) -> list[dict]:
    dirs = ensure_workspace(workspace)
    rows = []
    manifest = load_forge_manifest(forge_project)
    by_rel = {t.get('decoded_png_path','').replace('\\','/'): t for t in manifest if t.get('decoded_png_path')}
    for png in forge_project.glob('*.png'):
        info = parse_azahar_filename(png.name)
        if not info:
            continue
        # In azahar mode manifest paths are usually basenames; fall back to hash lookup.
        rec = by_rel.get(png.name, {})
        if not rec:
            rec = next((t for t in manifest if t.get('raw_data_hash_xxh64','').upper() == info['hash']), {})
        source = rec.get('source_file_path','')
        cat = category_for(source)
        asset_id = f"{cat[:3]}_{info['hash']}"
        orig_dir = dirs['originals']/cat
        master_dir = dirs['masters']/cat
        orig_dir.mkdir(parents=True, exist_ok=True); master_dir.mkdir(parents=True, exist_ok=True)
        orig = orig_dir/f'{asset_id}.png'
        master = master_dir/f'{asset_id}.png'
        shutil.copy2(png, orig)
        if not master.exists():
            shutil.copy2(png, master)
        dh, fh = rgba_hashes(orig)
        rows.append({
            'asset_id': asset_id, 'title_id': title_id, 'category': cat,
            'source': source, 'source_kind': 'forge_romfs',
            'width': info['width'], 'height': info['height'], 'format': info['format'],
            'candidate_hash': info['hash'], 'verified_hashes': [], 'mip': info['mip'],
            'original': str(orig.relative_to(workspace)).replace('\\','/'),
            'master': str(master.relative_to(workspace)).replace('\\','/'),
            'rgba_sha256': dh, 'rgba_flip_sha256': fh,
            'target_scale': target_scale(cat, info['width'], info['height']),
            'hash_algorithm': 'legacy-import', 'notes': 'Imported legacy Texture Forge candidate. Recomputed CityHash64 candidates are used by the strict EOU pipeline.'
        })
    return rows


def add_decoded_texture(workspace: Path, title_id: str, source_path: Path, tex: dict, index: int = 0,
                        protected_masters: set[str] | None = None) -> dict:
    dirs = ensure_workspace(workspace)
    raw = tex['raw']
    h = cityhash64_hex(raw)
    source_text = str(source_path)
    # Include embedded texture names in categorization; model archives often use
    # generic filenames while the BCH/STEX metadata contains enemy/environment names.
    categorization_text = source_text + '/' + str(tex.get('name',''))
    cat = category_for(categorization_text)
    asset_id = f"{cat[:3]}_{h}" + (f'_{index}' if index else '')
    orig_dir = dirs['originals']/cat; master_dir = dirs['masters']/cat
    orig_dir.mkdir(parents=True, exist_ok=True); master_dir.mkdir(parents=True, exist_ok=True)
    orig = orig_dir/f'{asset_id}.png'; master = master_dir/f'{asset_id}.png'
    rgba = tex['rgba']
    Image.fromarray(rgba).save(orig)
    protected_masters = protected_masters or set()
    if asset_id not in protected_masters:
        shutil.copy2(orig, master)
    elif not master.exists():
        # A protected record with a missing file is no longer protectable.
        shutil.copy2(orig, master)
    dh, fh = rgba_hashes(orig)
    parser = str(tex.get('parser_used','unknown'))
    source_kind = 'embedded_model' if parser in {'bch_struct','cgfx_struct'} else ('eou_stex' if parser == 'eou_stex_strict' else 'strict_container')
    rec = {
        'asset_id': asset_id, 'title_id': title_id, 'category': cat,
        'source': source_text, 'source_kind': source_kind,
        'texture_name': tex.get('name',''),
        'width': int(tex['width']), 'height': int(tex['height']), 'format': int(tex['format']),
        'format_name': tex.get('format_name',''),
        'candidate_hash': h, 'verified_hashes': [], 'mip': 0,
        'original': str(orig.relative_to(workspace)).replace('\\','/'),
        'master': str(master.relative_to(workspace)).replace('\\','/'),
        'rgba_sha256': dh, 'rgba_flip_sha256': fh,
        'target_scale': target_scale(cat, int(tex['width']), int(tex['height'])),
        'parser_used': parser, 'confidence': tex.get('confidence',''),
        'hash_algorithm': 'CityHash64-new',
        'notes': 'Strict EOU decoder output. Candidate hash uses Azahar current new-hash algorithm (CityHash64) over the encoded base texture payload; runtime equivalence still depends on the uploaded byte span matching this payload exactly.'
    }
    # Material bindings come from CGFX/MTOB or BCH/H3D metadata, never from PNG appearance.
    bindings = []
    for binding in tex.get('material_bindings', []) or []:
        if not isinstance(binding, dict):
            continue
        bindings.append({
            **binding,
            'source': source_text,
            'container_offset': int(tex.get('container_offset', 0) or 0),
            'texture_name': tex.get('name', ''),
        })
    if bindings:
        rec['material_bindings'] = bindings
        rec['is_3d_material_texture'] = True
    else:
        rec['material_bindings'] = []
        rec['is_3d_material_texture'] = bool(tex.get('is_3d_material_texture', False))
    for key in ('data_type','pixel_format','data_offset','declared_size','trailing_bytes','container_offset','texture_index',
                'bch_material_count','bch_backward_compat','bch_version','bch_material_parse_error','cgfx_material_count','cgfx_model_count'):
        if key in tex:
            rec[key] = tex[key]
    return rec

def dedupe_assets(assets: list[dict]) -> list[dict]:
    # Candidate hash is the primary identity because Azahar replacement mapping is hash-based.
    seen = {}
    for a in assets:
        key = (a.get('candidate_hash'), a.get('format'), a.get('width'), a.get('height'))
        if key not in seen:
            seen[key] = a
        else:
            prior = seen[key]
            srcs = prior.setdefault('alternate_sources', [])
            source = a.get('source','')
            if source and source not in srcs and source != prior.get('source'):
                srcs.append(source)
            merge_material_bindings(prior, a)
    return list(seen.values())


def save_manifest(workspace: Path, title_id: str, assets: list[dict], source_rom: str = '',
                  version: str = '0.12.0', game_profile: dict | None = None,
                  product_code: str = '') -> Path:
    dirs = ensure_workspace(workspace)
    out = {
        'schema_version': 5,
        'project': 'Etrian Odyssey HD Texture Extractor',
        'extractor_version': version,
        'workspace_mode': 'streamlined_upscaling',
        'title_id': title_id,
        'product_code': product_code,
        'game_profile': game_profile or {},
        'game_id': (game_profile or {}).get('id', ''),
        'game': (game_profile or {}).get('display_name', ''),
        'source_rom': Path(source_rom).name if source_rom else '',
        'asset_count': len(assets),
        'assets': assets,
    }
    p = manifest_path(workspace)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(out, indent=2, ensure_ascii=False), encoding='utf-8')

    # Lightweight reports are retained under .eouhd; no duplicate image trees.
    csvp = dirs['reports'] / 'targets.csv'
    with csvp.open('w', newline='', encoding='utf-8-sig') as f:
        fields = ['asset_id','category','width','height','target_scale','candidate_hash','source','master','is_3d_material_texture','material_binding_count']
        w = csv.DictWriter(f, fieldnames=fields); w.writeheader()
        for a in assets:
            row = {k:a.get(k,'') for k in fields}
            row['material_binding_count'] = len(a.get('material_bindings', []) or [])
            w.writerow(row)
    safep = dirs['reports'] / 'targets_spoiler_safe.csv'
    with safep.open('w', newline='', encoding='utf-8-sig') as f:
        fields = ['asset_id','category','width','height','target_scale','candidate_hash','master','is_3d_material_texture','material_binding_count']
        w = csv.DictWriter(f, fieldnames=fields); w.writeheader()
        for a in assets:
            row = {k:a.get(k,'') for k in fields}
            row['material_binding_count'] = len(a.get('material_bindings', []) or [])
            w.writerow(row)
    return p


def load_manifest(workspace: Path) -> dict:
    for candidate in _manifest_candidates(workspace):
        if candidate.is_file():
            return json.loads(candidate.read_text(encoding='utf-8'))
    raise FileNotFoundError(f'No Etrian Odyssey HD workspace manifest found under {workspace}')


def _write_pack_metadata(pack_root: Path, title_id: str, version: str = '0.12.0', game_name: str = '',
                         textures: dict[str, str] | None = None) -> Path:
    out = pack_root / 'load' / 'textures' / title_id
    out.mkdir(parents=True, exist_ok=True)
    pack = {
        'author': 'Etrian Odyssey HD Texture Extractor user',
        'version': version,
        'description': f'{game_name or "Etrian Odyssey"} HD texture upscaling pack',
        'options': {
            'skip_mipmap': False,
            'flip_png_files': True,
            'use_new_hash': True,
        },
        'textures': textures or {},
    }
    (out / 'pack.json').write_text(json.dumps(pack, indent=2), encoding='utf-8')
    return out


def _write_pack_texture_map(title_root: Path, mappings: dict[str, str]) -> None:
    p = title_root / 'pack.json'
    data = json.loads(p.read_text(encoding='utf-8')) if p.is_file() else {}
    data['textures'] = dict(sorted(mappings.items()))
    p.write_text(json.dumps(data, indent=2), encoding='utf-8')


def sync_azahar_master_pack(
    workspace: Path,
    assets: list[dict],
    protected_sources: dict[str, Path] | None = None,
    use_candidates: bool = True,
) -> Path:
    """Create/update the human-readable editable Azahar master pack.

    Each unique asset is stored once. pack.json maps one or more CityHash64 values
    to that readable basename. Existing edited/upscaled masters are migrated and
    preserved even when moving from old canonical tex1_* filenames.
    """
    workspace = Path(workspace)
    protected_sources = protected_sources or {}
    if not assets:
        raise ValueError('No assets available for master-pack creation.')
    title_id = str(assets[0].get('title_id') or '').upper().zfill(16)
    if not title_id.strip('0'):
        raise ValueError('Missing Title ID for master-pack creation.')

    assign_friendly_pack_names(assets)
    final_root = workspace / MASTER_PACK_DIR
    temp_root = workspace / METADATA_DIR / '_master_build'
    if temp_root.exists(): shutil.rmtree(temp_root)
    game_name = str((assets[0].get('game') if assets else '') or '')
    title_root = _write_pack_metadata(temp_root, title_id, '0.12.0', game_name)

    mappings: dict[str, str] = {}
    exported = 0; preserved = 0
    for asset in assets:
        hashes = _asset_hashes(asset, use_candidates)
        if not hashes: continue
        category = _safe_category(asset.get('category', 'misc'))
        source = workspace / str(asset.get('master') or '')
        if not source.is_file(): source = workspace / str(asset.get('original') or '')
        if not source.is_file(): continue
        filename = friendly_pack_filename(asset)
        dest = title_root / category / filename
        dest.parent.mkdir(parents=True, exist_ok=True)
        protected = _protected_source_for_asset(asset, protected_sources)
        if protected is not None:
            shutil.copy2(protected, dest); preserved += 1
        else:
            shutil.copy2(source, dest)
        rel = f'{MASTER_PACK_DIR}/' + str(dest.relative_to(temp_root)).replace('\\','/')
        asset['master_files'] = [rel]
        asset['master'] = rel
        asset['pack_filename'] = filename
        asset['pack_hashes'] = hashes
        asset.pop('original', None)
        for hh in hashes: mappings[hh] = filename
        exported += 1

    _write_pack_texture_map(title_root, mappings)
    install = temp_root / 'INSTALL_TO_AZAHAR.txt'
    install.write_text(
        'Etrian Odyssey HD Texture Extractor 0.12 - MASTER PACK\n'
        '==========================================\n\n'
        'This is the editable/upscaling source of truth.\n'
        'Upscale or retouch the human-readable PNG files here, then use "Rebuild deployment pack".\n\n'
        f'Title ID: {title_id}\n'
        'Do not rename files casually: pack.json maps runtime hashes to these basenames.\n'
        'If you intentionally rename a PNG, update the matching pack.json entries too.\n',
        encoding='utf-8',
    )
    if final_root.exists(): shutil.rmtree(final_root)
    temp_root.replace(final_root)
    return final_root


def _safe_category(value: object) -> str:
    text = re.sub(r'[^A-Za-z0-9._-]+', '_', str(value or 'misc')).strip('._-')
    return text or 'misc'

def _pack_hash_images(folder: Path) -> list[tuple[str, Path, dict]]:
    """Return (hash, image_path, metadata) from a runtime dump or existing texture pack.

    Supports:
      * Azahar/Citra tex1_<WxH>_<HASH>_<fmt>[_mipN].png dump names
      * pack.json `textures` mappings where the key is the runtime hash
    """
    found: dict[tuple[str, str], tuple[str, Path, dict]] = {}
    for png in folder.rglob('*.png'):
        info = parse_azahar_filename(png.name)
        if info:
            found[(info['hash'], str(png.resolve()))] = (info['hash'], png, {'source': 'filename', **info})
    for pj in folder.rglob('pack.json'):
        try:
            data = json.loads(pj.read_text(encoding='utf-8'))
        except Exception:
            continue
        mappings = data.get('textures', {})
        if not isinstance(mappings, dict):
            continue
        # Azahar resolves the mapped value by basename. Search under this pack root.
        by_name = {}
        for f in pj.parent.rglob('*'):
            if f.is_file() and f.suffix.lower() in {'.png', '.dds', '.ktx'}:
                by_name.setdefault(f.name, []).append(f)
        for h, value in mappings.items():
            try:
                int(str(h), 16)
            except ValueError:
                continue
            values = value if isinstance(value, list) else [value]
            for v in values:
                if not isinstance(v, str):
                    continue
                # Current importer can visually compare PNGs. DDS/KTX still contribute
                # when a same-basename PNG exists next to them.
                name = Path(v).name
                candidates = by_name.get(name, [])
                if not candidates and (pj.parent / v).is_file():
                    candidates = [pj.parent / v]
                for img in candidates:
                    if img.suffix.lower() != '.png':
                        continue
                    hu = str(h).upper().zfill(16)
                    found[(hu, str(img.resolve()))] = (hu, img, {'source': 'pack_json'})
    return list(found.values())


def _thumbnail_vector(path: Path, flip: bool = False, size: int = 32):
    import numpy as np
    with Image.open(path) as im:
        im = im.convert('RGBA')
        if flip:
            im = ImageOps.flip(im)
        im = im.resize((size, size), Image.Resampling.LANCZOS)
        arr = np.asarray(im, dtype=np.float32) / 255.0
        # Premultiply color by alpha so transparent padding does not dominate matches.
        alpha = arr[:, :, 3:4]
        rgb = arr[:, :, :3] * alpha
        # Alpha still matters, but slightly less than RGB structure.
        return np.concatenate([rgb, alpha * 0.65], axis=2).reshape(-1)


def _visual_match(old_pack_png: Path, workspace: Path, assets: list[dict]) -> tuple[dict | None, float, float]:
    """Conservative visual match for HD/upscaled packs.

    Old HD packs often contain 2x/4x images, so exact bytes cannot match the original.
    Comparing both sides at a small canonical resolution survives ordinary upscaling,
    sharpening and mild cleanup while still requiring a unique close match.
    """
    import numpy as np
    try:
        vec = _thumbnail_vector(old_pack_png)
        vecf = _thumbnail_vector(old_pack_png, flip=True)
        with Image.open(old_pack_png) as pim:
            pw, ph = pim.size
    except Exception:
        return None, 1.0, 1.0

    ratio = pw / max(ph, 1)
    scored = []
    for a in assets:
        aw, ah = int(a.get('width', 0)), int(a.get('height', 0))
        if aw <= 0 or ah <= 0:
            continue
        ar = aw / ah
        if abs(ar - ratio) > max(0.02, ar * 0.02):
            continue
        op = workspace / a['master']
        if not op.exists():
            continue
        try:
            rv = _thumbnail_vector(op)
            # MAE across premultiplied RGBA fingerprint; check both vertical orientations.
            s1 = float(np.mean(np.abs(vec - rv)))
            s2 = float(np.mean(np.abs(vecf - rv)))
            scored.append((min(s1, s2), a))
        except Exception:
            continue
    if not scored:
        return None, 1.0, 1.0
    scored.sort(key=lambda x: x[0])
    best_score, best = scored[0]
    second = scored[1][0] if len(scored) > 1 else 1.0
    # Conservative thresholds: close underlying image + meaningful separation from runner-up.
    if best_score <= 0.055 and (second - best_score >= 0.012 or second >= 0.10):
        return best, best_score, second
    return None, best_score, second


def import_runtime_dump(workspace: Path, dump_dir: Path) -> dict:
    """Recover runtime hashes from an emulator dump or an existing/old HD pack.

    Exact matches are preferred and allow vertical flipping. If the source image has
    been upscaled/repainted, a conservative perceptual fallback can match it to the
    extracted original. Ambiguous matches are never written automatically.
    """
    manifest = load_manifest(workspace)
    assets = manifest['assets']
    direct = {}
    for a in assets:
        direct.setdefault(a.get('rgba_sha256'), []).append(a)
        direct.setdefault(a.get('rgba_flip_sha256'), []).append(a)

    matched_exact = 0; matched_visual = 0; ambiguous = 0; unmatched = []
    candidate_hash_matches = 0; candidate_hash_mismatches = 0
    evidence = []
    entries = _pack_hash_images(dump_dir)
    for runtime_hash, png, meta in entries:
        try:
            dh, fh = rgba_hashes(png)
        except Exception:
            unmatched.append(png.name)
            continue
        candidates = []
        for k in (dh, fh): candidates.extend(direct.get(k, []))
        uniq = {id(x): x for x in candidates}
        target = None; method = '' ; score = 0.0; second = 0.0
        if len(uniq) == 1:
            target = next(iter(uniq.values())); method = 'exact_rgba'; matched_exact += 1
        elif len(uniq) > 1:
            ambiguous += 1
            continue
        else:
            target, score, second = _visual_match(png, workspace, assets)
            if target is not None:
                method = 'visual_hd_downsample'; matched_visual += 1
            else:
                unmatched.append(png.name)
                continue

        hs = target.setdefault('verified_hashes', [])
        if runtime_hash not in hs:
            hs.append(runtime_hash)
        if method == 'exact_rgba':
            if runtime_hash.upper().zfill(16) == str(target.get('candidate_hash','')).upper().zfill(16):
                candidate_hash_matches += 1
            else:
                candidate_hash_mismatches += 1
        ev = target.setdefault('hash_evidence', [])
        ev.append({'hash': runtime_hash, 'method': method, 'file': png.name,
                   'score': round(score, 6) if method != 'exact_rgba' else 0,
                   'runner_up_score': round(second, 6) if method != 'exact_rgba' else 0})
        evidence.append({'asset_id': target['asset_id'], 'hash': runtime_hash, 'method': method,
                         'file': str(png), 'score': score, 'runner_up_score': second})

    manifest['assets'] = assets
    # Refresh pack.json hash mappings while preserving any user-edited friendly master image.
    protected_sources = collect_protected_master_sources(workspace)
    for asset in assets:
        asset.setdefault('title_id', manifest.get('title_id', ''))
    if assets:
        sync_azahar_master_pack(workspace, assets, protected_sources=protected_sources, use_candidates=True)
    existing_manifest_path(workspace).write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8')
    report = {
        'hash_images_found': len(entries),
        'matched_exact': matched_exact,
        'matched_visual_hd': matched_visual,
        'matched_total': matched_exact + matched_visual,
        'candidate_hash_matches_on_exact': candidate_hash_matches,
        'candidate_hash_mismatches_on_exact': candidate_hash_mismatches,
        'candidate_hash_validation': ('PASS' if candidate_hash_matches > 0 and candidate_hash_mismatches == 0 else ('FAIL' if candidate_hash_mismatches > 0 else 'NO_EXACT_SAMPLES')),
        'ambiguous': ambiguous,
        'unmatched': len(unmatched),
        'unmatched_files': unmatched,
        'evidence': evidence,
    }
    report_dir = workspace / METADATA_DIR / 'reports'
    report_dir.mkdir(parents=True, exist_ok=True)
    (report_dir / 'runtime_import.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    return report

def build_azahar_pack(workspace: Path, use_candidates: bool = True) -> Path:
    """Rebuild azahar_pack from the editable human-readable master pack."""
    workspace = Path(workspace)
    manifest = load_manifest(workspace)
    title_id = str(manifest['title_id']).upper().zfill(16)
    master_root = workspace / MASTER_PACK_DIR
    assets = manifest.get('assets', [])
    assign_friendly_pack_names(assets)
    if not master_root.exists():
        for asset in assets: asset.setdefault('title_id', title_id)
        sync_azahar_master_pack(workspace, assets, protected_sources={}, use_candidates=use_candidates)
        master_root = workspace / MASTER_PACK_DIR

    deploy_root = workspace / DEPLOY_PACK_DIR
    temp_root = workspace / METADATA_DIR / '_deploy_build'
    if temp_root.exists(): shutil.rmtree(temp_root)
    game_name = str(manifest.get('game') or (manifest.get('game_profile') or {}).get('display_name') or '')
    title_root = _write_pack_metadata(temp_root, title_id, '0.12.0', game_name)

    count = 0; hash_count = 0; mappings: dict[str, str] = {}
    for asset in assets:
        hashes = _asset_hashes(asset, use_candidates)
        if not hashes: continue
        filename = friendly_pack_filename(asset)
        primary = workspace / str(asset.get('master') or '')
        if not primary.is_file():
            primary = _find_pack_file_by_name(master_root, filename) or Path()
        if not primary.is_file():
            # Migration fallback from 0.11 canonical master names.
            for candidate_hash in _asset_hashes(asset, True):
                found = _find_pack_file_by_name(master_root, canonical_pack_filename(asset, candidate_hash))
                if found is not None: primary = found; break
        if not primary.is_file(): continue

        category = _safe_category(asset.get('category', 'misc'))
        dest = title_root / category / filename
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(primary, dest)
        for hh in hashes:
            mappings[hh] = filename; hash_count += 1
        asset['pack_filename'] = filename
        asset['pack_hashes'] = hashes
        count += 1

    _write_pack_texture_map(title_root, mappings)
    install = temp_root / 'INSTALL_TO_AZAHAR.txt'
    install.write_text(
        'Etrian Odyssey HD Texture Extractor 0.12 - Azahar deployment pack\n'
        '====================================================\n\n'
        f'Title ID detected: {title_id}\n\n'
        f'1. In Azahar, right-click {game_name or "the game"} and choose "Open Custom Texture Location".\n'
        '2. Copy the CONTENTS of this pack\'s load/textures/<TITLEID>/ folder into the folder Azahar opened.\n'
        '3. Category subfolders may remain; Azahar scans recursively.\n'
        '4. Keep pack.json at the Title ID root; it maps readable filenames to CityHash64 hashes.\n'
        '5. Enable Graphics > Enhancements > Use Custom Textures and restart the game.\n'
        '6. Leave use_new_hash=true.\n\n'
        'Do not edit this deployment copy. Edit azahar_pack_master and rebuild instead.\n',
        encoding='utf-8',
    )
    if deploy_root.exists(): shutil.rmtree(deploy_root)
    temp_root.replace(deploy_root)

    manifest['assets'] = assets
    existing_manifest_path(workspace).write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8')
    report = {
        'title_id': title_id,
        'assets_exported': count,
        'hash_mappings_exported': hash_count,
        'physical_png_files': count,
        'hash_algorithm': 'CityHash64 (Azahar new hash)',
        'use_new_hash': True,
        'canonical_filenames': False,
        'human_readable_filenames': True,
        'pack_json_hash_mapping': True,
        'categorized_subfolders': True,
        'source_of_truth': MASTER_PACK_DIR,
        'mapping_mode': 'verified-first; candidate fallback' if use_candidates else 'verified-only',
    }
    reports = workspace / METADATA_DIR / 'reports'; reports.mkdir(parents=True, exist_ok=True)
    (reports / 'azahar_pack_report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    return deploy_root / 'load' / 'textures' / title_id


def make_contact_sheets(workspace: Path, columns: int = 5, thumb: int = 180, hide_names: bool = True) -> list[Path]:
    manifest = load_manifest(workspace)
    by_cat = {}
    for a in manifest['assets']: by_cat.setdefault(a['category'], []).append(a)
    outputs=[]
    for cat, assets in sorted(by_cat.items()):
        for page_idx in range(0, len(assets), columns*5):
            page = assets[page_idx:page_idx+columns*5]
            rows = (len(page)+columns-1)//columns
            label_h=34
            sheet=Image.new('RGB',(columns*thumb, rows*(thumb+label_h)),(32,34,38))
            draw=ImageDraw.Draw(sheet)
            for i,a in enumerate(page):
                x=(i%columns)*thumb; y=(i//columns)*(thumb+label_h)
                with Image.open(workspace/a['master']) as im:
                    im=im.convert('RGBA'); im.thumbnail((thumb-12,thumb-12),Image.Resampling.LANCZOS)
                    tile=Image.new('RGBA',(thumb,thumb),(52,55,61,255))
                    tile.alpha_composite(im,((thumb-im.width)//2,(thumb-im.height)//2))
                    sheet.paste(tile.convert('RGB'),(x,y))
                label = a['asset_id'] if hide_names else f"{a['asset_id']} {Path(a.get('source','')).name}"
                draw.text((x+5,y+thumb+6),label[:28],fill=(230,230,230))
            out=(workspace / METADATA_DIR / 'contact_sheets')/f"{cat}_{page_idx//(columns*5)+1:02d}.jpg"
            out.parent.mkdir(parents=True, exist_ok=True)
            sheet.save(out,quality=90)
            outputs.append(out)
    return outputs
