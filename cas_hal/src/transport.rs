use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write};
use std::net::Shutdown;
use std::ops::{Deref, DerefMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use maleicacid_cas_hal_core::{
    CasError, CasPathKind, CasPathOpenError, CasPathRouter, CasSystem, EcmKeyMaterial,
    GenerationSource, SessionIdGenerator, TunerKeyPublisher, MEDIA_CAS_SESSION_ID_MAX_BYTES,
};
use maleicacid_tuner_hal2_key_provisioning_bridge::{
    decode_response as decode_key_response, encode_command as encode_key_command, volatile_zeroize,
    KeyProvisioningCommand, KeyProvisioningStatus, Multi2KeyResource, ProvisioningIdentity,
    KEY_PROVISIONING_SOCKET_PATH,
};

const IO_DEADLINE: Duration = Duration::from_secs(2);
const KEY_BRIDGE_MAX_ATTEMPTS: usize = 3;
const CAS_SERVICE_KEY_PROVIDER_ID: u64 = 0x4d43_4153_4b45_5901;
const CAS_PATH_MAX_FRAME_BYTES: usize = 4_256;
const PATH_REQUEST_HEADER_BYTES: usize = 32;
const PATH_RESPONSE_HEADER_BYTES: usize = 20;
const PATH_REQUEST_MAGIC: [u8; 4] = *b"MCAS";
const PATH_RESPONSE_MAGIC: [u8; 4] = *b"MCAR";
const PATH_WIRE_VERSION: u8 = 1;
const ECM_KEY_RESPONSE_BYTES: usize = 56;

const B25_SMARTCARD_SOCKET: &str = "/dev/socket/maleicacid_cas_b25_smartcard";
const B1_SMARTCARD_SOCKET: &str = "/dev/socket/maleicacid_cas_b1_smartcard";
const YAKISOBA_SOCKET: &str = "/dev/socket/maleicacid_cas_yakisoba";

struct ZeroizingBytes(Vec<u8>);

impl Deref for ZeroizingBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ZeroizingBytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for ZeroizingBytes {
    fn drop(&mut self) {
        volatile_zeroize(&mut self.0);
    }
}

fn io_error(error: Error, request_started: bool) -> CasError {
    match error.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => CasError::Timeout,
        _ if request_started => CasError::InvalidState,
        _ => CasError::IoUnavailable,
    }
}

fn remaining(deadline: Instant) -> Result<Duration, CasError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(CasError::Timeout)
}

fn connect_before(endpoint: &str, deadline: Instant) -> Result<UnixStream, CasError> {
    // sockaddr_unは整数と固定長配列だけからなり、全zeroで初期化できる。
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let path = endpoint.as_bytes();
    if path.is_empty() || path.len() >= address.sun_path.len() || path.contains(&0) {
        return Err(CasError::BadValue);
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, source) in address.sun_path.iter_mut().zip(path) {
        *target = *source as libc::c_char;
    }
    // 作成済みfdだけをOwnedFdへ渡し、全失敗分岐でRAII closeする。
    let raw = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if raw < 0 {
        return Err(io_error(Error::last_os_error(), false));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let length = (std::mem::size_of::<libc::sa_family_t>() + path.len() + 1) as libc::socklen_t;
    // addressはlength以上の有効領域を持ち、connectは参照を保持しない。
    let connected = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            &address as *const _ as *const libc::sockaddr,
            length,
        )
    };
    if connected != 0 {
        let error = Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(io_error(error, false));
        }
        loop {
            let timeout = remaining(deadline)?.as_millis().clamp(1, i32::MAX as u128) as i32;
            let mut poll_fd = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            // poll_fdは1要素の有効なpollfd。期限を再計算し、EINTRで延長しない。
            let result = unsafe { libc::poll(&mut poll_fd, 1, timeout) };
            if result == 0 {
                return Err(CasError::Timeout);
            }
            if result < 0 {
                let error = Error::last_os_error();
                if error.kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Err(io_error(error, false));
            }
            let mut socket_error: libc::c_int = 0;
            let mut error_len = std::mem::size_of_val(&socket_error) as libc::socklen_t;
            // SO_ERRORはc_intを1個だけ書き込む。buffer長も同じ型から算出する。
            let status = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    &mut socket_error as *mut _ as *mut libc::c_void,
                    &mut error_len,
                )
            };
            if status != 0 {
                return Err(io_error(Error::last_os_error(), false));
            }
            if socket_error != 0 {
                return Err(io_error(Error::from_raw_os_error(socket_error), false));
            }
            break;
        }
    }
    let stream = UnixStream::from(fd);
    stream
        .set_nonblocking(false)
        .map_err(|error| io_error(error, false))?;
    Ok(stream)
}

