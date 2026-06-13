#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerPid(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceFilterRef {
    pub filter_id: i32,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerPidClaim {
    pid: DescramblerPid,
    source_filter: SourceFilterRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerPidClaimError {
    NullSourceFilterUnsupported,
    InvalidPid,
}

impl DescramblerPidClaim {
    pub fn from_source_filter(pid: u16, filter_id: i32, generation: u64) -> Result<Self, DescramblerPidClaimError> {
        if pid > 0x1fff {
            return Err(DescramblerPidClaimError::InvalidPid);
        }
        Ok(Self { pid: DescramblerPid(pid), source_filter: SourceFilterRef { filter_id, generation } })
    }

    pub fn reject_null_source_filter(_pid: u16) -> Result<Self, DescramblerPidClaimError> {
        Err(DescramblerPidClaimError::NullSourceFilterUnsupported)
    }

    pub fn pid(&self) -> DescramblerPid { self.pid }
    pub fn source_filter(&self) -> SourceFilterRef { self.source_filter }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_source_filter_is_not_current_runtime_target() {
        assert_eq!(DescramblerPidClaim::reject_null_source_filter(100).unwrap_err(), DescramblerPidClaimError::NullSourceFilterUnsupported);
    }

    #[test]
    fn pid_claim_keeps_source_filter_generation() {
        let claim = DescramblerPidClaim::from_source_filter(100, 4, 9).unwrap();
        assert_eq!(claim.pid(), DescramblerPid(100));
        assert_eq!(claim.source_filter(), SourceFilterRef { filter_id: 4, generation: 9 });
    }
}
