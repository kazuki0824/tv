use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
#[cfg(target_os = "android")]
use std::sync::OnceLock;

// 固定parameterはCAS方式ごとに別の製品入力として一度だけ読み込む。
// 動的なodd/even Ksはこの構造体・キャッシュに含めない。
pub(super) struct Multi2FixedParameters {
    pub(super) system_key: [u8; 32],
    pub(super) init_cbc: [u8; 8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Multi2Scheme {
    B25,
    B1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProductParametersError {
    Io,
    InvalidFile,
    InvalidLength,
    UnsupportedScheme,
}

impl Multi2FixedParameters {
    fn read_from(reader: impl Read) -> Result<Self, ProductParametersError> {
        // EOFまで最大41 byteを読むことで、末尾データや長さ超過も拒否する。
        let mut bytes = [0_u8; 41];
        let mut bounded = reader.take(bytes.len() as u64);
        let mut length = 0;
        loop {
            match bounded.read(&mut bytes[length..]) {
                Ok(0) => break,
                Ok(count) => length += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(ProductParametersError::Io),
            }
        }
        if length != 40 {
            return Err(ProductParametersError::InvalidLength);
        }
        let mut system_key = [0_u8; 32];
        let mut init_cbc = [0_u8; 8];
        system_key.copy_from_slice(&bytes[..32]);
        init_cbc.copy_from_slice(&bytes[32..40]);
        Ok(Self {
            system_key,
            init_cbc,
        })
    }

    fn read_file(path: &Path) -> Result<Self, ProductParametersError> {
        // vendor imageの固定入力をsymlink経由で開かず、FIFO等で無期限待機しない。
        // 秘密性は要求せず、入力境界として通常ファイルであることだけを確認する。
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(0x20000 | 0x800)
            .open(path)
            .map_err(|_| ProductParametersError::Io)?;
        let metadata = file.metadata().map_err(|_| ProductParametersError::Io)?;
        if !metadata.is_file() {
            return Err(ProductParametersError::InvalidFile);
        }
        Self::read_from(file)
    }
}

#[cfg(target_os = "android")]
pub(super) fn product_parameters(
    scheme: Multi2Scheme,
) -> Result<&'static Multi2FixedParameters, ProductParametersError> {
    static B25_PARAMETERS: OnceLock<Result<Multi2FixedParameters, ProductParametersError>> =
        OnceLock::new();
    match scheme {
        Multi2Scheme::B25 => B25_PARAMETERS
            .get_or_init(|| {
                Multi2FixedParameters::read_file(Path::new(
                    "/vendor/etc/maleicacid/b25_multi2_parameters",
                ))
            })
            .as_ref()
            .map_err(|error| *error),
        // B1値をB25から流用しない。B1 CAS実装が値と取得契約を接続するまで拒否する。
        Multi2Scheme::B1 => Err(ProductParametersError::UnsupportedScheme),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_input_keeps_system_and_cbc_byte_order() {
        let bytes: Vec<u8> = (0..40).collect();
        let parameters = Multi2FixedParameters::read_from(bytes.as_slice()).unwrap();
        assert_eq!(parameters.system_key.as_slice(), &bytes[..32]);
        assert_eq!(parameters.init_cbc.as_slice(), &bytes[32..]);
    }

    #[test]
    fn input_requires_exactly_forty_bytes() {
        for length in [0, 1, 39, 41, 4096] {
            let bytes = vec![0x55; length];
            assert_eq!(
                Multi2FixedParameters::read_from(bytes.as_slice()).err(),
                Some(ProductParametersError::InvalidLength)
            );
        }
    }

    #[test]
    fn io_failure_does_not_supply_defaults() {
        struct FailedReader;
        impl Read for FailedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            }
        }
        assert_eq!(
            Multi2FixedParameters::read_from(FailedReader).err(),
            Some(ProductParametersError::Io)
        );
    }

    #[test]
    fn non_regular_product_input_is_rejected() {
        assert_eq!(
            Multi2FixedParameters::read_file(Path::new("/dev/null")).err(),
            Some(ProductParametersError::InvalidFile)
        );
    }

    #[test]
    fn absent_product_input_does_not_supply_defaults() {
        assert_eq!(
            Multi2FixedParameters::read_file(Path::new("/dev/null/b25_multi2_parameters")).err(),
            Some(ProductParametersError::Io)
        );
    }

    #[test]
    fn schemes_are_distinct_and_b1_has_no_b25_fallback() {
        assert_ne!(Multi2Scheme::B25, Multi2Scheme::B1);
        assert_eq!(ProductParametersError::UnsupportedScheme, ProductParametersError::UnsupportedScheme);
    }
}
