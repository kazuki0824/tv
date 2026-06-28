# Tuner HAL2 コーディング規則

この文書は `tuner_hal2` 固有の実装規約を書く。公開契約、状態遷移、戻り値、capability/profile、VTS/product profile 方針、資源寿命、WorkerExit / WorkerFailureClassifier / ScanSessionTxn 論理契約、composed failure の意味は `tuner_hal2/DESIGN_JA.md` を正とする。

## 1. failure / rollback / cleanup の実装規約

- cleanup、rollback、stop、join、callback、unregister、close の失敗を `let _ =`、空分岐、ログだけ、`drop(result)` で捨てない。
- primary failure 発生後の cleanup / rollback を `?` だけで呼び、cleanup failure で primary failure を無診断で上書きしない。
- primary + cleanup failure を `format!(...)` や文字列 detail だけの generic internal error に潰さない。戻り値として片方の status を選ぶ場合でも、もう片方を composed failure または必須診断から消さない。
- `FirstErrorCollector` を primary + cleanup failure composition の代替にしない。collector は同一 cleanup phase 内の cleanup step 間 first cleanup error を集める部品としてだけ使う。
- primary + cleanup failure の実装は共通 failure composition helper 群へ寄せる。個別 transaction body で同等の precedence 判定や文字列 detail 合成をコピーしない。
- post-allocation / post-registration failure path では、object table rollback、runtime cleanup、callback rollback、diagnostic / cleanup-failed marking を可能な限りすべて試行する。途中の `?` で後続 cleanup を飛ばさない。
- rollback / public close / owner-loss cleanup では `Option::None` / missing target を無言成功扱いしない。missing を許容する処理は read-only query、idempotent stop、best-effort telemetry、defensive unavailable path など、DESIGN_JA.md が許容した範囲に限る。
- rollback / public close / owner-loss cleanup の正本に void / best-effort-only cleanup を使わない。失敗を表面化できる戻り値を持つ operation に接続する。
- cleanup-failed marking、callback unhealthy marking、Drop leak quarantine、owner-loss cleanup failed state、scan terminal failure record、Drop から返せない drop-leak error record、packet path の descrambler source-filter validation failure は必須診断として扱い、best-effort log 扱いにしない。
- `setKeyToken(VOID)` のように session state と token table refcount の両方を変える経路では、clear-key の状態遷移順序を `DESIGN_JA.md` の descrambler transaction 契約へ寄せる。外部 caller が plan / prepared token / commit を個別に組み立てられる public split API を置かず、full transaction façade 内で stale plan 検証、session clear commit、old token release、release failure diagnostic を固定する。old token release failure を理由に session key を旧 token へ戻してはならない。
- packet count、malformed count、throughput counter、補助ログなどの best-effort telemetry は primary failure を上書きしない。必須診断 store を持つ場合でも、service lifetime 中に unbounded に増える `Vec` を正本にしない。bounded store と observable dropped/failure counter に分離する。
- Drop は public close の代替にしない。`Drop` 実装は drop-leak 入口だけを呼び、object 種別固有 cleanup を書かない。

## 2. AIDL / service_runtime 境界の実装規約

