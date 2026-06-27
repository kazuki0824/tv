#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerPid(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceFilterRef {
    pub filter_id: i32,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DemuxInputRef {
    pub demux_id: i32,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DescramblerPidSource {
    SourceFilter(SourceFilterRef),
    DemuxInput(DemuxInputRef),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerPidClaim {
    pid: DescramblerPid,
    source: DescramblerPidSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerPidClaimError {
    InvalidPid,
}

impl DescramblerPidClaim {
    pub fn from_source_filter(
        pid: u16,
        filter_id: i32,
        generation: u64,
    ) -> Result<Self, DescramblerPidClaimError> {
        if pid > 0x1fff {
            return Err(DescramblerPidClaimError::InvalidPid);
        }
        Ok(Self {
            pid: DescramblerPid(pid),
            source: DescramblerPidSource::SourceFilter(SourceFilterRef {
                filter_id,
                generation,
            }),
        })
    }

    pub fn from_demux_input(
        pid: u16,
        demux_id: i32,
        generation: u64,
    ) -> Result<Self, DescramblerPidClaimError> {
        if pid > 0x1fff {
            return Err(DescramblerPidClaimError::InvalidPid);
        }
        Ok(Self {
            pid: DescramblerPid(pid),
            source: DescramblerPidSource::DemuxInput(DemuxInputRef {
                demux_id,
                generation,
            }),
        })
    }

    pub fn pid(&self) -> DescramblerPid {
        self.pid
    }
    pub fn source_filter_ref(&self) -> Option<SourceFilterRef> {
        match self.source {
            DescramblerPidSource::SourceFilter(source) => Some(source),
            DescramblerPidSource::DemuxInput(_) => None,
        }
    }
    pub fn demux_input(&self) -> Option<DemuxInputRef> {
        match self.source {
            DescramblerPidSource::SourceFilter(_) => None,
            DescramblerPidSource::DemuxInput(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demux_input_claim_keeps_demux_generation() {
        let claim = DescramblerPidClaim::from_demux_input(100, 2, 9).unwrap();
        assert_eq!(claim.pid(), DescramblerPid(100));
        assert_eq!(
            claim.demux_input(),
            Some(DemuxInputRef {
                demux_id: 2,
                generation: 9
            })
        );
        assert_eq!(claim.source_filter_ref(), None);
    }

    #[test]
    fn pid_claim_keeps_source_filter_generation() {
        let claim = DescramblerPidClaim::from_source_filter(100, 4, 9).unwrap();
        assert_eq!(claim.pid(), DescramblerPid(100));
        assert_eq!(
            claim.source_filter_ref(),
            Some(SourceFilterRef {
                filter_id: 4,
                generation: 9
            })
        );
    }
}
