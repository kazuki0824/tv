# r50ema_wp_r07_filter_delay_queue_runtime_gate

- WP-R07継続として、demux runtime の filter queue に queued-byte と delivery deadline を持たせ、`FilterDelayHint` を queue drain readiness へ接続した。
- packet pipeline の generated event から raw/record/section/PES payload を filter queue へ積み、time/data-size hint の ready 判定が実queue状態を見るようにした。
- filter `configure()` / `stop()` / `flush()` / queue clear / queue remove で delay queue state も同時に破棄するよう補正した。
- demux unit test に、time-delay の再arm、data-size 閾値、time/data OR 判定、packet push 経由の raw queue enqueue を追加した。
- `m maleicacid_tuner_hal2_demux_test maleicacid_tuner_hal2_service_runtime_test` は成功。rustfmt は実施。Rust unit test 実行、`atest`、VTS、実機確認は未実施。

# r50el_test_mod_cleanup_and_future_work_retirement_unverified

- Retired completed r51 future_work files for rollback/cleanup failure composition and tuner_hal2 failure-injection tests after moving the implemented scope into current code/tests.
- Added isolated `#[cfg(test)]` failure-injection modules instead of production test branches:
  - `common/src/failure_injection_tests.rs`
  - `service_runtime/src/failure_injection_tests.rs`
  - `aidl_service/src/failure_injection_tests.rs`
- Kept the new tests as logic-path tests over typed `HalError` composition, close cleanup marking, public-close missing-target cleanup, callback rollback, and close-domain cleanup; they do not use `include_str!`, grep, or source-text self-inspection.
- Routed remaining direct production `HalError::composed_failure(...)` call sites through the common composition helper where applicable.
- Removed `future_work/r51/rollback_failure_composition_common_component_plan.md` and `future_work/r51/tuner_hal2_failure_injection_tests_plan.md` so completed work is not left as a misleading future-work item.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ek_future_work_composition_and_boundary_fix_unverified

- Advanced the rollback/cleanup failure composition future-work item by adding common helpers in `common/src/lib.rs`: `compose_primary_cleanup_failure()`, `finish_cleanup_after_primary_failure()`, and `fail_after_cleanup()`.
- Routed open rollback and AIDL root/child cleanup composition helpers through the common composition helpers instead of local-only composition logic.
- Advanced the failure-injection future-work item by adding common-helper coverage for primary-only and primary+cleanup failure composition and updating the future-work file to list only remaining build-verified test coverage.
- Converted close-domain cleanup closure ABI from `BinderResult<()>` to `Result<(), HalError>` and updated frontend/LNB close call sites and close cleanup tests accordingly.
- Converted child filter/DVR object construction failure cleanup from Binder `Status` primary handling to typed `HalError` primary handling.
- Updated future_work/r51 documents to distinguish r50ek implemented progress from remaining compiler/build-verified work.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei106_callback_closure_typed_boundary_unverified

- Added `vendor/maleicacid/tv/future_work/r51/tuner_hal2_failure_injection_tests_plan.md` so failure-injection test coverage is tracked only as future work and not as a current-release completion requirement.
- Added `AidlCallbackStoreError::into_hal_error()` and routed callback store retain failures through typed `HalError` instead of object-local Binder `Status` conversion.
- Changed callback registration retain/rollback closure bounds in `object_runtime.rs` from `BinderResult<()>` to `Result<(), HalError>`.
- Updated frontend/LNB setCallback and child filter/DVR callback retain/rollback closures to use typed callback store and callback cleanup failures.
- Kept close-domain cleanup closure ABI as `BinderResult<()>`; converting it requires updating close call sites and tests together with compiler verification.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei105_object_runtime_status_bridge_reduction_unverified

- Reduced typed-failure loss inside `aidl_service/src/object_runtime.rs` by adding HalError-returning callback-registry cleanup/marking helpers while keeping Binder-returning public wrappers at the AIDL boundary.
- Changed callback registration record failure handling to preserve the typed primary `HalError` and compose it with rollback `Status` only at the final Binder boundary.
- Changed callback registration domain failure handling so the domain `HalError` is not converted to Binder `Status` before rollback/unhealthy-marking composition.
- Changed public close cleanup to collect callback cleanup and domain cleanup as `HalError`, and to compose cleanup-failed marking failure as typed composed failure before Binder status conversion.
- Changed Drop leak LNB domain-record failure from Binder `Status` conversion back to typed `HalError` collection.
- Stopped at closure/API boundaries that still expose `BinderResult` for callback retain/rollback and domain cleanup; converting those requires wider signature changes across object-specific AIDL methods and should be compiler-verified.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei104_status_bridge_reduction_unverified

- Reduced typed-failure loss at AIDL construction boundaries by adding HalError-preserving cleanup composition helpers for root object construction/id-conversion rollback.
- Updated frontend/demux/descrambler/LNB root object construction and runtime-id conversion rollback paths to compose typed primary `HalError` with typed rollback failure before Binder status conversion.
- Updated child filter/DVR runtime-id conversion rollback to preserve typed primary and rollback failure; callback-retain rollback now preserves typed rollback failure even though the primary callback retain failure is already a Binder `Status`.
- Rechecked runtime unregister signature propagation after r50ei103 and removed an ignored `Option` in the affected service-runtime test.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei103_unregister_cleanup_result_propagation_unverified

- Continued the r50ei102 failure-composition foundation by converting demux/filter/DVR/descrambler runtime unregister paths to return cleanup failure instead of silent `Option`-only success/failure.
- Converted descrambler session cleanup and demux owner-loss descrambler cleanup to `FirstErrorCollector`-based returned cleanup failure while retaining diagnostic records.
- Propagated unregister cleanup failures through root-open rollback, public close cleanup, and child filter/DVR open rollback paths.
- Preserved all-attempt cleanup where local code can continue after a cleanup failure.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei102_failure_composition_foundation_unverified

- Added `HalError::ComposedFailure` as the typed primary-plus-cleanup failure container and routed AIDL status mapping through the primary error while preserving cleanup detail in the display text.
- Updated root/child open rollback paths so rollback or cleanup failure is composed with the primary open/construction/registration failure instead of replacing it.
- Updated callback registration, public close cleanup, Drop leak quarantine, frontend tune/scan worker rollback, and cleanup-failed marking paths to preserve primary-plus-cleanup failure context.
- Tightened missing-target handling for rollback/public close/owner-loss cleanup paths where this can be detected locally.
- Updated the r50ei101 implementation plan target with code changes only; no new current-scope feature implementation was added.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei92_first_error_collector_residual_cleanup_fix

- Replaced the remaining child-open callback cleanup / rollback pairwise `match` blocks with `FirstErrorCollector` while preserving the filter/DVR child-open cleanup semantic wrappers.
- Replaced the frontend tune-worker commit-failure rollback secondary-error accumulator with `FirstErrorCollector`; primary commit error plus rollback error detail composition is still preserved.
- Replaced the Drop leak `Option<String>` first-error helper with `FirstErrorCollector<String>` while preserving the callback-registry clear-vs-unhealthy branch.
- Updated `tuner_hal2/RELEASE_VERSION` to this release name.
- Android/Soong `m`, `atest`, VTS, device checks, and `rustfmt` were not run in this environment.

# r50ei91_cleanup_all_attempt_first_error_fix

- Added `FirstErrorCollector` as the multi-step cleanup all-attempt / first-error common component and documented it in `tuner_hal2/DESIGN_JA.md`.
- Changed the AIDL object close finalizer so callback cleanup failure no longer prevents domain cleanup from being attempted; cleanup-failed marking is attempted after all close cleanup steps.
- Changed frontend close cleanup so LNB owner-loss close, tune/scan worker stop, scan cancel record, live-data close/unbind, and closed-LNB callback cleanup preserve first-error precedence while still attempting later cleanup steps.
- Changed scan-worker supersede cleanup so scan cancel recording no longer masks a completed worker failure.
- Updated `tuner_hal2/RELEASE_VERSION` to this release name.
- Android/Soong `m`, `atest`, VTS, device checks, and `rustfmt` were not run in this environment.

# r50ei90_worker_session_stop_child_open_drop_diagnostic_fix

- Added frontend backend-session stop finalization so tune/scan worker bodies still attempt `FrontendBackendSession::stop()` after session-open-success error paths; tune live-pump stop failure is combined with the primary worker-body failure instead of detaching silently.
- Changed frontend worker stop handling so `Completed { result: Err(..) }` remains distinguishable from `StopRequestFailed`: `stopTune()` / `stopScan()` now preserve first-failure precedence while still attempting safe terminal cleanup and live-data/live-reader cleanup where the worker has already completed.
- Strengthened Drop leak accounting so callback-store clear failure, domain drop-leak record failure, callback-registry missing, and object-quarantine failure are returned to `drop_leak_object_from_drop()` for in-memory error recording instead of being swallowed after unhealthy marking.
- Added `StartupDiagnosticKind::DuplicateLnbId` and records default-LNB registration collisions as LNB duplicate diagnostics instead of frontend duplicate diagnostics.
- Added rollback for filter/DVR child-open AIDL object registration when child `ledger_id` cannot be represented as `i32`, so typed AIDL object construction does not leave a registered child object on numeric conversion failure.
- Android/Soong `m`, `atest`, VTS, device checks, and `rustfmt` were not run in this environment; `rustfmt` was attempted but the command is not installed.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified13

- Generalized `DESIGN_JA.md` and `CODE_CONVENTION.md` from the close-specific AIDL object wrapper ban to a broad ban on public thin wrappers that do not own state, lifetime, phase order, rollback, or error precedence.
- Removed the public frontend/LNB `rollback_callback_registration()` wrapper methods by inlining callback cleanup into the callback registration rollback closures.
- Removed the public filter profile validation wrappers used only by tests; tests now call the service_runtime profile validation functions directly.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified12

- Fixed the LNB `Drop` syntax error introduced in the drop-leak wrapper conversion.
- Removed all remaining AIDL object close thin wrappers (`close_object_after_close_preflight()` / `close_object()`) from frontend/filter/demux/dvr/descrambler/LNB object wrappers; close trait methods now call the `object_runtime` façade directly.
- Changed `drop_leak_object()` lock ordering so callback-store owner cleanup runs before acquiring `TunerServiceRuntime` lock; runtime registry clear/unhealthy and object quarantine remain inside the runtime critical section.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to forbid AIDL object close thin wrappers and to require callback-store cleanup outside the service runtime lock in Drop leak handling.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified11

- Added rollback handling for `start_frontend_backend_tune_worker()` after worker start succeeds but `commit_frontend_active_tune_request()` fails: the started worker is stopped/joined via the two-phase stop path, frontend runtime snapshot restore and bound demux snapshot restore are attempted, and primary/secondary failure context is surfaced.
- Fixed `close_frontend_workers_and_live_data()` error precedence so live-data close/unbind failure no longer overwrites an earlier worker stop or scan-cancel-record failure; live-data cleanup is still attempted.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to require post-worker-start fallible commit rollback and first-failure precedence during frontend close cleanup.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified10

- Replaced frontend worker stop-and-join with a two-phase stop ticket: runtime lock now only removes the worker slot, records cancel reason, sets the cancel flag, and returns `FrontendWorkerStopTicket`; blocking `complete()` / `JoinHandle` wait is performed after releasing `TunerServiceRuntime` lock.
- Updated scan-start supersede handling to request the old scan worker stop under runtime lock, release the lock for join, then reacquire runtime lock before recording scan cancellation and starting the replacement scan worker.
- Updated stopTune/stopScan/frontend close paths by routing them through the same lock-free join phase in `stop_frontend_worker()`.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to forbid frontend worker blocking join while holding `TunerServiceRuntime` lock.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified9

- Changed actual AIDL object `Drop` implementations to call `drop_leak_object_from_drop()` instead of discarding `drop_leak_object()` results with `drop(...)`; returned drop-leak errors are recorded in the drop-leak diagnostic record without using `eprintln!`.
- Changed callback-store owner cleanup to return the number of removed callback artifacts and changed `RuntimeCallbackRegistry::clear_owner()` to return `CallbackRegistryUpdate`; callback clear now treats store-removed/runtime-missing as an error while allowing no-store/no-runtime as already-cleared.
- Removed stale `ensure_open()` wrappers and the exported `ensure_object_live()` helper so AIDL object wrappers cannot bypass the object-runtime façade.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to make the Drop-leak diagnostic record, callback owner clear missing semantics, and no-stderr/no-`drop(result)` rule explicit.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified8

- Tightened `DESIGN_JA.md` so Drop leak cannot treat `CallbackRegistryUpdate::Missing` as covered merely by quarantine; registry missing must be returned or recorded after quarantine is attempted.
- Changed `drop_leak_object()` to remember callback registry owner-missing during unhealthy marking, always attempt object quarantine, then return an error for the missing registry entry after quarantine/unregister work completes.
- Split scan END failure handling so callback artifact absence / callback store failure records only the scan-session failure and returns the artifact/store failure, while actual Binder delivery failure to a retrieved callback performs runtime registry unhealthy marking.
- Added a drop leak regression test proving registry-missing is reported while the object is still quarantined.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified7

- Changed `ThreadResultProducer` so producer-side result-cell lock poison is captured through the producer-failure side channel instead of returning a value that the worker thread must discard.
- Strengthened `execute_object_query_use_case()` so the query façade itself verifies object live / generation / kind before runtime dispatch planning and before running the query closure.
- Added a query façade behavior test proving a missing object is rejected before the query closure executes.
- Updated `DESIGN_JA.md` / `CODE_CONVENTION.md` wording for `ThreadResultProducer::record_or_capture_failure()` to match the side-channel design.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified6

- Added `RuntimeCommandDispatchError::RuntimeLockPoison` and changed root method dispatch planning so service runtime lock poison is no longer misclassified as `MissingDispatchTarget`.
- Changed filter/DVR child-object construction failure cleanup so callback cleanup and child-open rollback are both attempted; callback cleanup failure no longer prevents child-open rollback from running.
- Hardened `ThreadResultOwner` against double collection with an explicit already-collected failure, added a behavior test for the second collect, and added a producer-failure side channel so producer-side result lock poison is reported to the owner without `eprintln!`.
- Removed remaining `eprintln!` diagnostics from tuner_hal2 Rust sources; existing in-memory diagnostics and returned `HalError` paths remain the reporting mechanisms.
- `IFilter.getId()` / `getId64Bit()` remain on `execute_object_query_use_case()`; this is the intended pure-query façade and not a direct AIDL method-body call to `public_runtime_id_for_object_method()`.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified5

- Changed `ThreadResultProducer::record()` to return `HalError` on result-cell lock poison and log that failure from the worker thread, while leaving final caller-visible classification to `ThreadResultOwner`.
- Documented the `catch_unwind(AssertUnwindSafe(...))` contract for one-shot worker closures in `DESIGN_JA.md` and `CODE_CONVENTION.md`.
- Rechecked `IFilter.getId()` / `getId64Bit()` and kept them on `execute_object_query_use_case()`; their use of `public_runtime_id_for_object_method()` occurs inside the query façade and is the intended object live / generation / kind check for pure query.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified4

- Added `device/src/runtime/thread_result_owner.rs` to the `libmaleicacid_tuner_hal2_device` and `maleicacid_tuner_hal2_device_test` `Android.bp` `srcs` lists so the new module is included by Soong.
- No behavior changes beyond build graph correction.
- `rustfmt`, Android/Soong `m` gates, and `atest` gates were not run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified3

- Removed the unused `CallbackHealthState::Cleared` state and fixed the design wording so callback clear means RuntimeCallbackRegistry entry removal, not a tombstone health state.
- Narrowed `ThreadResultOwner`, `ThreadResultPoll`, and `ThreadResultFailure` to crate-internal/module-internal visibility, made `ThreadResultProducer` private, removed `ThreadResult*` re-exports from `device/src/runtime/mod.rs` and `device/src/lib.rs`, and kept spawn failure as a start-time `HalError` rather than a worker-result enum case.
- Removed all `plan_method_without_execution()` object-wrapper/service public helpers and the `plan_object_method_without_runtime_execution()` helper from `aidl_service::object_runtime`; plan-only object method paths must now go through the allowed façade use-cases.
- Removed the direct plan-only unit test that called the deleted helper and updated `DESIGN_JA.md` / `CODE_CONVENTION.md` so `aidl_service::object_runtime` remains the AIDL object method executor façade.
- Re-ran static source checks for stale `Cleared`, `ThreadResult*` public re-exports, plan-only helper names, `method_execution`, poison/missing-result success rounding, and direct AIDL `plan_object_method_dispatch`; they returned no matches outside intended internal uses.
- `rustfmt`, Android/Soong `m` gates, and `atest` gates were not run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified2

- Source-level completion pass for worker result ownership, method execution façade boundary, callback registry ownership, and ObjectMethodTxn/ObjectClose/root-open/query/unavailable method-category documentation.
- Added `device/src/runtime/thread_result_owner.rs` and connected `FrontendWorkerRegistry` / `FrontendWorkerSlot` and `FrontendLivePumpOwner` through `ThreadResultOwner`, so panic, result lock poison, and missing report are surfaced as `HalError` rather than success/finished states.
- Added behavior tests for `ThreadResultOwner`, frontend worker missing/poison result reporting, and live pump panic/missing/poison reporting.
- Deleted `service_runtime/src/method_execution.rs` and kept the low-level runtime execution helpers as private helpers inside `aidl_service/src/object_runtime.rs`, making that file the AIDL object method executor façade.
- Changed callback registry unhealthy marking to return `CallbackRegistryUpdate`, propagated frontend callback unhealthy-marking failures, and marked callback registrations unhealthy when callback registration rollback fails after domain commit failure.
- Added callback registry missing-update and callback rollback-failure behavior tests.
- Added `DESIGN_JA.md` ownership text for callback_store / RuntimeCallbackRegistry / domain runtime state and the AIDL method category required-boundary table.
- Static source checks for poison/missing success rounding, AIDL direct low-level executor references, and AIDL direct `plan_object_method_dispatch` references were run and returned no matches outside the allowed façade.
- Changed `drop_leak_object()` to return a Binder result instead of silently returning on runtime lock poison / quarantine failure; Drop implementations now log drop-leak handling failures rather than treating them as invisible.
- Prevented `FrontendWorkerRegistry::start()` from overwriting a completed-but-unreported worker failure; such failures must be reported via `take_completed()` before replacement.
- `rustfmt`, Android/Soong `m` gates, and `atest` gates were not run in this environment; this archive is source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_doc_responsibility_tuner_docs_fix

