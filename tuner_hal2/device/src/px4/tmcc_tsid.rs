use maleicacid_tuner_hal2_common::{FrontendDevicePath, HalError, HalErrorDetail};

use super::abi::{PtxTmccTsidList, ERRNO_EAGAIN, PTX_TMCC_TSID_MAX};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Px4TmccTsidListObservation {
    Pending,
    Available(Vec<u16>),
}

pub fn decode_tmcc_tsid_list(
    path: &FrontendDevicePath,
    raw: PtxTmccTsidList,
) -> Result<Vec<u16>, HalError> {
    let count = usize::try_from(raw.num).map_err(|_| malformed_tmcc_list(path, "num overflows usize"))?;
    if count > PTX_TMCC_TSID_MAX {
        return Err(malformed_tmcc_list(
            path,
            format!("driver returned num={} above max={PTX_TMCC_TSID_MAX}", raw.num),
        ));
    }
    let values = raw.tsid[..count].to_vec();
    if values.iter().any(|tsid| *tsid == 0) {
        return Err(malformed_tmcc_list(
            path,
            "driver returned zero inside the compact TMCC TSID list",
        ));
    }
    Ok(values)
}

pub fn classify_tmcc_tsid_read(
    result: Result<Vec<u16>, HalError>,
) -> Result<Px4TmccTsidListObservation, HalError> {
    match result {
        Ok(values) => Ok(Px4TmccTsidListObservation::Available(values)),
        Err(HalError::IoctlFailed { errno, .. }) if errno == ERRNO_EAGAIN => {
            Ok(Px4TmccTsidListObservation::Pending)
        }
        Err(error) => Err(error),
    }
}

fn malformed_tmcc_list(path: &FrontendDevicePath, detail: impl Into<String>) -> HalError {
    HalError::Io {
        backend: "px4",
        operation: "PTX_GET_TMCC_TSID_LIST",
        path: Some(path.as_path().to_path_buf()),
        errno: None,
        detail: HalErrorDetail::new(detail.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_tmcc_list_preserves_driver_slot_order() {
        let path = FrontendDevicePath::new("/dev/px4video0");
        let mut raw = PtxTmccTsidList::default();
        raw.num = 3;
        raw.tsid[..3].copy_from_slice(&[0x4010, 0x4011, 0x4030]);
        assert_eq!(
            decode_tmcc_tsid_list(&path, raw).unwrap(),
            vec![0x4010, 0x4011, 0x4030]
        );
    }

    #[test]
    fn malformed_driver_count_fails_closed() {
        let path = FrontendDevicePath::new("/dev/px4video0");
        let raw = PtxTmccTsidList {
            num: 13,
            ..PtxTmccTsidList::default()
        };
        assert!(matches!(decode_tmcc_tsid_list(&path, raw), Err(HalError::Io { .. })));
    }

    #[test]
    fn zero_in_compact_list_fails_closed() {
        let path = FrontendDevicePath::new("/dev/px4video0");
        let mut raw = PtxTmccTsidList::default();
        raw.num = 2;
        raw.tsid[0] = 0x4010;
        assert!(matches!(decode_tmcc_tsid_list(&path, raw), Err(HalError::Io { .. })));
    }

    #[test]
    fn eagain_is_pending_and_other_errors_are_preserved() {
        let pending = classify_tmcc_tsid_read(Err(HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTX_GET_TMCC_TSID_LIST",
            errno: ERRNO_EAGAIN,
        }));
        assert_eq!(pending, Ok(Px4TmccTsidListObservation::Pending));

        let failure = classify_tmcc_tsid_read(Err(HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTX_GET_TMCC_TSID_LIST",
            errno: 5,
        }));
        assert!(matches!(failure, Err(HalError::IoctlFailed { errno: 5, .. })));
    }
}
