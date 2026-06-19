use maleicacid_tuner_hal2_common::HalError;

pub(crate) fn finish_open_rollback<F>(
    object_registration_rollback: Result<(), HalError>,
    runtime_cleanup: F,
    resource: &'static str,
) -> Result<(), HalError>
where
    F: FnOnce() -> Result<(), HalError>,
{
    let runtime_cleanup = runtime_cleanup();
    match (object_registration_rollback, runtime_cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(object_error), Err(runtime_error)) => Err(HalError::composed_failure(
            resource,
            object_error,
            runtime_error,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{HalInternalKind, HalInvalidStateKind};

    #[test]
    fn runtime_cleanup_runs_after_object_rollback_failure() {
        let mut cleanup_called = false;
        let result = finish_open_rollback(
            Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "object rollback failed",
            )),
            || {
                cleanup_called = true;
                Ok(())
            },
            "test open rollback",
        );

        assert!(cleanup_called);
        assert!(matches!(result, Err(HalError::InvalidState { .. })));
    }

    #[test]
    fn both_failures_are_classified_as_composed_failure() {
        let result = finish_open_rollback(
            Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "object rollback failed",
            )),
            || {
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "runtime cleanup failed",
                ))
            },
            "test open rollback",
        );

        let Err(HalError::ComposedFailure {
            primary, cleanup, ..
        }) = result
        else {
            panic!("expected composed failure: {result:?}");
        };
        assert!(matches!(*primary, HalError::InvalidState { .. }));
        assert!(matches!(*cleanup, HalError::Internal { .. }));
    }
}
