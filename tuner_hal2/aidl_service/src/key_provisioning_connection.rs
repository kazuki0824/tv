use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use maleicacid_tuner_hal2_key_provisioning_bridge::{
    decode_command, encode_response, volatile_zeroize, KeyProvisioningCommand,
    KeyProvisioningReplayJournal, KeyProvisioningResponse, KeyProvisioningStatus, ReplayLookup,
    KEY_PROVISIONING_MAX_FRAME_BYTES,
};

const CONNECTION_DEADLINE: Duration = Duration::from_secs(2);

pub trait KeyProvisioningConnection: Read + Write {
    fn configure_read_timeout(&self, timeout: Duration) -> io::Result<()>;
    fn configure_write_timeout(&self, timeout: Duration) -> io::Result<()>;
}

impl KeyProvisioningConnection for UnixStream {
    fn configure_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(timeout))
    }

    fn configure_write_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.set_write_timeout(Some(timeout))
    }
}

pub fn process_key_provisioning_connection<Connection, Apply>(
    stream: &mut Connection,
    journal: &mut KeyProvisioningReplayJournal,
    mut apply_command: Apply,
) -> io::Result<()>
where
    Connection: KeyProvisioningConnection,
    Apply: FnMut(KeyProvisioningCommand) -> KeyProvisioningStatus,
{
    // Both deadlines are mandatory transport setup. No frame byte may be read,
    // decoded, journaled, or applied until both operations have succeeded.
    stream.configure_read_timeout(CONNECTION_DEADLINE)?;
    stream.configure_write_timeout(CONNECTION_DEADLINE)?;

    let mut frame = Vec::with_capacity(KEY_PROVISIONING_MAX_FRAME_BYTES);
    let read_result = (&mut *stream)
        .take(KEY_PROVISIONING_MAX_FRAME_BYTES.saturating_add(1) as u64)
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
                    let status = apply_command(command);
                    journal.record_key(request_id, replay_key, status);
                    status
                }
            };
            (request_id, status)
        }
        Some(Err(_)) | None => (0, KeyProvisioningStatus::BadRequest),
    };

    let response = encode_response(KeyProvisioningResponse { request_id, status });
    stream.write_all(&response)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::{Cursor, Read, Write};

    use maleicacid_tuner_hal2_key_provisioning_bridge::{
        decode_response, encode_command, KeyProvisioningCommand, KeyProvisioningReplayJournal,
        KeyProvisioningStatus, ProvisioningIdentity,
    };

    use super::{process_key_provisioning_connection, KeyProvisioningConnection};

    struct FakeConnection {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
        fail_read_timeout: bool,
        fail_write_timeout: bool,
        read_timeout_calls: Cell<usize>,
        write_timeout_calls: Cell<usize>,
        read_calls: usize,
    }

    impl FakeConnection {
        fn new(input: Vec<u8>) -> Self {
            Self {
                input: Cursor::new(input),
                output: Vec::new(),
                fail_read_timeout: false,
                fail_write_timeout: false,
                read_timeout_calls: Cell::new(0),
                write_timeout_calls: Cell::new(0),
                read_calls: 0,
            }
        }
    }

    impl Read for FakeConnection {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.read_calls = self.read_calls.saturating_add(1);
            self.input.read(buffer)
        }
    }

    impl Write for FakeConnection {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl KeyProvisioningConnection for FakeConnection {
        fn configure_read_timeout(&self, _timeout: std::time::Duration) -> std::io::Result<()> {
            self.read_timeout_calls
                .set(self.read_timeout_calls.get().saturating_add(1));
            if self.fail_read_timeout {
                Err(std::io::Error::other("injected read timeout setup failure"))
            } else {
                Ok(())
            }
        }

        fn configure_write_timeout(&self, _timeout: std::time::Duration) -> std::io::Result<()> {
            self.write_timeout_calls
                .set(self.write_timeout_calls.get().saturating_add(1));
            if self.fail_write_timeout {
                Err(std::io::Error::other(
                    "injected write timeout setup failure",
                ))
            } else {
                Ok(())
            }
        }
    }

    fn reserve_frame() -> Vec<u8> {
        let identity = ProvisioningIdentity::try_new(41, 7).expect("valid identity");
        encode_command(
            91,
            &KeyProvisioningCommand::Reserve {
                key_token: vec![0x31, 0x32],
                identity,
            },
        )
        .expect("valid reserve frame")
    }

    #[test]
    fn read_timeout_failure_skips_decode_journal_and_runtime_mutation() {
        let mut stream = FakeConnection::new(reserve_frame());
        stream.fail_read_timeout = true;
        let mut journal = KeyProvisioningReplayJournal::default();
        let mut runtime_mutations = 0usize;

        let result = process_key_provisioning_connection(&mut stream, &mut journal, |_| {
            runtime_mutations = runtime_mutations.saturating_add(1);
            KeyProvisioningStatus::Ok
        });

        assert!(result.is_err());
        assert_eq!(stream.read_timeout_calls.get(), 1);
        assert_eq!(stream.write_timeout_calls.get(), 0);
        assert_eq!(stream.read_calls, 0);
        assert!(stream.output.is_empty());
        assert!(journal.is_empty());
        assert_eq!(runtime_mutations, 0);
    }

    #[test]
    fn write_timeout_failure_skips_decode_journal_and_runtime_mutation() {
        let mut stream = FakeConnection::new(reserve_frame());
        stream.fail_write_timeout = true;
        let mut journal = KeyProvisioningReplayJournal::default();
        let mut runtime_mutations = 0usize;

        let result = process_key_provisioning_connection(&mut stream, &mut journal, |_| {
            runtime_mutations = runtime_mutations.saturating_add(1);
            KeyProvisioningStatus::Ok
        });

        assert!(result.is_err());
        assert_eq!(stream.read_timeout_calls.get(), 1);
        assert_eq!(stream.write_timeout_calls.get(), 1);
        assert_eq!(stream.read_calls, 0);
        assert!(stream.output.is_empty());
        assert!(journal.is_empty());
        assert_eq!(runtime_mutations, 0);
    }

    #[test]
    fn configured_deadlines_continue_through_decode_journal_and_runtime_apply() {
        let mut stream = FakeConnection::new(reserve_frame());
        let mut journal = KeyProvisioningReplayJournal::default();
        let mut runtime_mutations = 0usize;

        process_key_provisioning_connection(&mut stream, &mut journal, |command| {
            assert!(matches!(command, KeyProvisioningCommand::Reserve { .. }));
            runtime_mutations = runtime_mutations.saturating_add(1);
            KeyProvisioningStatus::Ok
        })
        .expect("configured connection must process the request");

        assert_eq!(stream.read_timeout_calls.get(), 1);
        assert_eq!(stream.write_timeout_calls.get(), 1);
        assert!(stream.read_calls > 0);
        assert_eq!(journal.len(), 1);
        assert_eq!(runtime_mutations, 1);
        let response = decode_response(&stream.output).expect("valid response");
        assert_eq!(response.request_id, 91);
        assert_eq!(response.status, KeyProvisioningStatus::Ok);
    }
}
