# Native texture extraction

EO-TexRip 0.60 is a native Rust desktop extraction path for **Etrian Odyssey Untold: The Millennium Girl** and **Etrian Odyssey 2 Untold: The Fafnir Knight**.

The application accepts a user-owned decrypted/cleartext Nintendo 3DS ROM container and writes decoded texture PNGs directly. The Windows release candidate does not require Python, pip, Git, a Rust toolchain, or 3DS Texture Forge on the user's machine.

## Windows desktop app

The normal 0.60 workflow is GUI-first:

1. Download and unzip the Windows x64 release candidate.
2. Run `EO-TexRip.exe`.
3. Choose a decrypted EOU1 or EO2U ROM, or drag the ROM onto the window.
4. Choose the output folder. EO-TexRip suggests a `<rom-name>-textures` folder beside the ROM by default.
5. Click **Extract Textures**.
6. Review the texture count and any warnings in the app. **Open Output Folder** opens the result in Explorer.

The extraction itself runs on a worker thread so the desktop window remains responsive while the ROM is scanned and textures are decoded.

## Output

Successful textures are written as RGBA PNG files under coarse category folders such as:

```text
EOU-textures/
├── characters/
├── monsters/
├── ui/
├── icons/
├── maps/
├── dungeon/
├── backgrounds/
├── effects/
├── fonts/
├── misc/
├── pack.json
└── extraction-report.json
```

Filenames prefer the texture's internal resource name when one exists. A dimension/format/hash suffix keeps names deterministic and collision-resistant, and filename components are sanitized for Windows.

`extraction-report.json` records the detected game profile, Title ID/product code when available, every written PNG, dimensions, parser provenance, candidate hash, material-binding count, and any warnings encountered while scanning the ROM.

`pack.json` is generated automatically for Azahar custom-texture loading. It maps the exact CityHash64 new-hash value of each extracted encoded level-0 texture to the PNG basename, while allowing EO-TexRip to keep the files organized in category subdirectories.

## Azahar custom texture compatibility

EO-TexRip emits Azahar's current `pack.json` format with:

- `use_new_hash: true`, matching Azahar's CityHash64 hash of the encoded guest texture upload bytes;
- `skip_mipmap: true`, because EO-TexRip exports the base texture and lets Azahar generate replacement mipmaps;
- `flip_png_files: true`, matching Azahar's ordinary PNG texture-pack convention;
- one `textures` mapping for every extracted texture hash.

To use the output in Azahar:

1. In Azahar, open the game's **Custom Texture Location**.
2. Copy the **contents** of the EO-TexRip output directory into that game's texture directory. Keep `pack.json` at the root and keep the category folders beside it.
3. Enable **Use Custom Textures** in Azahar.

The detected Title ID is also recorded in `extraction-report.json`, which helps identify the correct game-specific Azahar texture directory when needed.

## Supported native paths

The current Untold pipeline includes:

- native NCSD/CCI, CIA, NCCH/CXI and RomFS reading for cleartext inputs;
- HPI/HPB archive extraction;
- FARC archive extraction;
- EPL resource-package extraction;
- STEX textures;
- CGFX/BCMDL textures, including ATBC-wrapped payloads;
- BCH/H3D textures, including BAM/BAM2/ATBC-wrapped payloads;
- native PICA200 decoding to tightly packed RGBA8;
- native PNG export;
- Azahar new-hash `pack.json` export.

## Developer CLI

A small CLI remains available for automated investigation and development, but it is not the end-user workflow:

```powershell
cargo run -p eo-extract --bin eo-texrip-cli -- extract "D:\ROMs\EOU.3ds" --output "D:\Extracted\EOU"
```

End users should use `EO-TexRip.exe` from the packaged release candidate instead.

## Known boundary

CTPK, CTXB, and CMB are recognized as possible container candidates but are not yet claimed as supported. If an EOU1 or EO2U extraction report shows one of these is responsible for missing textures, that real failure becomes the evidence for implementing the corresponding native parser.

Encrypted content is intentionally rejected rather than guessed. EO-TexRip does not ship, download, or acquire Nintendo keys.

## What counts as 0.60 success

For 0.60, the important test is practical: the packaged desktop app should accept a decrypted EOU1 or EO2U ROM and produce useful PNG texture output plus an Azahar-compatible hash map, with understandable warnings for anything not decoded. Legacy Python fingerprint equality is optional diagnostic tooling, not a user requirement and not a release gate by itself.
