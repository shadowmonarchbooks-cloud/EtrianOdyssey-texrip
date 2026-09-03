//! Stable domain contracts for the independent EO-TexRip application.
//!
//! This crate deliberately contains no ROM/container parser logic. 0.20 defines
//! identities, texture storage semantics, evidence confidence, project records,
//! and validation rules that later parser/profile crates must obey.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("asset id is empty or contains unsupported characters: {0:?}")]
    InvalidAssetId(String),
    #[error("invalid 3DS Title ID: {0:?}")]
    InvalidTitleId(String),
    #[error("invalid runtime texture hash: {0:?}")]
    InvalidRuntimeHash(String),
    #[error("invalid texture dimensions {width}x{height}")]
    InvalidTextureDimensions { width: u32, height: u32 },
    #[error("unsupported PICA200 texture format value 0x{0:02X}")]
    UnsupportedTextureFormat(u8),
    #[error("project schema {found} is unsupported; expected {expected}")]
    UnsupportedProjectSchema { found: u32, expected: u32 },
    #[error("duplicate asset id in project: {0}")]
    DuplicateAssetId(AssetId),
    #[error("verified runtime hash {hash} is assigned to both {first} and {second}")]
    ConflictingVerifiedHash {
        hash: RuntimeHash,
        first: AssetId,
        second: AssetId,
    },
    #[error("asset {asset} belongs to {asset_game:?}, but project is {project_game:?}")]
    AssetGameMismatch {
        asset: AssetId,
        asset_game: GameId,
        project_game: GameId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(String);

impl AssetId {
    pub fn new(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'));
        if !valid {
            return Err(CoreError::InvalidAssetId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for AssetId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AssetId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TitleId(u64);

impl TitleId {
    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TitleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self.0)
    }
}

impl FromStr for TitleId {
    type Err = CoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let raw = value.trim();
        if raw.len() != 16 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::InvalidTitleId(value.to_owned()));
        }
        u64::from_str_radix(raw, 16)
            .map(Self)
            .map_err(|_| CoreError::InvalidTitleId(value.to_owned()))
    }
}

impl Serialize for TitleId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TitleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeHash(u64);

impl RuntimeHash {
    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for RuntimeHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self.0)
    }
}

impl FromStr for RuntimeHash {
    type Err = CoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let raw = value.trim();
        if raw.is_empty() || raw.len() > 16 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::InvalidRuntimeHash(value.to_owned()));
        }
        u64::from_str_radix(raw, 16)
            .map(Self)
            .map_err(|_| CoreError::InvalidRuntimeHash(value.to_owned()))
    }
}

impl Serialize for RuntimeHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for RuntimeHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GameId {
    EtrianOdysseyIv,
    EtrianOdysseyUntold,
    EtrianOdyssey2Untold,
    EtrianOdysseyV,
    EtrianOdysseyNexus,
    EtrianMysteryDungeon,
    EtrianMysteryDungeon2,
}

impl GameId {
    pub const ALL_3DS_TARGETS: [Self; 7] = [
        Self::EtrianOdysseyIv,
        Self::EtrianOdysseyUntold,
        Self::EtrianOdyssey2Untold,
        Self::EtrianOdysseyV,
        Self::EtrianOdysseyNexus,
        Self::EtrianMysteryDungeon,
        Self::EtrianMysteryDungeon2,
    ];

