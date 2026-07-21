# Worker termination and cleanup contract v55

This contract is event-driven. It contains no retry interval, join grace, or terminal millisecond deadline.

## States

`Running(owner_generation)`, `StopSignalled(owner_generation)`, `Completed(report)`, `CleanupPending(dependencies)`, `Quarantined(fenced_generation,reaper_lease)`, `Released`, and `ServiceCritical(witness)`.

## Transition rules

1. Stop/close sends every available cancellation and wake primitive once and records each outcome.
2. If completion is already observable, the caller collects the report, performs all residual cleanup, and releases the lease.
3. A retryable incomplete non-running dependency becomes `CleanupPending`; only repeated close, owner-death supervision, dependency-completion notification, or service reset may resume it. Triggers coalesce by `{owner_kind, owner_id, owner_generation, dependency}`.
4. A worker that is still running is generation-revoked and mutation-fenced before transfer. It becomes `Quarantined` and its join handle is transferred exactly once to `ReaperSupervisor`. The public caller never blocks on join.
5. The worker/resource/LNB endpoint lease remains consumed while CleanupPending or Quarantined. Reaper completion performs residual cleanup and releases the lease exactly once.
6. Reaper capacity is statically bounded by enforced live-worker ceilings. It does not create retry timer jobs; it waits on actual termination/service-reset events.
7. Transfer failure, failure to establish fencing, or a typed witness that the worker can still mutate unfenced global state becomes `ServiceCritical`. A fully fenced owner-local residual cannot shut down unrelated ITuner capabilities.
8. Public operation result preserves the primary operation result; later cleanup failures are returned only where the interface cleanup contract requires them and are always recorded in the typed aggregate cleanup report.

## Filter drain connection

Filter producer permits are short nonblocking RAII scopes and are not reaper-owned worker lifetimes. A delivery worker may be cancelled/woken by flush, but flush waits only for permit release, never for Binder callback completion or an unbounded thread join. A terminal worker failure releases any guard during unwind; lock poison or an unfenced terminal report closes/quarantines the filter.

## LNB connection

LNB logical close uses the same transitions. `LogicalClosed+CleanupPending` allows close only as a recovery retry. `Quarantined` is internal-reaper-owned. The endpoint lease is not returned to `openLnb()` admission until terminal cleanup is complete.
