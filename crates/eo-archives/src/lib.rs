//! Native archive/container inspection shared by every supported game profile.
//!
//! Parsers expose bounded metadata and member reads. They never write archive
//! paths directly to disk; workspace path policy and recursive extraction live
//! above this crate.

mod bytes;
pub mod epl;
pub mod farc;
pub mod hpi_hpb;
pub mod registry;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use epl::EplParser;
pub use farc::FarcParser;
pub use hpi_hpb::HpiHpbParser;
pub use registry::NativeArchiveRegistry;

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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionUsage {
    pub max_depth_seen: u16,
    pub members: u64,
    pub expanded_bytes: u64,
}

impl ExtractionUsage {
    /// Charge one inspected archive to the cumulative recursive-extraction budget.
    /// Depth zero is the first archive; nested archives increment depth by one.
    /// The update is transactional: a rejected inventory leaves usage unchanged.
    pub fn charge_inventory(
        &mut self,
        depth: u16,
        inventory: &ArchiveInventory,
        budget: ExtractionBudget,
    ) -> Result<(), ArchiveError> {
        if depth > budget.max_depth {
            return Err(ArchiveError::BudgetExceeded(format!(
                "archive depth {depth} exceeds {}",
                budget.max_depth
            )));
        }
        let added_members = inventory.members.len() as u64;
        let next_members = self
            .members
            .checked_add(added_members)
            .ok_or_else(|| ArchiveError::BudgetExceeded("member count overflow".to_owned()))?;
        if next_members > budget.max_members {
            return Err(ArchiveError::BudgetExceeded(format!(
                "cumulative member count {next_members} exceeds {}",
                budget.max_members
            )));
        }

        let added_expanded = inventory.members.iter().try_fold(0u64, |total, member| {
            total
                .checked_add(member.expanded_size.unwrap_or(member.stored_size))
                .ok_or_else(|| ArchiveError::BudgetExceeded("expanded byte count overflow".to_owned()))
        })?;
        let next_expanded = self
            .expanded_bytes
            .checked_add(added_expanded)
            .ok_or_else(|| ArchiveError::BudgetExceeded("expanded byte count overflow".to_owned()))?;
        if next_expanded > budget.max_expanded_bytes {
            return Err(ArchiveError::BudgetExceeded(format!(
                "cumulative expanded bytes {next_expanded} exceed {}",
                budget.max_expanded_bytes
            )));
        }

        self.max_depth_seen = self.max_depth_seen.max(depth);
        self.members = next_members;
        self.expanded_bytes = next_expanded;
        Ok(())
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
    #[error("archive member name is invalid: {0}")]
    InvalidName(String),
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
    fn inspect(
        &self,
        data: &[u8],
        budget: ExtractionBudget,
    ) -> Result<ArchiveInventory, ArchiveError>;
    fn read_member(
        &self,
        data: &[u8],
        member: &ArchiveMember,
        budget: ExtractionBudget,
    ) -> Result<Vec<u8>, ArchiveError>;
}

pub(crate) fn enforce_archive_budget(
    archive_bytes: u64,
    budget: ExtractionBudget,
) -> Result<(), ArchiveError> {
    if archive_bytes > budget.max_archive_bytes {
        return Err(ArchiveError::BudgetExceeded(format!(
            "archive size {archive_bytes} exceeds {}",
            budget.max_archive_bytes
        )));
    }
    Ok(())
}

pub(crate) fn enforce_inventory_budget(
    members: &[ArchiveMember],
    budget: ExtractionBudget,
) -> Result<(), ArchiveError> {
    let count = members.len() as u64;
    if count > budget.max_members {
        return Err(ArchiveError::BudgetExceeded(format!(
            "member count {count} exceeds {}",
            budget.max_members
        )));
    }

    let mut total_expanded = 0u64;
    for member in members {
        let expanded = member.expanded_size.unwrap_or(member.stored_size);
        if member.stored_size > budget.max_member_bytes || expanded > budget.max_member_bytes {
            return Err(ArchiveError::BudgetExceeded(format!(
                "member {} size exceeds {}",
                member.index, budget.max_member_bytes
            )));
        }
        total_expanded = total_expanded
            .checked_add(expanded)
            .ok_or_else(|| ArchiveError::BudgetExceeded("expanded byte count overflow".to_owned()))?;
        if total_expanded > budget.max_expanded_bytes {
            return Err(ArchiveError::BudgetExceeded(format!(
                "expanded bytes {total_expanded} exceed {}",
                budget.max_expanded_bytes
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(expanded_sizes: &[u64]) -> ArchiveInventory {
        ArchiveInventory {
            kind: ArchiveKind::Farc,
            members: expanded_sizes
                .iter()
                .enumerate()
                .map(|(index, size)| ArchiveMember {
                    index: index as u64,
                    name: None,
                    offset: 0,
                    stored_size: *size,
                    expanded_size: Some(*size),
                })
                .collect(),
        }
    }

    #[test]
    fn default_budget_is_bounded() {
        let budget = ExtractionBudget::default();
        assert!(budget.max_depth > 0);
        assert!(budget.max_members < u64::MAX);
        assert!(budget.max_member_bytes <= budget.max_expanded_bytes);
    }

    #[test]
    fn inventory_budget_counts_expanded_bytes_without_allocating_payloads() {
        let members = inventory(&[10, 11]).members;
        let budget = ExtractionBudget {
            max_expanded_bytes: 20,
            ..ExtractionBudget::default()
        };
        assert!(matches!(
            enforce_inventory_budget(&members, budget),
            Err(ArchiveError::BudgetExceeded(_))
        ));
    }

    #[test]
    fn archive_size_budget_is_checked_before_parser_work() {
        let budget = ExtractionBudget {
            max_archive_bytes: 3,
            ..ExtractionBudget::default()
        };
        assert!(matches!(
            enforce_archive_budget(4, budget),
            Err(ArchiveError::BudgetExceeded(_))
        ));
    }

    #[test]
    fn recursive_usage_is_cumulative_and_transactional() {
        let budget = ExtractionBudget {
            max_depth: 1,
            max_members: 3,
            max_expanded_bytes: 12,
            ..ExtractionBudget::default()
        };
        let mut usage = ExtractionUsage::default();
        usage.charge_inventory(0, &inventory(&[4, 4]), budget).unwrap();
        assert_eq!(usage.members, 2);
        assert_eq!(usage.expanded_bytes, 8);

        let before = usage;
        assert!(matches!(
            usage.charge_inventory(1, &inventory(&[5]), budget),
            Err(ArchiveError::BudgetExceeded(_))
        ));
        assert_eq!(usage, before);
        assert!(matches!(
            usage.charge_inventory(2, &inventory(&[]), budget),
            Err(ArchiveError::BudgetExceeded(_))
        ));
        assert_eq!(usage, before);
    }
}
