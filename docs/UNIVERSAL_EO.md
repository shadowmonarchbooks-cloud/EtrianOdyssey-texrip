# 0.70 Universal EO

0.70 extends EO-TexRip's native Atlus Etrian pipeline beyond the completed Untold milestone to **Etrian Odyssey IV: Legends of the Titan**, **Etrian Odyssey V: Beyond the Myth**, and **Etrian Odyssey Nexus**.

The milestone is evidence-driven. Shared engine lineage is not sufficient evidence to route a title through an Untold parser. Each game must first be identified from verified container metadata, then structurally surveyed with privacy-safe reconnaissance, and only then assigned the archive/container/model adapters its real ROM layout requires.

## Supported 0.70 identities

The profile registry recognizes the known JP/US/EU retail identities for the three 0.70 targets:

| Profile | Product family | Known Title IDs |
| --- | --- | --- |
| `eo4` | `ASJ` | `0004000000080100`, `00040000000BD300`, `00040000000EA600` |
| `eo5` | `BMZ` | `000400000018D000`, `00040000001C5100`, `00040000001C5300` |
| `eon` | `BZM` | `00040000001CA300`, `00040000001D4E00`, `00040000001D5200` |

Identity recognition does **not** imply parser support. These profiles remain `PlannedResearch` until real structural evidence establishes the extraction path.

## Exit criteria

### Identity and reconnaissance

- [x] Register verified JP/US/EU Title IDs and product-code families for EO IV, EO V, and Nexus.
- [x] Keep EO IV/V/Nexus separate from the Untold extraction gate even after identity detection succeeds.
- [x] Add a privacy-safe `eo-recon` library and CLI for the three 0.70 profiles.
- [x] Recon reports aggregate RomFS counts, file-size totals, extension counts, and known leading/embedded magic-family counts.
- [x] Recon reports omit RomFS paths, proprietary bytes, payload offsets, member names, and content hashes.
- [x] Collect one top-level recon report from a user-owned decrypted EO IV ROM.
- [x] Collect one top-level recon report from a user-owned decrypted EO V ROM.
- [x] Collect one top-level recon report from a user-owned decrypted Nexus ROM.
- [x] Extend reconnaissance to aggregate HPI-index member extensions and structurally inspected EPL-member extensions without exporting proprietary names.
- [ ] Collect v2 archive-aware recon reports for EO IV, EO V, and Nexus.
- [ ] Freeze the observed per-game archive/container/model matrix from the archive-aware reports.

### Native extraction

- [ ] Reuse existing STEX/CGFX/BCH/HPI/FARC/EPL adapters only where recon evidence supports them.
- [ ] Add any additional archive/container/model parser required by EO IV from structural evidence and synthetic regression fixtures.
- [ ] Add any additional archive/container/model parser required by EO V from structural evidence and synthetic regression fixtures.
- [ ] Add any additional archive/container/model parser required by Nexus from structural evidence and synthetic regression fixtures.
- [ ] Route EO IV through the reusable native extraction/export layer.
- [ ] Route EO V through the reusable native extraction/export layer.
- [ ] Route Nexus through the reusable native extraction/export layer.
- [ ] Preserve deterministic PNG export, exact runtime-hash evidence, `extraction-report.json`, and generated Azahar `pack.json` semantics established in 0.60.

### Coverage validation

- [ ] Extend independent structural coverage auditing to every texture-bearing format actually observed in EO IV.
- [ ] Extend independent structural coverage auditing to every texture-bearing format actually observed in EO V.
- [ ] Extend independent structural coverage auditing to every texture-bearing format actually observed in Nexus.
- [ ] EO IV audit completes with no unaccounted known texture-bearing structures.
- [ ] EO V audit completes with no unaccounted known texture-bearing structures.
- [ ] Nexus audit completes with no unaccounted known texture-bearing structures.

### Product smoke

