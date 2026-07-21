# Canonical rule registry

This is the current canonical registry. Release identity is recorded in `release.json`.

## CD-832bb65be403 — DP-001

SHA-256: `832bb65be4036100a18ee6957ed2540195d77a21159c50575a8b04e1e5ddb7eb`

同一demuxに属するlive PCR filterの有効AV sync IDでは、PCR未観測でも`getAvSyncTime()`を成功させ、値は`Tuner.INVALID_TIMESTAMP`を返す。最初の有効PCR観測後は90kHz系の最新観測timestampを返す。foreign/non-PCR/closed/unknown IDは`INVALID_ARGUMENT`とする。値0を未観測sentinelとして公開しない。

## CD-c126cb6caa91 — DP-002

SHA-256: `c126cb6caa9139f51025e43670a12e94ef7134b6375e736de8adf84c563af30c`

Stable Tuner AIDL is unchanged. DVR playback and record alone use the process-local QueueEpochProtocol because their FMQ begin/commit reservations can span a public flush boundary. Filter and SharedFilter use the HAL-internal FilterProducerDrainGate. Its state is Open, Draining or Closed and every FMQ write or pending-event enqueue requires a nonparcelable linear RAII permit with checked filter_delivery_generation. A permit is acquired only after blocking backend read, FMQ wait and parser staging and immediately before a nonblocking in-memory commit; it never spans Binder callback, backend/device I/O, FMQ/condition wait, thread join, fallible blocking allocation or an out-of-order service lock. Binder invocation consumes an immutable artifact after permit release. Draining rejects new permits, wakes the service-owned worker and waits without holding locks required by permit release. The finite permit set is structural: worker exit/panic/cancellation releases the guard; lock poison, owner-terminal failure or an unfenced holder closes and quarantines the filter. Successful flush clears unconsumed FMQ bytes and not-yet-dispatched event artifacts, resets parser/PCR/startId state, increments parser_state_generation and preserves filter_delivery_generation, descriptor identity, source/callback/monitor/hint state, already dequeued/in-flight callbacks and delivered AV allocations. Pre-commit failure preserves content, pointers, events and generations; impossible partial infrastructure commit is InfrastructureCorrupt and quarantines. Close/owner loss closes the gate and wakes waiters. queue_epoch, filter_delivery_generation and parser_state_generation remain independent.

## CD-23d2e1c35c4f — DP-003

SHA-256: `23d2e1c35c4f311d08b0831799b87de0c9fbf4ce3d30b1ee7cf7b0cd5c815a52`

Public close semantics use one interface×logical-lifecycle×cleanup-state table. A first close on a Live object commits LogicalClosed before all-attempt cleanup and rejects every non-recovery method. `IFrontend.close()` and `ILnb.close()` may be called more than once: LogicalClosed+CleanupComplete returns SUCCESS without rerunning completed cleanup. `IDvr.close()` and `IFilter.close()` on LogicalClosed+CleanupComplete return INVALID_STATE; IDvr's other methods also fail, and IFilter late `releaseAvHandle()` remains a separate release-ledger operation. For every interface, LogicalClosed+CleanupPending exposes close only as a recovery retry: it runs only pending cleanup steps, returns SUCCESS only when they complete, otherwise returns the operation-specific cleanup failure and remains CleanupPending. Quarantined rejects public close with INVALID_STATE and is serviced only by internal cleanup/reaper authority. LogicalClosed, CleanupPending, CleanupComplete and Quarantined are orthogonal axes.

## CD-7dce44077973 — DP-004

SHA-256: `7dce44077973ef1f58e0908288849da77aa708d70d716228971f6615de35a4fc`

configure()はsource bindingを変更しない。新settingsが既存bindingと非互換ならINVALID_STATEで拒否し、旧settings/bindingを保持する。切断はsetDataSource(null)で明示する。malformed settingsはINVALID_ARGUMENT。

## CD-784341f0278c — DP-005

SHA-256: `784341f0278cd3509f3b223168fbbdc066f6f5ae4f4e9b43a717c38f1e3e47a5`

payload planeとmonitor mask/event planeを分離し、対応profileでは初回状態と変化eventをcallback配送。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-5f092381c515 — DP-006

SHA-256: `5f092381c5159ca90120ac41723b7cce0d408ede181bf19d13c0ccc470e29f21`

Filter normal-FMQ payload, DVR record stream, and TS/MMTP record callback metadata are three distinct planes. TS/MMTP record filters do not expose a normal filter FMQ. Their payload is written only to the attached Record DVR FMQ, while PID/index/byte-number/PTS/start-code metadata is delivered by DemuxFilterTsRecordEvent/DemuxFilterMmtpRecordEvent callbacks. Section, PES and raw TS payload filters may use the normal filter FMQ according to their subtype table.

## CD-f6050e1fda11 — DP-007

SHA-256: `f6050e1fda110a1513e76948e81a8be0a04c2b6f378e995b37d9b2df29fc2e59`

IFilter.configureAvStreamType() is valid only for an open audio/video filter. In OpenUnconfigured or ConfiguredStopped state it returns SUCCESS and atomically replaces the AV stream-type hint; repeating the same value is a SUCCESS no-op. In Started state it returns INVALID_STATE and changes no state, source binding, backing, dataId or queue generation. A non-AV filter returns INVALID_ARGUMENT. A logically closed filter returns INVALID_STATE; closed-state precedence applies even when runtime_failed is also true.

## CD-715d65f37498 — DP-008

SHA-256: `715d65f374989e0fdbfe78b14316ec684392c1628068924e41c4ad8eb3c847d0`

open済みAV filterではconfigure前でも成功。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-18477953ae56 — DP-009

SHA-256: `18477953ae56f423402ebdda9174b6cb3e27871d7c003b21728a3f1cc52d4379`

maskを0へcommitし監視停止・再設定時初回通知を定義。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-f2a57e6a5c98 — DP-010

SHA-256: `f2a57e6a5c983dbf86f4f9c9762dcc09fdfa491f46e87a22f50d1095476fec34`

`releaseAvHandle()` is classified only by `decisions/av_release_state_matrix.csv`, which covers both shared-arena and event-local-FD `MediaEvent` modes. Negative dataId is `INVALID_ARGUMENT`. `empty handle + 0` is a success no-op. A returned shared handle + 0 releases only the client shared-handle lease; a known duplicate finalization is success no-op, while foreign/mismatched identity is `INVALID_ARGUMENT`. `empty handle + positive dataId` releases a matching active shared or event-local allocation and is success no-op for a known already-released issued ID. An event-local fd-bearing handle + matching positive dataId releases that event-local allocation; fd-bearing + 0 closes only the received event-handle lease when the allocation is retained by another framework reference. Unknown/never-issued/foreign/mismatched tuples are `INVALID_ARGUMENT`. Registry or fstat classification failure is `UNKNOWN_ERROR` with no uncertain free/reassignment. Release remains available after logical close for issued allocation identities; quarantined cleanup is internal only.

## CD-1a3afe124d5f — DP-011

SHA-256: `1a3afe124d5f4b50ba5c81f913f4e1bc0585b2bcb6db44808bdf731796d40949`

open済み未configureを有効source/sinkへ含め、全広告pairでVTS SetFilterLinkage同等試験を通す。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-e3aff2aeb4fa — DP-012

SHA-256: `e3aff2aeb4fa741a85e7914deaa9208ced4328eaf233e2611e01f980fc284854`

open済みrecord/playback DVRではconfigure前も同一queue descriptorを返す。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-6484d4ea4ac8 — DP-013

SHA-256: `6484d4ea4ac82103dd64bbb634a931b74fa186e8777932948730679f5e384fbc`

record DVRの`flush()`はstarted中`INVALID_STATE`、stopped/configured中は成功とする。playback DVRの`flush()`はstarted中も成功し、未読inputを既存queue上で破棄する。record/playbackを別セルへ分離する。

## CD-a0871e161a83 — DP-014

SHA-256: `a0871e161a830b516dada929af55a4b9378775d267de0fd3aa53830e2b0b5dd7`

read/writeをSDK/JNI wrapper契約へ移し、playback readはsource→playback FMQ、record writeはrecord FMQ→destinationとしてbyte countで定義する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-d4d379f3a2ab — DP-015

SHA-256: `d4d379f3a2abff507048617c9090d79d070f7d3df564f36f20a7dd79c7a2b2e3`

Section/PES processing has two independent planes. Envelope extraction proves a complete bounded block without reading outside the TS/PES/section length; semantic validation proves metadata fields are meaningful. For a raw filter, an envelope-extractable block is enqueued byte-for-byte even when semantic validation fails; no Section/Pes semantic event is emitted and a typed malformed diagnostic is recorded. For a non-raw filter, both byte delivery and semantic event require semantic validation. If the envelope is incomplete, length-impossible, over configured bound, or cannot be delimited, neither plane delivers data. No path fabricates tableId, version, streamId, PTS/DTS or dataLength metadata. Raw byte delivery and semantic event delivery have separate counters and acceptance tests.

