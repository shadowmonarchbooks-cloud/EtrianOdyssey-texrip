# EO-TexRip 0.60.0-rc.3

This is the third Windows desktop release candidate for the native Rust extractor. RC3 is intended for a fresh EOU1/EO2U smoke-test batch after tightening material-reference diagnostics.

## What changed since RC2

- Preserves every internal texture name attached to pixel-identical assets when native deduplication collapses them to one exported texture.
- Uses those retained aliases during ROM-wide material-reference reconciliation, preventing false `*_material_texture_missing` warnings caused only by dedupe.
- Ignores disabled H3D texture slots when deciding which EO2U BCH material references are actually required.
- Keeps the existing 0/1/multiple final classification for unresolved names: absent, uniquely resolved, or ambiguous across decoded assets.
- Does not intentionally change decoded PNG pixels, native texture hashes, Azahar `pack.json` mapping, output naming, archive expansion, or supported texture/container formats.
- Makes the Windows RC workflow derive its version and artifact names from `app/desktop/RC_READY` instead of hard-coding a release-candidate number.

The previous real smoke tests produced 2,279 EOU1 textures and 2,884 EO2U textures. RC3 should be rerun against both games so any material-reference warnings that remain can be treated as evidence rather than known diagnostic artifacts.

## What this RC does

- Opens user-owned decrypted/cleartext Etrian Odyssey Untold or Etrian Odyssey 2 Untold ROMs.
- Lets you choose the ROM and output folder from a desktop GUI; ROM files can also be dragged onto the window.
- Keeps the GUI responsive while extraction runs in the background.
- Uses the native Rust NCSD/CIA/NCCH/RomFS, archive, container, and PICA200 paths.
- Writes decoded textures as ordinary PNG files arranged in coarse category folders.
- Writes `pack.json` for Azahar and `extraction-report.json` for diagnostics.
- Shows the extracted texture count and warnings after completion and can open the output folder in Explorer.
- Requires no Python, pip, Git, Rust toolchain, or 3DS Texture Forge on the end-user machine.

## RC3 smoke-test focus

Run one extraction for EOU1 and one for EO2U, then keep the resulting `extraction-report.json` files. We want to verify:

- EOU1 still extracts the expected texture set and whether its former `parts04_jijiku` warning survives corrected alias reconciliation.
- EO2U still extracts the expected texture set and which BCH material-reference warnings survive after disabled-slot filtering and alias preservation.
- There are still no `texture_decode`, ROM/archive, or unexpected unsupported-container failures.
- Any surviving enabled unresolved names can then be investigated against BCH material-animation or other runtime reference mechanisms before 0.60 is closed.

## How to use it

1. Unzip the release archive.
2. Run `EO-TexRip.exe`.
3. Browse for a decrypted EOU1 or EO2U ROM, or drag the ROM onto the app window.
4. Choose the output folder or keep the suggested folder beside the ROM.
5. Click **Extract Textures**.
6. Use **Open Output Folder** when extraction finishes.

## Azahar custom textures

1. In Azahar, open the game's **Custom Texture Location**.
2. Copy the **contents** of the EO-TexRip output directory into that location. Keep `pack.json` at the root and the category folders beside it.
3. Enable **Use Custom Textures** in Azahar.

## Current boundaries

- Windows x64 is the packaged RC target.
- Inputs must already be decrypted/cleartext. EO-TexRip does not include, download, or obtain Nintendo keys.
- EOU1 and EO2U are the intended 0.60 game targets.
- CTPK, CTXB, and CMB are recognized but not yet claimed as supported. Real extraction reports will determine whether any of them need to be implemented for Untold coverage.
- RC3 does not yet add BCH material-animation parsing; it is designed to reveal which enabled unresolved references, if any, still require that deeper investigation.

## What to send when reporting a problem

Do not upload ROMs or extracted game assets. For this smoke-test batch, send the EOU1 and EO2U `extraction-report.json` files. For Azahar mapping problems, also include `pack.json` and a short relevant Azahar log excerpt.
