use crate::multi2::{
    multi2_decrypt_payload, Multi2KeyMaterial, Multi2PrepareError, PreparedMulti2Key,
};
use maleicacid_tuner_hal_common::TS_PACKET_SIZE;
use std::collections::BTreeSet;

pub const NULL_PID: u16 = 0x1fff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyParity {
    Even,
    Odd,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DescramblerKeySlot {
    even: Option<PreparedMulti2Key>,
    odd: Option<PreparedMulti2Key>,
}

impl DescramblerKeySlot {
    pub const fn empty() -> Self {
        Self {
            even: None,
            odd: None,
        }
    }

    pub fn try_with_even(mut self, key: Multi2KeyMaterial) -> Result<Self, Multi2PrepareError> {
        self.even = Some(key.prepare()?);
        Ok(self)
    }

    pub fn try_with_odd(mut self, key: Multi2KeyMaterial) -> Result<Self, Multi2PrepareError> {
        self.odd = Some(key.prepare()?);
        Ok(self)
    }

    pub fn with_even_prepared(mut self, key: PreparedMulti2Key) -> Self {
        self.even = Some(key);
        self
    }

    pub fn with_odd_prepared(mut self, key: PreparedMulti2Key) -> Self {
        self.odd = Some(key);
        self
    }

    pub fn key_for(&self, parity: KeyParity) -> Option<&PreparedMulti2Key> {
        match parity {
            KeyParity::Even => self.even.as_ref(),
            KeyParity::Odd => self.odd.as_ref(),
        }
    }

    pub fn has_any_key(&self) -> bool {
        self.even.is_some() || self.odd.is_some()
    }

    pub fn has_even_and_odd_keys(&self) -> bool {
        self.even.is_some() && self.odd.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PassThroughReason {
    ClearPacket,
    ClearAdaptationOnly,
    NullPid,
    PidNotTargeted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescrambleFailure {
    InvalidPacketSize,
    BadSyncByte,
    InvalidAfc,
    InvalidAdaptationField,
    InvalidTsc,
    TransportErrorRecord,
    ScrambledNullPid,
    ScrambledWithoutPayload,
    NoKey,
    BadToken,
    Multi2Fail,
    ScrambledPidNotRegistered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescrambleOutcome {
    Descrambled {
        pid: u16,
        parity: KeyParity,
        payload_offset: usize,
    },
    PassedThrough {
        pid: u16,
        reason: PassThroughReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TsPacketHeader {
    pub pid: u16,
    pub transport_error_indicator: bool,
    pub payload_unit_start: bool,
    pub transport_scrambling_control: u8,
    pub adaptation_field_control: u8,
    pub continuity_counter: u8,
    pub payload_offset: Option<usize>,
}

pub fn parse_ts_packet_header(packet: &[u8]) -> Result<TsPacketHeader, DescrambleFailure> {
    if packet.len() != TS_PACKET_SIZE {
        return Err(DescrambleFailure::InvalidPacketSize);
    }
    if packet[0] != 0x47 {
        return Err(DescrambleFailure::BadSyncByte);
    }

    let transport_scrambling_control = (packet[3] >> 6) & 0x03;
    let adaptation_field_control = (packet[3] >> 4) & 0x03;
    if adaptation_field_control == 0 {
        return Err(DescrambleFailure::InvalidAfc);
    }

    let mut offset = 4usize;
    if adaptation_field_control == 2 || adaptation_field_control == 3 {
        let adaptation_len = packet[offset] as usize;
        offset = offset
            .checked_add(1 + adaptation_len)
            .ok_or(DescrambleFailure::InvalidAdaptationField)?;
        if offset > packet.len() {
            return Err(DescrambleFailure::InvalidAdaptationField);
        }
    }

    if adaptation_field_control == 3 && offset >= TS_PACKET_SIZE {
        return Err(DescrambleFailure::InvalidAdaptationField);
    }

    Ok(TsPacketHeader {
        pid: (((packet[1] & 0x1f) as u16) << 8) | packet[2] as u16,
        transport_error_indicator: (packet[1] & 0x80) != 0,
        payload_unit_start: (packet[1] & 0x40) != 0,
        transport_scrambling_control,
        adaptation_field_control,
        continuity_counter: packet[3] & 0x0f,
        payload_offset: if adaptation_field_control == 2 {
            None
        } else {
            Some(offset)
        },
    })
}

pub fn descramble_ts_packet_in_place(
    packet: &mut [u8],
    target_pids: &BTreeSet<u16>,
    key_slot: &DescramblerKeySlot,
) -> Result<DescrambleOutcome, DescrambleFailure> {
    let header = parse_ts_packet_header(packet)?;
    if header.transport_error_indicator {
        return Err(DescrambleFailure::TransportErrorRecord);
    }
    match header.transport_scrambling_control {
        0 => {
            let reason = if header.pid == NULL_PID {
                PassThroughReason::NullPid
            } else if header.payload_offset.is_none() {
                PassThroughReason::ClearAdaptationOnly
            } else {
                PassThroughReason::ClearPacket
            };
            return Ok(DescrambleOutcome::PassedThrough {
                pid: header.pid,
                reason,
            });
        }
        1 => return Err(DescrambleFailure::InvalidTsc),
        2 | 3 => {
            if header.pid == NULL_PID {
                return Err(DescrambleFailure::ScrambledNullPid);
            }
        }
        _ => return Err(DescrambleFailure::InvalidTsc),
    }

    let parity = if header.transport_scrambling_control == 2 {
        KeyParity::Even
    } else {
        KeyParity::Odd
    };
    if !target_pids.contains(&header.pid) {
        return Err(DescrambleFailure::ScrambledPidNotRegistered);
    }
    let Some(payload_offset) = header.payload_offset else {
        return Err(DescrambleFailure::ScrambledWithoutPayload);
    };
    if payload_offset >= TS_PACKET_SIZE {
        return Err(DescrambleFailure::ScrambledWithoutPayload);
    }
    let key = key_slot.key_for(parity).ok_or(DescrambleFailure::NoKey)?;
    multi2_decrypt_payload(&mut packet[payload_offset..], key)
        .map_err(|_| DescrambleFailure::Multi2Fail)?;
    packet[3] &= 0x3f;
    Ok(DescrambleOutcome::Descrambled {
        pid: header.pid,
        parity,
        payload_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        multi2_decrypt_payload, multi2_encrypt_payload, Multi2KeyMaterial, Multi2PrepareError,
        PreparedMulti2Key,
    };
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;
    use std::collections::BTreeSet;

    fn sample_key(byte: u8) -> Multi2KeyMaterial {
        let mut system_key = [0u8; 32];
        for (i, b) in system_key.iter_mut().enumerate() {
            *b = byte.wrapping_add(i as u8);
        }
        let mut iv = [0u8; 8];
        for (i, b) in iv.iter_mut().enumerate() {
            *b = 0xa0u8.wrapping_add(byte).wrapping_add(i as u8);
        }
        let mut data_key = [0u8; 8];
        for (i, b) in data_key.iter_mut().enumerate() {
            *b = 0x40u8.wrapping_add(byte).wrapping_add((i * 3) as u8);
        }
        Multi2KeyMaterial::new(system_key, iv, data_key)
    }

    fn packet(pid: u16, tsc: u8, afc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0u8; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = ((pid >> 8) as u8) & 0x1f;
        p[2] = (pid & 0xff) as u8;
        p[3] = (tsc << 6) | (afc << 4) | 0x05;
        for i in 4..TS_PACKET_SIZE {
            p[i] = (i as u8).wrapping_mul(3).wrapping_add(1);
        }
        p
    }

    fn encrypt_payload_packet(
        mut p: [u8; TS_PACKET_SIZE],
        key: &PreparedMulti2Key,
    ) -> [u8; TS_PACKET_SIZE] {
        let off = parse_ts_packet_header(&p).unwrap().payload_offset.unwrap();
        multi2_encrypt_payload(&mut p[off..], key).unwrap();
        p
    }

    #[test]
    fn clear_packet_passes_byte_identical() {
        let mut p = packet(100, 0, 1);
        let original = p;
        let outcome = descramble_ts_packet_in_place(
            &mut p,
            &BTreeSet::from([100]),
            &DescramblerKeySlot::empty(),
        )
        .unwrap();
        assert_eq!(
            outcome,
            DescrambleOutcome::PassedThrough {
                pid: 100,
                reason: PassThroughReason::ClearPacket
            }
        );
        assert_eq!(p, original);
    }

    #[test]
    fn null_pid_clear_passes_as_null_pid() {
        let mut p = packet(NULL_PID, 0, 1);
        let original = p;
        let outcome = descramble_ts_packet_in_place(
            &mut p,
            &BTreeSet::from([NULL_PID]),
            &DescramblerKeySlot::empty(),
        )
        .unwrap();
        assert_eq!(
            outcome,
            DescrambleOutcome::PassedThrough {
                pid: NULL_PID,
                reason: PassThroughReason::NullPid
            }
        );
        assert_eq!(p, original);
    }

    #[test]
    fn null_pid_invalid_tsc_is_invalid_tsc() {
        let mut p = packet(NULL_PID, 1, 1);
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([NULL_PID]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidTsc)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn null_pid_scrambled_is_not_clear_null_pid() {
        let mut p = packet(NULL_PID, 2, 1);
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([NULL_PID]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::ScrambledNullPid)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn null_pid_scrambled_is_scrambled_null_pid() {
        let mut p = packet(NULL_PID, 3, 1);
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([NULL_PID]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::ScrambledNullPid)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn scrambled_adaptation_only_packet_is_failure() {
        let mut p = packet(100, 2, 2);
        p[4] = 183;
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::ScrambledWithoutPayload)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn afc10_clear_adaptation_only_remains_valid() {
        let mut p = packet(100, 0, 2);
        p[4] = 183;
        let original = p;
        let outcome = descramble_ts_packet_in_place(
            &mut p,
            &BTreeSet::from([100]),
            &DescramblerKeySlot::empty(),
        )
        .unwrap();
        assert_eq!(
            outcome,
            DescrambleOutcome::PassedThrough {
                pid: 100,
                reason: PassThroughReason::ClearAdaptationOnly
            }
        );
        assert_eq!(p, original);
    }

    #[test]
    fn afc_zero_is_invalid_before_tsc() {
        for tsc in 0..=3 {
            let mut p = packet(100, tsc, 0);
            assert_eq!(
                descramble_ts_packet_in_place(
                    &mut p,
                    &BTreeSet::from([100]),
                    &DescramblerKeySlot::empty()
                ),
                Err(DescrambleFailure::InvalidAfc)
            );
        }
    }

    #[test]
    fn header_parser_does_not_reject_tsc01_before_tei_priority() {
        let p = packet(100, 1, 1);
        let header = parse_ts_packet_header(&p).unwrap();
        assert_eq!(header.transport_scrambling_control, 1);
    }

    #[test]
    fn tei_invalid_tsc_prefers_transport_error_record() {
        let mut p = packet(100, 1, 1);
        p[1] |= 0x80;
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::TransportErrorRecord)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn non_tei_invalid_tsc_remains_invalid_tsc_after_header_parse() {
        let mut p = packet(100, 1, 1);
        assert_eq!(
            parse_ts_packet_header(&p)
                .unwrap()
                .transport_scrambling_control,
            1
        );
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidTsc)
        );
    }

    #[test]
    fn tei_packet_is_record_only_and_byte_identical() {
        let mut p = packet(100, 0, 1);
        p[1] |= 0x80;
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::TransportErrorRecord)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn tei_scrambled_packet_is_not_descrambled_even_with_key() {
        let even = sample_key(4);
        let even_prepared = even.prepare().unwrap();
        let clear = packet(100, 0, 1);
        let mut p = encrypt_payload_packet(clear, &even_prepared);
        p[1] |= 0x80;
        p[3] = (p[3] & 0x3f) | (2 << 6);
        let original = p;
        let keys = DescramblerKeySlot::empty().try_with_even(even).unwrap();
        assert_eq!(
            descramble_ts_packet_in_place(&mut p, &BTreeSet::from([100]), &keys),
            Err(DescrambleFailure::TransportErrorRecord)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn tei_packet_does_not_clear_tsc() {
        let mut p = packet(100, 2, 1);
        p[1] |= 0x80;
        let original_tsc = p[3] >> 6;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::TransportErrorRecord)
        );
        assert_eq!(p[3] >> 6, original_tsc);
    }

    #[test]
    fn tei_invalid_afc_prefers_invalid_afc() {
        let mut p = packet(100, 2, 0);
        p[1] |= 0x80;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidAfc)
        );
    }

    #[test]
    fn tei_invalid_adaptation_field_prefers_invalid_adaptation_field() {
        let mut p = packet(100, 2, 3);
        p[1] |= 0x80;
        p[4] = 183;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidAdaptationField)
        );
    }

    #[test]
    fn afc11_payload_zero_is_invalid_adaptation_field() {
        let mut p = packet(100, 0, 3);
        p[4] = 183;
        assert_eq!(
            parse_ts_packet_header(&p),
            Err(DescrambleFailure::InvalidAdaptationField)
        );
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidAdaptationField)
        );
    }

    #[test]
    fn afc11_payload_zero_is_record_only_not_clear() {
        let mut p = packet(100, 0, 3);
        p[4] = 183;
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidAdaptationField)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn scrambled_afc11_payload_zero_fails_at_header_validation() {
        let mut p = packet(100, 2, 3);
        p[4] = 183;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidAdaptationField)
        );
    }

    #[test]
    fn tsc_afc_matrix_matches_fixed_descrambler_contract() {
        let even = sample_key(1);
        let odd = sample_key(2);
        let keys = DescramblerKeySlot::empty()
            .try_with_even(even)
            .unwrap()
            .try_with_odd(odd)
            .unwrap();
        for tsc in 0..=3 {
            for afc in 0..=3 {
                let mut p = packet(0x0100 + ((tsc as u16) << 4) + afc as u16, tsc, afc);
                if afc == 2 {
                    p[4] = 183;
                }
                if afc == 3 {
                    p[4] = 0;
                }
                let result = descramble_ts_packet_in_place(
                    &mut p,
                    &BTreeSet::from([0x0100 + ((tsc as u16) << 4) + afc as u16]),
                    &keys,
                );
                match (tsc, afc) {
                    (_, 0) => assert_eq!(result, Err(DescrambleFailure::InvalidAfc)),
                    (0, 1) | (0, 3) => assert_eq!(
                        result,
                        Ok(DescrambleOutcome::PassedThrough {
                            pid: 0x0100 + ((tsc as u16) << 4) + afc as u16,
                            reason: PassThroughReason::ClearPacket,
                        })
                    ),
                    (0, 2) => assert_eq!(
                        result,
                        Ok(DescrambleOutcome::PassedThrough {
                            pid: 0x0100 + ((tsc as u16) << 4) + afc as u16,
                            reason: PassThroughReason::ClearAdaptationOnly,
                        })
                    ),
                    (1, 1) | (1, 2) | (1, 3) => {
                        assert_eq!(result, Err(DescrambleFailure::InvalidTsc))
                    }
                    (2, 1) => assert_eq!(
                        result,
                        Ok(DescrambleOutcome::Descrambled {
                            pid: 0x0100 + ((tsc as u16) << 4) + afc as u16,
                            parity: KeyParity::Even,
                            payload_offset: 4,
                        })
                    ),
                    (2, 2) => assert_eq!(result, Err(DescrambleFailure::ScrambledWithoutPayload)),
                    (2, 3) => assert_eq!(
                        result,
                        Ok(DescrambleOutcome::Descrambled {
                            pid: 0x0100 + ((tsc as u16) << 4) + afc as u16,
                            parity: KeyParity::Even,
                            payload_offset: 5,
                        })
                    ),
                    (3, 1) => assert_eq!(
                        result,
                        Ok(DescrambleOutcome::Descrambled {
                            pid: 0x0100 + ((tsc as u16) << 4) + afc as u16,
                            parity: KeyParity::Odd,
                            payload_offset: 4,
                        })
                    ),
                    (3, 2) => assert_eq!(result, Err(DescrambleFailure::ScrambledWithoutPayload)),
                    (3, 3) => assert_eq!(
                        result,
                        Ok(DescrambleOutcome::Descrambled {
                            pid: 0x0100 + ((tsc as u16) << 4) + afc as u16,
                            parity: KeyParity::Odd,
                            payload_offset: 5,
                        })
                    ),
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn adaptation_field_payload_offset_is_safe() {
        let mut p = packet(100, 2, 3);
        p[4] = 3;
        p[5] = 0;
        p[6] = 0xaa;
        p[7] = 0xbb;
        let header = parse_ts_packet_header(&p).unwrap();
        assert_eq!(header.payload_offset, Some(8));
    }

    #[test]
    fn even_key_is_selected_and_tsc_is_cleared() {
        let even = sample_key(1);
        let odd = sample_key(2);
        let clear = packet(100, 0, 1);
        let even_prepared = even.prepare().unwrap();
        let mut scrambled = encrypt_payload_packet(clear, &even_prepared);
        scrambled[3] = (scrambled[3] & 0x3f) | (2 << 6);
        let keys = DescramblerKeySlot::empty()
            .try_with_even(even)
            .unwrap()
            .try_with_odd(odd)
            .unwrap();
        let outcome =
            descramble_ts_packet_in_place(&mut scrambled, &BTreeSet::from([100]), &keys).unwrap();
        assert_eq!(
            outcome,
            DescrambleOutcome::Descrambled {
                pid: 100,
                parity: KeyParity::Even,
                payload_offset: 4
            }
        );
        assert_eq!(scrambled[3] >> 6, 0);
        assert_eq!(scrambled, clear);
    }

    #[test]
    fn odd_key_is_selected_and_tsc_is_cleared() {
        let even = sample_key(1);
        let odd = sample_key(2);
        let clear = packet(101, 0, 1);
        let odd_prepared = odd.prepare().unwrap();
        let mut scrambled = encrypt_payload_packet(clear, &odd_prepared);
        scrambled[3] = (scrambled[3] & 0x3f) | (3 << 6);
        let keys = DescramblerKeySlot::empty()
            .try_with_even(even)
            .unwrap()
            .try_with_odd(odd)
            .unwrap();
        descramble_ts_packet_in_place(&mut scrambled, &BTreeSet::from([101]), &keys).unwrap();
        assert_eq!(scrambled[3] >> 6, 0);
        assert_eq!(scrambled, clear);
    }

    #[test]
    fn invalid_tsc_is_rejected() {
        let mut p = packet(100, 1, 1);
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::InvalidTsc)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn failed_no_key_does_not_clear_tsc() {
        let mut p = packet(100, 2, 1);
        let original_tsc = p[3] >> 6;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([100]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::NoKey)
        );
        assert_eq!(p[3] >> 6, original_tsc);
    }

    #[test]
    fn clear_pid_not_targeted_passes_byte_identical() {
        let mut p = packet(100, 0, 1);
        let original = p;
        let outcome = descramble_ts_packet_in_place(
            &mut p,
            &BTreeSet::from([200]),
            &DescramblerKeySlot::empty(),
        )
        .unwrap();
        assert_eq!(
            outcome,
            DescrambleOutcome::PassedThrough {
                pid: 100,
                reason: PassThroughReason::ClearPacket
            }
        );
        assert_eq!(p, original);
    }

    #[test]
    fn scrambled_unregistered_pid_is_not_silently_decoded() {
        let mut p = packet(100, 2, 1);
        let original = p;
        assert_eq!(
            descramble_ts_packet_in_place(
                &mut p,
                &BTreeSet::from([200]),
                &DescramblerKeySlot::empty()
            ),
            Err(DescrambleFailure::ScrambledPidNotRegistered)
        );
        assert_eq!(p, original);
    }

    #[test]
    fn invalid_size_and_sync_are_rejected() {
        assert_eq!(
            parse_ts_packet_header(&[0u8; 187]),
            Err(DescrambleFailure::InvalidPacketSize)
        );
        let mut p = packet(100, 0, 1);
        p[0] = 0x00;
        assert_eq!(
            parse_ts_packet_header(&p),
            Err(DescrambleFailure::BadSyncByte)
        );
    }

    #[test]
    fn decrypt_hot_path_uses_prepared_key_type() {
        let mut payload = *b"0123456789abcdef";
        let key: PreparedMulti2Key = sample_key(9).prepare().unwrap();
        multi2_decrypt_payload(&mut payload, &key).unwrap();
    }

    #[test]
    fn encrypt_helper_uses_prepared_key_type() {
        let mut payload = *b"0123456789abcdef";
        let key: PreparedMulti2Key = sample_key(9).prepare().unwrap();
        multi2_encrypt_payload(&mut payload, &key).unwrap();
    }

    #[test]
    fn prepared_key_produces_same_ciphertext_as_raw_material() {
        let mut payload = *b"0123456789abcdef";
        let key = sample_key(9).prepare().unwrap();
        multi2_encrypt_payload(&mut payload, &key).unwrap();
        assert_eq!(
            payload,
            [
                0x81, 0x23, 0xc7, 0xf8, 0xb7, 0x1a, 0xfb, 0x06, 0xa9, 0x0e, 0x78, 0x3a, 0xd9, 0x87,
                0x5b, 0x5d
            ]
        );
    }

    #[test]
    fn prepared_key_decrypts_existing_known_answer_vector() {
        let mut payload = *b"0123456789abcdef";
        let key = sample_key(9).prepare().unwrap();
        multi2_encrypt_payload(&mut payload, &key).unwrap();
        assert_eq!(
            payload,
            [
                0x81, 0x23, 0xc7, 0xf8, 0xb7, 0x1a, 0xfb, 0x06, 0xa9, 0x0e, 0x78, 0x3a, 0xd9, 0x87,
                0x5b, 0x5d
            ]
        );
        multi2_decrypt_payload(&mut payload, &key).unwrap();
        assert_eq!(&payload, b"0123456789abcdef");
    }

    #[test]
    fn multi2_prepare_rejects_zero_rounds_as_invalid_rounds_zero() {
        let mut key = sample_key(7);
        key.rounds = 0;
        assert_eq!(key.prepare(), Err(Multi2PrepareError::InvalidRoundsZero));
    }

    #[test]
    fn decrypt_hot_path_accepts_only_prepared_key() {
        let mut payload = *b"0123456789abcdef";
        let key: PreparedMulti2Key = sample_key(9).prepare().unwrap();
        multi2_decrypt_payload(&mut payload, &key).unwrap();
    }

    #[test]
    fn encrypt_hot_path_accepts_only_prepared_key() {
        let mut payload = *b"0123456789abcdef";
        let key: PreparedMulti2Key = sample_key(9).prepare().unwrap();
        multi2_encrypt_payload(&mut payload, &key).unwrap();
    }

    #[test]
    fn test_registration_rejects_unprepared_invalid_key() {
        let mut key = sample_key(12);
        key.rounds = 0;
        assert_eq!(
            DescramblerKeySlot::empty().try_with_even(key),
            Err(Multi2PrepareError::InvalidRoundsZero)
        );
    }

    #[test]
    fn prepared_key_rejects_zero_rounds_at_registration() {
        let mut key = sample_key(7);
        key.rounds = 0;
        assert_eq!(key.prepare(), Err(Multi2PrepareError::InvalidRoundsZero));
        assert_eq!(
            DescramblerKeySlot::empty().try_with_even(key),
            Err(Multi2PrepareError::InvalidRoundsZero)
        );
    }

    #[test]
    fn test_key_registration_stores_prepared_key() {
        let key = sample_key(8);
        let slot = DescramblerKeySlot::empty().try_with_even(key).unwrap();
        assert!(slot.key_for(KeyParity::Even).is_some());
        assert!(slot.key_for(KeyParity::Odd).is_none());
    }

    #[test]
    fn key_preparation_failure_does_not_panic() {
        let mut key = sample_key(10);
        key.rounds = 0;
        let result = std::panic::catch_unwind(|| key.prepare());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Err(Multi2PrepareError::InvalidRoundsZero));
    }

    #[test]
    fn infallible_raw_key_slot_api_is_removed() {
        let slot = DescramblerKeySlot::empty()
            .try_with_even(sample_key(11))
            .unwrap();
        assert!(slot.has_any_key());
    }

    #[test]
    fn end_to_end_payload_vector() {
        let even = sample_key(3);
        let clear = packet(0x123, 0, 1);
        let even_prepared = even.prepare().unwrap();
        let mut scrambled = encrypt_payload_packet(clear, &even_prepared);
        scrambled[3] = (scrambled[3] & 0x3f) | (2 << 6);
        let keys = DescramblerKeySlot::empty().try_with_even(even).unwrap();
        descramble_ts_packet_in_place(&mut scrambled, &BTreeSet::from([0x123]), &keys).unwrap();
        assert_eq!(scrambled, clear);
    }
    #[test]
    fn descramble_existing_even_odd_parity_tests() {
        even_key_is_selected_and_tsc_is_cleared();
        odd_key_is_selected_and_tsc_is_cleared();
    }
}
