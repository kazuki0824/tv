use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_domain_request::{DomainProfileSupport, RuntimeExecutableRequest};

pub fn validate_runtime_executable_request(
    request: Option<&RuntimeExecutableRequest>,
) -> Result<(), HalError> {
    let Some(request) = request else {
        return Ok(());
    };
    match request.profile_support() {
        DomainProfileSupport::Supported => request.validate_supported_values(),
        DomainProfileSupport::UnsupportedRecordThenUnavailable => {
            Err(HalError::Unsupported(unsupported_profile_reason(request)))
        }
    }
}

fn unsupported_profile_reason(request: &RuntimeExecutableRequest) -> &'static str {
    match request {
        RuntimeExecutableRequest::UnsupportedProfile { reason } => reason,
        _ => "runtime executable request is outside the active product profile",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::HalInvalidArgumentKind;
    use maleicacid_tuner_hal2_domain_request::{
        DvrOpenKind, FilterAvStreamKind, FilterAvStreamTypeRequest, LnbSetSatellitePositionRequest,
        OpenDvrRequest,
    };

    #[test]
    fn rejects_invalid_open_dvr_buffer_size() {
        let error = validate_runtime_executable_request(Some(&RuntimeExecutableRequest::OpenDvr(
            OpenDvrRequest {
                kind: DvrOpenKind::Record,
                buffer_size: 0,
            },
        )))
        .expect_err("zero DVR buffer must fail");
        assert!(matches!(
            error,
            HalError::InvalidArgument {
                kind: HalInvalidArgumentKind::NumericRange,
                ..
            }
        ));
    }

    #[test]
    fn rejects_invalid_av_stream_type() {
        let error = validate_runtime_executable_request(Some(
            &RuntimeExecutableRequest::FilterConfigureAvStreamType(FilterAvStreamTypeRequest {
                kind: FilterAvStreamKind::Video,
                stream_type: -1,
            }),
        ))
        .expect_err("negative AV stream type must fail");
        assert!(matches!(
            error,
            HalError::InvalidArgument {
                kind: HalInvalidArgumentKind::NumericRange,
                ..
            }
        ));
    }

    #[test]
    fn rejects_invalid_lnb_satellite_position() {
        let error = validate_runtime_executable_request(Some(
            &RuntimeExecutableRequest::LnbSetSatellitePosition(LnbSetSatellitePositionRequest {
                position: -1,
            }),
        ))
        .expect_err("negative LNB satellite position must fail");
        assert!(matches!(
            error,
            HalError::InvalidArgument {
                kind: HalInvalidArgumentKind::NumericRange,
                ..
            }
        ));
    }
}
