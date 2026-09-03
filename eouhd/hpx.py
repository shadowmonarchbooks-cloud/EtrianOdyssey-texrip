from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import struct
from typing import Iterable

from .safety import UnsafeArchivePath, safe_archive_join


class HPXError(RuntimeError):
    pass


@dataclass(frozen=True)
class HPIEntry:
    index: int
    filename: str
    file_offset: int
    compressed_size: int
    decompressed_size: int


def _u16(data: bytes, off: int) -> int:
    return struct.unpack_from('<H', data, off)[0]


def _u32(data: bytes, off: int) -> int:
    return struct.unpack_from('<I', data, off)[0]


def parse_hpi(index_path: str | Path) -> list[HPIEntry]:
    """Parse an Atlus HPI index used by the Etrian Odyssey Untold-series 3DS games.

    This is a clean Python port of the documented layout used by UntoldUnpack:
      HPIH header (0x18), N unknown 4-byte entries, M 16-byte file entries,
      followed by a Shift-JIS filename table.
    """
    p = Path(index_path)
    data = p.read_bytes()
    if len(data) < 0x18 or data[:4] != b'HPIH':
        raise HPXError(f'Not a valid HPI index: {p}')

    num_unknown = _u16(data, 0x12)
    num_files = _u16(data, 0x14)
    file_table = 0x18 + num_unknown * 4
    names_base = file_table + num_files * 16
    if names_base > len(data):
        raise HPXError('HPI tables extend past end of file')

    entries: list[HPIEntry] = []
    names = data[names_base:]
    for i in range(num_files):
        off = file_table + i * 16
        filename_offset, file_offset, comp_size, decomp_size = struct.unpack_from('<IIII', data, off)
        if filename_offset >= len(names):
            filename = f'unnamed_{i:05d}.bin'
        else:
            end = names.find(b'\x00', filename_offset)
            if end < 0:
                end = len(names)
            filename = names[filename_offset:end].decode('cp932', errors='replace')
        entries.append(HPIEntry(i, filename, file_offset, comp_size, decomp_size))
    return entries


def _decompress_acmp_block(blob: bytes) -> bytes:
    """Decompress the reverse-LZ block used by Atlus HPB entries.

    Ported from UntoldUnpack's CompressedFile algorithm.  The compressed stream is
    consumed backwards and the output is written backwards through a 0x8000-byte
    history window.
    """
    if len(blob) < 0x28:
        raise HPXError('Compressed HPB entry is too small')

    magic = blob[:4]
    compressed_size = _u32(blob, 0x04)
    header_size = _u32(blob, 0x08)
    decompressed_size = _u32(blob, 0x10)
    if header_size < 0x20:
        raise HPXError(f'Unexpected compressed header size: {header_size:#x}')

    data_start = header_size
    if data_start + compressed_size > len(blob):
        # Original reader takes compressed_size bytes after the header. Keep the
        # implementation tolerant of archive slices whose size field excludes padding.
        compressed = blob[data_start:]
    else:
        compressed = blob[data_start:data_start + compressed_size]
    if len(compressed) < 8:
        raise HPXError('Compressed payload has no trailer')

    compressed_and_trailer = _u32(compressed, len(compressed) - 8)
    decompressed_increase = _u32(compressed, len(compressed) - 4)
    trailer_size = (compressed_and_trailer >> 24) & 0xFF
    trailer_compressed_size = compressed_and_trailer & 0xFFFFFF
    if trailer_size == 0 or trailer_size > len(compressed):
        raise HPXError(f'Invalid reverse-LZ trailer size: {trailer_size}')

    history = bytearray(0x8000)
    out = bytearray([0xAA]) * decompressed_size
    hist_i = 0
    written = 0
    in_ofs = len(compressed) - trailer_size
    out_ofs = decompressed_size
    target = trailer_compressed_size + decompressed_increase

    def read_back() -> int:
        nonlocal in_ofs
        in_ofs -= 1
        if in_ofs < 0:
            raise HPXError('Reverse-LZ input underflow')
        return compressed[in_ofs]

    def write_back(v: int) -> None:
        nonlocal out_ofs, written, hist_i
        out_ofs -= 1
        if out_ofs < 0:
            raise HPXError('Reverse-LZ output overflow')
        out[out_ofs] = v
        history[hist_i] = v
        hist_i = (hist_i + 1) & (len(history) - 1)
        written += 1

    while written < target and in_ofs >= 0:
        flags = read_back()
        for bit in range(7, -1, -1):
            if written >= target:
                break
            if (flags >> bit) & 1:
                x = read_back()
                count = (x >> 4) + 3
                distance = (((x & 0x0F) << 8) | read_back()) + 3
                for _ in range(count):
                    write_back(history[(hist_i - distance) & (len(history) - 1)])
            else:
                write_back(read_back())

    while written < decompressed_size:
        write_back(read_back())
    return bytes(out)


