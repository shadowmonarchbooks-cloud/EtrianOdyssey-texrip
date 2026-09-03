use crate::ArchiveError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ByteRange {
    pub offset: u64,
    pub size: u64,
}

impl ByteRange {
    pub(crate) fn new(offset: u64, size: u64, source_len: u64) -> Result<Self, ArchiveError> {
        let end = offset.checked_add(size).ok_or(ArchiveError::InvalidOffset)?;
        if end > source_len {
            return Err(ArchiveError::InvalidOffset);
        }
        Ok(Self { offset, size })
    }

    pub(crate) fn end(self) -> u64 {
        self.offset + self.size
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ByteReader<'a> {
    data: &'a [u8],
}

impl<'a> ByteReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub(crate) fn len(self) -> u64 {
        self.data.len() as u64
    }

    pub(crate) fn bytes(self, offset: u64, size: u64) -> Result<&'a [u8], ArchiveError> {
        let range = ByteRange::new(offset, size, self.len())?;
        let start = usize::try_from(range.offset).map_err(|_| ArchiveError::InvalidOffset)?;
        let end = usize::try_from(range.end()).map_err(|_| ArchiveError::InvalidOffset)?;
        self.data.get(start..end).ok_or(ArchiveError::InvalidOffset)
    }

    pub(crate) fn array<const N: usize>(self, offset: u64) -> Result<[u8; N], ArchiveError> {
        self.bytes(offset, N as u64)?
            .try_into()
            .map_err(|_| ArchiveError::InvalidOffset)
    }

    pub(crate) fn u16_le(self, offset: u64) -> Result<u16, ArchiveError> {
        Ok(u16::from_le_bytes(self.array(offset)?))
    }

    pub(crate) fn u32_le(self, offset: u64) -> Result<u32, ArchiveError> {
        Ok(u32::from_le_bytes(self.array(offset)?))
    }

    pub(crate) fn i32_le(self, offset: u64) -> Result<i32, ArchiveError> {
        Ok(i32::from_le_bytes(self.array(offset)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_ranges_reject_overflow_and_eof() {
        assert_eq!(
            ByteRange::new(u64::MAX, 2, u64::MAX),
            Err(ArchiveError::InvalidOffset)
        );
        assert_eq!(ByteRange::new(8, 4, 10), Err(ArchiveError::InvalidOffset));
        assert_eq!(ByteRange::new(8, 2, 10).unwrap().end(), 10);
    }

    #[test]
    fn little_endian_reads_are_checked() {
        let data = [0x78, 0x56, 0x34, 0x12];
        let reader = ByteReader::new(&data);
        assert_eq!(reader.u16_le(0).unwrap(), 0x5678);
        assert_eq!(reader.u32_le(0).unwrap(), 0x1234_5678);
        assert_eq!(reader.i32_le(0).unwrap(), 0x1234_5678);
        assert_eq!(reader.u32_le(1), Err(ArchiveError::InvalidOffset));
    }
}
