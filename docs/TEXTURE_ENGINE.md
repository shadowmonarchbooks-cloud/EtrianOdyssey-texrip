# Native PICA200 texture engine

EO-TexRip 0.40 introduces a native raw-texture engine in `crates/eo-textures`. The engine owns the Nintendo 3DS/PICA200 storage contract used by later STEX, BCH, CGFX and other container adapters.

The raw codec deliberately does **not** own game/container presentation rules. If a particular container or material system requires a vertical flip, UV transform or other presentation transform, that adapter applies it explicitly after raw texture decoding.

## Storage model

Every mip level has two dimensions:

- **visible dimensions** — the image dimensions reported by the texture/container;
- **storage dimensions** — visible width/height independently rounded up to multiples of 8 pixels.

For example:

```text
visible 13 x 17
storage 16 x 24
```

All encoded byte-span calculations use storage dimensions. Decoded output is cropped back to visible dimensions.

This distinction remains true for small mip levels. A visible `1 x 1` mip still occupies one `8 x 8` PICA storage tile for the formats supported by this engine.

## 8x8 PICA swizzle

Macro tiles are stored row-major. Pixels inside each 8x8 tile follow Morton/Z order by interleaving coordinate bits:

```text
x0, y0, x1, y1, x2, y2
```

The beginning of a tile therefore maps sequential storage pixels as:

```text
0  -> (0,0)
1  -> (1,0)
2  -> (0,1)
3  -> (1,1)
4  -> (2,0)
5  -> (3,0)
6  -> (2,1)
7  -> (3,1)
...
```

The codec walks the full padded storage image and discards only pixels whose storage-space coordinate lies outside the visible image.

## Uncompressed formats

0.40 decodes these PICA formats directly to row-major RGBA8:

- RGBA8
- RGB8
- RGBA5551
- RGB565
- RGBA4
- LA8
- HILO8
- L8
- A8
- LA4
- L4
- A4

Raw byte/channel interpretation is explicit. In particular:

- RGBA8 encoded bytes are ABGR memory order;
- RGB8 encoded bytes are BGR memory order;
- LA8 stores alpha then luminance;
- HILO8 maps the second encoded byte to output red and the first to output green;
- L4/A4 consume the low nibble before the high nibble.

## ETC1 and ETC1A4

ETC textures retain the same 8x8 PICA macro-tile structure. Each 8x8 tile contains four 4x4 ETC blocks corresponding to the four consecutive 16-pixel Morton groups:

```text
0..15   -> top-left 4x4
16..31  -> top-right 4x4
32..47  -> bottom-left 4x4
48..63  -> bottom-right 4x4
```

The decoder implements ETC1 individual and differential endpoint modes, both subblock orientations, all eight modifier tables and both selector bitplanes.

Nintendo 3DS ETC color blocks are interpreted in their stored little-endian 64-bit representation. ETC selector bits use ETC's column-major `x * 4 + y` numbering, so the decoder performs an explicit mapping between sequential PICA/Morton pixels and ETC selector indices.

ETC1A4 stores an additional little-endian 64-bit alpha plane per 4x4 block. Each alpha nibble uses the same ETC pixel index and is expanded from 4 bits to 8 bits.

Malformed ETC1 differential endpoints that leave the legal 5-bit endpoint range are rejected instead of being silently interpreted as another texture mode.

## Mip levels

`EncodedTexture` models a normalized payload as:

```text
level 0
level 1
level 2
...
```

Container adapters must normalize any container-specific physical layout before constructing the core value.

For each stored level, `MipLevelLayout` records:

- visible dimensions;
- padded storage dimensions;
- exact encoded offset;
- exact encoded size.

Each mip remains independently 8x8-padded. No generic container alignment is included in the raw mip size.

## Runtime-hash byte span

`EncodedTexture::runtime_hash_payload()` returns exactly the encoded level-0 storage bytes:

```text
base-level PICA bytes only
```

It deliberately excludes:

- later mip levels;
- unrelated container data;
- container alignment after level 0;
- decoded RGBA bytes.

Later Azahar/runtime-hash adapters should hash this exact encoded span when the game/emulator hash profile calls for the PICA base-level payload.

## Orientation boundary

The raw engine does not perform an implicit vertical flip. This is intentional.

A vertical flip can arise from a container convention, model UV convention, image export convention or renderer presentation choice. Baking one into the PICA hardware decoder would make otherwise-correct formats disagree and would prevent container adapters from expressing their actual semantics.

## Validation matrix

Committed tests are synthetic and cover every supported PICA format at:

- 4x4
- 7x7
- 8x8
- 9x9
- 13x17
- 64x64
- 257x129

The matrix checks both aligned and non-aligned dimensions, output cropping and exact decoded length. Additional tests cover:

- one-byte-short padded payload rejection for every format;
- Morton coordinate order;
- packed 4-bit and 16-bit channel layouts;
- ETC quadrant/block order;
- ETC selector-bit mapping;
- ETC1A4 alpha mapping;
- differential endpoint validation;
- multi-level mip sizing down to visible 1x1 / stored 8x8;
- exact runtime-hash level-0 spans.

No copyrighted game texture data is committed. Real user-owned game validation belongs to local fingerprint/parity runs against the frozen 0.13 reference.

## 0.40 non-goals

0.40 does not implement:

- STEX/BCH/CGFX/container parsing in Rust;
- material/UV reconstruction;
- PNG export policy;
- texture encoding/repacking;
- image upscaling;
- game-specific categorization.

Those layers consume the raw texture engine in later milestones rather than being embedded into the codec.
