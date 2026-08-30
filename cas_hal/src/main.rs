mod transport;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use android_hardware_cas::aidl::android::hardware::cas::{
    AidlCasPluginDescriptor::AidlCasPluginDescriptor,
    ICas::{BnCas, ICas},
    ICasListener::ICasListener,
    IDescrambler::IDescrambler,
    IMediaCasService::{BnMediaCasService, IMediaCasService},
    ScramblingMode::ScramblingMode,
    SessionIntent::SessionIntent,
};
use binder::{BinderFeatures, Interface, Result as BinderResult, Status, Strong};
use maleicacid_cas_hal_core::{
    CasError, CasPluginRuntime, CasScramblingMode, CasSessionIntent, CasSystem, GenerationSource,
    SessionIdGenerator,
};
use transport::{
    AtomicGenerationSource, CapabilitySnapshot, UnixCasPathRouter, UnixTunerKeyPublisher,
    UrandomSessionIdGenerator, CAS_CAPABILITY_PROFILE_PATH,
};

const CAS_SERVICE_NAME: &str = "android.hardware.cas.IMediaCasService/default";
const B25_PLUGIN_NAME: &str = "Maleicacid B25 CAS";
const B1_PLUGIN_NAME: &str = "Maleicacid B1 CAS";

fn binder_error(error: CasError) -> Status {
    Status::new_service_specific_error(error.service_specific_code(), None)
}

struct MaleicacidCasPlugin {
    runtime: Arc<CasPluginRuntime>,
    listener: Mutex<Option<Strong<dyn ICasListener>>>,
    drop_cleanup_failures: Arc<AtomicU64>,
}

impl MaleicacidCasPlugin {
    fn release_artifacts(&self) -> Result<(), CasError> {
        let release_result = self.runtime.release();
        let listener_result = self
            .listener
            .lock()
            .map(|mut listener| {
                listener.take();
            })
            .map_err(|_| CasError::InvalidState);
        release_result.and(listener_result)
    }

    fn record_drop_cleanup_failure(&self) {
        let _ = self.drop_cleanup_failures.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |current| Some(current.saturating_add(1)),
        );
    }
}

impl Drop for MaleicacidCasPlugin {
    fn drop(&mut self) {
        if self.release_artifacts().is_err() {
            self.record_drop_cleanup_failure();
        }
    }
}

impl Interface for MaleicacidCasPlugin {}

impl ICas for MaleicacidCasPlugin {
    fn closeSession(&self, session_id: &[u8]) -> BinderResult<()> {
        self.runtime.close_session(session_id).map_err(binder_error)
    }

    fn openSessionDefault(&self) -> BinderResult<Vec<u8>> {
        self.runtime.open_session_default().map_err(binder_error)
    }

    fn openSession(&self, intent: SessionIntent, mode: ScramblingMode) -> BinderResult<Vec<u8>> {
        let intent = if intent == SessionIntent::LIVE {
            CasSessionIntent::Live
        } else {
            CasSessionIntent::Unsupported
        };
        let mode = if mode == ScramblingMode::MULTI2 {
            CasScramblingMode::Multi2
        } else {
            CasScramblingMode::Unsupported
        };
        self.runtime
            .open_session(intent, mode)
            .map_err(binder_error)
    }

    fn processEcm(&self, session_id: &[u8], ecm: &[u8]) -> BinderResult<()> {
        self.runtime
            .process_ecm(session_id, ecm)
            .map_err(binder_error)
    }

    fn processEmm(&self, emm: &[u8]) -> BinderResult<()> {
        self.runtime.process_emm(emm).map_err(binder_error)
    }

    fn provision(&self, _provision_string: &str) -> BinderResult<()> {
        Err(binder_error(CasError::CannotHandle))
    }

    fn refreshEntitlements(&self, _refresh_type: i32, _refresh_data: &[u8]) -> BinderResult<()> {
        Err(binder_error(CasError::CannotHandle))
    }

    fn release(&self) -> BinderResult<()> {
        self.release_artifacts().map_err(binder_error)
    }

    fn sendEvent(&self, _event: i32, _arg: i32, _event_data: &[u8]) -> BinderResult<()> {
        Err(binder_error(CasError::CannotHandle))
    }

    fn sendSessionEvent(
        &self,
        _session_id: &[u8],
        _event: i32,
        _arg: i32,
        _event_data: &[u8],
    ) -> BinderResult<()> {
        Err(binder_error(CasError::CannotHandle))
    }

