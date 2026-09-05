# EO-TexRip 0.60.0-rc.5

This is the fifth Windows desktop release candidate for the native Rust extractor. RC5 fixes the single EOU1 structural coverage gap exposed by the RC4 coverage audit. EO2U's RC4 audit already passed cleanly and does not require another coverage run for this change.

## What changed since RC4

- Production strict scanning now recognizes structurally valid embedded CGFX payloads in already-selected files, instead of requiring CGFX to begin at byte 0 or appear through the ATBC special case.
- The scanner continues to validate the CGFX header/endian marker and declared extent; it does not fall back to unvalidated raw `CGFX` magic matching.
- Adds a regression test for an extension-selected `.bcmdl` containing wrapped/embedded CGFX evidence.
- RC4's independent EOU1 audit found 504 valid CGFX payloads with 1,659 ordinary image TXOBs, while production inventoried 503 payloads and 1,657 texture descriptors. This change targets exactly that one-payload/two-descriptor gap.
- RC4's EO2U audit passed: all 1,165 declared BCH texture entries resolved and parsed, all 6,148 STEX files parsed, and no cube-map, unsupported-container, or archive-order coverage gaps were reported.
- No EO2U extraction behavior is intentionally changed.
- No texture decoder, PNG encoder, native texture hash, Azahar `pack.json` format, or output naming rule is intentionally changed. EOU1 may now emit up to two additional deduplicated outputs because two previously skipped raw CGFX descriptors will be scanned; the final after-dedupe count is not assumed in advance.

## Why RC5 exists

RC3 removed all apparent missing-material-reference diagnostics, but that did not prove physical texture completeness. RC4 added an independent structural auditor. The first real RC4 audit proved EO2U complete and found one narrow EOU1 discrepancy: one valid CGFX payload with two ordinary image textures was visible to the auditor but never reached the production strict scan gate.

The production parser could already parse embedded CGFX once `scan_payload` ran. RC5 changes only the gate deciding whether an already-selected candidate reaches that parser.

## Run the normal extractor

1. Unzip the release archive.
2. Run `EO-TexRip.exe`.
3. Browse for a user-owned decrypted/cleartext EOU1 ROM, or drag the ROM onto the app window.
4. Choose the output folder or keep the suggested folder beside the ROM.
5. Click **Extract Textures**.
6. Keep the generated `extraction-report.json` for the final comparison.

EO2U does not need to be re-extracted solely for the RC5 coverage fix.

## Run the coverage audit

Open Command Prompt or PowerShell in the unzipped RC5 folder and run:

`EO-TexRip-Coverage-Audit.exe "C:\path\to\your\decrypted-eou1-rom.3ds"`

The auditor is read-only with respect to game assets. It writes a JSON report beside the ROM named like `<rom-name>-coverage-audit.json` unless a second output path is supplied.

For RC5, rerun the coverage audit for **EOU1 only**. The expected structural pass is:

- `coverage_complete: true`;
- an empty `audit_issues` array;
- production and independent CGFX payload counts both 504;
- production `texture_descriptors_found` / `decoded_3d_textures` accounting for the two previously skipped descriptors if both decode successfully;
- 1,659 independently declared image TXOBs and 1,659 successfully parsed image textures;
- zero cube/unknown CGFX texture objects;
- zero unsupported texture containers;
- zero cross-family archive candidates.

Do not assume the final exported EOU1 texture count will be 2,281: pixel-identical textures can deduplicate, so the after-dedupe increase may be zero, one, or two even when both raw descriptors are now decoded.

## Azahar custom textures

1. In Azahar, open the game's **Custom Texture Location**.
2. Copy the **contents** of the EO-TexRip extraction output directory into that location. Keep `pack.json` at the root and the category folders beside it.
3. Enable **Use Custom Textures** in Azahar.

## Current boundaries

- Windows x64 is the packaged RC target.
- Inputs must already be decrypted/cleartext. EO-TexRip does not include, download, or obtain Nintendo keys.
- EOU1 and EO2U are the intended 0.60 game targets.
- The coverage auditor is deliberately diagnostic: it does not write decoded textures or proprietary game data.
- Material-name ambiguities remain a separate resolver-quality issue. They do not by themselves indicate missing physical texture data.

## What to send for the final EOU1 coverage check

Do not upload ROMs or extracted game assets. Send only:

- the RC5 EOU1 `*-coverage-audit.json`;
- the RC5 EOU1 `extraction-report.json`.

If the EOU1 coverage report is clean and the normal extraction report introduces no new decode/missing/unsupported failure, physical texture completeness will be established for both Untold titles. The remaining product-level 0.60 gate is then the visible Azahar custom-texture replacement smoke test.
