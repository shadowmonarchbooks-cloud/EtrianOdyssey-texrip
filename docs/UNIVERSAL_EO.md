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

Identity recognition does **not** imply parser support. These profiles remain `PlannedResearch` until native extraction, independent coverage, and visible Azahar replacement are proven.

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
- [x] Collect v2 archive-aware recon reports for EO IV, EO V, and Nexus.
- [x] Freeze the observed per-game archive/container/model matrix from the archive-aware reports.

### Native extraction

- [ ] Reuse existing STEX/CGFX/BCH/HPI/FARC/EPL adapters only where recon evidence supports them.
- [x] Add a bounded standard-CTPK parser with synthetic regression fixtures; do not infer CTPK solely from a `.ctpk` extension.
- [ ] Account for the three EO IV `.ctpk`-named files whose payloads did not present standard `CTPK` magic in the bounded recon probe.
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

## Frozen USA archive-aware matrix

The first two reconnaissance passes establish two real layout families rather than one assumed "Universal EO" container stack.

| Game | Dominant storage | Texture/model evidence | Archive evidence | Open question |
| --- | --- | --- | --- | --- |
| EO IV | Direct RomFS files | 1,128 STEX, 350 ATBC/BAM, 156 leading CGFX, 352 bounded embedded-CGFX hits | 412 EPL files; all 412 structurally inspect, 2,553 aggregate EPL members | 3 `.ctpk`-named files do not present standard `CTPK` magic in the bounded probe and all three fail the standard parser |
| EO V | One HPI/HPB pair | HPI index: 1,479 STEX, 478 BCH, 225 BAM2, 3 `.ctpk` members | 11,627 HPI members total, 7,538 marked compressed, 1,381 EPL members by extension | Confirm payload magic/structure for the three `.ctpk` members before treating them as CTPK |
| Nexus | One HPI/HPB pair | HPI index: 1,990 STEX, 662 BCH, 448 BAM2, 3 `.ctpk` members | 17,883 HPI members total, 10,064 marked compressed, 1,968 EPL members by extension | Confirm payload magic/structure for the three `.ctpk` members before treating them as CTPK |

### EO IV

EO IV exposes its major texture/model families directly in RomFS. The existing bounded STEX, CGFX/ATBC, and EPL adapters are therefore justified reuse targets. All 412 top-level EPL files structurally inspect with zero EPL parser errors and contain 2,553 aggregate members.

The three `.ctpk`-named files are deliberately **not** recorded as proven CTPK containers. Neither v1 nor v2 observed `CTPK` magic in the bounded top-level magic scan, while v2's standard CTPK parser attempted all three files and rejected all three. Extension alone is not container proof. They remain an explicit payload-classification item for 0.70 coverage.

### EO V

EO V's dominant data path is HPI/HPB. Its HPI index contains 11,627 members with zero index errors; 7,538 entries are marked compressed. The texture/model-relevant extension surface includes 1,479 STEX, 478 BCH, 225 BAM2, 1,381 EPL, and three `.ctpk` members. This strongly supports reuse of the existing HPI/HPB, STEX, BCH/BAM2, and EPL families.

### Nexus

Nexus uses the same later-game archive family at larger scale. Its HPI index contains 17,883 members with zero index errors; 10,064 entries are marked compressed. The texture/model-relevant extension surface includes 1,990 STEX, 662 BCH, 448 BAM2, 1,968 EPL, and three `.ctpk` members. This supports the same HPI/HPB + STEX + BCH/BAM2 + EPL production architecture as EO V, while preserving the `.ctpk` payload question separately.

## Standard CTPK rule

The native CTPK adapter follows the documented CTR Texture PacKage structure: `CTPK` magic, bounded texture-info table, declared texture-data section, texture type, dimensions, format, and exact base-level payload bounds. It recognizes the standard PICA format IDs already implemented by the shared decoder.

A `.ctpk` filename is reconnaissance evidence only. Production may invoke the CTPK parser only when structural payload evidence supports it; the scanner must not reinterpret extension-only files as standard CTPK.

## Reconnaissance report

The development CLI is:

```text
eo-texrip-recon <decrypted EO4/EO5/EON ROM> [output-report.json]
```

The current branch emits schema `eo-texrip-universal-eo-recon-v2`. In addition to the v1 top-level aggregate fields, v2 reports:

- aggregate HPI/HPB pair counts;
- HPI index member counts and extension buckets, read from the small HPI index without copying the large HPB payload;
- the number of HPI entries marked compressed;
- structurally validated EPL file/member counts and aggregate EPL member extensions;
- structural standard-CTPK file/texture/type/format counts where actual CTPK parsing succeeds.

Reports remain safe to share for development because they contain aggregate counts rather than game file/member names or payload data.

Recon recognizes these known four-byte format hints at the start of a top-level file or embedded within the bounded probe: `STEX`, `CGFX`, `BCH\0`, `ATBC`, `CTPK`, `CTXB`/`ctxb`, `cmb `, `FARC`, and `SIR0`, plus structurally inventoried EPL packages.

A magic hit or file extension is reconnaissance evidence, not parser proof. Production support requires the corresponding bounded structural parser to validate the container.

## 0.70 rule

Do not generalize the Untold orchestrator into a universal Atlus scanner by assumption. Identify first, measure second, implement from structural evidence third, and declare support only after independent coverage and visible Azahar replacement are both proven for each title.
