use crate::{
    ByteReader, NcchHeader, NcchImage, NcsdImage, RomEntry, RomError, RomFsEntry, RomFsImage,
    RomIdentityHint, RomImageKind, RomMetadata, RomReader,
};
use eo_core::TitleId;

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
                let ncch = primary_ncch(image)?;
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
                let ncch = primary_ncch(image)?;
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
                let ncch = primary_ncch(image)?;
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

    fn identity_hint(&self) -> Result<RomIdentityHint, RomError> {
        match self {
            Self::Ncsd(image) => identity_from_ncch(&primary_ncch(image)?.header),
            Self::Ncch(image) => identity_from_ncch(&image.header),
            Self::RomFs(_) => Ok(RomIdentityHint::default()),
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

fn primary_ncch<'a>(image: &NcsdImage<'a>) -> Result<NcchImage<'a>, RomError> {
    NcchImage::parse(image.partition_bytes(0)?)
}

fn identity_from_ncch(header: &NcchHeader) -> Result<RomIdentityHint, RomError> {
    let title_id = if header.program_id == 0 {
        None
    } else {
        let normalized = format!("{:016X}", header.program_id);
        Some(
            normalized
                .parse::<TitleId>()
                .map_err(|_| RomError::Malformed("invalid NCCH program ID".to_owned()))?,
        )
    };
    let product_code = (!header.product_code.is_empty()).then(|| header.product_code.clone());
    Ok(RomIdentityHint {
        title_id,
        product_code,
    })
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

    const NONE: u32 = 0xFFFF_FFFF;

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_utf16(data: &mut [u8], offset: usize, value: &str) -> u32 {
        let encoded = value.encode_utf16().collect::<Vec<_>>();
        for (index, unit) in encoded.iter().enumerate() {
            let start = offset + index * 2;
            data[start..start + 2].copy_from_slice(&unit.to_le_bytes());
        }
        (encoded.len() * 2) as u32
    }

    fn romfs_fixture() -> Vec<u8> {
        let mut data = vec![0u8; 0x3000];
        data[0..4].copy_from_slice(b"IVFC");
        put_u32(&mut data, 4, 0x0001_0000);
        put_u32(&mut data, 0x08, 0x20);
        put_u64(&mut data, 0x44, 0x1000);
        put_u32(&mut data, 0x4C, 12);

        let l3 = 0x1000usize;
        put_u32(&mut data, l3, 0x28);
        put_u32(&mut data, l3 + 0x04, 0x28);
        put_u32(&mut data, l3 + 0x08, 0x0C);
        put_u32(&mut data, l3 + 0x0C, 0x34);
        put_u32(&mut data, l3 + 0x10, 0x50);
        put_u32(&mut data, l3 + 0x14, 0x84);
        put_u32(&mut data, l3 + 0x18, 0x0C);
        put_u32(&mut data, l3 + 0x1C, 0x90);
        put_u32(&mut data, l3 + 0x20, 0x70);
        put_u32(&mut data, l3 + 0x24, 0x100);

        let dirs = l3 + 0x34;
        put_u32(&mut data, dirs, 0);
        put_u32(&mut data, dirs + 0x04, NONE);
        put_u32(&mut data, dirs + 0x08, 0x18);
        put_u32(&mut data, dirs + 0x0C, NONE);
        put_u32(&mut data, dirs + 0x10, NONE);
        put_u32(&mut data, dirs + 0x14, 0);

        let child = dirs + 0x18;
        put_u32(&mut data, child, 0);
        put_u32(&mut data, child + 0x04, NONE);
        put_u32(&mut data, child + 0x08, NONE);
        put_u32(&mut data, child + 0x0C, 0);
        put_u32(&mut data, child + 0x10, NONE);
        let dir_name_len = put_utf16(&mut data, child + 0x18, "data");
        put_u32(&mut data, child + 0x14, dir_name_len);

        let files = l3 + 0x90;
        put_u32(&mut data, files, 0x18);
        put_u32(&mut data, files + 0x04, NONE);
        put_u64(&mut data, files + 0x08, 0x20);
        put_u64(&mut data, files + 0x10, 4);
        put_u32(&mut data, files + 0x18, NONE);
        let file_name_len = put_utf16(&mut data, files + 0x20, "test.bin");
        put_u32(&mut data, files + 0x1C, file_name_len);
        data[l3 + 0x120..l3 + 0x124].copy_from_slice(b"EO3D");
        data
    }

    fn ncch_fixture() -> Vec<u8> {
        let romfs = romfs_fixture();
        let mut data = vec![0u8; 0x3800];
        data[0x100..0x104].copy_from_slice(b"NCCH");
        put_u32(&mut data, 0x104, 0x1C);
        put_u64(&mut data, 0x118, 0x0004_0000_000E_C700);
        data[0x150..0x15D].copy_from_slice(b"CTR-P-BSK-USA");
        data[0x18D] = 0x02;
        data[0x18F] = 0x04;
        put_u32(&mut data, 0x1B0, 4);
        put_u32(&mut data, 0x1B4, 0x18);
        data[0x800..0x3800].copy_from_slice(&romfs);
        data
    }

    fn ncsd_fixture() -> Vec<u8> {
        let ncch = ncch_fixture();
        let mut data = vec![0u8; 0x3C00];
        data[0x100..0x104].copy_from_slice(b"NCSD");
        put_u32(&mut data, 0x104, 0x1E);
        put_u32(&mut data, 0x120, 2);
        put_u32(&mut data, 0x124, 0x1C);
        data[0x400..0x3C00].copy_from_slice(&ncch);
        data
    }

    #[test]
    fn rejects_unknown_container_without_guessing() {
        assert!(matches!(
            NativeRom::detect(&[0u8; 0x300]),
            Err(RomError::InvalidHeader)
        ));
    }

    #[test]
    fn native_identity_hint_comes_from_ncch_metadata() {
        let data = ncch_fixture();
        let rom = NativeRom::detect(&data).unwrap();
        let hint = rom.identity_hint().unwrap();
        assert_eq!(hint.title_id.unwrap().to_string(), "00040000000EC700");
        assert_eq!(hint.product_code.as_deref(), Some("CTR-P-BSK-USA"));
    }

    #[test]
    fn ncsd_to_ncch_to_romfs_reads_file_without_external_tools() {
        let data = ncsd_fixture();
        let rom = NativeRom::detect(&data).unwrap();
        assert_eq!(rom.metadata().unwrap().kind, RomImageKind::Ncsd);
        assert!(rom.metadata().unwrap().decrypted);
        assert_eq!(
            rom.identity_hint().unwrap().title_id.unwrap().to_string(),
            "00040000000EC700"
        );
        let entries = rom.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].virtual_path, "data/test.bin");
        assert_eq!(rom.read_entry("data/test.bin").unwrap(), b"EO3D");
    }
}
