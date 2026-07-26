# Tuner HAL2 コーディング規則

この文書は `tuner_hal2` 固有の実装規約を書く。公開契約、状態遷移、戻り値、capability/profile、VTS/product profile 方針、資源寿命、WorkerExit / WorkerFailureClassifier / ScanSessionTxn / SourceBoundaryTxn 論理契約、missing target、必須診断、descrambler transaction、Drop leak、close cascade、callback artifact、public nullable / close / frontend count、composed failure の意味は `../tuner_hal/DESIGN_JA.md` を正とする。以下で節名だけを示して`DESIGN_JA.md`と書く場合も、正本は`../tuner_hal/DESIGN_JA.md`であり、本ディレクトリの構造設計書を指さない。

旧文書で使っていた論理名は、次の正本箇所へ読み替える。存在しない節名を参照先として残さない。

| 論理名 | `../tuner_hal/DESIGN_JA.md`の正本箇所 |
|---|---|
| missing target / public close / Drop leak / close cascade | 表5、表7、表8、`close / unregister / quarantine 条件` |
| 必須診断 / best-effort telemetry / composed failure | `0-S-4. 失敗分類と波及範囲`、表6、表7、表10、表13、`失敗影響範囲` |
| descrambler transaction | 表17、表17-B |
| `WorkerExit` / `WorkerFailureClassifier` | `Tuner HAL runtime 設計契約`、`ワーカー abnormal exit と scan terminal state の固定方針`、`ワーカー失敗と所有権境界`、`ワーカー終了契約` |
| `ScanSessionTxn` | 表0-F、表19、`scan END 通知失敗の固定` |
| `SourceBoundaryTxn` | 表18、表18-B、`Stream boundary 契約` |
| callback artifact | 表7、表8、表0-S-3A |
| public nullable / frontend count | `nullable Binder 境界`、表1-D、`ITunerルートAPIの固定契約` |
| 型付き境界 | 表0-S-3、表0-S-3A |

## 1. failure / rollback / cleanup の実装規約

- cleanup、rollback、stop、join、callback、unregister、close の失敗を `let _ =`、空分岐、ログだけ、`drop(result)` で捨てない。
- primary failure 発生後の cleanup / rollback を `?` だけで呼び、cleanup failure で primary failure を無診断で上書きしない。
- primary + cleanup failure を `format!(...)` や文字列 detail だけの generic internal error に潰さない。戻り値として片方の status を選ぶ場合でも、もう片方を composed failure または必須診断から消さない。
- `FirstErrorCollector` を primary + cleanup failure composition の代替にしない。collector は同一 cleanup phase 内の cleanup step 間 first cleanup error を集める部品としてだけ使う。
- cleanup 系 top-level use-case は `CleanupExecutionReport<TStepOutcome, TFailure>` / `CleanupExecutionDiagnosticSnapshot<TRecord>` / `SharedCleanupDiagnostics<TRecord>` を共通部品として使い、all-attempt した per-step outcome を保持してから public failure 用 first-error を射影する。object close / drop-leak terminalization は `ObjectCleanupStepOutcome` / `ObjectCleanupDiagnosticRecord` adapter、frontend worker cleanup は `FrontendWorkerCleanupStepOutcome` / `FrontendWorkerCleanupDiagnosticRecord` adapter で接続する。report を `.into_result()` だけで破棄せず、shared cleanup diagnostic sink に保存してから public failure へ射影する。report 記録失敗と finish/terminalization/worker cleanup failure は composition で両方表面化する。cleanup diagnostic の production accessor は records と dropped count を同時に返し、overflow を観測不能にしない。個別 plan body で outcome vector なしの first-error collection へ戻さない。domain-specific context を `Option` field bag や `String` detail へ丸めて共通化しない。object cleanup は artifact/domain/runtime variants、frontend worker cleanup は target/step-specific variants で context を保持する。frontend tune/scan rollback cleanup と frontend close owner-loss cleanup も同じ shared cleanup execution path に載せ、snapshot restore、bound demux rollback、owned LNB close、worker/live-data cleanup outcome を `FirstErrorCollector` だけで潰してはならない。
- primary + cleanup failure の実装は共通 failure composition helper 群へ寄せる。個別 transaction body で同等の precedence 判定や文字列 detail 合成をコピーしない。
- post-allocation / post-registration failure path では、object table rollback、runtime cleanup、callback rollback、diagnostic / cleanup-failed marking を可能な限りすべて試行する。途中の `?` で後続 cleanup を飛ばさない。
- missing target の意味論は `DESIGN_JA.md` の表5、表7、表8と`close / unregister / quarantine 条件`を正とする。実装側では `Option::None` / missing target を空分岐や `.is_some()` だけで無言成功扱いにせず、許容範囲が DESIGN にない場合は failure / diagnostic helper へ接続する。
- rollback / public close / owner-loss cleanup の実装入口には、失敗を戻り値または diagnostic に接続できる operation を使う。void / best-effort-only helper を必須 cleanup の正本入口にしない。
- 必須診断と best-effort telemetry の分類は `DESIGN_JA.md` を正とする。必須診断対象の実装では、ログだけ・counter だけ・`.ok()` だけにせず、対応する diagnostic / unhealthy / quarantine / cleanup-failed helper へ接続する。
- `setKeyToken(VOID)` のように session state と token table refcount の両方を変える経路では、`DESIGN_JA.md` の descrambler transaction 契約を実装正本として参照し、AIDL 層・descrambler crate・個別 helper に同じ phase order を再定義しない。
- best-effort telemetry は primary failure を上書きしない。必須診断 store を持つ場合でも、service lifetime 中に unbounded に増える `Vec` を正本にせず、bounded store と observable dropped/failure counter に分離する。
- `Drop` 実装には object 種別固有 cleanup を書かず、`DESIGN_JA.md` の Drop leak 専用入口だけを呼ぶ。public close 相当の cleanup policy を `Drop` 側で再定義しない。

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
- close finalization / cleanup-failed marking の cascade helper では、root / descendant lifecycle 判定を `DESIGN_JA.md` の close cascade 契約から派生させる。helper 側で terminal lifecycle の新しい意味論を定義しない。
- close cleanup / finalization failure を cleanup-failed marking する場合、存在しない cleanup step 名を追加してはならない。step 対応は `DESIGN_JA.md` の close cascade 契約を正とし、helper 側で別表を持たない。
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


