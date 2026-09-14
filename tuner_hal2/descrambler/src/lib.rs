//! tuner_hal2 descrambler module。
//!
//! `core` はMULTI2とpacket単位descramble logicを所有する。runtime transaction state は
//! `service_runtime` が所有し、この crate は domain value / DTO / validation を公開する。

mod cas_key_resolver;
mod core;
#[cfg(any(target_os = "android", test))]
mod product_parameters;
mod runtime;

pub use cas_key_resolver::{CasKeyResolveError, CasKeyResolver, ProductCasKeyResolver};

pub use core::{
    descramble_validated_ts_packet_in_place, multi2_decrypt_payload, multi2_encrypt_payload,
    packet_policy_for_descramble_failure, DescrambleFailure, DescrambleOutcome, DescramblerKeySlot,
    KeyParity, Multi2KeyMaterial, Multi2PrepareError, PacketPolicyAction, PassThroughReason,
    PreparedMulti2Key, DEFAULT_MULTI2_ROUNDS, NULL_PID,
};
pub use runtime::{
    DescramblerKeyToken, DescramblerKeyTokenError, DescramblerPid, DescramblerPidClaim,
    DescramblerPidClaimError, DescramblerPidClaimSource, SourceFilterRef,
};
