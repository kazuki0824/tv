//! DVB explicit scan request断片。
//!
//! 製品scan candidate表はTISが所有する。このhelperは、DVB backendが日本向けscan rangeを独自展開しない規則だけを保持する。

use maleicacid_tuner_hal2_common::{
    FrontendScanMode, FrontendSystem, FrontendTuneRequest, HalError,
};

pub fn dvb_scan_requests(
    base: &FrontendTuneRequest,
    scan_mode: FrontendScanMode,
) -> Result<Vec<FrontendTuneRequest>, HalError> {
    if matches!(scan_mode, FrontendScanMode::Blind) {
        return Err(HalError::Unsupported(
            "DVB backend does not provide BLIND_SCAN; TIS owns the Japanese scan SSOT",
        ));
    }
    let candidate = base.clone().normalized_for_non_blind_operation();
    match base.system {
        FrontendSystem::IsdbT | FrontendSystem::IsdbS => Ok(vec![candidate]),
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => Err(HalError::Unsupported(
            "systems outside Japanese ISDB-T/ISDB-S are outside r51 scope",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{FrontendStreamIdKind, FrontendSystem};

    #[test]
    fn non_blind_scan_ignores_range_terminus() {
        let base = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: Some(479_142_857),
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        let candidates = dvb_scan_requests(&base, FrontendScanMode::Auto).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].end_frequency, None);
    }

    #[test]
    fn explicit_bs_candidate_is_returned_as_is() {
        let base = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        };
        assert_eq!(
            dvb_scan_requests(&base, FrontendScanMode::Auto).unwrap(),
            vec![base]
        );
    }
}
