# Etrian Odyssey HD Texture Extractor 0.12

A streamlined offline extractor and Azahar custom-texture workspace builder for the Nintendo 3DS **Etrian Odyssey Untold** games.

## Supported games

| Game | Profile | Main 3D path | Status |
|---|---|---|---|
| Etrian Odyssey Untold: The Millennium Girl | `eou1` | ATBC → CGFX/BCMDL → CMDL/MTOB/TXOB | verified against actual EOU1 files |
| Etrian Odyssey 2 Untold: The Fafnir Knight | `eo2u` | HPI/HPB → ATBC/BAM2 → BCH/H3D + external STEX | active; 0.10 fixed the real BAM2 discovery path |

The ROM is identified automatically from its Title ID and product code.

## 0.12: human-readable Azahar pack names

0.12 uses Azahar's `pack.json` hash mapping so the editable/deployment PNGs no longer need canonical `tex1_*` filenames. The runtime CityHash64 stays in `pack.json`, while the image itself gets a ROM-derived readable name.

Example:

```text
monsters/
├── monkey.png
├── monkey-alpha.png
└── monkey-specular.png
```

```json
"textures": {
  "4F365F6A8A6D6BFA": "monkey.png",
  "DB199FCCAAE59B30": "monkey-alpha.png"
}
```

Names come from real STEX/CGFX/BCH texture names, model metadata, or source filenames. The extractor does not invent English names when the ROM only provides an internal identifier. Basenames are globally unique because Azahar's mapping lookup is filename-based; collisions receive a short stable hash suffix.

A separate `-alpha` filename is only assigned when the ROM/material metadata supports a separate mask/alpha texture. Alpha embedded in the main RGBA/ETC1A4 texture stays inside the main PNG.

If one asset has multiple verified runtime hashes, all of those hashes map to **one physical PNG**, reducing pack duplication. Existing edited/upscaled 0.11 canonical masters are migrated into their new readable names on the next extraction.

### Effect-resource support retained

The 0.11 EPL resource-package stage remains enabled for EOU/EO2U effects. EPL members are structurally unpacked and fed into the existing STEX/CGFX/BCH/ATBC/CTPK/CTXB decoders.

## 3D material alpha

No grayscale/image-content heuristic is used. CGFX and BCH materials are interpreted from their actual texture slots and PICA TexEnv alpha inputs, including Texture0/1/2, Alpha/Red/Green/Blue operands, inversions, alpha test, and constant alpha behavior for RGB/ETC1 formats.

## Streamlined workspace

After a successful run, large extraction intermediates are removed. The persistent workspace stays:

```text
Workspace/
├── azahar_pack_master/
├── azahar_pack/
└── .eouhd/
    ├── manifest.json
    ├── reports/
    └── diagnostics/
```

`azahar_pack_master` is the editable/upscaling source of truth. `azahar_pack` is the deployment copy.

## Diagnostics

The full extracted RomFS is still deleted. Diagnostics are capped at **6 files / 32 MiB total** and are retained only when they are useful, including:

- failed STEX files;
- BAM/BAM2 BCH parsing failures;
- EO2U BAM2+BCH resources that never reach BCH parsing;
- failed EPL packages;
- one EPL investigation sample if packages exist but expose no recognized texture/model members.

## Azahar hashes, names, and folders

0.12 keeps:

```text
CityHash64
use_new_hash = true
```

The PNG filenames are human-readable and `pack.json` maps runtime hashes to them. `pack.json` remains at the title-ID root. Azahar recursively scans category folders below `load/textures/<TITLE_ID>/`.

Do not casually rename a PNG without updating the corresponding `pack.json` value.

## EO2U base title IDs

- Japan: `0004000000120500`
- USA: `000400000015F200`
- Europe/Australia: `000400000016E900`
