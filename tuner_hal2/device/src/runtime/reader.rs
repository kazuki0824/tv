use maleicacid_tuner_hal2_common::FrontendDevicePath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendLiveReaderDescriptorKind {
    Px4DuplicatedControlFd { control_path: FrontendDevicePath },
    DvbDvrDevice { dvr_path: FrontendDevicePath },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendLiveReaderDescriptor {
    pub kind: FrontendLiveReaderDescriptorKind,
    pub frontend_id: i32,
}

impl FrontendLiveReaderDescriptor {
    pub fn px4_from_control_fd(frontend_id: i32, control_path: FrontendDevicePath) -> Self {
        Self { kind: FrontendLiveReaderDescriptorKind::Px4DuplicatedControlFd { control_path }, frontend_id }
    }

    pub fn dvb_dvr_device(frontend_id: i32, dvr_path: FrontendDevicePath) -> Self {
        Self { kind: FrontendLiveReaderDescriptorKind::DvbDvrDevice { dvr_path }, frontend_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px4_reader_kind_records_control_fd_duplication_not_second_open() {
        let reader = FrontendLiveReaderDescriptor::px4_from_control_fd(1, FrontendDevicePath::new("/dev/px4video0"));
        match reader.kind {
            FrontendLiveReaderDescriptorKind::Px4DuplicatedControlFd { ref control_path } => {
                assert_eq!(control_path.display(), "/dev/px4video0");
            }
            _ => panic!("unexpected reader kind"),
        }
    }
}
