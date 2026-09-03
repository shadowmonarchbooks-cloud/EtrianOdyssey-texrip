use crate::RomError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub offset: u64,
    pub size: u64,
}

impl ByteRange {
    pub fn new(offset: u64, size: u64, source_len: u64) -> Result<Self, RomError> {
        let end = offset.checked_add(size).ok_or(RomError::InvalidOffset)?;
        if end > source_len {
            return Err(RomError::InvalidOffset);
        }
        Ok(Self { offset, size })
    }

    pub fn from_units(
        offset_units: u32,
        size_units: u32,
        unit_size: u64,
        source_len: u64,
    ) -> Result<Self, RomError> {
        let offset = u64::from(offset_units)
            .checked_mul(unit_size)
            .ok_or(RomError::InvalidOffset)?;
        let size = u64::from(size_units)
            .checked_mul(unit_size)
            .ok_or(RomError::InvalidOffset)?;
        Self::new(offset, size, source_len)
    }

    pub fn end(self) -> u64 {
        self.offset + self.size
    }
}

#[derive(Clone, Copy)]
pub struct ByteReader<'a> {
    data: &'a [u8],
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn len(self) -> u64 {
        self.data.len() as u64
    }

    pub fn is_empty(self) -> bool {
        self.data.is_empty()
    }

    pub fn slice(self, range: ByteRange) -> Result<&'a [u8], RomError> {
        let checked = ByteRange::new(range.offset, range.size, self.len())?;
        let start = usize::try_from(checked.offset).map_err(|_| RomError::InvalidOffset)?;
        let end = usize::try_from(checked.end()).map_err(|_| RomError::InvalidOffset)?;
        self.data.get(start..end).ok_or(RomError::InvalidOffset)
    }

    pub fn bytes(self, offset: u64, size: u64) -> Result<&'a [u8], RomError> {
        self.slice(ByteRange::new(offset, size, self.len())?)
    }

    pub fn array<const N: usize>(self, offset: u64) -> Result<[u8; N], RomError> {
        let bytes = self.bytes(offset, N as u64)?;
        bytes.try_into().map_err(|_| RomError::InvalidOffset)
    }

    pub fn u16_le(self, offset: u64) -> Result<u16, RomError> {
        Ok(u16::from_le_bytes(self.array(offset)?))
    }

    pub fn u32_le(self, offset: u64) -> Result<u32, RomError> {
        Ok(u32::from_le_bytes(self.array(offset)?))
    }

    pub fn u64_le(self, offset: u64) -> Result<u64, RomError> {
        Ok(u64::from_le_bytes(self.array(offset)?))
    }

    pub fn u16_be(self, offset: u64) -> Result<u16, RomError> {
        Ok(u16::from_be_bytes(self.array(offset)?))
    }

    pub fn u32_be(self, offset: u64) -> Result<u32, RomError> {
        Ok(u32::from_be_bytes(self.array(offset)?))
    }

    pub fn u64_be(self, offset: u64) -> Result<u64, RomError> {
        Ok(u64::from_be_bytes(self.array(offset)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_ranges_reject_end_overflow_and_eof() {
        assert_eq!(
            ByteRange::new(u64::MAX, 2, u64::MAX),
            Err(RomError::InvalidOffset)
        );
        assert_eq!(ByteRange::new(8, 4, 10), Err(RomError::InvalidOffset));
        assert_eq!(ByteRange::new(8, 2, 10).unwrap().end(), 10);
    }

    #[test]
    fn unit_ranges_are_checked_before_slicing() {
        let range = ByteRange::from_units(2, 3, 0x200, 0x1000).unwrap();
        assert_eq!(range.offset, 0x400);
        assert_eq!(range.size, 0x600);
        assert_eq!(
            ByteRange::from_units(2, 8, 0x200, 0x1000),
            Err(RomError::InvalidOffset)
        );
    }

    #[test]
    fn integer_reads_support_both_endiannesses() {
        let data = [0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0, 0];
        let reader = ByteReader::new(&data);
        assert_eq!(reader.u16_le(0).unwrap(), 0x1234);
        assert_eq!(reader.u32_le(2).unwrap(), 0x12345678);
        assert_eq!(reader.u16_be(0).unwrap(), 0x3412);
        assert_eq!(reader.u32_be(2).unwrap(), 0x78563412);
    }
}