- Corrected the `tuner_hal2/INTEGRATION.md` legacy `tuner_hal/INTEGRATION.md` wording: the legacy integration document may exist for reference, but it is not the default product integration SSOT.
- Updated the archive release marker after the tuner documentation responsibility cleanup.
- Code behavior was not changed. Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei88_callback_registration_phase_order_fix

- Added `future_work/r51/rollback_failure_composition_common_component_plan.md` to track generic primary-failure / rollback-failure status composition as r51 work instead of expanding that large-scope change in r50.
- Changed filter/DVR child open completion so callback artifact registration runs before typed AIDL object construction, avoiding the callback-failure path where explicit child rollback and Drop leak handling could run for the same just-created AIDL object.
- Moved frontend/LNB setCallback onto the existing `ObjectMethodTxn` dispatch-preflight boundary before callback store retain, and pass the dispatch-preflight proof into the service_runtime callback commit use-cases.
- Added a regression test that verifies callback retain is not attempted when ObjectMethodTxn dispatch preflight fails before registration.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to fix the callback registration and child open phase-order contracts around existing common components.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei87_callback_artifact_registration_core_unification

- Removed the old `retain_and_record_callback_registration()` helper so callback retain + runtime registry record is no longer split between setCallback and child-open-specific paths.
- Added `register_callback_artifact_after_owner_ready()` as the callback artifact registration core used after the owner object has already been validated or registered.
- Kept frontend/LNB `setCallback()` on `execute_callback_registration_runtime_use_case()`: live/generation/kind check, callback artifact registration, domain runtime commit, and rollback stay in one use-case boundary.
- Routed filter/DVR child open callback retain through the same callback artifact registration core after child runtime/object registration, while preserving child-open rollback on callback failure.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to describe the setCallback and child open callback registration split without reintroducing `retain_and_record_callback_registration()`.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei86_callback_registration_and_method_execution_boundary_fix

- Made `execute_callback_registration_runtime_use_case()` the primary callback registration transaction entry for frontend/LNB setCallback: live/generation/kind check, callback store retain, runtime callback registry record, domain runtime commit, and rollback now stay in one use-case boundary.
- Removed the frontend/LNB setCallback main-path `retain_callback()` wrappers so they no longer route through `retain_and_record_callback_registration()` before entering the callback registration transaction.
- Kept `retain_and_record_callback_registration()` for child object open callback retain/record use only.
- Added a module comment to `service_runtime/src/method_execution.rs` documenting it as a runtime lock / shared runtime / query closure executor, not a transaction owner.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` so ObjectMethodTxn is limited to fallible request-builder / status-precedence-risk paths and simple methods are not forced into `ObjectMethodTxnPlan` / dispatch-preflight tokens.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei85_aidl_method_plan_coverage_test_name_alignment

- Renamed the AIDL method plan-coverage helper/test so the names describe AIDL method call variants rather than a partial method-kind sample.
- Added the missing `PublicApi`, `UnsupportedPublicApi`, `DemuxOpenFilter`, `FilterConfigure`, and `FilterConfigureAvStreamType` variants to the plan-coverage helper.
- Renamed the LNB close transaction-table test to describe the specific `CommandPlan::for_api()` mapping it verifies.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei84_command_plan_for_api_transaction_ssot

- Changed command planning to use `CommandPlan::for_api(object, api)` as the primary path so the runtime transaction name is selected only from `AIDL_TRANSACTION_TABLE`.
- Removed the caller-supplied transaction-name lookup path from Rust code; command callers now pass object/API identity rather than an expected transaction triple.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to keep transaction-name ownership in the AIDL transaction table and avoid duplicating expected transaction names at call sites.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei83_binder_adapter_aidl_method_duplicate_import_cleanup

- Removed the duplicate `HalError` import from `binder_adapter/src/aidl_method.rs` after the fallible command-plan cleanup.
- No behavior was changed in this revision.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei82_request_builder_plan_result_propagation_fix

- Fixed `aidl_service/src/object_runtime.rs::object_method_txn_plan()` to return `BinderResult<(CommandPlan, Option<RuntimeExecutableRequest>)>` after `AidlMethodAdapter::plan()` became fallible.
- Updated `execute_object_runtime_use_case_with_request_builder()`, `execute_shared_object_runtime_use_case_with_request_builder()`, and `plan_unavailable_object_method_use_case()` to propagate the fallible plan result from inside their builder closures.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei81_design_contract_cleanup_command_plan_no_expect

- Removed implementation-rule wording from `DESIGN_JA.md` around capability-token visibility and kept only the current design contract; detailed construction/visibility rules remain in `CODE_CONVENTION.md`.
- Changed AIDL command plan construction to return `Result` instead of panicking with `expect("known AIDL command plan")` in production code paths.
- Updated binder adapter command planning, AIDL method planning, and AIDL service call sites to propagate command-plan construction failures as HAL/Binder errors instead of panics.
- Kept transaction-table-backed `CommandPlan` construction and private `CommandPlan` fields.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei80_capability_token_convention_generalization

- Rewrote the capability-token visibility rule in `CODE_CONVENTION.md` as a generic rule for validation tokens, dispatch-preflight proofs, transaction plans, ledger guards, and rollback guards.
- Removed the struct-by-struct convention wording from `CODE_CONVENTION.md`; concrete component names remain in `DESIGN_JA.md` where implementation ownership is fixed.
- No code behavior was changed in this revision.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei79_capability_token_visibility_hardening

- Hardened `ObjectMethodDispatchPreflight` so the dispatch-preflight-complete proof and required policy constructors are private to `service_runtime::object_method_txn`; request-builder callers only receive the proof returned by `build_and_plan_object_method_request_after_live()`.
- Removed `Clone` from `ObjectMethodDispatchPreflight` and `ObjectMethodTxnPlan`; the dispatch proof remains consume-only via `plan(self, ...)`, and transaction plans are generated inside `object_method_txn`.
- Changed `ObjectMethodTxnPlan::new()` to private and changed the request-builder transaction closure contract to pass `CommandPlan` / `RuntimeExecutableRequest` parts into `object_method_txn`, where the plan is constructed.
- Made `CommandPlan` fields private and added accessors plus transaction-table-backed construction so inconsistent `(object, api, transaction)` triples cannot be built with public fields.
- Hardened `LnbOperationGuard` by making its fields private, removing `Clone` / `Copy`, adding an internal nonce, and requiring `LnbOperationLedger::finish()` to match both kind and nonce before clearing active state.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to fix the capability-token and transaction-plan visibility contracts.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei78_filter_datasource_dispatch_preflight_arg_fix

- Fixed `IFilter.setDataSource()` request-builder execute closure to receive the `ObjectMethodDispatchPreflight` proof returned by `service_runtime::object_method_txn` and pass that proof to `set_filter_data_source_for_object()`.
- Verified the other request-builder execute closures under `aidl_service/src` already receive and pass `dispatch_preflight` explicitly.
- Verified `aidl_service/src` still does not directly construct `already_planned()` and no dispatch-preflight-specific public entry point was reintroduced.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei77_object_method_dispatch_preflight_private_constructor

- Verified the r50ei76 dispatch-preflight policy and child rollback diagnostic changes before applying this type-boundary hardening.
- Changed `ObjectMethodDispatchPreflight` from a public enum with public `required(...)` / `already_planned()` constructors into a public wrapper over a private state enum; constructors and `plan(...)` are now service_runtime-internal.
- Changed `build_and_plan_object_method_request_after_live()` to return the dispatch-preflight proof only after live/generation/kind validation, request build, method/object kind validation, executable request validation, and dispatch preflight all succeed.
- Routed AIDL request-builder execution closures to pass through the returned dispatch-preflight proof instead of constructing an already-planned value locally.
- Removed the unused normal child-open helper paths that required AIDL to construct the required dispatch policy directly; child open remains on the request-builder object-method transaction path and does not revive dispatch-preflight-specific public entry points.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to fix the proof rule: AIDL and individual method bodies must not freely create a preflight-complete value.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei76_object_method_dispatch_policy_child_rollback_diagnostics

- Verified the r50ei75 child-open, unavailable-plan, and duplicate-dispatch fixes before applying this follow-up hardening.
- Replaced the parallel dispatch-preflight-specific public use-case family with `ObjectMethodDispatchPreflight`; normal callers pass `required(...)`, request-builder callers pass `already_planned()`, and both paths use the same `*_for_object` service_runtime entry points.
- Routed filter/DVR child open through the demux/filter/DVR transaction context again while preserving the service_runtime object-method dispatch-preflight boundary.
- Added child-open rollback diagnostics for object registration rollback failure, runtime cleanup target missing, and combined rollback failure; rollback no longer treats a missing filter/DVR runtime cleanup target as success.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to require the dispatch-preflight policy object rather than parallel after-preflight public entry points, and to require child-open rollback diagnostics.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei75_object_method_txn_child_unavailable_duplicate_dispatch_fix

- Verified the r50ei74 medium request-builder dispatch-preflight changes before applying this follow-up fix.
- Routed child `openFilter()` / `openDvr()` request-builder paths through `execute_shared_object_runtime_use_case_with_request_builder()` and service_runtime after-preflight child open entry points, so open request building, executable request validation, and dispatch preflight share the `service_runtime::object_method_txn` boundary.
- Added after-object-method-preflight service_runtime entry points for request-builder domain operations that already passed dispatch preflight, and routed LNB, filter, descrambler, frontend tune/scan, and child open request-builder closures to those entry points to avoid duplicate dispatch planning.
- Changed `plan_unavailable_object_method_use_case()` to use the build-and-plan service_runtime transaction boundary directly; it no longer falls back to the old build-only helper plus a second plan-only dispatch pass.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to require request-builder execution closures to call after-preflight service_runtime use-case entry points and to forbid duplicate dispatch planning after `ObjectMethodTxnPlan` preflight.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei74_object_method_txn_dispatch_preflight_completion

- Verified the r50ei73 `service_runtime::object_method_txn` boundary before applying the medium hardening.
- Added `ObjectMethodTxnPlan` to carry the neutral `CommandPlan` and `RuntimeExecutableRequest` pair for object method transactions.
- Added `build_and_plan_object_method_request_after_live()` so request-builder methods now perform object live/generation/kind validation, request building, executable request validation, and dispatch planning preflight in the service_runtime object method transaction boundary.
- Routed normal and shared request-builder AIDL adapters through the new build-and-plan transaction path; AIDL helpers now generate the neutral plan and delegate validation/dispatch preflight to service_runtime.
- Added service_runtime regression coverage for dispatch-preflight execution under the runtime lock, builder failure before dispatch preflight, and method/object kind mismatch rejection.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to fix `ObjectMethodTxnPlan` and dispatch-preflight ownership in `service_runtime::object_method_txn`.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei73_object_method_txn_service_runtime_boundary

- Added `service_runtime::object_method_txn` as the implementation owner for the object method request-builder lifecycle critical section.
- Moved the request-builder live/generation/kind preflight out of the private AIDL-side `ObjectMethodUseCase` phase owner and into `service_runtime::object_method_txn::build_object_method_request_after_live()`.
- Kept AIDL-side `object_runtime` helpers as adapters for AIDL method static planning, Binder status conversion, and service_runtime transaction dispatch.
- Added service_runtime regression coverage that request builders run while the runtime lock is held and that closed objects reject before invoking the builder.
- Updated `tuner_hal2/DESIGN_JA.md` to fix `service_runtime::object_method_txn` as the common-component implementation owner and updated `tuner_hal2/CODE_CONVENTION.md` to forbid AIDL-side ownership of the request-builder critical section.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei72_request_builder_lifecycle_lock_fix

- Short-term hardened the object method request-builder boundary: `ObjectMethodUseCase` now runs fallible request builders while holding the same runtime lock used for the object live/generation/kind check.
- Kept the later service_runtime dispatch/runtime operation recheck, but removed the race where a concurrent close could occur between the first lifecycle check and a failing request builder.
- Applied the same lock-held builder preflight to shared-runtime request builders and unavailable / unsupported plan-only request builders.
- Added regression coverage that a request-builder closure observes the runtime lock as held and that closed objects reject before invoking the builder.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to require request builders to run inside the lifecycle critical section and to restrict builder closures to short, side-effect-free conversion.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei71_source_boundary_close_preflight_atomicity_fix

- Verified the r50ei70 object method and source boundary updates before applying the follow-up hardening.
- Reordered `SourceBoundaryTxn` so sink endpoint, queue presence, and generation increment feasibility are validated before any source disconnect. Queue clear, generation boundary reset, and packet pipeline reset now complete before disconnecting the existing source.
- Added regression coverage that a missing sink queue does not execute the disconnect step and preserves an already connected source filter.
- Added `service_runtime::object_close_txn::plan_and_begin_object_close_method_dispatch()` and routed `close_object_after_close_preflight_with_domain_cleanup()` through it so closeable lifecycle/dispatch preflight and the `Closing` transition happen in one runtime critical section.
- Added regression coverage that the close-preflight path has already moved the object to `Closing` before the domain cleanup hook runs.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` with the source boundary atomicity and close-preflight critical-section rules.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei70_object_method_source_boundary_completion

- Verified the r50ei69 changes and kept the common-component taxonomy, callback lifetime-first behavior, and `setDataSource(source)` same-demux precedence fixes.
- Added a private `ObjectMethodUseCase` boundary in `aidl_service/src/object_runtime.rs` so normal runtime execution, shared runtime execution, query, request-builder, and unavailable paths share one object method use-case phase owner.
- Connected `DemuxRuntime::set_filter_source_non_null()` to `SourceBoundaryTxn` so source switch now goes through existing source disconnect, sink queue clear, generation boundary, and packet pipeline reset before committing the new source filter.
- Extended `SourceBoundaryTxn` to expose the `PipelineResetReport` produced by the generation boundary reset.
- Added regression coverage for `set_filter_source_non_null()` using the source boundary transaction and preserving the new source only after boundary reset.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to state the `ObjectMethodUseCase` and `SourceBoundaryTxn` ownership rules.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei69_common_component_contract_hardening

- Added common-component definition rules to `tuner_hal/DESIGN_JA.md` and `tuner_hal2/DESIGN_JA.md`: logical contract, implementation owner, owned state, phase order, failure behavior, allowed callers, forbidden callers, and minimum tests must be defined before a helper is treated as a transaction/common component.
- Classified `tuner_hal2` common structures into transaction owners, phase helpers, lock helpers, façades, and adapters so `method_execution` and thin `*_ops.rs` wrappers are not mistaken for transaction owners.
- Fixed callback registration precedence by checking object live/generation before callback retain in both immediate registration and runtime-use-case registration paths.
- Fixed `IFilter.setDataSource(source)` service_runtime precedence: sink/source lifetime and owner demux are validated before same-demux and self-source input validation, and cross-demux source filters are rejected before dispatch/commit.
- Added regression coverage for callback registration paths that must not call the callback retain closure when the target object is already closed.
- Updated `tuner_hal2/CODE_CONVENTION.md` with common-component naming, callback registration, request-builder, and `setDataSource(source)` relation-validation rules.
- WP-E packet/stream/source boundary correspondence remains a separate audit scope; no PacketPipeline / StreamBoundaryTxn / SourceBoundaryTxn implementation change is included in this revision.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei68_filter_datasource_same_demux_design_docs

- Documented the `IFilter.setDataSource(source)` same-demux ownership rule in `tuner_hal/DESIGN_JA.md` and the corresponding `tuner_hal2` service_runtime validation responsibility in `tuner_hal2/DESIGN_JA.md`.
- Recorded the checked AOSP basis: AIDL VTS `SetFilterLinkage` opens the source and sink filters from the same demux before calling `setDataSource()`, and the checked VTS path does not require a cross-demux source filter success case.
- Recorded the AOSP API boundary: `IFilter.setDataSource()` / framework `Filter.setDataSource()` allow another filter output as source and NULL/demux fallback, but do not require cross-demux filter graph ownership.
- Kept historical/VTS confirmation detail in this CHANGELOG entry only; DESIGN_JA.md entries state the current product contract and implementation responsibility without release-history wording.
- No code change was made in this revision.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei61_object_close_txn_common_regression_tests

- Added common ObjectCloseTxn regression coverage instead of an LNB-only close test.
- Changed `RuntimeObjectTable::begin_close_cascade()` to reject a second begin on the same target object while still allowing already-closing descendants during parent cascade cleanup.
- Added `object_close_txn` coverage for second-begin rejection and lifecycle preservation.
- Added `object_runtime::close_object_with_domain_cleanup()` coverage for hook-after-begin ordering, successful commit to Closed, domain cleanup failure to CleanupFailed, and rejection of a domain hook attempting a second close begin.
- Did not add an `ILnb.close()`-specific second-begin regression test because the contract is common ObjectCloseTxn behavior, not an LNB-specific behavior.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei60_lnb_close_object_close_txn_single_begin

- Reworked `ILnb.close()` to use `close_object_after_aidl_method_plan_with_domain_cleanup()` instead of running `execute_object_runtime_use_case()` before `self.close_object()`.
- Removed the LNB service_runtime-side `begin_object_close_cascade()` / cleanup-failed marking path so ObjectCloseTxn begin remains owned by `object_runtime::close_object_with_domain_cleanup()`.
- Added `close_lnb_explicit_after_object_close_begin()` as the LNB domain cleanup hook, resolving the LNB public runtime id from a nonterminal AIDL object after the generic close begin.
- Confirmed no other service_runtime close path starts `begin_object_close_cascade()` before AIDL ObjectCloseTxn finalization.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to state that LNB cleanup, like frontend cleanup, is connected as an ObjectCloseTxn domain cleanup hook.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei59_method_transaction_and_frontend_txn_surface_cleanup

