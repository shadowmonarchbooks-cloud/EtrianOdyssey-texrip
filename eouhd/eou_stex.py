from __future__ import annotations

"""Strict Etrian Odyssey Untold-series STEX parser.

EOU STEX files do not encode the final PICA texture format in one field.
The 0x14 data-type field and 0x18 pixel-format field form a pair.  Treating
0x6752 (RGBA) as RGBA8 unconditionally, for example, corrupts RGBA4444 and
RGBA5551 textures.  This module follows the combinations documented by
UntoldUnpack's TileCodecs implementation.
"""

from dataclasses import dataclass
import struct
from typing import Optional


# OpenGL/PICA data-type constants used by Atlus STEX.
DT_UNSIGNED_BYTE = 0x1401
DT_UNSIGNED_BYTE_44_DMP = 0x6760
DT_UNSIGNED_4BITS_DMP = 0x6761
DT_UNSIGNED_SHORT_4444 = 0x8033
DT_UNSIGNED_SHORT_5551 = 0x8034
DT_UNSIGNED_SHORT_565 = 0x8363

# Pixel format constants.
PF_RGBA = 0x6752
PF_RGB = 0x6754
PF_ALPHA = 0x6756
PF_LUMINANCE = 0x6757
PF_LUMINANCE_ALPHA = 0x6758
PF_ETC1 = 0x675A
PF_ETC1A4 = 0x675B

# Texture Forge / PICA format IDs.
FMT_RGBA8 = 0x0
FMT_RGB8 = 0x1
FMT_RGBA5551 = 0x2
FMT_RGB565 = 0x3
FMT_RGBA4 = 0x4
FMT_LA8 = 0x5
FMT_L8 = 0x7
FMT_A8 = 0x8
FMT_LA4 = 0x9
FMT_L4 = 0xA
FMT_A4 = 0xB
FMT_ETC1 = 0xC
FMT_ETC1A4 = 0xD

FORMAT_PAIRS: dict[tuple[int, int], int] = {
    (DT_UNSIGNED_SHORT_4444, PF_RGBA): FMT_RGBA4,
    (DT_UNSIGNED_SHORT_5551, PF_RGBA): FMT_RGBA5551,
    (DT_UNSIGNED_BYTE, PF_RGBA): FMT_RGBA8,
    (DT_UNSIGNED_SHORT_565, PF_RGB): FMT_RGB565,
    (DT_UNSIGNED_BYTE, PF_RGB): FMT_RGB8,
    (DT_UNSIGNED_BYTE, PF_ETC1): FMT_ETC1,
    (DT_UNSIGNED_BYTE, PF_ETC1A4): FMT_ETC1A4,
    (DT_UNSIGNED_BYTE, PF_ALPHA): FMT_A8,
    (DT_UNSIGNED_4BITS_DMP, PF_ALPHA): FMT_A4,
    (DT_UNSIGNED_BYTE, PF_LUMINANCE): FMT_L8,
    (DT_UNSIGNED_4BITS_DMP, PF_LUMINANCE): FMT_L4,
    (DT_UNSIGNED_BYTE, PF_LUMINANCE_ALPHA): FMT_LA8,
    (DT_UNSIGNED_BYTE_44_DMP, PF_LUMINANCE_ALPHA): FMT_LA4,
}

FORMAT_BPP = {
    FMT_RGBA8: 32,
    FMT_RGB8: 24,
    FMT_RGBA5551: 16,
    FMT_RGB565: 16,
    FMT_RGBA4: 16,
    FMT_LA8: 16,
    FMT_L8: 8,
    FMT_A8: 8,
    FMT_LA4: 8,
    FMT_L4: 4,
    FMT_A4: 4,
    FMT_ETC1: 4,
    FMT_ETC1A4: 8,
}

FORMAT_NAMES = {
    FMT_RGBA8: 'RGBA8', FMT_RGB8: 'RGB8', FMT_RGBA5551: 'RGBA5551',
    FMT_RGB565: 'RGB565', FMT_RGBA4: 'RGBA4', FMT_LA8: 'LA8',
    FMT_L8: 'L8', FMT_A8: 'A8', FMT_LA4: 'LA4', FMT_L4: 'L4',
    FMT_A4: 'A4', FMT_ETC1: 'ETC1', FMT_ETC1A4: 'ETC1A4',
}


class STEXError(RuntimeError):
    pass


