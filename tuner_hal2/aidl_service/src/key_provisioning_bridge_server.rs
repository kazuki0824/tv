use std::env;
use std::io;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

use maleicacid_tuner_hal2_key_provisioning_bridge::{
    KeyProvisioningReplayJournal, KeyProvisioningStatus, KEY_PROVISIONING_SOCKET_NAME,
};

use crate::key_provisioning_connection::process_key_provisioning_connection;
use crate::service_context::SharedAidlServiceContext;

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
    let value =
        env::var(variable).map_err(|_| KeyProvisioningServerStartError::MissingControlSocket)?;
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
) -> io::Result<()> {
    process_key_provisioning_connection(&mut stream, journal, |command| {
        match context.runtime().lock() {
            Ok(mut runtime) => runtime.apply_key_provisioning_command(command),
            Err(_) => KeyProvisioningStatus::InvalidState,
        }
    })
}

fn server_main(
    listener: UnixListener,
    context: SharedAidlServiceContext,
) -> Result<(), KeyProvisioningServerError> {
    let mut journal = KeyProvisioningReplayJournal::default();
    loop {
        match listener.accept() {
            Ok((stream, _)) => match handle_connection(stream, &context, &mut journal) {
                Ok(()) => {}
                Err(_) => continue,
            },
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
