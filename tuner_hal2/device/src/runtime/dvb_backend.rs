use maleicacid_tuner_hal2_common::FrontendDevicePath;

use super::FrontendLiveReaderDescriptor;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvbBackend {
    pub adapter_id: u8,
    pub frontend_index: u8,
    frontend_path: FrontendDevicePath,
    dvr_path: FrontendDevicePath,
}

impl DvbBackend {
    pub fn new(adapter_id: u8, frontend_index: u8, frontend_path: FrontendDevicePath, dvr_path: FrontendDevicePath) -> Self {
        Self { adapter_id, frontend_index, frontend_path, dvr_path }
    }

    pub fn frontend_path(&self) -> &FrontendDevicePath { &self.frontend_path }
    pub fn dvr_path(&self) -> &FrontendDevicePath { &self.dvr_path }

    pub fn live_reader_descriptor(&self, frontend_id: i32) -> FrontendLiveReaderDescriptor {
        FrontendLiveReaderDescriptor::dvb_dvr_device(frontend_id, self.dvr_path.clone())
    }
}
