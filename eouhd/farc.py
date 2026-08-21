from __future__ import annotations

"""Read-only PMD-style FARC extraction used by EOU/EO2U model archives.

The layout follows the public pmd_farc/pmd_sir0 implementations:
  FARC header -> embedded SIR0 FAT -> file entries -> DAT payload region.
Both name-indexed (type 0) and CRC/hash-indexed (type 1) FATs are supported.
No decompression is guessed here: FARC itself is an archive, not the HPB reverse-LZ
format handled by :mod:`eouhd.hpx`.
"""

from dataclasses import dataclass
from pathlib import Path
import re
import struct
from typing import Iterable


class FARCError(RuntimeError):
    pass


@dataclass(frozen=True)
class FARCEntry:
    index: int
    name: str | None
    name_hash: int
    data_offset: int
    data_length: int
    absolute_offset: int


def _u32(data: bytes, off: int) -> int:
    if off < 0 or off + 4 > len(data):
        raise FARCError(f'FARC read out of bounds at 0x{off:X}')
    return struct.unpack_from('<I', data, off)[0]


def is_farc(data: bytes) -> bool:
    return len(data) >= 0x34 and data[:4] == b'FARC'


def _read_utf16z(data: bytes, off: int, limit: int = 0x1000) -> str:
    if off < 0 or off >= len(data):
        raise FARCError(f'FARC filename offset is out of bounds: 0x{off:X}')
    end_limit = min(len(data), off + limit)
    pos = off
    while pos + 2 <= end_limit:
        if data[pos:pos + 2] == b'\x00\x00':
            try:
                return data[off:pos].decode('utf-16le')
            except UnicodeDecodeError as exc:
                raise FARCError('Invalid UTF-16 filename in FARC FAT') from exc
        pos += 2
    raise FARCError('Unterminated UTF-16 filename in FARC FAT')


def parse_farc(data: bytes) -> tuple[list[FARCEntry], dict]:
    if not is_farc(data):
        raise FARCError('Not a PMD-style FARC archive')

    fat_type = _u32(data, 0x20)
    sir0_offset = _u32(data, 0x24)
    sir0_length = _u32(data, 0x28)
    all_data_offset = _u32(data, 0x2C)
    all_data_length = _u32(data, 0x30)

    if fat_type not in (4, 5):
        raise FARCError(f'Unsupported FARC SIR0 type {fat_type}; expected 4 or 5')
    if sir0_offset <= 0 or sir0_length < 12 or sir0_offset + sir0_length > len(data):
        raise FARCError('FARC SIR0 partition is out of bounds')
    if all_data_offset > len(data):
        raise FARCError('FARC data region starts past EOF')
    if all_data_length and all_data_offset + all_data_length > len(data):
        # Some games include alignment/padding semantics in this field. Do not
        # reject the whole archive when entry bounds themselves are valid.
        all_data_length = max(0, len(data) - all_data_offset)

    sir0 = data[sir0_offset:sir0_offset + sir0_length]
    if len(sir0) < 12 or sir0[:4] != b'SIR0':
        raise FARCError('FARC FAT does not contain a SIR0 header')
    header_offset = _u32(sir0, 4)
    pointer_offset = _u32(sir0, 8)
    if header_offset + 12 > len(sir0) or pointer_offset > len(sir0) or pointer_offset < header_offset:
        raise FARCError('Invalid SIR0 header/pointer offsets in FARC FAT')

    # This is the format-specific header exposed by pmd_sir0::Sir0::get_header().
    entry_table_offset = _u32(sir0, header_offset + 0)
    file_count = _u32(sir0, header_offset + 4)
    filename_mode = _u32(sir0, header_offset + 8)  # 0=name offset, 1=CRC/hash
    if filename_mode not in (0, 1):
        raise FARCError(f'Unsupported FARC FAT filename mode {filename_mode}')
    if file_count > 1_000_000:
        raise FARCError(f'Implausible FARC file count: {file_count}')
    if entry_table_offset + file_count * 12 > len(sir0):
        raise FARCError('FARC FAT entry table is out of bounds')

    entries: list[FARCEntry] = []
    for i in range(file_count):
        off = entry_table_offset + i * 12
        name_or_hash = _u32(sir0, off)
        rel_data = _u32(sir0, off + 4)
        length = _u32(sir0, off + 8)
        absolute = all_data_offset + rel_data
        if absolute < all_data_offset or absolute > len(data) or length > len(data) - absolute:
            raise FARCError(
                f'FARC entry {i} is out of bounds: start=0x{absolute:X}, size=0x{length:X}'
            )
        # PMD FARC payloads are normally aligned to 0x10. Keep an explicit flag
        # in metadata, but tolerate odd archives rather than throwing away them.
        name = _read_utf16z(sir0, name_or_hash) if filename_mode == 0 else None
        entries.append(FARCEntry(
            index=i,
            name=name,
            name_hash=name_or_hash if filename_mode == 1 else 0,
            data_offset=rel_data,
            data_length=length,
            absolute_offset=absolute,
        ))

    metadata = {
        'fat_type': fat_type,
        'sir0_offset': sir0_offset,
        'sir0_length': sir0_length,
        'all_data_offset': all_data_offset,
        'all_data_length': all_data_length,
        'header_offset': header_offset,
        'pointer_offset': pointer_offset,
        'entry_table_offset': entry_table_offset,
        'file_count': file_count,
        'filename_mode': filename_mode,
        'known_names': sum(1 for entry in entries if entry.name),
        'hashed_names': sum(1 for entry in entries if not entry.name),
        'misaligned_entries': sum(1 for entry in entries if entry.absolute_offset % 16 != 0),
    }
    return entries, metadata