- Moved `RuntimeExecutableRequest` validation out of the AIDL object executors and `service_runtime::method_transaction` executor heads to avoid bypassing AIDL status precedence.
- Kept validation in the service_runtime transaction boundary by routing it through `method_dispatch::plan_object_method_dispatch()` after object-handle use-cases perform live/generation validation and before runtime dispatch/mutation.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to state that validation must not run before object lifetime/profile precedence resolution.
- Flattened the flagged `FrontendTxn` methods so they own their operation bodies instead of forwarding the same arguments to private `transact_*` methods.
- Removed the corresponding unused private `transact_*` frontend helper methods for the flagged worker/snapshot paths.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei58_method_validation_close_hook_frontend_surface_fix

- Added `service_runtime/src/method_validation.rs` and routed object runtime executors through the shared `RuntimeExecutableRequest` profile/supported-value validation before runtime mutation.
- Reworked frontend close so `object_runtime::close_object_with_domain_cleanup()` owns the single ObjectCloseTxn begin, callback cleanup, frontend domain cleanup hook, cleanup-failed recording, and final close commit.
- Changed frontend-specific close cleanup to `cleanup_frontend_object_after_close_begin()` so service_runtime no longer begins close before the generic close transaction.
- Reduced frontend worker façade surface: public service_runtime re-export aliases remain for AIDL use-case boundaries, and same-name `TunerServiceRuntime -> FrontendTxn` delegates in `frontend_ops.rs` are crate-private.
- Removed `RuntimeObjectTable` from the service_runtime root public re-export; tests use `crate::object_table::RuntimeObjectTable` directly.
- Updated DESIGN_JA.md and CODE_CONVENTION.md for method validation and close domain cleanup hook ownership.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei57_doc_scope_object_lifecycle_frontend_wrapper_cleanup

- Re-scoped documentation responsibilities: DESIGN_JA.md remains the product/API/profile contract source, INTEGRATION.md only points product/VTS config at that contract, and CODE_CONVENTION.md remains implementation rules only.
- Added `service_runtime/src/object_lifecycle.rs` as the official service_runtime façade for AIDL object live checks and public runtime binding lookups.
- Removed production AIDL-side direct `object_table()` / `object_table_mut()` calls from `aidl_service/src/object_runtime.rs`; close commit and drop-leak quarantine now use service_runtime lifecycle/close façade helpers.
- Removed the frontend_ops one-line worker façade functions; root service_runtime re-exports the private frontend worker use-cases with public boundary names instead of maintaining same-argument wrappers.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei56_monitor_event_profile_contract_fix

- Documented the TS-only monitor event policy in DESIGN_JA.md: monitor event is not declared, `configureMonitorEvent(0)` is a supported no-op, and nonzero masks return `UNAVAILABLE`.
- Documented the VTS/product config policy in INTEGRATION.md: product config must not require nonzero monitor event unless a future WP implements and declares it.
- Fixed `IFilter.configureMonitorEvent(0)` so it uses a supported `NoPayload` no-op method plan instead of first entering the unsupported-profile status path.
- Kept nonzero `configureMonitorEvent()` routed through unsupported-profile status precedence.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei55_object_txn_lifetime_close_callback_query_fix

- Removed AIDL executor-side lifetime prechecks from `execute_object_runtime_use_case()` and `execute_shared_object_runtime_use_case()` so live/generation validation is owned by service_runtime object-handle use-case transactions.
- Removed the duplicate `begin_object_close_cascade()` from `close_object_after_aidl_method_plan()`; `close_object()` is the single ObjectCloseTxn begin/finalize entry for this path.
- Added `execute_callback_registration_runtime_use_case()` and moved `ILnb.setCallback()` retain/commit/rollback assembly out of the AIDL method body.
- Added root-open rollback for public id conversion failure after `RuntimeObjectEntry` acquisition.
- Added `execute_object_query_use_case()` and routed frontend status/readiness queries through it to remove AIDL-side `ensure_open()` + runtime query lifecycle double checks.
- Updated DESIGN_JA.md and CODE_CONVENTION.md for the narrowed transaction boundaries.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei54_lifetime_check_dedup_horizontal_fix

- Removed the remaining duplicated pre-check from `ILnb.setCallback()`: callback registration already validates the AIDL object through `record_callback_registration()`, and the runtime commit path validates again through `execute_object_runtime_use_case()`.
- Keeps unsupported/read-only/unavailable pre-checks unchanged because they are not followed by an object-runtime use-case live check.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei53_demux_child_open_lifetime_check_dedup

- Removed duplicated `ensure_open()` calls from `IDemux.openFilter()` and `IDemux.openDvr()` because the child-open common entry now routes through `execute_shared_object_runtime_use_case()`, which performs the authoritative live/generation check.
- Kept AIDL input conversion before the child-open helper because it is pure AIDL argument validation and does not depend on runtime state.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei52_demux_child_open_method_txn_alignment

- Routed `IDemux.setFrontendDataSource()` through `execute_object_runtime_use_case()` instead of hand-assembling `ensure_open()` / `AidlMethodAdapter::plan()` / runtime lock / service_runtime commit in the AIDL method body.
- Routed demux child-open runtime allocation in `child_object_open.rs` through `execute_shared_object_runtime_use_case()` so child open uses the declared AIDL object method planning boundary rather than local `AidlMethodAdapter::plan()` calls.
- Kept unsupported/read-only/unavailable `plan_method()` paths unchanged because they do not perform service_runtime state-changing commits.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei51_callback_close_dispatch_common_boundary_fix

- Routed frontend/LNB/child callback retain + runtime registration through `retain_and_record_callback_registration()` so registration failure rolls back retained callback store entries.
- Changed public close callback cleanup to use `clear_owner_callback_registration()` outside the runtime lock and preserve callback-registry unhealthy marking on callback store cleanup failure.
- Removed unused raw close wrappers from `TunerServiceRuntime` (`begin_aidl_object_close`, `mark_aidl_object_cleanup_failed`, `commit_aidl_object_close`).
- Added `aidl_service::object_runtime::{execute_object_runtime_use_case, execute_shared_object_runtime_use_case}` and routed frontend/LNB/filter/descrambler state-changing AIDL methods through them.
- Added `service_runtime::method_dispatch::plan_object_method_dispatch()` and routed root/frontend/demux/filter/DVR/descrambler/LNB dispatch planning through it, including frontend `setLnb`, so dispatch errors use the shared mapper.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to document callback registration and method dispatch common boundaries.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei50_object_close_txn_and_dead_common_cleanup

- Added `service_runtime::object_close_txn` as the concrete ObjectCloseTxn common component and routed AIDL close, frontend close, and LNB close begin/cleanup-failed marking through it.
- Routed LNB root-open runtime-open failure rollback through `finish_open_rollback()` instead of hand-written early-return rollback.
- Kept `transaction_registry.rs` as the useful runtime transaction -> dispatch target table, and removed the non-production second handler/status layer (`runtime_handlers.rs` / `runtime_result.rs`).
- Removed unused public AIDL helper modules/re-exports: `CallbackBridge`, `AidlCallbackSlot`, `NativeHandleBridge`, and `AidlErrorBridge` while keeping the production `error_bridge` status functions.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to document the useful registry and prohibit dead public bridge/helper common parts.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei49_open_rollback_completion_common_helper

- Added `service_runtime::open_rollback::finish_open_rollback()` as the shared rollback-completion helper for root/child object open transactions.
- Routed root object open rollback and child filter/DVR open rollback through the shared helper, so object-table rollback failure can no longer skip runtime unregister / LNB close.
- Classified dual object-rollback/runtime-cleanup failure as `CleanupFailed` through the shared helper.
- Documented the open rollback completion helper in the DESIGN_JA.md common-component catalogue and CODE_CONVENTION.md.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei48_object_table_error_mapping_and_rollback_completion

- Removed the duplicate `object_table_hal_error()` mapper from `aidl_service::object_runtime` and routed AIDL object lifecycle code through `service_runtime::error_mapping::object_table_error_to_hal()`.
- Fixed the `RuntimeObjectTableError::GenerationOverflow` mapping inconsistency: the shared mapper now treats generation overflow as internal counter exhaustion instead of `INVALID_STATE`.
- Expanded object-table error messages so missing object, kind mismatch, generation mismatch, owner mismatch, duplicate binding, unsupported kind, and generation overflow no longer collapse to one generic context string.
- Changed root object open rollback and child filter/DVR open rollback so runtime unregister / LNB close is attempted even when object-table rollback fails.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to require shared object-table error mapping and rollback cleanup continuation.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei47_typed_error_mapping_horizontal_fix

- Added `service_runtime::error_mapping` as the shared location for typed service_runtime error enum to `HalError` mapping.
- Routed root object registration, child filter/DVR object registration, registry allocation/commit, and runtime dispatch planning errors through shared typed mapping helpers instead of local `_ => Internal` collapse.
- Mapped registry duplicate errors to `InvalidState`, missing/mismatch registry errors to invalid input, and runtime id exhaustion to `Internal`.
- Kept command dispatch failures on the existing `RuntimeCommandDispatchError::into_hal_error()` mapping instead of manually recreating internal errors at each call site.
- Documented the shared typed-error mapping boundary in DESIGN_JA.md and CODE_CONVENTION.md.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei46_root_object_registration_status_mapping

- Fixed `service_runtime::root_object_ops::register_root_object()` to preserve `RuntimeObjectTableError` classification instead of collapsing all object table registration failures to `HalError::Internal`.
- Mapped duplicate object id / duplicate runtime binding / lifecycle / owner / kind mismatch root registration failures to `HalError::InvalidState`, preventing avoidable `UNKNOWN_ERROR` responses through binder status mapping.
- Kept root object generation overflow as `HalError::Internal` because it represents internal counter exhaustion, not client-visible object lifecycle conflict.
- Added unit tests for duplicate runtime binding and generation overflow status classification.
- Documented the root object open transaction status-mapping boundary in DESIGN_JA.md and CODE_CONVENTION.md.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei45_root_open_and_tuner_query_boundary

- Confirmed that the RuntimeQuery / query façade rule is not excessive: it fixes ownership of read-only projections and prevents AIDL from growing registry-specific dependencies.
- Added the existing/root object open transaction boundary to the common-component catalogue in DESIGN_JA.md and CODE_CONVENTION.md.
- Moved ITuner root open runtime allocation, availability check, AIDL object table registration, LNB runtime open, and rollback into `service_runtime::root_object_ops`.
- Updated ITuner root open methods to create typed Binder objects from returned runtime entries and call service_runtime rollback helper on Binder object construction failure.
- Kept pure read-only query calls routed through existing `TunerServiceRuntime` query wrappers; no AIDL direct registry/object-table access was added.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei44_filter_config_and_source_boundary_txn_completion

- Completed another ObjectMethodTxn horizontal cleanup for implemented filter mutation paths.
- Moved `IFilter.configure()` current-open-type lookup and config construction into a service_runtime object-handle use-case closure boundary so AIDL no longer resolves runtime filter open type before commit.
- Moved `IFilter.setDataSource()` self-source rejection into service_runtime `set_filter_data_source_for_object()` so source/sink handle validation and commit remain in the same method transaction boundary.
- Removed unused AIDL-side query helpers that had become stale after the RuntimeQuery / object-handle use-case migration.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei43_child_open_runtime_use_case_completion

- Moved child filter/DVR runtime allocation, runtime registration, and AIDL object table registration from `child_object_open.rs` into service_runtime object-handle child-open use-case helpers.
- `child_object_open.rs` now calls `open_filter_child_runtime_for_demux_object()` / `open_dvr_child_runtime_for_demux_object()` and performs only typed Binder object construction plus typed callback retain/rollback.
- Added service_runtime rollback helpers for child-open Binder/callback failure so AIDL does not manually compose child object unregister and public runtime unregister.
- Updated DESIGN_JA.md to describe the child-open helper as a service_runtime-backed object-handle use-case boundary.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei42_child_open_and_close_boundary_completion

- Moved `openFilter()` / `openDvr()` method planning into the existing `child_object_open.rs` common helper so the AIDL method body no longer performs `plan_method()` before child allocation/registration.
- Renamed child-open helpers to `open_filter_child_for_owner_object()` and `open_dvr_child_for_owner_object()` to reflect that they plan and resolve the owner object internally.
- Strengthened `close_object_after_aidl_method_plan()` so close method planning and `begin_close_cascade()` run under one runtime lock before callback cleanup / final close processing.
- Updated DESIGN_JA.md to reference the renamed child-open common helpers.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei41_existing_common_txn_usage_fix

- Updated `openFilter()` / `openDvr()` to stop resolving owner demux public id in the AIDL method body; `child_object_open.rs` now resolves the owner through the runtime query façade.
- Added object-handle based LNB callback registration commit path and routed `ILnb.setCallback()` through it.
- Strengthened `close_lnb_explicit_for_object()` so `ILnb.close()` moves the object cascade to `Closing` before LNB runtime cleanup and leaves final object close to the common close helper.
- Removed stale AIDL-side `runtime_entry_public_id` use from demux child-open and LNB callback registration paths.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei40_existing_common_txn_boundary_doc

- Merged existing but under-documented common components into the AIDL object lifecycle common-component table instead of adding a separate ad hoc section.
- Documented `aidl_service/src/child_object_open.rs` as the common child filter/DVR open transaction entry.
- Documented callback registration helpers (`record_callback_registration`, callback-store retain/rollback, `clear_owner_callback_registration`) as the registration-side counterpart of callback cleanup.
- Documented `execute_object_aidl_method` / `plan_object_aidl_method` and `close_object_after_aidl_method_plan` as existing object method planning / close helper boundaries.
- No implementation code was changed in this revision. Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei39_object_method_txn_close_boundary

- Added object-handle based service_runtime method transaction helpers for demux/filter/descrambler/LNB mutation paths, moving object live/generation resolution and dispatch planning out of AIDL method bodies.
- Strengthened frontend close begin semantics by marking the frontend object cascade Closing before owner-loss worker/live-data cleanup and marking cleanup failed on runtime cleanup failure.
- Made runtime object close cascade tolerate already-Closing entries so begin-close and finalize-close phases can be split safely.
- Updated LNB owner-loss callback cleanup lookup to find nonterminal objects, including Closing entries created by frontend close begin.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to describe `ObjectMethodTxn` and minimal `ObjectCloseTxn` boundaries across all domains.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei38_aidl_query_boundary_frontend_method_txn

- Moved remaining frontend read-only registry access behind `RuntimeQuery` / query façade methods; `boot.rs` frontend query wrappers now delegate through `self.query()`.
- Added single-lock frontend status query façade for `getStatus()` / `getFrontendStatusReadiness()` so frontend entry, runtime state, and signal state are read from one runtime snapshot.
- Converted frontend mutation AIDL methods (`tune`, `scan`, `stopTune`, `stopScan`, `close`, `setLnb`) to call object-handle based service_runtime use-case façades instead of assembling live/id/validate/plan/commit steps in AIDL.
- Moved frontend method planning, object live/generation resolution, request validation, and worker/session state reservation into service_runtime frontend use-case transaction boundaries.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` with the AIDL method transaction boundary and query façade requirements.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei37_aidl_boundary_public_planning

- Removed AIDL-layer direct imports of `service_runtime::frontend_worker_txn`; AIDL now calls service_runtime public frontend use-case façade functions.
- Made `service_runtime::frontend_worker_txn` crate-private and re-exported only the public frontend use-case boundary and scan-end notifier type.
- Moved AIDL object handle/public runtime id lookup behind `TunerServiceRuntime::public_*_for_aidl_object()` query methods so AIDL helpers no longer call `object_table()` directly.
- Added `AidlMethodCall::PublicApi` / `DomainCommand::PublicApi` and `Tuner/Frontend/DemuxPublicApiTxn` to separate supported public API planning from unsupported-by-design planning.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` with the AIDL/service_runtime boundary rules for worker use-cases, object query façade, and PublicApi vs UnsupportedPublicApi planning.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei36_registry_mut_boundary_doc_fix

- Adopted the documented registry mutation boundary that allows `TunerServiceRuntime::registry_mut()` only inside `service_runtime/src/boot/*_txn.rs` domain transaction implementations and tests.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` so the prohibition targets production code outside the boot transaction subtree, not the transaction context itself.
- Clarified `RuntimeQuery<'a>` documentation to say it holds only required immutable read-only sources; current implementation holds the runtime registry only.
- Corrected the r50ei35 changelog wording for `filter_open_type`: the AIDL-visible public/type-boundary wrapper is intentionally retained.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei35_query_wrapper_pruning

- Audited `TunerServiceRuntime` read-only query wrappers and moved service_runtime-internal call sites to `runtime.query().*`.
- Removed internal-only `TunerServiceRuntime` query wrappers for frontend snapshots, demux snapshots, same-tune checks, live-reader descriptors, frontend terminal events, and frontend demux sink readiness.
- Removed unused `RuntimeQuery` methods for frontend worker running generation. Kept `filter_open_type` because it is still used through the AIDL-visible public/type-boundary wrapper.
- Kept externally visible query wrappers used by `aidl_service` as public API/type boundaries.
- Updated `CODE_CONVENTION.md` to require existing wrapper audits and to prefer `runtime.query().*` for crate-internal read-only queries.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei34_transact_visibility_wrapper_rules

- Narrowed flat `transact_*` helper visibility inside `service_runtime/src/boot/*_txn.rs` from `pub(crate)` to private `fn`; domain transaction context methods remain the crate-visible boundary.
- Removed duplicate LNB registry lookup in `ServiceRuntimeLnbProfileAdapter::apply_lnb_state()`.
- Renamed the used `_state` parameter in `ServiceRuntimeLnbProfileAdapter::apply_lnb_state()` to `state`.
- Added explicit wrapper creation criteria and static-check positioning to `CODE_CONVENTION.md`.
- Added static checks for crate-visible flat `transact_*` helpers, top-level direct `transact_*` calls, adapter `_state` misuse, and wrapper-boundary conventions.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei33_lnb_callback_store_adapter_rename