- AIDL method body で `ensure_open()`、method planning、runtime lock、service_runtime use-case 呼び出しを手書きで組み合わせない。object_runtime façade または service_runtime use-case を通す。
- AIDL method body で fallible な request 変換、callback retain、source relation validation、unsupported / unavailable status mapping を object lifetime / generation / kind 確認より先に実行しない。
- child object open では、service_runtime child-open use-case が typed child runtime id と `RuntimeObjectEntry` を同一 result で返す。AIDL helper は `RuntimeObjectEntry.ledger_id` を filter / DVR id へ再変換しない。callback artifact retain failure・typed Binder object construction failure は service_runtime child-open finish use-case へ渡し、rollback command 生成、unhealthy marking、primary+cleanup failure composition を AIDL 側で実装しない。
- AIDL 層から `RuntimeObjectTable`、`TunerServiceRuntime::object_table()`、`TunerServiceRuntime::object_table_mut()`、runtime registry、`service_runtime::boot/*_txn.rs`、`service_runtime::frontend_worker_txn` を直接参照しない。
- AIDL method implementation files から低レベル executor helper や `plan_object_method_dispatch` を直接呼ばない。
- service_runtime domain use-case 以外から `public_runtime_id_for_object_method` + `plan_object_method_dispatch` の組を直接扱わない。
- `best_effort` 名の callback cleanup helper を追加しない。
- production code の file split module では `use super::*;` を使わない。親 module から必要な型・関数を使う場合は `use super::{...};` で明示する。
- `#[path]` / `include!` / `include_str!` を使わない。
- `Status::new_service_specific_error()` を `aidl_service::error_bridge` 以外で直接呼ばない。
- `status_from_hal_error` / `status_from_tuner_status` / `service_error` 相当の helper を `tuner_service.rs`、object wrapper、child open helper、runtime helper へ再定義しない。
- `android.hardware.tv.tuner::Result` の整数値を、Binder status 生成目的で `error_bridge` 以外へ拡散しない。
- AIDL helper は Binder status 変換と method identity adapter に留め、object lifetime / request-builder critical section / domain state commit を所有しない。
- supported public API planning には `PublicApi` を使い、unsupported-by-design の戻り値生成には `UnsupportedPublicApi` を使う。query / open / 状態取得系を unsupported planning に流用しない。
- unavailable / unsupported / plan-only 経路で、AIDL method body に plan-only public helper や public thin wrapper を残さない。`aidl_service::object_runtime::plan_unavailable_object_method_use_case()` は public façade として維持し、内部では object live / generation / kind、request build、`RuntimeExecutableRequest` validation、dispatch planning までを行う plan-only helper を使う。domain operation を実行しない経路で `ObjectMethodDispatchProof` を発行して捨ててはならない。
- close method は `ObjectCloseTxn` pattern を使う。close preflight、`Closing` 遷移、typed domain cleanup command、cleanup-failed marking を AIDL method body へ戻さない。
- close finalization で複数 public runtime entry を unregister する場合は、destructive unregister を開始する前に対象 entry の存在を全件 preflight する。preflight failure がある状態で一部 runtime unregister を始めてはならない。
- close finalization / cleanup-failed marking の cascade helper では root object の terminal lifecycle と descendant object の terminal lifecycle を混同しない。root の unexpected `Closed` / `Quarantined` は `InvalidLifecycle` として返し、already terminal descendant は親 close retry / finalization 対象から除外する。
- close cleanup / finalization failure を cleanup-failed marking する場合、存在しない cleanup step 名を追加してはならない。pre-finalization cleanup failure は callback cleanup=`CleanupStep::UnregisterRuntime`、typed domain cleanup command=`CleanupStep::ReleaseBackend`、descendant DVR notifier stop=`CleanupStep::StopWorker` に分類する。object table entry lookup / close commit failure は `CleanupStep::ReleaseLedger`、public runtime unregister failure は `CleanupStep::UnregisterRuntime` として既存 typed step に分類する。
- callback artifact cleanup helper へ raw `SharedTunerRuntime` を渡さない。frontend owner-loss など domain cleanup 内からも `SharedAidlServiceContext` owned callback store helper を使う。
- close cleanup 系 helper は `Closing` を許すため、close preflight に使わない。通常 method、close preflight、close cleanup で lifecycle helper を混用しない。

## 3. service_runtime transaction boundary

- object close / Drop leak の public runtime unregister を `FnOnce` closure で AIDL 側から注入しない。`service_runtime` が `ObjectRuntimeCleanupCommand` を生成し、AIDL executor は command execution bridge に限定する。
- descrambler key table は service_runtime に置き、descrambler crate から key table / key lookup error / key registration error / key slot id を public export しない。