## CD-0a54541fd508 — DP-016

SHA-256: `0a54541fd5087efefaadf7053e2086b361c558ca9b3ee59e745330ca1513aa13`

pending-undelivered dataとdelivered/in-use slotを分け、後者はreleaseAvHandleまで保持する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-ad630d6e167a — DP-018

SHA-256: `ad630d6e167aea8bc959988f0678f57f7491378a7f56fc565026efb97d44e669`

open時生成・close時破棄へ統一し、flushは内容clear、configureはsettings更新に限定する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-d1438a6d3709 — DP-019

SHA-256: `d1438a6d37091aec5f9163870da323a82309e9d86dfd1dccc533ef2042b11c52`

configureのkind変更分岐を削除し、open時kindと異なるsettings unionはINVALID_ARGUMENTにする。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-49b9ecfb3112 — DP-020

SHA-256: `49b9ecfb3112f21e922858a8dc9428e28d28778837c8d3a8fd70ff779a11b2e3`

WorkerPanic、JoinFailure、StopWakeFailure、EventFlagWakeFailureを別variantへ分ける。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-780d0b462c25 — DP-021

SHA-256: `780d0b462c259e1f0ec2778834957e3da5f1e7ad764b4579a5e78552710fb44f`

public Result表を固定する。不存在/foreign ID=INVALID_ARGUMENT、closed local object=INVALID_STATE、capacity/resource absence=UNAVAILABLE、未初期化dependency=NOT_INITIALIZED、内部不整合/破損=UNKNOWN_ERROR。phantom objectや自動quarantineは作らない。

## CD-538d0251a7a1 — DP-022

SHA-256: `538d0251a7a18b718ebe32c84ac1aa1b141e660f6e877422a76b0378036396a4`

Every queue/device/packet read outcome is classified by `decisions/failure_scope_taxonomy.csv`. Nonblocking empty/WouldBlock is `NoData`; EINTR is `Interrupted` and retries without state change; explicit stop/owned EOF is `Closed`. `InfrastructureCorrupt` is limited to FMQ descriptor/control/transaction invariants and quarantines the affected path. A malformed 188-byte TS packet is packet-local drop and typed diagnostic, not infrastructure corruption. TEI is preserved on raw/record output and excluded from semantic assembly. Continuity discontinuity preserves raw/record bytes and resets only PID-local semantic assemblers. Section/PES parse failure drops the semantic unit and restarts at a legal boundary. Permanent owned I/O failure terminates only the affected runtime unless a typed witness proves unfenced global mutation. No Corrupt/Fatal branch is silently mapped to NoData.

## CD-6a647f1fda89 — DP-023

SHA-256: `6a647f1fda89fe2ee27792865c5189c6c178e9acc8384e8a9b41777a3aca8e6c`

Capability publication is derived from one immutable CapabilitySnapshot selected after device probing. F=successful_frontend_count and L=successful_lnb_count are fixed first. The ordered runtime candidates C8, C4, C2 and C1 are enumerated in decisions/capability_snapshot_candidates.csv with numeric demux/filter/DVR/AV values and exact F/L formulas. For each candidate the service provisionally reserves the complete runtime vector, rolls back the whole vector on any component failure and commits the largest successful candidate exactly once. C1 is mandatory for ITuner publication and contains one audio plus one video AV filter, av_filter_count=2, av_ledger_entries_total=16 and av_reserved_bytes_total=16777216. The committed snapshot is the sole authority for getDemuxCaps(), admission, cleanup accounting and terminal release. VTS is not an unconditional part of C1: the AOSP branch, frontend source, tune parameters/PIDs, enabled flows, Filter/DVR buffer sizes and product memory budget form a pre-start VtsEnvironmentProfile. Until declared, VTS is DESIGN_HOLD_VTS_ENVIRONMENT_UNDECLARED, no default V1 XML is installed and no VTS-success claim is made. A bound static variant must fit C1 and atomically reserve its exact queue-byte vector before service/VTS startup.

## CD-f90c09663c36 — DP-024

SHA-256: `f90c09663c3613a39b0ead0b7da80985ecdd33840eeb63d6be839c178fd79ddb`

PCR等の実行状態とmonitor mask/event配送状態を別軸にする。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-2a23a0328beb — DP-025

SHA-256: `2a23a0328beb44ad51f45c84d1ef6f46d71d2c24dd4d2181d3599a9071800fc1`

queue非公開とcallback event無効を分離する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-ff9722480885 — DP-026

SHA-256: `ff97224808851efa8852ecc287bdf3a2ab11070d97bc224a9950ba5a16179e26`

Reset semantics use independent filter_delivery_generation and parser_state_generation axes; DVR queues additionally use queue_epoch. configure() success increments filter_delivery_generation and parser_state_generation, resets parser/PCR/startId state, and preserves queue backing/identity, source binding, callback, monitor mask and hints unless the configure contract explicitly changes them. Filter/SharedFilter flush enters FilterProducerDrainGate Draining, rejects new linear permits, wakes the service-owned worker and waits for the finite nonblocking permit set without holding locks needed by release. Permit scope begins only after blocking read/wait/staging and ends after FMQ commit or pending-event enqueue; Binder callbacks and external I/O are outside it. Flush then atomically discards unconsumed FMQ bytes and not-yet-dispatched event entries, preserves dequeued/in-flight callbacks and delivered AV allocations, resets parser state, increments only parser_state_generation and returns Open. Any failure before clear commit leaves pointers, content, pending events and all generations unchanged; poison or impossible partial commit closes/quarantines. DVR flush follows QueueEpochProtocol and advances only queue_epoch after its begin/commit transaction fence drains. stop() preserves queue bytes and identity while discarding partial parser state; source replacement increments both filter and parser generations at one atomic boundary; close fences all axes.

## CD-ca5c89902839 — DP-027

SHA-256: `ca5c899028393ffe1bff1f70eb6161066cb447adf1d0979d53e5d8fd6512b01a`

current generationから切り離してもretired backingとrelease台帳を保持し、UAF/slot再利用衝突を防ぐ。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-a3ed070a2132 — DP-028

SHA-256: `a3ed070a2132636ae3eda9ce232202c69646d89939723c8c7805c1f09e000fc3`

The FMQ table is subtype-specific. Section/PES/raw-TS payload filters use the normal filter FMQ. TS/MMTP record filters have no normal filter FMQ: payload goes to Record DVR FMQ and indexing metadata goes to callback events. Audio/Video media filters use AV shared memory plus MediaEvent, not normal FMQ. PCR/monitor/startId and other callback-only events have no payload FMQ. Record DVR owns record FMQ and Playback DVR owns playback FMQ. Valid-but-unsupported subtypes return UNAVAILABLE at openFilter.

## CD-c175c4d6b7f4 — DP-029

SHA-256: `c175c4d6b7f4f38f124d53f2bee5502dd31cb58fae2a873b7fedcd7caef7bca4`

decisions/av_allocation_profile.csv and decisions/av_release_state_matrix.csv are the sole AV SSOTs. Shared and exact-size event-local transport use one ledger per AV filter generation with a resource-safety ceiling of 8 live entries and 8 MiB. The service reserves snapshot.av_filter_count times that budget during CapabilitySnapshot selection; C1 has two AV filters and therefore reserves 16 entries and 16777216 bytes. Shared slots and event-local descriptors consume the same per-filter ledger. Allocation is allowed only when an entry is free, request_bytes <= 8388608 and the exact allocation fits remaining bytes. Oversize, exhaustion or allocator failure is rejected before callback/dataId publication; only that event is dropped and no live allocation is evicted. avDataId is positive signed-63-bit and never reused. Flush, reconfigure and logical close retain delivered allocations as ReleaseOnly. Active/ReleaseOnly release succeeds once; known finalized IDs are success no-op; unknown, foreign or tuple mismatch is INVALID_ARGUMENT; registry uncertainty is UNKNOWN_ERROR with storage fencing.

## CD-fc8b02c41794 — DP-030

SHA-256: `fc8b02c417946c297596a58d911fe24b8c1a4da8aba2dca82b7d3c14975e0863`

flush対象からmonitor mask、callback registration、PCR identityを除外して明記する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-da05e7b16091 — DP-031

SHA-256: `da05e7b16091d99e98a600f94ab99645feac80e597e09c15f0cf885211306c09`

All time hints are signed milliseconds. Negative is INVALID_ARGUMENT; zero disables/resets the hint; every positive value is accepted if conversion to the internal duration is representable. Checked conversion overflow is INVALID_ARGUMENT. No arbitrary ProductProfile maximum is defined; internal counters use saturating arithmetic and never reverse a committed public result.

## CD-93c614a55615 — DP-032

SHA-256: `93c614a55615010d11542e80e21cc9e48b8e3ac1e8507671fa88a5178a294d43`