fn exchange_stream(
    mut stream: UnixStream,
    request: &[u8],
    max_response: usize,
    deadline: Instant,
) -> Result<ZeroizingBytes, CasError> {
    let mut written = 0;
    while written < request.len() {
        stream
            .set_write_timeout(Some(remaining(deadline)?))
            .map_err(|error| io_error(error, written != 0))?;
        match stream.write(&request[written..]) {
            Ok(0) => return Err(CasError::InvalidState),
            Ok(count) => written += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(error, true)),
        }
    }
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| io_error(error, true))?;
    let mut frame = ZeroizingBytes(vec![0; max_response + 1]);
    let mut received = 0;
    loop {
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|error| io_error(error, true))?;
        match stream.read(&mut frame[received..]) {
            Ok(0) => {
                frame.0.truncate(received);
                return Ok(frame);
            }
            Ok(count) => {
                received += count;
                if received > max_response {
                    return Err(CasError::InvalidState);
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(error, true)),
        }
    }
}

fn exchange_frame(
    endpoint: &str,
    request: &[u8],
    max_response: usize,
) -> Result<ZeroizingBytes, CasError> {
    let deadline = Instant::now() + IO_DEADLINE;
    exchange_stream(
        connect_before(endpoint, deadline)?,
        request,
        max_response,
        deadline,
    )
}

pub const CAS_CAPABILITY_PROFILE_PATH: &str = "/vendor/etc/maleicacid/cas_capabilities";
const CAS_CAPABILITY_PROFILE_MAX_BYTES: u64 = 4_096;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilitySnapshot {
    systems: Vec<CasSystem>,
    b25_path_profile: Option<B25PathProfile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B25PathProfile {
    SmartCardOnly,
    PreferSmartCardThenYakisoba,
    YakisobaOnly,
}

impl CapabilitySnapshot {
    pub fn load(path: &str) -> Result<Self, CasError> {
        let file = File::open(path).map_err(|_| CasError::IoUnavailable)?;
        let mut profile = String::new();
        file.take(CAS_CAPABILITY_PROFILE_MAX_BYTES + 1)
            .read_to_string(&mut profile)
            .map_err(|_| CasError::BadValue)?;
        if profile.len() as u64 > CAS_CAPABILITY_PROFILE_MAX_BYTES {
            return Err(CasError::BadValue);
        }
        Self::parse(&profile)
    }

    pub fn parse(profile: &str) -> Result<Self, CasError> {
        let mut systems = Vec::new();
        let mut snapshot_b25_profile = None;
        for raw_line in profile.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (system, b25_profile) = match line {
                "b25-smartcard" => (CasSystem::B25, Some(B25PathProfile::SmartCardOnly)),
                "b25-smartcard-yakisoba" => (
                    CasSystem::B25,
                    Some(B25PathProfile::PreferSmartCardThenYakisoba),
                ),
                "b25-yakisoba" => (CasSystem::B25, Some(B25PathProfile::YakisobaOnly)),
                "b1-smartcard" => (CasSystem::B1, None),
                _ => return Err(CasError::BadValue),
            };
            if systems.contains(&system) {
                return Err(CasError::BadValue);
            }
            systems.push(system);
            if let Some(profile) = b25_profile {
                snapshot_b25_profile = Some(profile);
            }
        }
        Ok(Self {
            systems,
            b25_path_profile: snapshot_b25_profile,
        })
    }

    pub fn systems(&self) -> &[CasSystem] {
        &self.systems
    }

    pub fn supports(&self, system: CasSystem) -> bool {
        self.systems.contains(&system)
    }

    pub const fn b25_path_profile(&self) -> Option<B25PathProfile> {
        self.b25_path_profile
    }
}

