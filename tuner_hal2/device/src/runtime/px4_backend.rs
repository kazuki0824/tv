use maleicacid_tuner_hal2_common::FrontendDevicePath;

use super::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Px4Backend {
    pub family_code: u16,
    pub unit_index: u16,
    control_path: FrontendDevicePath,
}

impl Px4Backend {
    pub fn new(family_code: u16, unit_index: u16, control_path: FrontendDevicePath) -> Self {
        Self { family_code, unit_index, control_path }
    }

    pub fn control_path(&self) -> &FrontendDevicePath { &self.control_path }

    pub fn live_reader_descriptor(&self, frontend_id: i32) -> FrontendLiveReaderDescriptor {
        FrontendLiveReaderDescriptor::px4_from_control_fd(frontend_id, self.control_path.clone())
    }

    pub fn live_reader_descriptor_uses_control_fd_duplication(&self) -> bool {
        matches!(self.live_reader_descriptor(0).kind, FrontendLiveReaderDescriptorKind::Px4DuplicatedControlFd { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px4_backend_live_reader_descriptor_is_control_fd_duplication() {
        let backend = Px4Backend::new(1, 0, FrontendDevicePath::new("/dev/px4video0"));
        assert!(backend.live_reader_descriptor_uses_control_fd_duplication());
        let reader = backend.live_reader_descriptor(1000);
        match reader.kind {
            FrontendLiveReaderDescriptorKind::Px4DuplicatedControlFd { control_path } => {
                assert_eq!(control_path.display(), "/dev/px4video0");
            }
            _ => panic!("px4 backendは2回目のdevice openをmodel化してはならない"),
        }
    }
}
