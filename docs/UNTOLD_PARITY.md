# 0.60 Untold Native Extraction

0.60 composes the native Rust ROM, archive, texture, and model layers into the first end-to-end Etrian Odyssey Untold / Etrian Odyssey 2 Untold extraction path and packages that path as a Windows desktop application.

The product gate is practical native extraction plus working Azahar replacement: a user-owned decrypted/cleartext EOU1 or EO2U ROM should produce useful PNG texture output through the GUI without Python or 3DS Texture Forge, every discovered/declared texture-bearing structure should be accounted for by an independent known-format audit, and a visibly modified exported texture should load through Azahar for both games.

The frozen Python 0.13 implementation and schema-1 fingerprints remain optional developer regression tools; exact legacy equality is not a release gate.

## Exit criteria

### Native orchestration

- [x] Add a dedicated `eo-untold` orchestration crate at workspace version `0.60.0`.
- [x] Require a verified EOU1 or EO2U identity before using the Untold pipeline.
- [x] Enumerate candidate RomFS files through the native `RomReader` contract.
- [x] Reject oversized candidate reads before requesting their payload bytes.
- [x] Pair HPI/HPB paths case-insensitively without relying on host filesystem behavior.
- [x] Expand HPI/HPB, FARC, and EPL through bounded native archive stages.
- [x] Keep extracted proprietary bytes out of committed artifacts.

### Native texture and model path

- [x] Route STEX through the native STEX adapter and native PICA decoder.
- [x] Route direct, wrapped, and structurally valid embedded CGFX through native texture and material inspection.
- [x] Route direct and BAM/ATBC-wrapped BCH through native texture and material inspection.
- [x] Deduplicate decoded assets by stable encoded-texture identity.
- [x] Merge material bindings through structural texture-name relationships rather than image heuristics.
- [x] Surface known CTPK/CTXB/CMB candidates as explicit unsupported-container warnings instead of silently claiming support.
- [x] Carry decoded RGBA8 through the native pipeline for export.

### User-facing extraction

- [x] Add a reusable Rust extraction/export API separate from the GUI and developer CLI.
- [x] Write decoded textures as ordinary RGBA PNG files without Python or an external image tool.
- [x] Use deterministic Windows-safe filenames and coarse category folders.
- [x] Write `extraction-report.json` with output mappings and extraction warnings.
- [x] Emit an Azahar-ready `pack.json` mapping each exact new-hash CityHash64 value to its exported PNG.
- [x] Add a Windows desktop GUI with ROM picker, output-folder picker, drag-and-drop input, extraction status, warning summary, and output-folder action.
- [x] Run extraction off the GUI thread so the window remains responsive.
- [x] Add a Windows x64 release-candidate packaging workflow producing `EO-TexRip.exe`, `EO-TexRip-Coverage-Audit.exe`, a ZIP archive, and SHA-256 checksum.

### Independent structural coverage audit

- [x] Add a read-only coverage auditor independent of the normal export path.
- [x] Account for every STEX payload discovered by the audit and require every discovered STEX to parse.
- [x] For CGFX, account from the top-level texture dictionary and distinguish image, cube, reference, procedural, shadow, and unknown TXOB types.
- [x] For BCH, compare raw declared texture entries, resolvable pointers, parsed textures, and possible cube-map face registers.
- [x] Independently expand HPI/HPB, FARC, and EPL to a fixed point and flag cross-family archive candidates production would not revisit.
- [x] Fail the audit on known unsupported texture-capable CTPK/CTXB/CMB containers.
- [x] EOU1 RC5 audit passes with `coverage_complete: true`, an empty `audit_issues` array, 504 production/independent CGFX payloads, 1,659 declared/parsed image TXOBs, 3,555/3,555 parsed STEX files, and zero unsupported/cube/unknown/archive-order gaps.
- [x] EO2U RC4 audit passes with `coverage_complete: true`, an empty `audit_issues` array, all 1,165 declared BCH texture entries resolved/parsed/decoded, all 6,148 STEX files parsed, and zero cube-map/unsupported/archive-order gaps.
- [x] No additional CTPK/CTXB/CMB or other known texture-container implementation is required for 0.60 based on the audited EOU1/EO2U inputs.

