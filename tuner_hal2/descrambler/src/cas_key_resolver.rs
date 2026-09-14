#[cfg(any(target_os = "android", test))]
use crate::product_parameters::ProductMulti2Parameters;
#[cfg(any(target_os = "android", test))]
use crate::Multi2KeyMaterial;
use crate::{DescramblerKeySlot, DescramblerKeyToken};
#[cfg(target_os = "android")]
use std::ptr;
#[cfg(target_os = "android")]
use std::sync::atomic::{compiler_fence, Ordering};

#[cfg(target_os = "android")]
const CAS_KEY_OK: i32 = 0;
#[cfg(target_os = "android")]
const CAS_KEY_INVALID_TOKEN: i32 = 1;
#[cfg(target_os = "android")]
const CAS_KEY_UNKNOWN_TOKEN: i32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasKeyResolveError {
    InvalidToken,
    UnknownToken,
    Unavailable,
    InvalidKeyMaterial,
}

pub trait CasKeyResolver: Send + Sync {
    fn resolve(
        &self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlot, CasKeyResolveError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProductCasKeyResolver;

#[cfg(target_os = "android")]
fn erase(bytes: &mut [u8]) {
    for byte in bytes {
        // `byte` はこの可変slice内の有効な1 byteを指す。volatile write後も
        // sliceの長さ・所有権は変わらず、呼出し元からはゼロ化済みとして見える。
        unsafe { ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[cfg(any(target_os = "android", test))]
fn build_key_slot(
    parameters: &ProductMulti2Parameters,
    odd: [u8; 8],
    even: [u8; 8],
) -> Result<DescramblerKeySlot, CasKeyResolveError> {
    DescramblerKeySlot::empty()
        .try_with_even(Multi2KeyMaterial::new(
            parameters.system_key,
            parameters.init_cbc,
            even,
        ))
        .and_then(|slot| {
            slot.try_with_odd(Multi2KeyMaterial::new(
                parameters.system_key,
                parameters.init_cbc,
                odd,
            ))
        })
        .map_err(|_| CasKeyResolveError::InvalidKeyMaterial)
}

#[cfg(target_os = "android")]
extern "C" {
    fn maleicacid_cas_acquire_packet_keys(
        token: *const u8,
        length: usize,
        odd: *mut u8,
        even: *mut u8,
    ) -> i32;
}

#[cfg(target_os = "android")]
impl CasKeyResolver for ProductCasKeyResolver {
    fn resolve(
        &self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlot, CasKeyResolveError> {
        let parameters = crate::product_parameters::product_parameters()
            .map_err(|_| CasKeyResolveError::Unavailable)?;
        let mut odd = [0_u8; 8];
        let mut even = [0_u8; 8];
        // tokenは呼出し中有効な連続領域で、odd/evenは各8 byteの書込み可能領域である。
        // C++ wrapperはpointerを保持せず、戻り時には出力を鍵またはゼロで初期化する。
        let status = unsafe {
            maleicacid_cas_acquire_packet_keys(
                token.as_bytes().as_ptr(),
                token.as_bytes().len(),
                odd.as_mut_ptr(),
                even.as_mut_ptr(),
            )
        };
        let result = match status {
            CAS_KEY_OK => build_key_slot(parameters, odd, even),
            CAS_KEY_INVALID_TOKEN => Err(CasKeyResolveError::InvalidToken),
            CAS_KEY_UNKNOWN_TOKEN => Err(CasKeyResolveError::UnknownToken),
            _ => Err(CasKeyResolveError::Unavailable),
        };
        erase(&mut odd);
        erase(&mut even);
        result
    }
}

#[cfg(not(target_os = "android"))]
impl CasKeyResolver for ProductCasKeyResolver {
    fn resolve(
        &self,
        _token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlot, CasKeyResolveError> {
        Err(CasKeyResolveError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyParity;

    #[test]
    fn product_parameters_build_both_packet_keys() {
        let parameters = ProductMulti2Parameters {
            system_key: [0x33; 32],
            init_cbc: [0x44; 8],
        };
        let slot = build_key_slot(&parameters, [0x11; 8], [0x22; 8]).unwrap();
        assert!(slot.key_for(KeyParity::Odd).is_some());
        assert!(slot.key_for(KeyParity::Even).is_some());
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn host_build_does_not_claim_a_product_cas_connection() {
        let token = DescramblerKeyToken::try_from_bytes(vec![0x33; 16]).unwrap();
        assert_eq!(
            ProductCasKeyResolver.resolve(&token),
            Err(CasKeyResolveError::Unavailable)
        );
    }
}