- `DemuxRuntime` の production public mutation method は typed request / capability token / transaction proof / transaction-owned rollback token のいずれかで phase を固定する。`mutation_token()` のような standalone public factory、token の public field / public constructor、token なしの薄い public `&mut self` mutation method、`with_mutation_token()` のような arbitrary closure executor を追加しない。service_runtime の call-site は use-case transaction から具体的な demux domain API へ接続し、任意 closure で mutation capability を貸与しない。

- Rust visibility は実装規約として扱う。`pub` / `pub(crate)` / module-private の可否は、DESIGN_JA.md ではなく本節で判定する。crate 間 DTO / typed request / read-only snapshot / AIDL DTO 変換用 accessor は `pub` を許容するが、state mutation、rollback restore、queue export、registry/session mutation、object target construction、transaction plan construction を外部 caller が組み立てられる public constructor / public field / public enum variant / direct import surface にしない。
- production AIDL / binder / domain_request から demux runtime mutation façade を直接 import / call しない。demux runtime mutation は service_runtime use-case / transaction module からだけ呼ぶ。例外は read-only DTO、AIDL DTO 変換、test cfg、または low-level parser/value object に限る。
- 検証済み状態、dispatch 済み状態、transaction plan、ledger guard、rollback guard などを表す型は capability token として扱う。
- capability token は、状態検証または予約を所有する共通部品だけが発行する。外部 caller が public constructor / public enum variant / public field struct literal で偽造できる形にしない。crate 間 domain API の typed request constructor は `pub` を許容する。typed request は capability token ではなく operation DTO として使ってよく、request を forge しても snapshot 本体、one-shot token、queue export handle、registry entry、session map、再利用可能 restore 権限、任意状態復元権限を得られないことを実装条件にする。
- demux crate の public mutation façade が `*_from_typed_request` で、raw filter id / dvr id / config / reason だけを持つ request を受け取っていても、それだけで違反にしない。違反は、AIDL / binder / domain_request が `service_runtime` use-case を迂回して façade を直接呼べる場合、request が token / proof / handle を偽造できる場合、または demux method 側が lifecycle / subtype / source relation / queue availability の再確認を行わない場合である。
- 一回性 token に `Clone` / `Copy` を付けない。consume-by-value の API で消費する。rollback token は opaque id とし、snapshot 本体を外へ持たせず、runtime 内部 ledger で one-shot consumeする。rollback prepare request は token ではないため `Clone` / `Copy` の有無で authority 漏洩と判定しない。rollback token に read-only generation accessor を置く場合は consistency check 専用とし、restore 先、snapshot 本体、旧token cleanup order、key-slot 等の authority を読ませない。
- 複数回利用可能な値が必要な場合は token とは別の read-only descriptor 型に分離する。
- single-variant enum や将来用 variant で capability token の状態機械を装わない。現在の証跡が1状態だけなら、対象情報を直接保持する struct にする。