size/count/offset入力は負値=INVALID_ARGUMENT。0はAPIごとの明示意味に限定し、bufferSize=0とread/write size=0はINVALID_ARGUMENT、offset=0は有効、status interval=0は既定値復帰。size+offset overflowまたはusize変換不能=INVALID_ARGUMENT、allocation不能=OUT_OF_MEMORY。

## CD-14dbc0361d74 — DP-035

SHA-256: `14dbc0361d748e0f4bde036b31d98979a75bae8ae42de68a03b72c98c6e8e74d`

Backing identity validation precedes the release-state lookup in `decisions/av_release_state_matrix.csv`. A duplicated fd is accepted when the complete backing tuple matches; fd number is never identity. A generation mismatch with a known unreleased delivered token classifies as ReleaseOnly, not INVALID_STATE. Tuple mismatch or foreign handle classifies as UnknownOrForeign/INVALID_ARGUMENT. Internal fstat/registry failure returns UNKNOWN_ERROR and quarantines the affected registry without freeing uncertain memory.

## CD-18392effc3b5 — DP-036

SHA-256: `18392effc3b54162dee08057fdc30a55d2e8df1b4ada9a6ee5b08768a6c2c519`

Typed error mapping is: InvalidInput/Range/ForeignObject=INVALID_ARGUMENT; WrongLifecycle/Closed/AlreadyActive=INVALID_STATE; MissingResource/Busy/Capacity/UnsupportedValidInput=UNAVAILABLE; DependencyNotInitialized=NOT_INITIALIZED; AllocatorFailure=OUT_OF_MEMORY; Io/Permission/Corruption/InvariantViolation=UNKNOWN_ERROR. This table applies only where the interface-specific method contract does not override it. In particular, repeated `IFrontend.close()`/`ILnb.close()` use DP-003 SUCCESS semantics, while DVR/Filter repeated close use DP-003 INVALID_STATE semantics.

## CD-61d48d942c35 — DP-037

SHA-256: `61d48d942c3587ce22a7b1f1648a8284775709e1ef5943b70239f804faf5ae63`

Object lifecycle has independent public_closed and runtime_failed axes; cleanup_pending is a third internal axis. Normal (false,false) permits the interface methods. runtime_failed only permits diagnostics/snapshot and close; mutating or data methods return UNKNOWN_ERROR without mutation. public_closed permits only the interface-specific idempotent close contract; all other methods return INVALID_STATE. When both axes are true, public_closed has precedence for non-close methods and close remains interface-specific. Closing the public surface does not falsely claim cleanup completion; cleanup_pending may continue under the service cleanup supervisor.

## CD-a3b8c049b896 — DP-039

SHA-256: `a3b8c049b896373d46dbb3f3a86631ef5cc7e615348282fdb1188647d1e59754`

QueueFull/Backpressureは非破損としてpublic methodではUNAVAILABLE、running worker内ではstatus/counterだけ更新する。DescriptorMismatch/PointerCorruption/ImpossibleRegionはUNKNOWN_ERRORとし当該queueだけquarantine。service全体は閉鎖しない。

## CD-b6feea518693 — DP-040

SHA-256: `b6feea51869343a4a586ef1f7be658286a7f0b677d16a2b880645ea21f9f4595`

CleanupPending is owner-local, dependency-typed and event-driven under `decisions/worker_termination_contract.md`; it contains no normative millisecond schedule. The initiating operation attempts every immediately available cleanup step once. Completed dependencies release their leases. Retryable incomplete non-running dependencies remain CleanupPending and resume only on repeated close, owner-death supervision, dependency-completion notification or service reset, coalesced by owner/generation/dependency. A still-running worker is generation-revoked and fenced before one-time transfer to the bounded ReaperSupervisor and immediate quarantine; the public caller never waits on join. Leases remain consumed until actual termination and residual cleanup. Transfer/fencing failure or typed unfenced-global-mutation witness is service-critical; a fully fenced owner-local residual cannot stop unrelated ITuner capabilities. Public results preserve primary operation precedence and typed aggregate cleanup evidence.

## CD-286d4d848914 — DP-041

SHA-256: `286d4d848914891c35e4939057f95b36d3aff437a39872b9627480667dc2bfd1`

EINTR is retried while stop/cancel is not set and the existing operation deadline has not expired. There is no retry-count parameter. Cancellation returns the typed Cancelled outcome; deadline expiry returns Timeout; fatal wait errors retain errno in diagnostics and map through the method result table.

## CD-007dd3e15a9e — DP-043

SHA-256: `007dd3e15a9ed43ccfe1936648933a304f1ce8ba22c6e3bc0031cf34549ff976`

hardware state unknownとfrontend operational stateを分離する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-bb0b7b1493e9 — DP-044

SHA-256: `bb0b7b1493e9564fa46d0ac9d93ca6d15727f75e59edaa468c34815586c048cf`

record data/eventの経路を分離して明記する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-5b813c7ca8ac — DP-045

SHA-256: `5b813c7ca8acd2606c28f9dd89f1571cff3daffd54ef6f2c48be0822d9b54a2a`

Record DVR attach/detach表を固定する。duplicate attach=SUCCESS no-op、未attach detach=INVALID_STATE、foreign/wrong-demux/wrong-kind/playback DVRへのattach=INVALID_ARGUMENT、attachment capacity=UNAVAILABLE、backend failure=UNKNOWN_ERROR。attach順序は結果に影響しない。

## CD-dc79dedcdd71 — DP-046

SHA-256: `dc79dedcdd712ab27029687eb952c8fb9a53b5f65aaffb40568ddf8d86e85a1f`

malformed/range違反はINVALID_ARGUMENT、構文上validだが当該frontend/profileが非対応ならUNAVAILABLE。例: 負周波数/不正enum/selector型不一致=INVALID_ARGUMENT、対応外delivery system/帯域/機能=UNAVAILABLE。

## CD-af4d5a96cfa9 — DP-047

SHA-256: `af4d5a96cfa92b66f3c29690ac3e89188f2147eb0160fffe92c4e3cbb626232a`

wake generation overflow時は当該workerだけを停止し、新しいepochを持つworkerへ再生成する。worker再生成に失敗した場合だけownerをquarantineし、generation overflowだけでowner全体をfailedにしない。

## CD-94ebd39c8e98 — DP-048

SHA-256: `94ebd39c8e98cbeb59401c136a658ed5bfcee8e56a55c65c0a0eb15cd9b2bb68`

diagnostic counterのsaturation/dropは、diagnostic取得APIを除く全business APIの戻り値を変更しない。例外は設けない。

## CD-ca940c603a45 — DP-049

SHA-256: `ca940c603a457da99f7eecefa9703f063aa01453b223d5e69016bd97eb0ac539`

terminal reasonとcallback delivery outcomeを別軸にする。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-87d41c6451bf — DP-050

SHA-256: `87d41c6451bfeaed95bb208df5d79c7a32aa2e2210c38443aec3d28ee9761839`

owner-lossで非blocking cleanupを起動しreaperへ委譲する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-f8549eabc707 — DP-051

SHA-256: `f8549eabc70705df8f7d5fc032cef634e6060b5de4c56fd4352b3462191643db`

STREAM_IDとRELATIVE_STREAM_NUMBERを別validationにし、absolute 0..11特別拒否を削除する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-bda34cbad6d1 — DP-052

SHA-256: `bda34cbad6d195b4dbf94cab7a6458ad10230a1476f12a83f888b71d297323e4`

Base selector matrix: Linux DVB accepts ISDB-S STREAM_ID values 0..65534 and passes them unchanged to DTV_STREAM_ID; 65535 (`Constant.INVALID_STREAM_ID`) is rejected. Legacy unmodified px4 accepts RELATIVE_STREAM_NUMBER 0..7 only. The target `kazuki0824/px4_drv` `feat/android-ddk` backend advertises only selector modes that are release-eligible in the exact SupportedDeviceCapabilityCatalog entry; the current catalog enables RELATIVE_STREAM_NUMBER and does not enable STREAM_ID because values 0..11 collide with relative-slot semantics. Empty, unmatched, or selector-ineligible entries advertise no ISDB-S selector. ISDB-T/CATV/CS110 use no ISDB-S selector.

## CD-877bdf6adf13 — DP-053

SHA-256: `877bdf6adf131b6081454356fe05831728a7e3eeccfe0da01895cde390082e03`

selector typeを正として判定し、値域推測を廃止する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-5cdd4307b4a3 — DP-054

SHA-256: `5cdd4307b4a39b2c192221d072fd433e383e0c5d82cea85eafc92cb14816ace7`