- top-level `service_runtime/src/*_ops.rs` は public façade だけを置き、`TunerServiceRuntime` の private field を直接参照しない。
- 状態変更は `service_runtime/src/boot/*_txn.rs` の domain transaction context へ閉じる。
- flat `transact_*` helper は原則として boot child module 内の実装詳細であり、top-level `*_ops.rs` から直接呼ばない。ただし `DESIGN_JA.md` で明示した demux/filter/DVR の単純 operation は例外とし、`demux_filter_dvr_ops.rs` から `transact_*` helper を直接呼んでよい。複数 step の child open / owner object registration / rollback / cleanup composition は txn context を通す。
- `TunerServiceRuntime::registry_mut()` を呼んでよい production code は `service_runtime/src/boot/*_txn.rs` の domain transaction implementation に限る。top-level `*_ops.rs`、AIDL 層、domain crate、`query_api.rs` から呼ばない。
- `RuntimeQuery<'a>` は read-only query 専用とし、mutable reference や mutating transaction context を持たせない。
- read-only object query は `execute_object_query_use_case()` または service_runtime query façade を通す。AIDL method body で `ensure_open()` と query 側 lifecycle check を二重化しない。
- `transaction_registry.rs` は runtime transaction -> dispatch target の正本表に限定する。coverage / 接続済み表示 / stale 未接続表示を同表の第2責務として持たせない。`RuntimeCommandDispatcher` はこの表の dispatch target だけを消費し、production dispatch と別に第2の runtime handler / status 判定層を作らない。

## 4. wrapper 作成基準

Wrapper を置いてよい条件:

- public API 境界になる。
- domain naming を隠蔽する。
- AIDL/service_runtime から見える型境界を固定する。
- object handle based use-case 境界、callback artifact bridge result を service_runtime use-case へ返す façade 境界、phase order を所有する use-case 境界になる。

Wrapper を置くべきでない条件:

- 名前も責務も同じ単純委譲である。
- context method と1対1で、公開境界・domain naming・型境界の意味が増えていない。
- callback rollback だけ、profile validation だけ、close helper だけを包む public thin wrapper である。
- production 未接続の bridge / slot / mapper 型を public re-export するためだけの wrapper である。
- production 未接続の transaction skeleton を public type として残すだけの wrapper / 共通crate surface である。
- test だけで使う transaction 型は共通部品名を名乗らない。DESIGN_JA.md の共通部品表に載せる型は production call path から参照されていることを静的確認する。
- 旧 transaction 名を新 transaction 名へ置換した場合、DESIGN_JA.md の共通部品表と責務表を同一リリースで同期する。

## 5. capability token / guard 実装規約

- 検証済み状態、dispatch 済み状態、transaction plan、ledger guard、rollback guard などを表す型は capability token として扱う。
- capability token は、状態検証または予約を所有する共通部品だけが発行する。外部 caller が public constructor / public enum variant / public field struct literal で偽造できる形にしない。
- 一回性 token に `Clone` / `Copy` を付けない。consume-by-value の API で消費する。
- 複数回利用可能な値が必要な場合は token とは別の read-only descriptor 型に分離する。
- single-variant enum や将来用 variant で capability token の状態機械を装わない。現在の証跡が1状態だけなら、対象情報を直接保持する struct にする。

## 6. worker / callback / source boundary 実装規約

