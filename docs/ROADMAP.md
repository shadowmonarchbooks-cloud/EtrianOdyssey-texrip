# EO-TexRip independent application roadmap

EO-TexRip is being migrated from the current Python + 3DS Texture Forge workflow into a self-contained desktop application whose extraction, texture decoding, cataloguing, validation, and Azahar build pipeline are owned by this repository.

## Product target

The 1.0 application must:

- require no Python, pip, Git, Rust, Node.js, Java, or separately installed texture tools;
- perform no runtime dependency downloads;
- ship all redistributable application dependencies with the release;
- never ship Nintendo keys, ROM data, firmware, or proprietary game assets;
- support Etrian Odyssey IV, Etrian Odyssey Untold, Etrian Odyssey 2 Untold, Etrian Odyssey V, Etrian Odyssey Nexus, Etrian Mystery Dungeon, and Etrian Mystery Dungeon 2;
- use metadata-first, deterministic texture classification and retain `Unknown` when evidence is insufficient;
- preserve user edits, renames, tags, category overrides, and verified hash evidence across rescans;
- build validated Azahar texture packs without making Azahar naming rules the internal asset identity model.

## Release sequence

| Release | Objective |
| --- | --- |
| **0.13 Legacy Final** | Freeze and harden the Python extractor as the behavioral reference |
| **0.20 Core** | Rust workspace, core types, project model, structured errors |
| **0.30 Native ROM** | Native NCSD/CIA/NCCH/RomFS layer; remove Texture Forge ROM-reader dependency |
| **0.40 Texture Engine** | Native, tested PICA200 decoder and exact storage/hash semantics |
| **0.50 Containers** | Native archive/model/container framework |
| **0.60 Untold Parity** | EOU1 + EO2U parity with the frozen reference |
| **0.70 Universal EO** | EO IV + EO V + Nexus profiles |
| **0.75 Mystery Dungeon** | EMD1 + EMD2 profile/parser family |
| **0.80 Catalog** | Persistent asset catalog, browser, classification and editing workflow |
| **0.90 Beta** | Full supported-game matrix + Azahar integration |
| **0.95 RC** | Regression, security, packaging and recovery validation |
| **1.0** | Independent desktop release |

---

# 0.13 — Legacy Final

The Python implementation remains a reference implementation only after this milestone. New application architecture belongs in Rust from 0.20 onward.

## 0.13 exit criteria

### Correctness

- [x] Fix BCH/PICA base-level storage sizing for dimensions not aligned to 8x8 tiles.
- [x] Add non-8-aligned regression cases for ETC1, ETC1A4, RGBA8 and a 4-bit format.
- [x] Preserve the existing EOU1/EO2U structural parser regression suite through the hardening changes.
- [x] Capture a regression/fingerprint format for local legal game validation without committing copyrighted files.

### Input and extraction safety

- [x] Centralize archive/RomFS path containment.
- [x] Reject POSIX absolute paths, Windows drive paths, traversal components, device names and ADS syntax.
- [x] Reject truncated uncompressed HPB entries rather than silently slicing short data.
- [x] Add recursive extraction budgets: nesting depth, member count, expanded bytes and per-member allocation.

### Workspace safety

- [x] Preserve edited/upscaled master files even when the previous manifest is missing or corrupt.
- [x] Read live `pack.json` mappings when locating intentionally renamed masters.
- [x] Stage and validate master/deployment refreshes before atomic promotion.
- [x] Keep a rollback target until a new project state has been promoted successfully.
- [x] Require an EO-TexRip workspace marker before destructive legacy cleanup.

### Hash evidence

- [x] Exact RGBA/runtime evidence may become verified automatically.
- [x] Perceptual/downsample matches must remain candidates until explicitly confirmed.

### Material workspace

- [x] Stop persistent material reports from pointing at temporary files deleted after success.
- [x] Make material-workspace reconstruction compatible with the streamlined layout or retire the incompatible API.
- [x] Label reconstructed material alpha as diagnostic when exact shader reconstruction is not possible.

### Regression infrastructure

- [x] Add an explicit development/test dependency set.
- [x] Add Windows + Linux CI.
- [x] Run Python compile checks and the full pytest suite in CI.
- [x] Correct tests that inspect obsolete material-output paths.
- [x] Centralize the code-facing application version.
- [ ] Protect `main` with required CI after the workflow has proven stable.

## 0.13 implementation notes

- Canonical application version: `0.13.0`.
- Frozen parser provenance: `0.12.0` is retained separately as the legacy-reference version.
- `azahar_pack_master` is treated as recoverable user state independently of the manifest.
- Exact and perceptual runtime-hash evidence now have separate trust levels.
- Copyright-safe structural fingerprints can be emitted with `python eouhd_cli.py fingerprint <workspace>` or `tools/fingerprint_workspace.py`.
- Archive resource ceilings are configurable through `EO_TEXRIP_MAX_ARCHIVE_DEPTH`, `EO_TEXRIP_MAX_EXTRACTED_FILES`, `EO_TEXRIP_MAX_EXPANDED_BYTES`, `EO_TEXRIP_MAX_MEMBER_BYTES`, and `EO_TEXRIP_MAX_ARCHIVE_BYTES`.

## 0.13 rule

Do not broaden format heuristics while freezing the reference implementation. Every parser change must be supported by a structural reason and a regression test.

---

# 0.20 — Rust Core

0.20 begins only after the 0.13 reference behavior is stable enough to compare against.

Initial workspace target:

```text
eo-texrip/
├── crates/
│   ├── eo-core/
│   ├── eo-rom/
│   ├── eo-archives/
│   ├── eo-textures/
│   ├── eo-models/
│   ├── eo-catalog/
│   ├── eo-azahar/
│   └── eo-profiles/
└── app/
    └── desktop/
```

Core domain types will include `GameId`, `GameRegion`, `GameProfile`, `AssetId`, `TextureAsset`, `TextureFormat`, `TextureRole`, `ArchiveSource`, `RuntimeHash`, and structured extraction errors.

The long-term pipeline is:

```text
ROM Reader
  -> Game Detection
  -> Game Profile
  -> Archive Discovery
  -> Container Parsers
  -> PICA Texture Decoder
  -> Material Analysis
  -> Asset Catalog
  -> Deterministic Classification
  -> Master Workspace
  -> Azahar Builder
```

Game profiles decide where to look. Parsers decide what data is. The texture engine decides how it is decoded. The catalog stores stable asset identity and user decisions. Azahar remains an output target rather than the internal data model.