- Renamed `ServiceRuntimeLnbProfileBackend` to `ServiceRuntimeLnbProfileAdapter` to reflect that it adapts service_runtime registry/profile state to `LnbBackendOps` rather than performing real backend I/O.
- Renamed `mark_lnb_callback_registered()` to `commit_lnb_callback_registration()` across service_runtime and AIDL LNB method wiring.
- Changed LNB callback registration state update to follow `clone -> mutate -> store_lnb_runtime()` instead of mutating the registry slot directly.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` with the LNB adapter naming and LNB runtime mutation convention.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei32_lnb_txn_context_registry_mut_narrowing

- Added `LnbTxn<'a>` in `service_runtime/src/boot/lnb_txn.rs` and moved LNB binding, apply, lifecycle, callback registration, and drop-leak state mutations behind that boot child transaction context.
- Replaced top-level LNB transaction implementation files with `service_runtime/src/lnb_ops.rs` public wrappers that call `LnbTxn<'a>` methods.
- Removed production `registry_mut()` call sites outside the `boot` transaction subtree and made `TunerServiceRuntime::registry_mut()` private to `boot` and child modules.
- Added `registry_mut_for_test()` for test fixture setup only.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to include LNB transaction context ownership, flat `transact_*` helper boundaries, one-line wrapper limits, and `registry_mut()` production-use prohibition.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei31_runtime_query_txn_context_facade

- Added `RuntimeQuery<'a>` in `service_runtime/src/boot/query_api.rs`; existing `TunerServiceRuntime` read-only query methods now delegate through `self.query()`.
- Added domain transaction context facades in `boot/*_txn.rs`: `FrontendTxn<'a>`, `DemuxFilterDvrTxn<'a>`, `DescramblerTxn<'a>`, and `PacketTxn<'a>`.
- Updated top-level `service_runtime/src/*_ops.rs` wrappers to call domain transaction context methods instead of directly calling flat `transact_*` helpers.
- Updated `DESIGN_JA.md` section 6.2 to describe `RuntimeQuery<'a>` and domain transaction contexts as the current service_runtime boundary.
- Flat `transact_*` helpers remain inside `boot/*_txn.rs` as implementation helpers in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei30_design_boundary_clarification

- Reworked `DESIGN_JA.md` section 6.2 into explicit responsibility subsections for `boot.rs`, top-level `*_ops.rs`, `boot/*_txn.rs`, `query_api.rs`, and prohibitions.
- Removed the stale wording that regular operations live under `service_runtime/src/boot/*.rs`; the current contract places public wrappers in top-level `service_runtime/src/*_ops.rs` and state-changing implementations in `service_runtime/src/boot/*_txn.rs`.
- Clarified that mutating `transact_*` calls are limited to top-level `*_ops.rs` and `boot/*_txn.rs`, and that `query_api.rs` remains read-only.
- Added static checks for `transact_*` caller locations and the `query_api.rs` no-mutating-transaction boundary.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei29_transaction_boundary_final_audit

- Re-audited the service_runtime top-level operation wrapper and boot transaction owner boundary after the additional demux/filter/DVR, descrambler, and frontend helper wrapper split work.
- Confirmed top-level `service_runtime/src/*_ops.rs` files remain wrapper-only and do not directly access `TunerServiceRuntime` private fields.
- Confirmed `service_runtime/src/boot/*_txn.rs` state-changing public surface is reduced to `transact_*` methods, with `map_filter_runtime_error` kept as a shared non-mutating mapper.
- Narrowed `map_filter_runtime_error` visibility from `pub(crate)` to `pub(super)` because it is only shared inside the `boot` module subtree.
- Generated final static checks and an external final audit report for the transaction boundary.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei28_frontend_helper_txn_wrappers

- Extended the top-level frontend operation wrapper split to runtime snapshot restore, tune commit, signal/live-pump reporting, live-reader lifecycle, scan session, and frontend failure helper methods.
- Added `transact_*` transaction owner methods for those frontend helper operations in `boot/frontend_txn.rs`.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/frontend_txn.rs`; top-level `service_runtime/src/frontend_ops.rs` remains wrapper-only.
- Updated `DESIGN_JA.md` to describe the expanded frontend wrapper boundary as current structure.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei27_descrambler_allocation_txn_wrapper

- Extended the top-level descrambler operation wrapper split to `allocate_descrambler_runtime`.
- Added `transact_allocate_descrambler_runtime` in `boot/descrambler_txn.rs` and kept registry allocation there.
- Kept top-level `service_runtime/src/descrambler_ops.rs` wrapper-only and free of `TunerServiceRuntime` private field access.
- Updated `DESIGN_JA.md` to describe the expanded descrambler wrapper boundary as current structure.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei26_demux_filter_dvr_allocation_txn_wrappers

- Extended the top-level demux/filter/DVR operation wrapper split to allocation, unregister, AV stream type, and delay hint methods.
- Added `transact_allocate_demux_runtime`, `transact_unregister_demux_runtime`, `transact_allocate_filter_runtime`, `transact_unregister_filter_runtime`, `transact_configure_filter_av_stream_type_request`, `transact_set_filter_delay_hint_request`, `transact_allocate_dvr_runtime`, and `transact_unregister_dvr_runtime` transaction owner methods in `boot/demux_filter_dvr_txn.rs`.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/demux_filter_dvr_txn.rs`; top-level `service_runtime/src/demux_filter_dvr_ops.rs` remains wrapper-only.
- Updated `DESIGN_JA.md` to describe the expanded demux/filter/DVR wrapper boundary as current structure.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei25_top_level_transaction_audit

- Audited the service_runtime top-level operation wrapper boundary after frontend, demux/filter/DVR, descrambler, and packet wrapper split work.
- Confirmed top-level `service_runtime/src/*_ops.rs` files do not directly access `TunerServiceRuntime` private fields and call transaction/query APIs only.
- Clarified `DESIGN_JA.md` current contract: private field access is confined to `service_runtime/src/boot/query_api.rs` and `service_runtime/src/boot/*_txn.rs`, while top-level operation files remain wrapper-only.
- Generated static checks and an external audit report for the remaining boot child transaction owner methods, including allocation/unregister/helper methods that still intentionally own registry mutation.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei24_packet_top_level_wrappers

- Added true top-level `service_runtime/src/packet_ops.rs` for packet ingress and demux-binding public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/packet_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `set_demux_frontend_data_source`, `reset_bound_demuxes_for_frontend_tune_start`, `reset_and_unbind_bound_demuxes_for_frontend`, `quarantine_frontend_and_bound_demuxes`, and `push_frontend_ts_packet_to_bound_demuxes` wrappers out of `boot/packet_txn.rs`.
- Moved read-only `ensure_frontend_demux_sink_ready` into `boot/query_api.rs` to keep packet top-level wrappers free of private field access.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level packet operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei23_descrambler_top_level_wrappers

- Added true top-level `service_runtime/src/descrambler_ops.rs` for descrambler public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/descrambler_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `set_descrambler_demux_source`, `set_descrambler_key_token`, `add_descrambler_pid_non_null_source`, `remove_descrambler_pid_non_null_source`, and `unregister_descrambler_runtime` wrappers out of `boot/descrambler_txn.rs`.
- Moved the crate-visible `cleanup_descramblers_for_demux_owner_loss` wrapper to the top-level descrambler operation file while leaving owner-loss cleanup implementation in the transaction owner.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level descrambler operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Allocation and internal cleanup helpers still live in `boot/descrambler_txn.rs` because they directly own registry/session mutation.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei22_demux_filter_dvr_top_level_wrappers

- Added true top-level `service_runtime/src/demux_filter_dvr_ops.rs` for demux/filter/DVR public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/demux_filter_dvr_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `register_demux_filter_runtime`, `configure_filter_runtime_request`, `start_filter_runtime`, `stop_filter_runtime`, `flush_filter_runtime`, `set_filter_data_source_non_null`, and `register_demux_dvr_runtime` wrappers out of `boot/demux_filter_dvr_txn.rs`.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level demux/filter/DVR operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Only demux/filter/DVR methods already backed by explicit `transact_*` APIs were top-levelized in this release; allocation/unregister helpers still live in `boot/demux_filter_dvr_txn.rs`.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei21_frontend_top_level_worker_wrappers

- Added true top-level `service_runtime/src/frontend_ops.rs` for frontend worker lifecycle public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/frontend_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `start_frontend_worker`, `request_frontend_worker_stop`, `request_frontend_worker_stop_and_join`, and `clear_finished_frontend_workers` wrappers out of `boot/frontend_txn.rs`.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level frontend operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Only frontend worker lifecycle wrappers were top-levelized in this release; remaining frontend mutation public methods still live in `boot/frontend_txn.rs`.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei20_packet_txn_boundary

- Renamed `service_runtime/src/boot/packet_ops.rs` to `service_runtime/src/boot/packet_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public packet/demux-binding runtime method names stable while placing packet ingress and stream-boundary implementation under the transaction owner file.
- Added explicit `transact_set_demux_frontend_data_source`, `transact_reset_bound_demuxes_for_frontend_tune_start`, `transact_reset_and_unbind_bound_demuxes_for_frontend`, `transact_quarantine_frontend_and_bound_demuxes`, and `transact_push_frontend_ts_packet_to_bound_demuxes` internal transaction methods.
- Preserved existing `GenerationBoundaryTxn` usage inside the service_runtime transaction boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining packet helper internals still directly access `self.registry` / diagnostics inside `packet_txn.rs`; allocation/unregister helper cleanup consolidation is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei19_descrambler_txn_boundary

- Renamed `service_runtime/src/boot/descrambler_ops.rs` to `service_runtime/src/boot/descrambler_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public descrambler runtime method names stable while placing descrambler state-changing implementation under the transaction owner file.
- Added explicit `transact_set_descrambler_demux_source`, `transact_set_descrambler_key_token`, `transact_add_descrambler_pid_non_null_source`, `transact_remove_descrambler_pid_non_null_source`, `transact_unregister_descrambler_runtime`, and `transact_cleanup_descramblers_for_demux_owner_loss` internal transaction methods.
- Preserved existing `DescramblerSessionTxn` usage inside the service_runtime transaction boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining descrambler allocation/helper/cleanup internals still directly access `self.registry` inside `descrambler_txn.rs`; packet mutation transaction API work is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei18_demux_filter_dvr_txn_boundary

- Renamed `service_runtime/src/boot/demux_filter_dvr_ops.rs` to `service_runtime/src/boot/demux_filter_dvr_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public demux/filter/DVR runtime method names stable while placing demux/filter/DVR state-changing implementation under the transaction owner file.
- Added explicit `transact_register_demux_filter_runtime`, `transact_configure_filter_runtime_request`, `transact_start_filter_runtime`, `transact_stop_filter_runtime`, `transact_flush_filter_runtime`, `transact_set_filter_data_source_non_null`, and `transact_register_demux_dvr_runtime` internal transaction methods.
- Preserved existing `FilterConfigureTxn` usage inside the service_runtime transaction boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining demux/filter/DVR allocation/unregister paths still directly access `self.registry` inside `demux_filter_dvr_txn.rs`; descrambler and packet mutation transaction API work is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei17_frontend_txn_boundary

- Renamed `service_runtime/src/boot/frontend_ops.rs` to `service_runtime/src/boot/frontend_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public frontend runtime method names stable while placing frontend state-changing implementation under the frontend transaction owner file.
- Added explicit `transact_frontend_worker_start`, `transact_frontend_worker_stop`, `transact_frontend_worker_stop_and_join`, and `transact_clear_finished_frontend_workers` internal transaction methods for worker lifecycle operations.
- Moved remaining read-only frontend descriptor/event query methods to `query_api.rs`.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining frontend runtime mutation methods still directly access `self.registry` inside `frontend_txn.rs`; demux/filter/DVR, descrambler, and packet mutation transaction API work is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei16_query_api_design_cleanup

- Revised `DESIGN_JA.md` section 6.2 to remove staged wording such as future/next-step/incomplete-state descriptions and keep only the current service_runtime structure contract.
- Added `service_runtime/src/boot/query_api.rs` as the owner of read-only runtime query methods.
- Moved read-only query methods from `frontend_ops.rs`, `demux_filter_dvr_ops.rs`, and `packet_ops.rs` into `query_api.rs`.
- Updated `Android.bp` to include `service_runtime/src/boot/query_api.rs` in the service_runtime source lists.
- Kept `TunerServiceRuntime` fields private and did not widen them to `pub(crate)` for operation modules.
- Mutation transaction API implementation is not completed in this release; remaining direct registry/worker mutation paths stay in the domain operation modules.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei15_boot_child_normal_mod_layout

- Removed `#[path]` from `service_runtime/src/boot.rs` operation module declarations.
- Removed the remaining demux parser `#[path]` declarations by making `demux/src/parser/mod.rs` own the parser child modules and re-exporting them from `demux/src/lib.rs`.
- Moved service_runtime operation files back under normal Rust child-module layout:
  - `service_runtime/src/boot/frontend_ops.rs`
  - `service_runtime/src/boot/demux_filter_dvr_ops.rs`
  - `service_runtime/src/boot/descrambler_ops.rs`
  - `service_runtime/src/boot/packet_ops.rs`
- Updated `Android.bp` source lists to the `service_runtime/src/boot/*.rs` paths.
- Updated `DESIGN_JA.md` to define this as the current supported layout: ops remain `boot` child modules, no `#[path]`, no `include!`, and no `include_str!`.
- Documented the next-stage rule: true `service_runtime` top-level module migration requires transaction API design first, and must not be achieved by widening `TunerServiceRuntime` fields to `pub(crate)`.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei14_import_doc_test_cleanup

- Moved `GLOBAL_CODE_CONVENTION.md` production `use super::*;` ban into the Rust section instead of leaving it under the Kotlin heading.
- Clarified `service_runtime/src/*_ops.rs` ownership in `DESIGN_JA.md`: files are top-level, modules remain `boot` children via `#[path]` to avoid widening `TunerServiceRuntime` field visibility.
- Added an explanatory comment beside the service_runtime ops module declarations in `boot.rs`.
- Reduced r50ei13 broad explicit imports in AIDL method files and service_runtime ops files to usage-oriented explicit imports.
- Extended drop leak unit tests with a test-only callback-store marker so `drop_leak_object()` now verifies callback store clearing as well as runtime callback registry and quarantine state.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei13_explicit_import_split_drop_leak_tests

- `GLOBAL_CODE_CONVENTION.md` に production code の `use super::*;` 禁止を追加した。
- `aidl_service/src/tuner_service/support.rs` を追加し、AIDL object lookup / unsupported planning / source filter handle helper を `tuner_service.rs` から分離した。
- `aidl_service/src/tuner_service/*_methods.rs` の `use super::*;` を明示 import へ置換した。
- service_runtime operation files を `service_runtime/src/*_ops.rs` へ移し、`DESIGN_JA.md` に top-level ops file 境界を明文化した。
- `service_runtime/src/*_ops.rs` の `use super::*;` を明示 import へ置換した。
- `drop_leak_object()` の通常 quarantine / callback registry clear と、domain drop record failure 時の callback unhealthy 化を unit test で固定した。
- build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ei12_structure_cleanup_status_worker_guard

- Split child AIDL trait implementations out of `aidl_service/src/tuner_service.rs` into normal Rust modules under `aidl_service/src/tuner_service/`; no `include!` split is used.
- Split large `service_runtime/src/boot.rs` operation groups into normal Rust modules under `service_runtime/src/boot/` for frontend, demux/filter/DVR, descrambler, and packet ingress operations.
- Documented the tuner_hal2 file ownership boundaries in `DESIGN_JA.md`, including the explicit `include!` ban for structural splitting.
- Replaced callback cleanup best-effort/drop-specific paths with common `drop_leak_object()` plus `DropLeakDomainAction`; LNB Drop keeps only the domain leak marker hook and no longer owns bespoke cleanup flow.
- Added owner-wide callback unhealthy marking to `RuntimeCallbackRegistry` and tests for that registry behavior.
- Changed frontend worker cancel-reason poisoning to return `HalError`/`StopRequestFailed` instead of normalizing to `None`; added a poison-lock unit test.
- Centralized Binder status construction in `aidl_service::error_bridge`; direct `Status::new_service_specific_error()` calls are limited to that file.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment. `rustfmt` and `rustc` binaries were not available in the container.

# r50ei11_origs215_373_expired_token_resolution

- ORIG-215 / ORIG-373 token resolution fix candidate.
- Added `DescramblerKeyTable::has_token_resolution_state()` so expired-token tombstones used by test/fake-token paths are not hidden behind the CAS-token-producer-unavailable branch.
- Mapped expired descrambler key tokens to `INVALID_STATE` consistently for direct key lookup and session-token resolution.
- Preserves CAS producer unavailable for the no-token-state case; no real CAS HAL implementation is added.
- Build / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei10_qg04_quarantine_api_design_guard

- QG-04: Hid the single-object quarantine transition behind a private `RuntimeObjectTable` helper and kept `quarantine_cascade()` as the public object lifecycle entry point.
- QG-04: Added a shared AIDL runtime unregister helper used by both explicit close and Drop-leak quarantine paths.
- QG-04: Added `tuner_hal2/DESIGN_JA.md` section 5 as a structure-difference mapping to the existing `tuner_hal/DESIGN_JA.md` close / Drop leak / quarantine contract. This does not add a new design contract.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei9_qg03_qg04_drop_quarantine_transaction_gate

- QG-03: Added Drop leak quarantine handling to Frontend/Demux/Filter/Dvr/Descrambler AIDL objects instead of leaving live object-table entries behind. LNB keeps its LNB-runtime leak record path and then uses the same common object quarantine helper.
- QG-03: Changed Drop leak handling to call a shared `quarantine_live_aidl_object_after_drop_leak()` helper; this is a common helper call, not copy-pasted cleanup logic.
- QG-04: Added `RuntimeObjectTable::quarantine_cascade()` so Drop leak terminalization covers the owner and descendants as one object-table transition.
- QG-04: Added object-table tests that quarantine-cascade terminalizes descendants and permits later rebinding of the same runtime id after quarantine.
- QG-04 scope in this release is AIDL object lifecycle / Drop leak transaction integrity. Broader Demux/Filter/Dvr data-path rollback auditing remains a follow-up quality gate.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei8_qg01_qg02_lnb_profile_backend_callback_cleanup