- frontend worker の blocking join を `TunerServiceRuntime` lock 保持中に実行しない。runtime lock 内では cancel 設定済み join ticket 取得までに限定し、join は lock 外で行う。
- worker start 成功後に fallible commit を置く場合、commit failure path で起動済み worker を stop/join し、runtime snapshot / demux snapshot rollback を試行する。
- `CallbackRegistryUpdate::Missing` は rollback / public close / owner-loss cleanup で空分岐にしない。callback store の削除対象と runtime registry clear 結果を照合する。
- callback 未登録は Binder delivery failure ではない。AIDL delivery façade は callback artifact absence / store failure を primary error として `finish_callback_delivery_failure_use_case()` へ渡すが、service_runtime はこの phase だけで runtime registry unhealthy marking を行わない。実際に callback `Strong` を取得した後の Binder delivery failure、event conversion failure、post-commit notification failure では、service_runtime の finish use-case が診断・unhealthy marking・failure composition を所有する。
- callback artifact store clear の production entry は `OwnerCallbackCleanupArtifactCommand` を受ける bridge に限定する。`AidlObjectHandle` を直接受けて callback store を clear する helper は private raw helper または `#[cfg(test)]` helper に限定し、production code から呼ばない。
- callback artifact store、DVR notifier store、filter event dispatcher bridge、drop-leak diagnostic store は process-global `OnceLock` / `static Mutex` に置かない。これらは `AidlServiceContext` または `TunerServiceRuntime` instance field の lifetime に閉じる。drop-leak diagnostic store の lock poison は `poisoned.into_inner()` で吸収せず、context-owned failure counter または reset failure として表面化する。
- `IFilter.setDataSource(source)` の non-null source relation validation は、sink/source object の lifetime / generation / kind 確認後に行う。same-demux 検証と self-source 検証を lifetime check より前に置かない。
- `DemuxRuntime::set_filter_source_non_null()` と `setDataSource(null)` 相当の source disconnect は `SourceBoundaryTxn` を通す。source boundary を迂回して sink source だけを更新しない。non-null source commit は `SourceBoundaryTxn::apply()` 内に含め、boundary cleanup と commit を別 transaction に分離しない。source boundary mutation 後の failure は snapshot rollback を試行し、rollback failure は demux quarantine と `SourceBoundaryRollbackFailed` / cleanup failure として表面化する。

## 7. 静的チェックの位置づけ

- 静的チェックは規約違反候補を検出する補助確認であり、build / unit test / atest / VTS / 実機確認の代替にしない。
- 静的チェックを追加する場合は、何を検出するかを明示し、完了判定の主根拠にしない。
- テストは公開関数、戻り値、状態、診断を直接検査し、同じソースファイルの文字列検索で完了判定しない。
- close cascade helper では root object の terminal lifecycle と descendant object の terminal lifecycle を混同しない。root の unexpected terminal は invariant failure とし、already terminal descendant は close retry / finalization 対象から除外する。
- public runtime unregister preflight は destructive unregister 前に registry entry と runtime state の両方を確認する。片側 missing を `.is_some()` のみで成功扱いにしない。

- `ObjectMethodDispatchProof` の生成口を `service_runtime::object_method_txn` 外へ出さない。`TunerServiceRuntime` の public method や crate root re-export で proof を発行できる surface を置かない。
- request-builder 経路、child open、callback registration は、owner live / generation / kind 確認、builder 実行、`RuntimeExecutableRequest` validation、dispatch planning、proof 発行を `object_method_txn` helper 境界に閉じる。AIDL helper が `aidl_object_live()`、`AidlMethodAdapter::plan()`、`runtime_executable_request()` 抽出、dispatch proof 発行を手組みしない。
- 必須診断 store は bounded store と dropped counter を持つ。service lifetime 中に増え続ける startup / descrambler / child open rollback / DVR post-commit / filter callback delivery 診断を無制限 `Vec` へ直接積まない。
- `setKeyToken(non-VOID)` は session replace 前に old token を release しない。new token acquire 後に session replace を commit し、replace 失敗時は new token rollback release を composed failure として返す。old token release は replace 成功後 cleanup として行う。
- AV handle release は backing 欠落時に transient backing を生成しない。marker / backing 実体不整合は backing failure として返す。

- bounded diagnostic store は reset 時にも同じ bounded store 型を使い、`clear()` で records と dropped counter を同時に初期化する。reset 用に unbounded `Vec` へ戻してはならない。