## 6. worker / callback / source boundary 実装規約

- frontend worker の blocking join を `TunerServiceRuntime` lock 保持中に実行しない。runtime lock 内では cancel 設定済み join ticket 取得までに限定し、join は lock 外で行う。
- worker start 成功後に fallible commit を置く場合、commit failure path で起動済み worker を stop/join し、runtime snapshot / demux snapshot rollback を試行する。
- frontend worker replacement では、旧 worker stop/join 後に `prepare_frontend_worker_generation()`、bound demux rollback token preparation などの fallible generation candidate calculation / pre-start preparation を初めて実行しない。旧 worker stop 前に generation candidate calculation / rollback-token preparation / request preflight できるものは replacement prepare ticket へ含める。candidate は runtime state へ予約 commit した generation ではないため、post-complete install / begin step は `commit_generation(candidate)` 相当で単調増加条件を再検証する。complete 失敗は `CompleteReplacement` 相当の typed step で旧 worker stopped / new worker not-started / rollback-or-no-restart decision と primary failure を `FrontendWorkerCleanupDiagnosticRecord` または同等の typed transaction diagnostic に記録し、public error だけで失わせない。candidate を使う post-complete install / begin / worker start failure は start rollback diagnostic に `CompleteReplacement` context を含め、stopped old generation と candidate generation を失わせない。
- `CallbackRegistryUpdate::Missing` は rollback / public close / owner-loss cleanup で空分岐にしない。callback store の削除対象と runtime registry clear 結果を照合する。
- AIDL delivery façade は callback artifact lookup、event conversion、Binder callback execution を phase として区別し、phase meaning は `DESIGN_JA.md` の callback artifact lookup / delivery failure 境界を正とする。delivery module 側で unhealthy marking 条件を再定義しない。
- callback artifact store clear の production entry は、owner callback cleanup では `OwnerCallbackCleanupArtifactCommand`、service boot reset では `CallbackArtifactResetCommand` を受ける bridge に限定する。`AidlObjectHandle` を直接受けて callback store を clear する helper と all-artifact reset raw helper は private raw helper または `#[cfg(test)]` helper に限定し、production code から直接呼ばない。
- callback artifact store、DVR notifier store、filter event dispatcher bridge、drop-leak diagnostic store は process-global `OnceLock` / `static Mutex` に置かない。これらは `AidlServiceContext` または `TunerServiceRuntime` instance field の lifetime に閉じる。drop-leak diagnostic store の lock poison は `poisoned.into_inner()` で吸収せず、context-owned failure counter または reset failure として表面化する。
- `IFilter.setDataSource(source)` の non-null source relation validation は service_runtime use-case へ置き、AIDL method body に same-demux / self-source / lifecycle precedence を手書きしない。
- `DemuxRuntime::set_filter_source_non_null()` と `setDataSource(null)` 相当の source disconnect は `SourceBoundaryTxn` を通す。source boundary の状態遷移、rollback、quarantine 条件は `DESIGN_JA.md` を正とし、runtime helper 側で別定義しない。

## 7. 静的チェックの位置づけ

- 静的チェックは規約違反候補を検出する補助確認であり、build / unit test / atest / VTS / 実機確認の代替にしない。
- 静的チェックを追加する場合は、何を検出するかを明示し、完了判定の主根拠にしない。
- テストは公開関数、戻り値、状態、診断を直接検査し、同じソースファイルの文字列検索で完了判定しない。
- close cascade helper では root object と descendant object の lifecycle 判定を混同しない。判定の意味論は `DESIGN_JA.md` を正とし、CODE_CONVENTION 側で再定義しない。
- public runtime unregister preflight は destructive unregister 前に registry entry と runtime state の両方を確認する。片側 missing を `.is_some()` のみで成功扱いにしない。

