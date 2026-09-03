from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from eouhd.regression import build_workspace_fingerprint, compare_fingerprints


def main() -> int:
    parser = argparse.ArgumentParser(
        description='Create or compare a copyright-safe EO-TexRip structural regression fingerprint.'
    )
    parser.add_argument('workspace', type=Path, help='EO-TexRip workspace directory')
    parser.add_argument('--write', type=Path, help='Write fingerprint JSON to this path')
    parser.add_argument('--compare', type=Path, help='Compare against an existing fingerprint JSON')
    args = parser.parse_args()

    actual = build_workspace_fingerprint(args.workspace)
    if args.write:
        args.write.parent.mkdir(parents=True, exist_ok=True)
        args.write.write_text(json.dumps(actual, indent=2) + '\n', encoding='utf-8')

    if args.compare:
        expected = json.loads(args.compare.read_text(encoding='utf-8'))
        result = compare_fingerprints(expected, actual)
        print(json.dumps(result, indent=2))
        return 0 if result['match'] else 1

    print(json.dumps(actual, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