Tuner HAL owns generic MPEG-TS section transport only: TS payload extraction, section framing, declared-length enforcement, optional CRC checking, filter matching, queue/FMQ delivery, and transport diagnostics. It must not perform table-specific semantic parsing, normalization, cross-section aggregation, database updates, or semantic-object construction for any PSI/SI table. This applies uniformly to PAT, CAT, PMT, NIT, SDT, BAT, EIT, TDT, TOT, BIT, NBIT, LDT, CDT, PCAT, SDTT, AIT, AMT, and other standard-defined, private, reserved, or future table IDs. A client such as TIS configures the generic section filter and owns table semantics above the HAL boundary; a reusable SI parser library may be used only in that client layer, not as Tuner-HAL policy. For every matched section, Tuner HAL either delivers the complete generic section and metadata under the configured filter contract or reports a generic transport/framing/CRC failure; it must not silently discard a matched EIT, TOT, AMT, or other PSI/SI section merely because HAL does not understand its semantics. The closed registry records syntax bounds and the semantic owner above HAL for each table_id/range. Registered 1021-class tables have section_length at most 1021 and total section size at most 1024; registered extended-class tables have section_length at most 4093 and total section size at most 4096. Reserved, unassigned, private, and externally owned IDs are never inferred as typed ARIB SI by Tuner HAL, but still remain eligible for generic raw-section delivery when selected by a valid client filter. This ownership and transport boundary is recorded in decisions/arib_table_section_length_registry.csv and decisions/psi_si_semantic_ownership.csv.

## CD-269afeb9ba9e — DP-058

SHA-256: `269afeb9ba9ea0444d4e482cfab9052d70b7be3fccfd744160d80913188593be`

delivered-in-use slotはreleaseAvHandleまで保持する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-e4bc78b405ad — DP-059

SHA-256: `e4bc78b405adede5e78c1ffe415244a7a49539fa94fb0f588e79ae3769ee7c1d`

複数demux boundaryで一部成功した場合、public operationはerrorを返す。commit済みdemuxは新generationで継続し、失敗demuxはmutation前失敗なら旧状態を維持、mutation後で実状態不明ならそのdemuxだけquarantineする。依存childへのfailure波及も失敗demux配下だけに限定する。

## CD-c4a1397ad849 — DP-060

SHA-256: `c4a1397ad849219313612b7c0d7079459af388c2bf156e4714d1b33d548bf07a`

全demux失敗時も一律quarantineしない。各demuxをstep outcomeで判定し、precondition/prepare失敗は旧状態維持、mutation後の実状態不明だけをquarantineする。frontendはoperation failureを返すが、健全な旧generationを保持できるdemuxは再試行可能とする。

## CD-c11d68a79f3d — DP-061

SHA-256: `c11d68a79f3dd60e0d785ceeeaba78b194b38fdc7d8058789b677ebc762c11a6`

retryable lock failureとregistry corruptionを分離する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-6b38a338ba5e — DP-062

SHA-256: `6b38a338ba5e79f521331a746b486954f631003bf62f23d73e4771b906b8a62a`

boundary transaction内部失敗では、commit済みdemuxを維持し、未処理demuxは未変更のまま再試行対象へ残す。quarantineはmutation開始後にcompletion不明となったdemuxだけに限定し、未処理対象を自動quarantineしない。

## CD-e09f67bd54a2 — DP-063

SHA-256: `e09f67bd54a2f90feb5ab90d6a1c72614918d33efe78797edc46ba5f7119f769`

IDvr has no AIDL read/write methods. Remove read/write from every AIDL lifecycle/result/worker table. Describe SDK/JNI beginRead/commitRead and beginWrite/commitWrite byte-count helpers in a separate DVR FMQ data-plane section, with static AIDL-surface and integration cases.

## CD-2f9cb4c25252 — DP-064

SHA-256: `2f9cb4c25252c62f49e4f27f494dbeb38607e676bfa7ab01ba01f356b2309d02`

DP-004と同一規則へ統合する。非互換reconfigureはINVALID_STATEで旧settings/bindingを保持し、切断はsetDataSource(null)またはsource closeだけ。

## CD-f0bf306d0422 — DP-065

SHA-256: `f0bf306d04222d1984afd5381894cc7663a1507fd899c2c37ef0fe650b324fa3`

PES assembly is scoped per PID. For PES_packet_length > 0, completion occurs only after exactly the declared number of PES bytes has been collected; an early same-PID PUSI is corruption and the incomplete bounded PES is discarded. For PES_packet_length == 0, completion occurs only immediately before a later same-PID TS payload whose payload_unit_start_indicator is 1 and whose payload begins with a structurally valid 0x000001 PES start-code prefix and minimally valid PES header. The boundary packet starts the next PES and is never appended to the previous PES. A 0x000001 elementary-stream start code occurring without a same-PID PUSI and valid PES header never terminates the current PES. A same-PID PUSI without a structurally valid PES start/header is transport corruption: discard the incomplete current PES, record a typed diagnostic, and do not emit it as complete. A PUSI on another PID has no effect. TEI, continuity discontinuity, flush, stop, or close each independently discards any incomplete PES and records the corresponding typed diagnostic; none emits a complete PES. The ARIB STD-B32/H.222.0 locators and the verified bounded/unbounded claims are recorded in decisions/arib_normative_clause_fingerprints.csv.

## CD-bb476146a101 — DP-066

SHA-256: `bb476146a101ed23dbc909a619ff9b815af78449ad7b166c66821f78f7aa85e7`

closeとowner-lossが同じcleanup正本へ入るよう修正する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-b3bc6ffe7012 — DP-067

SHA-256: `b3bc6ffe7012b91ed1fd75418326aef15cded3859369dcf381bc400177a27b36`

Every deferred cleanup job is keyed by owner_id, owner_generation, dependency_kind and dependency_id. States are Queued, Running, WaitingForTrigger, Released, Quarantined and Complete. Duplicate enqueue coalesces. No timer, retry offset, TTL, deadline or acknowledgement protocol exists. Cleanup is attempted on enqueue and on explicit lifecycle triggers only. Success releases then completes. A retryable failure moves to WaitingForTrigger while retaining the lease. An unfenced or indeterminate dependency moves to Quarantined while retaining the lease; completion notification may later resume residual cleanup. Owner death transfers the linear authority to the service cleanup supervisor. The maximum number of queued/running/quarantined jobs is bounded by the same advertised object/worker ceilings.

## CD-6c1cdb71a895 — DP-068

SHA-256: `6c1cdb71a895e18eaccf0192e5638c0e8610d61c16469d3e857a72f5e4140d5a`

owner-loss cleanupを追加する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-755c21bee4a2 — DP-069

SHA-256: `755c21bee4a2da79de46c3d428d410110c84238ad75d071243c0eb75b8a0ddb8`

worker failure後の`flush()`はpending-undelivered payloadとparser partialだけを破棄する。FMQ descriptor/backing、monitor設定、delivered AV slotは維持する。clear失敗時はruntime_failedへ移し、close/reaperだけを許可する。

## CD-acc8220a0d1d — DP-071

SHA-256: `acc8220a0d1dd6fb62f3b05b863c142e16269dc27cf1e69f0d663f8081fd18d7`

close/unregister失敗は`cleanup_pending`へ記録して再試行可能にする。quarantineはdevice/queue/registryへmutation済みで実状態を確定できない場合だけに限定し、その他のcleanup failureを一律quarantineしない。

## CD-45e91720cae1 — DP-072

SHA-256: `45e91720cae19064dad1943c16d049d641cefa0ab69b3fff610fd84d2b8b08ec`

child unregister/closeの未完了stepはdependency別`cleanup_pending`へ保存する。quarantineは共有state corruptionまたはmutation結果不明の対象だけに適用し、通常のremove failureは再試行対象とする。

## CD-ed2607ed14ad — DP-073

SHA-256: `ed2607ed14ad713105a0e4b8c3d52d14cfcee3294d9b7ca284c231902772afff`

新規配送停止とclient-held backing寿命を分離する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-4fe2b10cfb21 — DP-074

SHA-256: `4fe2b10cfb2119d711752b29dd0abff281e3a113e4fa67516365d97fc6863720`

pendingとdelivered-in-useを分離しactiveはrelease時のみfree化する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-9828932a3560 — DP-076

SHA-256: `9828932a3560764378d153fd199bae9d70d5e3882b0da91c6c09d9b3d6ad83e3`

Resource lifetime is explicit and dual-mode. FMQ descriptor/backing is queue-runtime-owned, survives flush and is released only after logical close plus zero admitted transactions. AV shared backing is filter-generation-owned and event-local backing is allocation-owned. Delivered avDataId allocations remain client-held across flush, reconfigure and logical close until releaseAvHandle() or internal terminal quarantine. Each AV filter generation owns one 8-entry/8-MiB ledger; service reservation is the sum across snapshot.av_filter_count, so C1 reserves 16 entries and 16 MiB for its audio and video filters. Oversize, exhaustion and allocation failure occur before callback/dataId publication and never evict a live allocation. Worker handles remain service-worker-store/reaper-owned until actual termination. Queue epoch, filter delivery generation and parser generation reset logical state but never destroy exported/client-held backing.

## CD-e76e00542742 — DP-077

SHA-256: `e76e005427422acc2fd7c4f03585986608738812bf2d32cd0346c849eee6fe8c`