- `ObjectMethodDispatchProof` の生成口を `service_runtime::object_method_txn` 外へ出さない。`TunerServiceRuntime` の public method や crate root re-export で proof を発行できる surface を置かない。
- request-builder 経路、child open、callback registration は、owner live / generation / kind 確認、builder 実行、`RuntimeExecutableRequest` validation、dispatch planning、proof 発行を `object_method_txn` helper 境界に閉じる。AIDL helper が `aidl_object_live()`、`AidlMethodAdapter::plan()`、`runtime_executable_request()` 抽出、dispatch proof 発行を手組みしない。
- 必須診断 store は bounded store と dropped counter を持つ。service lifetime 中に増え続ける startup / descrambler / child open rollback / DVR post-commit / DVR status notifier reset cleanup / callback artifact runtime split / demux transaction / queue descriptor query / filter callback delivery / frontend callback delivery 診断を無制限 `Vec` へ直接積まない。production accessor は records だけでなく dropped counter も観測できる snapshot を返す。
- `setKeyToken(non-VOID)` の phase order は `DESIGN_JA.md` の descrambler transaction 契約を正とする。AIDL 層、descrambler crate、個別 helper に同じ順序判定を再定義せず、service_runtime の full transaction use-case へ接続する。
- AV handle release の backing 欠落・marker 不整合の意味論は `DESIGN_JA.md` の AV shared backing 契約を正とする。実装側では transient backing を生成する個別回避コードを置かず、AV shared backing runtime の outcome / diagnostic helper へ接続する。

- bounded diagnostic store は reset 時にも同じ bounded store 型を使い、`clear()` で records と dropped counter を同時に初期化する。clear failure は診断だけで成功扱いにせず、該当 reset / cleanup の failure composition に含める。reset 用に unbounded `Vec` へ戻してはならない。

- Dispatch-proof consumption cleanup: after a service_runtime use-case switches from `CommandPlan` / `RuntimeExecutableRequest` to `ObjectMethodExecutionToken`, remove the former command-plan façade and do not pass `ObjectMethodTxnPlan` or `ObjectMethodDispatchProof` through execute closures when the closure does not consume it. Binder-facing wrappers that only map an already-internal HAL helper to `BinderResult` must be removed unless a production AIDL call site uses them.
- `ObjectMethodExecutionToken` を受け取る service_runtime `*_for_object` use-case は、token を最初の runtime-critical operation として消費してから、`public_runtime_id_for_object_method()`、`public_entry_for_object_method()`、frontend entry 解決、owner relation 検証、source relation 検証、runtime state dependent request build を行う。token 消費前に object/runtime id を再解決しない。AIDL closure や top-level façade へ `ObjectMethodDispatchProof` を渡してはならない。

- root `ITuner` query / command、object pure query、packet diagnostic、transaction 正本の具体的な公開契約は `DESIGN_JA.md` の型付き境界契約を正とする。実装側では DTO façade / typed diagnostic constructor / owning-module use-case façade を使い、`query_api.rs` への任意 closure executor 追加、registry entry 返却、`format!(...)` だけの diagnostic 丸め、外部 caller が phase order を組める public constructor / plan / commit 追加を行わない。typed diagnostic record を bounded production store に保存したうえで public `HalError` detail を表示用に整形することは許容するが、文字列 detail だけを唯一の診断正本にしてはならない。demux transaction の public `HalError` detail と typed diagnostic record は diagnostic id で対応付ける。diagnostic record の保存が成功しただけの non-quarantine configure failure を synthetic cleanup failure として合成してはならず、public error の同一 status category を保った detail へ diagnostic id を付与する。filter runtime stop / flush のように pipeline mutation 後に queue clear / queued payload clear / AV backing flush が続く operation は、失敗 step と rollback / skip decision を typed `FilterRuntimeOperationReport` に残す。AIDL object method façade も service_runtime transaction façade を通し、raw demux runtime helper の result だけを返して typed report を捨ててはならない。dynamic source-boundary failure で AOSP status として unsupported/unavailable を維持する必要がある場合も、static-only `HalError::Unsupported(&'static str)` へ落とさず、diagnostic id を保持できる dynamic detail variant を使う。read-only DTO の `into_parts()` / accessor は AIDL DTO 変換境界で必要な場合に限り許容するが、DTO から runtime state / queue mutable handle / one-shot export handle を取り出せる形にしない。

## 8. 型付き境界 hardening

