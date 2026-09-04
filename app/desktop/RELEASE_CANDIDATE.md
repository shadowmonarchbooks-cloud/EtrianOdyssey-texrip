# EO-TexRip 0.60.0-rc.1

This is the first Windows desktop release candidate for the native Rust extractor.

## What this RC does

- Opens user-owned decrypted/cleartext Etrian Odyssey Untold or Etrian Odyssey 2 Untold ROMs.
- Lets you choose the ROM and output folder from a desktop GUI; ROM files can also be dragged onto the window.
- Keeps the GUI responsive while extraction runs in the background.
- Uses the native Rust NCSD/CIA/NCCH/RomFS, archive, container, and PICA200 paths.
- Writes decoded textures as ordinary PNG files arranged in coarse category folders.
- Shows the extracted texture count and warnings after completion and can open the output folder in Explorer.
- Writes `extraction-report.json` with extraction details and warnings.
- Requires no Python, pip, Git, Rust toolchain, or 3DS Texture Forge on the end-user machine.

## How to use it

1. Unzip the release archive.
2. Run `EO-TexRip.exe`.
3. Browse for a decrypted EOU1 or EO2U ROM, or drag the ROM onto the app window.
4. Choose the output folder or keep the suggested folder beside the ROM.
5. Click **Extract Textures**.
6. Use **Open Output Folder** when extraction finishes.

## Current boundaries

- Windows x64 is the packaged RC target.
- Inputs must already be decrypted/cleartext. EO-TexRip does not include, download, or obtain Nintendo keys.
- EOU1 and EO2U are the intended 0.60 game targets.
- CTPK, CTXB, and CMB are recognized but not yet claimed as supported. Real extraction reports will determine whether any of them need to be implemented for Untold coverage.

## What to send when reporting a problem

Do not upload ROMs or extracted game assets. `extraction-report.json` is sufficient for most parser/extraction diagnostics.