#[derive(Clone, Copy)]
enum PathOperation {
    Open = 1,
    SetSessionPrivateData = 2,
    ProcessEcm = 3,
    ProcessEmm = 4,
    Close = 5,
}

#[derive(Clone, Copy)]
enum PathStatus {
    Ok,
    BadValue,
    CannotHandle,
    InvalidState,
    ResourceBusy,
    NoLicense,
    LicenseExpired,
    NotProvisioned,
    NoCard,
    CardMute,
    CardInvalid,
    IoUnavailable,
    Timeout,
    Unknown,
}

impl PathStatus {
    fn from_wire(value: u8) -> Result<Self, CasError> {
        match value {
            0 => Ok(Self::Ok),
            1 => Ok(Self::BadValue),
            2 => Ok(Self::CannotHandle),
            3 => Ok(Self::InvalidState),
            4 => Ok(Self::ResourceBusy),
            5 => Ok(Self::NoLicense),
            6 => Ok(Self::LicenseExpired),
            7 => Ok(Self::NotProvisioned),
            8 => Ok(Self::NoCard),
            9 => Ok(Self::CardMute),
            10 => Ok(Self::CardInvalid),
            11 => Ok(Self::IoUnavailable),
            12 => Ok(Self::Timeout),
            13 => Ok(Self::Unknown),
            _ => Err(CasError::Unknown),
        }
    }

    fn into_result(self) -> Result<(), CasError> {
        match self {
            Self::Ok => Ok(()),
            Self::BadValue => Err(CasError::BadValue),
            Self::CannotHandle => Err(CasError::CannotHandle),
            Self::InvalidState => Err(CasError::InvalidState),
            Self::ResourceBusy => Err(CasError::ResourceBusy),
            Self::NoLicense => Err(CasError::NoLicense),
            Self::LicenseExpired => Err(CasError::LicenseExpired),
            Self::NotProvisioned => Err(CasError::NotProvisioned),
            Self::NoCard => Err(CasError::NoCard),
            Self::CardMute => Err(CasError::CardMute),
            Self::CardInvalid => Err(CasError::CardInvalid),
            Self::IoUnavailable => Err(CasError::IoUnavailable),
            Self::Timeout => Err(CasError::Timeout),
            Self::Unknown => Err(CasError::Unknown),
        }
    }
}

struct PathResponse {
    path: CasPathKind,
    payload: Vec<u8>,
}

impl Drop for PathResponse {
    fn drop(&mut self) {
        volatile_zeroize(&mut self.payload);
    }
}

pub struct AtomicGenerationSource {
    next: AtomicU64,
}

impl AtomicGenerationSource {
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }
}

impl GenerationSource for AtomicGenerationSource {
    fn next_generation(&self) -> Result<u64, CasError> {
        self.next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(1)
            })
            .map_err(|_| CasError::GenerationExhausted)
    }
}

pub struct UrandomSessionIdGenerator;

impl SessionIdGenerator for UrandomSessionIdGenerator {
    fn next_session_id(&self) -> Result<Vec<u8>, CasError> {
        let mut bytes = [0u8; MEDIA_CAS_SESSION_ID_MAX_BYTES];
        let mut random = File::open("/dev/urandom").map_err(|_| CasError::IoUnavailable)?;
        random
            .read_exact(&mut bytes)
            .map_err(|_| CasError::IoUnavailable)?;
        Ok(bytes.to_vec())
    }
}

pub struct UnixTunerKeyPublisher {
    request_id: AtomicU64,
}

impl UnixTunerKeyPublisher {
    pub const fn new() -> Self {
        Self {
            request_id: AtomicU64::new(1),
        }
    }

