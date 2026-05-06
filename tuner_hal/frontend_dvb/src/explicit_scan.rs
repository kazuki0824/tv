use maleicacid_tuner_hal_common::{FrontendScanMode, FrontendSystem, FrontendTuneRequest, HalError};

pub fn dvb_scan_requests(base: &FrontendTuneRequest, scan_mode: FrontendScanMode) -> Result<Vec<FrontendTuneRequest>, HalError> {
    if matches!(scan_mode, FrontendScanMode::Blind) {
        return Err(HalError::Unsupported("DVB backend does not provide BLIND_SCAN; TIS owns the Japanese scan SSOT"));
    }
    if base.end_frequency.unwrap_or(base.frequency) != base.frequency {
        return Err(HalError::Unsupported("DVB backend no longer expands Japanese scan ranges; TIS must submit explicit tune candidates"));
    }
    match base.system {
        FrontendSystem::IsdbT | FrontendSystem::IsdbS => Ok(vec![base.clone()]),
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => Err(HalError::Unsupported("日本向け ISDB-T/ISDB-S 以外の scan 表は対象外です")),
    }
}
