# EO-TexRip independent application roadmap

EO-TexRip is being migrated from the current Python + 3DS Texture Forge workflow into a self-contained desktop application whose extraction, texture decoding, cataloguing, validation, and Azahar build pipeline are owned by this repository.

## Product target

The 1.0 application must:

- require no Python, pip, Git, Rust, Node.js, Java, or separately installed texture tools;
- perform no runtime dependency downloads;
- ship all redistributable application dependencies with the release;
- never ship Nintendo keys, ROM data, firmware, or proprietary game assets;
- support Etrian Odyssey IV, Etrian Odyssey Untold, Etrian Odyssey 2 Untold, Etrian Odyssey V, Etrian Odyssey Nexus, Etrian Mystery Dungeon, and Etrian Mystery Dungeon 2;
- use metadata-first, deterministic texture classification and retain `Unknown` when evidence is insufficient;
- preserve user edits, renames, tags, category overrides, and verified hash evidence across rescans;
- build validated Azahar texture packs without making Azahar naming rules the internal asset identity model.

## Release sequence

| Release | Objective |
| --- | --- |
| **0.13 Legacy Final** | Freeze and harden the Python extractor as the behavioral reference |
| **0.20 Core** | Rust workspace, core types, project model, structured errors |
| **0.30 Native ROM** | Native NCSD/CIA/NCCH/RomFS layer; remove Texture Forge ROM-reader dependency |
| **0.40 Texture Engine** | Native, tested PICA200 decoder and exact storage/hash semantics |
| **0.50 Containers** | Native archive/model/container framework |
| **0.60 Untold Parity** | EOU1 + EO2U parity with the frozen reference |
| **0.70 Universal EO** | EO IV + EO V + Nexus profiles |
| **0.75 Mystery Dungeon** | EMD1 + EMD2 profile/parser family |
| **0.80 Catalog** | Persistent asset catalog, browser, classification and editing workflow |
| **0.90 Beta** | Full supported-game matrix + Azahar integration |
| **0.95 RC** | Regression, security, packaging and recovery validation |
| **1.0** | Independent desktop release |

---

# 0.13 — Legacy Final

The Python implementation remains a reference implementation only after this milestone. New application architecture belongs in Rust from 0.20 onward.

## 0.13 exit criteria

### Correctness

- [x] Fix BCH/PICA base-level storage sizing for dimensions not aligned to 8x8 tiles.
- [x] Add non-8-aligned regression cases for ETC1, ETC1A4, RGBA8 and a 4-bit format.
- [x] Preserve the existing EOU1/EO2U structural parser regression suite through the hardening changes.
- [x] Capture a regression/fingerprint format for local legal game validation without committing copyrighted files.

### Input and extraction safety

- [x] Centralize archive/RomFS path containment.
- [x] Reject POSIX absolute paths, Windows drive paths, traversal components, device names and ADS syntax.
- [x] Reject truncated uncompressed HPB entries rather than silently slicing short data.
- [x] Add recursive extraction budgets: nesting depth, member count, expanded bytes and per-member allocation.

### Workspace safety

- [x] Preserve edited/upscaled master files even when the previous manifest is missing or corrupt.
- [x] Read live `pack.json` mappings when locating intentionally renamed masters.
- [x] Stage and validate master/deployment refreshes before atomic promotion.
- [x] Keep a rollback target until a new project state has been promoted successfully.
- [x] Require an EO-TexRip workspace marker before destructive legacy cleanup.

### Hash evidence

- [x] Exact RGBA/runtime evidence may become verified automatically.
- [x] Perceptual/downsample matches must remain candidates until explicitly confirmed.

### Material workspace

- [x] Stop persistent material reports from pointing at temporary files deleted after success.
- [x] Make material-workspace reconstruction compatible with the streamlined layout or retire the incompatible API.
- [x] Label reconstructed material alpha as diagnostic when exact shader reconstruction is not possible.

### Regression infrastructure

- [x] Add an explicit development/test dependency set.
- [x] Add Windows + Linux CI.
- [x] Run Python compile checks and the full pytest suite in CI.
- [x] Correct tests that inspect obsolete material-output paths.
- [x] Centralize the code-facing application version.
- [x] Prove the regression CI matrix on Windows/Linux and Python 3.10/3.12 before merging 0.13.

## 0.13 implementation notes