    fn next_request_id(&self) -> Result<u64, CasError> {
        self.request_id
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(1)
            })
            .map_err(|_| CasError::GenerationExhausted)
    }

    fn exchange(&self, command: KeyProvisioningCommand) -> Result<(), CasError> {
        let request_id = self.next_request_id()?;
        let request = ZeroizingBytes(
            encode_key_command(request_id, &command).map_err(|_| CasError::BadValue)?,
        );
        let reserve_request = matches!(&command, KeyProvisioningCommand::Reserve { .. });

        let mut last_io_error = CasError::IoUnavailable;
        for attempt in 0..KEY_BRIDGE_MAX_ATTEMPTS {
            let exchange_result = (|| -> Result<KeyProvisioningStatus, CasError> {
                let response = exchange_frame(KEY_PROVISIONING_SOCKET_PATH, &request, 16)?;
                let response =
                    decode_key_response(&response).map_err(|_| CasError::InvalidState)?;
                if response.request_id != request_id {
                    return Err(CasError::InvalidState);
                }
                Ok(response.status)
            })();

            match exchange_result {
                Ok(status) => {
                    return match status {
                        KeyProvisioningStatus::Ok => Ok(()),
                        KeyProvisioningStatus::InvalidToken | KeyProvisioningStatus::Revoked
                            if reserve_request =>
                        {
                            Err(CasError::TokenCollision)
                        }
                        KeyProvisioningStatus::BadRequest | KeyProvisioningStatus::InvalidToken => {
                            Err(CasError::BadValue)
                        }
                        KeyProvisioningStatus::StaleEpoch
                        | KeyProvisioningStatus::Revoked
                        | KeyProvisioningStatus::InvalidState => Err(CasError::InvalidState),
                        KeyProvisioningStatus::ResourceBusy => Err(CasError::ResourceBusy),
                        KeyProvisioningStatus::Internal => Err(CasError::Unknown),
                    };
                }
                Err(CasError::IoUnavailable) => {
                    last_io_error = CasError::IoUnavailable;
                    if attempt + 1 == KEY_BRIDGE_MAX_ATTEMPTS {
                        return Err(last_io_error);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_io_error)
    }
}

impl TunerKeyPublisher for UnixTunerKeyPublisher {
    fn reserve(&self, key_token: &[u8], provider_generation: u64) -> Result<(), CasError> {
        let identity =
            ProvisioningIdentity::try_new(CAS_SERVICE_KEY_PROVIDER_ID, provider_generation)
                .map_err(|_| CasError::BadValue)?;
        self.exchange(KeyProvisioningCommand::Reserve {
            key_token: key_token.to_vec(),
            identity,
        })
    }

    fn publish(
        &self,
        key_token: &[u8],
        provider_generation: u64,
        key_epoch: u64,
        mut material: EcmKeyMaterial,
    ) -> Result<(), CasError> {
        let resource = Multi2KeyResource::try_new(
            CAS_SERVICE_KEY_PROVIDER_ID,
            provider_generation,
            key_epoch,
            std::mem::take(&mut material.system_key),
            std::mem::take(&mut material.cbc_initial_value),
            std::mem::take(&mut material.even_ks),
            std::mem::take(&mut material.odd_ks),
        )
        .map_err(|_| CasError::BadValue)?;
        self.exchange(KeyProvisioningCommand::Publish {
            key_token: key_token.to_vec(),
            resource,
        })
    }

    fn revoke(&self, key_token: &[u8], provider_generation: u64) -> Result<(), CasError> {
        let identity =
            ProvisioningIdentity::try_new(CAS_SERVICE_KEY_PROVIDER_ID, provider_generation)
                .map_err(|_| CasError::BadValue)?;
        self.exchange(KeyProvisioningCommand::Revoke {
            key_token: key_token.to_vec(),
            identity,
        })
    }
}

pub struct UnixCasPathRouter {
    request_id: AtomicU64,
    b25_profile: B25PathProfile,
}

impl UnixCasPathRouter {
    pub const fn for_b25_profile(profile: Option<B25PathProfile>) -> Self {
        Self {
            request_id: AtomicU64::new(1),
            b25_profile: match profile {
                Some(profile) => profile,
                None => B25PathProfile::SmartCardOnly,
            },
        }
    }

    fn next_request_id(&self) -> Result<u64, CasError> {
        self.request_id
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(1)
            })
            .map_err(|_| CasError::GenerationExhausted)
    }

    fn endpoint(system: CasSystem, path: CasPathKind) -> Result<&'static str, CasError> {
        match (system, path) {
            (CasSystem::B25, CasPathKind::SmartCard) => Ok(B25_SMARTCARD_SOCKET),
            (CasSystem::B1, CasPathKind::SmartCard) => Ok(B1_SMARTCARD_SOCKET),
            (CasSystem::B25, CasPathKind::Yakisoba) => Ok(YAKISOBA_SOCKET),
            (CasSystem::B1, CasPathKind::Yakisoba) => Err(CasError::CannotHandle),
        }
    }

    fn path_wire_value(path: CasPathKind) -> u8 {
        match path {
            CasPathKind::SmartCard => 1,
            CasPathKind::Yakisoba => 2,
        }
    }

    fn path_from_wire(value: u8) -> Result<CasPathKind, CasError> {
        match value {
            1 => Ok(CasPathKind::SmartCard),
            2 => Ok(CasPathKind::Yakisoba),
            _ => Err(CasError::Unknown),
        }
    }

    fn system_wire_value(system: CasSystem) -> u8 {
        match system {
            CasSystem::B25 => 1,
            CasSystem::B1 => 2,
        }
    }

    fn encode_path_request(
        request_id: u64,
        operation: PathOperation,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        generation: u64,
        payload: &[u8],
    ) -> Result<Vec<u8>, CasError> {
        if session_id.len() > MEDIA_CAS_SESSION_ID_MAX_BYTES
            || payload.len()
                > CAS_PATH_MAX_FRAME_BYTES - PATH_REQUEST_HEADER_BYTES - session_id.len()
        {
            return Err(CasError::BadValue);
        }
        let payload_len = u32::try_from(payload.len()).map_err(|_| CasError::BadValue)?;
        let mut output = Vec::with_capacity(
            PATH_REQUEST_HEADER_BYTES
                .saturating_add(session_id.len())
                .saturating_add(payload.len()),
        );
        output.extend_from_slice(&PATH_REQUEST_MAGIC);
        output.push(PATH_WIRE_VERSION);
        output.push(operation as u8);
        output.push(Self::system_wire_value(system));
        output.push(Self::path_wire_value(path));
        output.extend_from_slice(&request_id.to_be_bytes());
        output.extend_from_slice(&generation.to_be_bytes());
        output.push(session_id.len() as u8);
        output.extend_from_slice(&[0, 0, 0]);
        output.extend_from_slice(&payload_len.to_be_bytes());
        output.extend_from_slice(session_id);
        output.extend_from_slice(payload);
        Ok(output)
    }

    fn exchange(
        &self,
        operation: PathOperation,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        generation: u64,
        payload: &[u8],
    ) -> Result<PathResponse, CasError> {
        let request_id = self.next_request_id()?;
        let request = ZeroizingBytes(Self::encode_path_request(
            request_id, operation, system, path, session_id, generation, payload,
        )?);
        let endpoint = Self::endpoint(system, path)?;
        let frame = exchange_frame(endpoint, &request, CAS_PATH_MAX_FRAME_BYTES)?;
        Self::decode_path_response(&frame, request_id, operation, path)
    }

    fn decode_path_response(
        frame: &[u8],
        request_id: u64,
        operation: PathOperation,
        path: CasPathKind,
    ) -> Result<PathResponse, CasError> {
        if frame.len() < PATH_RESPONSE_HEADER_BYTES || frame.len() > CAS_PATH_MAX_FRAME_BYTES {
            return Err(CasError::Unknown);
        }
        if frame[0..4] != PATH_RESPONSE_MAGIC || frame[4] != PATH_WIRE_VERSION || frame[7] != 0 {
            return Err(CasError::Unknown);
        }
        let mut request_id_bytes = [0u8; 8];
        request_id_bytes.copy_from_slice(&frame[8..16]);
        if u64::from_be_bytes(request_id_bytes) != request_id {
            return Err(CasError::InvalidState);
        }
        let mut payload_len_bytes = [0u8; 4];
        payload_len_bytes.copy_from_slice(&frame[16..20]);
        let payload_len = u32::from_be_bytes(payload_len_bytes) as usize;
        if payload_len != frame.len() - PATH_RESPONSE_HEADER_BYTES {
            return Err(CasError::Unknown);
        }
        let response_path = Self::path_from_wire(frame[6])?;
        if response_path != path {
            return Err(CasError::InvalidState);
        }
        PathStatus::from_wire(frame[5])?.into_result()?;
        let expected = if matches!(operation, PathOperation::ProcessEcm) {
            ECM_KEY_RESPONSE_BYTES
        } else {
            0
        };
        if payload_len != expected {
            return Err(CasError::InvalidState);
        }
        Ok(PathResponse {
            path: response_path,
            payload: frame[PATH_RESPONSE_HEADER_BYTES..].to_vec(),
        })
    }

    fn open_on_path(
        &self,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        generation: u64,
        private_data: &[u8],
    ) -> Result<CasPathKind, CasPathOpenError> {
        self.exchange(
            PathOperation::Open,
            system,
            path,
            session_id,
            generation,
            private_data,
        )
        .map(|response| response.path)
        .map_err(|error| CasPathOpenError {
            error,
            cleanup_path: matches!(
                error,
                CasError::Timeout | CasError::InvalidState | CasError::Unknown
            )
            .then_some(path),
        })
    }

    fn may_fallback(error: CasError) -> bool {
        matches!(
            error,
            CasError::NoCard
                | CasError::CardInvalid
                | CasError::CannotHandle
                | CasError::IoUnavailable
        )
    }
}

