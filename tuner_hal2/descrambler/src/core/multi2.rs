use std::ptr;
use std::sync::atomic::{compiler_fence, Ordering};

pub const DEFAULT_MULTI2_ROUNDS: usize = 4;

fn volatile_zeroize_u8(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

fn volatile_zeroize_u32(words: &mut [u32]) {
    for word in words {
        unsafe { ptr::write_volatile(word, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[derive(Clone, Eq, PartialEq)]
pub struct Multi2KeyMaterial {
    pub system_key: [u8; 32],
    pub cbc_iv: [u8; 8],
    pub data_key: [u8; 8],
    pub rounds: usize,
}

impl std::fmt::Debug for Multi2KeyMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Multi2KeyMaterial")
            .field("rounds", &self.rounds)
            .field("key_material", &"<redacted>")
            .finish()
    }
}

impl Drop for Multi2KeyMaterial {
    fn drop(&mut self) {
        volatile_zeroize_u8(&mut self.system_key);
        volatile_zeroize_u8(&mut self.cbc_iv);
        volatile_zeroize_u8(&mut self.data_key);
    }
}

impl Multi2KeyMaterial {
    pub const fn new(system_key: [u8; 32], cbc_iv: [u8; 8], data_key: [u8; 8]) -> Self {
        Self {
            system_key,
            cbc_iv,
            data_key,
            rounds: DEFAULT_MULTI2_ROUNDS,
        }
    }

    pub fn prepare(&self) -> Result<PreparedMulti2Key, Multi2PrepareError> {
        if self.rounds == 0 {
            return Err(Multi2PrepareError::InvalidRoundsZero);
        }
        let mut system_key = parse_system_key(&self.system_key);
        let mut data_key = [load_be(&self.data_key[0..4]), load_be(&self.data_key[4..8])];
        let work_key = schedule(&data_key, &system_key);
        volatile_zeroize_u32(&mut data_key);
        volatile_zeroize_u32(&mut system_key);
        let cbc_iv = [load_be(&self.cbc_iv[0..4]), load_be(&self.cbc_iv[4..8])];
        Ok(PreparedMulti2Key {
            cbc_iv,
            work_key,
            rounds: self.rounds,
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct PreparedMulti2Key {
    pub(crate) cbc_iv: [u32; 2],
    pub(crate) work_key: [u32; 8],
    pub(crate) rounds: usize,
}

impl std::fmt::Debug for PreparedMulti2Key {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedMulti2Key")
            .field("rounds", &self.rounds)
            .field("key_material", &"<redacted>")
            .finish()
    }
}

impl Drop for PreparedMulti2Key {
    fn drop(&mut self) {
        volatile_zeroize_u32(&mut self.cbc_iv);
        volatile_zeroize_u32(&mut self.work_key);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Multi2PrepareError {
    InvalidRoundsZero,
}

pub fn multi2_decrypt_payload(payload: &mut [u8], key: &PreparedMulti2Key) {
    decrypt_cbc_ofb(payload, key.cbc_iv, &key.work_key, key.rounds);
}

pub fn multi2_encrypt_payload(payload: &mut [u8], key: &PreparedMulti2Key) {
    encrypt_cbc_ofb(payload, key.cbc_iv, &key.work_key, key.rounds);
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

fn rot<const N: u32>(v: u32) -> u32 {
    v.rotate_left(N)
}
fn rot1_sub(v: u32) -> u32 {
    v.wrapping_add(v >> 31)
}
fn rot1_add_dec(v: u32) -> u32 {
    rot::<1>(v).wrapping_add(v).wrapping_sub(1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Block {
    left: u32,
    right: u32,
}

impl Block {
    fn load(p: &[u8]) -> Self {
        Self {
            left: load_be(&p[0..4]),
            right: load_be(&p[4..8]),
        }
    }
    fn store(self, p: &mut [u8]) {
        store_be(&mut p[0..4], self.left);
        store_be(&mut p[4..8], self.right);
    }
    fn xor(self, other: Block) -> Self {
        Self {
            left: self.left ^ other.left,
            right: self.right ^ other.right,
        }
    }
    fn cbc_post_decrypt(self, ciphertext: Block, state: Block) -> (Block, Block) {
        (self.xor(state), ciphertext)
    }
}

fn pi1(p: Block) -> Block {
    Block {
        left: p.left,
        right: p.right ^ p.left,
    }
}
fn pi2(p: Block, k1: u32) -> Block {
    let x = p.right;
    let y = x.wrapping_add(k1);
    let z = rot1_add_dec(y);
    Block {
        left: p.left ^ rot::<4>(z) ^ z,
        right: p.right,
    }
}
fn pi3(p: Block, k2: u32, k3: u32) -> Block {
    let x = p.left;
    let y = x.wrapping_add(k2);
    let z = rot::<2>(y).wrapping_add(y).wrapping_add(1);
    let a = rot::<8>(z) ^ z;
    let b = a.wrapping_add(k3);
    let c = rot1_sub(b);
    Block {
        left: p.left,
        right: p.right ^ rot::<16>(c) ^ (c | x),
    }
}
fn pi4(p: Block, k4: u32) -> Block {
    let x = p.right;
    let y = x.wrapping_add(k4);
    Block {
        left: p.left ^ rot::<2>(y).wrapping_add(y).wrapping_add(1),
        right: p.right,
    }
}

fn cipher_encrypt(mut b: Block, wk: &[u32; 8], rounds: usize) -> Block {
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

fn cipher_decrypt(mut b: Block, wk: &[u32; 8], rounds: usize) -> Block {
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

fn schedule(dk: &[u32; 2], sk: &[u32; 8]) -> [u32; 8] {
    let a0 = pi1(Block {
        left: dk[0],
        right: dk[1],
    });
    let a1 = pi2(a0, sk[0]);
    let a2 = pi3(a1, sk[1], sk[2]);
    let a3 = pi4(a2, sk[3]);
    let a4 = pi1(a3);
    let a5 = pi2(a4, sk[4]);
    let a6 = pi3(a5, sk[5], sk[6]);
    let a7 = pi4(a6, sk[7]);
    let a8 = pi1(a7);
    [
        a1.left, a2.right, a3.left, a4.right, a5.left, a6.right, a7.left, a8.right,
    ]
}

fn encrypt_cbc_ofb(buf: &mut [u8], iv: [u32; 2], key: &[u32; 8], rounds: usize) {
    let mut state = Block {
        left: iv[0],
        right: iv[1],
    };
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
        volatile_zeroize_u8(&mut t);
    }
}

fn decrypt_cbc_ofb(buf: &mut [u8], iv: [u32; 2], key: &[u32; 8], rounds: usize) {
    let mut state = Block {
        left: iv[0],
        right: iv[1],
    };
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
        volatile_zeroize_u8(&mut t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM_KEY: [u8; 32] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
        0xee, 0xff, 0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xbb,
        0xdc, 0xdd, 0xde, 0xef,
    ];
    const CBC_IV: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    const EVEN_DATA_KEY: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    const ODD_DATA_KEY: [u8; 8] = [0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10];
    const CIPHERTEXT_17: [u8; 17] = [
        0xcc, 0x34, 0x88, 0xc9, 0xb9, 0x54, 0x5c, 0x65, 0x29, 0xa7, 0xbc, 0x5f, 0xc5, 0x37,
        0xea, 0xb2, 0x8e,
    ];
    const CIPHERTEXT_184: [u8; 184] = [
        0xd2, 0xb5, 0x77, 0xae, 0x04, 0x52, 0x66, 0xed, 0x5e, 0x2c, 0x2b, 0x52, 0x87, 0xde,
        0xce, 0xbf, 0xe8, 0x4c, 0x69, 0x7d, 0x15, 0xed, 0x4a, 0xd7, 0xd6, 0x23, 0x12, 0xe9,
        0x96, 0xcf, 0x40, 0xcc, 0x3d, 0x9a, 0x58, 0xe1, 0xd8, 0x32, 0x14, 0x30, 0x5d, 0xe0,
        0x09, 0xc3, 0x12, 0x54, 0x24, 0x17, 0xaa, 0x6d, 0x5b, 0x6c, 0x0d, 0xa4, 0x7e, 0x57,
        0x7c, 0x79, 0x4f, 0x33, 0xce, 0xb7, 0xb4, 0x55, 0xd3, 0xd8, 0xaf, 0x8a, 0xa5, 0x29,
        0x02, 0xb8, 0xff, 0x33, 0x50, 0xeb, 0x5c, 0xe7, 0xfa, 0x2b, 0x89, 0x92, 0xf3, 0xd6,
        0x01, 0x21, 0xd1, 0xaa, 0x40, 0xee, 0x5e, 0x52, 0xf9, 0xeb, 0xfc, 0x8b, 0x6f, 0xbd,
        0x34, 0x53, 0xf4, 0x5d, 0x35, 0xc0, 0xfe, 0xab, 0x15, 0x3f, 0xdf, 0xa8, 0x26, 0x86,
        0x6e, 0x0d, 0x59, 0x1e, 0xb3, 0x3e, 0x09, 0x11, 0x0d, 0x40, 0x95, 0x5d, 0x09, 0x64,
        0xf7, 0x82, 0x22, 0x46, 0xcb, 0xc2, 0x61, 0xf6, 0x91, 0xd9, 0x87, 0x03, 0xaa, 0x8a,
        0x49, 0x23, 0xee, 0x7f, 0x1f, 0x7e, 0x1e, 0xf3, 0x84, 0x6e, 0x80, 0xca, 0x26, 0x79,
        0x7b, 0x59, 0xf7, 0x1c, 0xfd, 0x87, 0x5e, 0xba, 0xa9, 0x67, 0x64, 0x27, 0x26, 0x74,
        0x89, 0x5a, 0xac, 0xef, 0xca, 0xd4, 0x94, 0xc2, 0x27, 0xa1, 0x14, 0x79, 0x86, 0x3b,
        0xd5, 0xdb,
    ];

    fn plaintext(len: usize) -> Vec<u8> {
        (0..len)
            .map(|index| ((index * 73 + 41) & 0xff) as u8)
            .collect()
    }

    fn assert_kat(data_key: [u8; 8], ciphertext: &[u8]) {
        let prepared = Multi2KeyMaterial::new(SYSTEM_KEY, CBC_IV, data_key)
            .prepare()
            .expect("known vector has a valid MULTI2 key");
        let expected_plaintext = plaintext(ciphertext.len());

        let mut encrypted = expected_plaintext.clone();
        multi2_encrypt_payload(&mut encrypted, &prepared);
        assert_eq!(encrypted, ciphertext);

        let mut decrypted = ciphertext.to_vec();
        multi2_decrypt_payload(&mut decrypted, &prepared);
        assert_eq!(decrypted, expected_plaintext);
    }

    #[test]
    fn matches_libarib_bxx_multi2_known_answer_vectors() {
        // Provenance: kazuki0824/libarib-bxx@af77dac51f197a039b046b40471598358b227f15
        // tests/multi2_kat.cc. Type 0x03 uses the first 8-byte scramble key;
        // type 0x02 uses the second 8-byte scramble key in that implementation.
        assert_kat(EVEN_DATA_KEY, &CIPHERTEXT_17);
        assert_kat(ODD_DATA_KEY, &CIPHERTEXT_184);
    }
}
