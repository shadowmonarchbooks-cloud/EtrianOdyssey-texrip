# EO-TexRip 0.60.0-rc.2

This is the second Windows desktop release candidate for the native Rust extractor.

## What changed since RC1

- Automatically writes `pack.json` for Azahar custom-texture loading.
- Maps each extracted texture's Azahar new-hash CityHash64 value to the exported PNG basename.
- Keeps EO-TexRip's category folders while allowing Azahar to resolve the mapped PNGs recursively.
- Emits `use_new_hash: true`, `skip_mipmap: true`, and `flip_png_files: true` in the generated pack configuration.
- RC1's real EOU1 smoke test produced 2,279 textures with one reported material-reference warning, so RC2 keeps the same extraction pipeline and fixes the missing Azahar compatibility artifact.

## What this RC does

- Opens user-owned decrypted/cleartext Etrian Odyssey Untold or Etrian Odyssey 2 Untold ROMs.
- Lets you choose the ROM and output folder from a desktop GUI; ROM files can also be dragged onto the window.
- Keeps the GUI responsive while extraction runs in the background.
- Uses the native Rust NCSD/CIA/NCCH/RomFS, archive, container, and PICA200 paths.
- Writes decoded textures as ordinary PNG files arranged in coarse category folders.
- Writes `pack.json` for Azahar and `extraction-report.json` for diagnostics.
- Shows the extracted texture count and warnings after completion and can open the output folder in Explorer.
- Requires no Python, pip, Git, Rust toolchain, or 3DS Texture Forge on the end-user machine.

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

## What to send when reporting a problem

Do not upload ROMs or extracted game assets. For extraction problems, send `extraction-report.json`. For Azahar mapping problems, also include `pack.json` and a short relevant Azahar log excerpt.
