# Native Nintendo 3DS ROM layer

EO-TexRip 0.30 introduces a native, read-only Nintendo 3DS ROM layer in `crates/eo-rom`. Its purpose is to remove the independent Rust application's dependency on a separately downloaded ROM-reading tool.

The frozen Python 0.13 reference implementation is intentionally unchanged and may continue to use its historical bridge while Rust parity is built. No claim in this document means the legacy Python application has already been deleted.

## Supported structural inputs

The native reader currently recognizes:

- NCSD/CCI cartridge images (`.3ds`) with a valid partition table;
- NCCH/CXI content images;
- CIA installable archives with TMD/content metadata;
- extracted IVFC/RomFS images;
- ExeFS regions when reached through a cleartext NCCH/CXI.

Container detection is structural. File extensions are not trusted as proof of format.

## Native read path

For cleartext game data, the supported paths are:

```text
NCSD / CCI
  -> partition 0
  -> NCCH / CXI
  -> RomFS
  -> files

CIA
  -> TMD + content-index metadata
  -> included main content (index 0)
  -> NCCH / CXI
  -> RomFS
  -> files

NCCH / CXI
  -> RomFS
  -> files

IVFC / RomFS
  -> files
```

ExeFS is inspected separately through the NCCH/CXI ExeFS region.

## Identity handoff

`eo-rom` does not depend on the game-profile crate. Instead it exposes a `RomIdentityHint` containing information recoverable from native container metadata:

- Nintendo 3DS Title ID / NCCH Program ID when available;
- NCCH product code when available.

The application layer can pass that hint to `eo-profiles::detect_verified_profile`. Only profile identities already verified by project evidence may auto-detect; the ROM layer does not guess game compatibility from similar file layouts.

For a CIA, the TMD Title ID remains available even when the main content is encrypted. The product code requires readable NCCH metadata.

## Encryption boundary

EO-TexRip does not ship Nintendo keys, title keys, firmware, ROM data, or proprietary game assets.

The 0.30 parser deliberately separates **metadata inspection** from **cleartext content access**:

- NCCH headers may be inspected without treating encrypted ExeFS/RomFS bytes as plaintext;
- CIA headers, TMD records, content-index membership and encryption flags may be inspected structurally;
- an encrypted CIA main content returns `RomError::EncryptedInput` when file bytes are requested;
- an encrypted NCCH returns `RomError::EncryptedInput` when ExeFS or RomFS bytes are requested.

A later optional decryption layer may accept keys supplied by the user. That layer must remain explicit and must not add bundled Nintendo keys or silent key acquisition.

## Safety and validation

All binary readers use checked ranges before slicing. The current implementation includes protections for:

- integer overflow in offset/size arithmetic;
- partitions and sections extending beyond the actual source length;
- trimmed NCSD images whose nominal media capacity is larger than the physical file, while still validating every real partition;
- dynamic NCCH media-unit sizing;
- CIA section alignment and TMD content extents;
- both standard 64-byte CIA content alignment and makerom-style 16-byte content packing when the declared size and TMD records prove the layout;
- RomFS metadata cycles and excessive metadata-node traversal;
- malformed UTF-16 names;
- unsafe RomFS path components;
- overlapping, duplicate or out-of-range ExeFS entries.

RomFS file offsets are validated against the IVFC Level-3 filesystem extent before data is exposed.

## Test policy

Committed ROM-layer tests use synthetic byte arrays only. They exercise the same structural layouts without embedding copyrighted Nintendo or game content.

Important synthetic integration paths include:

```text
NCSD -> NCCH -> RomFS -> data/test.bin
CIA  -> NCCH -> RomFS -> data/test.bin
```

The test payload is project-created fixture data, not extracted game data.

Real-game validation belongs in local, user-owned test runs and should record only copyright-safe fingerprints/results in the repository.

## 0.30 non-goals

0.30 does not implement:

- PICA200 texture decoding;
- Etrian Odyssey archive/model formats;
- bundled or automatic console-key acquisition;
- modification/repacking/signing of Nintendo containers;
- writable RomFS/ExeFS/CIA/NCCH support.

Those boundaries keep the ROM layer small, auditable and independent of later texture/container work.
