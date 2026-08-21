"""Pure-Python CityHash64 compatible with Azahar's current custom-texture hash.

Ported from Azahar's src/common/cityhash.cpp (Google CityHash, MIT license).
Only CityHash64 is implemented because that is what Azahar currently uses for
both the legacy and new custom-texture hashing paths.
"""
from __future__ import annotations

MASK = (1 << 64) - 1
K0 = 0xC3A5C85C97CB3127
K1 = 0xB492B66FBE98F273
K2 = 0x9AE16A3B2F90404F
KMUL = 0x9DDFEA08EB382D69


def _u64(x: int) -> int:
    return x & MASK


def _fetch64(s: bytes, off: int) -> int:
    return int.from_bytes(s[off:off + 8], 'little', signed=False)


def _fetch32(s: bytes, off: int) -> int:
    return int.from_bytes(s[off:off + 4], 'little', signed=False)


def _rotate(v: int, shift: int) -> int:
    v &= MASK
    if shift == 0:
        return v
    return ((v >> shift) | ((v << (64 - shift)) & MASK)) & MASK


def _shift_mix(v: int) -> int:
    v &= MASK
    return (v ^ (v >> 47)) & MASK


def _bswap64(v: int) -> int:
    return int.from_bytes((v & MASK).to_bytes(8, 'little'), 'big')


def _hash128to64(low: int, high: int) -> int:
    a = _u64((low ^ high) * KMUL)
    a ^= a >> 47
    b = _u64((high ^ a) * KMUL)
    b ^= b >> 47
    b = _u64(b * KMUL)
    return b


def _hash_len16(u: int, v: int, mul: int | None = None) -> int:
    if mul is None:
        return _hash128to64(u & MASK, v & MASK)
    mul &= MASK
    a = _u64((u ^ v) * mul)
    a ^= a >> 47
    b = _u64((v ^ a) * mul)
    b ^= b >> 47
    b = _u64(b * mul)
    return b


def _hash_len0to16(s: bytes) -> int:
    n = len(s)
    if n >= 8:
        mul = _u64(K2 + n * 2)
        a = _u64(_fetch64(s, 0) + K2)
        b = _fetch64(s, n - 8)
        c = _u64(_rotate(b, 37) * mul + a)
        d = _u64((_rotate(a, 25) + b) * mul)
        return _hash_len16(c, d, mul)
    if n >= 4:
        mul = _u64(K2 + n * 2)
        a = _fetch32(s, 0)
        return _hash_len16(_u64(n + (a << 3)), _fetch32(s, n - 4), mul)
    if n > 0:
        a = s[0]
        b = s[n >> 1]
        c = s[n - 1]
        y = a + (b << 8)
        z = n + (c << 2)
        return _u64(_shift_mix(_u64(y * K2) ^ _u64(z * K0)) * K2)
    return K2


def _hash_len17to32(s: bytes) -> int:
    n = len(s)
    mul = _u64(K2 + n * 2)
    a = _u64(_fetch64(s, 0) * K1)
    b = _fetch64(s, 8)
    c = _u64(_fetch64(s, n - 8) * mul)
    d = _u64(_fetch64(s, n - 16) * K2)
    return _hash_len16(
        _u64(_rotate(_u64(a + b), 43) + _rotate(c, 30) + d),
        _u64(a + _rotate(_u64(b + K2), 18) + c),
        mul,
    )


def _weak_hash_len32_with_seeds_vals(w: int, x: int, y: int, z: int, a: int, b: int) -> tuple[int, int]:
    a = _u64(a + w)
    b = _rotate(_u64(b + a + z), 21)
    c = a
    a = _u64(a + x + y)
    b = _u64(b + _rotate(a, 44))
    return _u64(a + z), _u64(b + c)


def _weak_hash_len32_with_seeds(s: bytes, off: int, a: int, b: int) -> tuple[int, int]:
    return _weak_hash_len32_with_seeds_vals(
        _fetch64(s, off), _fetch64(s, off + 8), _fetch64(s, off + 16), _fetch64(s, off + 24), a, b
    )


def _hash_len33to64(s: bytes) -> int:
    n = len(s)
    mul = _u64(K2 + n * 2)
    a = _u64(_fetch64(s, 0) * K2)
    b = _fetch64(s, 8)
    c = _fetch64(s, n - 24)
    d = _fetch64(s, n - 32)
    e = _u64(_fetch64(s, 16) * K2)
    f = _u64(_fetch64(s, 24) * 9)
    g = _fetch64(s, n - 8)
    h = _u64(_fetch64(s, n - 16) * mul)
    u = _u64(_rotate(_u64(a + g), 43) + _u64((_rotate(b, 30) + c) * 9))
    v = _u64(((a + g) ^ d) + f + 1)
    w = _u64(_bswap64(_u64((u + v) * mul)) + h)
    x = _u64(_rotate(_u64(e + f), 42) + c)
    y = _u64((_bswap64(_u64((v + w) * mul)) + g) * mul)
    z = _u64(e + f + c)
    a = _u64(_bswap64(_u64((x + z) * mul + y)) + b)
    b = _u64(_shift_mix(_u64((z + a) * mul + d + h)) * mul)
    return _u64(b + x)


def cityhash64(data: bytes | bytearray | memoryview) -> int:
    s = bytes(data)
    n = len(s)
    if n <= 32:
        return _hash_len0to16(s) if n <= 16 else _hash_len17to32(s)
    if n <= 64:
        return _hash_len33to64(s)

    x = _fetch64(s, n - 40)
    y = _u64(_fetch64(s, n - 16) + _fetch64(s, n - 56))
    z = _hash_len16(_u64(_fetch64(s, n - 48) + n), _fetch64(s, n - 24))
    v = _weak_hash_len32_with_seeds(s, n - 64, n, z)
    w = _weak_hash_len32_with_seeds(s, n - 32, _u64(y + K1), x)
    x = _u64(x * K1 + _fetch64(s, 0))

    remaining = (n - 1) & ~63
    off = 0
    while remaining:
        x = _u64(_rotate(_u64(x + y + v[0] + _fetch64(s, off + 8)), 37) * K1)
        y = _u64(_rotate(_u64(y + v[1] + _fetch64(s, off + 48)), 42) * K1)
        x ^= w[1]
        x &= MASK
        y = _u64(y + v[0] + _fetch64(s, off + 40))
        z = _u64(_rotate(_u64(z + w[0]), 33) * K1)
        v = _weak_hash_len32_with_seeds(s, off, _u64(v[1] * K1), _u64(x + w[0]))
        w = _weak_hash_len32_with_seeds(s, off + 32, _u64(z + w[1]), _u64(y + _fetch64(s, off + 16)))
        z, x = x, z
        off += 64
        remaining -= 64

    return _hash_len16(
        _u64(_hash_len16(v[0], w[0]) + _u64(_shift_mix(y) * K1) + z),
        _u64(_hash_len16(v[1], w[1]) + x),
    )


def cityhash64_hex(data: bytes | bytearray | memoryview) -> str:
    return f"{cityhash64(data):016X}"