- Dispatch-proof consumption cleanup: after a service_runtime use-case switches from `CommandPlan` / `RuntimeExecutableRequest` to `ObjectMethodExecutionToken`, remove the former command-plan façade and do not pass `ObjectMethodTxnPlan` or `ObjectMethodDispatchProof` through execute closures when the closure does not consume it. Binder-facing wrappers that only map an already-internal HAL helper to `BinderResult` must be removed unless a production AIDL call site uses them.
- `ObjectMethodExecutionToken` を受け取る service_runtime `*_for_object` use-case は、token を最初の runtime-critical operation として消費してから、`public_runtime_id_for_object_method()`、`public_entry_for_object_method()`、frontend entry 解決、owner relation 検証、source relation 検証、runtime state dependent request build を行う。token 消費前に object/runtime id を再解決しない。AIDL closure や top-level façade へ `ObjectMethodDispatchProof` を渡してはならない。

- root `ITuner` query / command は `RootQueryRequest` / `RootQueryResponse` / `RootCommandRequest` の DTO 境界を使う。`query_api.rs` に planning、unsupported / unavailable helper、mutable precedence、任意 closure executor を置かない。
- object pure query は `ObjectQueryRequest` / `ObjectQueryResponse` の DTO 境界を使う。query façade は `&mut TunerServiceRuntime` や任意 closure を AIDL 側から受け取ってはならない。frontend status / readiness query helper は `FrontendRegistryEntry` や registry/runtime tuple を返さず、service_runtime 側で `ObjectFrontendStatusSnapshot` を構築して返す。
- `PipelineDiagnostic` は failure 種別ごとの typed enum とし、required context を `Option` field bag で表現しない。typed `HalError` / `DemuxRuntimeError` / descramble policy failure を `format!(...)` 文字列だけに丸めない。
- transaction 正本の constructor / plan / commit は所有 module 外から独自 phase order を組み立てられる public API にしない。`SourceBoundaryTxn`、service_runtime-owned descrambler session transaction、`LnbLifecycleTxn` の外部 caller は owning module use-case façade を通す。

## 8. 型付き境界 hardening

- Root query response は registry entry を AIDL 層へ返してはならない。`FrontendRegistryEntry`、runtime registry entry、object table entry は service_runtime 内部の正本であり、AIDL 層へ返す場合は専用 snapshot DTO に変換する。
- Object query response は `FrontendRegistryEntry` を返してはならない。frontend status / readiness policy は service_runtime 側 DTO で確定し、AIDL 層は AIDL 型への変換だけを行う。
- `IDemux.getAvSyncHwId(filter)` のような fallible local Binder downcast は、対象 object live / generation / kind と dispatch preflight の後に実行する AIDL input conversion helper だけで扱う。query façade に arbitrary closure や `&mut TunerServiceRuntime` を渡してはならない。
- capability token / transaction plan は public enum variant や public field で偽造可能にしない。`DescramblerReplaceKeyPlan` は private fields の struct にし、replace/clear の stale plan 検出を transaction 内で固定する。
- LNB Drop leak は public close reason として選択可能にしない。Drop leak 専用入口だけから record/quarantine し、通常 close / owner-loss close の代替にしない。
- `PipelineDiagnostic` に `detail: String` fallback variant を置かない。AV delivery non-delivered outcome も typed variant に分け、PID/filter id/typed error または typed outcome を保持する。
- `transaction_registry.rs` は dispatch target mapping だけを持つ。coverage 表示・接続済み表示・stale 未接続表示を同じ表に再導入しない。

- `ObjectMethodTxnTarget` は public constructor を持たせない。AIDL 層から target を直接構築させず、service_runtime の object method/query entry point が object id / generation / kind から private target を生成する。
- Descrambler clear-key / replace-key は plan / validate / prepared token / commit の public split API を置かない。key table 操作まで含む full transaction façade に限定し、transaction 内で snapshot 再検証、session commit、token release / rollback release を固定する。
- `PipelineDiagnosticKind` のような kind-only enum を production diagnostic 生成入力にしない。`PipelineDiagnostic` typed enum を正本とし、集計・表示は pattern match で派生させる。

