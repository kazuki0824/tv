use std::env;
use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

use maleicacid_tuner_hal2_key_provisioning_bridge::{
    decode_command, encode_response, volatile_zeroize, KeyProvisioningReplayJournal,
    KeyProvisioningResponse, KeyProvisioningStatus, ReplayLookup, KEY_PROVISIONING_MAX_FRAME_BYTES,
    KEY_PROVISIONING_SOCKET_NAME,
};

use crate::service_context::SharedAidlServiceContext;

const CONNECTION_DEADLINE: Duration = Duration::from_secs(2);
const ACCEPT_RETRY_BACKOFF: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub(crate) enum KeyProvisioningServerStartError {
    MissingControlSocket,
    InvalidControlSocket,
    WorkerSpawn,
}

#[derive(Debug)]
enum KeyProvisioningServerError {
    FatalAccept,
}

fn control_socket_fd() -> Result<RawFd, KeyProvisioningServerStartError> {
    let variable = format!("ANDROID_SOCKET_{KEY_PROVISIONING_SOCKET_NAME}");
    let value = env::var(variable).map_err(|_| KeyProvisioningServerStartError::MissingControlSocket)?;
    let fd = value
        .parse::<RawFd>()
        .map_err(|_| KeyProvisioningServerStartError::InvalidControlSocket)?;
    if fd < 0 {
        return Err(KeyProvisioningServerStartError::InvalidControlSocket);
    }
    Ok(fd)
}

fn accept_error_is_fatal(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(9 | 22 | 88))
}

fn handle_connection(
    mut stream: UnixStream,
    context: &SharedAidlServiceContext,
    journal: &mut KeyProvisioningReplayJournal,
) {
    let _ = stream.set_read_timeout(Some(CONNECTION_DEADLINE));
    let _ = stream.set_write_timeout(Some(CONNECTION_DEADLINE));
    let mut frame = Vec::with_capacity(KEY_PROVISIONING_MAX_FRAME_BYTES);
    let read_result = (&mut stream)
        .take((KEY_PROVISIONING_MAX_FRAME_BYTES + 1) as u64)
        .read_to_end(&mut frame);
    let decoded = match read_result {
        Ok(_) => Some(decode_command(&frame)),
        Err(_) => None,
    };
    volatile_zeroize(&mut frame);

    let (request_id, status) = match decoded {
        Some(Ok((request_id, command))) => {
            let replay_key = command.replay_key();
            let status = match journal.lookup_key(request_id, &replay_key) {
                ReplayLookup::Hit(status) => status,
                ReplayLookup::Conflict => KeyProvisioningStatus::BadRequest,
                ReplayLookup::Miss => {
                    let status = match context.runtime().lock() {
                        Ok(mut runtime) => runtime.apply_key_provisioning_command(command),
                        Err(_) => KeyProvisioningStatus::InvalidState,
                    };
                    journal.record_key(request_id, replay_key, status);
                    status
                }
            };
            (request_id, status)
        }
        Some(Err(_)) | None => (0, KeyProvisioningStatus::BadRequest),
    };

    let response = encode_response(KeyProvisioningResponse { request_id, status });
    let _ = stream.write_all(&response);
    let _ = stream.flush();
}

fn server_main(
    listener: UnixListener,
    context: SharedAidlServiceContext,
) -> Result<(), KeyProvisioningServerError> {
    let mut journal = KeyProvisioningReplayJournal::default();
    loop {
        match listener.accept() {
            Ok((stream, _)) => handle_connection(stream, &context, &mut journal),
            Err(error) if accept_error_is_fatal(&error) => {
                return Err(KeyProvisioningServerError::FatalAccept)
            }
            Err(_) => thread::sleep(ACCEPT_RETRY_BACKOFF),
        }
    }
}

pub(crate) fn start_key_provisioning_server(
    context: SharedAidlServiceContext,
) -> Result<(), KeyProvisioningServerStartError> {
    let fd = control_socket_fd()?;
    let listener = unsafe { UnixListener::from_raw_fd(fd) };
    thread::Builder::new()
        .name("tuner-key-provisioning".to_owned())
        .spawn(move || {
            if server_main(listener, context.clone()).is_err() {
                if let Ok(mut runtime) = context.runtime().lock() {
                    runtime.mark_service_critical();
                }
            }
        })
        .map(|_| ())
        .map_err(|_| KeyProvisioningServerStartError::WorkerSpawn)
}
