//! Native, read-only Nintendo 3DS ROM access for EO-TexRip.
//!
//! 0.30 replaces the external ROM-reader dependency incrementally. Every parser
//! is bounds-checked and may inspect encrypted metadata, but encrypted content is
//! never exposed as cleartext without an explicit future user-key path.

pub mod bytes;
pub mod cia;
pub mod exefs;
pub mod native;
pub mod ncch;
pub mod ncsd;
pub mod romfs;

use eo_core::{GameIdentity, TitleId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use bytes::{ByteRange, ByteReader};
pub use cia::{CiaContent, CiaHeader, CiaImage, CiaSection};
pub use exefs::{ExeFsEntry, ExeFsImage};
pub use native::NativeRom;
pub use ncch::{NcchHeader, NcchImage, NcchRegion};
pub use ncsd::{NcsdHeader, NcsdImage, NcsdPartition};
pub use romfs::{RomFsEntry, RomFsImage, RomFsLayout};

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

/// Identity information recoverable from native container metadata before a game
/// profile is selected. `eo-rom` deliberately does not depend on `eo-profiles`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RomIdentityHint {
    pub title_id: Option<TitleId>,
    pub product_code: Option<String>,
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
    #[error("malformed ROM structure: {0}")]
    Malformed(String),
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
    fn identity_hint(&self) -> Result<RomIdentityHint, RomError>;
    fn entries(&self) -> Result<Vec<RomEntry>, RomError>;
    fn read_entry(&self, virtual_path: &str) -> Result<Vec<u8>, RomError>;

    /// Read at most `max_bytes` from the start of one virtual file.
    ///
    /// The default preserves compatibility for custom readers. Native readers
    /// override this so discovery probes allocate only the requested prefix.
    fn read_entry_prefix(
        &self,
        virtual_path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, RomError> {
        let mut data = self.read_entry(virtual_path)?;
        data.truncate(max_bytes);
        Ok(data)
    }
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
