use crate::{
    ByteReader, NcchImage, NcsdImage, RomEntry, RomError, RomFsEntry, RomFsImage, RomImageKind,
    RomMetadata, RomReader,
};

pub enum NativeRom<'a> {
    Ncsd(NcsdImage<'a>),
    Ncch(NcchImage<'a>),
    RomFs(RomFsImage<'a>),
}

impl<'a> NativeRom<'a> {
    pub fn detect(data: &'a [u8]) -> Result<Self, RomError> {
        let reader = ByteReader::new(data);
        if reader.len() >= 0x104 && reader.bytes(0x100, 4)? == b"NCSD" {
            return Ok(Self::Ncsd(NcsdImage::parse(data)?));
        }
        if reader.len() >= 0x104 && reader.bytes(0x100, 4)? == b"NCCH" {
            return Ok(Self::Ncch(NcchImage::parse(data)?));
        }
        if reader.len() >= 4 && reader.bytes(0, 4)? == b"IVFC" {
            return Ok(Self::RomFs(RomFsImage::parse(data)?));
        }
        Err(RomError::InvalidHeader)
    }

    pub fn romfs_entries(&self) -> Result<Vec<RomFsEntry>, RomError> {
        match self {
            Self::Ncsd(image) => {
                let ncch = NcchImage::parse(image.partition_bytes(0)?)?;
                let romfs = ncch
                    .romfs_bytes()?
                    .ok_or_else(|| RomError::MissingEntry("RomFS".to_owned()))?;
                RomFsImage::parse(romfs)?.entries()
            }
            Self::Ncch(image) => {
                let romfs = image
                    .romfs_bytes()?
                    .ok_or_else(|| RomError::MissingEntry("RomFS".to_owned()))?;
                RomFsImage::parse(romfs)?.entries()
            }
            Self::RomFs(image) => image.entries(),
        }
    }

    fn read_romfs_entry(&self, target: &str) -> Result<Vec<u8>, RomError> {
        match self {
            Self::Ncsd(image) => {
                let ncch = NcchImage::parse(image.partition_bytes(0)?)?;
                let romfs_data = ncch
                    .romfs_bytes()?
                    .ok_or_else(|| RomError::MissingEntry("RomFS".to_owned()))?;
                read_from_romfs(romfs_data, target)
            }
            Self::Ncch(image) => {
                let romfs_data = image
                    .romfs_bytes()?
                    .ok_or_else(|| RomError::MissingEntry("RomFS".to_owned()))?;
                read_from_romfs(romfs_data, target)
            }
            Self::RomFs(image) => {
                let entry = image
                    .entries()?
                    .into_iter()
                    .find(|entry| entry.virtual_path == target)
                    .ok_or_else(|| RomError::MissingEntry(target.to_owned()))?;
                Ok(image.read_entry(&entry)?.to_vec())
            }
        }
    }
}

impl RomReader for NativeRom<'_> {
    fn metadata(&self) -> Result<RomMetadata, RomError> {
        match self {
            Self::Ncsd(image) => {
                let ncch = NcchImage::parse(image.partition_bytes(0)?)?;
                Ok(RomMetadata {
                    kind: RomImageKind::Ncsd,
                    game: None,
                    decrypted: ncch.header.no_crypto,
                })
            }
            Self::Ncch(image) => Ok(RomMetadata {
                kind: image.header.image_kind(),
                game: None,
                decrypted: image.header.no_crypto,
            }),
            Self::RomFs(_) => Ok(RomMetadata {
                kind: RomImageKind::ExtractedRomFs,
                game: None,
                decrypted: true,
            }),
        }
    }

    fn entries(&self) -> Result<Vec<RomEntry>, RomError> {
        Ok(self
            .romfs_entries()?
            .into_iter()
            .map(|entry| RomEntry {
                virtual_path: entry.virtual_path,
                size: entry.size,
            })
            .collect())
    }

    fn read_entry(&self, virtual_path: &str) -> Result<Vec<u8>, RomError> {
        self.read_romfs_entry(virtual_path)
    }
}

fn read_from_romfs(data: &[u8], target: &str) -> Result<Vec<u8>, RomError> {
    let romfs = RomFsImage::parse(data)?;
    let entry = romfs
        .entries()?
        .into_iter()
        .find(|entry| entry.virtual_path == target)
        .ok_or_else(|| RomError::MissingEntry(target.to_owned()))?;
    Ok(romfs.read_entry(&entry)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_container_without_guessing() {
        assert!(matches!(
            NativeRom::detect(&[0u8; 0x300]),
            Err(RomError::InvalidHeader)
        ));
    }
}
