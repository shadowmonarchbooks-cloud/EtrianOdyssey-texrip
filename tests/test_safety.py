from pathlib import Path

import pytest

from eouhd.safety import UnsafeArchivePath, safe_archive_join, safe_archive_relative_path


@pytest.mark.parametrize(
    'member',
    [
        '../escape.bin',
        'nested/../../escape.bin',
        '/absolute/file.bin',
        r'\rooted\file.bin',
        r'C:\absolute\file.bin',
        'C:/absolute/file.bin',
        r'C:relative-on-drive.bin',
        'CON',
        'nul.txt',
        'folder/PRN.dat',
        'folder/name:stream',
        'folder/trailing.',
        'folder/trailing ',
        'folder//double.bin',
        'folder/./dot.bin',
    ],
)
def test_rejects_cross_platform_unsafe_archive_paths(member: str):
    with pytest.raises(UnsafeArchivePath):
        safe_archive_relative_path(member)


def test_safe_archive_join_keeps_normal_members_under_root(tmp_path: Path):
    dest = safe_archive_join(tmp_path, r'graphics\monsters\enemy01.stex')
    assert dest == (tmp_path / 'graphics' / 'monsters' / 'enemy01.stex').resolve()
    assert dest.is_relative_to(tmp_path.resolve())