impl CasPathRouter for UnixCasPathRouter {
    fn open_session(
        &self,
        system: CasSystem,
        session_id: &[u8],
        session_generation: u64,
        plugin_private_data: &[u8],
    ) -> Result<CasPathKind, CasPathOpenError> {
        if system == CasSystem::B25 && self.b25_profile == B25PathProfile::YakisobaOnly {
            return self.open_on_path(
                system,
                CasPathKind::Yakisoba,
                session_id,
                session_generation,
                plugin_private_data,
            );
        }
        match self.open_on_path(
            system,
            CasPathKind::SmartCard,
            session_id,
            session_generation,
            plugin_private_data,
        ) {
            Ok(path) => Ok(path),
            Err(error)
                if system == CasSystem::B25
                    && self.b25_profile == B25PathProfile::PreferSmartCardThenYakisoba
                    && Self::may_fallback(error.error) =>
            {
                self.open_on_path(
                    system,
                    CasPathKind::Yakisoba,
                    session_id,
                    session_generation,
                    plugin_private_data,
                )
            }
            Err(error) => Err(error),
        }
    }

    fn set_session_private_data(
        &self,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        session_generation: u64,
        private_data: &[u8],
    ) -> Result<(), CasError> {
        self.exchange(
            PathOperation::SetSessionPrivateData,
            system,
            path,
            session_id,
            session_generation,
            private_data,
        )
        .map(|_| ())
    }