- Canonical application version: `0.13.0`.
- Frozen parser provenance: `0.12.0` is retained separately as the legacy-reference version.
- `azahar_pack_master` is treated as recoverable user state independently of the manifest.
- Exact and perceptual runtime-hash evidence now have separate trust levels.
- Copyright-safe structural fingerprints can be emitted with `python eouhd_cli.py fingerprint <workspace>` or `tools/fingerprint_workspace.py`.
- Archive resource ceilings are configurable through `EO_TEXRIP_MAX_ARCHIVE_DEPTH`, `EO_TEXRIP_MAX_EXTRACTED_FILES`, `EO_TEXRIP_MAX_EXPANDED_BYTES`, `EO_TEXRIP_MAX_MEMBER_BYTES`, and `EO_TEXRIP_MAX_ARCHIVE_BYTES`.
- Branch protection is intentionally not a 0.13 exit requirement; the legacy implementation is retained only as a comparison/reference baseline while active development moves to Rust.

## 0.13 rule

Do not broaden format heuristics while freezing the reference implementation. Every parser change must be supported by a structural reason and a regression test.

---

# 0.20 — Rust Core

0.20 begins only after the 0.13 reference behavior is stable enough to compare against.

## 0.20 exit criteria

### Workspace and domain model

- [x] Create the Cargo workspace at application version `0.20.0`.
- [x] Define stable `GameId`, region, Title ID, runtime-hash and asset-ID types.
- [x] Represent all seven EO-branded Nintendo 3DS targets explicitly.
- [x] Keep Atlus EO and Mystery Dungeon as separate profile families.
- [x] Separate visible texture dimensions from PICA200 8x8 storage dimensions.
- [x] Represent all common PICA200 texture formats and encoded base-level sizing.
- [x] Separate candidate, structural, runtime-verified and user-verified hash evidence.
- [x] Validate project schema, duplicate asset IDs, game mismatches and conflicting verified hashes.

### Rescan and user-state semantics

- [x] Preserve user-friendly names, category overrides and tags across rescans.
- [x] Preserve user-overridden classification across rescans.
- [x] Never downgrade stronger runtime-hash evidence to a weaker candidate.
- [x] Add explicit project serialization/load-save helpers around the core manifest contract.

### Stable subsystem boundaries

- [x] Add profile-registry contracts without guessing unsupported game compatibility.
- [x] Add ROM-reader interfaces without implementing NCSD/CIA/NCCH yet.
- [x] Add bounded archive-parser interfaces and shared extraction-budget types.
- [x] Add PICA texture-decoder interfaces and encoded/decoded payload validation.
- [x] Add model/material structural inspection interfaces.
- [x] Add Azahar pack-planning interfaces that only auto-map verified runtime hashes.

### Quality gate

- [x] Add Rust CI on Ubuntu and Windows.
- [x] Pass Clippy with warnings denied across the full workspace.
- [x] Pass the complete Rust workspace test suite on Ubuntu and Windows.
- [x] Document the behavioral comparison boundary between 0.13 Python fingerprints and future Rust extraction output.

Implemented workspace:

```text
eo-texrip/
├── crates/
│   ├── eo-core/
│   ├── eo-project/
│   ├── eo-rom/
│   ├── eo-archives/
│   ├── eo-textures/
│   ├── eo-models/
│   ├── eo-catalog/
│   ├── eo-azahar/
│   └── eo-profiles/
└── app/
    └── desktop/        # later UI milestone
```

Core domain types include `GameId`, `GameRegion`, `GameProfile`, `AssetId`, `TextureAsset`, `TextureFormat`, `TextureRole`, source provenance, `RuntimeHash`, and structured subsystem errors.

The long-term pipeline is:

```text
ROM Reader
  -> Game Detection
  -> Game Profile
  -> Archive Discovery
  -> Container Parsers
  -> PICA Texture Decoder
  -> Material Analysis
  -> Asset Catalog
  -> Deterministic Classification
  -> Master Workspace
  -> Azahar Builder
```

Game profiles decide where to look. Parsers decide what data is. The texture engine decides how it is decoded. The catalog stores stable asset identity and user decisions. Azahar remains an output target rather than the internal data model.

## 0.20 rule

Do not port binary parsers into this milestone. 0.20 freezes application-facing contracts first so later format work can be compared to the 0.13 reference without repeatedly changing project identity or persistence semantics.

---

# 0.30 — Native ROM

