import struct
import tempfile
from pathlib import Path

import pytest

from eouhd.extraction_budget import (
    ExtractionBudget,
    ExtractionBudgetError,
    ExtractionLimits,
    current_budget,
    reset_budget,
)
from eouhd.hpx import unpack_hpi_pair
from eouhd.workspace import reset_generated_workspace


def _limits(**overrides) -> ExtractionLimits:
    base = dict(
        max_depth=16,
        max_members=100,
        max_expanded_bytes=10_000,
        max_member_bytes=5_000,
        max_archive_bytes=5_000,
    )
    base.update(overrides)
    return ExtractionLimits(**base)


def test_budget_tracks_nested_archive_depth_across_output_roots() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        budget = ExtractionBudget(_limits(max_depth=2))

        a1 = root / 'a1.bin'; a1.write_bytes(b'a')
        out1 = root / 'out1'; out1.mkdir()
        assert budget.preflight_archive(a1, out1, [1]) == 1

        a2 = out1 / 'a2.bin'; a2.write_bytes(b'b')
        out2 = root / 'out2'; out2.mkdir()
        assert budget.preflight_archive(a2, out2, [1]) == 2

        a3 = out2 / 'a3.bin'; a3.write_bytes(b'c')
        with pytest.raises(ExtractionBudgetError, match='nesting depth 3'):
            budget.preflight_archive(a3, root / 'out3', [1])


def test_budget_enforces_file_and_expanded_byte_limits() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        source = root / 'archive.bin'; source.write_bytes(b'x')

        files = ExtractionBudget(_limits(max_members=2))
        with pytest.raises(ExtractionBudgetError, match='file-count budget'):
            files.preflight_archive(source, root / 'files', [1, 1, 1])

        expanded = ExtractionBudget(_limits(max_expanded_bytes=9))
        with pytest.raises(ExtractionBudgetError, match='expanded-byte budget'):
            expanded.preflight_archive(source, root / 'bytes', [5, 5])


def test_budget_rejects_oversized_single_member_before_allocation() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        hpi = root / 'test.hpi'
        hpb = root / 'test.hpb'

        data = bytearray(0x18 + 16 + len(b'huge.bin\0'))
        data[:4] = b'HPIH'
        struct.pack_into('<H', data, 0x12, 0)
        struct.pack_into('<H', data, 0x14, 1)
        struct.pack_into('<IIII', data, 0x18, 0, 0, 4, 4096)
        data[0x28:] = b'huge.bin\0'
        hpi.write_bytes(data)
        hpb.write_bytes(b'\0' * 64)

        reset_budget(_limits(max_member_bytes=1024))
        with pytest.raises(ExtractionBudgetError, match='per-member limit'):
            unpack_hpi_pair(hpi, root / 'out')
        assert not (root / 'out').exists()


def test_new_pipeline_workspace_reset_starts_a_fresh_budget() -> None:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        budget = reset_budget(_limits())
        source = root / 'archive.bin'; source.write_bytes(b'x')
        budget.preflight_archive(source, root / 'expanded', [10, 20])
        assert current_budget().snapshot()['members'] == 2

        reset_generated_workspace(root)
        snapshot = current_budget().snapshot()
        assert snapshot['members'] == 0
        assert snapshot['expanded_bytes_reserved'] == 0
        assert snapshot['archives'] == 0
