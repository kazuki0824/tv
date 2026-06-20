use super::{
    compose_primary_cleanup_failure, fail_after_cleanup, finish_cleanup_after_primary_failure,
    HalError, HalInvalidStateKind,
};

fn primary_error(label: &'static str) -> HalError {
    HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, label)
}

fn cleanup_error(label: &'static str) -> HalError {
    HalError::cleanup_failed("test cleanup", label)
}

#[test]
fn common_composition_preserves_primary_and_cleanup() {
    let primary = primary_error("primary failure");
    let cleanup = cleanup_error("cleanup failure");

    let composed = compose_primary_cleanup_failure(
        "failure injection common composition",
        primary.clone(),
        cleanup.clone(),
    );

    assert_eq!(composed.primary_error(), &primary);
    assert_eq!(composed.cleanup_error(), Some(&cleanup));
}

#[test]
fn common_finish_keeps_primary_when_cleanup_succeeds() {
    let primary = primary_error("primary failure");

    let result = finish_cleanup_after_primary_failure(
        "failure injection cleanup success",
        primary.clone(),
        Ok(()),
    );

    assert_eq!(result, primary);
}

#[test]
fn common_finish_composes_when_cleanup_fails() {
    let primary = primary_error("primary failure");
    let cleanup = cleanup_error("cleanup failure");

    let result = finish_cleanup_after_primary_failure(
        "failure injection cleanup failure",
        primary.clone(),
        Err(cleanup.clone()),
    );

    assert_eq!(result.primary_error(), &primary);
    assert_eq!(result.cleanup_error(), Some(&cleanup));
}

#[test]
fn common_fail_after_cleanup_returns_err_with_composition() {
    let primary = primary_error("primary failure");
    let cleanup = cleanup_error("cleanup failure");

    let result: Result<(), HalError> = fail_after_cleanup(
        "failure injection result composition",
        primary.clone(),
        Err(cleanup.clone()),
    );

    let Err(error) = result else {
        panic!("expected failure");
    };
    assert_eq!(error.primary_error(), &primary);
    assert_eq!(error.cleanup_error(), Some(&cleanup));
}
