from __future__ import annotations

"""Shared safety helpers for paths originating from game/archive metadata.

Archive member names are untrusted input.  They must never be allowed to escape
an extractor-owned output root, even when a Windows-style absolute path is
encountered while the extractor itself is running on POSIX (or vice versa).
"""

from pathlib import Path, PurePosixPath, PureWindowsPath
import re


class UnsafeArchivePath(ValueError):
    """Raised when an archive member path is unsafe to materialize."""


_WINDOWS_DEVICE = re.compile(
    r"^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$",
    re.IGNORECASE,
)


def safe_archive_relative_path(value: str) -> Path:
    """Convert an archive member name to a safe relative host path.

    Both POSIX and Windows absolute/path-traversal forms are rejected regardless
    of the host operating system.  Windows device names and NTFS alternate data
    stream syntax are rejected as well so a project created on Linux remains
    safe if later opened on Windows.
    """
    if not isinstance(value, str) or not value or "\x00" in value:
        raise UnsafeArchivePath("archive member path is empty or contains NUL")

    # Check the original spelling with Windows semantics before normalizing
    # separators.  This catches C:\\foo, C:foo and root-relative \\foo.
    win = PureWindowsPath(value)
    if win.drive or win.root or win.is_absolute():
        raise UnsafeArchivePath(f"absolute or drive-qualified archive path: {value!r}")

    normalized = value.replace("\\", "/")
    posix = PurePosixPath(normalized)
    if posix.is_absolute() or normalized.startswith("/"):
        raise UnsafeArchivePath(f"absolute archive path: {value!r}")

    raw_parts = normalized.split("/")
    if any(part in {"", ".", ".."} for part in raw_parts):
        raise UnsafeArchivePath(f"unsafe archive path component: {value!r}")

    for part in raw_parts:
        # Windows strips trailing spaces/dots and interprets ':' as ADS syntax.
        if part.endswith((" ", ".")) or ":" in part or _WINDOWS_DEVICE.match(part):
            raise UnsafeArchivePath(f"unsafe Windows archive path component: {part!r}")

    return Path(*raw_parts)


def safe_archive_join(root: str | Path, value: str) -> Path:
    """Return a contained destination for an untrusted archive member path."""
    root_path = Path(root).resolve(strict=False)
    rel = safe_archive_relative_path(value)
    dest = (root_path / rel).resolve(strict=False)
    try:
        dest.relative_to(root_path)
    except ValueError as exc:
        raise UnsafeArchivePath(f"archive member escapes output root: {value!r}") from exc
    return dest