- object method helper は通常 / shared のどちらでも `ObjectMethodDispatchProof` を AIDL closure へ渡さない。proof は `object_method_txn` 内で即時消費し、後続の domain use-case には `ObjectMethodExecutionToken` だけを渡す。
- LNB apply は `LnbApplyTxn` を crate public re-export せず、caller-supplied generation を受け取る public API を置かない。公開境界は generation 算出を内部化した façade に限定する。


## public nullable / close / frontend count 規約

- public close は `Closed` を成功 no-op にしない。`ObjectCloseTxn` の close preflight は `Live | CleanupFailed` のみを許容し、AIDL façade は `AlreadyClosed => Ok(())` のような意味論を持たない。
- `IFilter.setDataSource(NULL)` は demux input へ戻す disconnect であり、AIDL public method implementation は nullable 引数を受けて `None` を `SourceBoundaryTxn` 経由の service_runtime use-case に到達させる。nullable helper だけを追加し、public method が常に `Some(...)` を渡す構造を完了扱いにしない。
- `IFrontend.setCallback(NULL)` / `ILnb.setCallback(NULL)` は callback unregister とし、AIDL public method implementation は nullable callback を受けて `None` を unregister 経路に到達させる。callback artifact cleanup と runtime callback registry clear を同期させ、片側失敗を成功扱いにしない。
- `IDescrambler.addPid(pid, NULL)` / `removePid(pid, NULL)` は demux-input PID claim とし、AIDL public method implementation は nullable source filter を受けて `None` を demux-input claim 経路に到達させる。source filter claim と同じ lifecycle / dispatch / duplicate 検証境界を通し、source filter 由来か demux input 由来かは typed claim source で区別する。
- `frontend_system_from_type()` は未対応 frontend type を既対応 type へ丸めない。`setMaxNumberOfFrontends()` は負数を `INVALID_ARGUMENT`、`0..=default_max(type)` を成功、範囲外を `INVALID_ARGUMENT` にし、`getMaxNumberOfFrontends()` は type ごとの default max を返す。

## 共通部品境界禁止規約

- AIDL façade に callback unregister / cleanup policy を書かない。callback artifact retain は service_runtime object-method live / dispatch preflight 後にだけ行い、preflight failure で artifact を先行保持しない。domain clear、runtime registry clear、artifact store clear は service_runtime の typed command / callback registry use-case に集約し、AIDL は callback artifact bridge の結果だけを返す。
  production helper 内で callback unregister の primary failure を `expect_err()` / panic 前提で保持してはならない。domain result は共通 cleanup entry の戻り値として合成する。
- raw descrambler session key mutator を crate 外へ公開しない。key clear / replace は full transaction façade だけを public entry にする。
  descrambler runtime の public key transaction façade は arbitrary key table trait を受けない。key table owner は service_runtime registry に限定する。
- close cascade の cleanup ordering / failure composition policy を AIDL façade に戻さない。public close は `service_runtime::object_close_txn::close_object_use_case()` で plan を作り、`finish_object_close_use_case()` で cleanup result、public runtime unregister、commit / cleanup-failed marking を確定する。AIDL 側で descendant 判定、DVR notifier 要否判定、runtime unregister 対象 kind 判定を再実装しない。
  AIDL 側で BeforeDomainCleanup / AfterDomainCleanup を読み分けて phase ordering を実装してはならない。phase ordering は service_runtime の plan executor に置く。
- `SourceBoundaryTxn` の step recording method を共通部品外へ公開しない。
- packet path diagnostic に raw `i32` PID を入れない。
  `TsPacketView` から raw PID を public に再取得できる accessor を置かない。record-index 内部 event model も `PacketPid` を保持する。
