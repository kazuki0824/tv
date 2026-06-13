pub use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AidlObjectHandle {
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AidlObjectHandleError {
    pub expected: AidlObjectKind,
    pub actual: AidlObjectKind,
}

impl AidlObjectHandle {
    pub const fn new(object_kind: AidlObjectKind, object_id: AidlObjectId, generation: AidlObjectGeneration) -> Self {
        Self { object_kind, object_id, generation }
    }

    pub const fn object_kind(&self) -> AidlObjectKind { self.object_kind }
    pub const fn object_id(&self) -> AidlObjectId { self.object_id }
    pub const fn generation(&self) -> AidlObjectGeneration { self.generation }

    pub fn ensure_kind(&self, expected: AidlObjectKind) -> Result<(), AidlObjectHandleError> {
        if self.object_kind == expected {
            Ok(())
        } else {
            Err(AidlObjectHandleError { expected, actual: self.object_kind })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_carries_kind_id_generation_only() {
        let handle = AidlObjectHandle::new(AidlObjectKind::Filter, AidlObjectId(7), AidlObjectGeneration(3));
        assert_eq!(handle.object_kind(), AidlObjectKind::Filter);
        assert_eq!(handle.object_id(), AidlObjectId(7));
        assert_eq!(handle.generation(), AidlObjectGeneration(3));
    }

    #[test]
    fn kind_mismatch_is_typed_error_not_panic() {
        let handle = AidlObjectHandle::new(AidlObjectKind::Filter, AidlObjectId(7), AidlObjectGeneration(3));
        assert_eq!(
            handle.ensure_kind(AidlObjectKind::Dvr),
            Err(AidlObjectHandleError { expected: AidlObjectKind::Dvr, actual: AidlObjectKind::Filter }),
        );
    }
}
