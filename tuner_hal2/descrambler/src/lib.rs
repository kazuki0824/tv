//! tuner_hal2 descrambler module。
//!
//! `core` はMULTI2とpacket単位descramble logicを所有する。`runtime` はAIDL向けsession/key/PID状態を所有する。これは継承ではなく合成である。

pub mod core;
pub mod runtime;

pub use core::{
    descramble_ts_packet_in_place, multi2_decrypt_payload, multi2_encrypt_payload,
    packet_policy_for_descramble_failure, parse_ts_packet_header, DescrambleFailure,
    DescrambleOutcome, DescramblerKeySlot, KeyParity, Multi2KeyMaterial, Multi2PrepareError,
    PacketPolicyAction, PassThroughReason, PreparedMulti2Key, TsPacketHeader,
    DEFAULT_MULTI2_ROUNDS, NULL_PID,
};
pub use runtime::{
    add_pid_claim_with_session_txn, bind_demux_with_session_txn, cleanup_all_with_session_txn,
    clear_key_with_session_txn, remove_pid_claim_with_session_txn, replace_key_with_session_txn,
    DescramblerCleanupReport, DescramblerClearKeyTxnError, DescramblerKeyLookupError,
    DescramblerKeyRegistrationError, DescramblerKeyTable, DescramblerKeyToken,
    DescramblerKeyTokenError, DescramblerKeyTxnOps, DescramblerPid, DescramblerPidClaim,
    DescramblerPidClaimError, DescramblerReplaceKeyOutcome, DescramblerReplaceKeyTxnError,
    DescramblerRuntime, DescramblerSession, DescramblerSessionFailure,
    DescramblerSessionFailureKind, DescramblerSessionTxnStep, SourceFilterRef,
};

#[doc(hidden)]
pub mod test_support {
    use crate::{DescramblerKeyTable, DescramblerKeyToken};

    pub fn expire_key_token(table: &mut DescramblerKeyTable, token: &DescramblerKeyToken) {
        table.expire_for_test_support(token);
    }
}