- Root / object query、capability token、transaction phase、packet diagnostic、LNB lifecycle reason、transaction registry の意味論は `DESIGN_JA.md` の表0-S-3、表0-S-3Aと各公開API状態表を正とする。この節では意味論を再定義せず、実装時の禁止形だけを列挙する。
- AIDL 変換層は registry entry、runtime entry、object table entry を返す helper を新設しない。AIDL 型へ渡す直前は、service_runtime が作成した snapshot DTO だけを入力にする。
- query façade は `&mut TunerServiceRuntime`、任意 closure、registry/runtime tuple を AIDL 側から受け取らない。fallible local Binder downcast は、object live / generation / kind と dispatch preflight 後の AIDL input conversion helper に限定する。
- capability token / transaction plan は public enum variant、public field、public constructor で偽造可能にしない。外部 caller へ phase order を組ませる代わりに、owning module use-case façade へ接続する。crate 間 domain API の typed request はこの禁止対象ではないが、request 単体で capability token や rollback snapshot 本体を構築できる形にしない。
- Drop leak、descrambler clear / replace、LNB apply、source boundary、transaction registry、pipeline diagnostic の個別契約は `DESIGN_JA.md` を参照し、CODE_CONVENTION 側で別の状態名、戻り値、phase order、必須 context を定義しない。
- `ObjectMethodDispatchProof` を AIDL closure へ渡さない。proof は `object_method_txn` 内で即時消費し、後続の domain use-case には `ObjectMethodExecutionToken` だけを渡す。
- `PipelineDiagnosticKind` のような kind-only enum、`detail: String` fallback variant、registry entry を返す query helper、coverage / 接続済み表示を持つ transaction registry 表を再導入しない。

## public nullable / close / frontend count 実装入口規約

- public nullable API、public close、frontend count API の状態遷移・戻り値・到達条件は `DESIGN_JA.md` の`nullable Binder 境界`、表1-D、表5、`ITunerルートAPIの固定契約`を正とする。
- AIDL public method implementation は、nullable 引数を helper 内で非 nullable に潰さず、`None` を service_runtime use-case へ到達させる。
- AIDL façade は close / callback unregister / demux-input PID claim / frontend count の意味論を再定義せず、service_runtime use-case 呼び出しと Binder status bridge に限定する。
- `frontend_system_from_type()` のような入力変換 helper は、未対応 type の丸め込みや戻り値 policy を持たず、DESIGN_JA.md の契約に従う service_runtime request/command DTO へ接続する。

## 共通部品境界禁止規約

- AIDL façade に callback unregister / cleanup policy を書かない。callback artifact retain は service_runtime object-method live / dispatch preflight 後にだけ行い、preflight failure で artifact を先行保持しない。domain clear、runtime registry clear、artifact store clear は service_runtime の typed command / callback registry use-case に集約し、AIDL は callback artifact bridge の結果だけを返す。
  production helper 内で callback unregister の primary failure を `expect_err()` / panic 前提で保持してはならない。domain result は共通 cleanup entry の戻り値として合成する。
- raw descrambler session key mutator を crate 外へ公開しない。key clear / replace は full transaction façade だけを public entry にする。
  descrambler runtime の public key transaction façade は arbitrary key table trait を受けない。key table owner は service_runtime registry に限定する。
- close cascade の cleanup ordering / failure composition policy を AIDL façade に戻さない。AIDL 側で descendant 判定、DVR notifier 要否判定、runtime unregister 対象 kind 判定を再実装せず、`DESIGN_JA.md` が定める service_runtime close use-case へ接続する。
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

- object close cascade の callback cleanup command 生成条件は `DESIGN_JA.md` を正とする。実装側で object kind ごとの callback 要否を AIDL façade に手書きしない。
- LNB owner-loss callback cleanup は service_runtime plan が生成する callback artifact cleanup command に接続し、AIDL 側に個別の runtime registry clear / artifact store clear / unhealthy marking 手順を持たない。
- close cascade の低レベル begin / commit / mark / entries helper は service_runtime 内部 helper とし、public API 主経路は close_object_use_case / finish_object_close_use_case に限定する。
- TsPacketView は forgeable public field 型にしない。packet-bearing ingress は ValidatedTsPacket を正本とし、raw byte 入口は validation 境界または preflight-only に限定する。
- public query surface は DTO response を正本とし、frontend runtime/signal 中間 state helper を crate public API として公開しない。


- callback cleanup failure の registry clear / unhealthy marking ordering は `DESIGN_JA.md` を正とする。AIDL helper は service_runtime finish use-case の戻り値を橋渡しし、ordering を再実装しない。
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

