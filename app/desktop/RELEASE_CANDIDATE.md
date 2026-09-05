# EO-TexRip 0.60.0-rc.4

This is the fourth Windows desktop release candidate for the native Rust extractor. RC4 keeps the RC3 extraction behavior and adds an independent read-only coverage auditor so the final Untold completeness claim can be based on declared texture structures, not only successful extraction counts.

## What changed since RC3

- Adds `EO-TexRip-Coverage-Audit.exe`, a read-only audit executable packaged beside the desktop GUI.
- The auditor runs the production native inventory and then independently rescans the same decrypted ROM for structural coverage.
- CGFX coverage is checked against the top-level DATA texture dictionary rather than trusting only successfully parsed image TXOB objects.
- CGFX texture object types are classified explicitly, including image (`0x20000011`), cube (`0x20000009`), reference, procedural, shadow, and unknown TXOB types.
- BCH/H3D coverage compares the declared texture-section count, resolvable descriptor pointers, and successfully parsed descriptors.
- BCH texture command streams are checked for additional cube-map face addresses (`GPUREG_TEXUNIT0_ADDR2` through `ADDR6`).
- Archive expansion is independently repeated to a fixed point and reports cross-family nesting that the production HPI -> FARC -> EPL stage order would not revisit.
- The auditor uses a broader embedded-CGFX discovery probe and reports a valid payload found in a top-level RomFS file that the production selector would not read.
- Known unsupported texture-capable CTPK, CTXB, and CMB candidates are explicit audit failures rather than silently ignored coverage.
- No decoded PNG, native texture hash, Azahar `pack.json`, output naming, or normal extraction behavior is intentionally changed from RC3.

## What RC3 established

Fresh real smoke tests remained stable at 2,279 EOU1 textures and 2,884 EO2U textures. The former missing-material diagnostics disappeared: EOU1 retained one name ambiguity and EO2U retained 35 name ambiguities, but neither report contained a material texture that was referenced and absent from all decoded assets. Neither report showed `texture_decode` or unsupported-container failures.

Those results are strong evidence, but they do not by themselves prove every declared texture object was parsed. RC4 exists to close that remaining structural-audit gap.

## Run the normal extractor

1. Unzip the release archive.
2. Run `EO-TexRip.exe`.
3. Browse for a user-owned decrypted/cleartext EOU1 or EO2U ROM, or drag the ROM onto the app window.
4. Choose the output folder or keep the suggested folder beside the ROM.
5. Click **Extract Textures**.
6. Use **Open Output Folder** when extraction finishes.

## Run the coverage audit

Open Command Prompt or PowerShell in the unzipped RC4 folder and run:

`EO-TexRip-Coverage-Audit.exe "C:\path\to\your\decrypted-rom.3ds"`

The auditor does not extract game assets. It writes a JSON report beside the ROM named like `<rom-name>-coverage-audit.json` unless a second output path is supplied.

Run it once for EOU1 and once for EO2U. A clean structural result has `coverage_complete: true` and an empty `audit_issues` array. Important counters include:

- CGFX top-level textures declared vs image textures successfully parsed;
- CGFX cube/unknown texture object counts;
- BCH texture entries declared vs pointers resolved vs entries parsed;
- BCH cube-map texture/face counts;
- independently discovered CGFX/BCH payload counts vs the production extractor inventory;
- cross-family archive candidates;
- known unsupported texture-container candidates.

If either report is not clean, keep the report: the issue entries are designed to identify the exact source and coverage class that still needs engineering work.

## Azahar custom textures

1. In Azahar, open the game's **Custom Texture Location**.
2. Copy the **contents** of the EO-TexRip extraction output directory into that location. Keep `pack.json` at the root and the category folders beside it.
3. Enable **Use Custom Textures** in Azahar.

## Current boundaries

- Windows x64 is the packaged RC target.
- Inputs must already be decrypted/cleartext. EO-TexRip does not include, download, or obtain Nintendo keys.
- EOU1 and EO2U are the intended 0.60 game targets.
- The coverage auditor is deliberately diagnostic: it does not write decoded textures or proprietary game data.
- Material-animation texture-name tables remain relevant to runtime binding semantics, but physical texture completeness is determined here from declared texture objects/sections and archive/container reachability. Any surviving binding ambiguity remains a separate resolver-quality issue rather than evidence that pixel data is absent.

## What to send for the final coverage audit

Do not upload ROMs or extracted game assets. Send only the two generated `*-coverage-audit.json` files, one for EOU1 and one for EO2U. If we later test Azahar replacement behavior, `pack.json` and a short relevant Azahar log excerpt are also safe and useful.