lock観測不能ならDEMOD_LOCKをadvertise/true化せず、別の内部TuneSubmitted状態にする。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-f4c611431b0b — DP-078

SHA-256: `f4c611431b0b9c52dfbe9b2d7bdddc0c1a7903b4fa32033f21d37f46ef255c2f`

Frontend existence/capability is derived only from the device/driver catalog and is never suppressed because a measured lock timeout is absent. Tune is asynchronous: a successful backend tune request remains active until lock or terminal status callback, explicit stop, retune, close or backend fatal failure. No fixed elapsed-time deadline may reverse a successful tune or remove frontend advertisement. A service diagnostic may report elapsed-lock-time threshold crossings, but it cannot alter the public Result, capability or state. Any backend operation deadline used solely to prevent a stuck ioctl/read is an internal bounded-I/O policy, not a device capability.

## CD-1b216b960772 — DP-079

SHA-256: `1b216b96077292474264c1033f1182414767d456b2c4c5ce3dd42f655df366c9`

DvrLeasePool is a view of the committed immutable CapabilitySnapshot and is the sole source for getDemuxCaps and openDvr admission. Global playback/record counts are snapshot.playback_count and snapshot.record_count; per-demux limits are one playback and one record DVR. Admission validates lifecycle/arguments, atomically reserves direction and per-demux leases, then prepares caller-requested FMQ and the exact notifier slot budget. Failure rolls back every provisional lease and publishes no partial object. CleanupPending/Quarantined objects remain counted until terminal release. Tuner VTS is environment-bound rather than a default C1 promise: until a pre-start VtsEnvironmentProfile declares source, PIDs, flows, queue sizes and memory budget, VTS is DESIGN_HOLD and no XML is installed. A bound static variant must fit C1 and reserve its exact queue-byte demand.

## CD-fa92b03abef6 — DP-080

SHA-256: `fa92b03abef6b77557e1947139bd7927aeb3f27e99ac51e7799675e3c9099ff6`

DVR concurrency is defined by the committed CapabilitySnapshot: P=snapshot.playback_count, R=snapshot.record_count, P_d=1 and R_d=1. A scenario is admitted only when its global direction and per-demux lease are available and requested queue plus the exact notifier slot are prepared transactionally. Validation order is lifecycle/argument, direction capacity, per-demux limit, then fallible preparation. Failure returns INVALID_ARGUMENT, UNAVAILABLE or UNKNOWN_ERROR as appropriate with no committed mutation. Capability reporting, admission, cleanup and terminal release read the same snapshot. VTS has no generated runtime configuration and no unconditional default XML. A pre-start environment profile may select a static V1 variant only after source/flows/PIDs/queue budgets are declared and the exact queue vector fits C1; otherwise VTS remains DESIGN_HOLD without weakening runtime service guarantees.

## CD-f19898c24226 — DP-081

SHA-256: `f19898c24226ba41274c8cf3a81c067eb782e8554bc68987dd204b4983f294a1`

Compile all active Record-DVR-attached record-filter predicates into one immutable union predicate at the demux ingress generation. Evaluate each arriving 188-byte TS packet once; if it matches any attached record predicate, write it exactly once to the Record DVR in arrival order. Maintain per-filter index/callback state separately. Attach/detach/configuration transactionally replaces the union predicate at a generation boundary. Do not fan out then globally sort, deduplicate or infer gaps with an ingress_sequence.

## CD-84285ae93e78 — DP-082

SHA-256: `84285ae93e78738e53ac6460f3951675879590238cf837914c7fb7a6a7654d8d`

started中のrecord filter attach/detachはrecord route lock下で次の188-byte packet境界にcommitする。重複attachは冪等成功、未attach filterのdetachは`INVALID_STATE`、detach boundary以後のpacketは配送しない。route generationで重複・遅延配送を抑止する。

## CD-4138be949451 — DP-083

SHA-256: `4138be94945142fff2032d1f4a5dda4ba8ab4098ee889bd1cafdebe70b978724`

Playback consumption uses one owned staging buffer bounded by the configured playback FMQ capacity and one cursor per queue generation. FMQ beginRead/commit transfers bytes exactly once into staging; after commit, retry operates only from staging. The inject cursor advances only for bytes accepted by the backend and is monotonic, preventing duplicates. A new FMQ batch is not consumed until staging is empty. Retryable backend errors retain the remaining suffix. Fatal error, stop or close records the exact remaining-byte loss and terminal reason before discarding; no silent loss occurs. Generation change invalidates an empty cursor only; non-empty staging must be completed or explicitly terminalized first.

## CD-926c6165abfb — DP-084

SHA-256: `926c6165abfb3cf5b52ca7a3ce26f07b3d85064c653a203f54f60fccdb19b0f2`

ISDB-T enum domains follow reviewed ARIB STD-B31 2.2 clauses and official 2.2-E1 translation provenance recorded in `decisions/arib_revision_bridge.csv`. Domain validity and target-driver programmability are separate. The target backend advertises/accepts AUTO for mode, modulation, code rate, guard interval and time interleave unless the TARGET_DRIVER evidence proves a concrete value is programmed and honored.

## CD-eec1d09349d6 — DP-085

SHA-256: `eec1d09349d6178d1ab774dddac8b5e0c7d34f3fedd5ad74cb8f2707c24af7d0`

For target px4/earth_pt1 ISDB-T, frequency and 6 MHz/AUTO bandwidth are supported as defined by the programming matrix. Mode, layer modulation, layer code rate, guard interval and layer time interleave are AUTO-only because the current `FrontendTuneRequest`/px4 tune mapping does not carry or program concrete values. AUTO succeeds. Every concrete known value for those fields returns `UNAVAILABLE` and leaves backend and previous request unchanged. Invalid tags/ranges return `INVALID_ARGUMENT`. Capability, AIDL validation, ProductProfile and VTS tune inputs are generated from the same matrix. ARIB STD-B31 v2.2 pages 20 and 24 define the broadcast parameter domain; the AUTO-only subset is the truthful implementation capability, not an ARIB restriction.

## CD-ecab28a4133a — DP-086

SHA-256: `ecab28a4133a8c66317eee59b095b6f48f4afbd76d19b4b2df7afff395733543`

ISDB-T setting validity follows the reviewed ARIB STD-B31 domain and official translation provenance. Target-driver programmability is independently authoritative. For mode, modulation, code rate, guard interval and time interleave, the target backend advertises and accepts AUTO only unless TARGET_DRIVER evidence proves that a concrete value is programmed and honored. Concrete domain values may be represented internally for parsing/testing but must not be advertised or accepted as controllable settings without that proof.

## CD-b0d4ada43334 — DP-087

SHA-256: `b0d4ada433345ffadcaf5cd13123f7e1d5d9f04ab612a6db07a0e9687e0bb3bd`

For target px4/earth_pt1 ISDB-S, modulation and code rate are AUTO-only unless a future exact driver/device entry proves a concrete programmer. AUTO succeeds; concrete known enum values return `UNAVAILABLE` with no mutation; malformed values return `INVALID_ARGUMENT`. Frequency and relative/absolute selector behavior remain separately governed by the selector programming matrix and ARIB B20/B21 evidence.

## CD-e605d48c671e — DP-088

SHA-256: `e605d48c671e24f775afe134a7509eef42f5e9595fe81d177f17f991728e317d`

ISDB-S modulation is AUTO-only for the target backend. BPSK/QPSK/TC8PSK explicit input returns `UNAVAILABLE` without mutation until a concrete programmer and capability evidence are added.

## CD-041de602bb3a — DP-089

SHA-256: `041de602bb3a86417ffc7159a49c8d9b0c8ff1ae818fd41a0fc1f9df770a5554`

ISDB-S code rate is AUTO-only for the target backend. Every explicit code rate returns `UNAVAILABLE` without mutation until a concrete programmer and capability evidence are added.

## CD-e33f02576328 — DP-092

SHA-256: `e33f02576328fe458cb5eaf979916f9a6afaae3548b8b39132e3852acbfe279b`

BackendApplyOutcome={Applied,Rejected,Indeterminate,RollbackFailed}。Applied→commit、Rejected→旧状態維持、Indeterminate→対象resource quarantine+UNKNOWN_ERROR、RollbackFailed→quarantine+UNKNOWN_ERROR。retryは新operation IDでだけ許可。

## CD-0035f501335a — DP-093

SHA-256: `0035f501335a1ac45b74baace3e4927b0e5c57c652967582a12cdbb51c293fef`

Drop/owner-lossで非blocking safe-state cleanupを起動する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-5abddd905719 — DP-094

SHA-256: `5abddd905719a22ccf2e61f8fd1baac7cd293bf4abeb43c80ced996497f931d8`

LNB backend apply後にregistry commitが失敗した場合、diagnosticへrequested state、backend apply outcome、最後に確認できたhardware state、registry errorを原子的に保存する。当該LNBをquarantineし、close/reaperで安全状態を再適用してcleanupする。

## CD-ea31a3f44c02 — DP-095