The structural conclusion is intentionally bounded: for both audited Untold titles, every texture-bearing structure discovered by the independent known-format audit is accounted for by production extraction. This is not a claim that arbitrary unknown future wrappers are mathematically impossible.

### Practical 0.60 gate

- [x] Publish Windows prereleases through `v0.60.0-rc.5` from green package workflows.
- [x] Smoke-test packaged extraction with a user-owned decrypted EOU1 ROM and inspect PNG output/report.
- [x] Smoke-test packaged extraction with a user-owned decrypted EO2U ROM and inspect PNG output/report.
- [x] Generate Azahar `pack.json` automatically from native runtime hashes.
- [x] Establish physical texture extraction completeness for EOU1 with the RC5 independent coverage audit.
- [x] Establish physical texture extraction completeness for EO2U with the RC4 independent coverage audit.
- [x] Verify one visibly modified EOU1 exported PNG is actually rendered by Azahar through the generated pack.
- [x] Verify one visibly modified EO2U exported PNG is actually rendered by Azahar through the generated pack.

Both Azahar replacement checks were confirmed against user-owned game inputs after the structural coverage audits passed.

### Quality and legal boundary

- [x] Pass Rust formatting, Clippy with warnings denied, and the full workspace tests on Ubuntu and Windows at the RC5 release head.
- [x] Build and package the Windows desktop executable and coverage auditor successfully in CI.
- [x] Keep the frozen Python regression matrix green on Ubuntu/Windows and Python 3.10/3.12 through the RC5 release head.
- [x] Use synthetic fixtures only in the repository.
- [x] Do not add Nintendo keys, ROM data, firmware, decoded game images, local source paths, or proprietary model/texture fixtures to committed evidence.

Any commits after the RC5 release head remain subject to the repository's normal CI before merge.

## Current release candidate

- `v0.60.0-rc.5`
- release commit: `ead7437bcee47c13d844dbaeabd1293a48a2161e`
- EOU1 normal extraction: 2,280 deduplicated outputs; the only remaining extraction diagnostic is the known `parts04_jijiku` exact-name ambiguity, which is a resolver ambiguity rather than missing pixel data.
- EO2U physical coverage was already established by the RC4 audit and was not intentionally changed by RC5.

## Final Azahar smoke procedure

For each Untold title independently:

1. Use the current EO-TexRip output including `pack.json` and the exported category folders.
2. Choose one exported PNG that can be reached predictably in-game and make an unmistakable visible edit while preserving its filename, relative path, dimensions, and PNG format.
3. Place the pack contents in Azahar's per-title custom-texture location.
4. Enable **Use custom textures**. Async custom texture loading may remain enabled; preloading is not required for the gate.
5. Launch the game directly in Azahar and reach the screen/model that uses the edited texture.
6. Confirm the edited image is visibly rendered in-game.
7. Repeat for the other Untold title.

A screenshot or short description of the visible replacement is sufficient project evidence; ROMs, keys, extracted asset sets, and proprietary game files must not be uploaded.

## Optional legacy regression tooling

The native fingerprint CLI and frozen Python schema-1 fingerprint remain available for developer investigation. They can help explain regressions but do not define correctness when the native implementation intentionally improves on legacy behavior.

No user needs to create Python workspaces or fingerprint JSON files to use the desktop application.

## 0.60 rule

Prioritize correct native extraction and useful user-facing output over reproducing legacy quirks. Keep unknown or unsupported binary structures explicit, add parsers only from structural evidence, and never ship or acquire Nintendo keys or proprietary game data.