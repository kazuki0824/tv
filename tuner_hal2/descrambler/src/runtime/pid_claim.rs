use maleicacid_tuner_hal2_common::TransportStreamPid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerPid(TransportStreamPid);

impl DescramblerPid {
    pub(crate) fn from_validated_pid_for_descrambler_core(pid: u16) -> Self {
        Self(TransportStreamPid::validate_u16(pid).expect("validated descrambler PID"))
    }

    pub const fn to_transport_stream_pid_for_packet_pid_bridge(self) -> TransportStreamPid {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceFilterRef {
    pub filter_id: i32,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DescramblerPidClaimSource {
    DemuxInput,
    SourceFilter(SourceFilterRef),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerPidClaim {
    pid: DescramblerPid,
    source: DescramblerPidClaimSource,
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
        let pid = TransportStreamPid::validate_u16(pid)
            .map_err(|_| DescramblerPidClaimError::InvalidPid)?;
        Ok(Self {
            pid: DescramblerPid(pid),
            source: DescramblerPidClaimSource::SourceFilter(SourceFilterRef {
                filter_id,
                generation,
            }),
        })
    }

    pub fn from_demux_input(pid: u16) -> Result<Self, DescramblerPidClaimError> {
        let pid = TransportStreamPid::validate_u16(pid)
            .map_err(|_| DescramblerPidClaimError::InvalidPid)?;
        Ok(Self {
            pid: DescramblerPid(pid),
            source: DescramblerPidClaimSource::DemuxInput,
        })
    }

    pub fn pid(&self) -> DescramblerPid {
        self.pid
    }

    pub fn source(&self) -> DescramblerPidClaimSource {
        self.source
    }

    pub fn source_filter_ref(&self) -> Option<SourceFilterRef> {
        match self.source {
            DescramblerPidClaimSource::SourceFilter(source) => Some(source),
            DescramblerPidClaimSource::DemuxInput => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_descrambler_pid(pid: u16) -> DescramblerPid {
        DescramblerPidClaim::from_demux_input(pid)
            .expect("valid test PID")
            .pid()
    }

    #[test]
    fn pid_claim_keeps_source_filter_generation() {
        let claim = DescramblerPidClaim::from_source_filter(100, 4, 9).unwrap();
        assert_eq!(claim.pid(), test_descrambler_pid(100));
        assert_eq!(
            claim.source_filter_ref(),
            Some(SourceFilterRef {
                filter_id: 4,
                generation: 9
            })
        );
    }

    #[test]
    fn pid_claim_can_target_demux_input() {
        let claim = DescramblerPidClaim::from_demux_input(100).unwrap();
        assert_eq!(claim.pid(), test_descrambler_pid(100));
        assert_eq!(claim.source_filter_ref(), None);
    }
}
