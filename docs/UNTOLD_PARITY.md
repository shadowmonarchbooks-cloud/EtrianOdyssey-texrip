# 0.60 Untold Native Extraction

0.60 composes the native Rust ROM, archive, texture, and model layers into the first end-to-end Etrian Odyssey Untold / Etrian Odyssey 2 Untold extraction path and packages that path as a Windows desktop application.

The product gate is practical native extraction: a user-owned decrypted/cleartext EOU1 or EO2U ROM should produce useful PNG texture output through the GUI without Python or 3DS Texture Forge. The frozen Python 0.13 implementation and schema-1 fingerprints remain optional developer regression tools; exact legacy equality is not a release gate.

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
- [x] Route direct and ATBC-wrapped CGFX through native texture and material inspection.
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
- [x] Add a Windows x64 release-candidate packaging workflow producing `EO-TexRip.exe`, a ZIP archive, and SHA-256 checksum.

### Practical 0.60 gate

- [x] Publish the Windows `0.60.0-rc.1` prerelease from a green Windows package workflow.
- [x] Smoke-test the packaged RC with a user-owned decrypted EOU1 ROM and inspect PNG output/report.
- [ ] Publish `0.60.0-rc.2` with automatic Azahar `pack.json` generation.
- [ ] Verify RC2's generated EOU1 pack loads through Azahar's custom-texture path.
- [ ] Smoke-test the packaged RC with a user-owned decrypted EO2U ROM and inspect PNG output/report.
- [ ] Implement CTPK/CTXB/CMB or another format only if a real extraction report shows it is preventing useful Untold textures from being recovered.

### Quality and legal boundary

- [ ] Pass Rust formatting, Clippy with warnings denied, and the full workspace tests on Ubuntu and Windows at the final 0.60 head.
- [ ] Build and package the Windows desktop executable successfully in CI.
- [ ] Keep the frozen Python regression matrix green while the legacy implementation remains in the repository.
- [x] Use synthetic fixtures only in the repository.
- [x] Do not add Nintendo keys, ROM data, firmware, decoded game images, local source paths, or proprietary model/texture fixtures to committed evidence.

## Optional legacy regression tooling

The native fingerprint CLI and frozen Python schema-1 fingerprint remain available for developer investigation. They can help explain regressions but do not define correctness when the native implementation intentionally improves on legacy behavior.

No user needs to create Python workspaces or fingerprint JSON files to use the desktop application.

## 0.60 rule

Prioritize correct native extraction and useful user-facing output over reproducing legacy quirks. Keep unknown or unsupported binary structures explicit, add parsers only from structural evidence, and never ship or acquire Nintendo keys or proprietary game data.
