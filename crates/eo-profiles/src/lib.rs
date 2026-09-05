//! Game profile registry for the independent application.
//!
//! A profile declares identity and support state only. Parser behavior belongs
//! in later crates. Unknown/unverified games are never forced through a nearby
//! profile merely because their files look similar.

use eo_core::{GameFamily, GameId, TitleId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatus {
    /// Structural behavior is covered by the frozen 0.13 reference.
    LegacyReferenceVerified,
    /// Included in the product target, but binary-format reconnaissance is still required.
    PlannedResearch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameProfile {
    pub profile_id: &'static str,
    pub game_id: GameId,
    pub display_name: &'static str,
    pub family: GameFamily,
    pub status: ProfileStatus,
    pub known_title_ids: &'static [&'static str],
    pub known_product_families: &'static [&'static str],
}

pub const EOU1: GameProfile = GameProfile {
    profile_id: "eou1",
    game_id: GameId::EtrianOdysseyUntold,
    display_name: "Etrian Odyssey Untold: The Millennium Girl",
    family: GameFamily::AtlusEtrian,
    status: ProfileStatus::LegacyReferenceVerified,
    known_title_ids: &["00040000000EC700", "000400000010EB00"],
    known_product_families: &["BSK"],
};

pub const EO2U: GameProfile = GameProfile {
    profile_id: "eo2u",
    game_id: GameId::EtrianOdyssey2Untold,
    display_name: "Etrian Odyssey 2 Untold: The Fafnir Knight",
    family: GameFamily::AtlusEtrian,
    status: ProfileStatus::LegacyReferenceVerified,
    known_title_ids: &[
        "0004000000120500",
        "000400000015F200",
        "000400000016E900",
    ],
    known_product_families: &["BM9"],
};

pub const EO4: GameProfile = GameProfile {
    profile_id: "eo4",
    game_id: GameId::EtrianOdysseyIv,
    display_name: "Etrian Odyssey IV: Legends of the Titan",
    family: GameFamily::AtlusEtrian,
    status: ProfileStatus::PlannedResearch,
    known_title_ids: &[
        "0004000000080100",
        "00040000000BD300",
        "00040000000EA600",
    ],
    known_product_families: &["ASJ"],
};

pub const EO5: GameProfile = GameProfile {
    profile_id: "eo5",
    game_id: GameId::EtrianOdysseyV,
    display_name: "Etrian Odyssey V: Beyond the Myth",
    family: GameFamily::AtlusEtrian,
    status: ProfileStatus::PlannedResearch,
    known_title_ids: &[
        "000400000018D000",
        "00040000001C5100",
        "00040000001C5300",
    ],
    known_product_families: &["BMZ"],
};

pub const EON: GameProfile = GameProfile {
    profile_id: "eon",
    game_id: GameId::EtrianOdysseyNexus,
    display_name: "Etrian Odyssey Nexus",
    family: GameFamily::AtlusEtrian,
    status: ProfileStatus::PlannedResearch,
    known_title_ids: &[
        "00040000001CA300",
        "00040000001D4E00",
        "00040000001D5200",
    ],
    known_product_families: &["BZM"],
};

pub const EMD1: GameProfile = GameProfile {
    profile_id: "emd1",
    game_id: GameId::EtrianMysteryDungeon,
    display_name: "Etrian Mystery Dungeon",
    family: GameFamily::MysteryDungeon,
    status: ProfileStatus::PlannedResearch,
    known_title_ids: &[],
    known_product_families: &[],
};

pub const EMD2: GameProfile = GameProfile {
    profile_id: "emd2",
    game_id: GameId::EtrianMysteryDungeon2,
    display_name: "Etrian Mystery Dungeon 2",
    family: GameFamily::MysteryDungeon,
    status: ProfileStatus::PlannedResearch,
    known_title_ids: &[],
    known_product_families: &[],
};

pub const ALL_PROFILES: [GameProfile; 7] = [EO4, EOU1, EO2U, EO5, EON, EMD1, EMD2];

pub fn profile_for_game(game_id: GameId) -> &'static GameProfile {
    ALL_PROFILES
        .iter()
        .find(|profile| profile.game_id == game_id)
        .expect("every core 3DS target must have exactly one profile")
}

pub fn detect_verified_profile(
    title_id: Option<TitleId>,
    product_code: Option<&str>,
) -> Option<&'static GameProfile> {
    if let Some(title_id) = title_id {
        let title = title_id.to_string();
        if let Some(profile) = ALL_PROFILES.iter().find(|profile| {
            profile
                .known_title_ids
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&title))
        }) {
            return Some(profile);
        }
    }

    let normalized_product = product_code
        .unwrap_or_default()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect::<String>();
    if normalized_product.is_empty() {
        return None;
    }

    ALL_PROFILES.iter().find(|profile| {
        !profile.known_product_families.is_empty()
            && profile
                .known_product_families
                .iter()
                .any(|family| normalized_product.contains(family))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_core_target_has_exactly_one_profile() {
        assert_eq!(ALL_PROFILES.len(), GameId::ALL_3DS_TARGETS.len());
        for game in GameId::ALL_3DS_TARGETS {
            let matches = ALL_PROFILES
                .iter()
                .filter(|profile| profile.game_id == game)
                .count();
            assert_eq!(matches, 1, "{game:?}");
        }
    }

    #[test]
    fn mystery_dungeon_profiles_are_kept_in_their_own_family() {
        assert_eq!(EMD1.family, GameFamily::MysteryDungeon);
        assert_eq!(EMD2.family, GameFamily::MysteryDungeon);
        assert_eq!(EOU1.family, GameFamily::AtlusEtrian);
        assert_eq!(EO4.family, GameFamily::AtlusEtrian);
        assert_eq!(EO5.family, GameFamily::AtlusEtrian);
        assert_eq!(EON.family, GameFamily::AtlusEtrian);
    }

    #[test]
    fn verified_atlus_eo_identities_auto_detect_without_claiming_parser_support() {
        let eo4: TitleId = "00040000000BD300".parse().unwrap();
        let eo5: TitleId = "00040000001C5100".parse().unwrap();
        let eon: TitleId = "00040000001D4E00".parse().unwrap();
        let eo2u: TitleId = "000400000015F200".parse().unwrap();

        assert_eq!(detect_verified_profile(Some(eo4), None), Some(&EO4));
        assert_eq!(detect_verified_profile(Some(eo5), None), Some(&EO5));
        assert_eq!(detect_verified_profile(Some(eon), None), Some(&EON));
        assert_eq!(detect_verified_profile(Some(eo2u), None), Some(&EO2U));
        assert_eq!(detect_verified_profile(None, Some("CTR-P-ASJE")), Some(&EO4));
        assert_eq!(detect_verified_profile(None, Some("CTR-P-BMZP")), Some(&EO5));
        assert_eq!(detect_verified_profile(None, Some("CTR-P-BZMJ")), Some(&EON));
        assert_eq!(detect_verified_profile(None, Some("CTR-P-BSK-EUR")), Some(&EOU1));
        assert_eq!(detect_verified_profile(None, Some("UNKNOWN")), None);

        assert_eq!(EO4.status, ProfileStatus::PlannedResearch);
        assert_eq!(EO5.status, ProfileStatus::PlannedResearch);
        assert_eq!(EON.status, ProfileStatus::PlannedResearch);
    }
}
