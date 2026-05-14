use android_hardware_cas::aidl::android::hardware::cas::{
    AidlCasPluginDescriptor::AidlCasPluginDescriptor,
    ICas::ICas,
    ICasListener::ICasListener,
    IDescrambler::IDescrambler,
    IMediaCasService::{BnMediaCasService, IMediaCasService},
};
use binder::{BinderFeatures, Interface, Result as BinderResult, StatusCode, Strong};

const CAS_SERVICE_NAME: &str = "android.hardware.cas.IMediaCasService/default";

struct MediaCasStub;

impl Interface for MediaCasStub {}

impl IMediaCasService for MediaCasStub {
    fn createDescrambler(&self, _ca_system_id: i32) -> BinderResult<Strong<dyn IDescrambler>> {
        Err(StatusCode::NAME_NOT_FOUND.into())
    }

    fn createPlugin(&self, _ca_system_id: i32, _listener: &Strong<dyn ICasListener>) -> BinderResult<Strong<dyn ICas>> {
        Err(StatusCode::NAME_NOT_FOUND.into())
    }

    fn enumeratePlugins(&self) -> BinderResult<Vec<AidlCasPluginDescriptor>> {
        Ok(Vec::new())
    }

    fn isDescramblerSupported(&self, _ca_system_id: i32) -> BinderResult<bool> {
        Ok(false)
    }

    fn isSystemIdSupported(&self, _ca_system_id: i32) -> BinderResult<bool> {
        Ok(false)
    }
}

fn main() {
    binder::ProcessState::start_thread_pool();
    let cas_binder = BnMediaCasService::new_binder(MediaCasStub, BinderFeatures::default());
    binder::add_service(CAS_SERVICE_NAME, cas_binder.as_binder())
        .unwrap_or_else(|e| panic!("CAS HAL service 登録に失敗しました {}: {:?}", CAS_SERVICE_NAME, e));
    binder::ProcessState::join_thread_pool();
}


#[cfg(test)]
mod cas_placeholder_tests {
    use super::*;

    #[test]
    fn プレースホルダーはプラグインを広告しない() {
        let service = MediaCasStub;
        assert!(service.enumeratePlugins().unwrap().is_empty());
        assert!(!service.isSystemIdSupported(0x0005).unwrap());
        assert!(!service.isSystemIdSupported(0x0001).unwrap());
        assert!(!service.isDescramblerSupported(0x0005).unwrap());
        assert!(!service.isDescramblerSupported(0x0001).unwrap());
    }

    #[test]
    fn プレースホルダーは_descrambler_を返さない() {
        let service = MediaCasStub;
        assert!(service.createDescrambler(0x0005).is_err());
        assert!(service.createDescrambler(0x0001).is_err());
    }
}
