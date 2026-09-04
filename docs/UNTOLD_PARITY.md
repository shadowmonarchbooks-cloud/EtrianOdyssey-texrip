# 0.60 Untold Native Extraction

0.60 composes the native Rust ROM, archive, texture, and model layers into the first usable end-to-end Etrian Odyssey Untold / Etrian Odyssey 2 Untold extractor. The product goal is straightforward: give EO-TexRip a supported decrypted ROM and get usable decoded texture files back without Python or 3DS Texture Forge.

The frozen Python 0.13 implementation remains available as optional regression evidence. Exact fingerprint equality is **not** a 0.60 release blocker. Native behavior should be driven by format correctness, real extraction results, and bounded synthetic tests rather than reproducing legacy quirks.

## Exit criteria

### Native orchestration

- [x] Add a dedicated `eo-untold` orchestration crate at workspace version `0.60.0`.
- [x] Require a verified EOU1 or EO2U identity before using the Untold pipeline.
- [x] Enumerate candidate RomFS files through the native `RomReader` contract.
- [x] Reject oversized candidate reads before requesting their payload bytes.
- [x] Pair HPI/HPB paths case-insensitively without relying on host filesystem behavior.
- [x] Expand HPI/HPB, FARC, and EPL archives through bounded native parsers.
- [x] Keep archive discovery counters separate from strict texture/model candidates.
- [x] Keep Nintendo keys, ROM bytes, firmware, and proprietary game assets out of the repository.

### Native texture and model path

- [x] Route STEX through the native STEX adapter and native PICA decoder.
- [x] Route direct and ATBC-wrapped CGFX through native texture and material inspection.
- [x] Route direct and BAM/ATBC-wrapped BCH through native texture and material inspection.
- [x] Deduplicate decoded assets by `(candidate_hash, format, width, height)`.
- [x] Merge material bindings through structural texture-name relationships rather than image heuristics.
- [x] Expose structural model and material relationships for CGFX and BCH/H3D.
- [x] Surface known but unsupported CTPK/CTXB/CMB candidates explicitly instead of silently claiming support.

### User-facing native extraction

- [x] Retain successfully decoded RGBA8 pixels for native output rather than discarding them after validation.
- [x] Add `eo-texrip extract <decrypted-rom> [-o|--output <directory>]`.
- [x] Write decoded textures as ordinary RGBA PNG files with no Python or external image tool dependency.
- [x] Use deterministic Windows-safe filenames and coarse category directories.
- [x] Preserve internal texture names when available and fall back to source-derived names when they are not.
- [x] Write `extraction-report.json` with game identity, output mapping, parser provenance, dimensions, hashes, material-binding counts, and warnings.
- [x] Keep unsupported-container/decode failures visible in the report instead of dropping them silently.
- [ ] Smoke-test the native extractor against a user-owned decrypted EOU1 ROM and inspect the produced PNG set.
- [ ] Smoke-test the native extractor against a user-owned decrypted EO2U ROM and inspect the produced PNG set.
- [ ] Implement any additional container support only when a real extraction shows that missing support is preventing textures from being recovered.

### Optional regression tooling

- [x] Port the CityHash64 variant used by the legacy/Azahar candidate-hash path.
- [x] Emit privacy-safe schema-1 structural fingerprints.
- [x] Keep the native fingerprint comparison CLI available for developer diagnostics.
- [x] Preserve the frozen Python regression suite in CI while the old implementation remains in the repository.

Fingerprint mismatches may be useful debugging signals, but they do not override a correct native extraction result and they do not gate 0.60 by themselves.

### Quality and legal boundary

- [ ] Pass Rust formatting, Clippy with warnings denied, and the full workspace tests on Ubuntu and Windows at the final 0.60 head.
- [ ] Keep the frozen Python regression matrix green on Ubuntu/Windows and Python 3.10/3.12 at the final 0.60 head.
- [x] Use synthetic fixtures only in the repository.
- [x] Do not add Nintendo keys, ROM data, firmware, decoded game images, local source paths, or proprietary model/texture names to committed test evidence.

## Native extraction workflow

During development, the extractor can be run directly from the workspace:

```text
cargo run -p eo-untold --bin eo-texrip -- extract <decrypted-rom> --output <directory>
```

After building the binary, the user-facing form is:

```text
eo-texrip.exe extract <decrypted-rom> --output <directory>
```

If `--output` is omitted, EO-TexRip creates `<rom-name>-textures` beside the ROM. The output contains category folders of PNG files plus `extraction-report.json`.

See `docs/NATIVE_EXTRACTION.md` for the current usage and supported-input boundary.

## Remaining verification boundary

The remaining 0.60 verification is practical extraction, not legacy fingerprint equality. EOU1 and EO2U should each be run through the native extractor using user-owned decrypted inputs, the produced PNGs should be inspectable, and any warnings that correspond to genuinely missing textures should drive the next parser work.

CTPK, CTXB, and CMB remain intentionally unsupported until real extraction evidence shows they are needed for one of the Untold games. Their magic or filename extensions alone are not sufficient reason to add speculative parsers.

## 0.60 rule

Prefer correct, bounded native extraction over behavioral imitation of the legacy app. Do not tune parsers to opaque expected numbers. Every format change must be explained by file structure or a reproducible extraction failure and must receive synthetic regression coverage. Unknown data stays unknown until there is enough evidence to support it safely.
