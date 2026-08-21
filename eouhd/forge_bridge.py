from __future__ import annotations

from contextlib import contextmanager
from pathlib import Path
import importlib
import os
import subprocess
import sys
from typing import Callable


class ForgeError(RuntimeError):
    pass


def locate_forge(root: str | Path | None) -> Path:
    candidates = []
    if root:
        candidates.append(Path(root))
    env = os.environ.get('TEXTURE_FORGE_HOME')
    if env:
        candidates.append(Path(env))
    here = Path(__file__).resolve().parents[1]
    candidates += [here / 'tools' / '3DS-Texture-Forge', here.parent / 'tools' / '3DS-Texture-Forge']
    for c in candidates:
        if c.is_file() and c.name == 'main.py':
            return c.parent
        if (c / 'main.py').exists() and (c / 'parsers').exists():
            return c
    raise ForgeError('3DS Texture Forge source folder not found. Run bootstrap_tools.py or select its folder in Settings.')


# Kept for compatibility/debugging. v0.2 deliberately does NOT use this broad
# scan-all path during a normal extraction because it can promote heuristic false
# positives into the HD workspace.
def run_forge_extract(rom: str | Path, output_base: str | Path, forge_root: str | Path,
                      on_line: Callable[[str], None] | None = None) -> Path:
    forge = locate_forge(forge_root)
    cmd = [sys.executable, str(forge / 'main.py'), 'extract', str(rom),
           '-o', str(output_base), '--report', '--output-mode', 'azahar']
    env = os.environ.copy()
    project_root = str(Path(__file__).resolve().parents[1])
    env['PYTHONPATH'] = project_root + (os.pathsep + env['PYTHONPATH'] if env.get('PYTHONPATH') else '')
    proc = subprocess.Popen(cmd, cwd=str(forge), stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, bufsize=1, universal_newlines=True, env=env)
    assert proc.stdout is not None
    lines = []
    for line in proc.stdout:
        lines.append(line)
        if on_line:
            on_line(line.rstrip())
    rc = proc.wait()
    if rc != 0:
        raise ForgeError('3DS Texture Forge extraction failed.\n' + ''.join(lines[-30:]))
    manifests = sorted(Path(output_base).glob('*/manifest.json'), key=lambda p: p.stat().st_mtime, reverse=True)
    if not manifests:
        raise ForgeError('Texture Forge finished but no manifest.json was produced')
    return manifests[0].parent


@contextmanager
def forge_import_path(forge_root: str | Path):
    forge = str(locate_forge(forge_root))
    old = list(sys.path)
    sys.path.insert(0, forge)
    try:
        yield
    finally:
        sys.path[:] = old


def _romfs_candidate(path: str, probe: bytes) -> bool:
    """Conservative RomFS pre-filter for supported Etrian Odyssey 3DS titles.

    Always keep Atlus HPI/HPB archives. Keep standalone STEX and known CTR texture
    containers. EOU1 uses ATBC/CGFX BAM resources; EO2U is known to use
    BAM/BAM2-wrapped BCH/H3D resources. Both are selected by extension/magic.
    """
    ext = Path(path).suffix.lower()
    if ext in {'.hpi', '.hpb', '.stex', '.bch', '.bcres', '.bcmdl', '.cmb', '.ctpk', '.ctxb', '.bam', '.bam2', '.farc', '.epl'}:
        return True
    if probe.startswith((b'STEX', b'BCH\x00', b'CGFX', b'ATBC', b'CTPK', b'CTXB', b'ctxb', b'cmb ', b'FARC')):
        return True
    if b'BCH\x00' in probe:
        return True
    return False


def extract_romfs_selected(rom: str | Path, output_dir: str | Path, forge_root: str | Path,
                           extensions: set[str] | None = None) -> tuple[str, str, list[Path]]:
    """Extract supported Etrian Odyssey archives and strict texture/model candidates from RomFS.

    Texture Forge is used only for its tested NCSD/CIA/NCCH/RomFS reader. Selection
    covers the shared HPI/HPB/STEX layer plus EOU1 ATBC/CGFX and EO2U BAM2/BCH
    model resources.
    """
    out = Path(output_dir)
    out.mkdir(parents=True, exist_ok=True)
    with forge_import_path(forge_root):
        main = importlib.import_module('main')
        romfs_data, title_id, product_code, _chain = main.parse_rom(str(rom))
        RomFSParser = importlib.import_module('parsers.romfs').RomFSParser
        fs = RomFSParser(romfs_data)
        entries = fs.list_files()
        written: list[Path] = []
        for idx, (path, offset, size) in enumerate(entries):
            ext = Path(path).suffix.lower()
            # Optional compatibility override used by tests/custom callers.
            if extensions is not None:
                selected = ext in extensions
            else:
                # A 1 MiB probe is cheap because parse_rom already holds RomFS in RAM,
                # and is large enough to see EOU1's ATBC wrapper and embedded CGFX header.
                probe = romfs_data[offset:offset + min(size, 0x100000)] if size > 0 else b''
                selected = _romfs_candidate(path, probe)
            if not selected:
                continue
            _p, data = fs.read_file_by_index(idx)
            rel = Path(path.replace('\\', '/').lstrip('/'))
            if '..' in rel.parts:
                continue
            dest = out / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
            written.append(dest)
    return title_id, product_code, written


def decode_file_with_forge(path: str | Path, forge_root: str | Path, title_id: str = '') -> list[dict]:
    """Legacy generic decoder, retained for diagnostics.

    The v0.2 main pipeline uses eouhd.strict_scan instead because STEX requires an
    EOU-specific two-field format mapping and BCH heuristics can create noise.
    """
    p = Path(path)
    data = p.read_bytes()
    with forge_import_path(forge_root):
        scanner = importlib.import_module('textures.scanner')
        decoder = importlib.import_module('textures.decoder')
        texs, fp = scanner.extract_textures_with_confidence(data, str(p), scan_all=False, title_id=title_id)
        out = []
        for t in texs:
            raw = t.get('data', b'')
            w, h, fmt = int(t.get('width', 0)), int(t.get('height', 0)), int(t.get('format', 0))
            if not raw or w <= 0 or h <= 0:
                continue
            rgba = decoder.decode_texture_fast(raw, w, h, fmt)
            if rgba is None:
                continue
            out.append({
                'width': w, 'height': h, 'format': fmt,
                'format_name': decoder.get_format_name(fmt),
                'raw': raw, 'rgba': rgba,
                'parser_used': t.get('parser_used', fp.detected_type or 'unknown'),
                'confidence': t.get('confidence', 'unknown'),
                'name': t.get('name', ''),
            })
        return out