def _case_insensitive_sibling(path: Path, suffix: str) -> Path:
    candidate = path.with_suffix(suffix)
    if candidate.exists():
        return candidate
    try:
        wanted_stem = path.stem.casefold()
        wanted_suffix = suffix.casefold()
        for sibling in path.parent.iterdir():
            if (
                sibling.is_file()
                and sibling.stem.casefold() == wanted_stem
                and sibling.suffix.casefold() == wanted_suffix
            ):
                return sibling
    except OSError:
        pass
    return candidate


def unpack_hpi_pair(index_path: str | Path, output_dir: str | Path) -> list[Path]:
    index = Path(index_path)
    if index.suffix.lower() == '.hpb':
        binary = index
        index = _case_insensitive_sibling(index, '.hpi')
    else:
        binary = _case_insensitive_sibling(index, '.hpb')
    if not index.exists() or not binary.exists():
        raise HPXError(f'Missing HPI/HPB pair for {index}')

    entries = parse_hpi(index)
    hpb = binary.read_bytes()
    out_root = Path(output_dir)
    written: list[Path] = []

    for e in entries:
        if e.file_offset >= len(hpb):
            continue
        # For uncompressed entries the index size is the direct payload size.
        if e.decompressed_size == 0:
            end = e.file_offset + e.compressed_size
            if end > len(hpb):
                raise HPXError(
                    f'Uncompressed HPB entry {e.filename!r} exceeds archive bounds: '
                    f'0x{e.file_offset:X}+0x{e.compressed_size:X} > 0x{len(hpb):X}'
                )
            payload = hpb[e.file_offset:end]
        else:
            # The reverse-LZ header tells us how much payload is present. Include
            # enough of the HPB tail for the declared block and let the decoder validate.
            tail = hpb[e.file_offset:]
            if len(tail) < 0x20:
                continue
            block_comp_size = _u32(tail, 0x04)
            header_size = _u32(tail, 0x08)
            total = header_size + block_comp_size
            if header_size < 0x20 or total > len(tail):
                raise HPXError(
                    f'Compressed HPB entry {e.filename!r} exceeds archive bounds: '
                    f'header=0x{header_size:X}, compressed=0x{block_comp_size:X}, '
                    f'available=0x{len(tail):X}'
                )
            payload = _decompress_acmp_block(tail[:total])

        # Archive filenames may use either slash style. Unsafe members are ignored
        # rather than rewritten, so an archive can never redirect output elsewhere.
        try:
            dest = safe_archive_join(out_root, e.filename)
        except UnsafeArchivePath:
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(payload)
        written.append(dest)
    return written


def find_hpi_pairs(root: str | Path) -> Iterable[Path]:
    root = Path(root)
    for hpi in root.rglob('*'):
        if not hpi.is_file() or hpi.suffix.lower() != '.hpi':
            continue
        if _case_insensitive_sibling(hpi, '.hpb').exists():
            yield hpi
