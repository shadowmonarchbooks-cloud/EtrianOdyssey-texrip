from __future__ import annotations

from dataclasses import dataclass, asdict
import re


class UnsupportedGameError(RuntimeError):
    pass


@dataclass(frozen=True)
class GameProfile:
    id: str
    short_name: str
    display_name: str
    product_family: str
    known_title_ids: tuple[str, ...]
    archive_families: tuple[str, ...]
    model_families: tuple[str, ...]
    notes: str = ''

    def to_dict(self) -> dict:
        return asdict(self)


EOU1 = GameProfile(
    id='eou1',
    short_name='EOU',
    display_name='Etrian Odyssey Untold: The Millennium Girl',
    product_family='BSK',
    known_title_ids=(
        '00040000000EC700',  # USA
        '000400000010EB00',  # EUR
    ),
    archive_families=('HPI/HPB', 'FARC (optional/fallback)'),
    model_families=('ATBC -> CGFX/BCMDL', 'direct CGFX/BCMDL', 'BCH/H3D fallback'),
    notes='EOU1 primary 3D path verified against an actual ATBC-wrapped CGFX enemy model.',
)

EO2U = GameProfile(
    id='eo2u',
    short_name='EO2U',
    display_name='Etrian Odyssey 2 Untold: The Fafnir Knight',
    product_family='BM9',
    known_title_ids=(
        '0004000000120500',  # Japan
        '000400000015F200',  # USA
        '000400000016E900',  # EUR/AUS
    ),
    archive_families=('HPI/HPB', 'FARC (optional/fallback)'),
    model_families=('ATBC/BAM2 -> BCH/H3D', 'direct BCH/H3D', 'CGFX/BCMDL fallback'),
    notes='Actual EO2U inventory confirms ATBC .BAM2 wrappers containing BCH plus many direct BCH/STEX resources.',
)

SUPPORTED_PROFILES = (EOU1, EO2U)
_BY_TITLE = {title.upper(): profile for profile in SUPPORTED_PROFILES for title in profile.known_title_ids}


def normalize_product_code(product_code: str) -> str:
    return re.sub(r'[^A-Z0-9]', '', str(product_code or '').upper())


def detect_game_profile(title_id: str, product_code: str = '') -> GameProfile:
    title = str(title_id or '').upper().zfill(16)
    if title in _BY_TITLE:
        return _BY_TITLE[title]

    product = normalize_product_code(product_code)
    for profile in SUPPORTED_PROFILES:
        if profile.product_family in product:
            return profile

    raise UnsupportedGameError(
        'Unsupported 3DS title. This build supports Etrian Odyssey Untold: '
        'The Millennium Girl and Etrian Odyssey 2 Untold: The Fafnir Knight. '
        f'Detected Title ID={title or "unknown"}, Product={product_code or "unknown"}.'
    )


def profile_summary(profile: GameProfile) -> str:
    return f'{profile.short_name} — {profile.display_name}'
