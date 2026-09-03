from __future__ import annotations

"""Shared resource budget for recursive legacy archive extraction.

All HPI/HPB, FARC and EPL expansion goes through one process-local budget during a
pipeline run.  The budget is reset when a new workspace extraction starts.  It
preflights archive metadata before payload allocation/writes, which prevents a
malformed nested archive from expanding without a depth/file/byte ceiling.
"""

from dataclasses import dataclass
from pathlib import Path
import os
import threading
from typing import Iterable


class ExtractionBudgetError(RuntimeError):
    pass


def _env_int(name: str, default: int) -> int:
    raw = os.environ.get(name)
    if not raw:
        return default
    try:
        value = int(raw, 0)
    except ValueError:
        return default
    return value if value > 0 else default


@dataclass(frozen=True)
class ExtractionLimits:
    max_depth: int = 16
    max_members: int = 250_000
    max_expanded_bytes: int = 32 * 1024 * 1024 * 1024
    max_member_bytes: int = 1024 * 1024 * 1024
    max_archive_bytes: int = 4 * 1024 * 1024 * 1024

    @classmethod
    def from_environment(cls) -> 'ExtractionLimits':
        return cls(
            max_depth=_env_int('EO_TEXRIP_MAX_ARCHIVE_DEPTH', cls.max_depth),
            max_members=_env_int('EO_TEXRIP_MAX_EXTRACTED_FILES', cls.max_members),
            max_expanded_bytes=_env_int('EO_TEXRIP_MAX_EXPANDED_BYTES', cls.max_expanded_bytes),
            max_member_bytes=_env_int('EO_TEXRIP_MAX_MEMBER_BYTES', cls.max_member_bytes),
            max_archive_bytes=_env_int('EO_TEXRIP_MAX_ARCHIVE_BYTES', cls.max_archive_bytes),
        )


class ExtractionBudget:
    def __init__(self, limits: ExtractionLimits | None = None):
        self.limits = limits or ExtractionLimits.from_environment()
        self.members = 0
        self.expanded_bytes = 0
        self.archives = 0
        self.max_depth_seen = 0
        self._output_depths: dict[Path, int] = {}
        self._lock = threading.Lock()

    @staticmethod
    def _resolved(path: str | Path) -> Path:
        return Path(path).resolve(strict=False)

    @staticmethod
    def _is_under(path: Path, root: Path) -> bool:
        try:
            path.relative_to(root)
            return True
        except ValueError:
            return False

    def _depth_for_source(self, source: Path) -> int:
        parent_depth = 0
        # Pick the deepest registered extraction root containing this archive.
        for root, depth in self._output_depths.items():
            if self._is_under(source, root):
                parent_depth = max(parent_depth, depth)
        return parent_depth + 1

    def preflight_archive(
        self,
        source: str | Path,
        output_dir: str | Path,
        member_sizes: Iterable[int],
    ) -> int:
        source = self._resolved(source)
        output = self._resolved(output_dir)
        sizes = [int(x) for x in member_sizes]
        if any(x < 0 for x in sizes):
            raise ExtractionBudgetError(f'Archive declares a negative member size: {source}')

        try:
            archive_size = source.stat().st_size
        except OSError as exc:
            raise ExtractionBudgetError(f'Cannot stat archive before extraction: {source}: {exc}') from exc
        if archive_size > self.limits.max_archive_bytes:
            raise ExtractionBudgetError(
                f'Archive exceeds input-size budget ({archive_size} > {self.limits.max_archive_bytes} bytes): {source}'
            )

        count = len(sizes)
        total = sum(sizes)
        largest = max(sizes, default=0)
        with self._lock:
            depth = self._depth_for_source(source)
            if depth > self.limits.max_depth:
                raise ExtractionBudgetError(
                    f'Archive nesting depth {depth} exceeds limit {self.limits.max_depth}: {source}'
                )
            if largest > self.limits.max_member_bytes:
                raise ExtractionBudgetError(
                    f'Archive member requires {largest} bytes; per-member limit is {self.limits.max_member_bytes}: {source}'
                )
            if self.members + count > self.limits.max_members:
                raise ExtractionBudgetError(
                    f'Extraction would exceed file-count budget: {self.members + count} > {self.limits.max_members}'
                )
            if self.expanded_bytes + total > self.limits.max_expanded_bytes:
                raise ExtractionBudgetError(
                    'Extraction would exceed expanded-byte budget: '
                    f'{self.expanded_bytes + total} > {self.limits.max_expanded_bytes}'
                )

            self.members += count
            self.expanded_bytes += total
            self.archives += 1
            self.max_depth_seen = max(self.max_depth_seen, depth)
            self._output_depths[output] = depth
            return depth

    def snapshot(self) -> dict:
        return {
            'archives': self.archives,
            'members': self.members,
            'expanded_bytes_reserved': self.expanded_bytes,
            'max_depth_seen': self.max_depth_seen,
            'limits': {
                'max_depth': self.limits.max_depth,
                'max_members': self.limits.max_members,
                'max_expanded_bytes': self.limits.max_expanded_bytes,
                'max_member_bytes': self.limits.max_member_bytes,
                'max_archive_bytes': self.limits.max_archive_bytes,
            },
        }


_CURRENT = ExtractionBudget()
_INSTALLED = False


def current_budget() -> ExtractionBudget:
    return _CURRENT


def reset_budget(limits: ExtractionLimits | None = None) -> ExtractionBudget:
    global _CURRENT
    _CURRENT = ExtractionBudget(limits)
    return _CURRENT


def _hpi_member_sizes(index_path: Path) -> list[int]:
    from . import hpx

    index = index_path
    if index.suffix.lower() == '.hpb':
        index = hpx._case_insensitive_sibling(index, '.hpi')
    entries = hpx.parse_hpi(index)
    return [int(e.decompressed_size or e.compressed_size) for e in entries]


def install() -> None:
    """Patch archive extractors before pipeline imports their public functions."""
    global _INSTALLED
    if _INSTALLED:
        return

    from . import hpx, farc, epl

    orig_hpi = hpx.unpack_hpi_pair
    orig_farc = farc.unpack_farc
    orig_epl = epl.unpack_epl

    def unpack_hpi_pair(index_path, output_dir):
        source = Path(index_path)
        sizes = _hpi_member_sizes(source)
        current_budget().preflight_archive(source, output_dir, sizes)
        return orig_hpi(index_path, output_dir)

    def unpack_farc(path, output_dir):
        source = Path(path)
        size = source.stat().st_size
        if size > current_budget().limits.max_archive_bytes:
            raise ExtractionBudgetError(
                f'Archive exceeds input-size budget ({size} > {current_budget().limits.max_archive_bytes} bytes): {source}'
            )
        data = source.read_bytes()
        entries, _metadata = farc.parse_farc(data)
        current_budget().preflight_archive(source, output_dir, (e.data_length for e in entries))
        return orig_farc(path, output_dir)

    def unpack_epl(path, output_dir):
        source = Path(path)
        size = source.stat().st_size
        if size > current_budget().limits.max_archive_bytes:
            raise ExtractionBudgetError(
                f'Archive exceeds input-size budget ({size} > {current_budget().limits.max_archive_bytes} bytes): {source}'
            )
        data = source.read_bytes()
        entries, _metadata = epl.parse_epl(data)
        current_budget().preflight_archive(source, output_dir, (e.data_size for e in entries))
        return orig_epl(path, output_dir)

    hpx.unpack_hpi_pair = unpack_hpi_pair
    farc.unpack_farc = unpack_farc
    epl.unpack_epl = unpack_epl
    _INSTALLED = True
