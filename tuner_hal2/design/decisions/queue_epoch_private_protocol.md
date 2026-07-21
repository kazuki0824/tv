# Queue and producer private protocols (v55 repaired v2)

## Scope

Stable Tuner AIDL is unchanged. `QueueEpochProtocol` is DVR-only. `FilterProducerDrainGate` is process-local to Filter/SharedFilter and creates no Binder endpoint, parcelable token or shared-memory control plane.

## FilterProducerDrainGate

State is exactly `Open`, `Draining`, or `Closed`. The gate stores checked `filter_delivery_generation`, `parser_state_generation`, `admitted_producer_count`, and a bounded service-owned pending-event queue. A nonparcelable linear `FilterProducerPermit(g)` is RAII-owned and released exactly once.

### Permit scope and finite drain

1. Blocking backend reads, FMQ waits, parser input accumulation and all external I/O occur before permit acquisition.
2. The permit is acquired immediately before the nonblocking in-memory commit that writes FMQ bytes or enqueues an immutable callback artifact. It may cover only declared object-local locks in the established lock order.
3. It never spans a Binder callback, backend/device I/O, FMQ wait, condition-variable wait, thread join, allocator operation that may block, or acquisition of a service lock needed by flush.
4. Binder invocation consumes an immutable artifact after permit release. A dequeued/in-flight callback is already committed; flush does not cancel or wait for the Binder call. A pending artifact not yet dequeued is unconsumed and may be discarded by flush.
5. Worker exit, panic unwind and cancellation own the RAII guard and therefore release the permit. The service-owned nonblocking critical section gives structural finite drain without an arbitrary timer. Lock poison, owner-terminal failure or evidence of an unfenced holder is a typed invariant failure: the object transitions to `Closed`, waiters wake, and the filter is quarantined.
6. Flush waits without holding any lock that permit release requires.

### Flush

1. Validate descriptor identity and transition `Open -> Draining`.
2. Reject new permits and wake/cancel the service-owned delivery worker.
3. Wait for `admitted_producer_count == 0` under the finite-scope rules above.
4. Prepare an identity-preserving libfmq clear; do not mutate pointers or generations during preparation.
5. Atomically clear unconsumed FMQ bytes and not-yet-dispatched pending event artifacts. Preserve dequeued/in-flight callbacks, callback registration, monitor/hint state, source binding, descriptor identity and all delivered AV allocations.
6. Reset parser/PCR/startId state, increment only `parser_state_generation`, preserve `filter_delivery_generation`, transition `Draining -> Open`, and wake waiters.

A pre-commit drain/identity/clear failure restores `Open` with content, pointers, events and generations unchanged. An impossible partial infrastructure commit is `InfrastructureCorrupt`, closes and quarantines the object, and is never reported as successful rollback.

### Close and owner loss

`Open|Draining -> Closed`; no new permit or event enqueue is admitted. Pending undelivered artifacts are discarded, dequeued/in-flight callbacks remain already committed, waiters wake, and terminal cleanup owns remaining resources. Checked generation exhaustion closes the gate and returns `UNAVAILABLE`; generations are never reused.

## QueueEpochProtocol for DVR

State is exactly `Open(g)`, `Draining(g)` or `Closed`. `beginRead/beginWrite` returns a nonparcelable one-shot token containing queue identity, checked queue epoch, direction and reservation. `commit/cancel` consumes it exactly once. Flush enters `Draining`, rejects new transactions, waits for admitted transactions of epoch g, atomically clears the DVR queue, advances to checked g+1 and returns to `Open`. Failure preserves pointers/content and epoch. Close/owner death closes the identity, makes all tokens stale and wakes waiters. Descriptor replacement closes the old identity and creates a distinct identity at epoch zero.

## Independent axes

`queue_epoch`, `filter_delivery_generation`, and `parser_state_generation` are never aliases or advanced as one bundled generation.