- QG-01: Renamed the service-runtime LNB adapter to `ServiceRuntimeLnbProfileBackend` and fixed the responsibility boundary as profile validation/backend policy rather than real hardware LNB control.
- QG-01: Kept DiSEqC as explicit validated-then-unsupported profile behavior for current exported LNB profiles; no success no-op path is introduced.
- QG-02: Removed callback-store cleanup from `LnbBackendOps`; callback object cleanup remains owned by AIDL callback store/runtime callback registry.
- QG-02: `LnbLifecycleTxn` now records `ClearRuntimeCallbackState` and only clears `LnbRuntime` callback ownership state; backend callback cleanup is no longer modeled as a hardware/profile backend operation.
- Added/updated LNB lifecycle tests so public close clears runtime callback state while Drop leak still avoids normal backend cleanup.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei7_wp_r11_lnb_completion_quality_gate

- WP-R11 LNB residual completion candidate.
- Routed `ILnb.sendDiseqcMessage()` through LNB runtime/backend validation instead of a direct ad-hoc reject. Current exported profiles reject DiSEqC as unsupported after payload validation; no silent success path is added.
- Strengthened LNB profile backend validation so voltage/tone/position applies are checked by both service-runtime profile validation and the backend adapter.
- Quarantined live LNB AIDL object entries on Rust Drop leak after recording the LNB runtime leak; Drop still does not perform normal safe-state backend cleanup.
- Added LNB unit tests for DiSEqC payload validation/profile rejection and fixed-profile voltage rejection.
- Build / rust unit / atest / VTS / device validation: not executed in this environment.

# r50ei5_wp_r07a_descrambler_prereq_packet_pipeline_min

- WP-R10 descrambler / CAS token / MULTI2 packet path の前提として、keyなし scrambled TS packet を record/raw path と section/PES/AV assembly path で分離した。
- keyなし scrambled packet は record path ではTS scrambling metadataだけを観測し、PES/SC/PTS payload metadataを意味値として扱わないようにした。
- keyなし scrambled packet が section/PES/AV assemblyへ入る場合は diagnostic を出し、該当PIDの partial assembly をresetするようにした。
- TEI / duplicate continuity counter packet は record/raw TS path へ到達させ、section/PES/AV assembly からだけ除外するように補正した。
- demux unit testに、`flush()` がPES partial assemblyを消しつつruntime状態/queueを維持すること、`remove_filter()` がqueueとpacket pipelineのpartial stateを破棄することを追加した。
- release marker を tuner_hal2 / tuner_hal ともに r50ei5_wp_r07a_descrambler_prereq_packet_pipeline_min へ更新した。
- packet pipeline の診断モデルを、packet reject/drop と assembly suppression に分離した。
- `plan_and_assemble_ts_packet_report()` の no-preflight 入口と、旧意味論の test helper `accept_ts_packet_with_outcome()` / `accept_ts_packet()` を削除した。
- descrambler / CAS token / MULTI2 / key table / IDescrambler public API は未実装のまま維持した。

# r50ei_wp_r07_filter_hint_runtime

- WP-R07継続として、`IFilter.configureAvStreamType()` を service_runtime / demux runtime のAV stream type hint保持へ接続した。
- `configureAvStreamType()` は、未configure AVを `INVALID_STATE`、AV開始中を `INVALID_STATE`、非AVを `UNAVAILABLE`、open subtypeと異なるaudio/video指定を `INVALID_ARGUMENT` に倒す。
- `IFilter.setDelayHint()` を runtime保持へ接続し、旧 `tuner_hal` と同じくmedia filterは `UNAVAILABLE`、record filterのdata-size hintは `INVALID_ARGUMENT`、time hint上限は10秒にした。
- `configureMonitorEvent()` / `configureIpCid()` / 読み取り系を含む `IFilter` 公開APIに `ensure_open()` を追加し、閉鎖後公開API成功を避けるようにした。
- Filter close時のruntime unregisterで、queue除去に加えてpacket pipeline側のfilter一過性状態も破棄するようにした。
- `FilterDelayHint` のtime/data併用時にOR条件でready判定する純粋部品を追加した。
- demux runtimeの単体テストに、AV configure時のbacking marker、AV stream hintの保持/再configure時clear、delay hintのtime/data独立更新、delay OR ready判定を追加した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eh_wp_r07_filter_runtime_foundation

- WP-R07の先行実装として、公開 `IFilter.configure()` を `FilterConfigureTxn` 経由で demux runtime / packet pipeline へ接続した。
- filter configure commit時に、runtimeのqueue markerと実queueを同期し、再configure時に旧queue cleanupが失敗しないようにした。
- `IFilter.start()` / `stop()` / `flush()` を filter runtime state と packet pipeline clear/flush境界へ接続し、未configure状態は `INVALID_STATE` へ倒すようにした。
- `IFilter.getId()` / `getId64Bit()` はAIDL object idではなく公開filter runtime idを返すよう修正した。
- `child_object_open.rs` への分離で再発していた Filter/DVR callback保持rollback失敗の黙殺を修正した。
- `m maleicacid_tuner_hal2_device_test maleicacid_tuner_hal2_service_runtime_test maleicacid_tuner_hal2_aidl_service_test` は成功。atest、VTS、実機確認は未実施。

# r50eh_tuner_service_trait_shim_refactor

- 非機能リファクタとして、`tuner_service.rs` に集まっていた AIDL frontend settings 変換を `binder_adapter/src/aidl_frontend_settings.rs` へ分離した。
- frontend request の registry / backend validation を `service_runtime/src/frontend_request_txn.rs` へ分離した。
- frontend worker generation / snapshot / rollback / scan session / live descriptor 処理を `service_runtime/src/frontend_worker_txn.rs` へ分離した。
- scan END callback配送を `aidl_service/src/frontend_callback_delivery.rs` へ分離し、Binder callback型を service_runtime へ持ち込まない構造へ寄せた。
- openFilter/openDvr の child object生成・callback保持・rollback を `aidl_service/src/child_object_open.rs` へ分離した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eg_wp_r06_demux_open_foundation

- WP-R06として demux capability / demux info、filter open request、DVR open request、Filter/DVR callback保持の基盤を実装した。
- `getDemuxCaps()` / `getDemuxInfo()` は TS-only profile と旧 `tuner_hal` の demux/filter/DVR capability 数へ合わせ、未知 demux id は `Unsupported` へ倒す。
- `openFilter()` は filter type / buffer size / callback_present を `FilterRuntime` へ保持し、AIDL child object生成・callback owner登録・失敗時rollbackまで接続した。
- `openDvr()` は record/playback種別、buffer size、callback_present を `DvrRuntime` へ保持し、demux owner配下のDVR runtime登録・callback owner登録・失敗時rollbackまで接続した。
- Android 14 AIDL Rust生成境界に合わせ、section table info生成名と `FilterDelayHint.hintValue` の型変換を修正した。
- `m maleicacid_tuner_hal2_device_test maleicacid_tuner_hal2_service_runtime_test maleicacid_tuner_hal2_aidl_service_test` は成功。atest、VTS、実機確認は未実施。

# r50eg_tuner_hal2_dead_code_cleanup

- 品質整理として、moduleを定義しない `tuner_hal2/aidl_service/Android.bp` を削除した。
- 旧snapshot/field validation由来の未使用 `DomainValueValidation` と re-export を削除した。
- 未使用 `FrontendTuneStep` と re-export を削除した。
- production source / config の作業単位・過去版数由来文言を削除し、現行 scope の未接続メッセージへ寄せた。
- `common/src/os_abi.rs` を common library / common test の Android.bp srcs に接続した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eg_tuner_hal2_test_cleanup

- 旧snapshot / field contract / Px4Backend wrapper 系の削除対象テストは、該当旧ファイル削除済みであることを確認した。
- 新規 `#[test]` 関数は追加せず、既存テストを atest 対象に接続するため `maleicacid_tuner_hal2_control_core_test` / `maleicacid_tuner_hal2_fmq_test` / `maleicacid_tuner_hal2_descrambler_test` を Android.bp に追加した。
- `maleicacid_tuner_hal2_device_test` の srcs を library 側 runtime srcs と整合させ、`backend_worker.rs` / `scan_session.rs` / `live_pump.rs` を含めるようにした。
- `GLOBAL_CODE_CONVENTION.md` に Rust test / loom test の分担規約を追加した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eg_tuner_hal2_quality_fix5

- tuner_hal2 の品質是正として、AIDL snapshot / Debug文字列変換層を本番経路から外し、AIDL型から型付き filter config へ直接変換する正本へ置換した。
- descriptor-only の `px4_backend.rs` / `dvb_backend.rs` を削除し、live reader descriptor生成をregistry entry / backend session側へ寄せた。
- transaction / dispatch / handler coverage の正本を分離し、runtime側は `service_runtime/src/transaction_registry.rs` の `RuntimeTransactionSpec` を正とした。
- demux runtime generation 更新を `checked_add()` に寄せ、overflow時は対象runtimeをFailedにして成功扱いしない。
- backend tune transaction の generation=0固定を廃止し、frontend worker generationを `FrontendBackendTunePlan` から `BackendTuneTxn` へ渡すようにした。
- live pump join reportを破棄せず、packet数、malformed byte数、cancel理由、終端理由、join結果をfrontend runtime diagnosticsへ記録するようにした。
- 本番経路から外れた旧snapshot placeholderファイルを削除し、リリース物規則に合わせて英語のみのコメントを日本語化した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ef5

- product release marker を r50ef5 に更新。
- tv直下の `README.md` / `AGENTS.md` 追加、`開発規則.md` の root Markdown 許可リスト更新、Tuner HAL / TIS 統合手順の target 初期化補正のみで、tuner_hal2 のコード実装変更なし。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ee97_future_wording_and_wallclock_research_fix

- product release marker を r50ee97_future_wording_and_wallclock_research_fix に更新。
- 文書責務範囲の補正のみで、tuner_hal2 のコード実装変更なし。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ee96_doc_responsibility_scope_final

- product release marker を r50ee96_doc_responsibility_scope_final に更新。
- 文書責務範囲の補正のみで、tuner_hal2 のコード実装変更なし。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ee95_doc_responsibility_readfix

- product release marker を r50ee95_doc_responsibility_readfix へ更新した。
- 本変更は文書責務範囲の是正であり、tuner_hal2 実装コードは変更していない。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee94_nullable_future_work_reference_fix

- R1として `future_work/r51/android14_aidl_rust_nullable_filter_boundary_blocker.md` の正本性表現を是正した。
- future_work側を後続検討資料とし、現行 r51 設計判断・実装済み範囲・完了判定の正本を `tuner_hal/DESIGN_JA.md` とアーカイブ外○×表へ戻した。
- tuner_hal2 実装コードは変更していない。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee93_responsibility_boundary_docs

- product release marker を r50ee93_responsibility_boundary_docs へ更新した。
- 本変更は文書責務境界の是正であり、tuner_hal2 実装コードは変更していない。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee89_wp_r05f_backend_telemetry_readiness_partial

- WP-R05Fとして backend initial telemetry を frontend runtimeへ接続した。
- `FrontendSignalState` を追加し、frontend runtime snapshot/restore対象へ含めた。
- DVB backendは `FE_READ_STATUS` を読み、`FE_HAS_LOCK` / `FE_HAS_SIGNAL` から Locked / SignalDetected / NoSignal を記録する。
- PX4 backendは `PTX_GET_CNR` を読み、CNR>0をSignalDetected、0をNoSignalとして記録する。
- tune/scan worker起動後、backend initial telemetryを `record_frontend_signal_state()` でruntimeへ記録する。
- `IFrontend.getStatus(DEMOD_LOCK)` はruntime signal stateに基づき返す。
- `IFrontend.getFrontendStatusReadiness()` はruntime state + signal stateを使い、LockedをSTABLE、NoSignal/SignalDetected/UnknownをUNSTABLE、Closing/FailedをUNAVAILABLEへ写像する。
- ORIG-045b / ORIG-062 を○へ更新した。
- WP-R05には stopTune/close unbind、px4 reader完全化、live reader end-to-end、setDataSource(NULL) が残るためpartialである。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee88_wp_r05e_demux_rollback_restore_partial

- WP-R05Eとして rollback成功時のbound demux runtime / pipeline snapshot restoreを追加した。
- `DemuxRuntimeSnapshot` を追加し、DemuxRuntimeのstate/generation/pipeline/filter/dvr/filter queue状態をsnapshot/restore可能にした。
- `TunerServiceRuntime::bound_demux_runtime_snapshots()` / `restore_bound_demux_runtime_snapshots()` を追加した。
- tune/scan開始前にfrontend snapshotとbound demux snapshotを取得し、backend rollback成功時は両方を復元する。
- backend rollback失敗時は復元せず、frontend failed + bound demux quarantineへ落とす既存経路を維持する。
- ORIG-060を○へ更新した。LOCK/NO_SIGNAL status polling と backend telemetry readiness は未接続のため ORIG-045b / ORIG-062 は×維持。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee87_wp_r05d_backend_tune_rollback_partial

- WP-R05Dとして backend tune/scan rollback executor を公開AIDL経路へ接続した。
- `FrontendBackendSession::open_and_submit_with_previous()` を追加し、runtime snapshotの旧active tune requestをbackend rollback snapshotとして渡す。
- backend tune executorは capture / stop previous / apply system / apply channel / start streaming / read initial status / rollback stop / rollback restore previous request を型付き手順で実行する。
- `run_frontend_backend_tune_worker_with_previous()` を追加し、tune workerとscan workerのbackend submit失敗時に旧request復元を試す。
- rollback失敗はworker errorとしてfrontend failed + bound demux quarantineへ落とす既存経路へ接続する。
- backend旧tune復旧は前進したが、rollback成功時にbound demux runtime/pipeline状態を旧snapshotへ戻す経路は未実装のため ORIG-060 は×維持。LOCK/NO_SIGNAL status polling と backend telemetry readiness は未接続のため ORIG-045b / ORIG-062 も×維持。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee86_wp_r05c_tune_runtime_txn_partial

- WP-R05Cとして `IFrontend.tune()` / `IFrontend.scan()` のruntime transaction接続を進めた。
- `FrontendTuneTxnApply` / `FrontendScanTxn` を runtime handler coverage 上 `Connected` に変更し、公開AIDL経路がNotConnectedで止まらないようにした。
- frontend runtime snapshot / restore を追加し、tune/scan起動前にsnapshotを取り、descriptor install / scan session begin / worker start失敗時にruntime状態をrollbackする。
- `IFrontend.tune()` は同一tuneをgeneration boundary前にno-op成功として扱う。
- tune/scan開始時にbound demuxへgeneration boundary resetを実行する。
- tune/scan backend failure時にfrontend failed記録とbound demux quarantineへ接続した。
- ただし実backend旧tune取得・旧tune復元APIは未接続であるため、ORIG-038 / ORIG-056 / ORIG-058 / ORIG-059 / ORIG-060は×維持。ORIG-061はfrontend failed + bound demux quarantine接続部分のみ前進だが、rollback失敗検出起点が未接続のため×維持。ORIG-062はLOCK/NO_SIGNAL判定が未接続のため×維持。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee85_wp_r05b_filter_set_data_source

- WP-R05Bとして `IFilter.setDataSource(non-null)` を未実装拒否からruntime接続へ変更した。
- `FilterSetDataSourceTxn` を追加し、domain transaction / dispatch / handler coverageを `Connected` に分類した。
- source filterのlocal HAL object性、owner demux一致、source/sink自己参照、source lifecycle、source/sink subtype、PID mismatchを検証する経路を追加した。
- demux runtimeへ filter open kind を登録し、source filter接続成功時にgeneration boundaryを進め、sink filter originを `SourceFilter { source_filter_id, source_filter_generation }` へ変更する。
- `setDataSource(nullptr)` 系はAndroid 14 AIDL Rust nullable境界の別課題であるため、ORIG-279 / ORIG-280 は×を維持する。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee84_wp_r05a_live_pump_sink_route_fix

- r50ee83で○にした ORIG-040a について、`FrontendDemuxPacketSink` が本体に存在するだけで実運用側の起動経路へ接続されていなかったため修正した。
- `start_frontend_demux_live_pump_from_reader()` を追加し、非testのservice_runtime経路から `FrontendLivePumpOwner::start(reader, FrontendDemuxPacketSink)` へ到達できるようにした。
- live pump開始前にfrontend存在とbound demux存在を `ensure_frontend_demux_sink_ready()` で検証し、demux未接続を成功no-opにしない。
- 前回の ORIG-040a=○ は、完了条件を満たしていないのに○を付けたため虚偽を含んでいた。r50ee84で修正後も ORIG-040a は○維持とする。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee81_frontend_readiness_runtime_state

- WP-R05の先頭未達に着手し、`IFrontend.getFrontendStatusReadiness()` がfrontend runtime状態を読むようにした。supported statusはIdleで`STABLE`、Tune/Scan worker activeで`UNSTABLE`、Closing/Failedで`UNAVAILABLE`、unsupported statusは`UNSUPPORTED`を返す。
- ただし実backend telemetry取得、旧tune snapshot/rollback、bound demux quarantine、live pump demux sink接続、demux unbind/lease releaseは未達のため、WP-R05内の該当行は×を維持する。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee80_ssot_quality_cleanup_test_boundary

- r50ee79の○×表全件性について、物理行数と派生ID宣言が現物と一致していなかったため修正した。ORIG-045b / ORIG-226b を派生行として明示し、WP行数合計を379、元ORIG coverageを376/376へ揃えた。
- `aidl_service` の release APIに残っていた `has_callback_for_test()` を削除し、callback存在確認helperを `#[cfg(test)]` 内へ閉じた。
- `descrambler` の `insert_test_key()` / `expire_test_key()` を `#[cfg(test)] pub(crate)` へ閉じ、release APIからtest token補助を除外した。
- `fmq` の `FmqQueue::new()` / `Default` / `test_fill` / native未生成fallbackを削除し、release経路ではnative queue生成済みの `FmqQueue` だけを扱う構造にした。
- `TunerAidlService::new_degraded()` を削除し、空runtime service生成helperをrelease APIから除外した。
- 未実装domainの実装は進めず、release/test境界と捨て対象の除去だけを行った。