@dataclass(frozen=True)
class EOUSTEX:
    width: int
    height: int
    data_type: int
    pixel_format: int
    pica_format: int
    data_size_declared: int
    data_offset: int
    raw: bytes
    name: str
    trailing_bytes: int = 0


def is_stex(data: bytes) -> bool:
    return len(data) >= 0x24 and data[:4] == b'STEX'


def _base_level_size(width: int, height: int, fmt: int) -> int:
    """Expected base-level byte count including PICA 8x8 tile padding.

    STEX dimensions in EOU are normally multiples of 8, but using padded tile
    dimensions makes validation safe for small/nonstandard assets too.
    """
    bpp = FORMAT_BPP.get(fmt, 0)
    if not bpp:
        return 0
    pw = (width + 7) // 8 * 8
    ph = (height + 7) // 8 * 8
    return (pw * ph * bpp + 7) // 8


def parse_eou_stex(data: bytes) -> EOUSTEX:
    if not is_stex(data):
        raise STEXError('Not an STEX file')
    try:
        width = struct.unpack_from('<I', data, 0x0C)[0]
        height = struct.unpack_from('<I', data, 0x10)[0]
        data_type = struct.unpack_from('<I', data, 0x14)[0]
        pixel_format = struct.unpack_from('<I', data, 0x18)[0]
        declared = struct.unpack_from('<I', data, 0x1C)[0]
        image_offset = struct.unpack_from('<I', data, 0x20)[0]
    except struct.error as e:
        raise STEXError(f'Truncated STEX header: {e}') from e

    if width <= 0 or height <= 0 or width > 8192 or height > 8192:
        raise STEXError(f'Invalid STEX dimensions {width}x{height}')

    fmt = FORMAT_PAIRS.get((data_type, pixel_format))
    if fmt is None:
        raise STEXError(
            f'Unsupported EOU STEX format pair data_type=0x{data_type:04X}, '
            f'pixel_format=0x{pixel_format:04X}'
        )

    # UntoldUnpack accepts the declared offset when it is the normal 0x80 or
    # when declared size + offset exactly reaches EOF.  Preserve that behavior,
    # but reject impossible offsets rather than heuristically interpreting bytes.
    if image_offset == 0x80 or (image_offset > 0 and image_offset + declared == len(data)):
        data_offset = image_offset
    elif 0x24 <= image_offset < len(data) and declared > 0 and image_offset + declared <= len(data):
        data_offset = image_offset
    else:
        # Rare legacy STEX variant mirrored by UntoldUnpack: if the offset field
        # is not really an offset, pixel bytes begin at 0x20.  Only allow this
        # when the resulting payload is large enough for the declared format.
        data_offset = 0x20

    if data_offset >= len(data):
        raise STEXError(f'STEX data offset 0x{data_offset:X} is out of range')

    available = len(data) - data_offset
    base_size = _base_level_size(width, height, fmt)
    if base_size <= 0:
        raise STEXError('Could not determine STEX base texture size')

    # The declared size can include mip levels.  We only need the base level for
    # decoding and for an offline candidate hash, so take exactly its padded size.
    if available < base_size:
        raise STEXError(
            f'STEX payload too small: need {base_size} bytes for {width}x{height} '
            f'{FORMAT_NAMES.get(fmt, fmt)}, have {available}'
        )

    # UntoldUnpack intentionally tolerates NumImageBytes being larger than the
    # bytes physically available after ImageOffset: BinaryReader.ReadBytes simply
    # returns the remaining payload.  Several EOU/EO2U EFFECT STEX files use this
    # layout.  The base-level size derived from dimensions/format is the meaningful
    # safety check for our decoder, so do not reject a usable texture solely because
    # the declared byte count overshoots EOF.
    raw = data[data_offset:data_offset + base_size]
    trailing = max(0, available - base_size)

    name = ''
    # Most EOU STEX files use a 0x80-byte header and keep a filename after 0x28.
    if data_offset > 0x28:
        name_blob = data[0x28:data_offset]
        name = name_blob.split(b'\x00', 1)[0].decode('ascii', errors='replace').strip()

    return EOUSTEX(
        width=width, height=height, data_type=data_type,
        pixel_format=pixel_format, pica_format=fmt,
        data_size_declared=declared, data_offset=data_offset,
        raw=raw, name=name, trailing_bytes=trailing,
    )


def format_name(fmt: int) -> str:
    return FORMAT_NAMES.get(fmt, f'UNKNOWN_0x{fmt:X}')
