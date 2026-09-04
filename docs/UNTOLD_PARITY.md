# 0.60 Untold Parity

0.60 composes the native Rust ROM, archive, texture, and model layers into the first end-to-end Etrian Odyssey Untold / Etrian Odyssey 2 Untold extraction path. The frozen Python 0.13 implementation remains the behavioral reference for this milestone only; parity is measured with privacy-safe structural fingerprints, never by committing proprietary game data.

## Exit criteria

### Native orchestration

- [x] Add a dedicated `eo-untold` orchestration crate at workspace version `0.60.0`.
- [x] Require a verified EOU1 or EO2U identity before using the Untold pipeline.
- [x] Enumerate candidate RomFS files through the native `RomReader` contract.
- [x] Reject oversized candidate reads before requesting their payload bytes.
- [x] Pair HPI/HPB paths case-insensitively without relying on host filesystem behavior.
- [x] Expand HPI/HPB, FARC, and EPL recursively in memory under cumulative extraction budgets.
- [x] Keep extracted proprietary bytes out of persistent parity artifacts.

### Native texture and model path

- [x] Route STEX through the native STEX adapter and native PICA decoder.
- [x] Route direct and ATBC-wrapped CGFX through native texture and material inspection.
- [x] Route direct and BAM/ATBC-wrapped BCH through native texture and material inspection.
- [x] Preserve the frozen parser provenance labels used by asset descriptors (`eou_stex_strict`, `cgfx_struct`, `bch_struct`).
- [x] Deduplicate decoded assets by `(candidate_hash, format, width, height)` like the frozen reference.
- [x] Merge material bindings through structural texture-name relationships rather than image heuristics.
- [x] Surface known legacy-only CTPK/CTXB/CMB candidates as explicit parity gaps instead of silently claiming support.

### Runtime candidate hash and fingerprint compatibility

- [x] Port the CityHash64 variant used by the frozen reference/Azahar candidate-hash path.
- [x] Hash the exact encoded PICA base-level bytes rather than decoded RGBA or container padding.
- [x] Add cross-language CityHash64 reference vectors.
- [x] Emit schema-1 `eo-texrip-structural-regression-fingerprint` data without ROM bytes, decoded pixels, source paths, or model/texture names.
- [x] Match the frozen asset descriptor fields, sort order, aggregate counter keys, and comparison keys.
- [x] Pin the cross-language canonical asset-descriptor SHA-256 vector in Rust tests.
- [x] Add a native CLI that emits a schema-1 fingerprint and optionally compares it to a frozen schema-1 reference with a failing exit status on mismatch.

### Real-game parity gate

- [ ] Produce a frozen Python schema-1 fingerprint from a local legal EOU1 source/workspace.
- [ ] Produce the native Rust schema-1 fingerprint from the same EOU1 source and make all comparison keys match.
- [ ] Produce a frozen Python schema-1 fingerprint from a local legal EO2U source/workspace.
- [ ] Produce the native Rust schema-1 fingerprint from the same EO2U source and make all comparison keys match.
- [ ] If a residual CTPK/CTXB/CMB or other format appears in a real mismatch, implement it from structural evidence and add synthetic regression coverage before accepting it.
- [ ] If model counts differ, expose the real structural model count from `eo-models`; do not infer it from payload counts.
- [ ] Match material-alpha/shader summary fields from actual PICA/CGFX/BCH metadata; do not infer mask semantics from grayscale appearance or texture slot alone.

### Quality and legal boundary

- [ ] Pass Rust formatting, Clippy with warnings denied, and the full workspace tests on Ubuntu and Windows at the final 0.60 head.
- [ ] Keep the frozen Python regression matrix green on Ubuntu/Windows and Python 3.10/3.12 at the final 0.60 head.
- [x] Use synthetic fixtures only in the repository.
- [x] Do not add Nintendo keys, ROM data, firmware, decoded game images, local source paths, or proprietary model/texture names to committed parity evidence.

## Fingerprint contract

The native fingerprint intentionally mirrors `eouhd.regression.build_structural_fingerprint()` schema 1. Asset descriptors contain only:

- candidate CityHash64;
- verified runtime hashes, when independently known;
- dimensions, format, and mip index;
- parser provenance;
- coarse category;
- structural material-binding count.

The descriptor list is sorted deterministically and SHA-256 hashed as compact canonical JSON. Aggregate parser, format, dimension, category, archive, model, and material counters are then compared field-by-field. Source paths and embedded resource names are transient matching data and must not be serialized into the fingerprint.

The compatibility test pins both the legacy CityHash64 vectors and a complete two-asset canonical descriptor digest. This catches accidental changes to JSON field order, separators, descriptor sorting, parser labels, category rules, format IDs, or candidate hashes even when aggregate counts still look plausible.

## Local parity workflow

The frozen Python side remains the authority for generating the expected schema-1 fingerprint from a local legal reference workspace:

```text
python tools/fingerprint_workspace.py <reference-workspace> -o eou1-python.json
```

The native side can then emit and compare the same privacy-safe schema from a cleartext ROM container whose NCCH metadata identifies EOU1 or EO2U:

```text
cargo run -p eo-untold --bin untold-fingerprint -- <decrypted-rom> eou1-python.json
```

The native fingerprint is written to standard output. When a reference fingerprint is supplied, a structural comparison is written to standard error and the process exits with status 1 on a mismatch. Encrypted ROM content remains outside the 0.60 parser boundary; EO-TexRip does not ship or acquire Nintendo keys.

## Known provisional gaps

The first native parity implementation deliberately leaves several fields capable of failing the real comparison rather than fabricating them:

- `eo-models` currently returns a payload-level model inventory rather than an explicit multi-model count;
- exact CGFX/BCH alpha-combiner reconstruction is not yet represented in the Rust fingerprint, so alpha-resolution counters remain zero until structural shader semantics are ported;
- CTPK, CTXB, and CMB remain unsupported unless the EOU1/EO2U fingerprints prove they occur on the required extraction path.

These are diagnostic gaps, not permissions to loosen probing. A real fingerprint difference should identify which structural implementation is missing.

## 0.60 rule

Do not tune parsers or counters to opaque expected numbers. Every parity fix must be explained by format structure, exact hash semantics, or a reproducible reference behavior and must receive synthetic regression coverage. Unknown data stays unknown until the evidence is strong enough to make it native support.
