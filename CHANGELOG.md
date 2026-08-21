# Changelog

## 0.12.0

- Replaced persistent canonical `tex1_*` PNG names with human-readable ROM-derived filenames mapped through Azahar `pack.json`.
- Friendly names prefer embedded texture names, then model metadata/source filenames; no invented English names are used.
- Added conservative `-alpha` naming for explicitly identified auxiliary alpha/mask textures; embedded alpha remains in the primary image.
- Enforced globally unique basenames because Azahar resolves `pack.json` mappings by filename. Name collisions receive a stable short hash suffix.
- Multiple verified/candidate hashes for one deduplicated asset now map to a single physical PNG instead of producing image aliases.
- Added automatic migration/preservation of edited 0.11 canonical masters when rebuilding into the 0.12 friendly-name layout.
- Retained the EOU/EO2U EPL effect-resource stage, CityHash64 `use_new_hash=true`, and streamlined two-pack workspace.

## 0.11.0

- Added a conservative **Atlus EPL general-resource-package** extraction layer for EOU/EO2U effects.
- EPL parsing follows the public AtlusLibSharp/Amicitia layout: resource count/table pointer at `0x80`, fixed resource records, descriptor pointers, and bounded member payload offsets/sizes.
- EPL members are written to the transient workspace and then re-enter the existing strict texture/model pipeline. Known `STEX`, `CGFX`, `BCH`, `ATBC`, `CTPK`, and `CTXB` members can therefore be decoded without broad binary guessing.
- Added `epl_inventory.json` with archive/member counts, signature summaries, parse errors, and small member samples.
- Added EPL-aware diagnostics. If EPL parsing fails, or EPL packages exist but expose zero recognized texture/model members, a tiny representative EPL sample can survive cleanup under `.eouhd/diagnostics/`.
- Confirmed from a real EOU1 0.10 report that all 1,291 standalone STEX files decode with zero STEX errors; the remaining missing effects are therefore not caused by standalone STEX decoding.
- Kept the streamlined two-pack workspace, CityHash64 `use_new_hash=true`, EOU1 ATBC/CGFX path, and EO2U BAM2/BCH path unchanged.

## 0.10.0

- Fixed EO2U ATBC/BAM2 resources containing BCH but no CGFX being rejected before BCH parsing.
- BCH validation is version-aware and permits model-only BCH resources with external STEX textures.
- Relaxed EO effect STEX handling when the declared byte count overshoots EOF but a complete base image payload is present.
- Added capped failure-only diagnostics under `.eouhd/diagnostics/`.

## 0.9.0

- Added multi-game profiles for EOU1 and EO2U.