    pub const fn family(self) -> GameFamily {
        match self {
            Self::EtrianMysteryDungeon | Self::EtrianMysteryDungeon2 => GameFamily::MysteryDungeon,
            _ => GameFamily::AtlusEtrian,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GameFamily {
    AtlusEtrian,
    MysteryDungeon,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GameRegion {
    Japan,
    NorthAmerica,
    EuropeAustralia,
    Korea,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameIdentity {
    pub game_id: GameId,
    pub region: GameRegion,
    pub title_id: Option<TitleId>,
    pub product_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TextureFormat {
    Rgba8 = 0x0,
    Rgb8 = 0x1,
    Rgba5551 = 0x2,
    Rgb565 = 0x3,
    Rgba4 = 0x4,
    La8 = 0x5,
    Hilo8 = 0x6,
    L8 = 0x7,
    A8 = 0x8,
    La4 = 0x9,
    L4 = 0xA,
    A4 = 0xB,
    Etc1 = 0xC,
    Etc1A4 = 0xD,
}

impl TextureFormat {
    pub const ALL: [Self; 14] = [
        Self::Rgba8,
        Self::Rgb8,
        Self::Rgba5551,
        Self::Rgb565,
        Self::Rgba4,
        Self::La8,
        Self::Hilo8,
        Self::L8,
        Self::A8,
        Self::La4,
        Self::L4,
        Self::A4,
        Self::Etc1,
        Self::Etc1A4,
    ];

    pub const fn bits_per_pixel(self) -> u32 {
        match self {
            Self::Rgba8 => 32,
            Self::Rgb8 => 24,
            Self::Rgba5551 | Self::Rgb565 | Self::Rgba4 | Self::La8 | Self::Hilo8 => 16,
            Self::L8 | Self::A8 | Self::La4 | Self::Etc1A4 => 8,
            Self::L4 | Self::A4 | Self::Etc1 => 4,
        }
    }

    pub const fn stores_alpha(self) -> bool {
        matches!(
            self,
            Self::Rgba8
                | Self::Rgba5551
                | Self::Rgba4
                | Self::La8
                | Self::A8
                | Self::La4
                | Self::A4
                | Self::Etc1A4
        )
    }
}

impl TryFrom<u8> for TextureFormat {
    type Error = CoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|format| *format as u8 == value)
            .ok_or(CoreError::UnsupportedTextureFormat(value))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextureDimensions {
    pub visible_width: u32,
    pub visible_height: u32,
    pub storage_width: u32,
    pub storage_height: u32,
}

impl TextureDimensions {
    pub fn new(width: u32, height: u32) -> Result<Self, CoreError> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(CoreError::InvalidTextureDimensions { width, height });
        }
        let storage_width = width.div_ceil(8) * 8;
        let storage_height = height.div_ceil(8) * 8;
        Ok(Self {
            visible_width: width,
            visible_height: height,
            storage_width,
            storage_height,
        })
    }

    pub fn encoded_base_size(self, format: TextureFormat) -> u64 {
        let pixels = u64::from(self.storage_width) * u64::from(self.storage_height);
        pixels * u64::from(format.bits_per_pixel()) / 8
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TextureRole {
    Color,
    Alpha,
    Mask,
    Normal,
    Specular,
    Emissive,
    Ui,
    Icon,
    Map,
    Font,
    Effect,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidence {
    Candidate,
    Structural,
    RuntimeVerified,
    UserVerified,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeHashEvidence {
    pub hash: RuntimeHash,
    pub confidence: EvidenceConfidence,
    pub method: String,
}

impl RuntimeHashEvidence {
    pub fn is_verified(&self) -> bool {
        self.confidence >= EvidenceConfidence::RuntimeVerified
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocator {
    /// ROM-internal virtual path. This is provenance only and never asset identity.
    pub virtual_path: Option<String>,
    /// Ordered container nesting, e.g. ["HPI/HPB", "ATBC", "BCH"].
    pub container_chain: Vec<String>,
    pub byte_offset: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetClassification {
    pub category: String,
    pub role: TextureRole,
    pub confidence: EvidenceConfidence,
    pub reason: String,
    pub user_override: bool,
}

impl Default for AssetClassification {
    fn default() -> Self {
        Self {
            category: "unknown".to_owned(),
            role: TextureRole::Unknown,
            confidence: EvidenceConfidence::Candidate,
            reason: "unclassified".to_owned(),
            user_override: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserMetadata {
    pub friendly_name: Option<String>,
    pub category_override: Option<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextureAsset {
    /// Stable identity. Never derive this from friendly filename/category.
    pub id: AssetId,
    pub game_id: GameId,
    pub dimensions: TextureDimensions,
    pub format: TextureFormat,
    pub mip_level: u8,
    pub internal_name: Option<String>,
    pub source: SourceLocator,
    pub classification: AssetClassification,
    pub runtime_hashes: Vec<RuntimeHashEvidence>,
    pub user: UserMetadata,
}

impl TextureAsset {
    pub fn verified_runtime_hashes(&self) -> impl Iterator<Item = RuntimeHash> + '_ {
        self.runtime_hashes
            .iter()
            .filter(|evidence| evidence.is_verified())
            .map(|evidence| evidence.hash)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub application_version: String,
    pub game: GameIdentity,
    pub assets: Vec<TextureAsset>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl ProjectManifest {
    pub fn new(game: GameIdentity) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            application_version: APPLICATION_VERSION.to_owned(),
            game,
            assets: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.schema_version != PROJECT_SCHEMA_VERSION {
            return Err(CoreError::UnsupportedProjectSchema {
                found: self.schema_version,
                expected: PROJECT_SCHEMA_VERSION,
            });
        }

        let mut ids = BTreeSet::new();
        let mut verified_hash_owners: BTreeMap<RuntimeHash, AssetId> = BTreeMap::new();
        for asset in &self.assets {
            if asset.game_id != self.game.game_id {
                return Err(CoreError::AssetGameMismatch {
                    asset: asset.id.clone(),
                    asset_game: asset.game_id,
                    project_game: self.game.game_id,
                });
            }
            if !ids.insert(asset.id.clone()) {
                return Err(CoreError::DuplicateAssetId(asset.id.clone()));
            }
            for hash in asset.verified_runtime_hashes() {
                if let Some(first) = verified_hash_owners.insert(hash, asset.id.clone()) {
                    if first != asset.id {
                        return Err(CoreError::ConflictingVerifiedHash {
                            hash,
                            first,
                            second: asset.id.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_asset(id: &str, hash: &str, confidence: EvidenceConfidence) -> TextureAsset {
        TextureAsset {
            id: AssetId::new(id).unwrap(),
            game_id: GameId::EtrianOdysseyUntold,
            dimensions: TextureDimensions::new(13, 17).unwrap(),
            format: TextureFormat::Etc1,
            mip_level: 0,
            internal_name: Some("rom_texture_name".to_owned()),
            source: SourceLocator::default(),
            classification: AssetClassification::default(),
            runtime_hashes: vec![RuntimeHashEvidence {
                hash: hash.parse().unwrap(),
                confidence,
                method: "test".to_owned(),
            }],
            user: UserMetadata::default(),
        }
    }

    #[test]
    fn all_seven_3ds_targets_have_stable_game_ids() {
        assert_eq!(GameId::ALL_3DS_TARGETS.len(), 7);
        assert_eq!(GameId::EtrianMysteryDungeon.family(), GameFamily::MysteryDungeon);
        assert_eq!(GameId::EtrianOdysseyNexus.family(), GameFamily::AtlusEtrian);
    }

    #[test]
    fn pica_storage_dimensions_are_distinct_from_visible_dimensions() {
        let dims = TextureDimensions::new(13, 17).unwrap();
        assert_eq!((dims.visible_width, dims.visible_height), (13, 17));
        assert_eq!((dims.storage_width, dims.storage_height), (16, 24));
        assert_eq!(dims.encoded_base_size(TextureFormat::Etc1), 192);
        assert_eq!(dims.encoded_base_size(TextureFormat::Etc1A4), 384);
        assert_eq!(dims.encoded_base_size(TextureFormat::Rgba8), 1536);
        assert_eq!(dims.encoded_base_size(TextureFormat::L4), 192);
    }

    #[test]
    fn runtime_hash_serializes_as_normalized_hex_not_integer() {
        let hash: RuntimeHash = "abc".parse().unwrap();
        assert_eq!(hash.to_string(), "0000000000000ABC");
        assert_eq!(serde_json::to_string(&hash).unwrap(), "\"0000000000000ABC\"");
        let round_trip: RuntimeHash = serde_json::from_str("\"ABC\"").unwrap();
        assert_eq!(round_trip, hash);
    }

    #[test]
    fn user_rename_does_not_change_stable_asset_identity() {
        let mut asset = sample_asset(
            "tex:0001",
            "1111111111111111",
            EvidenceConfidence::RuntimeVerified,
        );
        let id = asset.id.clone();
        asset.user.friendly_name = Some("my-upscaled-monkey".to_owned());
        asset.user.category_override = Some("monsters/boss".to_owned());
        assert_eq!(asset.id, id);
    }

    #[test]
    fn project_rejects_duplicate_asset_ids() {
        let game = GameIdentity {
            game_id: GameId::EtrianOdysseyUntold,
            region: GameRegion::NorthAmerica,
            title_id: Some("00040000000EC700".parse().unwrap()),
            product_code: None,
        };
        let mut project = ProjectManifest::new(game);
        project.assets.push(sample_asset(
            "tex:0001",
            "1111111111111111",
            EvidenceConfidence::Candidate,
        ));
        project.assets.push(sample_asset(
            "tex:0001",
            "2222222222222222",
            EvidenceConfidence::Candidate,
        ));
        assert!(matches!(
            project.validate(),
            Err(CoreError::DuplicateAssetId(_))
        ));
    }

    #[test]
    fn project_rejects_conflicting_verified_runtime_hashes_but_allows_candidates() {
        let game = GameIdentity {
            game_id: GameId::EtrianOdysseyUntold,
            region: GameRegion::NorthAmerica,
            title_id: None,
            product_code: None,
        };
        let mut project = ProjectManifest::new(game.clone());
        project.assets.push(sample_asset(
            "tex:0001",
            "1111111111111111",
            EvidenceConfidence::RuntimeVerified,
        ));
        project.assets.push(sample_asset(
            "tex:0002",
            "1111111111111111",
            EvidenceConfidence::RuntimeVerified,
        ));
        assert!(matches!(
            project.validate(),
            Err(CoreError::ConflictingVerifiedHash { .. })
        ));

        let mut candidates = ProjectManifest::new(game);
        candidates.assets.push(sample_asset(
            "tex:0001",
            "1111111111111111",
            EvidenceConfidence::Candidate,
        ));
        candidates.assets.push(sample_asset(
            "tex:0002",
            "1111111111111111",
            EvidenceConfidence::Candidate,
        ));
        assert_eq!(candidates.validate(), Ok(()));
    }
}