0.30 replaces the independent Rust application's external ROM-reader path with native, read-only Nintendo 3DS container inspection. The frozen Python 0.13 reference remains untouched while later Rust milestones reach extraction parity.

## 0.30 exit criteria

### Checked binary access

- [x] Add overflow-safe byte ranges and checked slicing before any binary region is exposed.
- [x] Support little-endian and big-endian primitive reads required by native 3DS container metadata.
- [x] Reject partitions, sections and file extents that exceed the actual source image.

### NCSD / CCI

- [x] Parse the NCSD header and eight-slot partition table using fixed 0x200 media units.
- [x] Expose partition bytes through validated ranges.
- [x] Permit correctly trimmed cartridge images whose nominal media capacity exceeds EOF while still validating every populated partition.

### NCCH / CXI

- [x] Parse NCCH/CXI identity, product code, media-unit shift and declared content size.
- [x] Parse Extended Header, Plain, Logo, ExeFS and RomFS regions with checked extents.
- [x] Distinguish CXI/executable content from non-executable NCCH content.
- [x] Detect the NCCH no-crypto and no-mount-RomFS flags.
- [x] Refuse encrypted ExeFS/RomFS bytes as cleartext.

### ExeFS

- [x] Parse the 0x200-byte ExeFS file table.
- [x] Validate entry extents and reject duplicate, overlapping or unsafe entries.
- [x] Expose structured ExeFS inspection through a cleartext NCCH/CXI.

### RomFS / IVFC

- [x] Parse IVFC/RomFS Level-3 layout without assuming a fixed Level-3 offset.
- [x] Traverse UTF-16 directory and file metadata.
- [x] Reject malformed metadata cycles and excessive node traversal.
- [x] Reject unsafe path components and malformed names.
- [x] Validate every file extent against the Level-3 filesystem region before exposing bytes.

### CIA / TMD

- [x] Parse the little-endian CIA archive header and aligned section layout.
- [x] Parse big-endian TMD signature/header/content records.
- [x] Read the CIA content-index bitmap and TMD encryption flags.
- [x] Derive included-content offsets from declared/TMD sizes with bounds checks.
- [x] Accept standard 64-byte content alignment and makerom-style 16-byte content packing only when the declared layout proves it.
- [x] Expose a clear main NCCH content and reject encrypted main content as `EncryptedInput`.
- [x] Recover the TMD Title ID even when main CIA content is encrypted.

### Native reader and profile handoff

- [x] Detect NCSD, CIA, NCCH/CXI and extracted IVFC/RomFS structurally rather than by file extension.
- [x] Add a native `RomIdentityHint` carrying Title ID and product code without coupling `eo-rom` to `eo-profiles`.
- [x] Support cleartext NCSD -> NCCH -> RomFS file enumeration/read.
- [x] Support cleartext CIA -> NCCH -> RomFS file enumeration/read.
- [x] Support direct NCCH/CXI and direct extracted RomFS inputs.
- [x] Keep unsupported/unknown data explicit instead of guessing a container or game profile.

### Security, legal boundary and regression coverage

- [x] Commit synthetic binary fixtures only; do not add copyrighted game/ROM content.
- [x] Do not bundle Nintendo keys, title keys, firmware or proprietary assets.
- [x] Keep user-supplied-key decryption as an explicit future optional layer rather than a hidden parser behavior.
- [x] Document supported native inputs and the encryption boundary in `docs/NATIVE_ROM.md`.
- [x] Pass Rust formatting/Clippy/tests on Ubuntu and Windows.
- [x] Keep the frozen Python regression matrix green while the native Rust ROM layer is introduced.

## 0.30 implementation notes

- Canonical Rust workspace version: `0.30.0`.
- `eo-rom` owns native NCSD/CCI, NCCH/CXI, ExeFS, IVFC/RomFS and CIA/TMD structural inspection.
- Container headers may be inspectable when payload content is encrypted; encrypted payload bytes are never silently interpreted as plaintext.
- CIA parsing supports the standard 64-byte section alignment and detects legacy makerom-style 16-byte content packing from structural size evidence.
- Title ID/product-code extraction is kept separate from game-profile policy. `eo-profiles` remains the authority on which identities are verified enough to auto-detect.
- Synthetic end-to-end fixtures exercise NCSD -> NCCH -> RomFS and CIA -> NCCH -> RomFS without external tools.
- Native ROM support removes the external ROM-reader requirement for the Rust application architecture; it does not delete the frozen Python reference implementation.

