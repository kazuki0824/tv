use std::collections::{BTreeMap, VecDeque};
use std::ptr;
use std::sync::atomic::{compiler_fence, Ordering};

pub const KEY_PROVISIONING_SOCKET_NAME: &str = "maleicacid_key_provisioning";
pub const KEY_PROVISIONING_SOCKET_PATH: &str = "/dev/socket/maleicacid_key_provisioning";
pub const KEY_PROVISIONING_MAX_FRAME_BYTES: usize = 160;
pub const DESCRAMBLER_KEY_TOKEN_MAX_BYTES: usize = 16;
pub const KEY_PROVISIONING_REPLAY_ENTRIES: usize = 64;

const REQUEST_MAGIC: [u8; 4] = *b"MKPR";
const RESPONSE_MAGIC: [u8; 4] = *b"MKPS";
const WIRE_VERSION: u8 = 1;
const REQUEST_HEADER_BYTES: usize = 20;
const RESPONSE_BYTES: usize = 16;

pub fn volatile_zeroize(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum KeyProvisioningStatus {
    Ok = 0,
    BadRequest = 1,
    InvalidToken = 2,
    StaleEpoch = 3,
    Revoked = 4,
    ResourceBusy = 5,
    InvalidState = 6,
    Internal = 7,
}

impl KeyProvisioningStatus {
    fn from_wire(value: u8) -> Result<Self, KeyProvisioningWireError> {
        match value {
            0 => Ok(Self::Ok),
            1 => Ok(Self::BadRequest),
            2 => Ok(Self::InvalidToken),
            3 => Ok(Self::StaleEpoch),
            4 => Ok(Self::Revoked),
            5 => Ok(Self::ResourceBusy),
            6 => Ok(Self::InvalidState),
            7 => Ok(Self::Internal),
            _ => Err(KeyProvisioningWireError::UnknownStatus(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisioningIdentityError {
    ZeroProviderId,
    ZeroProviderGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProvisioningIdentity {
    provider_id: u64,
    provider_generation: u64,
}

impl ProvisioningIdentity {
    pub fn try_new(
        provider_id: u64,
        provider_generation: u64,
    ) -> Result<Self, ProvisioningIdentityError> {
        if provider_id == 0 {
            return Err(ProvisioningIdentityError::ZeroProviderId);
        }
        if provider_generation == 0 {
            return Err(ProvisioningIdentityError::ZeroProviderGeneration);
        }
        Ok(Self {
            provider_id,
            provider_generation,
        })
    }

    pub const fn provider_id(self) -> u64 {
        self.provider_id
    }

    pub const fn provider_generation(self) -> u64 {
        self.provider_generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Multi2KeyResourceError {
    InvalidIdentity(ProvisioningIdentityError),
    ZeroKeyEpoch,
}

#[derive(Eq, PartialEq)]
pub struct Multi2KeyResource {
    identity: ProvisioningIdentity,
    key_epoch: u64,
    system_key: [u8; 32],
    cbc_initial_value: [u8; 8],
    even_ks: [u8; 8],
    odd_ks: [u8; 8],
}

impl std::fmt::Debug for Multi2KeyResource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Multi2KeyResource")
            .field("provider_id", &self.identity.provider_id())
            .field("provider_generation", &self.identity.provider_generation())
            .field("key_epoch", &self.key_epoch)
            .field("key_material", &"<redacted>")
            .finish()
    }
}

impl Multi2KeyResource {
    pub fn try_new(
        provider_id: u64,
        provider_generation: u64,
        key_epoch: u64,
        system_key: [u8; 32],
        cbc_initial_value: [u8; 8],
        even_ks: [u8; 8],
        odd_ks: [u8; 8],
    ) -> Result<Self, Multi2KeyResourceError> {
        let identity = ProvisioningIdentity::try_new(provider_id, provider_generation)
            .map_err(Multi2KeyResourceError::InvalidIdentity)?;
        if key_epoch == 0 {
            return Err(Multi2KeyResourceError::ZeroKeyEpoch);
        }
        Ok(Self {
            identity,
            key_epoch,
            system_key,
            cbc_initial_value,
            even_ks,
            odd_ks,
        })
    }

    pub const fn identity(&self) -> ProvisioningIdentity {
        self.identity
    }

    pub const fn key_epoch(&self) -> u64 {
        self.key_epoch
    }

    pub const fn system_key(&self) -> &[u8; 32] {
        &self.system_key
    }

    pub const fn cbc_initial_value(&self) -> &[u8; 8] {
        &self.cbc_initial_value
    }

    pub const fn even_ks(&self) -> &[u8; 8] {
        &self.even_ks
    }

    pub const fn odd_ks(&self) -> &[u8; 8] {
        &self.odd_ks
    }
}

impl Drop for Multi2KeyResource {
    fn drop(&mut self) {
        volatile_zeroize(&mut self.system_key);
        volatile_zeroize(&mut self.cbc_initial_value);
        volatile_zeroize(&mut self.even_ks);
        volatile_zeroize(&mut self.odd_ks);
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum KeyProvisioningCommand {
    Ping,
    Reserve {
        key_token: Vec<u8>,
        identity: ProvisioningIdentity,
    },
    Publish {
        key_token: Vec<u8>,
        resource: Multi2KeyResource,
    },
    Revoke {
        key_token: Vec<u8>,
        identity: ProvisioningIdentity,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum KeyProvisioningCommandKey {
    Ping,
    Reserve {
        key_token: Vec<u8>,
        provider_id: u64,
        provider_generation: u64,
    },
    Publish {
        key_token: Vec<u8>,
        provider_id: u64,
        provider_generation: u64,
        key_epoch: u64,
    },
    Revoke {
        key_token: Vec<u8>,
        provider_id: u64,
        provider_generation: u64,
    },
}

impl KeyProvisioningCommand {
    pub fn replay_key(&self) -> KeyProvisioningCommandKey {
        match self {
            Self::Ping => KeyProvisioningCommandKey::Ping,
            Self::Reserve {
                key_token,
                identity,
            } => KeyProvisioningCommandKey::Reserve {
                key_token: key_token.clone(),
                provider_id: identity.provider_id(),
                provider_generation: identity.provider_generation(),
            },
            Self::Publish {
                key_token,
                resource,
            } => KeyProvisioningCommandKey::Publish {
                key_token: key_token.clone(),
                provider_id: resource.identity().provider_id(),
                provider_generation: resource.identity().provider_generation(),
                key_epoch: resource.key_epoch(),
            },
            Self::Revoke {
                key_token,
                identity,
            } => KeyProvisioningCommandKey::Revoke {
                key_token: key_token.clone(),
                provider_id: identity.provider_id(),
                provider_generation: identity.provider_generation(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayLookup {
    Miss,
    Hit(KeyProvisioningStatus),
    Conflict,
}

#[derive(Debug)]
pub struct KeyProvisioningReplayJournal {
    entries: BTreeMap<u64, (KeyProvisioningCommandKey, KeyProvisioningStatus)>,
    order: VecDeque<u64>,
    max_entries: usize,
}

impl Default for KeyProvisioningReplayJournal {
    fn default() -> Self {
        Self::new(KEY_PROVISIONING_REPLAY_ENTRIES)
    }
}

impl KeyProvisioningReplayJournal {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            order: VecDeque::new(),
            max_entries,
        }
    }

    pub fn lookup(&self, request_id: u64, command: &KeyProvisioningCommand) -> ReplayLookup {
        self.lookup_key(request_id, &command.replay_key())
    }

    pub fn lookup_key(&self, request_id: u64, key: &KeyProvisioningCommandKey) -> ReplayLookup {
        let Some((stored_key, status)) = self.entries.get(&request_id) else {
            return ReplayLookup::Miss;
        };
        if stored_key == key {
            ReplayLookup::Hit(*status)
        } else {
            ReplayLookup::Conflict
        }
    }

    pub fn record(
        &mut self,
        request_id: u64,
        command: &KeyProvisioningCommand,
        status: KeyProvisioningStatus,
    ) {
        self.record_key(request_id, command.replay_key(), status);
    }

    pub fn record_key(
        &mut self,
        request_id: u64,
        key: KeyProvisioningCommandKey,
        status: KeyProvisioningStatus,
    ) {
        if self.max_entries == 0 || self.entries.contains_key(&request_id) {
            return;
        }
        while self.entries.len() >= self.max_entries {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.entries.insert(request_id, (key, status));
        self.order.push_back(request_id);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyProvisioningResponse {
    pub request_id: u64,
    pub status: KeyProvisioningStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyProvisioningWireError {
    FrameTooShort { len: usize, minimum: usize },
    FrameTooLong { len: usize, maximum: usize },
    BadRequestMagic,
    BadResponseMagic,
    UnsupportedVersion(u8),
    NonZeroReserved,
    UnknownOperation(u8),
    UnknownStatus(u8),
    PayloadLengthMismatch { declared: usize, actual: usize },
    InvalidTokenLength(usize),
    VoidKeyToken,
    InvalidResource(Multi2KeyResourceError),
    InvalidIdentity(ProvisioningIdentityError),
    UnexpectedPayloadLength { operation: u8, len: usize },
}

fn validate_key_token(key_token: &[u8]) -> Result<(), KeyProvisioningWireError> {
    if key_token.is_empty() || key_token.len() > DESCRAMBLER_KEY_TOKEN_MAX_BYTES {
        return Err(KeyProvisioningWireError::InvalidTokenLength(
            key_token.len(),
        ));
    }
    if key_token == [0] {
        return Err(KeyProvisioningWireError::VoidKeyToken);
    }
    Ok(())
}

fn append_request_header(
    output: &mut Vec<u8>,
    operation: u8,
    request_id: u64,
    payload_len: usize,
) -> Result<(), KeyProvisioningWireError> {
    let payload_len =
        u32::try_from(payload_len).map_err(|_| KeyProvisioningWireError::FrameTooLong {
            len: payload_len,
            maximum: u32::MAX as usize,
        })?;
    output.extend_from_slice(&REQUEST_MAGIC);
    output.push(WIRE_VERSION);
    output.push(operation);
    output.extend_from_slice(&[0, 0]);
    output.extend_from_slice(&request_id.to_be_bytes());
    output.extend_from_slice(&payload_len.to_be_bytes());
    Ok(())
}

pub fn encode_command(
    request_id: u64,
    command: &KeyProvisioningCommand,
) -> Result<Vec<u8>, KeyProvisioningWireError> {
    let mut payload = Vec::with_capacity(KEY_PROVISIONING_MAX_FRAME_BYTES);
    let operation = match command {
        KeyProvisioningCommand::Ping => 1,
        KeyProvisioningCommand::Publish {
            key_token,
            resource,
        } => {
            validate_key_token(key_token)?;
            payload.push(key_token.len() as u8);
            payload.extend_from_slice(key_token);
            payload.extend_from_slice(&resource.identity().provider_id().to_be_bytes());
            payload.extend_from_slice(&resource.identity().provider_generation().to_be_bytes());
            payload.extend_from_slice(&resource.key_epoch().to_be_bytes());
            payload.extend_from_slice(resource.system_key());
            payload.extend_from_slice(resource.cbc_initial_value());
            payload.extend_from_slice(resource.even_ks());
            payload.extend_from_slice(resource.odd_ks());
            2
        }
        KeyProvisioningCommand::Revoke {
            key_token,
            identity,
        } => {
            validate_key_token(key_token)?;
            payload.push(key_token.len() as u8);
            payload.extend_from_slice(key_token);
            payload.extend_from_slice(&identity.provider_id().to_be_bytes());
            payload.extend_from_slice(&identity.provider_generation().to_be_bytes());
            3
        }
        KeyProvisioningCommand::Reserve {
            key_token,
            identity,
        } => {
            validate_key_token(key_token)?;
            payload.push(key_token.len() as u8);
            payload.extend_from_slice(key_token);
            payload.extend_from_slice(&identity.provider_id().to_be_bytes());
            payload.extend_from_slice(&identity.provider_generation().to_be_bytes());
            4
        }
    };
    let frame_len = REQUEST_HEADER_BYTES.saturating_add(payload.len());
    if frame_len > KEY_PROVISIONING_MAX_FRAME_BYTES {
        volatile_zeroize(&mut payload);
        return Err(KeyProvisioningWireError::FrameTooLong {
            len: frame_len,
            maximum: KEY_PROVISIONING_MAX_FRAME_BYTES,
        });
    }
    let mut output = Vec::with_capacity(frame_len);
    append_request_header(&mut output, operation, request_id, payload.len())?;
    output.extend_from_slice(&payload);
    volatile_zeroize(&mut payload);
    Ok(output)
}

fn read_u64(bytes: &[u8]) -> u64 {
    let mut value = [0u8; 8];
    value.copy_from_slice(bytes);
    u64::from_be_bytes(value)
}

fn copy_array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut value = [0u8; N];
    value.copy_from_slice(bytes);
    value
}

pub fn decode_command(
    frame: &[u8],
) -> Result<(u64, KeyProvisioningCommand), KeyProvisioningWireError> {
    if frame.len() < REQUEST_HEADER_BYTES {
        return Err(KeyProvisioningWireError::FrameTooShort {
            len: frame.len(),
            minimum: REQUEST_HEADER_BYTES,
        });
    }
    if frame.len() > KEY_PROVISIONING_MAX_FRAME_BYTES {
        return Err(KeyProvisioningWireError::FrameTooLong {
            len: frame.len(),
            maximum: KEY_PROVISIONING_MAX_FRAME_BYTES,
        });
    }
    if frame[0..4] != REQUEST_MAGIC {
        return Err(KeyProvisioningWireError::BadRequestMagic);
    }
    if frame[4] != WIRE_VERSION {
        return Err(KeyProvisioningWireError::UnsupportedVersion(frame[4]));
    }
    if frame[6] != 0 || frame[7] != 0 {
        return Err(KeyProvisioningWireError::NonZeroReserved);
    }
    let operation = frame[5];
    let request_id = read_u64(&frame[8..16]);
    let payload_len = u32::from_be_bytes(copy_array(&frame[16..20])) as usize;
    let payload = &frame[REQUEST_HEADER_BYTES..];
    if payload_len != payload.len() {
        return Err(KeyProvisioningWireError::PayloadLengthMismatch {
            declared: payload_len,
            actual: payload.len(),
        });
    }
    let command = match operation {
        1 => {
            if !payload.is_empty() {
                return Err(KeyProvisioningWireError::UnexpectedPayloadLength {
                    operation,
                    len: payload.len(),
                });
            }
            KeyProvisioningCommand::Ping
        }
        2 => decode_publish(payload, operation)?,
        3 => decode_identity_command(payload, operation, false)?,
        4 => decode_identity_command(payload, operation, true)?,
        _ => return Err(KeyProvisioningWireError::UnknownOperation(operation)),
    };
    Ok((request_id, command))
}

fn decode_publish(
    payload: &[u8],
    operation: u8,
) -> Result<KeyProvisioningCommand, KeyProvisioningWireError> {
    let token_len = payload.first().copied().unwrap_or(0) as usize;
    if token_len == 0 || token_len > DESCRAMBLER_KEY_TOKEN_MAX_BYTES {
        return Err(KeyProvisioningWireError::InvalidTokenLength(token_len));
    }
    let expected = 1usize
        .saturating_add(token_len)
        .saturating_add(8 + 8 + 8 + 32 + 8 + 8 + 8);
    if payload.len() != expected {
        return Err(KeyProvisioningWireError::UnexpectedPayloadLength {
            operation,
            len: payload.len(),
        });
    }
    let token_end = 1 + token_len;
    let key_token = payload[1..token_end].to_vec();
    validate_key_token(&key_token)?;
    let mut cursor = token_end;
    let provider_id = read_u64(&payload[cursor..cursor + 8]);
    cursor += 8;
    let provider_generation = read_u64(&payload[cursor..cursor + 8]);
    cursor += 8;
    let key_epoch = read_u64(&payload[cursor..cursor + 8]);
    cursor += 8;
    let system_key = copy_array(&payload[cursor..cursor + 32]);
    cursor += 32;
    let cbc_initial_value = copy_array(&payload[cursor..cursor + 8]);
    cursor += 8;
    let even_ks = copy_array(&payload[cursor..cursor + 8]);
    cursor += 8;
    let odd_ks = copy_array(&payload[cursor..cursor + 8]);
    let resource = Multi2KeyResource::try_new(
        provider_id,
        provider_generation,
        key_epoch,
        system_key,
        cbc_initial_value,
        even_ks,
        odd_ks,
    )
    .map_err(KeyProvisioningWireError::InvalidResource)?;
    Ok(KeyProvisioningCommand::Publish {
        key_token,
        resource,
    })
}

fn decode_identity_command(
    payload: &[u8],
    operation: u8,
    reserve: bool,
) -> Result<KeyProvisioningCommand, KeyProvisioningWireError> {
    let token_len = payload.first().copied().unwrap_or(0) as usize;
    if token_len == 0 || token_len > DESCRAMBLER_KEY_TOKEN_MAX_BYTES {
        return Err(KeyProvisioningWireError::InvalidTokenLength(token_len));
    }
    let expected = 1usize.saturating_add(token_len).saturating_add(8 + 8);
    if payload.len() != expected {
        return Err(KeyProvisioningWireError::UnexpectedPayloadLength {
            operation,
            len: payload.len(),
        });
    }
    let token_end = 1 + token_len;
    let key_token = payload[1..token_end].to_vec();
    validate_key_token(&key_token)?;
    let provider_id = read_u64(&payload[token_end..token_end + 8]);
    let provider_generation = read_u64(&payload[token_end + 8..token_end + 16]);
    let identity = ProvisioningIdentity::try_new(provider_id, provider_generation)
        .map_err(KeyProvisioningWireError::InvalidIdentity)?;
    if reserve {
        Ok(KeyProvisioningCommand::Reserve {
            key_token,
            identity,
        })
    } else {
        Ok(KeyProvisioningCommand::Revoke {
            key_token,
            identity,
        })
    }
}

pub fn encode_response(response: KeyProvisioningResponse) -> [u8; RESPONSE_BYTES] {
    let mut output = [0u8; RESPONSE_BYTES];
    output[0..4].copy_from_slice(&RESPONSE_MAGIC);
    output[4] = WIRE_VERSION;
    output[5] = response.status as u8;
    output[8..16].copy_from_slice(&response.request_id.to_be_bytes());
    output
}

pub fn decode_response(frame: &[u8]) -> Result<KeyProvisioningResponse, KeyProvisioningWireError> {
    if frame.len() != RESPONSE_BYTES {
        return Err(if frame.len() < RESPONSE_BYTES {
            KeyProvisioningWireError::FrameTooShort {
                len: frame.len(),
                minimum: RESPONSE_BYTES,
            }
        } else {
            KeyProvisioningWireError::FrameTooLong {
                len: frame.len(),
                maximum: RESPONSE_BYTES,
            }
        });
    }
    if frame[0..4] != RESPONSE_MAGIC {
        return Err(KeyProvisioningWireError::BadResponseMagic);
    }
    if frame[4] != WIRE_VERSION {
        return Err(KeyProvisioningWireError::UnsupportedVersion(frame[4]));
    }
    if frame[6] != 0 || frame[7] != 0 {
        return Err(KeyProvisioningWireError::NonZeroReserved);
    }
    Ok(KeyProvisioningResponse {
        request_id: read_u64(&frame[8..16]),
        status: KeyProvisioningStatus::from_wire(frame[5])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ProvisioningIdentity {
        ProvisioningIdentity::try_new(41, 7).unwrap()
    }

    fn resource(epoch: u64) -> Multi2KeyResource {
        Multi2KeyResource::try_new(41, 7, epoch, [0x11; 32], [0x22; 8], [0x33; 8], [0x44; 8])
            .unwrap()
    }

    #[test]
    fn commands_round_trip() {
        let commands = [
            KeyProvisioningCommand::Reserve {
                key_token: vec![1, 2],
                identity: identity(),
            },
            KeyProvisioningCommand::Publish {
                key_token: vec![1, 2],
                resource: resource(3),
            },
            KeyProvisioningCommand::Revoke {
                key_token: vec![1, 2],
                identity: identity(),
            },
        ];
        for (index, command) in commands.into_iter().enumerate() {
            let frame = encode_command(index as u64 + 1, &command).unwrap();
            let (_, decoded) = decode_command(&frame).unwrap();
            assert_eq!(decoded, command);
        }
    }

    #[test]
    fn rejects_void_key_token() {
        let command = KeyProvisioningCommand::Reserve {
            key_token: vec![0],
            identity: identity(),
        };
        assert_eq!(
            encode_command(1, &command),
            Err(KeyProvisioningWireError::VoidKeyToken)
        );
    }

    #[test]
    fn identity_is_provider_opaque() {
        let first = ProvisioningIdentity::try_new(1, 9).unwrap();
        let second = ProvisioningIdentity::try_new(u64::MAX, 9).unwrap();
        assert_ne!(first.provider_id(), second.provider_id());
    }

    #[test]
    fn replay_journal_returns_old_result_without_key_material() {
        let mut journal = KeyProvisioningReplayJournal::new(2);
        let command = KeyProvisioningCommand::Publish {
            key_token: vec![9],
            resource: resource(1),
        };
        journal.record(10, &command, KeyProvisioningStatus::Ok);
        assert_eq!(
            journal.lookup(10, &command),
            ReplayLookup::Hit(KeyProvisioningStatus::Ok)
        );
        let debug = format!("{journal:?}");
        assert!(!debug.contains("17, 17"));
        assert!(!debug.contains("34, 34"));
    }

    #[test]
    fn reused_request_id_for_different_command_is_conflict() {
        let mut journal = KeyProvisioningReplayJournal::new(2);
        let first = KeyProvisioningCommand::Reserve {
            key_token: vec![1],
            identity: identity(),
        };
        let second = KeyProvisioningCommand::Reserve {
            key_token: vec![2],
            identity: identity(),
        };
        journal.record(1, &first, KeyProvisioningStatus::Ok);
        assert_eq!(journal.lookup(1, &second), ReplayLookup::Conflict);
    }

    #[test]
    fn replay_journal_is_bounded() {
        let mut journal = KeyProvisioningReplayJournal::new(1);
        let first = KeyProvisioningCommand::Reserve {
            key_token: vec![1],
            identity: identity(),
        };
        let second = KeyProvisioningCommand::Reserve {
            key_token: vec![2],
            identity: identity(),
        };
        journal.record(1, &first, KeyProvisioningStatus::Ok);
        journal.record(2, &second, KeyProvisioningStatus::Ok);
        assert_eq!(journal.len(), 1);
        assert_eq!(journal.lookup(1, &first), ReplayLookup::Miss);
        assert_eq!(
            journal.lookup(2, &second),
            ReplayLookup::Hit(KeyProvisioningStatus::Ok)
        );
    }

    #[test]
    fn resource_debug_redacts_keys() {
        let debug = format!("{:?}", resource(1));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("17, 17"));
    }
}
