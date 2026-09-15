#[cfg(any(target_os = "android", test))]
use crate::product_parameters::{Multi2FixedParameters, Multi2Scheme};
#[cfg(any(target_os = "android", test))]
use crate::Multi2KeyMaterial;
use crate::{DescramblerKeySlot, DescramblerKeyToken};
#[cfg(target_os = "android")]
use std::ptr::{self, NonNull};
#[cfg(target_os = "android")]
use std::sync::atomic::{compiler_fence, Ordering};
use std::sync::Arc;

#[cfg(target_os = "android")]
const CAS_KEY_OK: i32 = 0;
#[cfg(target_os = "android")]
const CAS_KEY_INVALID_TOKEN: i32 = 1;
#[cfg(target_os = "android")]
const CAS_KEY_UNKNOWN_TOKEN: i32 = 2;
#[cfg(target_os = "android")]
const CAS_SCHEME_B25: u8 = 1;
#[cfg(target_os = "android")]
const CAS_SCHEME_B1: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasKeyResolveError {
    InvalidToken,
    UnknownToken,
    Unavailable,
    InvalidKeyMaterial,
}

/// CAS が更新する同じ参照先から、その packet だけで使用する鍵を局所取得する。
/// snapshot は外部プロセスへの問い合わせや参照先の変更を行わない。
pub trait CasKeyReference: std::fmt::Debug + Send + Sync {
    fn snapshot(&self) -> Result<DescramblerKeySlot, CasKeyResolveError>;
}

pub trait CasKeyResolver: Send + Sync {
    fn resolve(
        &self,
        token: &DescramblerKeyToken,
    ) -> Result<Arc<dyn CasKeyReference>, CasKeyResolveError>;
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
    parameters: &Multi2FixedParameters,
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
fn scheme_from_reference(
    reference: *const std::ffi::c_void,
) -> Result<Multi2Scheme, CasKeyResolveError> {
    // token自体を解析せず、CAS側が結合結果へ付けた方式識別だけを使う。
    match unsafe { maleicacid_cas_key_reference_scheme(reference) } {
        CAS_SCHEME_B25 => Ok(Multi2Scheme::B25),
        CAS_SCHEME_B1 => Ok(Multi2Scheme::B1),
        _ => Err(CasKeyResolveError::Unavailable),
    }
}

#[cfg(target_os = "android")]
extern "C" {
    fn maleicacid_cas_bind_key_reference(
        token: *const u8,
        length: usize,
        reference: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn maleicacid_cas_key_reference_scheme(reference: *const std::ffi::c_void) -> u8;
    fn maleicacid_cas_release_key_reference(reference: *mut std::ffi::c_void);
    fn maleicacid_cas_snapshot_key_reference(
        reference: *const std::ffi::c_void,
        odd: *mut u8,
        even: *mut u8,
    ) -> i32;
}

#[cfg(target_os = "android")]
#[derive(Debug)]
struct ProductKeyReference {
    reference: NonNull<std::ffi::c_void>,
    scheme: Multi2Scheme,
}

// C++ 参照は読取り専用 mapping と不変の所有者 fd を所有する。snapshot 同士は
// 競合せず、最後の Arc が破棄されるまで release は実行されない。
#[cfg(target_os = "android")]
unsafe impl Send for ProductKeyReference {}
#[cfg(target_os = "android")]
unsafe impl Sync for ProductKeyReference {}

#[cfg(target_os = "android")]
impl Drop for ProductKeyReference {
    fn drop(&mut self) {
        // bind で取得した唯一の所有参照を一度だけ解放する。
        unsafe { maleicacid_cas_release_key_reference(self.reference.as_ptr()) };
    }
}

#[cfg(target_os = "android")]
impl CasKeyReference for ProductKeyReference {
    fn snapshot(&self) -> Result<DescramblerKeySlot, CasKeyResolveError> {
        let parameters = crate::product_parameters::product_parameters(self.scheme)
            .map_err(|_| CasKeyResolveError::Unavailable)?;
        let mut odd = [0_u8; 8];
        let mut even = [0_u8; 8];
        // self が所有する C++ 参照は呼出し中有効であり、各出力は8 byteを確保済み。
        let status = unsafe {
            maleicacid_cas_snapshot_key_reference(
                self.reference.as_ptr(),
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

#[cfg(target_os = "android")]
impl CasKeyResolver for ProductCasKeyResolver {
    fn resolve(
        &self,
        token: &DescramblerKeyToken,
    ) -> Result<Arc<dyn CasKeyReference>, CasKeyResolveError> {
        let mut reference = ptr::null_mut();
        // token は呼出し中有効。成功時の不透明参照の所有権を受け取る。
        let status = unsafe {
            maleicacid_cas_bind_key_reference(
                token.as_bytes().as_ptr(),
                token.as_bytes().len(),
                &mut reference,
            )
        };
        match status {
            CAS_KEY_OK => {
                let reference = NonNull::new(reference).ok_or(CasKeyResolveError::Unavailable)?;
                let scheme = match scheme_from_reference(reference.as_ptr()) {
                    Ok(scheme) => scheme,
                    Err(error) => {
                        unsafe { maleicacid_cas_release_key_reference(reference.as_ptr()) };
                        return Err(error);
                    }
                };
                // 対応する固定parameterがない方式は結合成功として扱わない。
                if crate::product_parameters::product_parameters(scheme).is_err() {
                    unsafe { maleicacid_cas_release_key_reference(reference.as_ptr()) };
                    return Err(CasKeyResolveError::Unavailable);
                }
                Ok(Arc::new(ProductKeyReference { reference, scheme }) as Arc<dyn CasKeyReference>)
            }
            CAS_KEY_INVALID_TOKEN => Err(CasKeyResolveError::InvalidToken),
            CAS_KEY_UNKNOWN_TOKEN => Err(CasKeyResolveError::UnknownToken),
            _ => Err(CasKeyResolveError::Unavailable),
        }
    }
}

#[cfg(not(target_os = "android"))]
impl CasKeyResolver for ProductCasKeyResolver {
    fn resolve(
        &self,
        _token: &DescramblerKeyToken,
    ) -> Result<Arc<dyn CasKeyReference>, CasKeyResolveError> {
        Err(CasKeyResolveError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyParity;

    #[test]
    fn b25_parameters_build_both_packet_keys() {
        let parameters = Multi2FixedParameters {
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
        assert!(matches!(
            ProductCasKeyResolver.resolve(&token),
            Err(CasKeyResolveError::Unavailable)
        ));
    }
}