## 0.30 rule

The ROM layer is read-only and structural. Never guess encrypted bytes, container types, or game compatibility. Any future decryption support must be a separate explicit provider using keys supplied by the user; EO-TexRip must not ship or acquire Nintendo keys automatically.

---

# 0.40 — Native PICA200 Texture Engine

0.40 replaces the independent Rust application's external texture-decoder dependency with a native raw PICA200 decoder. Container parsing remains a later milestone: 0.40 accepts normalized encoded texture payloads and defines exactly how storage, mips, decoding and runtime-hash byte spans behave.

## 0.40 exit criteria

### Storage and mip semantics

- [x] Keep visible dimensions separate from 8x8-padded PICA storage dimensions.
- [x] Calculate encoded byte spans from padded storage dimensions for every supported format.
- [x] Keep sub-8-pixel mip levels in a complete 8x8 storage tile.
- [x] Produce exact per-mip offsets and sizes with overflow checks.
- [x] Expose the exact encoded level-0 span for runtime-hash adapters without later mips or container padding.

### PICA tile order

- [x] Implement row-major 8x8 macro tiles.
- [x] Implement Morton/Z-order pixels inside each 8x8 tile using interleaved X/Y bits.
- [x] Consume the entire padded storage image while cropping only decoded output to visible dimensions.
- [x] Keep vertical/presentation flips out of the raw hardware codec so container adapters can express them explicitly.

### Uncompressed PICA formats

- [x] Decode RGBA8.
- [x] Decode RGB8.
- [x] Decode RGBA5551.
- [x] Decode RGB565.
- [x] Decode RGBA4.
- [x] Decode LA8.
- [x] Decode HILO8.
- [x] Decode L8.
- [x] Decode A8.
- [x] Decode LA4.
- [x] Decode L4.
- [x] Decode A4.
- [x] Preserve the documented raw channel/nibble order instead of normalizing container assumptions into the codec.

### ETC1 / ETC1A4

- [x] Decode ETC1 individual mode.
- [x] Decode ETC1 differential mode and reject invalid 5-bit differential endpoints.
- [x] Support both ETC subblock orientations and all modifier tables/selectors.
- [x] Map the four sequential 4x4 ETC blocks to the four 16-pixel Morton groups of each 8x8 PICA tile.
- [x] Interpret Nintendo 3DS ETC color blocks in their stored little-endian representation.
- [x] Decode ETC1A4's little-endian 64-bit alpha plane with the same ETC pixel-index mapping.

### Validation matrix

- [x] Exercise every PICA format at 4x4, 7x7, 8x8, 9x9, 13x17, 64x64 and 257x129 visible dimensions.
- [x] Reject a one-byte-short padded payload for every format.
- [x] Exercise full mip chains down to visible 1x1 / stored 8x8.
- [x] Test Morton order, 4-bit nibble order, packed 16-bit channels, ETC block order, selector mapping and A4 alpha mapping.
- [x] Use synthetic fixtures only; do not commit copyrighted game textures.
- [x] Document the raw-codec, orientation and runtime-hash boundaries in `docs/TEXTURE_ENGINE.md`.
- [x] Pass Rust formatting, Clippy with warnings denied and the full workspace tests on Ubuntu and Windows.
- [x] Keep the frozen Python regression matrix green while the native texture engine is introduced.

## 0.40 implementation notes

- Canonical Rust workspace version: `0.40.0`.
- `eo-textures::NativePicaDecoder` owns the raw PICA200 decode path for all 14 formats represented by `TextureFormat`.
- Encoded texture data is normalized as level 0 followed by later mip levels; future container adapters must normalize any different physical container arrangement before constructing `EncodedTexture`.
- Runtime-hash adapters consume `EncodedTexture::runtime_hash_payload()`, which is exactly the PICA level-0 storage bytes.
- Raw decoding does not perform a vertical flip. Container/model/export layers remain responsible for presentation transforms when structurally required.
- Synthetic format tests cover both aligned and non-aligned dimensions and compressed/uncompressed storage.

## 0.40 rule

The texture engine owns PICA storage and pixel decoding, not container presentation policy. Never hash decoded RGBA pixels when the runtime profile expects encoded texture bytes, never include unrelated mip/alignment data in the base-level span, and never silently add a vertical flip to compensate for one container's convention.

