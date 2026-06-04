# r50eb3 completion check

Base: r50eb2 rev5 static rechecked
Scope: customer-confirmed residuals 2, 6, 9, 10, 12, 14, 15, 17, 18.
Nullable Binder boundary remains future_work/blocker and is not treated as implemented.
Build, Rust unit tests, atest, VTS, and real-device verification are not executed.

## Result

Within the r50eb3 targeted scope, static/source review confirms the previous residuals have been addressed.

| No | Status | Static confirmation |
|---:|---|---|
| 2 | Addressed | `setDemuxSource()` now keeps the descrambler session lock from pending binding through session commit, so a second session-lock failure cannot leave an unrolled pending binding after ledger commit. Ledger failure rolls back pending binding while the lock is held. |
| 6 | Addressed | `setDemuxSource()` rechecks demux generation and demux closed state while holding `demux_record` immediately before descrambler ledger reserve/commit. Demux close cannot pass the same record-owned section while the record guard is held. |
| 9 | Addressed | frontend failure now calls `quarantine_demux_after_stop_tune_boundary_failure()` for each bound demux in addition to runtime I/O fail and stream-boundary execution. |
| 10 | Addressed | normal stream-boundary reset failure now marks runtime I/O failed and attempts demux ledger quarantine, not only runtime I/O failure. |
| 12 | Addressed | frontend close/unbind now commits demux ledger unbind before demux handle state unbind; on ledger/state failure it marks runtime I/O failed and quarantines the demux. No state-only unbind remains on this path. |
| 14 | Addressed | scan now stops previous tune worker and cancels previous scan session before spawning the new scan worker. The previous spawn-before-destroy sequence is removed. |
| 15 | Addressed | scan start signal failure now joins the still-owned local worker handle before any worker-slot transfer. Worker-slot lock failure after start requests stop and joins the local handle. |
| 17 | Addressed | scan session phase read/write now recovers a poisoned mutex with `into_inner()` after recording diagnostics, so terminal phase recording is not silently dropped on mutex poison. |
| 18 | Addressed | unclosed `LnbHal::drop()` now marks the LNB failed/quarantined in the registry before setting the local closed flag and clearing callback. Drop still does not perform normal backend reset. |

## Grep checks

- No `mark_live_path_failed` function is present in production code.
- `setDemuxSource()` contains demux generation and demux closed rechecks before ledger commit.
- `StreamBoundaryResetQuarantineFailed` diagnostic is present on normal boundary reset failure.
- frontend failure path records `quarantine_failed` diagnostic when demux quarantine fails.
- scan has `destroy the previous tune/scan state before spawning the new scan worker` marker.
- LNB Drop contains `lnb_drop_unclosed_resource` quarantine marker.