- descrambler close / owner-loss cleanup で raw key token を service_runtime 側へ読み出してから release しない。key table 操作込みの cleanup transaction façade を使う。
- packet delivery / section / PES planning helper は validated packet 由来の `PacketPid` を受け取り、raw integer PID を入口にしない。
- record index / record event path も packet-bearing production path として扱い、`TsPacketView::validate()` / `TsPacketView::parse()` を crate 外 production ingress にしない。`ValidatedTsPacket::validate()` から view と `PacketPid` を得る。
- record index parser が既に検証済み packet を受け取れる場面では、raw byte 入口を再利用せず `ValidatedTsPacket` 入口を使う。raw byte wrapper は未検証外部入力を validation 境界へ接続するためだけに置く。
- packet descramble / diagnostics path は `RuntimeRegistry::descrambler_key_table()`、key-slot-id lookup helper、raw `(claims, key_slot_id)` tuple を直接使わない。registry-owned resolved claim snapshot / keyless predicate / stale source generation predicate を使い、key table resolution を packet transaction へ漏らさない。
- descrambler runtime の packet-facing snapshot に raw key-slot id accessor を置かない。packet consumer へ渡す claim set は key table owner が解決した resolved form に限定する。
- query API で registry entry を public に返さない。
- リリース報告で「主な修正内容」という見出しを使わない。

- object close cascade は callback 登録を持ち得る object にだけ callback cleanup command を生成する。callback 未登録は close cleanup / unregister cleanup の成功扱いとし、callback store failure だけを cleanup failure として扱う。
- LNB owner-loss callback cleanup は service_runtime の close cascade plan が生成する callback artifact cleanup command に統合し、個別の runtime registry clear / artifact store clear / unhealthy marking 手順を AIDL 側に持たない。
- close cascade の低レベル begin / commit / mark / entries helper は service_runtime 内部 helper とし、public API 主経路は close_object_use_case / finish_object_close_use_case に限定する。
- TsPacketView は forgeable public field 型にしない。packet-bearing ingress は ValidatedTsPacket を正本とし、raw byte 入口は validation 境界または preflight-only に限定する。
- public query surface は DTO response を正本とし、frontend runtime/signal 中間 state helper を crate public API として公開しない。
- 追加 failure injection test file は Android.bp の rust_test srcs に明示登録する。


- callback cleanup failure 時に registry entry を clear してから unhealthy marking してはならない。artifact cleanup failure など registry に unhealthy 状態を残す必要がある失敗では、service_runtime が mark を先に所有し、clear は成功時に限定する。
- Drop leak cleanup の対象列挙・DVR notifier stop 判定を AIDL façade に書かない。service_runtime plan が返す artifact command だけを AIDL bridge が実行する。
- `ValidatedTsPacket::view()` を crate 外 public API にしない。packet-derived PID は `PacketPid` のまま helper 境界を渡し、raw PID 変換は低レベル parser / assembler 呼び出し直前だけに限定する。
- AIDL input PID と filter config TPID は packet-derived PID とは別の typed validation boundary を通す。


- packet-derived PID と設定由来 PID を同一 helper 引数で混用しない。flush / config / AIDL input の PID は dedicated validation type を通す。
- callback registration rollback の failure composition を AIDL helper に手書きしない。service_runtime の use-case に委譲する。
- descrambler key/session transaction façade は public API 主経路以外から分解利用できる visibility にしない。

## 共通部品境界閉鎖規約

- object close cleanup に domain cleanup closure injection を使わない。domain cleanup は service_runtime typed command と executor adapter を通す。
- AIDL 側は ObjectDomainCleanupCommand を生成せず、cleanup ordering と failure composition を持たない。
- descrambler crate は runtime transaction façade を public export しない。runtime/session state と key table transaction は service_runtime が所有する。
- cross-crate 制限を Rust visibility だけで表現しない。必要な境界は crate 構成と module graph で表現する。

### callback delivery failure boundary 規約

- callback artifact lookup failure を Binder delivery failure と同じ unhealthy marking path に流さない。
- `aidl_service/src/*callback_delivery*.rs` は callback delivery failure の primary `HalError` を生成して service_runtime finish use-case へ渡すだけにする。
- production code から callback artifact store を owner handle で直接 clear しない。production clear は service_runtime 発行 command を受ける artifact bridge だけにする。
- callback artifact bridge は runtime callback registry を mutate しない。runtime callback registry mutation と primary+cleanup failure composition は service_runtime に置く。