- callback artifact lookup failure と Binder delivery failure の区別は `DESIGN_JA.md` を正とする。delivery module は phase を保持した primary error を service_runtime finish use-case へ渡すだけにする。
- `aidl_service/src/*callback_delivery*.rs` は callback delivery failure の primary `HalError` を生成して service_runtime finish use-case へ渡すだけにする。runtime lock poison により finish use-case に到達できない場合は、AIDL service context の typed fallback diagnostic store へ記録し、記録不能を silent return しない。
- production code から callback artifact store を owner handle または all-artifact raw helper で直接 clear しない。production clear は service_runtime 発行 command を受ける artifact bridge だけにする。
- callback artifact bridge は runtime callback registry を mutate しない。runtime callback registry mutation と primary+cleanup failure composition は service_runtime に置く。

## worker / callback / query / packet 境界実装規約

- frontend worker の join 前後をまたぐ処理では、join 後の commit / rollback / cleanup を frontend id だけで実行してはならない。旧 worker を supersede stop する前に検証可能な tune / scan request precondition は stop ticket 発行前に評価し、invalid request failure で既存 worker を停止してはならない。runtime lock 内で発行された transition / stop ticket を consume する complete helper だけが runtime mutation を実行する。ticket は public constructor、public fields、Clone、Copy を持ってはならない。blocking join 中に TunerServiceRuntime lock を保持してはならない。
- DVR status callback missing、notifier unavailable、callback store lock poison を silent success として返してはならない。public start/stop の戻り値を post-commit failure で反転させない場合でも、DVR post-commit notification diagnostic へ typed phase として記録する。runtime lock 再取得に失敗する post-commit accounting 経路では、cleanup に限らず initial delivery / runtime policy skip / Binder delivery / notifier preflight / notifier terminal / notifier cleanup を `AidlServiceContext` が保持する shared DVR post-commit diagnostic sink へ fallback 記録する。fallback 記録自体に失敗した場合も shared diagnostic snapshot の record failure counter へ反映し、`let _ =` で accounting failure を完全破棄してはならない。bool 戻り値で delivered / missing / skipped を表現しない。notifier cleanup / runtime policy skip / artifact lookup は Binder delivery failure と同じ unhealthy marking 対象にせず、phase で識別する。`JoinHandle::join()` 後の terminal failure を retryable handle として保持する設計を要求しない。
- callback artifact store mutation と RuntimeCallbackRegistry mutation を別々の AIDL helper 手順として実装してはならない。artifact mutation は runtime が発行した command だけを実行し、結果を runtime finish へ返す。artifact failure、runtime finish failure、registry missing を let _、空分岐、ログだけで捨ててはならない。artifact mutation 成功後の runtime finish failure は callback artifact/runtime split diagnostic に記録し、成功扱いにしてはならない。
- artifact mutation 後に runtime finish lock が失敗する経路では、AIDL 側ローカル store ではなく `TunerServiceRuntime` instance から clone した shared callback artifact/runtime split diagnostic sink に記録する。runtime lock failure を理由に artifact attempt outcome を失ってはならない。
- TunerServiceRuntime の public query surface に registry entry、runtime state、signal state、中間 helper を返す wrapper を追加してはならない。query_api.rs に残せる public façade は RootQuery / ObjectQuery DTO executor だけとする。DTO executor 内部の補助関数は private または crate-private helper とし、crate public API にしない。
- validated typed id の raw 値 accessor は production mutation / validation / routing path で使ってはならない。PacketPid の raw 値を取り出す production conversion は `to_i32_for_aidl_boundary()` だけとし、AIDL DTO 変換境界に限定する。service_runtime で descrambler claim と packet path を照合する場合は、検証済み `DescramblerPid` から `PacketPid` へ入る一方向 typed bridge だけを使い、`PacketPid` から raw PID を取り出す accessor を追加してはならない。`get()` のような汎用名は禁止する。ログと診断表示は raw accessor ではなく typed diagnostic context から生成する。Display 実装は表示整形専用であり、routing、validation、diagnostic classification に使ってはならない。PipelineDiagnostic::pid() -> Option<i32> のように typed diagnostic を raw Option field bag へ戻す accessor を追加してはならない。
- diagnostic record に複数の Option field を並べ、kind / phase / optional ids の組み合わせで意味を復元する field bag を追加してはならない。診断種別ごとに必須 context を持つ variant-specific struct / enum を使う。kind-only enum と optional ids で production diagnostic の意味を復元してはならない。