    fn process_ecm(
        &self,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        session_generation: u64,
        ecm: &[u8],
    ) -> Result<EcmKeyMaterial, CasError> {
        let response = self.exchange(
            PathOperation::ProcessEcm,
            system,
            path,
            session_id,
            session_generation,
            ecm,
        )?;
        if response.payload.len() != ECM_KEY_RESPONSE_BYTES {
            return Err(CasError::Unknown);
        }
        let mut system_key = [0u8; 32];
        system_key.copy_from_slice(&response.payload[0..32]);
        let mut cbc_initial_value = [0u8; 8];
        cbc_initial_value.copy_from_slice(&response.payload[32..40]);
        let mut even_ks = [0u8; 8];
        even_ks.copy_from_slice(&response.payload[40..48]);
        let mut odd_ks = [0u8; 8];
        odd_ks.copy_from_slice(&response.payload[48..56]);
        Ok(EcmKeyMaterial {
            system_key,
            cbc_initial_value,
            even_ks,
            odd_ks,
        })
    }

    fn process_emm(&self, system: CasSystem, emm: &[u8]) -> Result<(), CasError> {
        if system == CasSystem::B1 {
            return Err(CasError::CannotHandle);
        }
        if self.b25_profile == B25PathProfile::YakisobaOnly {
            return self
                .exchange(
                    PathOperation::ProcessEmm,
                    system,
                    CasPathKind::Yakisoba,
                    &[],
                    0,
                    emm,
                )
                .map(|_| ());
        }
        match self.exchange(
            PathOperation::ProcessEmm,
            system,
            CasPathKind::SmartCard,
            &[],
            0,
            emm,
        ) {
            Ok(_) => Ok(()),
            Err(error)
                if self.b25_profile == B25PathProfile::PreferSmartCardThenYakisoba
                    && Self::may_fallback(error) =>
            {
                self.exchange(
                    PathOperation::ProcessEmm,
                    system,
                    CasPathKind::Yakisoba,
                    &[],
                    0,
                    emm,
                )
                .map(|_| ())
            }
            Err(error) => Err(error),
        }
    }

