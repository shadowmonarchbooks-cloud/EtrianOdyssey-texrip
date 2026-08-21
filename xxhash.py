"""Tiny compatibility subset of the third-party `xxhash` module.

Implements xxh64(data, seed=0).hexdigest()/intdigest()/digest() in pure Python.
It exists so 3DS Texture Forge's optional `import xxhash` path works without an
extra binary wheel. This is the standard XXH64 algorithm, not a custom hash.
"""
from __future__ import annotations
import struct

MASK=(1<<64)-1
P1=11400714785074694791
P2=14029467366897019727
P3=1609587929392839161
P4=9650029242287828579
P5=2870177450012600261

def _rotl(x,r): return ((x<<r)|(x>>(64-r))) & MASK

def _round(acc, inp):
    acc=(acc + inp*P2)&MASK
    acc=_rotl(acc,31)
    return (acc*P1)&MASK

def _merge(acc,val):
    acc ^= _round(0,val)
    return (acc*P1 + P4)&MASK

def _avalanche(h):
    h ^= h>>33; h=(h*P2)&MASK; h ^= h>>29; h=(h*P3)&MASK; h ^= h>>32
    return h&MASK

def xxh64_int(data: bytes|bytearray|memoryview, seed: int=0) -> int:
    b=memoryview(data).cast('B'); n=len(b); p=0; seed &= MASK
    if n>=32:
        v1=(seed+P1+P2)&MASK; v2=(seed+P2)&MASK; v3=seed; v4=(seed-P1)&MASK
        limit=n-32
        while p<=limit:
            v1=_round(v1,struct.unpack_from('<Q',b,p)[0]); p+=8
            v2=_round(v2,struct.unpack_from('<Q',b,p)[0]); p+=8
            v3=_round(v3,struct.unpack_from('<Q',b,p)[0]); p+=8
            v4=_round(v4,struct.unpack_from('<Q',b,p)[0]); p+=8
        h=(_rotl(v1,1)+_rotl(v2,7)+_rotl(v3,12)+_rotl(v4,18))&MASK
        h=_merge(h,v1); h=_merge(h,v2); h=_merge(h,v3); h=_merge(h,v4)
    else:
        h=(seed+P5)&MASK
    h=(h+n)&MASK
    while p+8<=n:
        k=_round(0,struct.unpack_from('<Q',b,p)[0]); h ^= k
        h=(_rotl(h,27)*P1+P4)&MASK; p+=8
    if p+4<=n:
        h ^= (struct.unpack_from('<I',b,p)[0]*P1)&MASK
        h=(_rotl(h,23)*P2+P3)&MASK; p+=4
    while p<n:
        h ^= (b[p]*P5)&MASK
        h=(_rotl(h,11)*P1)&MASK; p+=1
    return _avalanche(h)

class _XXH64:
    def __init__(self,data=b'',seed=0): self._seed=seed; self._buf=bytearray(data)
    def update(self,data): self._buf.extend(data); return self
    def intdigest(self): return xxh64_int(self._buf,self._seed)
    def hexdigest(self): return f'{self.intdigest():016x}'
    def digest(self): return self.intdigest().to_bytes(8,'big')
    def copy(self): return _XXH64(bytes(self._buf),self._seed)

def xxh64(data=b'', seed=0): return _XXH64(data,seed)
