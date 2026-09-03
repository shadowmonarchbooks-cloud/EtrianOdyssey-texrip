//! Format-agnostic ROM access contracts.
//!
//! 0.20 intentionally defines no NCSD/CIA/NCCH/RomFS parser. Later milestones
//! implement these contracts without changing the application-facing model.

use eo_core::GameIdentity;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RomImageKind {
    Ncsd,
    Cia,
    Cxi,
    Ncch,
    ExtractedRomFs,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RomMetadata {
    pub kind: RomImageKind,
    pub game: Option<GameIdentity>,
    pub decrypted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RomEntry {
    pub virtual_path: String,
    pub size: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RomError {
    #[error("invalid or unsupported ROM header")]
    InvalidHeader,
    #[error("encrypted ROM input requires user-supplied keys")]
    EncryptedInput,
    #[error("ROM offset or size is outside the source image")]
    InvalidOffset,
    #[error("unsafe ROM path rejected: {0}")]
    UnsafePath(String),
    #[error("ROM entry does not exist: {0}")]
    MissingEntry(String),
    #[error("ROM feature is not implemented for this image: {0}")]
    Unsupported(String),
    #[error("ROM I/O failed: {0}")]
    Io(String),
}

/// Read-only application boundary for a decrypted 3DS ROM source.
pub trait RomReader {
    fn metadata(&self) -> Result<RomMetadata, RomError>;
    fn entries(&self) -> Result<Vec<RomEntry>, RomError>;
    fn read_entry(&self, virtual_path: &str) -> Result<Vec<u8>, RomError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_kinds_are_data_contracts_not_parser_selection_guesses() {
        let meta = RomMetadata {
            kind: RomImageKind::Cxi,
            game: None,
            decrypted: true,
        };
        assert_eq!(meta.kind, RomImageKind::Cxi);
        assert!(meta.game.is_none());
    }
}
