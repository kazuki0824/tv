use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
#[cfg(target_os = "android")]
use std::sync::OnceLock;

// 製品入力は更新不能なvendor設定としてプロセス内で一度だけ読み込む。
// 動的なodd/even Ksはこの構造体・キャッシュに含めない。
pub(super) struct ProductMulti2Parameters {
    pub(super) system_key: [u8; 32],
    pub(super) init_cbc: [u8; 8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProductParametersError {
    Io,
    InvalidFile,
    InvalidLength,
}

impl ProductMulti2Parameters {
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
        // Android/LinuxのO_NOFOLLOWとO_NONBLOCK。symlinkを追わず、FIFO等で
        // metadata検証前に無期限待機しない。open済みfdの属性だけを検証する。
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(0x20000 | 0x800)
            .open(path)
            .map_err(|_| ProductParametersError::Io)?;
        let metadata = file.metadata().map_err(|_| ProductParametersError::Io)?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.gid() != 1000
            || metadata.mode() & 0o7777 != 0o640
        {
            return Err(ProductParametersError::InvalidFile);
        }
        Self::read_from(file)
    }
}

#[cfg(target_os = "android")]
pub(super) fn product_parameters(
) -> Result<&'static ProductMulti2Parameters, ProductParametersError> {
    static PARAMETERS: OnceLock<Result<ProductMulti2Parameters, ProductParametersError>> =
        OnceLock::new();
    PARAMETERS
        .get_or_init(|| {
            ProductMulti2Parameters::read_file(Path::new(
                "/vendor/etc/maleicacid/multi2_parameters",
            ))
        })
        .as_ref()
        .map_err(|error| *error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_input_keeps_system_and_cbc_byte_order() {
        let bytes: Vec<u8> = (0..40).collect();
        let parameters = ProductMulti2Parameters::read_from(bytes.as_slice()).unwrap();
        assert_eq!(parameters.system_key.as_slice(), &bytes[..32]);
        assert_eq!(parameters.init_cbc.as_slice(), &bytes[32..]);
    }

    #[test]
    fn input_requires_exactly_forty_bytes() {
        for length in [0, 1, 39, 41, 4096] {
            let bytes = vec![0x55; length];
            assert_eq!(
                ProductMulti2Parameters::read_from(bytes.as_slice()).err(),
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
            ProductMulti2Parameters::read_from(FailedReader).err(),
            Some(ProductParametersError::Io)
        );
    }

    #[test]
    fn non_regular_product_input_is_rejected() {
        assert_eq!(
            ProductMulti2Parameters::read_file(Path::new("/dev/null")).err(),
            Some(ProductParametersError::InvalidFile)
        );
    }

    #[test]
    fn absent_product_input_does_not_supply_defaults() {
        assert_eq!(
            ProductMulti2Parameters::read_file(Path::new("/dev/null/multi2_parameters")).err(),
            Some(ProductParametersError::Io)
        );
    }
}
