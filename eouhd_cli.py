from __future__ import annotations

import argparse
import json
from pathlib import Path

from eouhd.pipeline import run_full_pipeline
from eouhd.workspace import build_azahar_pack, import_runtime_dump


def main() -> None:
    ap = argparse.ArgumentParser(description='Etrian Odyssey HD Texture Extractor 0.12 CLI')
    sub = ap.add_subparsers(dest='cmd', required=True)

    e = sub.add_parser('extract', help='Build/refresh the streamlined EOU/EO2U upscaling workspace.')
    e.add_argument('rom')
    e.add_argument('workspace')
    e.add_argument('--forge', default='tools/3DS-Texture-Forge')

    i = sub.add_parser('import-hashes', help='Import verified runtime hashes from an Azahar dump/old pack.')
    i.add_argument('workspace')
    i.add_argument('dump_or_pack')

    b = sub.add_parser('build-pack', help='Rebuild azahar_pack from azahar_pack_master.')
    b.add_argument('workspace')

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
    elif a.cmd == 'build-pack':
        print(build_azahar_pack(Path(a.workspace), True))


if __name__ == '__main__':
    main()
