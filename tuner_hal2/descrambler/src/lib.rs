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
    DescramblerCleanupReport, DescramblerKeyLookupError, DescramblerKeyRegistrationError,
    DescramblerKeyTable, DescramblerKeyToken, DescramblerKeyTokenError, DescramblerPid,
    DescramblerPidClaim, DescramblerPidClaimError, DescramblerRuntime, DescramblerSession,
    DescramblerSessionFailure, DescramblerSessionFailureKind, DescramblerSessionTxn,
    DescramblerSessionTxnStep, SourceFilterRef,
};

#[doc(hidden)]
pub mod test_support {
    use crate::{DescramblerKeyTable, DescramblerKeyToken};

    pub fn expire_key_token(table: &mut DescramblerKeyTable, token: &DescramblerKeyToken) {
        table.expire_for_test_support(token);
    }
}