    fn setPrivateData(&self, private_data: &[u8]) -> BinderResult<()> {
        self.runtime
            .set_private_data(private_data)
            .map_err(binder_error)
    }

    fn setSessionPrivateData(&self, session_id: &[u8], private_data: &[u8]) -> BinderResult<()> {
        self.runtime
            .set_session_private_data(session_id, private_data)
            .map_err(binder_error)
    }
}

struct MaleicacidMediaCasService {
    capabilities: CapabilitySnapshot,
    path_router: Arc<UnixCasPathRouter>,
    key_publisher: Arc<UnixTunerKeyPublisher>,
    session_id_generator: Arc<dyn SessionIdGenerator>,
    generation_source: Arc<dyn GenerationSource>,
    plugin_drop_cleanup_failures: Arc<AtomicU64>,
}

impl MaleicacidMediaCasService {
    fn new() -> Self {
        let capabilities = match CapabilitySnapshot::load(CAS_CAPABILITY_PROFILE_PATH) {
            Ok(snapshot) => snapshot,
            Err(_) => CapabilitySnapshot::default(),
        };
        let path_router = Arc::new(UnixCasPathRouter::for_b25_profile(
            capabilities.b25_path_profile(),
        ));
        Self {
            capabilities,
            path_router,
            key_publisher: Arc::new(UnixTunerKeyPublisher::new()),
            session_id_generator: Arc::new(UrandomSessionIdGenerator),
            generation_source: Arc::new(AtomicGenerationSource::new()),
            plugin_drop_cleanup_failures: Arc::new(AtomicU64::new(0)),
        }
    }

    fn descriptor(system: CasSystem) -> AidlCasPluginDescriptor {
        let mut descriptor = AidlCasPluginDescriptor::default();
        descriptor.caSystemId = system.ca_system_id();
        descriptor.name = match system {
            CasSystem::B25 => B25_PLUGIN_NAME,
            CasSystem::B1 => B1_PLUGIN_NAME,
        }
        .to_owned();
        descriptor
    }
}

impl Interface for MaleicacidMediaCasService {}

impl IMediaCasService for MaleicacidMediaCasService {
    fn createDescrambler(&self, _ca_system_id: i32) -> BinderResult<Strong<dyn IDescrambler>> {
        Err(binder_error(CasError::CannotHandle))
    }

    fn createPlugin(
        &self,
        ca_system_id: i32,
        listener: &Strong<dyn ICasListener>,
    ) -> BinderResult<Strong<dyn ICas>> {
        let system = CasSystem::from_ca_system_id(ca_system_id)
            .ok_or_else(|| binder_error(CasError::CannotHandle))?;
        if !self.capabilities.supports(system) {
            return Err(binder_error(CasError::CannotHandle));
        }
        let plugin_generation = self
            .generation_source
            .next_generation()
            .map_err(binder_error)?;
        let runtime = Arc::new(
            CasPluginRuntime::try_new(
                system,
                plugin_generation,
                self.path_router.clone(),
                self.key_publisher.clone(),
                self.session_id_generator.clone(),
                self.generation_source.clone(),
            )
            .map_err(binder_error)?,
        );
        Ok(BnCas::new_binder(
            MaleicacidCasPlugin {
                runtime,
                listener: Mutex::new(Some(listener.clone())),
                drop_cleanup_failures: self.plugin_drop_cleanup_failures.clone(),
            },
            BinderFeatures::default(),
        ))
    }

    fn enumeratePlugins(&self) -> BinderResult<Vec<AidlCasPluginDescriptor>> {
        Ok(self
            .capabilities
            .systems()
            .iter()
            .copied()
            .map(Self::descriptor)
            .collect())
    }

    fn isDescramblerSupported(&self, _ca_system_id: i32) -> BinderResult<bool> {
        Ok(false)
    }

    fn isSystemIdSupported(&self, ca_system_id: i32) -> BinderResult<bool> {
        Ok(CasSystem::from_ca_system_id(ca_system_id)
            .map(|system| self.capabilities.supports(system))
            .unwrap_or(false))
    }
}

fn main() {
    binder::ProcessState::start_thread_pool();
    let cas_binder =
        BnMediaCasService::new_binder(MaleicacidMediaCasService::new(), BinderFeatures::default());
    if binder::add_service(CAS_SERVICE_NAME, cas_binder.as_binder()).is_err() {
        std::process::exit(1);
    }
    binder::ProcessState::join_thread_pool();
}
