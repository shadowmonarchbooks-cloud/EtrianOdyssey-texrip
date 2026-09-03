use crate::{
    ArchiveError, ArchiveInventory, ArchiveKind, ArchiveParser, EplParser, ExtractionBudget,
    FarcParser,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeArchiveRegistry;

impl NativeArchiveRegistry {
    pub fn detect(&self, data: &[u8]) -> Option<ArchiveKind> {
        let farc = FarcParser;
        if farc.probe(data) {
            return Some(ArchiveKind::Farc);
        }

        let epl = EplParser;
        if epl.probe(data) {
            return Some(ArchiveKind::Epl);
        }
        None
    }

    pub fn inspect(
        &self,
        data: &[u8],
        budget: ExtractionBudget,
    ) -> Result<Option<ArchiveInventory>, ArchiveError> {
        match self.detect(data) {
            Some(ArchiveKind::Farc) => FarcParser.inspect(data, budget).map(Some),
            Some(ArchiveKind::Epl) => EplParser.inspect(data, budget).map(Some),
            Some(_) | None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_data_stays_unknown() {
        let registry = NativeArchiveRegistry;
        assert_eq!(registry.detect(b"not an archive"), None);
        assert_eq!(
            registry.inspect(b"not an archive", ExtractionBudget::default()),
            Ok(None)
        );
    }

    #[test]
    fn hpi_index_is_not_misrepresented_as_single_buffer_archive() {
        let mut hpi = vec![0u8; 0x18];
        hpi[0..4].copy_from_slice(b"HPIH");
        let registry = NativeArchiveRegistry;
        assert_eq!(registry.detect(&hpi), None);
    }

    #[test]
    fn farc_magic_has_unambiguous_registry_detection() {
        let mut farc = vec![0u8; 0x34];
        farc[0..4].copy_from_slice(b"FARC");
        assert_eq!(
            NativeArchiveRegistry.detect(&farc),
            Some(ArchiveKind::Farc)
        );
    }
}