# r50ee79_ssot_quality_cleanup

- ○×表の派生IDを明示し、ORIG-305 / ORIG-306 / ORIG-307 を復元して元ORIG-001〜ORIG-376のcoverageを376/376へ戻した。
- `aidl_service/src/input_snapshot.rs` の `DemuxFilterType` Debug文字列 `contains()` 分類を削除し、AIDL型fieldのmatchによる分類へ置換した。
- `control/src/lib.rs` の `WorkerFailureClassifier` unit structを削除し、`WorkerFailureDomain::runtime_failure_kind()`へ畳んだ。
- `service_runtime/src/capability_profile.rs` の `CapabilityProfilePolicy` unit structを削除し、profile判定をmodule関数へ畳んだ。

# r50ee78_ssot_quality_rework

- `tuner_hal2/DESIGN_JA.md` を、既存 `tuner_hal/DESIGN_JA.md` との差分構造だけを書く文書へ縮小した。完了判定、未達理由、実装規約、product統合手順、変更履歴は他文書へ重複配置しない。
- `CODE_CONVENTION.md` を実装規約だけに整理し、WorkerExit / WorkerFailureClassifier / ScanSessionTxn の契約再定義を削除した。
- `README_JA.md` を使い始め入口に限定し、設計責務表や統合手順を重複させない形へ戻した。
- アーカイブ外の○×表から過去経緯本文を削り、WP/完了条件/判定だけをSSOT化した。
- r50ee77でpartial名にした理由のうち、build/atest/VTS/実機確認をこちらが実行できないことを通常成果物否認理由にしないよう整理した。

# r50ee77_ssot_quality_rework_partial

- `tuner_hal2/DESIGN_JA.md` の記載範囲を tv 直下 `開発規則.md` に集約し、README/CODE_CONVENTION/DESIGN_JA 間の責務重複を削減した。
- `README_JA.md` は使い始め入口に限定し、設計責務表と統合手順の重複説明を削除した。
- `CODE_CONVENTION.md` は実装規約に限定し、WorkerExit/WorkerFailureClassifierの契約定義を再掲しない形へ直した。
- `FrontendLivePumpOwner` を追加し、live pump coreをthread/cancel/join/report所有へ進めた。ただしdemux sink接続は未達であり、WP-R05のdemux配送行は×維持。
- r50ee75の「tuner_hal2ではFrontendWorkerRegistry/FrontendWorkerStopOutcomeを正本とする」記載はr50ee76で撤回済みであり、現行契約ではWorkerExit/WorkerFailureClassifierの契約名を維持する。

# r50ee76_design_name_alignment

- `tuner_hal2/DESIGN_JA.md` を、既存 `tuner_hal/DESIGN_JA.md` と異なる、または既存文書に存在しない tuner_hal2 固有事項だけを書く方針へ改訂した。
- `WorkerExit` / `WorkerFailureClassifier` は旧`tuner_hal`の単なる参照概念ではなく、tuner_hal2でも契約名として維持する方針へ修正した。
- `FrontendWorkerStopOutcome` はworker終了分類の正本ではなく、停止要求APIの操作結果に限定した。`Completed` には `WorkerExit` を含めるよう実装を改めた。
- `control/src/lib.rs` に `WorkerFailureClassifier` / `WorkerFailureDomain` を追加し、自由文字列分類やモジュール固有の別名分類器を作らないようにした。
- r50ee69以降に追加された `backend_worker.rs` / `scan_session.rs` / `live_pump.rs` が `Android.bp` の `libmaleicacid_tuner_hal2_device` srcsに含まれていなかったため追加した。

# r50ee75_quality_boundary_fix

- `tuner_hal2/DESIGN_JA.md` を追加し、tuner_hal2固有の worker / ScanSession / live pump / 旧`tuner_hal`参照境界を同モジュール内の設計正本へ移した。
- 旧`tuner_hal/DESIGN_JA.md` 末尾に混入していた tuner_hal2 固有の品質再構築追補を削除した。
- `WorkerFailureClassifier` / `WorkerExit` は旧`tuner_hal`側の参照概念であり、tuner_hal2では `FrontendWorkerRegistry` / `FrontendWorkerStopOutcome` を正本とすることを固定した。
- リリース物規則に反する `tuner_hal2/COMPLETION_MATRIX.md` をアーカイブから除外し、○×表は配布補助成果物としてアーカイブ外に置く方針へ戻した。

# r50ee72_tune_failure_diagnostic

- WP-R05の先頭未達であるORIG-038に対し、backend tune worker failureをfrontend runtimeへ診断反映する経路を追加した。
- `FrontendRuntime`に`TuneFailed` terminal eventを追加し、active tune generationのworker失敗時に`Failed`状態へ落とす。
- `TunerServiceRuntime::mark_frontend_tune_worker_failed()`を追加し、AIDL tune workerからfailureを記録できるようにした。
- ただし旧backend session snapshot、rollback executor、bound demux quarantineは未実装のためORIG-038は×のまま維持する。

# r50ee70_scan_session_aidl_connect

- r50ee69で追加したScanSession正本を、AIDL `IFrontend.scan()` 成功経路へ接続した。
- `IFrontend.scan()` は既存Scan worker/ScanSessionをSupersededByNewRequestで停止し、新generationへlive reader descriptorとScanSessionをcommitしてからScan workerを起動する。
- Scan workerはcandidateごとにbackend sessionをopen/submit/stopし、candidate progressionとScanEnd terminal記録へ接続する。
- backend submit失敗時はScanSessionをbackend failedへ落とす。
- callback delivery、live TS pump、demux binding、旧tune rollbackは未接続のためWP-R05全体は未完了。

# r50ee68_quality_rework

- WP-R05品質是正。旧`tuner_hal`は丸ごと移植せず、`WorkerLifecycleTxn` / `WorkerExit` / `ScanSessionTxn` / `backend_submit_tune()` / live pump stop/join方針を設計参照として採用することを `tuner_hal/DESIGN_JA.md` に固定した。
- `run_frontend_backend_scan_worker()` と `FrontendBackendScanPlan` を削除した。これは最初に成功したcandidateをbackend tuneし、cancelまで待つだけでScanSession、candidate progression、scan END callback delivery、callback failure診断を持たず、scan実装として不適切だったためである。
- `IFrontend.scan()` はsettings変換、frontend境界validate、backend candidate生成までは行うが、ScanSession正本が未実装のため `UNAVAILABLE` を返す。tune-backed scan placeholderによる成功no-opを禁止する。
- `LiveReader` / `LiveReaderKind` を `FrontendLiveReaderDescriptor` / `FrontendLiveReaderDescriptorKind` へ改名した。descriptorだけをlive pump完了条件の根拠にしない。
- worker開始後のlive descriptor install失敗時に `request_stop()` 結果を `let _ =` で捨てず、stop/join結果を検査するようにした。
- ○×表をSSOTとし、全WP・完了条件Markdownは生成しない方針へ変更した。r50ee67で過大○だったORIG-039/041/049/050/051/052を×へ戻した。
- WP-R05は未完了。ScanSession正本、live pump、callback terminal delivery、rollback/quarantine、demux bindingは未達である。

# r50ee66_partial_wp_r05_scan_supersede

- WP-R05の未達解消を継続。前回の未達理由は一部「未実装宣言」に留まっていたため、先頭×行のうちscan supersedeに必要な実コードを追加した。
- `FrontendWorkerRegistry::request_stop_and_join()` を追加し、既存scan workerを `SupersededByNewRequest` reasonでcancelし、完了回収してから新scan workerを起動できるようにした。
- `IFrontend.scan()` は新scan開始前に既存scan workerを停止・回収し、live readerをclearしてから新generationのscan workerを起動するようにした。
- `stopTune()` / `stopScan()` はcancel要求だけでなくworker完了回収経路へ接続した。
- `FrontendRuntime` にterminal event generation filterを追加し、新generation開始後に旧generationのterminal eventを受理しない基礎を追加した。
- terminal eventの実callback配送、Cancelled/END診断、rollback/quarantine、demux bindingは未実装のためWP-R05全体は未完了。

# r50ee40_partial_wp_r04

- WP-R04の未達解消を継続。前回の課題群は外部ブロッカーではなく、主に未実装・未接続であるため追加実装した。
- `RuntimeRegistry` に filter / DVR / descrambler runtime ID台帳を追加し、各ID発行を `checked_add()` 化した。
- `IDemux.openFilter()` / `IDemux.openDvr()` を、demux owner検証 -> child runtime ID確保 -> AIDL child object table登録 -> wrapper生成失敗時rollbackへ進めた。
- `ITuner.openDescrambler()` を固定UNAVAILABLEから、descrambler runtime ID確保 -> AIDL object table登録 -> wrapper生成失敗時rollbackへ進めた。
- parent demux close時のcascade closeで、object table上のfilter / DVR childとそれぞれのruntime IDを解除する経路へ進めた。
- `boot_from_probe_results()` でtransient runtime registryとAIDL object tableを初期化し、probe再実行時の旧object混入を避けるようにした。
- LNB export/probe、filter/DVR/descramblerの実data path cleanup、worker ownership、FMQ/AV/callback worker cleanup、全domain generation overflowは未実装のためWP-R04全体は未完了。

# r50ee38_partial_wp_r04

- WP-R04の未達解消を継続。
- `openDemux()` を固定UNAVAILABLEから、runtime demux ID確保 -> AIDL object table登録 -> wrapper生成 -> wrapper生成失敗時runtime ID解除へ進めた。
- `getDemuxIds()` を固定空配列からruntime registry由来へ変更した。
- `openDemuxById()` をruntime registry上の既存demux ID検証後にAIDL objectを返す経路へ進めた。
- `RuntimeRegistry` に demux runtime ID台帳とchecked_add()によるID発行を追加し、ID枯渇時は失敗へ落とすようにした。
- Demux object close成功後にruntime registry上のdemux IDを解除する経路を追加した。
- `getLnbIds()` / `openLnbById()` はLNB registry由来へ寄せたが、LNB probe/export IDが未実装のため通常は空/UNAVAILABLEのままである。
- LNB name解決、demux配下filter/DVR/descrambler生成、実runtime cleanup、worker ownershipは未実装のためWP-R04全体は未完了。

# r50ee37_partial_wp_r04

- WP-R04の未達解消を継続。
- `AidlObjectGeneration` はstale handle / ABA防止の世代IDとして使う局所手法であり、AOSP AIDL標準API名ではないことを明確化。
- `RuntimeObjectTable` のchild object登録時にowner objectがliveであることを検証するよう変更し、owner missing / owner generation mismatch / owner kind mismatch / owner not liveをtyped error化した。
- public method lookupで対象object自身だけでなくowner chainがliveであることも検証するよう変更した。
- parent object close時にobject table上のdescendant child objectをClosing/CleanupFailed/Closedへ連動させるcascade lifecycle helperを追加した。
- AIDL close共通経路をsingle-object closeからobject table cascade closeへ変更した。
- これはobject table上の所有・世代・親子lifetimeの前進であり、実filter/DVR/descrambler runtime cleanupやworker cleanupはまだ未接続のためWP-R04全体は未完了。

# r50ee36_partial_wp_r04

- WP-R04のうち、外部ブロッカーではなく実装未達だった箇所へ追加対応。
- `TunerServiceRuntime` にAIDL object generation発行状態を追加し、AIDL child object登録時の固定 `AidlObjectGeneration(1)` を廃止した。
- AIDL object generation発行を `checked_add()` 化し、overflow時は `RuntimeObjectTableError::GenerationOverflow` として失敗させるようにした。
- `RuntimeObjectTableError::GenerationOverflow` のAIDL error mappingを追加した。
- `ResourceLedger::reserve()` のgeneration発行を `saturating_add()` から `checked_add()` へ変更し、wrap/saturating reuseを禁止した。
- `getFrontendIds()` をprobe済みfrontend registry由来のID返却へ変更した。
- `openFrontendById()` をprobe済みfrontend ID検証後にAIDL object tableへ登録し、wrapper生成失敗時は登録rollbackする経路へ進めた。
- demux / LNB / child object / worker / callback delivery / FMQ / AV / descrambler の本番runtime未接続は残るため、WP-R04全体は未完了。WP-R04の○×表は×を維持する。

# r50ee35_partial_wp_r04

- WP-R04の先頭未達に着手。
- `RuntimeObjectTable` に `RuntimeObjectLifecycle` を追加し、Live / Closing / CleanupFailed / Closed / Quarantined を区別するようにした。
- close時にobject table entryを即削除せず、begin close -> callback cleanup -> commit close の状態遷移を通すようにした。
- callback store cleanup失敗を握りつぶさず、objectをCleanupFailedへ落として `UNKNOWN_ERROR` を返すようにした。
- object registration後にAIDL object wrapper生成が失敗した場合、登録済みobjectをrollbackする補助経路を追加した。
- AIDL公開method内の `unreachable!()` を削除し、想定外成功時は `UNKNOWN_ERROR` を返すようにした。
- WP-R04全体は未完了。worker failure、全公開API transaction rollback、cascade cleanup、generation overflow、DropLeakTxn、表5/7/8/10/11/14/18/20の全面実装は未達。

# r50ee34

- WP-R03の○行を再確認し、r50ee33のORIG-024完了判定に虚偽があったことを記録。
- `IFilter.getId()` / `IFilter.getId64Bit()` が `AidlMethodCall` / `DomainCommand` / `CommandPlan` を通らず直接IDを返していた問題を修正。
- `FilterGetId` / `FilterGetId64Bit` の `AidlApi`、`RuntimeTransactionName`、`CommandPlan`、dispatch table、`FilterCommand`、`AidlMethodCall` を追加。
- `service_runtime` が `binder_adapter` に依存しない構造は維持し、公開AIDL methodのtyped command網羅を再確認。

# r50ee33

- WP-R03 / ORIG-024の未達に対応。
- `UnsupportedPublicApi` typed commandを追加し、unsupported/probe未接続系の公開AIDL methodを `AidlMethodCall` / `DomainCommand` / `CommandPlan` へ載せる構造へ変更。
- ITuner query系の空結果返却も `CommandPlan` 経由へ通し、直接returnのみの経路を削減。
- `status_unavailable()` / `StatusCode::NAME_NOT_FOUND` の直接返却経路を削除。
- WP-R03を全件○へ更新。

# r50ee32

- 品質問題対応として、`service_runtime` が `binder_adapter` の型正本へ依存していた責務逆転を是正。
- `AidlObjectKind` / `AidlObjectId` / `AidlObjectGeneration` / `AidlApi` / `RuntimeTransactionName` / `CommandPlan` / `AIDL_TRANSACTION_TABLE` の正本を `domain_request` crateへ移動し、`binder_adapter` は再exportする構造へ変更。
- `service_runtime` の dispatch / object table / callback registry / runtime handler / unit tests から `binder_adapter` 依存を削除。
- `TunerServiceRuntime::plan_domain_command_dispatch()` を削除し、`CommandPlan` と `RuntimeExecutableRequest` を受ける `plan_command_dispatch()` に置換。
- 不要な二重正本を残さず、runtime側の参照元を `domain_request` に統一。
- WP-R03のORIG-024は引き続き×。直接 `status_unavailable()` / `NAME_NOT_FOUND` を返す公開AIDL methodが残るため、全公開methodが型付き格納へ入る条件は未達。

# r50ee31

- WP-R03 の前回○を再確認。
- ORIG-024 は主要経路では `AidlMethodCall` / `DomainCommand` / `CommandPlan` / `RuntimeExecutableRequest` へ到達するが、`getStatus()`、`setLnb()`、`openTimeFilter()`、`getAvSyncHwId()`、`connectCiCam()` など直接 `status_unavailable()` を返す公開AIDL methodが残っていたため、全公開method条件としては未達と判定。
- r50ee30 の ORIG-024 ○は虚偽として扱い、○×表を×へ戻した。
- コード品質評価として、B案の責務分離は前進しているが、`service_runtime` が `binder_adapter` 型へ依存している点と、直接return経路が残る点を未達理由へ明記。

# r50ee30

- WP-R03 / ORIG-024 の未達解消を継続し、「ブロッカー」は存在しないことを明確化。残っていたのは判断待ちではなく実装順序上の未作成部品である。
- `DomainCommand::runtime_executable_request()` を追加し、typed domain request を runtime dispatch plan へ載せる境界を追加。
- `RuntimeCommandDispatchPlan` が `RuntimeExecutableRequest` を保持できるよう変更し、runtime handler側でも profile support / value validation を検査する前段を追加。
- `AidlMethodAdapter::plan()` の重複宣言を削除し、r50ee29の構文上の粗さを修正。
- ORIG-024は、AIDL method相当前段から `AidlMethodCall` / `DomainCommand` / `CommandPlan` / runtime executable request へ型付き格納する条件を満たすため○へ更新。

# r50ee29

- WP-R03 / ORIG-024 の未達解消を継続。
- ORIG-024 の前回説明は、状況と未達理由の分離が弱く、特に「なぜ今回完了できないか」の阻害課題としては不十分だったため、○×表の様式を是正。
- `DemuxOpenFilterRequest` / `DemuxOpenDvrRequest` を `domain_request` crateへ追加し、openFilter/openDvr の入力も runtime executable request 前段へ載せた。
- `DemuxFilterTypeDomain` を追加し、openFilter時の filter type を `ts/section/av/pes_data/record/unknown` のdomain分類として保持するよう変更。
- `ScIndexMaskRequest` を追加し、record `scIndexMask` を debug文字列だけでなく正規化可能なscalar maskとして保持する前段を追加。
- `binder_adapter/src/domain_request.rs` の未定義 `format_static` 依存を排除し、DVR field名を静的tableで解決するよう修正。
- ORIG-024は前進したが、Filter/DVR object生成transactionが未実装で、openFilter時のfilter typeをfilter object lifetimeへ永続化できず、configure(settings)との対応検証を実methodで保証できないため×を維持。