SHA-256: `ea31a3f44c020d3fb5e7b1c6f92c2b53edf43abac8f583dcaf7ad45c5ad272dd`

revokeで即時invalid化し、新規/既存resolveを停止してkey materialを使用不可にする。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-17a78fc72be2 — DP-096

SHA-256: `17a78fc72be241e6185d3d226a88870325a045737fd0062bdb6a239ed11237eb`

closed local objectをINVALID_STATEへ統一する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-c3fca9bd0cfb — DP-097

SHA-256: `c3fca9bd0cfb8c3e2a7370f25e910b09db4a63e6e5d71bb48fc2c220bcd29ed4`

`addPid(pid, source)`は完全同一のdemux generation・PID・source filter generation tupleだけ冪等成功とする。sourceが異なる既存登録には`INVALID_STATE`を返し、変更には先行`removePid()`を必須とする。

## CD-18226a3350a0 — DP-098

SHA-256: `18226a3350a0722d83e4ba11aa13b21887398e89fdcec9d29a44231287e30ba6`

閉鎖済みsource filterは`INVALID_STATE`へ統一し、`INVALID_ARGUMENT`行を削除する。

## CD-0996803849af — DP-099

SHA-256: `0996803849aff6fad5ae87f041e12d07186e6add8813f13ef6eadffe2f5eca68`

AIDL surfaceを正しcallback依存操作だけへ制限する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-a625b795dfe0 — DP-100

SHA-256: `a625b795dfe0870b0106c8c39781a4c9bdc6915a63c53ab375e8bf883ed05f2c`

`IFrontend.getStatus(statusTypes)` and `getFrontendStatusReadiness(statusTypes)` have distinct cardinality contracts. `getStatus` rejects an unknown enum representation with `INVALID_ARGUMENT` and no output. For known values it emits one `FrontendStatus` only for each requested type advertised in `FrontendInfo.statusCaps`, preserving relative order and duplicates among emitted advertised types; known-unadvertised types are ignored, so an all-unadvertised request succeeds with an empty vector. It never fabricates type-specific not-available sentinels. Failure to obtain any advertised requested value atomically returns `UNAVAILABLE` and no partial vector. The HAL performs this filter because the public framework/JNI forwards the request while the SDK contract says unadvertised types are ignored. `getFrontendStatusReadiness` also rejects unknown representations, but for every known requested type returns exactly one result in request order: advertised types are `UNAVAILABLE`, `UNSTABLE` or `STABLE`; unadvertised types are `UNSUPPORTED`. The APIs may share enum validation but not an output-cardinality helper.

## CD-89a38be520eb — DP-101

SHA-256: `89a38be520ebc37af948b4cd5252e73ccf5d1c57a4dfeda91e4f8d56d6579e24`

SSOT修正後にtest期待値を更新する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-37b6e0fd27ba — DP-102

SHA-256: `37b6e0fd27ba3922c57898910ac32f76ea5c7f4aa68464ad6ac829973ce0644a`

For non-close public methods, precedence is LogicalClosed→InvalidArgument→WrongLifecycle→ResourceUnavailable→BackendFailure→Success. The result of `close()` itself is not defined by this generic precedence: it is delegated exclusively to the DP-003 interface-specific close table. Late `IFilter.releaseAvHandle()` follows the AV release ledger and is not a generic post-close method.

## CD-f11f9b437f06 — DP-106

SHA-256: `f11f9b437f06640b3e441572c7e6b4f8d829e50351af9c7d8ad6bd46456ad2f5`

Raw section uses the two-plane contract. A complete section envelope requires pointer/section_length bounds and a complete byte extent. If the envelope is complete but table syntax, reserved bits, CRC or semantic fields are invalid, raw bytes may be delivered only to a raw filter, no DemuxFilterSectionEvent is emitted, and a typed section-parse diagnostic is recorded. Non-raw section filters drop the block. An invalid or incomplete envelope is dropped for every filter.

## CD-29afb20f7bfd — DP-107

SHA-256: `29afb20f7bfd0b960ad15046fdbdafc318eeeeec296432ae00291d42b03482e3`

PES uses the two-plane contract. A complete PES envelope supports valid bounded PES and packet_length=0, including headers split across TS packets. Semantic event emission additionally requires prefix, stream_id-specific optional-header form, flags, marker bits, header_data_length and PTS/DTS validation. Semantic failure suppresses DemuxFilterPesEvent; an envelope-complete raw PES filter may still receive exact bytes with a typed diagnostic. Envelope failure drops all output.

## CD-2e715668ecfe — DP-109

SHA-256: `2e715668ecfe5f6c0a3551e3f04a347f5206bcb7cc0ced0e8250731f39bb9873`

source comment言語規則を`CODE_CONVENTION.md`へ移し、DESIGN_JA.mdから削除する。設計文書にはAPI・状態・資源寿命だけを残す。

## CD-d6a10d6f3d8f — DP-110

SHA-256: `d6a10d6f3d8f1188283a8aa59bc3a32a3083c202bcc49bc30e193270b16c88e4`

曖昧な「表1/表4等」を廃止し、各移動規則にstable anchor IDを付ける。状態契約=STATE-FE-01/STATE-FILTER-01/STATE-DVR-01、Result precedence=RESULT-01、cleanup=LC-01、AV release=AV-01。本文はanchor IDだけを参照する。

## CD-4f095fd17166 — DP-111

SHA-256: `4f095fd17166b695e2bea13b5632c2d9c6d1ce839f087ce21da07aa1d0d89f38`

Runningをscan lifecycle stateへ分離し、terminal reason enumはCompleted/Cancelled/Failed*だけに限定する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-ee2559d5330c — DP-112

SHA-256: `ee2559d5330cb78b49167454cbe4ab6a5b970bf58c1e73201107e8bd02b348a4`

ISDB-S selector support is created only by a capability-domain-eligible fact in an exact SupportedDeviceCapabilityCatalog entry keyed by SupportedBackendIdentity, driver repository/commit, exact USB VID/PID or equivalent device identity, and revision scope. With no matching verified entry, or when selector_capability_release_eligible is false, no ISDB-S selector capability is advertised and no tune object for that selector is created. An eligible entry may advertise RELATIVE_STREAM_NUMBER 0..7 when the exact adapter path is proven; an absolute STREAM_ID request is then UNAVAILABLE without backend mutation. A separately proven entry may advertise absolute TSID 0..65534; 65535 is INVALID_ARGUMENT and there is no special 0..11 rejection. ProductProfile may suppress an eligible selector fact but cannot create or widen one. Runtime reads only immutable EffectiveCapabilities.

## CD-a58fa66e5923 — DP-113

SHA-256: `a58fa66e5923aa8153982b047c5b5258d14e937c4e6d903cb10cf10719bead00`

LNB is a device-scoped endpoint resource governed solely by `decisions/lnb_device_resource_contract.csv` and the event-driven worker termination contract, not a static ServiceResourcePlan pool or TargetDriverTimingProfile. `getLnbIds()` enumerates successfully probed eligible endpoints. `openLnb()` acquires one endpoint lease; unknown ID returns `INVALID_ARGUMENT`; leased, CleanupPending or Quarantined endpoint returns `UNAVAILABLE` without mutation. First close commits LogicalClosed, rejects new public work, and attempts all immediate cleanup. Retryable incomplete dependencies remain CleanupPending; running workers are fenced and transferred once to ReaperSupervisor. The lease is returned exactly once only after backend and worker cleanup complete; quarantine retains it. ProductProfile may suppress LNB but cannot fabricate endpoint or voltage capability.

## CD-c27eef50e6e1 — DP-114

SHA-256: `c27eef50e6e1324b21bdf19da88fee26c7d384e06894a7fbdc2210c305b7f6a0`

Cleanup diagnostics are internal only. Each owner stores one bounded in-memory aggregate snapshot for the latest cleanup attempt. The normative design fixes atomic replacement, typed primary/cleanup failures, saturating omitted_failure_count and non-destructive reads; it does not fix a record count. The implementation-local capacity is at least one and is selected from the approved diagnostic memory slice, with overwrite/omission behavior independent of public results. There is no persistence, operation ID, TTL, acknowledge/delete protocol or ProductProfile capacity.

## CD-f5a3c9aad0f5 — DP-117

SHA-256: `f5a3c9aad0f5c953300eea6d7f7bf6c7173ca65fbba2bb282128ba45b3bfb2c2`

backend error mappingを固定する。client malformed/range=INVALID_ARGUMENT、Missing/Busy/Capacity/valid-but-unsupported=UNAVAILABLE、wrong lifecycle=INVALID_STATE、dependency未初期化=NOT_INITIALIZED、allocation=OUT_OF_MEMORY、permission/I/O/config corruption/invariant=UNKNOWN_ERROR。

## CD-73adc4b61306 — DP-118

SHA-256: `73adc4b61306be367e9c68734e9694538a205f704037b517bec5d6d04eb8c47e`