- [ ] Packaged Windows extraction succeeds on a user-owned decrypted EO IV ROM.
- [ ] Packaged Windows extraction succeeds on a user-owned decrypted EO V ROM.
- [ ] Packaged Windows extraction succeeds on a user-owned decrypted Nexus ROM.
- [ ] One visibly modified EO IV exported PNG is rendered by Azahar through the generated pack.
- [ ] One visibly modified EO V exported PNG is rendered by Azahar through the generated pack.
- [ ] One visibly modified Nexus exported PNG is rendered by Azahar through the generated pack.

### Quality and legal boundary

- [ ] Rust formatting, Clippy with warnings denied, and workspace tests pass on Ubuntu and Windows at the final 0.70 head.
- [ ] Frozen Python regression CI remains green while the legacy implementation remains in the repository.
- [ ] Windows packaging includes the user-facing executables required to collect evidence and exercise 0.70.
- [x] Repository tests use synthetic fixtures only.
- [x] Do not commit Nintendo keys, ROM data, firmware, decoded game images, proprietary paths, or proprietary binary fixtures.

## Initial USA reconnaissance evidence

The first top-level `v1` reports establish a real split between EO IV and the later two titles.

### EO IV

- 4,900 RomFS files, about 739 MB total; no top-level HPI/HPB pair.
- 1,128 `.stex` files and 1,128 leading `STEX` signatures.
- 350 `.bam` files and 350 leading `ATBC` signatures.
- 38 `.bcmdl`, 3 `.bcres`, and 156 leading `CGFX` payloads.
- 352 files contain embedded `CGFX` within the bounded top-level probe.
- 412 `.epl` files are present and require structural member inventory rather than magic-only inference.
- 3 direct `.ctpk` files are present. CTPK therefore remains a concrete EO IV parser/coverage question rather than a hypothetical future format.

This strongly supports reuse of the existing STEX, CGFX/ATBC, and EPL families for EO IV, but the v2 EPL inventory and the three observed CTPK containers must be accounted for before the EO IV matrix is frozen.

### EO V

- Only 45 top-level RomFS files, about 562 MB total.
- One `.hpi` and one `.hpb`; the largest top-level file is about 318 MB.
- The top-level surface otherwise consists mostly of audio/movie/system files.
- One leading `CGFX` exists, but no bounded embedded texture/container magic was observed at top level.

The top-level result is not evidence that EO V lacks STEX/CGFX/BCH or other texture families: most game data is hidden behind the HPI/HPB pair. The v2 HPI-index extension aggregate is required before assigning EO V's extraction path.

### Nexus

- 87 top-level RomFS files, about 850 MB total.
- One `.hpi` and one `.hpb`; the largest top-level file is about 509 MB.
- The top-level surface otherwise consists mostly of audio/movie/system files.
- One leading `CGFX` exists; one bounded embedded `CGFX` and one embedded `STEX` hit were observed.

As with EO V, the HPI/HPB pair is the dominant evidence boundary. The v2 HPI-index extension aggregate is required before assigning Nexus's extraction path.

## Reconnaissance report

The development CLI is:

```text
eo-texrip-recon <decrypted EO4/EO5/EON ROM> [output-report.json]
```

The current branch emits schema `eo-texrip-universal-eo-recon-v2`. In addition to the v1 top-level aggregate fields, v2 reports:

- aggregate HPI/HPB pair counts;
- HPI index member counts and extension buckets, read from the small HPI index without copying the large HPB payload;
- the number of HPI entries marked compressed;
- structurally validated EPL file/member counts and aggregate EPL member extensions.

Reports remain safe to share for development because they contain aggregate counts rather than game file/member names or payload data.

Recon recognizes these known four-byte format hints at the start of a top-level file or embedded within the bounded probe: `STEX`, `CGFX`, `BCH\0`, `ATBC`, `CTPK`, `CTXB`/`ctxb`, `cmb `, `FARC`, and `SIR0`, plus structurally inventoried EPL packages.

A magic hit or file extension is reconnaissance evidence, not parser proof. Production support still requires the corresponding bounded structural parser to validate the container.

## 0.70 rule

Do not generalize the Untold orchestrator into a universal Atlus scanner by assumption. Identify first, measure second, implement from structural evidence third, and declare support only after independent coverage and visible Azahar replacement are both proven for each title.
