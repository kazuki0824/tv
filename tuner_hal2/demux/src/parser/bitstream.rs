pub(crate) fn nal_to_rbsp(bytes: &[u8]) -> Result<Vec<u8>, std::collections::TryReserveError> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())?;
    let mut zero_run = 0usize;
    for &byte in bytes {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        out.push(byte);
        if byte == 0x00 {
            zero_run += 1;
        } else {
            zero_run = 0;
        }
    }
    Ok(out)
}

pub(crate) struct BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    pub(crate) fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.bit_offset / 8)?;
        let bit = 7 - (self.bit_offset % 8);
        self.bit_offset += 1;
        Some((byte >> bit) & 1)
    }

    pub(crate) fn read_bits(&mut self, count: usize) -> Option<u32> {
        if count > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Some(value)
    }

    pub(crate) fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zero_bits = 0usize;
        while self.read_bit()? == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return None;
            }
        }
        let suffix = if leading_zero_bits == 0 {
            0
        } else {
            self.read_bits(leading_zero_bits)?
        };
        Some(((1u32 << leading_zero_bits) - 1) + suffix)
    }
}