def _safe_component(value: str) -> str:
    # FARC conceptually has no directories; names occasionally still contain
    # slashes. Flatten them to keep extraction traversal-proof and deterministic.
    value = value.replace('\\', '_').replace('/', '_').strip().strip('.')
    value = re.sub(r'[\x00-\x1f<>:"|?*]+', '_', value)
    value = re.sub(r'\s+', ' ', value).strip()
    return value[:180] or 'unnamed'


def _guess_suffix(payload: bytes) -> str:
    if payload.startswith(b'BCH\x00') or b'BCH\x00' in payload[:0x10000]:
        return '.bchbin'
    if payload.startswith(b'STEX'):
        return '.stex'
    if payload.startswith(b'FARC'):
        return '.farc'
    if payload.startswith(b'SIR0'):
        return '.sir0'
    if payload.startswith(b'CGFX'):
        return '.cgfx'
    if payload.startswith(b'CTPK'):
        return '.ctpk'
    return '.bin'


def unpack_farc(path: str | Path, output_dir: str | Path) -> tuple[list[Path], dict]:
    source = Path(path)
    data = source.read_bytes()
    entries, metadata = parse_farc(data)
    out_root = Path(output_dir)
    out_root.mkdir(parents=True, exist_ok=True)
    written: list[Path] = []

    for entry in entries:
        payload = data[entry.absolute_offset:entry.absolute_offset + entry.data_length]
        if entry.name:
            filename = _safe_component(entry.name)
            # Preserve a real suffix when present; add a diagnostic suffix only
            # when the archive name has no useful extension.
            if not Path(filename).suffix:
                filename += _guess_suffix(payload)
        else:
            filename = f'hash_{entry.name_hash:08X}_{entry.index:05d}{_guess_suffix(payload)}'
        dest = out_root / filename
        if dest.exists():
            stem, suffix = dest.stem, dest.suffix
            n = 2
            while (out_root / f'{stem}_{n}{suffix}').exists():
                n += 1
            dest = out_root / f'{stem}_{n}{suffix}'
        dest.write_bytes(payload)
        written.append(dest)

    return written, metadata


def find_farc_files(root: str | Path) -> Iterable[Path]:
    root = Path(root)
    if not root.exists():
        return
    for path in root.rglob('*'):
        if not path.is_file():
            continue
        try:
            with path.open('rb') as handle:
                if handle.read(4) == b'FARC':
                    yield path
        except OSError:
            continue
