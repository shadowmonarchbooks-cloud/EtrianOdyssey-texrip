# Etrian Odyssey HD Texture Extractor 0.13

`0.13.0` is the **Legacy Final** milestone: the current Python extractor is being frozen and hardened as the behavioral reference for the independent EO-TexRip application described in [`docs/ROADMAP.md`](docs/ROADMAP.md).

The 0.13 app is still a streamlined extractor and Azahar custom-texture workspace builder for the Nintendo 3DS **Etrian Odyssey Untold** games. Broader 3DS Etrian Odyssey support and removal of Python/3DS Texture Forge are later roadmap milestones, not claims of this legacy build.

## Supported reference games

| Game | Profile | Main 3D path | Status |
|---|---|---|---|
| Etrian Odyssey Untold: The Millennium Girl | `eou1` | ATBC → CGFX/BCMDL → CMDL/MTOB/TXOB | verified against actual EOU1 files |
| Etrian Odyssey 2 Untold: The Fafnir Knight | `eo2u` | HPI/HPB → ATBC/BAM2 → BCH/H3D + external STEX | active; verified structural path |

The ROM is identified automatically from its Title ID and product code.

## What 0.13 hardens

- BCH/PICA texture storage sizing now uses the correct 8×8 tile-aligned base-level span, including non-8-aligned dimensions.
- HPI/HPB and selected RomFS paths share cross-platform containment rules for traversal, absolute/drive paths, Windows device names, ADS syntax, and unsafe path components.
- Recursive HPI/FARC/EPL expansion shares limits for nesting depth, extracted file count, expanded bytes, per-member size, and archive input size.
- `azahar_pack_master` is recoverable user state even when `.eouhd/manifest.json` is missing or corrupt.
- Intentional PNG renames recorded in live `pack.json` mappings survive reruns.
- Master and deployment packs are built in staging, validated, and promoted transactionally with rollback until the replacement succeeds.
- Destructive legacy cleanup requires an EO-TexRip workspace marker.
- Exact RGBA runtime-hash evidence can verify automatically; perceptual/upscaled matches remain candidates until explicitly confirmed.
- Retained 3D material reports no longer point at transient files after streamlined cleanup.
- Reconstructed material alpha is explicitly diagnostic and is not presented as an exact full-GPU rendering reconstruction.
- Copyright-safe structural fingerprints can be generated for local legal-ROM regression validation.
- Windows/Linux Python 3.10/3.12 regression CI is part of the repository. Requiring that CI through `main` branch protection/rulesets is an external repository-admin follow-up documented in the roadmap.

## Human-readable Azahar pack names

Azahar's `pack.json` hash mapping lets editable/deployment PNGs use ROM-derived readable filenames instead of canonical `tex1_*` names. Runtime CityHash64 values remain in `pack.json`.

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

A separate `-alpha` filename is only assigned when ROM/material metadata supports a separate mask/alpha texture. Alpha embedded in the main RGBA/ETC1A4 texture stays inside the main PNG.

If one asset has multiple verified runtime hashes, all verified hashes map to **one physical PNG**. Existing edited/upscaled canonical masters are migrated/preserved when possible.

### Intentional renames

If you intentionally rename a master PNG, update the corresponding `pack.json` value. 0.13 reads the live mapping on the next rerun and preserves that renamed master rather than relying only on the previous manifest.

## Effect-resource support

The EPL resource-package stage remains enabled for EOU/EO2U effects. EPL members are structurally unpacked and fed into the existing STEX/CGFX/BCH/ATBC/CTPK/CTXB decoders.

## 3D material alpha

No grayscale/image-content heuristic is used to decide material texture roles. CGFX and BCH materials are interpreted from their actual texture slots and PICA TexEnv alpha inputs, including Texture0/1/2, Alpha/Red/Green/Blue operands, inversions, alpha test, and constant alpha behavior for RGB/ETC1 formats.

Stored alpha/channel extraction is structural. Any reconstructed `resolved_material_alpha` is a **diagnostic scalar reconstruction**, not a guarantee of exact rendered appearance: UV transforms, filtering/wrapping, and other GPU state are not fully modeled by the legacy exporter.

## Streamlined workspace

After a successful run, large extraction intermediates are removed. The persistent workspace is:

```text
Workspace/
├── azahar_pack_master/
├── azahar_pack/
└── .eouhd/
    ├── workspace.json
    ├── manifest.json
    ├── reports/
    └── diagnostics/
```

`azahar_pack_master` is the editable/upscaling source of truth. `azahar_pack` is the generated deployment copy.

A rerun does not delete the previous deployment pack before its replacement has been successfully staged and validated.

## Extraction budgets

Defaults are deliberately generous for legitimate 3DS data but finite. They can be overridden for local investigation with:

- `EO_TEXRIP_MAX_ARCHIVE_DEPTH`
- `EO_TEXRIP_MAX_EXTRACTED_FILES`
- `EO_TEXRIP_MAX_EXPANDED_BYTES`
- `EO_TEXRIP_MAX_MEMBER_BYTES`
- `EO_TEXRIP_MAX_ARCHIVE_BYTES`

Malformed archives that would exceed the active budget are rejected before the corresponding member allocation/write.

## Runtime hash evidence

`import-hashes` now distinguishes confidence:

- exact decoded-RGBA match → `verified_hashes`;
- perceptual/downsample match against an HD/upscaled image → `runtime_hash_candidates` only.

A candidate can be promoted explicitly with:

```text
python eouhd_cli.py confirm-hash <workspace> <asset_id> <runtime_hash>
```

## Copyright-safe regression fingerprints

After running a legal local dump, generate a structural fingerprint with:

```text
python eouhd_cli.py fingerprint <workspace>
```

or:

```text
python tools/fingerprint_workspace.py <workspace> --write expected.json
```

Compare a later extraction with:

```text
python eouhd_cli.py fingerprint <workspace> --compare expected.json
```

Fingerprints contain structural counts, formats, dimensions, runtime/candidate hashes, and an aggregate descriptor SHA-256. They do **not** contain ROM bytes, decoded texture pixels, source paths, or embedded texture/model names.

## Diagnostics

The full extracted RomFS is deleted after a successful run. Failure diagnostics remain capped at **6 files / 32 MiB total** and are retained only when useful, including failed STEX files, BCH parsing failures, EO2U BAM2+BCH investigation samples, and failed/unknown EPL cases.

## Azahar hashes

The legacy reference continues to use:

```text
CityHash64
use_new_hash = true
```

The PNG filenames are human-readable and `pack.json` maps runtime hashes to them. `pack.json` remains at the title-ID root. Azahar recursively scans category folders below `load/textures/<TITLE_ID>/`.

## EO2U base title IDs

- Japan: `0004000000120500`
- USA: `000400000015F200`
- Europe/Australia: `000400000016E900`

## Independent application roadmap

The next milestones move the project to a self-contained Rust desktop application, then replace the external ROM reader and PICA decoder, then expand game profiles across the full Nintendo 3DS Etrian Odyssey catalog. See [`docs/ROADMAP.md`](docs/ROADMAP.md).
