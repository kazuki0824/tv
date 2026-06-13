#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DescramblerKeyToken(Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyTokenError {
    Empty,
    TooLong { len: usize, max: usize },
}

pub const MAX_DESCRAMBLER_TOKEN_BYTES: usize = 16;

impl DescramblerKeyToken {
    pub fn try_from_bytes(bytes: Vec<u8>) -> Result<Self, DescramblerKeyTokenError> {
        if bytes.is_empty() {
            return Err(DescramblerKeyTokenError::Empty);
        }
        if bytes.len() > MAX_DESCRAMBLER_TOKEN_BYTES {
            return Err(DescramblerKeyTokenError::TooLong {
                len: bytes.len(),
                max: MAX_DESCRAMBLER_TOKEN_BYTES,
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
        assert_eq!(
            DescramblerKeyToken::try_from_bytes(vec![0x55; 17]).unwrap_err(),
            DescramblerKeyTokenError::TooLong { len: 17, max: 16 }
        );
        assert!(DescramblerKeyToken::try_from_bytes(vec![0x01; 16]).is_ok());
    }
}