# r50ee28

- WP-R03 の前回○を再確認。ORIG-020/021/022/023/025/027 は現物上、完了条件を満たすことを確認。
- ORIG-232 は WP-R03 ではなく、runtime execution / rollback / callback / FMQ / EventFlag / cleanup failure の実発生点が揃った後に全公開method横断で検証すべき条件であるため、WP-R13へ再配置。
- ORIG-024 の残未達理由は、返答内で状況と実装完了を妨げている課題を説明する形式として成立していたが、表側の残理由も同じ粒度へ維持。
- 完了条件行数は376件のまま維持し、欠落・重複なしを確認。

# r50ee27

- WP-R03のORIG-024/ORIG-232未達解消を継続。
- `domain_request` crateに `RuntimeExecutableRequest` を追加し、`AidlDomainRequest` からruntime handlerが直接受け取る最終request enumへ変換できる前段を追加。
- `SectionBitsConditionRequest` をfilter/mask/modeの長さだけではなくbyte列本体を所有する型へ変更し、section conditionの本体所有先を `domain_request` 側へ固定。
- `aidl_service/src/input_snapshot.rs` が section condition のfilter/mask/mode byte列をhex fieldとしてsnapshot化するよう変更。
- `binder_adapter/src/domain_request.rs` がsection condition hex fieldをdecodeし、`SectionBitsConditionRequest` のbyte列本体へ渡すよう変更。
- `execute_object_aidl_method()` を `AidlDomainRequest` ではなく `RuntimeExecutableRequest` のprofile support / value validationを評価する形へ変更し、runtime実行用requestへの変換結果をexecutor前段の検査対象にした。
- `configureIpCid()` / `configureMonitorEvent(nonzero)` の個別validator直返しを減らし、`unavailable_after_method_plan()` 経由でstatus precedence resolverを通すよう変更。
- ORIG-024は最終request enumとsection condition本体所有先まで進んだが、DemuxFilterTypeとDemuxFilterSettingsの対応検証、record scIndexMask構造化、runtime handler本体への接続が未達のため×を維持。
- ORIG-232は個別早期returnを一部削減したが、runtime execution failure / rollback failure / callback・FMQ・EventFlag・cleanup failureの発火点が未実装であり、全公開methodのexecutor集約は未達のため×を維持。

# r50ee26

- B案を採用固定。純粋domain request型の正本を `binder_adapter` から分離し、専用crate `domain_request` へ移した。
- `domain_request/src/lib.rs` を追加し、`AidlDomainRequest` / `DemuxFilterDomainRequest` / `TsFilterRuntimeRequest` / `DvrRuntimeRequest` / `AvStreamDomainRequest` / `FilterDelayDomainRequest` / `DomainValueValidation` を収容した。
- `binder_adapter/src/domain_request.rs` はAIDL `AidlInputSnapshot` から `domain_request` crateのdomain request型へ変換するadapter層に限定した。
- `binder_adapter/src/aidl_method.rs` は `domain_request_from_snapshot()` を通じてdomain requestを作るよう変更し、domain型そのものの正本を持たない構造へ寄せた。
- `Android.bp` に `libmaleicacid_tuner_hal2_domain_request` を追加し、`binder_adapter` / `service_runtime` から参照可能にした。
- ORIG-024はB案のcrate分離まで進んだが、runtime handlerが受け取る最終request enumへの接続、section condition byte列本体、record scIndexMask構造化、DemuxFilterTypeとの対応検証が未完成のため×を維持。
- ORIG-232はB案の影響で前提構造は改善したが、runtime execution failure / rollback failure / callback・FMQ・EventFlag・cleanup failure / 個別早期returnのexecutor集約が未達のため×を維持。

# r50ee25

- WP-R03のORIG-024/ORIG-232未達解消を継続。
- `binder_adapter/src/domain_request.rs` の `AidlDomainRequest` を文字列field bag中心から、TS filter / DVR / AV stream / delay hint別のruntime request型へ分解。
- `DemuxFilterSettings::Ts` 配下の `Noinit` / `Section` / `Av` / `PesData` / `Record` を `TsFilterRuntimeRequest` として区別し、`TsPid`、section condition、DVR direction、AV stream kind、delay kindを型へ昇格。
- unsupported demux variantを `UnsupportedDemuxFilterRequest` として保持し、TS-only profile外を記録したうえで `UNAVAILABLE` に落とす前段を強化。
- `execute_object_aidl_method()` が `AidlDomainRequest` のprofile unsupportedとinput validation failureをfailure sourceとして集約するよう変更。
- ただしruntime実行用requestの全field完全変換、section condition byte列本体の所有先、rollback failure/runtime execution failureのexecutor集約は未達として残す。

# r50ee24

- WP-R03の先頭未達解消を続行。
- `binder_adapter/src/domain_request.rs` を追加し、AIDL入力snapshotを値域検証規則付きdomain request型へ変換する層を追加。
- `DomainCommand` の Demux / Filter / DVR / Frontend callback / LNB 適用系を `AidlDomainRequest` 保持へ変更し、runtimeがdebug snapshot文字列だけを再解釈する構造を縮小。
- `AidlFailureSource::ObjectLifetime` と `resolve_failure_source_by_precedence()` を追加し、共通executor骨格がobject lifetime / runtime dispatch failureをfailure sourceとして解決する形へ前進。
- `unavailable_after_method_plan()` を固定UNAVAILABLE返却から、`AidlFailureSource::RuntimeDispatch` をstatus precedence resolverへ渡す経路へ変更。
- ORIG-024 / ORIG-232は未達のまま維持し、残る阻害課題を○×表へ記載。

# r50ee23

- WP-R03の先頭未達解消を継続。
- 「TS/MMTP/IP/TLV/ALP variantを記録し、TS-only profile外をUNAVAILABLEへ落とす」責務を、variant/field保持はORIG-024、status優先順位はORIG-232として整理。
- Frontend/LNB callback `Strong` 保持をAIDL object本体から `callback_store` へ移し、AIDL object本体をhandle + shared runtime参照だけに戻した。これによりORIG-025を○へ戻した。
- `AidlFailureSource` と `AidlStatusMapper::resolve_failure_by_precedence()` を追加し、profile unsupportedをinput validationより先に解決できるstatus resolverを追加。
- `IDescrambler.addPid()` の重複関数宣言を削除。
- ORIG-024はruntime消費用domain request型への完全変換が未実装、ORIG-232は全公開methodのfailure source集約が未接続のため×のまま維持。

# r50ee22

- 前回着手WPであるWP-R03の○行を再確認。
- r50ee21の `ORIG-025` は、Frontend/LNB AIDL objectにcallback slotを保持しており「handleと共有runtime参照だけ」とは言えないため虚偽○として×へ戻した。
- `ORIG-027` は `IFrontend.tune()` / `scan()` と `IDescrambler.addPid()` / `removePid()` で入力変換がclose検査より先に走る経路があったため修正し、close状態検査を先行させた。
- WP-R03の残未達理由を、状況説明と実装完了を妨げる課題に分けて更新。

# r50ee21

- WP-R03の未達解消を継続。
- `aidl_service/src/aidl_v2_conversion_contract.rs` を追加し、Android 14 / LineageOS 21相当の `android.hardware.tv.tuner-V2` frozen AIDL生成hash、`DemuxFilterSettings` union tag、TS/DVR/AV/delay系field契約をコード上に固定。
- `input_snapshot.rs` をr50ee21のAIDL V2変換契約へ接続し、schema hashとfield contractをsnapshotへ添付。
- `AidlMethodCall::api()` と `AidlMethodPlan.api` を追加し、`execute_object_aidl_method()` をAPI別status precedence tableを通る共通executor骨格へ接続。
- WP-R03内に残していた `ORIG-026` / `ORIG-033` は再ソート不足だったため、Filter/DVR open transactionおよびcallback worker/failure接続の後続WPへ移動。
- r50ee20の残理由が浅かった `ORIG-026` / `ORIG-033` / `ORIG-232` は、状況説明と実装完了を妨げる課題を分けて再記載。

# r50ee20

- WP-R03のORIG-024未達を前進。
- `aidl_service/src/input_snapshot.rs` をAndroid 14 / LineageOS 21相当の `android.hardware.tv.tuner-V2` AIDL schema前提の構造化snapshotへ拡張。
- `DemuxFilterSettings::Ts` の `Noinit` / `Section` / `Av` / `PesData` / `Record`、`DvrSettings::Record` / `Playback`、`AvStreamType::Video` / `Audio`、`FilterDelayHint` の主要fieldをfield名付きで保持するよう変更。
- MMTP / IP / TLV / ALPはTS-only profile外としてvariantを記録し、`UNAVAILABLE`へ落とす前提を明示。
- ORIG-024は、unsupported variantの全field domain保存、invalid/unsupported precedence、実domain request型への完全変換が未完のため×のまま維持。

# r50ee19

- WP-R03の未達解消を継続。
- `aidl_service/src/input_snapshot.rs` を追加し、`DemuxFilterSettings` / `DvrSettings` / `AvStreamType` / `FilterDelayHint` / openFilter / openDvr のsnapshot生成を公開method本体から分離した。
- `IFilter.configure()` / `configureAvStreamType()` / `setDelayHint()`、`IDvr.configure()`、`IDemux.openFilter()` / `openDvr()` を専用snapshot生成関数へ接続し、公開method本体がdebug文字列を直接組み立てる経路を縮小した。
- `unavailable_after_method_plan()` をAPI別status precedence tableを参照する経路へ変更し、固定UNAVAILABLE返却でもtransaction tableのprecedence entry欠落を検出するようにした。
- ただしAIDL生成型の全variant・全fieldをdomain型へlosslessに落とす変換、Filter/DVR callbackのowner object寿命、callback配送worker/failure状態、全公開methodの共通executor化は未達のためWP-R03は×のまま維持する。

# r50ee18

- WP-R03の未達解消を継続。
- r50ee17の残未達理由が「何が未実装か」の説明に寄っていたため、○×表の理由欄を「今回実装完了を妨げている課題」へ書き直した。
- `aidl_service/src/callback_slot.rs` を追加し、Frontend/LNB callback の `Strong<dyn ...Callback>` をAIDL object shell内で保持するslotを追加。
- `FrontendAidlObject` / `LnbAidlObject` の `setCallback()` を、method plan後に callback slot と runtime callback registry へ登録する経路へ進めた。
- `close_object()` で runtime callback registry のowner entryを消すようにし、Frontend/LNB object close時はcallback slotもclearするようにした。
- Filter/DVR callbackは、openFilter/openDvrの実object生成・owner rollbackがWP-R04未達のため、まだ実callback配送経路へ保持できないものとして×に残した。

# r50ee17

- WP-R03の未達解消を継続。
- `IFilter.getQueueDesc()` / `IFilter.getAvSharedHandle()` / `IFilter.releaseAvHandle()` / `IDvr.getQueueDesc()` を `AidlMethodCall` / `DomainCommand` / `CommandPlan` / runtime dispatch table へ接続。
- `service_runtime/src/callback_registry.rs` を追加し、callback owner / generation / registration API / health state の台帳型を追加。
- ただし実callback配送worker、Strong callback保持、callback failure発生時の状態遷移接続は未実装のため、WP-R03のcallback系完了条件は×のまま維持。
- `DemuxFilterSettings` / `DvrSettings` / `AvStreamType` / `FilterDelayHint` のAIDL生成型全field lossless domain変換も未完了のため、ORIG-024は×のまま維持。

# r50ee16

- WP-R03の未達解消を継続。
- `AidlInputSnapshot` を単一debug文字列だけでなく `AidlInputField` の構造化field列を保持できる形へ拡張。
- `Demux.openFilter/openDvr`、`Demux.setFrontendDataSource`、`IFilter.configure/configureAvStreamType/setDataSource/setDelayHint`、`IDvr.configure/attachFilter/detachFilter`、`ILnb.set*` 系の入力snapshotを、既知scalar/handle有無についてfield名付きで保持する形へ更新。
- `AidlStatusMapper` にAPI別status precedence tableを追加し、少なくともtransaction table上の全APIが precedence entryを持つことを単体テストで固定。
- ただしAIDL生成型の全fieldを個別domain型へ写すlossless変換、callback実配送、callback failure状態遷移、全method実経路へのprecedence適用は未達のため、WP-R03は×のまま維持。

# r50ee15

- WP-R03の未達解消を継続。
- child AIDL methodのtyped command前段を拡張し、Demux openFilter/openDvr、Filter configure/configureAvStreamType/setDataSource/setDelayHint、DVR configure/attach/detach/status interval、Descrambler setDemuxSource、LNB callback/voltage/tone/position/DiSEqCの入力を `AidlMethodCall` / `DomainCommand` へ格納する経路を追加。
- ただしAIDL生成型を構造化したlossless変換、callback配送worker、callback failure状態、method別error precedenceは未達のため、WP-R03は×のまま維持。

# r50ee14

- WP-R03 の未達解消を継続。
- `IFrontend.tune()` / `scan()` が placeholder `FrontendTuneRequest` を使わず、AIDL `FrontendSettings` から ISDB-T / ISDB-S の公開入力を検証して `FrontendTuneRequest` へ変換する前段を追加。
- `IDescrambler.addPid()` / `removePid()` が placeholder PID 0 を使わず、AIDL `DemuxPid::TPid` を検証して typed command へ渡すよう修正。
- callback / FMQ / AV / data source / DemuxFilterSettings / DvrSettings の lossless DomainCommand 化は未達のまま、WP-R03 の該当欄を×で維持。

# r50ee13

- 先頭未達WPであるWP-R03に着手した。
- `aidl_service/src/object_runtime.rs` に `plan_object_aidl_method()` / `close_object_after_aidl_method_plan()` を追加し、AIDL object tableのclose状態検査後に `AidlMethodCall` / `DomainCommand` / `CommandPlan` / runtime dispatch target計画へ到達する共通前段を追加した。
- child AIDL object shellに `plan_method()` / `close_object_after_plan()` を追加した。
- `IFrontend.tune/stopTune/scan/stopScan/close`、`IDemux.openFilter/openDvr/close`、`IFilter.configure/configureIpCid/configureMonitorEvent/start/stop/flush/close`、`IDvr.configure/start/stop/flush/close`、`IDescrambler.setDemuxSource/setKeyToken/addPid/removePid/close`、`ILnb.setVoltage/setTone/setSatellitePosition/sendDiseqcMessage/close` から型付きmethod計画へ入るようにした。
- ただし `FrontendSettings` / `DemuxPid` 等のAIDL入力をlosslessにDomainCommandへ格納する変換、callback objectの実配送経路保持、callback failure状態接続、全公開methodのAPI別error precedenceは未達のため、WP-R03は×のまま維持する。
- 静的確認は実施した。Soong build / Rust unit test / atest / VTS はこの環境では未実行であり、最終gate側のWP-R13へ残す。

# r50ee12

- 先頭未達WPであるWP-R02に再着手した。
- `IFilter.configureIpCid()` / `IFilter.configureMonitorEvent()` のprofile境界を `FilterAidlObject` のmethod別前段へ移し、公開AIDL methodから直接通る経路として固定した。
- `configureIpCid()` は値が負数・0・正数のいずれでも、TS-only profile外のため `UNAVAILABLE` へ写像する。
- `configureMonitorEvent(0)` は成功、非0 mask は profile非宣言のため `UNAVAILABLE` へ写像する。
- unsupported API precedence を、IP CID値検証よりprofile unsupportedを先に評価する形で固定した。
- WP-R02内の残未達3行を○へ更新した。WP-R02内に未達は残さない。
- 静的確認は実施した。Soong build / Rust unit test / atest / VTS はこの環境では未実行であり、最終gate側のWP-R13へ残す。

# r50ee11

- WP-R02再確認で、r50ee10の○×表に虚偽が含まれていたことを記録した。
- r50ee10では「API別に `INVALID_ARGUMENT` / `INVALID_STATE` / `UNAVAILABLE` / `UNKNOWN_ERROR` をDESIGN_JA.mdと一致させる」を○にしていたが、実態は汎用 `HalError` kind 写像までであり、公開AIDL method別の入力検証・状態検証・unsupported precedence を実method経路へ接続していなかった。
- そのため同完了条件を×へ戻し、未達理由を○×表へ明記した。
- ○×表のWP順序を再ソートした。probe/capability、AIDL method別error接続、VTS/profile完全一致など、後続runtimeや最終gateを前提にする行をWP-R02から後続WPへ移した。
- 元の完了条件行は376件すべて保持し、欠落0・重複0を確認した。
- 実装コードは進めていない。

# r50ee10

- 再編成後の先頭未達WPであるWP-R02に着手した。
- `service_runtime/src/capability_profile.rs` を追加し、TS-only capability/profile、IP CID非対応、monitor event非0非対応、HAL-generated Japanese scan plan禁止、失敗領域分類を型として固定した。
- `HalError` に callback / FMQ / EventFlag / cleanup の失敗領域を追加し、device missing/open失敗は `UNAVAILABLE`、runtime ioctl/read・callback・FMQ・EventFlag・cleanup失敗は `UNKNOWN_ERROR` へ写像するよう `AidlStatusMapper` を修正した。
- `IFilter.configureIpCid()` は値の妥当性にかかわらず、TS-only profile外のため `UNAVAILABLE` へ写像する。
- `IFilter.configureMonitorEvent(0)` は成功、非0 mask は profile非宣言のため `UNAVAILABLE` へ写像する。
- frontend/demux/filter/DVR/descrambler/LNB capability のruntime/probe/profile完全一致、VTS profile生成、export frontend ID / physical group ID生成、linkCaps/AV shared/descrambler/LNB/statusCaps profile連動は後続WPが前提のためWP-R02内に×として残す。

# r50ee8

