# Native container and model boundary

EO-TexRip 0.50 moves the independent Rust application from format contracts to bounded native parsers and adapters for the archive, texture-container, and model relationships needed by later game-parity milestones.

The 0.50 layer is deliberately structural. It identifies formats from validated metadata, exposes exact bounded payloads, and preserves explicit unknowns. It does not infer game compatibility from a container signature, guess malformed offsets, or assign semantic texture roles from slot position or decoded image appearance.

## Archive layer

`eo-archives` owns archive inspection and member reads under shared resource budgets.

### FARC

The native FARC parser validates table and member extents before exposing payload bytes. Named entries preserve their structural names; hashed entries remain unknown metadata rather than receiving invented filenames.

### EPL

The native EPL parser follows the bounded general-resource-package structure used by the frozen Untold reference. Resource/member counts are checked before allocation, member extents are validated, and Shift-JIS names are decoded explicitly when required.

### HPI / HPB

HPI index metadata and HPB payload storage are handled as a paired format. The parser validates entry bounds, rejects truncated payloads, implements the legacy reverse-LZ stream natively, and applies output-size limits before decompression allocation. Shift-JIS member names are decoded without folding filesystem policy into the binary parser.

### Registry and recursive budgets

Single-buffer formats use a conservative archive registry: unknown input remains unknown and HPI is not misrepresented as a standalone archive. Recursive extraction usage is cumulative and transactional across nesting depth, member count, expanded bytes, member size, and archive size.

## Texture-container adapters

Container adapters normalize physical format metadata into `EncodedTexture`. The raw PICA200 decoder from 0.40 remains responsible only for hardware storage and pixel decoding.

### STEX

The native STEX adapter resolves the Untold format pair, visible dimensions, padded base-level storage span, and exact encoded payload. Unsupported format pairs remain unsupported rather than being guessed. Declared-size overshoot is accepted only under the frozen-reference rule where the complete structurally required base level is physically present.

### CGFX and ATBC

The native CGFX adapter validates the CGFX header and declared extent, then accepts only image-texture `TXOB` objects with the expected structural type. ATBC is treated as a wrapper: embedded CGFX payloads may occur at non-fixed offsets, but candidate magic is accepted only after full CGFX header validation and scanning is capped.

CGFX texture data is normalized to the exact encoded PICA payload. Container presentation policy remains outside the raw decoder.

### BCH, BAM2, and ATBC BCH payloads

The native BCH adapter parses version-aware BCH section metadata and PICA texture command streams. Direct BCH is supported, while BAM2/ATBC wrappers use bounded embedded-BCH discovery rather than fixed offsets. A `BCH\0` byte sequence alone is not sufficient: the payload must contain a real bounded content section and every declared section must fit the source.

Texture dimensions, formats, data addresses, and exact level-0 spans are derived from the command metadata. Unknown or malformed descriptors are skipped rather than converted into speculative textures.

## Model and material inspection

`eo-models` exposes structural material-to-texture relationships separately from texture decoding.

### CGFX / ATBC models

`CgfxModelInspector` validates direct CGFX or a structurally valid CGFX payload embedded in ATBC. It reads the first CMDL model name, valid MTOB material records, their three texture mapper fields, TexInfo references, and reference-texture TXOB names.

Only structurally linked texture slots are emitted. `TextureRole` remains `Unknown` because a slot number alone is not evidence that a texture is diffuse, mask, normal, alpha, or another semantic role.

### BCH / BAM2 models

`BchModelInspector` accepts direct BCH or structurally valid BCH embedded in BAM2/ATBC. It follows the H3D model section, model material table, compatibility-dependent material record layout, Texture0/1/2 names, and the material texture-unit command block.

Texture enable bits come from `GPUREG_TEXUNIT_CONFIG`; disabled references remain explicit metadata rather than disappearing. Semantic roles remain `Unknown` unless a later layer has stronger structural evidence.

Multiple H3D models may contribute material records to one inventory. The singular `model_name` field is retained only when the payload has one model name (or all discovered model names agree); otherwise it remains absent rather than mislabeling the combined inventory.

## Safety and scope rules

- All offsets, extents, counts, and decompression allocations are bounded before use.
- Wrapper scanning is capped and validates full candidate structure; raw magic alone is never authoritative.
- Unknown containers and unsupported revisions remain explicit.
- Shift-JIS decoding is a binary-format concern; path containment and workspace naming are separate policies.
- No Nintendo keys, ROM data, firmware, or proprietary game assets are bundled.
- Committed regression fixtures are synthetic only.
- 0.50 does not add decryption or runtime key acquisition.
- 0.50 does not claim full EOU1/EO2U extraction parity; that is the 0.60 milestone.
- Exact shader/material reconstruction beyond the structural texture bindings exposed here remains a later parity concern. The model layer must not manufacture semantic texture roles to make the catalog look complete.

## Handoff to 0.60

0.50 is complete when the native archive, texture-container, and model boundaries are stable, documented, regression-tested, and green on the Rust and frozen-Python CI matrices. 0.60 then uses those components end to end for Etrian Odyssey Untold and Etrian Odyssey 2 Untold and compares the resulting structural fingerprints against the frozen 0.13 reference.