共有primitive/invariantのみ共通化し、interface-specific orchestrationを許容。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-b25dddb0e92b — DP-119

SHA-256: `b25dddb0e92b783cd538c828dbb95a597d2e615db6bbf0ab11262d62942ffe85`

Worker cleanup is event-driven under `decisions/worker_termination_contract.md` and has no TargetDriverTimingProfile or normative millisecond bound. Cancellation sends all available stop/wake signals once. Already-complete workers are collected immediately. A running worker is owner-generation-revoked and mutation-fenced before one-time transfer to bounded ReaperSupervisor and quarantine; the public caller never blocks on join. Lease consumption continues until actual termination and residual cleanup. Reaper capacity is derived from enforced live-worker ceilings and creates no retry timer jobs. Only typed evidence of unfenced global mutation, exclusive unfenceable global resource, or replacement race permits service-critical escalation; a fully fenced owner-local residual cannot stop unrelated capabilities.

## CD-d2a67d36ae9b — DP-120

SHA-256: `d2a67d36ae9bc96d4913126a7e2ef7da01f2b3d7920e40f95a29a29d057eb179`

Failure propagation is owner×generation×dependency-local by default; a new spawn/callback/request failure must not destroy healthy siblings. A residual worker is service-critical iff, after owner-generation revocation, it can still mutate a service-global registry/backend, holds an exclusive service-global singleton/FD/queue, cannot be fenced by owner/generation/dependency tokens, or would race a new boot for the same resource. If none applies, quarantine is owner×generation×dependency-local and unrelated owners remain available.Escalation to service quarantine requires an explicit predicate witness in the typed diagnostic.

## CD-0235ec29ab63 — DP-121

SHA-256: `0235ec29ab636e3c01a1d26751f8294d929ccefd3ee9e1d1fb50ba53551f750a`

health gate表を固定する。callback sink failure: domain operation継続可・新callback配送停止。diagnostic store failure: domain継続可・fallback counterのみ。backend unavailable: query/close可、mutation=UNAVAILABLE。registry corruption:当該domain mutation=UNKNOWN_ERROR、close/query可。FMQ corruption:当該object start/write不可、flush/close可。

## CD-c0c3b6c7452d — DP-122

SHA-256: `c0c3b6c7452dea85dc3e0b8743112a833190177758c564c3bf8df55508fcb44e`

run_state、hint、handle_export、generationを直交型へ分離し、不可能組合せだけ型で禁止。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-cbde311bfcd9 — DP-123

SHA-256: `cbde311bfcd91ceeda102a7cb6d1aca1f6252970698f45913ff1b600ace65cda`

Shared and event-local fd identity is never numeric-fd equality. On export/allocation HAL records `backing_id`, transport mode, handle integer payload, expected size and fstat identity `{st_dev, st_ino}`. Duplicated fds for the same backing resolve to the same identity. Shared-handle release validates the recorded shared backing. Event-local release validates the fd identity and full allocation tuple. `empty+dataId` validates the ledger identity without an fd. A mismatched handle/dataId/backing pair is `INVALID_ARGUMENT` and cannot release another allocation. Registry/fstat failure is `UNKNOWN_ERROR` and fences uncertain storage.

## CD-be1cd667d295 — DP-124

SHA-256: `be1cd667d295f9a07c3b8f95c4150c289ba1acc9d6ad8f1b5a9167ea9614823a`

TS→TS linkCapsとnon-null `setDataSource()` graphを維持する。open済み未configureのUNDEFINED/TS endpointをVTS用`TsRaw`として接続可能にし、具体subtypeのvalid-but-unsupportedは`UNAVAILABLE`へ写像する。製品利用要件を正本へ記載し、linkCaps撤回案は採用しない。

## CD-744c7a6b8d38 — DP-125

SHA-256: `744c7a6b8d38d9605a275f9cb66d1fee2818ddca06c07d3093c99c90b41df73f`

open済みrecord DVRでconfigure前もattach/detachを許可する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-607c3aafef57 — DP-127

SHA-256: `607c3aafef5709cfcea929b966865cb14769e019825fd6c012ff46d71001ef76`

Drop/owner-lossで非blocking cleanupを起動し、blocking joinはreaperへ委譲する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-3ccd79b1315f — DP-128

SHA-256: `3ccd79b1315f92c84529c14dd82e306dbebbce36b2efc31bb558132af214493f`

queue commit後のEventFlag wake失敗はpost-commit diagnostic。committed dataはqueueに保持しrollbackしない。producerを停止し、flushは破棄、closeは破棄、再wake成功後はdrain再開を許可する。public commit済みoperation結果は反転しない。

## CD-31def4318a93 — DP-129

SHA-256: `31def4318a93ac7d3664cdd2aa9991991328548eeb3b9adda4b263a48e63ec0e`

FMQ bytesをowned stagingへcopy後、commitReadしてFMQ_CONSUMEDへ遷移する。backend inject成功時DEMUX_INJECTED。inject失敗はstagingからretryし、stop/close時残存はexplicit loss diagnostic。

## CD-5c05f634918b — DP-130

SHA-256: `5c05f634918b59d15608d7f4e6903a180d4f7a577899aaf093bf3d01cf95f8d1`

Common transaction framework requires all fallible prepare work before an operation-specific linearization point. Replacement tune is an explicit exception with two domain commits defined only by DP-134: Commit A terminalizes old stream state before the fallible new backend submit; Commit B activates the new generation after submit succeeds. DP-130 must not collapse or reorder these commits and non-tune operations do not inherit tune boundary reset semantics.

## CD-ee4cbaef9d3a — DP-131

SHA-256: `ee4cbaef9d3a30acf86658a10b2aeccaaf27fb8d12224386188dea85b29e5cab`

callback healthを独立軸にし、callback依存operationだけへ波及を限定する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-bb523f266dc2 — DP-132

SHA-256: `bb523f266dc2ffad2c4370247ae2d6133043656fb83ee2e18f801c8013682a17`

OpaqueKeyToken、TokenEntryId、ResolvedKeyMaterial、CAS validityを別型・別寿命に分離する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-7e20ddf99cc0 — DP-133

SHA-256: `7e20ddf99cc07e27cb7b142aa564714f416e9fe6c18a427185e86215c84fbaef`

snapshot queryを純粋読取にし、stale cleanupを明示transactionへ分離する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-26c7896fa081 — DP-134

SHA-256: `26c7896fa08182f398286914149afe481ea408fb195c089ca63340e3bdbbb24e`

Replacement tune has two explicitly distinct linearization points. Phase A: validate and dormant-prepare; acquire frontend transaction lock; stop old backend; quiesce old worker. Commit A (old-generation terminalization commit) atomically marks the old generation terminal and resets bound demux/assembler boundary state. Then submit the new backend tune. On submit success, Commit B (new-generation activation commit) publishes the new generation and activates the prepared worker. On submit failure, release prepared state and remain Untuned/Failed; never restore the old tune. Commit A and Commit B must not be described as one commit, and boundary reset must occur in Commit A before backend submit.

## CD-97ffaaad87a9 — DP-135

SHA-256: `97ffaaad87a956423a5770939cdcdfd9b95bb01ddee0b6a3aa44552b860078b8`

対象filterの新規operation拒否に限定し、demux quarantineは共有ledger corruption時だけにする。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-62c3099decc5 — DP-136

SHA-256: `62c3099decc515ba87eafd323ddf6fde9ad139e0b7cd95b133f9181a64acd073`

reservation/preflightと外部prepareを分け、lock内をowner再検証とcommitへ限定する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-49748cef4071 — DP-137

SHA-256: `49748cef407187ac25ec5503bae4b68f6f3ab0cfca5fee1679af5e5feabe588c`

After all-attempt cleanup, apply the service-critical predicate. A residual worker is service-critical iff, after owner-generation revocation, it can still mutate a service-global registry/backend, holds an exclusive service-global singleton/FD/queue, cannot be fenced by owner/generation/dependency tokens, or would race a new boot for the same resource. If none applies, quarantine is owner×generation×dependency-local and unrelated owners remain available. Abort new service boot and quarantine the service only when the predicate is true; otherwise quarantine the DVR owner/generation and permit unrelated service owners to continue. Integration/rebase rule: when applied to a newer main branch, preserve every current DVR playback worker cleanup, notifier cleanup, callback-clear, owner-generation revocation and reaper transfer obligation. The proposal may refine only the escalation predicate; it must not replace or delete newer cleanup steps. A three-way merge conflict is a hard adoption gate.

## CD-de39c931c827 — DP-138

SHA-256: `de39c931c82729344cc4e63774ac2fa3a5971a6529df7b74ac8c07fd7526fefc`

Dispatch and execution use one non-Clone one-shot ExecutionToken containing object_id, object_generation, owner_id, method_domain and nonce. It is created under the validation/reservation lock in Prepared state, transferred exactly once to the executor, and consumed exactly once immediately before the first externally visible side effect. States are Prepared, Transferred, Consumed, Cancelled and Invalidated. Duplicate consume, stale generation, wrong owner or method returns a typed authority error with no side effect. Cancellation before consumption invalidates the token. Owner death and object close invalidate all unconsumed tokens. After execution failure the token is never returned; retry requires a new validation and token. No second proof/execution authority exists outside this state machine.

