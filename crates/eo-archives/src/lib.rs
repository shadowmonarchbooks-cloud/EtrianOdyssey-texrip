//! Archive/container inspection contracts shared by every supported game profile.
//!
//! Parsers expose bounded metadata and member reads. They never write arbitrary
//! archive paths directly to disk; workspace policy lives above this crate.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveKind {
    HpiHpb,
    Farc,
    Epl,
    Atbc,
    Bam2,
    CgFx,
    Bch,
    Ctpk,
    Ctxb,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionBudget {
    pub max_depth: u16,
    pub max_members: u64,
    pub max_expanded_bytes: u64,
    pub max_member_bytes: u64,
    pub max_archive_bytes: u64,
}

impl Default for ExtractionBudget {
    fn default() -> Self {
        Self {
            max_depth: 16,
            max_members: 250_000,
            max_expanded_bytes: 32 * 1024 * 1024 * 1024,
            max_member_bytes: 2 * 1024 * 1024 * 1024,
            max_archive_bytes: 8 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveMember {
    pub index: u64,
    pub name: Option<String>,
    pub offset: u64,
    pub stored_size: u64,
    pub expanded_size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveInventory {
    pub kind: ArchiveKind,
    pub members: Vec<ArchiveMember>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArchiveError {
    #[error("archive header is invalid")]
    InvalidHeader,
    #[error("archive offset or length is outside the source")]
    InvalidOffset,
    #[error("archive member is truncated")]
    TruncatedMember,
    #[error("archive resource budget exceeded: {0}")]
    BudgetExceeded(String),
    #[error("archive member does not exist: {0}")]
    MissingMember(u64),
    #[error("archive revision is not supported: {0}")]
    UnsupportedRevision(String),
}

pub trait ArchiveParser {
    fn kind(&self) -> ArchiveKind;
    fn probe(&self, data: &[u8]) -> bool;
    fn inspect(&self, data: &[u8], budget: ExtractionBudget)
        -> Result<ArchiveInventory, ArchiveError>;
    fn read_member(
        &self,
        data: &[u8],
        member: &ArchiveMember,
        budget: ExtractionBudget,
    ) -> Result<Vec<u8>, ArchiveError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budget_is_bounded() {
        let budget = ExtractionBudget::default();
        assert!(budget.max_depth > 0);
        assert!(budget.max_members < u64::MAX);
        assert!(budget.max_member_bytes <= budget.max_expanded_bytes);
    }
}
