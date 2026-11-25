use crate::StorageCodecError;

pub struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// # Errors
    /// Returns a codec error when the next bytes do not match the requested magic value or are truncated.
    pub fn expect_magic(&mut self, magic: [u8; 4]) -> Result<(), StorageCodecError> {
        if self.read_exact(magic.len())? == magic {
            Ok(())
        } else {
            Err(invalid("invalid column-batch magic"))
        }
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_u8(&mut self) -> Result<u8, StorageCodecError> {
        Ok(self.read_exact(1)?[0])
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_u16(&mut self) -> Result<u16, StorageCodecError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_u32(&mut self) -> Result<u32, StorageCodecError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_u64(&mut self) -> Result<u64, StorageCodecError> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_i64(&mut self) -> Result<i64, StorageCodecError> {
        Ok(i64::from_le_bytes(self.read_array()?))
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_i128(&mut self) -> Result<i128, StorageCodecError> {
        Ok(i128::from_le_bytes(self.read_array()?))
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_f64(&mut self) -> Result<f64, StorageCodecError> {
        Ok(f64::from_bits(self.read_u64()?))
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8], StorageCodecError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| invalid("column-batch offset overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("truncated column-batch record"))?;
        self.offset = end;
        Ok(value)
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_len(&mut self, maximum: usize) -> Result<usize, StorageCodecError> {
        let length = usize::try_from(self.read_u32()?)
            .map_err(|_| invalid("column-batch length overflow"))?;
        if length > maximum || length > self.remaining() {
            return Err(invalid("column-batch length exceeds limit"));
        }
        Ok(length)
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_bounded_bytes(&mut self, maximum: usize) -> Result<&'a [u8], StorageCodecError> {
        let length = self.read_len(maximum)?;
        self.read_exact(length)
    }

    /// # Errors
    /// Returns a codec error when the input is truncated, invalid, or exceeds the requested bound.
    pub fn read_bounded_string(&mut self, maximum: usize) -> Result<String, StorageCodecError> {
        let raw = self.read_bounded_bytes(maximum)?;
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| invalid("invalid column-batch UTF-8"))
    }

    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    /// # Errors
    /// Returns a codec error when unread trailing bytes remain.
    pub fn finish(self) -> Result<(), StorageCodecError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(invalid("trailing column-batch bytes"))
        }
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], StorageCodecError> {
        self.read_exact(N)?
            .try_into()
            .map_err(|_| invalid("truncated column-batch number"))
    }
}

pub fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn write_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn write_i128(out: &mut Vec<u8>, value: i128) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn write_f64(out: &mut Vec<u8>, value: f64) {
    write_u64(out, value.to_bits());
}

/// # Errors
/// Returns a codec error when the length cannot be represented as a 32-bit value.
pub fn write_len(out: &mut Vec<u8>, length: usize) -> Result<(), StorageCodecError> {
    write_u32(
        out,
        u32::try_from(length).map_err(|_| invalid("column-batch length exceeds u32"))?,
    );
    Ok(())
}

/// # Errors
/// Returns a codec error when the length cannot be represented in the format.
pub fn write_bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), StorageCodecError> {
    write_len(out, value.len())?;
    out.extend_from_slice(value);
    Ok(())
}

/// # Errors
/// Returns a codec error when the length cannot be represented in the format.
pub fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), StorageCodecError> {
    write_bytes(out, value.as_bytes())
}

#[must_use]
pub fn invalid(message: &str) -> StorageCodecError {
    StorageCodecError::InvalidEncoding(message.to_string())
}
