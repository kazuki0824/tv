pub const DEFAULT_MULTI2_ROUNDS: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Multi2KeyMaterial {
    pub system_key: [u8; 32],
    pub cbc_iv: [u8; 8],
    pub data_key: [u8; 8],
    pub rounds: usize,
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
        let system_key = parse_system_key(&self.system_key);
        let data_key = [load_be(&self.data_key[0..4]), load_be(&self.data_key[4..8])];
        let work_key = schedule(data_key, system_key);
        let cbc_iv = [load_be(&self.cbc_iv[0..4]), load_be(&self.cbc_iv[4..8])];
        Ok(PreparedMulti2Key {
            cbc_iv,
            work_key,
            rounds: self.rounds,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMulti2Key {
    pub(crate) cbc_iv: [u32; 2],
    pub(crate) work_key: [u32; 8],
    pub(crate) rounds: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Multi2PrepareError {
    InvalidRoundsZero,
}

pub fn multi2_decrypt_payload(payload: &mut [u8], key: &PreparedMulti2Key) {
    decrypt_cbc_ofb(payload, key.cbc_iv, key.work_key, key.rounds);
}

pub fn multi2_encrypt_payload(payload: &mut [u8], key: &PreparedMulti2Key) {
    encrypt_cbc_ofb(payload, key.cbc_iv, key.work_key, key.rounds);
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

fn encrypt_cbc_ofb(buf: &mut [u8], iv: [u32; 2], key: [u32; 8], rounds: usize) {
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
    }
}

fn decrypt_cbc_ofb(buf: &mut [u8], iv: [u32; 2], key: [u32; 8], rounds: usize) {
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
    }
}
