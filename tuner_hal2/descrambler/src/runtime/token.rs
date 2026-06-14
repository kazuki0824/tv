#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DescramblerKeyToken(Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyTokenError {
    Empty,
    InvalidLength { len: usize, expected: usize },
}

pub const DESCRAMBLER_TOKEN_BYTES: usize = 8;

impl DescramblerKeyToken {
    pub fn try_from_bytes(bytes: Vec<u8>) -> Result<Self, DescramblerKeyTokenError> {
        if bytes.is_empty() {
            return Err(DescramblerKeyTokenError::Empty);
        }
        if bytes.len() != DESCRAMBLER_TOKEN_BYTES {
            return Err(DescramblerKeyTokenError::InvalidLength {
                len: bytes.len(),
                expected: DESCRAMBLER_TOKEN_BYTES,
            });
        }
        Ok(Self(bytes))
    }

    pub fn as_binder_token_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn stable_slot_id(&self) -> u64 {
        let mut bytes = [0u8; DESCRAMBLER_TOKEN_BYTES];
        bytes.copy_from_slice(&self.0);
        u64::from_be_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_length_is_checked_at_boundary() {
        assert_eq!(
            DescramblerKeyToken::try_from_bytes(Vec::new()).unwrap_err(),
            DescramblerKeyTokenError::Empty
        );
        assert_eq!(
            DescramblerKeyToken::try_from_bytes(vec![0x55; 1]).unwrap_err(),
            DescramblerKeyTokenError::InvalidLength {
                len: 1,
                expected: 8
            }
        );
        assert_eq!(
            DescramblerKeyToken::try_from_bytes(vec![0x55; 9]).unwrap_err(),
            DescramblerKeyTokenError::InvalidLength {
                len: 9,
                expected: 8
            }
        );
        assert!(DescramblerKeyToken::try_from_bytes(vec![0x01; 8]).is_ok());
    }
}