---

# 0.50 — Native Containers

0.50 turns the format-agnostic archive, texture-container and model contracts from 0.20 into bounded native implementations. This milestone establishes the reusable parsing framework and the structurally proven Untold container families; full EOU1/EO2U end-to-end parity remains the 0.60 gate.

## 0.50 exit criteria

### Archive framework

- [x] Add checked archive byte access before exposing offsets, counts or payloads.
- [x] Add a conservative single-buffer archive registry that leaves unknown data unknown.
- [x] Carry recursive extraction budgets cumulatively across nested archive work.
- [x] Parse FARC with bounded tables, names and member extents.
- [x] Parse EPL general-resource packages with bounded resource/member metadata.
- [x] Parse paired HPI/HPB archives, including native reverse-LZ decoding and truncated-member rejection.
- [x] Decode Shift-JIS archive/member names explicitly without folding filesystem policy into binary parsing.

### Texture-container adapters

- [x] Parse Atlus STEX into normalized `EncodedTexture` metadata and exact padded level-0 payloads.
- [x] Parse CGFX image-texture `TXOB` objects and normalize their encoded PICA payloads.
- [x] Discover structurally valid CGFX payloads inside ATBC without assuming a fixed wrapper offset.
- [x] Parse version-aware BCH texture descriptors and PICA command streams into normalized encoded textures.
- [x] Discover structurally valid BCH payloads inside BAM2/ATBC without assuming a fixed wrapper offset.
- [x] Reject raw `CGFX`/`BCH` magic when the candidate header or declared sections do not validate.
- [x] Cap embedded-container scanning rather than treating arbitrary binary search as an extraction strategy.

### Model and material structure

- [x] Add native CGFX/CMDL/MTOB material inspection and structural Texture0/1/2 references.
- [x] Add native BCH/H3D model material-table inspection and compatibility-dependent material record layouts.
- [x] Preserve BCH texture-unit enabled/disabled state from `GPUREG_TEXUNIT_CONFIG`.
- [x] Keep semantic texture roles `Unknown` when slot position or decoded appearance is the only evidence.
- [x] Support direct model containers and the structurally validated ATBC/BAM2 wrapper paths used by the Untold families.

### Safety, documentation and regression coverage

- [x] Validate declared offsets, extents, counts and decompression allocations before use.
- [x] Commit synthetic fixtures only; do not add copyrighted game/container data.
- [x] Keep Nintendo keys, ROM content, firmware and proprietary assets outside the repository.
- [x] Document the native archive/container/model boundary in `docs/CONTAINERS.md`.
- [x] Pass Rust formatting, Clippy with warnings denied and the full workspace tests on Ubuntu and Windows.
- [x] Keep the frozen Python regression matrix green on Ubuntu/Windows and Python 3.10/3.12.

## 0.50 implementation notes

- Canonical Rust workspace version: `0.50.0`.
- `eo-archives` owns the bounded FARC, EPL and paired HPI/HPB parser implementations plus the conservative registry and shared recursive usage accounting.
- `eo-textures` owns native STEX, CGFX/ATBC and BCH/BAM2/ATBC adapters that normalize structurally declared payloads into the raw PICA texture contract established by 0.40.
- `eo-models::CgfxModelInspector` follows validated CGFX CMDL/MTOB/ReferenceTexture relationships rather than inferring material meaning from image appearance.
- `eo-models::BchModelInspector` follows H3D model/material tables and texture-unit command metadata while preserving disabled references and unknown semantic roles.
- BCH wrapper validation requires a real bounded content section and validates every declared section; a `BCH\0` byte sequence alone is not accepted as a payload.
- Formats that the legacy implementation delegated to generic Texture Forge paths are not claimed merely because their extension or magic is known. If EOU1/EO2U parity demonstrates that an additional format is required, 0.60 must add it from structural evidence and regression coverage rather than broadening heuristics preemptively.
- Exact H3D/CGFX shader semantics beyond the structural material/texture relationships required by the current model contract are parity work for 0.60 when supported by reference evidence.

## 0.50 rule

The container layer owns bounded structural interpretation, not compatibility guesses. A magic value is a probe hint, never proof by itself. Unknown formats remain unknown, semantic texture roles remain unknown without evidence, decompression is budgeted before allocation, and full Untold extraction parity is not declared until the 0.60 fingerprint comparison gate.
