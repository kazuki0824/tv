use maleicacid_tuner_hal_common::TS_PACKET_SIZE;
use std::collections::BTreeSet;

pub const NULL_PID: u16 = 0x1fff;
pub const DEFAULT_MULTI2_ROUNDS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyParity {
    Even,
    Odd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Multi2KeyMaterial {
    pub system_key: [u8; 32],
    pub cbc_iv: [u8; 8],
    pub data_key: [u8; 8],
    pub rounds: usize,
}

impl Multi2KeyMaterial {
    pub const fn new(system_key: [u8; 32], cbc_iv: [u8; 8], data_key: [u8; 8]) -> Self {
        Self { system_key, cbc_iv, data_key, rounds: DEFAULT_MULTI2_ROUNDS }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DescramblerKeySlot {
    even: Option<Multi2KeyMaterial>,
    odd: Option<Multi2KeyMaterial>,
}

impl DescramblerKeySlot {
    pub const fn empty() -> Self { Self { even: None, odd: None } }

    pub fn with_even(mut self, key: Multi2KeyMaterial) -> Self {
        self.even = Some(key);
        self
    }

    pub fn with_odd(mut self, key: Multi2KeyMaterial) -> Self {
        self.odd = Some(key);
        self
    }

    pub fn key_for(&self, parity: KeyParity) -> Option<&Multi2KeyMaterial> {
        match parity {
            KeyParity::Even => self.even.as_ref(),
            KeyParity::Odd => self.odd.as_ref(),
        }
    }

    pub fn has_any_key(&self) -> bool {
        self.even.is_some() || self.odd.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PassThroughReason {
    ClearPacket,
    NullPid,
    PidNotTargeted,
    NoPayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescrambleFailure {
    InvalidPacketSize,
    BadSyncByte,
    InvalidScramblingControl,
    NoPayload,
    NoKey,
    BadToken,
    Multi2Fail,
    ScrambledPidNotRegistered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescrambleOutcome {
    Descrambled { pid: u16, parity: KeyParity, payload_offset: usize },
    PassedThrough { pid: u16, reason: PassThroughReason },
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
    let adaptation_field_control = (packet[3] >> 4) & 0x03;
    if adaptation_field_control == 0 {
        return Err(DescrambleFailure::NoPayload);
    }
    let mut offset = 4usize;
    if adaptation_field_control == 2 || adaptation_field_control == 3 {
        let adaptation_len = packet[offset] as usize;
        offset = offset.checked_add(1 + adaptation_len).ok_or(DescrambleFailure::NoPayload)?;
        if offset > packet.len() {
            return Err(DescrambleFailure::NoPayload);
        }
        if adaptation_field_control == 2 {
            return Ok(TsPacketHeader {
                pid: (((packet[1] & 0x1f) as u16) << 8) | packet[2] as u16,
                transport_error_indicator: (packet[1] & 0x80) != 0,
                payload_unit_start: (packet[1] & 0x40) != 0,
                transport_scrambling_control: (packet[3] >> 6) & 0x03,
                adaptation_field_control,
                continuity_counter: packet[3] & 0x0f,
                payload_offset: None,
            });
        }
    }
    Ok(TsPacketHeader {
        pid: (((packet[1] & 0x1f) as u16) << 8) | packet[2] as u16,
        transport_error_indicator: (packet[1] & 0x80) != 0,
        payload_unit_start: (packet[1] & 0x40) != 0,
        transport_scrambling_control: (packet[3] >> 6) & 0x03,
        adaptation_field_control,
        continuity_counter: packet[3] & 0x0f,
        payload_offset: Some(offset),
    })
}

pub fn descramble_ts_packet_in_place(
    packet: &mut [u8],
    target_pids: &BTreeSet<u16>,
    key_slot: &DescramblerKeySlot,
) -> Result<DescrambleOutcome, DescrambleFailure> {
    let header = parse_ts_packet_header(packet)?;
    if header.pid == NULL_PID {
        return Ok(DescrambleOutcome::PassedThrough { pid: header.pid, reason: PassThroughReason::NullPid });
    }
    let parity = match header.transport_scrambling_control {
        0 => return Ok(DescrambleOutcome::PassedThrough { pid: header.pid, reason: PassThroughReason::ClearPacket }),
        1 => return Err(DescrambleFailure::InvalidScramblingControl),
        2 => KeyParity::Even,
        3 => KeyParity::Odd,
        _ => return Err(DescrambleFailure::InvalidScramblingControl),
    };
    if !target_pids.contains(&header.pid) {
        return Err(DescrambleFailure::ScrambledPidNotRegistered);
    }
    let Some(payload_offset) = header.payload_offset else {
        return Ok(DescrambleOutcome::PassedThrough { pid: header.pid, reason: PassThroughReason::NoPayload });
    };
    if payload_offset >= TS_PACKET_SIZE {
        return Ok(DescrambleOutcome::PassedThrough { pid: header.pid, reason: PassThroughReason::NoPayload });
    }
    let key = key_slot.key_for(parity).ok_or(DescrambleFailure::NoKey)?;
    multi2_decrypt_payload(&mut packet[payload_offset..], key).map_err(|_| DescrambleFailure::Multi2Fail)?;
    packet[3] &= 0x3f;
    Ok(DescrambleOutcome::Descrambled { pid: header.pid, parity, payload_offset })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Multi2Error {
    InvalidRounds,
}

pub fn multi2_decrypt_payload(payload: &mut [u8], key: &Multi2KeyMaterial) -> Result<(), Multi2Error> {
    if key.rounds == 0 {
        return Err(Multi2Error::InvalidRounds);
    }
    let system_key = parse_system_key(&key.system_key);
    let data_key = [load_be(&key.data_key[0..4]), load_be(&key.data_key[4..8])];
    let work_key = schedule(data_key, system_key);
    let iv = [load_be(&key.cbc_iv[0..4]), load_be(&key.cbc_iv[4..8])];
    decrypt_cbc_ofb(payload, iv, work_key, key.rounds);
    Ok(())
}

pub fn multi2_encrypt_payload(payload: &mut [u8], key: &Multi2KeyMaterial) -> Result<(), Multi2Error> {
    if key.rounds == 0 {
        return Err(Multi2Error::InvalidRounds);
    }
    let system_key = parse_system_key(&key.system_key);
    let data_key = [load_be(&key.data_key[0..4]), load_be(&key.data_key[4..8])];
    let work_key = schedule(data_key, system_key);
    let iv = [load_be(&key.cbc_iv[0..4]), load_be(&key.cbc_iv[4..8])];
    encrypt_cbc_ofb(payload, iv, work_key, key.rounds);
    Ok(())
}

fn parse_system_key(bytes: &[u8; 32]) -> [u32; 8] {
    let mut out = [0u32; 8];
    for i in 0..8 {
        out[i] = load_be(&bytes[i * 4..i * 4 + 4]);
    }
    out
}

fn load_be(p: &[u8]) -> u32 {
    ((p[0] as u32) << 24) | ((p[1] as u32) << 16) | ((p[2] as u32) << 8) | p[3] as u32
}

fn store_be(p: &mut [u8], v: u32) {
    p[0] = ((v >> 24) & 0xff) as u8;
    p[1] = ((v >> 16) & 0xff) as u8;
    p[2] = ((v >> 8) & 0xff) as u8;
    p[3] = (v & 0xff) as u8;
}

fn rot<const N: u32>(v: u32) -> u32 { v.rotate_left(N) }
fn rot1_sub(v: u32) -> u32 { v.wrapping_add(v >> 31) }
fn rot1_add_dec(v: u32) -> u32 { rot::<1>(v).wrapping_add(v).wrapping_sub(1) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Block {
    left: u32,
    right: u32,
}

impl Block {
    fn load(p: &[u8]) -> Self { Self { left: load_be(&p[0..4]), right: load_be(&p[4..8]) } }
    fn store(self, p: &mut [u8]) {
        store_be(&mut p[0..4], self.left);
        store_be(&mut p[4..8], self.right);
    }
    fn xor(self, other: Block) -> Self { Self { left: self.left ^ other.left, right: self.right ^ other.right } }
    fn cbc_post_decrypt(self, ciphertext: Block, state: Block) -> (Block, Block) { (self.xor(state), ciphertext) }
}

fn pi1(p: Block) -> Block { Block { left: p.left, right: p.right ^ p.left } }
fn pi2(p: Block, k1: u32) -> Block {
    let x = p.right;
    let y = x.wrapping_add(k1);
    let z = rot1_add_dec(y);
    Block { left: p.left ^ rot::<4>(z) ^ z, right: p.right }
}
fn pi3(p: Block, k2: u32, k3: u32) -> Block {
    let x = p.left;
    let y = x.wrapping_add(k2);
    let z = rot::<2>(y).wrapping_add(y).wrapping_add(1);
    let a = rot::<8>(z) ^ z;
    let b = a.wrapping_add(k3);
    let c = rot1_sub(b);
    Block { left: p.left, right: p.right ^ rot::<16>(c) ^ (c | x) }
}
fn pi4(p: Block, k4: u32) -> Block {
    let x = p.right;
    let y = x.wrapping_add(k4);
    Block { left: p.left ^ rot::<2>(y).wrapping_add(y).wrapping_add(1), right: p.right }
}

fn cipher_encrypt(mut b: Block, wk: [u32; 8], rounds: usize) -> Block {
    for _ in 0..rounds {
        b = pi1(b);
        b = pi2(b, wk[0]);
        b = pi3(b, wk[1], wk[2]);
        b = pi4(b, wk[3]);
        b = pi1(b);
        b = pi2(b, wk[4]);
        b = pi3(b, wk[5], wk[6]);
        b = pi4(b, wk[7]);
    }
    b
}

fn cipher_decrypt(mut b: Block, wk: [u32; 8], rounds: usize) -> Block {
    for _ in 0..rounds {
        b = pi4(b, wk[7]);
        b = pi3(b, wk[5], wk[6]);
        b = pi2(b, wk[4]);
        b = pi1(b);
        b = pi4(b, wk[3]);
        b = pi3(b, wk[1], wk[2]);
        b = pi2(b, wk[0]);
        b = pi1(b);
    }
    b
}

fn schedule(dk: [u32; 2], sk: [u32; 8]) -> [u32; 8] {
    let a0 = pi1(Block { left: dk[0], right: dk[1] });
    let a1 = pi2(a0, sk[0]);
    let a2 = pi3(a1, sk[1], sk[2]);
    let a3 = pi4(a2, sk[3]);
    let a4 = pi1(a3);
    let a5 = pi2(a4, sk[4]);
    let a6 = pi3(a5, sk[5], sk[6]);
    let a7 = pi4(a6, sk[7]);
    let a8 = pi1(a7);
    [a1.left, a2.right, a3.left, a4.right, a5.left, a6.right, a7.left, a8.right]
}

fn encrypt_cbc_ofb(buf: &mut [u8], iv: [u32; 2], key: [u32; 8], rounds: usize) {
    let mut state = Block { left: iv[0], right: iv[1] };
    let mut chunks = buf.chunks_exact_mut(8);
    for chunk in &mut chunks {
        let p = Block::load(chunk);
        let c = cipher_encrypt(p.xor(state), key, rounds);
        c.store(chunk);
        state = c;
    }
    let rem = chunks.into_remainder();
    if !rem.is_empty() {
        let mut t = [0u8; 8];
        t[..rem.len()].copy_from_slice(rem);
        let p = Block::load(&t);
        let c = p.xor(cipher_encrypt(state, key, rounds));
        c.store(&mut t);
        rem.copy_from_slice(&t[..rem.len()]);
    }
}

fn decrypt_cbc_ofb(buf: &mut [u8], iv: [u32; 2], key: [u32; 8], rounds: usize) {
    let mut state = Block { left: iv[0], right: iv[1] };
    let mut chunks = buf.chunks_exact_mut(8);
    for chunk in &mut chunks {
        let c = Block::load(chunk);
        let d = cipher_decrypt(c, key, rounds);
        let (p, next_state) = d.cbc_post_decrypt(c, state);
        p.store(chunk);
        state = next_state;
    }
    let rem = chunks.into_remainder();
    if !rem.is_empty() {
        let mut t = [0u8; 8];
        t[..rem.len()].copy_from_slice(rem);
        let c = Block::load(&t);
        let p = c.xor(cipher_encrypt(state, key, rounds));
        p.store(&mut t);
        rem.copy_from_slice(&t[..rem.len()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key(byte: u8) -> Multi2KeyMaterial {
        let mut system_key = [0u8; 32];
        for (i, b) in system_key.iter_mut().enumerate() { *b = byte.wrapping_add(i as u8); }
        let mut iv = [0u8; 8];
        for (i, b) in iv.iter_mut().enumerate() { *b = 0xa0u8.wrapping_add(byte).wrapping_add(i as u8); }
        let mut data_key = [0u8; 8];
        for (i, b) in data_key.iter_mut().enumerate() { *b = 0x40u8.wrapping_add(byte).wrapping_add((i * 3) as u8); }
        Multi2KeyMaterial::new(system_key, iv, data_key)
    }

    fn packet(pid: u16, tsc: u8, afc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0u8; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = ((pid >> 8) as u8) & 0x1f;
        p[2] = (pid & 0xff) as u8;
        p[3] = (tsc << 6) | (afc << 4) | 0x05;
        for i in 4..TS_PACKET_SIZE { p[i] = (i as u8).wrapping_mul(3).wrapping_add(1); }
        p
    }

    fn encrypt_payload_packet(mut p: [u8; TS_PACKET_SIZE], key: &Multi2KeyMaterial) -> [u8; TS_PACKET_SIZE] {
        let off = parse_ts_packet_header(&p).unwrap().payload_offset.unwrap();
        multi2_encrypt_payload(&mut p[off..], key).unwrap();
        p
    }

    #[test]
    fn clear_packet_passes_byte_identical() {
        let mut p = packet(100, 0, 1);
        let original = p;
        let outcome = descramble_ts_packet_in_place(&mut p, &BTreeSet::from([100]), &DescramblerKeySlot::empty()).unwrap();
        assert_eq!(outcome, DescrambleOutcome::PassedThrough { pid: 100, reason: PassThroughReason::ClearPacket });
        assert_eq!(p, original);
    }

    #[test]
    fn null_pid_passes_byte_identical() {
        let mut p = packet(NULL_PID, 2, 1);
        let original = p;
        let outcome = descramble_ts_packet_in_place(&mut p, &BTreeSet::from([NULL_PID]), &DescramblerKeySlot::empty()).unwrap();
        assert_eq!(outcome, DescrambleOutcome::PassedThrough { pid: NULL_PID, reason: PassThroughReason::NullPid });
        assert_eq!(p, original);
    }

    #[test]
    fn adaptation_only_packet_passes_byte_identical() {
        let mut p = packet(100, 2, 2);
        p[4] = 183;
        let original = p;
        let outcome = descramble_ts_packet_in_place(&mut p, &BTreeSet::from([100]), &DescramblerKeySlot::empty()).unwrap();
        assert_eq!(outcome, DescrambleOutcome::PassedThrough { pid: 100, reason: PassThroughReason::NoPayload });
        assert_eq!(p, original);
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
        let mut scrambled = encrypt_payload_packet(clear, &even);
        scrambled[3] = (scrambled[3] & 0x3f) | (2 << 6);
        let keys = DescramblerKeySlot::empty().with_even(even).with_odd(odd);
        let outcome = descramble_ts_packet_in_place(&mut scrambled, &BTreeSet::from([100]), &keys).unwrap();
        assert_eq!(outcome, DescrambleOutcome::Descrambled { pid: 100, parity: KeyParity::Even, payload_offset: 4 });
        assert_eq!(scrambled[3] >> 6, 0);
        assert_eq!(scrambled, clear);
    }

    #[test]
    fn odd_key_is_selected_and_tsc_is_cleared() {
        let even = sample_key(1);
        let odd = sample_key(2);
        let clear = packet(101, 0, 1);
        let mut scrambled = encrypt_payload_packet(clear, &odd);
        scrambled[3] = (scrambled[3] & 0x3f) | (3 << 6);
        let keys = DescramblerKeySlot::empty().with_even(even).with_odd(odd);
        descramble_ts_packet_in_place(&mut scrambled, &BTreeSet::from([101]), &keys).unwrap();
        assert_eq!(scrambled[3] >> 6, 0);
        assert_eq!(scrambled, clear);
    }

    #[test]
    fn invalid_tsc_is_rejected() {
        let mut p = packet(100, 1, 1);
        let original = p;
        assert_eq!(descramble_ts_packet_in_place(&mut p, &BTreeSet::from([100]), &DescramblerKeySlot::empty()), Err(DescrambleFailure::InvalidScramblingControl));
        assert_eq!(p, original);
    }

    #[test]
    fn failed_no_key_does_not_clear_tsc() {
        let mut p = packet(100, 2, 1);
        let original_tsc = p[3] >> 6;
        assert_eq!(descramble_ts_packet_in_place(&mut p, &BTreeSet::from([100]), &DescramblerKeySlot::empty()), Err(DescrambleFailure::NoKey));
        assert_eq!(p[3] >> 6, original_tsc);
    }

    #[test]
    fn clear_pid_not_targeted_passes_byte_identical() {
        let mut p = packet(100, 0, 1);
        let original = p;
        let outcome = descramble_ts_packet_in_place(&mut p, &BTreeSet::from([200]), &DescramblerKeySlot::empty()).unwrap();
        assert_eq!(outcome, DescrambleOutcome::PassedThrough { pid: 100, reason: PassThroughReason::ClearPacket });
        assert_eq!(p, original);
    }

    #[test]
    fn scrambled_unregistered_pid_is_not_silently_decoded() {
        let mut p = packet(100, 2, 1);
        let original = p;
        assert_eq!(descramble_ts_packet_in_place(&mut p, &BTreeSet::from([200]), &DescramblerKeySlot::empty()), Err(DescrambleFailure::ScrambledPidNotRegistered));
        assert_eq!(p, original);
    }

    #[test]
    fn invalid_size_and_sync_are_rejected() {
        assert_eq!(parse_ts_packet_header(&[0u8; 187]), Err(DescrambleFailure::InvalidPacketSize));
        let mut p = packet(100, 0, 1);
        p[0] = 0x00;
        assert_eq!(parse_ts_packet_header(&p), Err(DescrambleFailure::BadSyncByte));
    }

    #[test]
    fn multi2_known_answer_vector_for_core_encrypt() {
        let mut payload = *b"0123456789abcdef";
        let key = sample_key(9);
        multi2_encrypt_payload(&mut payload, &key).unwrap();
        assert_eq!(payload, [0x81, 0x23, 0xc7, 0xf8, 0xb7, 0x1a, 0xfb, 0x06, 0xa9, 0x0e, 0x78, 0x3a, 0xd9, 0x87, 0x5b, 0x5d]);
        multi2_decrypt_payload(&mut payload, &key).unwrap();
        assert_eq!(&payload, b"0123456789abcdef");
    }

    #[test]
    fn end_to_end_payload_vector() {
        let even = sample_key(3);
        let clear = packet(0x123, 0, 1);
        let mut scrambled = encrypt_payload_packet(clear, &even);
        scrambled[3] = (scrambled[3] & 0x3f) | (2 << 6);
        let keys = DescramblerKeySlot::empty().with_even(even);
        descramble_ts_packet_in_place(&mut scrambled, &BTreeSet::from([0x123]), &keys).unwrap();
        assert_eq!(scrambled, clear);
    }
}
