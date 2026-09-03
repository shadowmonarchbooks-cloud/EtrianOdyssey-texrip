from __future__ import annotations

import argparse
import json
from pathlib import Path

from eouhd.pipeline import run_full_pipeline
from eouhd.regression import build_workspace_fingerprint, compare_fingerprints
from eouhd.version import DISPLAY_VERSION
from eouhd.workspace import (
    build_azahar_pack,
    confirm_runtime_hash_candidate,
    import_runtime_dump,
)


def main() -> None:
    ap = argparse.ArgumentParser(description=f'Etrian Odyssey HD Texture Extractor {DISPLAY_VERSION} CLI')
    sub = ap.add_subparsers(dest='cmd', required=True)

    e = sub.add_parser('extract', help='Build/refresh the streamlined EOU/EO2U reference workspace.')
    e.add_argument('rom')
    e.add_argument('workspace')
    e.add_argument('--forge', default='tools/3DS-Texture-Forge')

    i = sub.add_parser('import-hashes', help='Import runtime-hash evidence from an Azahar dump/old pack.')
    i.add_argument('workspace')
    i.add_argument('dump_or_pack')

    c = sub.add_parser('confirm-hash', help='Promote one pending perceptual runtime-hash candidate to verified.')
    c.add_argument('workspace')
    c.add_argument('asset_id')
    c.add_argument('runtime_hash')

    b = sub.add_parser('build-pack', help='Rebuild azahar_pack from azahar_pack_master.')
    b.add_argument('workspace')

    f = sub.add_parser('fingerprint', help='Emit a copyright-safe structural workspace fingerprint.')
    f.add_argument('workspace')
    f.add_argument('--compare', type=Path)

    a = ap.parse_args()
    if a.cmd == 'extract':
        result = run_full_pipeline(
            Path(a.rom),
            Path(a.workspace),
            Path(a.forge),
            print,
            False,
            True,
        )
        print(json.dumps(result, indent=2))
    elif a.cmd == 'import-hashes':
        print(json.dumps(import_runtime_dump(Path(a.workspace), Path(a.dump_or_pack)), indent=2))
    elif a.cmd == 'confirm-hash':
        print(json.dumps(confirm_runtime_hash_candidate(Path(a.workspace), a.asset_id, a.runtime_hash), indent=2))
    elif a.cmd == 'build-pack':
        print(build_azahar_pack(Path(a.workspace), True))
    elif a.cmd == 'fingerprint':
        actual = build_workspace_fingerprint(Path(a.workspace))
        if a.compare:
            expected = json.loads(a.compare.read_text(encoding='utf-8'))
            result = compare_fingerprints(expected, actual)
            print(json.dumps(result, indent=2))
            if not result['match']:
                raise SystemExit(1)
        else:
            print(json.dumps(actual, indent=2))


if __name__ == '__main__':
    main()