    fn close_session(
        &self,
        system: CasSystem,
        path: CasPathKind,
        session_id: &[u8],
        session_generation: u64,
    ) -> Result<(), CasError> {
        self.exchange(
            PathOperation::Close,
            system,
            path,
            session_id,
            session_generation,
            &[],
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use maleicacid_cas_hal_core::{CasError, CasSystem};
    use std::io::{Read, Write};

    use super::{B25PathProfile, CapabilitySnapshot};

    #[test]
    fn capability_profile_is_strict_and_stable() {
        let snapshot =
            CapabilitySnapshot::parse("# validated product profile\nb25-smartcard\nb1-smartcard\n")
                .unwrap();
        assert_eq!(snapshot.systems(), &[CasSystem::B25, CasSystem::B1]);
        assert!(snapshot.supports(CasSystem::B25));
        assert!(snapshot.supports(CasSystem::B1));
        assert_eq!(
            snapshot.b25_path_profile(),
            Some(B25PathProfile::SmartCardOnly)
        );
    }

    #[test]
    fn malformed_or_duplicate_capability_is_rejected() {
        assert_eq!(
            CapabilitySnapshot::parse("b25-smartcard\nb25-smartcard"),
            Err(CasError::BadValue)
        );
        assert_eq!(
            CapabilitySnapshot::parse("b1-yakisoba"),
            Err(CasError::BadValue)
        );
    }

    #[test]
    fn b25_debug_profiles_are_explicit_and_mutually_exclusive() {
        assert_eq!(
            CapabilitySnapshot::parse("b25-smartcard-yakisoba")
                .unwrap()
                .b25_path_profile(),
            Some(B25PathProfile::PreferSmartCardThenYakisoba)
        );
        assert_eq!(
            CapabilitySnapshot::parse("b25-yakisoba")
                .unwrap()
                .b25_path_profile(),
            Some(B25PathProfile::YakisobaOnly)
        );
        assert_eq!(
            CapabilitySnapshot::parse("b25-smartcard\nb25-yakisoba"),
            Err(CasError::BadValue)
        );
    }
    #[test]
    fn socket_timeout_cannot_enable_fallback() {
        let (client, mut server) = std::os::unix::net::UnixStream::pair().unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut request = Vec::new();
            server.read_to_end(&mut request).unwrap();
            wait.recv().unwrap();
        });
        let error = super::exchange_stream(
            client,
            &[1],
            16,
            std::time::Instant::now() + std::time::Duration::from_millis(50),
        )
        .err()
        .unwrap();
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(error, maleicacid_cas_hal_core::CasError::Timeout);
        assert!(!super::UnixCasPathRouter::may_fallback(error));
    }

    #[test]
    fn transport_errors_after_request_are_outcome_unknown() {
        for kind in [
            std::io::ErrorKind::BrokenPipe,
            std::io::ErrorKind::ConnectionReset,
        ] {
            let error = super::io_error(std::io::Error::from(kind), true);
            assert_eq!(error, maleicacid_cas_hal_core::CasError::InvalidState);
            assert!(!super::UnixCasPathRouter::may_fallback(error));
        }
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        drop(server);
        let error = super::exchange_stream(
            client,
            &[1],
            16,
            std::time::Instant::now() + super::IO_DEADLINE,
        )
        .err()
        .unwrap();
        assert!(!super::UnixCasPathRouter::may_fallback(error));
    }

