from __future__ import annotations

"""Read-only Atlus EPL general-resource-package extractor.

The layout implemented here follows the public AtlusLibSharp/Amicitia EPL reader:
  * file count / data table pointer at 0x80
  * fixed 0xC0-byte resource records from DataStart
  * per-resource descriptor pointer at record + 0x90
  * resource payload relative offset + size at descriptor + 0x20

EOU/EO2U use many .EPL effect resources.  Parsing is deliberately conservative:
invalid counts, pointers or bounds reject the archive rather than guessing.
"""

from dataclasses import dataclass, asdict
from pathlib import Path
import re
import struct
from collections import Counter


class EPLError(RuntimeError):
    pass


@dataclass
class EPLEntry:
    index: int
    name: str
    table_offset: int
    data_offset: int
    data_size: int
    magic_ascii: str


def _i32(data: bytes, off: int) -> int:
    if off < 0 or off + 4 > len(data):
        raise EPLError(f'offset 0x{off:X} is outside EPL')
    return struct.unpack_from('<i', data, off)[0]


def _cstring(raw: bytes) -> str:
    raw = raw.split(b'\0', 1)[0]
    try:
        return raw.decode('utf-8')
    except UnicodeDecodeError:
        return raw.decode('shift_jis', errors='replace')


def _safe_name(value: str, fallback: str) -> str:
    value = value.replace('\\', '/').split('/')[-1].strip()
    value = re.sub(r'[^A-Za-z0-9._@+\-]+', '_', value)
    value = value.strip('._')
    return value[:120] or fallback


def _magic_ascii(payload: bytes) -> str:
    return ''.join(chr(x) if 32 <= x < 127 else '.' for x in payload[:4])


def guess_member_suffix(payload: bytes, original_name: str = '') -> str:
    """Return a conservative extension for unnamed EPL members."""
    if payload.startswith(b'STEX'):
        return '.stex'
    if payload.startswith(b'CGFX'):
        return '.cgfx'
    if payload.startswith(b'BCH\x00'):
        return '.bch'
    if payload.startswith(b'ATBC'):
        return '.bam'
    if payload.startswith(b'CTPK'):
        return '.ctpk'
    if payload.startswith((b'CTXB', b'ctxb')):
        return '.ctxb'
    if payload.startswith(b'FARC'):
        return '.farc'
    if payload.startswith(b'EPL'):
        return '.epl'
    # Preserve a meaningful source extension if the package supplied one.
    suffix = Path(original_name).suffix
    if suffix and len(suffix) <= 12:
        return suffix.lower()
    return '.bin'


def parse_epl(data: bytes) -> tuple[list[EPLEntry], dict]:
    if len(data) < 0x8C:
        raise EPLError('EPL is too small for the 0x80 resource header')

    file_count = _i32(data, 0x80)
    unknown = _i32(data, 0x84)
    data_start = _i32(data, 0x88)

    if file_count <= 0 or file_count > 10000:
        raise EPLError(f'implausible EPL file count {file_count}')
    if data_start < 0x8C or data_start >= len(data):
        raise EPLError(f'EPL data table offset 0x{data_start:X} is out of range')

    record_size = 0xC0
    table_end = data_start + file_count * record_size
    if table_end > len(data):
        raise EPLError(
            f'EPL resource table exceeds file: need 0x{table_end:X}, file is 0x{len(data):X}'
        )

    entries: list[EPLEntry] = []
    signature_counts: Counter[str] = Counter()
    for i in range(file_count):
        rec = data_start + i * record_size
        table_offset = _i32(data, rec + 0x90)
        name = _cstring(data[rec + 0x9C: rec + 0x9C + 36])
        if table_offset < 0 or table_offset + 0x28 > len(data):
            raise EPLError(f'entry {i} descriptor offset 0x{table_offset:X} is out of range')

        rel_offset = _i32(data, table_offset + 0x20)
        data_size = _i32(data, table_offset + 0x24)
        data_offset = table_offset + rel_offset
        if data_size < 0:
            raise EPLError(f'entry {i} has negative payload size {data_size}')
        if data_offset < 0 or data_offset + data_size > len(data):
            raise EPLError(
                f'entry {i} payload bounds 0x{data_offset:X}+0x{data_size:X} exceed file 0x{len(data):X}'
            )

        payload = data[data_offset:data_offset + data_size]
        magic = _magic_ascii(payload)
        signature_counts[magic] += 1
        entries.append(EPLEntry(
            index=i,
            name=name,
            table_offset=table_offset,
            data_offset=data_offset,
            data_size=data_size,
            magic_ascii=magic,
        ))

    metadata = {
        'file_count': file_count,
        'unknown': unknown,
        'data_start': data_start,
        'record_size': record_size,
        'member_magics': dict(signature_counts.most_common()),
    }
    return entries, metadata


def unpack_epl(path: str | Path, output_dir: str | Path) -> tuple[list[Path], dict]:
    source = Path(path)
    data = source.read_bytes()
    entries, metadata = parse_epl(data)
    out = Path(output_dir)
    out.mkdir(parents=True, exist_ok=True)

    written: list[Path] = []
    members: list[dict] = []
    known_texture_members = 0
    for e in entries:
        payload = data[e.data_offset:e.data_offset + e.data_size]
        base = _safe_name(e.name, f'member_{e.index:04d}')
        suffix = guess_member_suffix(payload, base)
        # Do not double-append a matching extension supplied by the archive.
        if Path(base).suffix.lower() != suffix:
            base = f'{base}{suffix}'
        dest = out / f'{e.index:04d}_{base}'
        dest.write_bytes(payload)
        written.append(dest)
        if payload.startswith((b'STEX', b'CGFX', b'BCH\x00', b'ATBC', b'CTPK', b'CTXB', b'ctxb')):
            known_texture_members += 1
        members.append({**asdict(e), 'output': dest.name, 'guessed_suffix': suffix})

    return written, {
        **metadata,
        'known_texture_members': known_texture_members,
        'members': members,
    }


def find_epl_files(root: str | Path):
    root = Path(root)
    if not root.exists():
        return
    for p in root.rglob('*'):
        if p.is_file() and p.suffix.lower() == '.epl':
            yield p
