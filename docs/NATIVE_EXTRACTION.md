# Native texture extraction

EO-TexRip 0.60 includes a native Rust extraction path for **Etrian Odyssey Untold: The Millennium Girl** and **Etrian Odyssey 2 Untold: The Fafnir Knight**.

The extractor is intended to accept a user-owned decrypted/cleartext Nintendo 3DS ROM container and write decoded texture PNGs directly. It does not require Python, pip, 3DS Texture Forge, Nintendo keys, or a legacy EO-TexRip workspace.

## Current command

From a development checkout:

```powershell
cargo run -p eo-untold --bin eo-texrip -- extract "D:\ROMs\EOU.3ds" --output "D:\Extracted\EOU"
```

After building the binary:

```powershell
.\target\debug\eo-texrip.exe extract "D:\ROMs\EOU.3ds" --output "D:\Extracted\EOU"
```

The output directory is optional. Without it:

```powershell
.\target\debug\eo-texrip.exe extract "D:\ROMs\EOU.3ds"
```

EO-TexRip writes beside the source ROM using a directory such as `EOU-textures`.

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
└── extraction-report.json
```

Filenames prefer the texture's internal resource name when one exists. A dimension/format/hash suffix keeps names deterministic and collision-resistant, and filename components are sanitized for Windows.

`extraction-report.json` records the detected game profile, Title ID/product code when available, every written PNG, dimensions, parser provenance, candidate hash, material-binding count, and any warnings encountered while scanning the ROM.

## Supported native paths

The current Untold pipeline includes:

- native NCSD/CCI, CIA, NCCH/CXI and RomFS reading for cleartext inputs;
- HPI/HPB archive extraction;
- FARC archive extraction;
- EPL resource-package extraction;
- STEX textures;
- CGFX/BCMDL textures, including ATBC-wrapped payloads;
- BCH/H3D textures, including BAM/BAM2/ATBC-wrapped payloads;
- native PICA200 decoding to tightly packed RGBA8.

## Known boundary

CTPK, CTXB, and CMB are recognized as possible container candidates but are not yet claimed as supported. If an EOU1 or EO2U extraction report shows one of these is responsible for missing textures, that real failure becomes the evidence for implementing the corresponding native parser.

Encrypted content is intentionally rejected rather than guessed. EO-TexRip does not ship, download, or acquire Nintendo keys.

## What counts as 0.60 success

For 0.60, the important test is practical: a decrypted EOU1 or EO2U ROM should produce useful PNG texture output with understandable warnings for anything not decoded. Legacy Python fingerprint equality is optional diagnostic tooling, not a user requirement and not a release gate by itself.
