#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DescramblerKeyToken(Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyTokenError {
    Empty,
    InvalidLength { len: usize, min: usize, max: usize },
}

pub const DESCRAMBLER_TOKEN_MIN_BYTES: usize = 1;
pub const DESCRAMBLER_TOKEN_MAX_BYTES: usize = 16;

impl DescramblerKeyToken {
    pub fn try_from_bytes(bytes: Vec<u8>) -> Result<Self, DescramblerKeyTokenError> {
        if bytes.is_empty() {
            return Err(DescramblerKeyTokenError::Empty);
        }
        if bytes.len() > DESCRAMBLER_TOKEN_MAX_BYTES {
            return Err(DescramblerKeyTokenError::InvalidLength {
                len: bytes.len(),
                min: DESCRAMBLER_TOKEN_MIN_BYTES,
                max: DESCRAMBLER_TOKEN_MAX_BYTES,
            });
        }
        Ok(Self(bytes))
    }

    pub fn as_binder_token_bytes(&self) -> &[u8] {
        &self.0
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
        assert!(DescramblerKeyToken::try_from_bytes(vec![0x55; 1]).is_ok());
        assert_eq!(
            DescramblerKeyToken::try_from_bytes(vec![0x55; 17]).unwrap_err(),
            DescramblerKeyTokenError::InvalidLength {
                len: 17,
                min: 1,
                max: 16
            }
        );
        assert!(DescramblerKeyToken::try_from_bytes(vec![0x01; 8]).is_ok());
        assert!(DescramblerKeyToken::try_from_bytes(vec![0x01; 16]).is_ok());
    }
}
