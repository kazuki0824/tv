use std::collections::BTreeMap;
use std::ptr;
use std::sync::atomic::{compiler_fence, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub const B25_CA_SYSTEM_ID: i32 = 0x0005;
pub const B1_CA_SYSTEM_ID: i32 = 0x0001;
pub const MEDIA_CAS_SESSION_ID_MAX_BYTES: usize = 16;

fn volatile_zeroize(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

pub const MAX_CAS_SECTION_BYTES: usize = 4_098;
pub const MAX_PRIVATE_DATA_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasSystem {
    B25,
    B1,
}

impl CasSystem {
    pub const fn from_ca_system_id(ca_system_id: i32) -> Option<Self> {
        match ca_system_id {
            B25_CA_SYSTEM_ID => Some(Self::B25),
            B1_CA_SYSTEM_ID => Some(Self::B1),
            _ => None,
        }
    }

    pub const fn ca_system_id(self) -> i32 {
        match self {
            Self::B25 => B25_CA_SYSTEM_ID,
            Self::B1 => B1_CA_SYSTEM_ID,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasSessionIntent {
    Live,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasScramblingMode {
    Multi2,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum CasPathKind {
    SmartCard,
    Yakisoba,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasError {
    NoLicense,
    LicenseExpired,
    SessionNotOpened,
    CannotHandle,
    InvalidState,
    BadValue,
    NotProvisioned,
    ResourceBusy,
    Unknown,
    NoCard,
    CardMute,
    CardInvalid,
    IoUnavailable,
    Timeout,
    PoisonedLock,
    GenerationExhausted,
    /// Internal-only signal that a candidate MediaCas session ID already belongs to
    /// the service-global Tuner token namespace and must be regenerated.
    TokenCollision,
}

impl CasError {
    pub const fn service_specific_code(self) -> i32 {
        match self {
            Self::NoLicense => 1,
            Self::LicenseExpired => 2,
            Self::SessionNotOpened => 3,
            Self::CannotHandle => 4,
            Self::InvalidState | Self::Timeout | Self::PoisonedLock | Self::GenerationExhausted => {
                5
            }
            Self::BadValue => 6,
            Self::NotProvisioned => 7,
            Self::ResourceBusy | Self::TokenCollision => 8,
            Self::NoCard => 17,
            Self::CardMute => 18,
            Self::CardInvalid => 19,
            Self::IoUnavailable | Self::Unknown => 14,
        }
    }

    fn makes_session_fail(self) -> bool {
        matches!(
            self,
            Self::NoLicense
                | Self::LicenseExpired
                | Self::InvalidState
                | Self::NotProvisioned
                | Self::NoCard
                | Self::CardMute
                | Self::CardInvalid
                | Self::IoUnavailable
                | Self::Timeout
                | Self::PoisonedLock
                | Self::GenerationExhausted
        )
    }
}

#[derive(Eq, PartialEq)]
pub struct EcmKeyMaterial {
    pub system_key: [u8; 32],
    pub cbc_initial_value: [u8; 8],
    pub even_ks: [u8; 8],
    pub odd_ks: [u8; 8],
}

impl std::fmt::Debug for EcmKeyMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EcmKeyMaterial")
            .field("key_material", &"<redacted>")
            .finish()
    }
}

impl Drop for EcmKeyMaterial {
    fn drop(&mut self) {
        volatile_zeroize(&mut self.system_key);
        volatile_zeroize(&mut self.cbc_initial_value);
        volatile_zeroize(&mut self.even_ks);
        volatile_zeroize(&mut self.odd_ks);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CasPathOpenError {
    pub error: CasError,
    pub cleanup_path: Option<CasPathKind>,
}

pub trait CasPathRouter: Send + Sync {
    fn open_session(
        &self,
        system: CasSystem,
        session_id: &[u8],
        session_generation: u64,
        plugin_private_data: &[u8],
    ) -> Result<CasPathKind, CasPathOpenError>;

    fn set_session_private_data(
        &self,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        session_generation: u64,
        private_data: &[u8],
    ) -> Result<(), CasError>;

    fn process_ecm(
        &self,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        session_generation: u64,
        ecm: &[u8],
    ) -> Result<EcmKeyMaterial, CasError>;

    fn process_emm(&self, system: CasSystem, emm: &[u8]) -> Result<(), CasError>;

    fn close_session(
        &self,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        session_generation: u64,
    ) -> Result<(), CasError>;
}

pub trait TunerKeyPublisher: Send + Sync {
    fn reserve(&self, key_token: &[u8], provider_generation: u64) -> Result<(), CasError>;
    fn publish(
        &self,
        key_token: &[u8],
        provider_generation: u64,
        key_epoch: u64,
        material: EcmKeyMaterial,
    ) -> Result<(), CasError>;
    fn revoke(&self, key_token: &[u8], provider_generation: u64) -> Result<(), CasError>;
}

pub trait SessionIdGenerator: Send + Sync {
    fn next_session_id(&self) -> Result<Vec<u8>, CasError>;
}

pub trait GenerationSource: Send + Sync {
    fn next_generation(&self) -> Result<u64, CasError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionLifecycle {
    Opening,
    Active,
    Failed,
    Closing,
}

#[derive(Debug)]
struct SessionRecord {
    generation: u64,
    lifecycle: SessionLifecycle,
    path: Option<CasPathKind>,
    private_data: Vec<u8>,
    key_epoch: u64,
    io_in_flight: bool,
    key_reserved: bool,
    cleanup_in_flight: bool,
    cleanup_errors: CleanupErrors,
    failure_reason: Option<CasError>,
}

impl Drop for SessionRecord {
    fn drop(&mut self) {
        volatile_zeroize(&mut self.private_data);
    }
}

#[derive(Debug, Default)]
struct PluginState {
    released: bool,
    plugin_private_data: Vec<u8>,
    emm_in_flight: bool,
    sessions: BTreeMap<Vec<u8>, SessionRecord>,
}

impl Drop for PluginState {
    fn drop(&mut self) {
        volatile_zeroize(&mut self.plugin_private_data);
    }
}

struct SessionIoSnapshot {
    path: CasPathKind,
    generation: u64,
    next_key_epoch: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CleanupErrors {
    pub revoke: Option<CasError>,
    pub close: Option<CasError>,
}

struct SessionCleanup {
    session_id: Vec<u8>,
    generation: u64,
    path: Option<CasPathKind>,
    revoke: bool,
}

pub struct CasPluginRuntime {
    system: CasSystem,
    plugin_generation: u64,
    state: Mutex<PluginState>,
    path_router: Arc<dyn CasPathRouter>,
    key_publisher: Arc<dyn TunerKeyPublisher>,
    session_id_generator: Arc<dyn SessionIdGenerator>,
    generation_source: Arc<dyn GenerationSource>,
}

impl CasPluginRuntime {
    pub fn try_new(
        system: CasSystem,
        plugin_generation: u64,
        path_router: Arc<dyn CasPathRouter>,
        key_publisher: Arc<dyn TunerKeyPublisher>,
        session_id_generator: Arc<dyn SessionIdGenerator>,
        generation_source: Arc<dyn GenerationSource>,
    ) -> Result<Self, CasError> {
        if plugin_generation == 0 {
            return Err(CasError::InvalidState);
        }
        Ok(Self {
            system,
            plugin_generation,
            state: Mutex::new(PluginState::default()),
            path_router,
            key_publisher,
            session_id_generator,
            generation_source,
        })
    }

    pub const fn plugin_generation(&self) -> u64 {
        self.plugin_generation
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, PluginState>, CasError> {
        self.state.lock().map_err(|_| CasError::PoisonedLock)
    }

    fn ensure_plugin_live(state: &PluginState) -> Result<(), CasError> {
        if state.released {
            Err(CasError::InvalidState)
        } else {
            Ok(())
        }
    }

    fn allocate_session_id(&self) -> Result<Vec<u8>, CasError> {
        for _ in 0..16 {
            let session_id = self.session_id_generator.next_session_id()?;
            if session_id.is_empty()
                || session_id.len() > MEDIA_CAS_SESSION_ID_MAX_BYTES
                || session_id == [0]
            {
                continue;
            }
            let state = self.lock_state()?;
            Self::ensure_plugin_live(&state)?;
            if !state.sessions.contains_key(&session_id) {
                return Ok(session_id);
            }
        }
        Err(CasError::ResourceBusy)
    }

    pub fn set_private_data(&self, private_data: &[u8]) -> Result<(), CasError> {
        if private_data.len() > MAX_PRIVATE_DATA_BYTES {
            return Err(CasError::BadValue);
        }
        let mut state = self.lock_state()?;
        Self::ensure_plugin_live(&state)?;
        volatile_zeroize(&mut state.plugin_private_data);
        state.plugin_private_data = private_data.to_vec();
        Ok(())
    }

    pub fn open_session_default(&self) -> Result<Vec<u8>, CasError> {
        self.open_session(CasSessionIntent::Live, CasScramblingMode::Multi2)
    }

    pub fn open_session(
        &self,
        intent: CasSessionIntent,
        mode: CasScramblingMode,
    ) -> Result<Vec<u8>, CasError> {
        if intent != CasSessionIntent::Live || mode != CasScramblingMode::Multi2 {
            return Err(CasError::CannotHandle);
        }
        let generation = self.generation_source.next_generation()?;
        for _ in 0..16 {
            let session_id = self.allocate_session_id()?;
            let plugin_private_data = {
                let mut state = self.lock_state()?;
                Self::ensure_plugin_live(&state)?;
                if state.sessions.contains_key(&session_id) {
                    continue;
                }
                let private_data = state.plugin_private_data.clone();
                state.sessions.insert(
                    session_id.clone(),
                    SessionRecord {
                        generation,
                        lifecycle: SessionLifecycle::Opening,
                        path: None,
                        private_data: Vec::new(),
                        key_epoch: 0,
                        io_in_flight: true,
                        key_reserved: true,
                        cleanup_in_flight: false,
                        cleanup_errors: CleanupErrors::default(),
                        failure_reason: None,
                    },
                );
                private_data
            };

            // The Tuner key registry is the service-global token namespace
            // linearization point. A collision only invalidates this candidate.
            match self.key_publisher.reserve(&session_id, generation) {
                Ok(()) => {}
                Err(CasError::TokenCollision) => {
                    let mut state = self.lock_state()?;
                    state.sessions.remove(&session_id);
                    continue;
                }
                Err(error) => {
                    return self.fail_open(&session_id, generation, None, error);
                }
            }

            let path = match self.path_router.open_session(
                self.system,
                &session_id,
                generation,
                &plugin_private_data,
            ) {
                Ok(path) => path,
                Err(failure) => {
                    return self.fail_open(
                        &session_id,
                        generation,
                        failure.cleanup_path,
                        failure.error,
                    );
                }
            };

            let result = match self.lock_state() {
                Ok(mut state) => {
                    let released = state.released;
                    match state.sessions.get_mut(&session_id) {
                        Some(session)
                            if !released
                                && session.generation == generation
                                && session.lifecycle == SessionLifecycle::Opening =>
                        {
                            session.path = Some(path);
                            session.lifecycle = SessionLifecycle::Active;
                            session.io_in_flight = false;
                            Ok(session_id.clone())
                        }
                        _ => Err(CasError::InvalidState),
                    }
                }
                Err(error) => Err(error),
            };
            match result {
                Ok(session_id) => return Ok(session_id),
                Err(error) => {
                    return self.fail_open(&session_id, generation, Some(path), error);
                }
            }
        }
        Err(CasError::ResourceBusy)
    }

    fn begin_session_io(&self, session_id: &[u8]) -> Result<SessionIoSnapshot, CasError> {
        let mut state = self.lock_state()?;
        Self::ensure_plugin_live(&state)?;
        let session = state
            .sessions
            .get_mut(session_id)
            .ok_or(CasError::SessionNotOpened)?;
        if session.lifecycle != SessionLifecycle::Active {
            return Err(CasError::SessionNotOpened);
        }
        if session.io_in_flight {
            return Err(CasError::ResourceBusy);
        }
        let path = session.path.ok_or(CasError::InvalidState)?;
        let next_key_epoch = session
            .key_epoch
            .checked_add(1)
            .ok_or(CasError::GenerationExhausted)?;
        session.io_in_flight = true;
        Ok(SessionIoSnapshot {
            path,
            generation: session.generation,
            next_key_epoch,
        })
    }

    fn fail_open(
        &self,
        session_id: &[u8],
        generation: u64,
        path: Option<CasPathKind>,
        error: CasError,
    ) -> Result<Vec<u8>, CasError> {
        {
            let mut state = self.lock_state()?;
            let session = state
                .sessions
                .get_mut(session_id)
                .ok_or(CasError::InvalidState)?;
            if session.generation != generation {
                return Err(CasError::InvalidState);
            }
            session.path = path;
            session.io_in_flight = false;
            session.lifecycle = SessionLifecycle::Closing;
            session.failure_reason = Some(error);
        }
        // cleanupの失敗もsessionが所有し続け、release/reaperが未完了stepだけを再試行する。
        self.cleanup_session(session_id)?;
        Err(error)
    }

    fn finish_session_io(
        &self,
        session_id: &[u8],
        generation: u64,
        result: Result<(), CasError>,
        commit: impl FnOnce(&mut SessionRecord),
    ) -> Result<(), CasError> {
        let (result, cleanup_needed) = {
            let mut state = self.lock_state()?;
            let released = state.released;
            let session = state
                .sessions
                .get_mut(session_id)
                .ok_or(CasError::InvalidState)?;
            if session.generation != generation || !session.io_in_flight {
                return Err(CasError::InvalidState);
            }
            session.io_in_flight = false;
            let result = if released || session.lifecycle == SessionLifecycle::Closing {
                session.lifecycle = SessionLifecycle::Closing;
                Err(CasError::InvalidState)
            } else {
                result
            };
            match result {
                Ok(()) => commit(session),
                Err(error) => {
                    session.failure_reason = Some(error);
                    if error.makes_session_fail() && session.lifecycle == SessionLifecycle::Active {
                        session.lifecycle = SessionLifecycle::Failed;
                    }
                }
            }
            (
                result,
                matches!(
                    session.lifecycle,
                    SessionLifecycle::Failed | SessionLifecycle::Closing
                ),
            )
        };
        if cleanup_needed {
            self.cleanup_session(session_id)?;
        }
        result
    }

    pub fn process_ecm(&self, session_id: &[u8], ecm: &[u8]) -> Result<(), CasError> {
        validate_complete_section(ecm)?;
        let snapshot = self.begin_session_io(session_id)?;
        let result = self
            .path_router
            .process_ecm(
                self.system,
                snapshot.path,
                session_id,
                snapshot.generation,
                ecm,
            )
            .and_then(|material| {
                self.key_publisher.publish(
                    session_id,
                    snapshot.generation,
                    snapshot.next_key_epoch,
                    material,
                )
            });
        self.finish_session_io(session_id, snapshot.generation, result, |session| {
            session.key_epoch = snapshot.next_key_epoch;
        })
    }

    pub fn process_emm(&self, emm: &[u8]) -> Result<(), CasError> {
        if self.system == CasSystem::B1 {
            return Err(CasError::CannotHandle);
        }
        validate_complete_section(emm)?;
        {
            let mut state = self.lock_state()?;
            Self::ensure_plugin_live(&state)?;
            if state.emm_in_flight {
                return Err(CasError::ResourceBusy);
            }
            state.emm_in_flight = true;
        }
        let result = self.path_router.process_emm(self.system, emm);
        let mut state = self.lock_state()?;
        state.emm_in_flight = false;
        if state.released {
            return Err(CasError::InvalidState);
        }
        result
    }

    pub fn set_session_private_data(
        &self,
        session_id: &[u8],
        private_data: &[u8],
    ) -> Result<(), CasError> {
        if private_data.len() > MAX_PRIVATE_DATA_BYTES {
            return Err(CasError::BadValue);
        }
        let snapshot = self.begin_session_io(session_id)?;
        let result = self.path_router.set_session_private_data(
            self.system,
            snapshot.path,
            session_id,
            snapshot.generation,
            private_data,
        );
        self.finish_session_io(session_id, snapshot.generation, result, |session| {
            volatile_zeroize(&mut session.private_data);
            session.private_data = private_data.to_vec();
        })
    }

    fn cleanup_session(&self, session_id: &[u8]) -> Result<(), CasError> {
        let cleanup = {
            let mut state = self.lock_state()?;
            let Some(session) = state.sessions.get_mut(session_id) else {
                return Ok(());
            };
            // openのreserve/path結果が未確定の間は、そのI/O ownerが結果を回収する。
            if session.cleanup_in_flight || (session.io_in_flight && session.path.is_none()) {
                return Err(CasError::ResourceBusy);
            }
            session.cleanup_in_flight = true;
            SessionCleanup {
                session_id: session_id.to_vec(),
                generation: session.generation,
                revoke: session.key_reserved,
                path: if !session.io_in_flight && session.lifecycle == SessionLifecycle::Closing {
                    session.path
                } else {
                    None
                },
            }
        };
        let revoke = if cleanup.revoke {
            self.key_publisher
                .revoke(&cleanup.session_id, cleanup.generation)
        } else {
            Ok(())
        };
        let close = if let Some(path) = cleanup.path {
            self.path_router.close_session(
                self.system,
                path,
                &cleanup.session_id,
                cleanup.generation,
            )
        } else {
            Ok(())
        };
        let mut state = self.lock_state()?;
        let session = state
            .sessions
            .get_mut(session_id)
            .ok_or(CasError::InvalidState)?;
        if session.generation != cleanup.generation {
            return Err(CasError::InvalidState);
        }
        session.cleanup_in_flight = false;
        session.cleanup_errors = CleanupErrors {
            revoke: revoke.err(),
            close: close.err(),
        };
        if revoke.is_ok() {
            session.key_reserved = false;
        }
        if cleanup.path.is_some() && close.is_ok() {
            session.path = None;
        }
        let io_pending = session.io_in_flight;
        if session.lifecycle == SessionLifecycle::Closing
            && !io_pending
            && !session.key_reserved
            && session.path.is_none()
        {
            state.sessions.remove(session_id);
        }
        revoke.and(close)?;
        if io_pending {
            Err(CasError::ResourceBusy)
        } else {
            Ok(())
        }
    }

    pub fn close_session(&self, session_id: &[u8]) -> Result<(), CasError> {
        {
            let mut state = self.lock_state()?;
            Self::ensure_plugin_live(&state)?;
            let session = state
                .sessions
                .get_mut(session_id)
                .ok_or(CasError::SessionNotOpened)?;
            session.lifecycle = SessionLifecycle::Closing;
        }
        self.cleanup_session(session_id)
    }

    pub fn release(&self) -> Result<(), CasError> {
        let (session_ids, emm_pending) = {
            let mut state = self.lock_state()?;
            state.released = true;
            for session in state.sessions.values_mut() {
                session.lifecycle = SessionLifecycle::Closing;
            }
            (
                state.sessions.keys().cloned().collect::<Vec<_>>(),
                state.emm_in_flight,
            )
        };
        let mut first_error = None;
        for session_id in &session_ids {
            if let Err(error) = self.cleanup_session(session_id) {
                first_error.get_or_insert(error);
            }
        }
        if emm_pending {
            first_error.get_or_insert(CasError::ResourceBusy);
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn cleanup_errors(&self) -> Result<Vec<CleanupErrors>, CasError> {
        Ok(self
            .lock_state()?
            .sessions
            .values()
            .filter_map(|session| {
                let errors = session.cleanup_errors;
                (errors != CleanupErrors::default()).then_some(errors)
            })
            .collect())
    }

    pub fn is_released(&self) -> Result<bool, CasError> {
        Ok(self.lock_state()?.released)
    }

    fn retry_pending_cleanup(&self) -> Result<bool, CasError> {
        let session_ids = self
            .lock_state()?
            .sessions
            .iter()
            .filter_map(|(id, session)| {
                matches!(
                    session.lifecycle,
                    SessionLifecycle::Failed | SessionLifecycle::Closing
                )
                .then_some(id.clone())
            })
            .collect::<Vec<_>>();
        let mut first_error = None;
        for id in session_ids {
            if let Err(error) = self.cleanup_session(&id) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)?;
        let state = self.lock_state()?;
        Ok(state.released && state.sessions.is_empty() && !state.emm_in_flight)
    }
}

/// Binder artifact消滅後も、未完了の失効/closeをservice寿命で所有する。
#[derive(Default)]
pub struct CasCleanupOwner {
    runtimes: Mutex<Vec<Arc<CasPluginRuntime>>>,
}

impl CasCleanupOwner {
    pub fn track(&self, runtime: Arc<CasPluginRuntime>) -> Result<(), CasError> {
        let mut runtimes = self.runtimes.lock().map_err(|_| CasError::PoisonedLock)?;
        if runtimes.len() >= 256 {
            return Err(CasError::ResourceBusy);
        }
        runtimes.push(runtime);
        Ok(())
    }

    pub fn retry_pending(&self) -> Result<(), CasError> {
        let snapshot = self
            .runtimes
            .lock()
            .map_err(|_| CasError::PoisonedLock)?
            .clone();
        let mut completed = Vec::new();
        let mut first_error = None;
        for runtime in snapshot {
            match runtime.retry_pending_cleanup() {
                Ok(true) => completed.push(runtime),
                Ok(false) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        self.runtimes
            .lock()
            .map_err(|_| CasError::PoisonedLock)?
            .retain(|runtime| !completed.iter().any(|done| Arc::ptr_eq(runtime, done)));
        first_error.map_or(Ok(()), Err)
    }
}

pub fn validate_complete_section(section: &[u8]) -> Result<(), CasError> {
    if section.len() < 3 || section.len() > MAX_CAS_SECTION_BYTES {
        return Err(CasError::BadValue);
    }
    let declared = (((section[1] & 0x0f) as usize) << 8) | section[2] as usize;
    let total = declared.checked_add(3).ok_or(CasError::BadValue)?;
    if total != section.len() {
        return Err(CasError::BadValue);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{
        CasError, CasPathKind, CasPathOpenError, CasPathRouter, CasPluginRuntime,
        CasScramblingMode, CasSessionIntent, CasSystem, EcmKeyMaterial, GenerationSource,
        SessionIdGenerator, TunerKeyPublisher,
    };

    #[derive(Default)]
    struct FakeRouter {
        calls: Mutex<Vec<String>>,
        ecm_error: Mutex<Option<CasError>>,
        open_error: Mutex<Option<CasError>>,
        private_error: Mutex<Option<CasError>>,
        close_error: Mutex<Option<CasError>>,
        open_barriers: Mutex<Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>>,
    }

    impl CasPathRouter for FakeRouter {
        fn open_session(
            &self,
            _system: CasSystem,
            _session_id: &[u8],
            _session_generation: u64,
            _plugin_private_data: &[u8],
        ) -> Result<CasPathKind, CasPathOpenError> {
            self.calls.lock().unwrap().push("open".to_owned());
            let barriers = self.open_barriers.lock().unwrap().clone();
            if let Some((entered, resume)) = barriers {
                entered.wait();
                resume.wait();
            }
            match *self.open_error.lock().unwrap() {
                Some(error) => Err(CasPathOpenError {
                    error,
                    cleanup_path: None,
                }),
                None => Ok(CasPathKind::SmartCard),
            }
        }

        fn set_session_private_data(
            &self,
            _system: CasSystem,
            _path: CasPathKind,
            _session_id: &[u8],
            _session_generation: u64,
            _private_data: &[u8],
        ) -> Result<(), CasError> {
            self.calls.lock().unwrap().push("private".to_owned());
            self.private_error.lock().unwrap().map_or(Ok(()), Err)
        }

        fn process_ecm(
            &self,
            _system: CasSystem,
            _path: CasPathKind,
            _session_id: &[u8],
            _session_generation: u64,
            _ecm: &[u8],
        ) -> Result<EcmKeyMaterial, CasError> {
            self.calls.lock().unwrap().push("ecm".to_owned());
            if let Some(error) = *self.ecm_error.lock().unwrap() {
                return Err(error);
            }
            Ok(EcmKeyMaterial {
                system_key: [0x11; 32],
                cbc_initial_value: [0x22; 8],
                even_ks: [0x33; 8],
                odd_ks: [0x44; 8],
            })
        }

        fn process_emm(&self, _system: CasSystem, _emm: &[u8]) -> Result<(), CasError> {
            self.calls.lock().unwrap().push("emm".to_owned());
            Ok(())
        }

        fn close_session(
            &self,
            _system: CasSystem,
            _path: CasPathKind,
            _session_id: &[u8],
            _session_generation: u64,
        ) -> Result<(), CasError> {
            self.calls.lock().unwrap().push("close".to_owned());
            self.close_error.lock().unwrap().map_or(Ok(()), Err)
        }
    }

    #[derive(Default)]
    struct FakePublisher {
        calls: Mutex<Vec<String>>,
        epochs: Mutex<Vec<u64>>,
        reserve_error: Mutex<Option<CasError>>,
        publish_error: Mutex<Option<CasError>>,
        revoke_error: Mutex<Option<CasError>>,
    }

    impl TunerKeyPublisher for FakePublisher {
        fn reserve(&self, _session_id: &[u8], _session_generation: u64) -> Result<(), CasError> {
            self.calls.lock().unwrap().push("reserve".to_owned());
            match *self.reserve_error.lock().unwrap() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        fn publish(
            &self,
            _session_id: &[u8],
            _provider_generation: u64,
            key_epoch: u64,
            _material: EcmKeyMaterial,
        ) -> Result<(), CasError> {
            self.calls.lock().unwrap().push("publish".to_owned());
            self.epochs.lock().unwrap().push(key_epoch);
            match *self.publish_error.lock().unwrap() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        fn revoke(&self, _session_id: &[u8], _session_generation: u64) -> Result<(), CasError> {
            self.calls.lock().unwrap().push("revoke".to_owned());
            self.revoke_error.lock().unwrap().map_or(Ok(()), Err)
        }
    }

    struct SequenceIds(Mutex<VecDeque<Vec<u8>>>);

    impl SessionIdGenerator for SequenceIds {
        fn next_session_id(&self) -> Result<Vec<u8>, CasError> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(CasError::ResourceBusy)
        }
    }

    #[derive(Default)]
    struct AtomicGeneration(AtomicU64);

    impl GenerationSource for AtomicGeneration {
        fn next_generation(&self) -> Result<u64, CasError> {
            self.0
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                    current.checked_add(1)
                })
                .map(|previous| previous + 1)
                .map_err(|_| CasError::GenerationExhausted)
        }
    }

    fn section(table_id: u8, body: &[u8]) -> Vec<u8> {
        let mut output = vec![table_id, 0, body.len() as u8];
        output.extend_from_slice(body);
        output
    }

    fn runtime(system: CasSystem) -> (Arc<FakeRouter>, Arc<FakePublisher>, CasPluginRuntime) {
        let router = Arc::new(FakeRouter::default());
        let publisher = Arc::new(FakePublisher::default());
        let runtime = CasPluginRuntime::try_new(
            system,
            1,
            router.clone(),
            publisher.clone(),
            Arc::new(SequenceIds(Mutex::new(VecDeque::from([
                vec![0],
                vec![0x10, 0x20],
            ])))),
            Arc::new(AtomicGeneration::default()),
        )
        .unwrap();
        (router, publisher, runtime)
    }

    struct CollisionOncePublisher {
        reserve_ids: Mutex<Vec<Vec<u8>>>,
        collided: Mutex<bool>,
    }

    impl Default for CollisionOncePublisher {
        fn default() -> Self {
            Self {
                reserve_ids: Mutex::new(Vec::new()),
                collided: Mutex::new(false),
            }
        }
    }

    impl TunerKeyPublisher for CollisionOncePublisher {
        fn reserve(&self, session_id: &[u8], _session_generation: u64) -> Result<(), CasError> {
            self.reserve_ids.lock().unwrap().push(session_id.to_vec());
            let mut collided = self.collided.lock().unwrap();
            if !*collided {
                *collided = true;
                Err(CasError::TokenCollision)
            } else {
                Ok(())
            }
        }

        fn publish(
            &self,
            _session_id: &[u8],
            _provider_generation: u64,
            _key_epoch: u64,
            _material: EcmKeyMaterial,
        ) -> Result<(), CasError> {
            Ok(())
        }

        fn revoke(&self, _session_id: &[u8], _session_generation: u64) -> Result<(), CasError> {
            Ok(())
        }
    }

    #[test]
    fn global_token_collision_regenerates_before_opening_lower_path() {
        let router = Arc::new(FakeRouter::default());
        let publisher = Arc::new(CollisionOncePublisher::default());
        let runtime = CasPluginRuntime::try_new(
            CasSystem::B25,
            1,
            router.clone(),
            publisher.clone(),
            Arc::new(SequenceIds(Mutex::new(VecDeque::from([
                vec![0x10, 0x20],
                vec![0x30, 0x40],
            ])))),
            Arc::new(AtomicGeneration::default()),
        )
        .unwrap();

        let session_id = runtime.open_session_default().unwrap();
        assert_eq!(session_id, vec![0x30, 0x40]);
        assert_eq!(
            *publisher.reserve_ids.lock().unwrap(),
            vec![vec![0x10, 0x20], vec![0x30, 0x40]]
        );
        assert_eq!(*router.calls.lock().unwrap(), vec!["open"]);
    }

    #[test]
    fn ecm_success_publishes_session_id_token_before_epoch_commit() {
        let (_router, publisher, runtime) = runtime(CasSystem::B25);
        let session_id = runtime.open_session_default().unwrap();
        assert_eq!(session_id, vec![0x10, 0x20]);
        runtime
            .process_ecm(&session_id, &section(0x82, &[1, 2]))
            .unwrap();
        runtime
            .process_ecm(&session_id, &section(0x82, &[3, 4]))
            .unwrap();
        assert_eq!(*publisher.epochs.lock().unwrap(), vec![1, 2]);
        runtime.close_session(&session_id).unwrap();
        assert_eq!(
            *publisher.calls.lock().unwrap(),
            vec!["reserve", "publish", "publish", "revoke"]
        );
    }

    #[test]
    fn publisher_failure_does_not_advance_key_epoch() {
        let (_router, publisher, runtime) = runtime(CasSystem::B25);
        let session_id = runtime.open_session_default().unwrap();
        *publisher.publish_error.lock().unwrap() = Some(CasError::InvalidState);
        assert_eq!(
            runtime.process_ecm(&session_id, &section(0x82, &[1])),
            Err(CasError::InvalidState)
        );
        *publisher.publish_error.lock().unwrap() = None;
        assert_eq!(
            runtime.process_ecm(&session_id, &section(0x82, &[2])),
            Err(CasError::SessionNotOpened)
        );
        assert_eq!(*publisher.epochs.lock().unwrap(), vec![1]);
    }

    #[test]
    fn transient_publish_failure_preserves_session_and_epoch() {
        let (_router, publisher, runtime) = runtime(CasSystem::B25);
        let session_id = runtime.open_session_default().unwrap();
        *publisher.publish_error.lock().unwrap() = Some(CasError::ResourceBusy);
        assert_eq!(
            runtime.process_ecm(&session_id, &section(0x82, &[1])),
            Err(CasError::ResourceBusy)
        );
        *publisher.publish_error.lock().unwrap() = None;
        assert_eq!(
            runtime.process_ecm(&session_id, &section(0x82, &[2])),
            Ok(())
        );
        assert_eq!(*publisher.epochs.lock().unwrap(), vec![1, 1]);
        assert_eq!(
            *publisher.calls.lock().unwrap(),
            vec!["reserve", "publish", "publish"]
        );
    }

    #[test]
    fn uncertain_reservation_failure_revokes_without_opening_lower_path() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        *publisher.reserve_error.lock().unwrap() = Some(CasError::IoUnavailable);
        assert_eq!(runtime.open_session_default(), Err(CasError::IoUnavailable));
        assert_eq!(*publisher.calls.lock().unwrap(), vec!["reserve", "revoke"]);
        assert!(router.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn b1_rejects_emm_without_calling_lower_path() {
        let (router, _publisher, runtime) = runtime(CasSystem::B1);
        assert_eq!(
            runtime.process_emm(&section(0x84, &[1])),
            Err(CasError::CannotHandle)
        );
        assert!(router.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn explicit_session_accepts_only_live_multi2() {
        let (_router, _publisher, runtime) = runtime(CasSystem::B25);
        assert_eq!(
            runtime.open_session(CasSessionIntent::Unsupported, CasScramblingMode::Multi2),
            Err(CasError::CannotHandle)
        );
        assert_eq!(
            runtime.open_session(CasSessionIntent::Live, CasScramblingMode::Unsupported),
            Err(CasError::CannotHandle)
        );
    }

    #[test]
    fn release_revokes_and_closes_every_live_session() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        let first = runtime.open_session_default().unwrap();
        runtime.close_session(&first).unwrap();
        assert_eq!(runtime.release(), Ok(()));
        assert_eq!(runtime.release(), Ok(()));
        assert_eq!(*publisher.calls.lock().unwrap(), vec!["reserve", "revoke"]);
        assert_eq!(*router.calls.lock().unwrap(), vec!["open", "close"]);
    }

    #[test]
    fn malformed_section_is_rejected_before_path_io() {
        let (router, _publisher, runtime) = runtime(CasSystem::B25);
        let session_id = runtime.open_session_default().unwrap();
        assert_eq!(
            runtime.process_ecm(&session_id, &[0x82, 0x00, 0x02, 0x01]),
            Err(CasError::BadValue)
        );
        assert_eq!(*router.calls.lock().unwrap(), vec!["open"]);
    }
    #[test]
    fn close_retries_failed_revoke_without_reclosing_lower_path() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        let id = runtime.open_session_default().unwrap();
        *publisher.revoke_error.lock().unwrap() = Some(CasError::IoUnavailable);
        assert_eq!(runtime.close_session(&id), Err(CasError::IoUnavailable));
        assert_eq!(
            runtime.process_ecm(&id, &section(0x82, &[1])),
            Err(CasError::SessionNotOpened)
        );
        assert_eq!(
            runtime.cleanup_errors().unwrap(),
            vec![super::CleanupErrors {
                revoke: Some(CasError::IoUnavailable),
                close: None
            }]
        );
        *publisher.revoke_error.lock().unwrap() = None;
        runtime.close_session(&id).unwrap();
        assert_eq!(*router.calls.lock().unwrap(), vec!["open", "close"]);
        assert_eq!(
            *publisher.calls.lock().unwrap(),
            vec!["reserve", "revoke", "revoke"]
        );
        assert_eq!(runtime.close_session(&id), Err(CasError::SessionNotOpened));
    }

    #[test]
    fn release_retries_lower_close_without_repeating_successful_revoke() {
        let (router, publisher, runtime) = runtime(CasSystem::B1);
        runtime.open_session_default().unwrap();
        *router.close_error.lock().unwrap() = Some(CasError::Timeout);
        assert_eq!(runtime.release(), Err(CasError::Timeout));
        assert_eq!(runtime.release(), Err(CasError::Timeout));
        assert_eq!(runtime.set_private_data(&[1]), Err(CasError::InvalidState));
        *router.close_error.lock().unwrap() = None;
        runtime.release().unwrap();
        runtime.release().unwrap();
        assert_eq!(*publisher.calls.lock().unwrap(), vec!["reserve", "revoke"]);
        assert_eq!(
            *router.calls.lock().unwrap(),
            vec!["open", "close", "close", "close"]
        );
    }

    #[test]
    fn failed_open_keeps_reservation_cleanup_owned() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        *router.open_error.lock().unwrap() = Some(CasError::NoCard);
        *publisher.revoke_error.lock().unwrap() = Some(CasError::IoUnavailable);
        assert_eq!(runtime.open_session_default(), Err(CasError::IoUnavailable));
        assert_eq!(runtime.cleanup_errors().unwrap().len(), 1);
        *publisher.revoke_error.lock().unwrap() = None;
        runtime.release().unwrap();
        assert_eq!(*router.calls.lock().unwrap(), vec!["open"]);
        assert!(runtime.lock_state().unwrap().sessions.is_empty());
    }

    #[test]
    fn fatal_private_data_failure_revokes_published_key() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        let id = runtime.open_session_default().unwrap();
        runtime.process_ecm(&id, &section(0x82, &[1])).unwrap();
        *router.private_error.lock().unwrap() = Some(CasError::NoCard);
        assert_eq!(
            runtime.set_session_private_data(&id, &[2]),
            Err(CasError::NoCard)
        );
        assert_eq!(
            *publisher.calls.lock().unwrap(),
            vec!["reserve", "publish", "revoke"]
        );
        assert_eq!(
            runtime.process_ecm(&id, &section(0x82, &[1])),
            Err(CasError::SessionNotOpened)
        );
        runtime.close_session(&id).unwrap();
    }

    #[test]
    fn service_owner_retains_and_retries_after_plugin_owner_drops() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        let runtime = Arc::new(runtime);
        let weak = Arc::downgrade(&runtime);
        let owner = super::CasCleanupOwner::default();
        owner.track(runtime.clone()).unwrap();
        runtime.open_session_default().unwrap();
        *publisher.revoke_error.lock().unwrap() = Some(CasError::IoUnavailable);
        *router.close_error.lock().unwrap() = Some(CasError::Timeout);
        assert_eq!(runtime.release(), Err(CasError::IoUnavailable));
        assert_eq!(
            runtime.cleanup_errors().unwrap(),
            vec![super::CleanupErrors {
                revoke: Some(CasError::IoUnavailable),
                close: Some(CasError::Timeout)
            }]
        );
        drop(runtime);
        assert!(weak.upgrade().is_some());
        assert_eq!(owner.retry_pending(), Err(CasError::IoUnavailable));
        *publisher.revoke_error.lock().unwrap() = None;
        *router.close_error.lock().unwrap() = None;
        owner.retry_pending().unwrap();
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn release_during_open_waits_for_io_owner_then_closes_returned_path() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        let runtime = Arc::new(runtime);
        let entered = Arc::new(std::sync::Barrier::new(2));
        let resume = Arc::new(std::sync::Barrier::new(2));
        *router.open_barriers.lock().unwrap() = Some((entered.clone(), resume.clone()));
        let opening = runtime.clone();
        let worker = std::thread::spawn(move || opening.open_session_default());
        entered.wait();
        assert_eq!(runtime.release(), Err(CasError::ResourceBusy));
        resume.wait();
        assert_eq!(worker.join().unwrap(), Err(CasError::InvalidState));
        runtime.release().unwrap();
        assert_eq!(*router.calls.lock().unwrap(), vec!["open", "close"]);
        assert_eq!(*publisher.calls.lock().unwrap(), vec!["reserve", "revoke"]);
        assert!(runtime.lock_state().unwrap().sessions.is_empty());
    }

    #[test]
    fn fatal_ecm_revoke_is_retried_while_plugin_remains_live() {
        let (router, publisher, runtime) = runtime(CasSystem::B25);
        let runtime = Arc::new(runtime);
        let owner = super::CasCleanupOwner::default();
        owner.track(runtime.clone()).unwrap();
        let id = runtime.open_session_default().unwrap();
        *router.ecm_error.lock().unwrap() = Some(CasError::NoLicense);
        *publisher.revoke_error.lock().unwrap() = Some(CasError::IoUnavailable);
        assert_eq!(
            runtime.process_ecm(&id, &section(0x82, &[1])),
            Err(CasError::IoUnavailable)
        );
        *publisher.revoke_error.lock().unwrap() = None;
        owner.retry_pending().unwrap();
        assert!(runtime.cleanup_errors().unwrap().is_empty());
        assert_eq!(
            *publisher.calls.lock().unwrap(),
            vec!["reserve", "revoke", "revoke"]
        );
        runtime.close_session(&id).unwrap();
    }

    #[test]
    fn aidl_service_specific_codes_are_positive() {
        for (error, expected) in [
            (CasError::NoLicense, 1),
            (CasError::LicenseExpired, 2),
            (CasError::SessionNotOpened, 3),
            (CasError::CannotHandle, 4),
            (CasError::InvalidState, 5),
            (CasError::BadValue, 6),
            (CasError::NotProvisioned, 7),
            (CasError::ResourceBusy, 8),
            (CasError::Unknown, 14),
            (CasError::NoCard, 17),
            (CasError::CardMute, 18),
            (CasError::CardInvalid, 19),
            (CasError::Timeout, 5),
        ] {
            assert_eq!(error.service_specific_code(), expected);
        }
    }
}