- WP-02再確認で、r50ee7の○×表に虚偽が含まれていたことを記録した。
- r50ee7では「AIDL method相当前段から `AidlMethodCall` / `DomainCommand` / `CommandPlan` へ型付き格納する」を○にしていたが、公開AIDL method本体はその経路を実際には呼ばず、`plan_method()` と単体テスト用経路に留まっていた。
- そのため、同完了条件を×へ戻し、未達理由を○×表に明記した。
- 実装コードは進めていない。WP-02総合判定は×のまま維持する。

# r50ee7

- 先頭未達WPであるWP-02に再着手した。
- child AIDL objectが `AidlObjectHandle` に加えて共有 `TunerServiceRuntime` 参照だけを持つようにし、runtime状態の複製は行わないまま、全child公開methodの先頭で object table の kind / generation / closed 状態を検査する経路を追加した。
- `aidl_service/src/object_runtime.rs` を追加し、close後・missing object・generation mismatch・kind mismatchを `INVALID_STATE` に写像する共通gateにした。
- `IFrontend` / `IDemux` / `IFilter` / `IDvr` / `IDescrambler` / `ILnb` の `close()` は、WP-02段階ではAIDL object table登録を解除するところまで実装した。
- callback object保持、probe/capability生成、open系owner/rollback、callback failure状態接続は後続WPのruntime / worker / delivery 実体化が前提のため、WP-02総合判定は×のまま維持する。

# r50ee6

- WP-01再確認で、r50ee5の○×表に虚偽が含まれていたことを記録した。
- r50ee5では「旧 `tuner_hal` の旧product統合用断片を残さない」を○にしていたが、旧 `tuner_hal/INTEGRATION.md` が残っていたため、この○は完了条件を満たしていなかった。
- 旧 `tuner_hal/INTEGRATION.md` を削除し、product統合手順のSSOTを `tuner_hal2/INTEGRATION.md` のみに固定した。
- `tuner_hal/README_JA.md` と `tuner_hal2/INTEGRATION.md` に、旧 `tuner_hal/INTEGRATION.md` を置かない方針を反映した。

# r50ee5

- product default Tuner HAL service を `tuner_hal2` だけに固定した。
- 旧 `tuner_hal` は参照用ソースとして残すが、product package、VINTF manifest、init rc、PRODUCT_PACKAGES、product integration には含めない方針へ変更した。
- `tuner_hal2/INTEGRATION.md`、`config/product_integration.mk`、`BoardConfigVendorSePolicy.mk`、`ueventd.tuner_hal2.rc` を追加した。
- 旧 `tuner_hal/config/product_integration.mk` は削除し、旧product統合用のconfig、VINTF/init、sepolicy、profile断片も削除した。
- `README_JA.md` と `開発規則.md` の既定service方針をr50ee5の固定方針へ更新した。
- WP-01のproduct統合切替・旧service排他未達を解消し、WP-01を○へ更新した。

# r50ee4

- 先頭未達WPであるWP-02に着手した。
- `android.hardware.tv.tuner-service.maleicacid2` の `rust_binary`、`aidl_service/src/main.rs`、`aidl_service/src/service_entry.rs`、init rc、VINTF fragment、sepolicy contextを追加した。
- `TunerAidlService` に `ITuner` AIDL trait実装本体を追加し、`FrontendAidlObject` / `DemuxAidlObject` / `FilterAidlObject` / `DvrAidlObject` / `DescramblerAidlObject` / `LnbAidlObject` にchild AIDL trait実装本体を追加した。
- 未実装runtimeへ到達する公開methodは成功扱いせず、`UNAVAILABLE` / `NAME_NOT_FOUND` へ落とす方針にした。
- callback保持、close後状態検査、probe/capability生成、open系rollback、callback failure状態接続は後続WP前提のため、WP-02総合判定は×のまま維持する。

# r50ee3

- r50ee2で `README_JA.md` にWP-01判定・未達理由を記載していたリリース物規則違反を修正。READMEは第三者が使い始めるために必要な現行情報だけに戻した。
- WP-01の○行を再確認し、`README_JA.md` / `CHANGELOG.md` の責務分離をWP-01完了条件へ追加した。
- r50ee2の説明にあった「WP-01の未達3行」は誤りであり、実際は未達5行だったため訂正した。
- 実装コードは進めていない。WP-01総合判定は×のまま維持する。

# r50ee2

- 先頭未達WPであるWP-01を再確認。
- `tuner_hal2` を `android.hardware.tv.tuner.ITuner/default` の既定serviceへ登録する条件は、WP-02の `rust_binary` / Binder登録entrypoint / `ITuner` trait実装が未達のため満たせないと固定。
- WP-01の未達5行は×のまま維持し、未達理由を○×表へ明記。
- 未完成AIDL serviceを製品統合へ出して二重登録や不可能なbuild pathを作らない方針を維持。

# CHANGELOG

## r50ee1

- r50ed16を基準に、`tuner_hal/DESIGN_JA.md` と `tuner_hal2` 配下の `#[test]` 単体テストの準拠関係を全件再監査した。
- DESIGN_JA.mdと矛盾する単体テストは検出されなかったため、単体テスト削除は行っていない。
- r50ed16の健全性根拠は、未接続runtime handlerを成功扱いせず `RuntimeHandlerError::NotConnected` として失敗させる点にある。
- r50ee1はテスト削除結果のリリースであり、公開AIDL実装、runtime実処理、worker、callback、VTS対応範囲を新規完了扱いにしない。

## r50ed16

- WP-R2を実施し、`service_runtime` に object table と runtime dispatch handler を追加した。
- `RuntimeObjectTable` は `BTreeMap<AidlObjectId, RuntimeObjectEntry>` を単一正本とし、object kind、ledger id、generation、owner relation を型付きで保持する。
- generation mismatch、missing object、owner/kind mismatch は `RuntimeObjectTableError` / `RuntimeHandlerError` として返し、成功扱いしない。
- `RuntimeHandlerCoverage` により全 `RuntimeTransactionName` を `Connected` / `NotConnected` / `UnsupportedByDesign` のいずれかへ分類する。
- r50ed16時点では実runtime handler接続はまだ行わず、未接続handlerは `RuntimeHandlerError::NotConnected` として型付き失敗に固定する。
- WP-R2はAIDL公開契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせる内部object table / dispatch handler実体化に限定する。

## r50ed15

- WP-R1を実施し、`aidl_service` の AIDL Binder object外側骨格を追加した。
- `TunerAidlService`、各 AIDL object wrapper、`AidlObjectHandle`、`CallbackBridge`、`NativeHandleBridge`、`AidlErrorBridge` を追加した。
- AIDL objectは `AidlObjectHandle` だけを保持し、runtime状態を複製しない構造にした。
- `aidl_service` は `device`、`demux`、`descrambler`、`lnb` の内部runtime型やdriver ABIへ直接依存しない。
- Android 14 Tuner AIDL生成crate、`binder_adapter`、`service_runtime` への依存をSoong定義へ追加した。
- WP-R1はAIDL公開契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` と Android 14 Tuner AIDL を正とする外側骨格追加に限定する。

## r50ed14

- WP-10を実施し、AIDL公開メソッド相当前段を `binder_adapter/src/aidl_method.rs` として実体化した。
- `AidlMethodCall` / `AidlMethodAdapter` / `AidlMethodPlan` により、公開メソッド相当入力から `DomainCommand` と `CommandPlan` を型付き生成する。
- `service_runtime/src/command_dispatch.rs` を追加し、`DomainCommand` から `ServiceRuntimeDispatchTarget` までを型付きで接続する。
- `TunerServiceRuntime::plan_domain_command_dispatch()` を追加し、dispatch target欠落時は型付き診断へ落とす。
- WP-10はAIDL Binder service本体を新規実装せず、公開AIDL method相当前段から内部runtime dispatch targetまでの接続に限定する。

## r50ed13

- WP-09を実施し、`service_runtime` の boot / registry / startup diagnostics / dispatch table を実体化した。
- `StartupDiagnosticRecord` は `StartupDiagnosticKind` / `StartupDiagnosticPhase` / backend / path / typed error を保持し、起動診断に `Vec<String>` を使わない。
- probeで見つからないfrontend、open失敗、capability suppress、duplicate frontend idを診断へ残し、存在しないfrontendをruntime registryへ登録しない。
- `RuntimeRegistry` は export対象frontendだけを `FrontendRegistryEntry` として保持する。
- `SERVICE_RUNTIME_DISPATCH_TABLE` により `binder_adapter` の `RuntimeTransactionName` から service runtime dispatch target を追跡可能にした。
- `maleicacid_tuner_hal2_service_runtime_test` をSoong test moduleとして追加した。
- WP-09はAIDL公開API契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせるservice runtime内部の実体化に限定する。

## r50ed12

- WP-07を実施し、`lnb/src` の runtime / apply transaction / lifecycle transaction / operation ledgerを実体化した。
- `LnbApplyTxn` は backend apply と registry commit を同一取引で扱い、registry commit失敗時は通常状態へ戻さず `Failed` に落とす。
- `LnbLifecycleTxn` は public close / owner loss と Drop leak を分離し、Drop leakではbackend applyやcallback clearを実行しない。
- `LnbOperationLedger` はactive操作と解除失敗を型付きで保持し、自由文字列の操作失敗表を使わない。
- WP-08を実施し、`binder_adapter` の command / transaction / status mapper前段を実体化した。
- 各AIDL API相当のdomain commandは `CommandPlan` により呼び出すruntime transactionを型付きで追跡できる。
- `AidlStatusMapper` は `HalError` のkindから `TunerStatusCode` へ写像し、表示文字列には依存しない。
- `binder_adapter` は driver ABI、FMQ shim、device/demux/descrambler/lnb runtime crateへ直接依存しない構造にした。
- WP-07/08はAIDL公開API契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせる内部runtime / adapter前段の実体化に限定する。
## r50ed11

- WP-06を実施し、`descrambler/src/runtime` の session / key token / PID claim / cleanup transactionを実体化した。
- `DescramblerKeyToken` をBinder境界でだけ `Vec<u8>` から生成するnewtypeにし、sessionはtoken bytesではなく `DescramblerKeySlotId` を保持する。
- `DescramblerKeyTable` は unknown token と expired token を型付きで分ける。raw CWは保持・出力しない。
- `DescramblerSessionTxn` が demux binding、key replacement、PID claim、cleanupを所有し、unknown token時のrollbackとclose時の一括cleanupをテストで固定した。
- `DescramblerPidClaim` は source filter id と generation を型付きで保持し、NULL source filter経路は現行Android 14 Rust AIDL境界の実装対象外として型付き拒否する。
- WP-06はIDescrambler公開API契約を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせる内部runtime実体化に限定する。

## r50ed10

- WP-05を実施し、FMQ write と EventFlag wake を `FmqDeliveryTxn::commit_payload()` の一体commit条件にした。
- write済みbyte数が期待値と一致しない場合は `ShortWrite` として write段階失敗に固定し、wake成功だけで配送成功にしない。
- `demux/src/av` の `AvSharedBacking` を実体化し、slot、active/stale `dataId`、client handle release状態を保持するようにした。
- `releaseAvHandle()` 判定順序を `AvHandleReleaseTxn` に固定し、fd付きhandle + `dataId=0` と empty handle + `dataId=0` を client handle release通知として扱う。
- `dataId=0` release で active slot を全解放しないこと、active slot release 後に stale dataId として遅延releaseを吸収することをテストで固定した。
- WP-05はFMQ / AV内部runtimeの実体化であり、AIDL公開API契約やARIB構文処理を新規定義しない。

## r50ed9

- WP-04を実施し、`demux/src/runtime` の `DemuxRuntime` / `FilterRuntime` / `DvrRuntime` / boundary transactionを実体化した。
- `SourceBoundaryTxn` は存在しないqueueを新規生成して成功扱いにせず、`QueueMissing` と typed outcome で返す。
- `GenerationBoundaryTxn` は packet pipeline の assembler / continuity / resync を reset し、demux generation を進める。
- `FilterConfigureTxn` / `DvrConfigureTxn` は snapshot / rollback outcome を型付きで保持し、configure失敗時に内部設定だけ新状態になることを禁止した。
- loom利用準備として、`external/rust/android-crates-io` が `libloom` を供給する環境で使う任意Soong defaults `maleicacid_tuner_hal2_loom_test_defaults` を追加した。通常testへは適用していない。

## r50ed8

- WP-03を実施し、`device/src/runtime` の backend tune 取引を実体化した。
- `BackendTuneTxn::apply()` に capture / stop / apply system / apply channel / start streaming / read status / commit と rollback を実装した。
- rollback失敗は `BackendTuneRollbackStep` と `BackendTuneRollbackReport` で型付き保持する。
- `FrontendTuneTxn::apply()` で worker起動失敗時に backend tune を rollback し、backend tune済みだけが残る状態を禁止した。
- `Px4Backend` の live reader は control fd 複製を表す `LiveReaderKind::Px4DuplicatedControlFd` に固定し、同一 chardev path の二重openを構造上モデル化しない。
- `FrontendRuntime` は backend kind、generation、live reader、last error を所有するようにした。

## r50ed7

- `README_JA.md` から r50ed5 / r50ed6 固有の補足を削除し、現行構造の使い始め説明に限定した。
- `tuner_hal2` の設計正本は既存 `tuner_hal/DESIGN_JA.md` と tv 直下 `開発規則.md` であり、README は履歴・設計判断の正本ではないことを明確化した。

## r50ed6

- WP-02を実施し、`resource_ledger` を単一 `BTreeMap<LedgerId, LedgerEntry>` 正本へ実体化した。
- `reserve`、`commit_live`、`begin_close`、`advance_cleanup_step`、`mark_cleanup_failed`、`commit_close`、`rollback_open`、`quarantine` を型付き遷移として追加した。
- cleanup step を `CleanupStep` enum で保持し、文字列stepを使わない構造にした。
- `FrontendLedger` / `DemuxLedger` / `FilterLedger` / `DvrLedger` / `DescramblerLedger` / `LnbLedger` を type alias ではなく resource kind を保持するwrapperへ変更した。
- begin前commit、terminal後commit、二重rollback、cleanup step、wrapper kind を単体テストで固定した。

## r50ed5

- WP-01を改訂し、既存フォルダ固定を前提にしない階層構造へ変更した。
- `frontend_dvb` / `frontend_px4` のトップレベルcrateを廃止し、`device/src/dvb` / `device/src/px4` / `device/src/runtime` へ集約した。
- `soft_demux` のトップレベルcrateを廃止し、parser断片を `demux/src/parser`、新しいruntime骨格を `demux/src/runtime`、AV共有メモリ骨格を `demux/src/av` へ集約した。
- `descrambler` は `core` と `runtime` を同一module内に置き、`descrambler_runtime` というトップレベル横並びmoduleを作らない構造にした。
- `resource_ledger`、`binder_adapter`、`service_runtime`、`lnb` の新規骨格を追加した。
- `tuner_hal2/DESIGN_JA.md` は引き続き置かない。設計正本は既存 `tuner_hal/DESIGN_JA.md` と tv 直下 `開発規則.md` とする。

## r50ed4

- WP-00後の残骸確認として、test-only の空構造体を状態付きtest helperへ変更した。
- 新規実装計画は外部Markdown `tuner_hal2_static_dynamic_structure_plan_r50ed4.md` を正とし、`tuner_hal2/DESIGN_JA.md` は引き続き置かない。

## r50ed3 WP-00 hygiene

- copied fragment hygiene を実施した。
- `HalError::InvalidArgument(String)` / `InvalidState(String)` / `Internal(String)` を廃止し、型付き kind と表示用 detail に分離した。
- `frontend_dvb` / `frontend_px4` の tune mapping は `HalInvalidArgumentKind` を返すように変更した。
- px4 mapping のテストから `err.contains(...)` によるエラーメッセージ文字列依存を削除した。
- `HalError` の `message: String` 直接保持を `HalErrorDetail` へ寄せた。

## r50ed2 copy-complete-fix

- copy-complete 再確認で見つかった parser 断片内のコピー不要要素を削除した。
- `RecordIndexParser` を空構造体ではなく処理済みpacket数を保持する型へ変更した。
- record index の `TsRecordEventData` 重複フィールドを削除した。
- `packet_pipeline` の診断文字列 `code: &'static str` を `PipelineDiagnosticKind` へ型付き化した。
- PES assembler の drop reason を文字列から `PesDropReason` enum へ型付き化した。
- `TsPacketValidator` 参照を `TsPacketView::validate()` へ修正した。

## r50ed2 copy-complete

- `tuner_hal2/DESIGN_JA.md` を削除した。`tuner_hal2` は新しい設計正本を持たず、既存 `tuner_hal/DESIGN_JA.md` と tv 直下の `開発規則.md` を正とする。
- r50ed2で不足していた DVB explicit scan 境界と DVB tune property mapping 断片を追加した。
- common に POSIX poll/ioctl/read ABI、Japan frequency helper、FrontendScanMode を追加した。
- `Android.bp` に追加断片を反映した。

## r50ed2

- r50ed で未達だったコピー対象断片を追加した。
- `soft_demux` から section / PES / packet pipeline / record index parser 断片を追加した。ただし旧 `DemuxHandle` 制御層はコピーしていない。
- `frontend_dvb` に Linux DVB / earth_pt1 ioctl ABI 断片を追加した。
- `frontend_px4` に px4_drv ioctl ABI 断片を追加した。
- `fmq_shim` の native C++ shim と Rust側 `FmqQueue` wrapper 断片を追加した。Rust wrapper は `NoNativeQueue` と fill/clear の単一variant型を整理してから配置した。
- 旧 `binder_service/src/tuner_hal.rs`、薄いTxn、空分類器、文字列 `contains()` による EventFlag wake 失敗分類はコピーしていない。
- r50ed2 時点でも Binder service / AIDL HAL 実装は未達である。

## r50ed

- `tuner_hal` 隣に `tuner_hal2` を追加した。
- `tuner_hal` 全体のコピーは禁止し、必要な実ロジック断片だけを取り込む方針にした。
- `common`、`descrambler`、`frontend_px4` の再利用断片を追加した。
- 旧制御層をコピーせず、`control` crate に worker / lifecycle / FMQ delivery / stream boundary の型付き骨格を追加した。
- r50ed 時点では Binder service / AIDL HAL 実装は未達である。
