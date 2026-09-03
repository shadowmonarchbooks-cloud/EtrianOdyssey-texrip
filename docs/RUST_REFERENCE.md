# Rust implementation reference boundary

EO-TexRip 0.13 is retained only as a behavioral reference for the independent Rust application. New Rust code does not need to reproduce the Python module layout, temporary workspace structure, GUI implementation, or third-party bootstrap mechanism.

## What Rust must preserve

For a known legal local ROM dump, compare the 0.13 structural fingerprint with the Rust extraction result. The comparison contract covers:

- detected game/profile and region when known;
- unique texture asset count;
- visible width and height;
- PICA200 storage width and height;
- PICA200 texture format;
- encoded base-level byte size;
- mip-level metadata when available;
- structural model/material texture relationships;
- candidate runtime hashes;
- runtime-verified and user-verified hashes;
- aggregate structural digest produced from those records.

A mismatch in these fields is a migration regression until explained by a documented bug fix or stronger structural evidence.

## What Rust must not preserve

The Rust application is intentionally free to replace:

- Python package/module names;
- Tkinter UI behavior;
- 3DS Texture Forge integration;
- transient extraction directory names;
- legacy manifest file placement;
- heuristic implementation details that are not part of the structural result;
- friendly filenames and category paths as asset identity.

Stable Rust `AssetId` values must remain independent of friendly filenames, user categories, and Azahar deployment paths.

## Copyright-safe validation workflow

Do not commit ROMs, extracted copyrighted game files, decoded game textures, Nintendo keys, or proprietary binary fixtures.

For local validation:

1. Run the 0.13 extractor against a decrypted dump made from a copy you own.
2. Produce the copyright-safe structural fingerprint with `python eouhd_cli.py fingerprint <workspace>`.
3. Run the Rust implementation against the same local dump once the relevant parser milestone exists.
4. Produce the equivalent Rust structural fingerprint.
5. Compare structural records and aggregate digests.
6. Investigate differences at the parser/texture-storage level before changing expected values.

Synthetic test fixtures created specifically for the project may be committed when they contain no copyrighted game data.

## Correctness priority

When the legacy result conflicts with independently verified 3DS/PICA structure, the Rust implementation should fix the bug rather than reproduce it. Such intentional differences must receive a regression test and a short migration note describing why the Rust result is more correct.