    #[test]
    fn confirmed_probe_results_only_allow_defined_fallbacks() {
        use maleicacid_cas_hal_core::CasError;
        for error in [
            CasError::NoCard,
            CasError::CardInvalid,
            CasError::CannotHandle,
            CasError::IoUnavailable,
        ] {
            assert!(super::UnixCasPathRouter::may_fallback(error));
        }
        for error in [
            CasError::Timeout,
            CasError::Unknown,
            CasError::InvalidState,
            CasError::CardMute,
            CasError::NoLicense,
        ] {
            assert!(!super::UnixCasPathRouter::may_fallback(error));
        }
    }

    fn response(status: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = b"MCAR".to_vec();
        frame.extend_from_slice(&[1, status, 1, 0]);
        frame.extend_from_slice(&1u64.to_be_bytes());
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn response_success_requires_exact_operation_payload() {
        use super::{PathOperation, UnixCasPathRouter};
        use maleicacid_cas_hal_core::{CasError, CasPathKind};
        let parse = |bytes: &[u8], operation| {
            UnixCasPathRouter::decode_path_response(bytes, 1, operation, CasPathKind::SmartCard)
        };
        assert!(parse(&response(0, &[]), PathOperation::Open).is_ok());
        assert!(parse(&response(0, &[1]), PathOperation::Open).is_err());
        assert!(parse(&response(0, &[1; 56]), PathOperation::ProcessEcm).is_ok());
        for length in [0, 55, 57] {
            assert!(parse(&response(0, &vec![1; length]), PathOperation::ProcessEcm).is_err());
        }
        assert_eq!(
            parse(&response(8, &[]), PathOperation::Open).err(),
            Some(CasError::NoCard)
        );
        assert_eq!(
            parse(&response(12, &[]), PathOperation::Open).err(),
            Some(CasError::Timeout)
        );
    }

    #[test]
    fn malformed_response_cannot_commit_or_enable_fallback() {
        use super::{PathOperation, UnixCasPathRouter};
        use maleicacid_cas_hal_core::CasPathKind;
        for index in [0, 4, 6, 7, 15, 19] {
            let mut frame = response(0, &[]);
            frame[index] ^= 0x7f;
            let error = UnixCasPathRouter::decode_path_response(
                &frame,
                1,
                PathOperation::Open,
                CasPathKind::SmartCard,
            )
            .err()
            .unwrap();
            assert!(!UnixCasPathRouter::may_fallback(error));
        }
        assert!(UnixCasPathRouter::decode_path_response(
            &[],
            1,
            PathOperation::Open,
            CasPathKind::SmartCard
        )
        .is_err());
    }

    #[test]
    fn response_frame_is_bounded_even_when_peer_sends_more() {
        let (client, mut server) = std::os::unix::net::UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            let mut request = Vec::new();
            server.read_to_end(&mut request).unwrap();
            server.write_all(&[1; 17]).unwrap();
        });
        let error = super::exchange_stream(
            client,
            &[1],
            16,
            std::time::Instant::now() + super::IO_DEADLINE,
        )
        .err()
        .unwrap();
        worker.join().unwrap();
        assert_eq!(error, maleicacid_cas_hal_core::CasError::InvalidState);
    }

    #[test]
    fn deadline_includes_connection_and_does_not_accept_expired_io() {
        let error = super::connect_before(
            "/nonexistent/maleicacid-cas-test.sock",
            std::time::Instant::now() + super::IO_DEADLINE,
        )
        .err()
        .unwrap();
        assert_eq!(error, maleicacid_cas_hal_core::CasError::IoUnavailable);
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let error = super::exchange_stream(client, &[1], 16, std::time::Instant::now())
            .err()
            .unwrap();
        assert_eq!(error, maleicacid_cas_hal_core::CasError::Timeout);
    }
}