## CD-b5b5ff3e8a44 — DP-140

SHA-256: `b5b5ff3e8a44cde89d8040125a67a0512f427413968d1747b53d4c36bf7da34c`

wait outcomeをtimeout/retryable interruption/fatal corruptionへ型分けする。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-7c3b864016b3 — DP-141

SHA-256: `7c3b864016b34e3452d59cef2f7680be357c0957fb3f04deedf1e8667f3d3362`

capacity/oversize/allocation/corruptionを別variantとfailure scopeへ分ける。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-756d4401c071 — DP-142

SHA-256: `756d4401c07134b880f133ee3684a9582193add63cc8cc7d563f50e027a7ea51`

pre-commit callback registration/delivery failureはbackendを停止しgenerationをTerminalFailedへ遷移、以後callbackを抑止、bound demux boundaryをresetしpublic operationはUNKNOWN_ERROR。post-commit callback delivery failureはdomain状態を維持しpublic結果を反転せずdiagnostic/fallback accountingへ記録。

## CD-a698e84336b0 — DP-143

SHA-256: `a698e84336b0a21929d721155ac20b255f90fa6f4b61f93b04b64f02dfbdff69`

`addPid/removePid`はbackend packet routeをprepareし、成功後にPID claim ledgerをcommitする。backendがprepare APIを持たない場合はidempotent applyと補償rollbackを同一transaction内で完了し、rollback失敗時だけdescramblerをquarantineする。

## CD-f45e7fb8ca5c — DP-144

SHA-256: `f45e7fb8ca5c1566d9ec2a68c4ed3ff8020768f6a89466a80de152162853c2ed`

A token with no active registry entry is INVALID_ARGUMENT; this includes unknown, foreign, expired, revoked or previously released tokens because no persistent expired/revoked tombstone is retained. A currently registered token whose active entry cannot be used in the requested session/lifecycle is INVALID_STATE. VOID token returns SUCCESS and unlinks the session key. Registry lock timeout is UNAVAILABLE. Registry corruption is UNKNOWN_ERROR and quarantines the registry. Client token rejection alone never closes or poisons the descrambler object.

## CD-7ebca776dbd1 — DP-145

SHA-256: `7ebca776dbd14b4dad7a39937a74c7b2f330aa52bbe5d1584b62fc8431004db0`

old token entryを使用不可のcleanup_pendingへ移し、close/resetから再試行できるrelease authorityを保存する。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-507707a73419 — DP-147

SHA-256: `507707a73419657788fe3065489ef39ff5157f3806435f38db22b4b131e04e96`

Malformed/OversizeSection、StalePartialDiscard、QueueOverflowを別result/counter/statusへ分ける。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-3b8012881358 — DP-151

SHA-256: `3b80128813583221f60f0859e530ea639063cff72710e4547474cb1d994802b7`

Commit-critical state is limited to generation, lifecycle, source binding, backend ownership, PID ledger, cleanup authority and queue-pointer commit. Callback delivery/accounting and diagnostic text are post-commit; storage failure never reverses a committed public result. Diagnostics use saturating counters and one implementation-local bounded ring of typed callback failures. Capacity is at least one and is derived from the diagnostic memory slice and accepted failure burst, not fixed by DESIGN or ProductProfile. Full insertion overwrites oldest and increments saturating overwritten_record_count; reads are non-destructive. No persistent report, operation ID, TTL, acknowledge/delete or public HAL API is introduced.

## CD-de1ca7f6a3b9 — DP-152

SHA-256: `de1ca7f6a3b9f9561ba934b02dea0fdbff63cac827c72fbc2475249d3ed6b7bc`

各operationをPrepare→CommitCritical→PostCommitへ統一する。CommitCriticalは正本state/ownership/backend apply、PostCommitはcallback、status wake、diagnostic、cleanup accounting。PostCommit失敗はtyped secondary outcomeとして保存し、primary Resultを変更しない。

## CD-d3650ae4aad6 — DP-155

SHA-256: `d3650ae4aad629ee6f617c0235ca848f1597b770c8019fe02798672fce7671d7`

handle_exportedとclient_handle_activeを分離しfresh dup再取得遷移を追加。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-89c7c4a029c5 — DP-157

SHA-256: `89c7c4a029c50f1dc216db989c2fb0fc6a198ab0a16b7f22d1096d10f1c8f1a0`

Result表を固定する。null/foreign/wrong-demux object=INVALID_ARGUMENT、closed/wrong lifecycle=INVALID_STATE、validだがunsupported subtype/capability=UNAVAILABLE、TPID/tag mismatch=INVALID_ARGUMENT、resource capacity=UNAVAILABLE、internal corruption=UNKNOWN_ERROR。

## CD-0512afc37228 — DP-158

SHA-256: `0512afc37228356ba96273f526075edbdf260c1cf8d337c2fdf4e9d7fb159714`

自己検査を宣言だけでなく実際の表へ適用し、各NGセルを修正してから完了条件を満たす。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-a06efdfafb1e — DP-159

SHA-256: `a06efdfafb1e54c3b468a3b60eda7213f1539a9924265ba201fe9fc6257948e1`

started中の`setDataSource(non-null/null)`は常に`INVALID_STATE`とする。source接続・切断はopen/configured/stopped中だけ許可し、hot-switchは実装しない。境界表の`started維持`行を削除する。

## CD-100ea74f7c46 — DP-160

SHA-256: `100ea74f7c467e0fd6710942a8df83e40c07b36ccd2b6edec2b0a1f1b9edb68c`

Store terminal_reason and end_delivery_outcome as orthogonal fields. terminal_reason is one of Completed, Cancelled, FailedBackend, FailedPanic and is never overwritten by END delivery. end_delivery_outcome is Delivered, CallbackMissing, StoreFailure or BinderFailure. Backend stop and generation terminalization occur exactly once; delivery failure is secondary diagnostic/accounting only.

## CD-e7e1b35f2ec1 — DP-162

SHA-256: `e7e1b35f2ec139d5d010d14fd77313de18a9e1b2eca6fe28cbe691323eed4095`

Descrambler/TS failure behavior is governed by the failure-scope taxonomy. Infrastructure framing corruption alone quarantines the affected path. Malformed TS is packet-local drop; TEI and continuity remain path-specific; valid still-scrambled packets may remain on raw/record paths but never produce decoded semantic events. ARIB STD-B25 6.7-E1 Part 1 clauses 2.2.2.4, 2.2.2.10-2.2.2.11, 3.1.5-3.1.7, 3.2.3-3.2.4, 4.3.3.3 Tables 4-11 to 4-14, 4.8, 4.9 and 4.10 are reviewed and pinned by decisions/arib_b25_6_7_e1_bridge.md. They establish TS-payload/per-packet scrambling, receiver-side ECM/EMM transfer to the CA module, Ks return to the receiver, scrambling detection, at least one odd/even key pair per tuner, and at least 12 simultaneous PIDs. These capacity obligations must be separately advertised and enforced. The no-public-ECM/EMM/Ks boundary is justified by the AOSP public Tuner HAL surface and least-exposure design, not asserted as verbatim STD-B25 text. HAL quarantine/error mapping remains an AOSP/internal design decision.

## CD-5c2a8939c31b — DP-163

SHA-256: `5c2a8939c31bf6959d64063197d8289a17008ef5ba2c63d8a961d720bf6fdc2e`

A complete 188-byte TS packet with TEI=1 is preserved in raw-TS and TS-record output in ingress order. The HAL increments a saturating TEI counter and keeps record byteNumber relative to bytes actually written. Section/PES/AV and other semantic consumers discard or resynchronize on that packet and emit no parsed event. Malformed sync/length is a distinct packet-local drop. Continuity discontinuity is a distinct assembler reset. None of these broadcast packet variants quarantines the queue/path; only infrastructure corruption may do so. Error-stripped raw/record output requires a separate explicit ProductProfile with its own byte-number contract.

## CD-19a0feba3093 — DP-165

SHA-256: `19a0feba3093e8ee9e1820b9b9ff8509bffdab7a2aa6f0e25fe35621c4be7fda`

playback inputを通常demux routingへ流し、attach済みrecord filter/DVRへの配送を許可。この規則を唯一のSSOTとし、旧分岐・重複表・例外規則を削除する。

## CD-e4bafb2c9420 — DP-166

SHA-256: `e4bafb2c9420230b654c53c75cd5a74987d08c3e53ed9cc9f9c02af25e2d8f66`

attached sink/filterが1つ以上startedになるまでplayback consumerはFMQを読まない。別staging queueは導入せず、FMQ自体のbackpressureで待機する。sink停止でconsumerを再pauseし、queue容量超過は通常FMQ statusで通知する。

