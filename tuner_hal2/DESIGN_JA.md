# tuner_hal2 DESIGN_JA.md

## 1. 文書スコープと既存契約との関係

この文書は、tv直下 `開発規則.md` が許可した `tuner_hal2` 固有の構造差分、および既存 `tuner_hal/DESIGN_JA.md` の公開契約を `tuner_hal2` の実体名へ接続するために必要な補足だけを記載する。既存契約を再定義する場合は、既存契約との差分理由と、どちらを正とするかを同じ節で明記する。

DESIGN_JA.md は責務境界、状態遷移、phase order、failure precedence、resource lifetime を扱う。`let _`、`?`、`Option::None`、`format!(...)` のような具体的な実装規約は `CODE_CONVENTION.md` に置く。

本書に出てくるファイル名、module名、型名は、LLM/作業者が責務正本を取り違えないための実装境界アンカーである。単なる例示ではない。rename / split / merge を行う場合は、同じ変更で本書のアンカーも同期し、状態遷移、戻り値、資源寿命、failure precedence の正本がどこへ移ったかを明記する。ただし、syntax、import、visibility、禁止APIなどの実装規約は `CODE_CONVENTION.md` へ置き、本書へ重複定義しない。

## 2. レイヤ構造とファイル分割境界
本節は巨大制御層を再発させないための tuner_hal2 固有の配置規則である。ファイル分割は Rust の通常 `mod` による module 分割を前提とする。

### 2.1 AIDL 層


`aidl_service::object_runtime` は AIDL object method executor façade であり、transaction 正本を新規に所有しない。置いてよいものは、AIDL method plan 入口、runtime lock / shared runtime executor の private helper、Binder status 変換境界、service_runtime use-case 呼び出し façade、close / unavailable / query / callback registration 入口の façade に限る。

`object_runtime` に root / child open rollback 本体、callback artifact registration の状態遷移本体、domain cleanup policy、object table lifecycle commit 本体、registry / callback / domain cleanup の個別 rollback 手順、AIDL method ごとの例外的 phase order を追加してはならない。`object_runtime` に残せる callback registration 関連処理は façade 入口、runtime lock 境界、status bridge、typed retain glue の呼び出しに限る。状態遷移本体は callback_store / RuntimeCallbackRegistry / service_runtime domain transaction 側へ置く。

`aidl_service/src/tuner_service.rs` は root `ITuner` service の AIDL DTO 変換、root object open/query/command DTO の service_runtime 呼び出し、Binder status 変換だけを所有する。root method planning、dispatch preflight、unsupported / unavailable status precedence、read-only snapshot 取得は `service_runtime::root_method_txn` と `RuntimeQuery` 側へ閉じる。AIDL object lookup、local binder downcast、source filter handle 検証などの service-level helper は `aidl_service/src/tuner_service/support.rs` へ分ける。child object の公開 AIDL trait 実装は `aidl_service/src/tuner_service/*_methods.rs` へ分ける。

| ファイル | 所有する実装 | 所有外の実装 |
|---|---|---|
| `aidl_service/src/tuner_service.rs` | `TunerAidlService`、`ITuner`、root open/query/command の DTO 変換、service_runtime root façade 呼び出し、AIDL 型変換 | root method planning、dispatch preflight、unsupported / unavailable status precedence、read-only snapshot 本体、child trait 実装、service-level helper を戻さない |
| `aidl_service/src/tuner_service/support.rs` | AIDL object lookup、local binder downcast、AIDL method call DTO 生成、source filter owner/public id helper | AIDL trait 実装、root method planning、unsupported / unavailable status precedence、runtime状態遷移、Binder status helper再定義を置かない |
| `aidl_service/src/child_object_open.rs` | demux配下 filter / DVR child object open の共通手順、callback registration、rollback | `openFilter()` / `openDvr()` AIDL method body へ child allocation / callback rollback 手順をコピーしない。request-builder 版 child open は `execute_shared_object_runtime_use_case_with_request_builder()` を使い、`service_runtime::object_method_txn` の object live / generation / kind、request build、`RuntimeExecutableRequest` validation、dispatch planning 境界を通す。dispatch 済みの child allocation は `service_runtime::object_method_txn` が dispatch proof を内部消費して発行した `ObjectMethodExecutionToken` を、service_runtime の統一 `*_for_object` use-case へ渡して接続する。AIDL 層や個別 method body が dispatch proof を自由生成してはならない。dispatch proof 専用の別名 public entry point を増やしてはならない。callback retain と AIDL object 生成失敗時 rollback はこの helper が所有する。Binder status helperを再定義しない |
| `aidl_service/src/tuner_service/frontend_methods.rs` | `impl IFrontend for FrontendAidlObject` | runtime registry の直接所有を増やさない |
| `aidl_service/src/tuner_service/demux_methods.rs` | `impl IDemux for DemuxAidlObject` | filter/DVR/descrambler 状態遷移を直接所有しない |
| `aidl_service/src/tuner_service/filter_methods.rs` | `impl IFilter for FilterAidlObject` | callback/FMQ/AV cleanup failure を空消費しない |
| `aidl_service/src/tuner_service/dvr_methods.rs` | `impl IDvr for DvrAidlObject` | FMQ/EventFlag commit 条件を局所実装しない |
| `aidl_service/src/tuner_service/descrambler_methods.rs` | `impl IDescrambler for DescramblerAidlObject` | token / PID lifetime を AIDL 層で所有しない |
| `aidl_service/src/tuner_service/lnb_methods.rs` | `impl ILnb for LnbAidlObject` | LNB backend safe-state apply を Drop 経路へ戻さない |

AIDL method body は object handle 取得、service_runtime use-case 呼び出し、`error_bridge` による Binder status 変換だけを行う。AIDL input の domain request 変換が失敗し得る場合は、method body で先に実行せず、request-builder closure として use-case helper へ渡す。request-builder closure は object live / generation / kind 確認と同じ runtime critical section 内で実行し、close と input conversion failure が競合した場合に builder failure が lifecycle より先へ出る構造にしない。builder 成功後の `RuntimeExecutableRequest` validation と dispatch planning も `service_runtime::object_method_txn` の境界で行い、AIDL 側 adapter が method planning / `RuntimeExecutableRequest` 抽出 / validation / dispatch planning の順序を所有してはならない。AIDL helper は `AidlMethodCall` と request-builder result だけを `service_runtime::object_method_txn` へ渡す。request-builder helper の execute closure は `service_runtime::object_method_txn` が dispatch planning 成功後に `ObjectMethodDispatchProof` を内部で即時消費して発行した `ObjectMethodExecutionToken` だけを受け取る。domain operation 側はその execution token を統一 `*_for_object` use-case へ渡し、同じ `plan_object_method_dispatch()` を再実行してはならない。通常経路の dispatch 必須 policy は service_runtime 内部でのみ生成し、AIDL 層や個別 method body が直接生成してはならない。dispatch proof 専用 public entry point を新設してはならない。runtime registry / object table / callback registry の状態遷移を AIDL method body へ新規追加する場合は、対応する service_runtime use-case function を先に追加する。 root `ITuner` の query / command は `RootQueryRequest` / `RootQueryResponse` / `RootCommandRequest` の DTO 境界を通し、root method planning、`RuntimeExecutableRequest` validation、dispatch preflight、unsupported / unavailable precedence、read-only snapshot 取得は `service_runtime::root_method_txn` が所有する。root query の snapshot 取得は `RuntimeQuery<'_>` の immutable method だけを使い、`query_api.rs` に arbitrary closure、unsupported helper、mutable API precedence を置いてはならない。object pure query は `ObjectQueryRequest` / `ObjectQueryResponse` の DTO 境界を通し、`execute_object_query_use_case()` は query closure を受け取らず、DTO request を `service_runtime::object_method_txn` へ渡して `RuntimeQuery<'_>` だけで read-only snapshot を生成する。pure query は `ObjectMethodDispatchProof` を発行しない。AIDL query façade が `&mut TunerServiceRuntime`、任意 closure、または direct runtime accessor を受け取る構造を作ってはならない。

`IFilter.setDataSource(source)` の source handle 取得は AIDL 層で local Binder object を domain request builder へ変換するだけに留める。source / sink の lifetime、generation、kind、owner demux、自己参照、dispatch planning、commit / rollback は service_runtime の demux/filter/DVR use-case が所有する。source / sink の owner demux 不一致や自己参照を、sink object lifetime / generation 確認より前に判定してはならない。

状態変更を伴う AIDL method では、AIDL 層で `ensure_open()` → public id 解決 → runtime validate → plan-only status 写像 → commit を別々に組み立ててはならない。object wrapper / tuner service に plan-only public helper や public thin wrapper を置かず、object method executor の正本は `aidl_service::object_runtime` façade に限定する。unavailable / unsupported 経路は `plan_unavailable_object_method_use_case()`、object pure query は `ObjectQueryRequest` / `ObjectQueryResponse` を使う `execute_object_query_use_case()`、root query / command は `RootQueryRequest` / `RootQueryResponse` / `RootCommandRequest` を使う `service_runtime::root_method_txn` へ寄せる。supported mutating method は object handle / generation、domain request または domain request builder、`service_runtime::object_method_txn` が dispatch proof を内部消費して発行した `ObjectMethodExecutionToken` を service_runtime の object-handle based use-case façade へ渡す。service_runtime の統一 `*_for_object` use-case は proof を消費して domain operation へ進み、同じ `plan_object_method_dispatch()` を再実行しない。object live/generation 検証、method planning、runtime validate、dispatch planning は `object_method_txn` または root method transaction 境界で一度だけ行い、domain use-case は state reservation、commit/rollback/quarantine を所有する。fallible request-builder は object live/generation 検証と同一 runtime critical section 内で実行する。AIDL入力のdomain変換に現在runtime stateが必要な場合（例: `IFilter.configure()` の current open type）は、AIDL層で先にruntime queryを行わず、service_runtime use-case へ純粋な変換closureまたは marker request を渡し、runtime state取得とdomain request確定を同一 method transaction 内で行う。callback rollback だけを包む wrapper、profile validation だけを包む wrapper、close helper だけを包む wrapper も同じ非許容類型とする。

ObjectMethodTxn は全 object method を機械的に包む共通層ではない。ObjectMethodTxn / request-builder helper が必須なのは、AIDL入力変換が失敗し得る method、child open、source relation、callback registration、object pure query、unavailable / unsupported / plan-only など、status precedence を壊しやすい経路である。object pure query は `ObjectQueryRequest` / `ObjectQueryResponse` に閉じ、dispatch preflight 後に `RuntimeQuery<'_>` の read-only snapshot だけを使う。query closure、`&mut TunerServiceRuntime` を受け取る façade、query 用 request-builder façade は置かない。request-builder を持たない単純 mutating method は、service_runtime use-case 側で live/generation/kind 確認、dispatch planning、domain operation の順序を守ればよく、不要に `ObjectMethodTxnPlan` / `ObjectMethodDispatchProof` token へ移行しない。fallible な domain request は object live/generation 検証後、かつ同一 runtime critical section 内で確定する。builder failure では dispatch planning / domain operation を実行せず、dispatch planning failure では domain operation を実行しない。`ObjectMethodDispatchProof` は dispatch planning 完了を示す一回性内部証跡であり、`service_runtime::object_method_txn` が dispatch planning 成功直後に同 module 内で即時消費する。AIDL closure、top-level use-case façade、domain operation へ proof を渡してはならない。後続 domain operation へ渡せるのは `ObjectMethodExecutionToken` だけとし、token は対象 `AidlObjectId` / generation / kind を直接保持する。消費側の統一 `*_for_object` use-case は同じ対象に対してだけ `consume_for_object()` で token を消費する。unavailable / unsupported / plan-only 経路は domain operation を実行しないため、`ObjectMethodTxnPlan` だけを返す plan-only helper を使い、`ObjectMethodDispatchProof` を発行して捨ててはならない。`ObjectMethodTxnPlan` は `object_method_txn` 内で生成され、`AidlMethodCall` から AIDL transaction table を引いて得た `CommandPlan` と `RuntimeExecutableRequest` を束ねる。AIDL 層で `AidlMethodAdapter::plan()` や `runtime_executable_request()` 抽出を呼んではならない。`CommandPlan` は caller が transaction 名を持ち込んで照合する値ではなく、transaction 名は AIDL transaction table 側だけが所有する。plan-unavailable / unsupported 経路も同じ `ObjectMethodTxnPlan` planning 境界を通す。旧 service_runtime 側低レベル executor module は廃止済みである。runtime lock / shared runtime executor は `aidl_service::object_runtime` の private helper とし、個別 AIDL method body や service_runtime public API へ低レベル closure executor として露出しない。


ObjectCloseTxn は close 開始遮断の最小共通部品とする。close begin preflight は `Live | CleanupFailed` のみを closeable とし、`Closed` / `Closing` / `Quarantined` を拒否する。`Closed` に対する public `close()` は AOSP-facing API 境界で成功 no-op にせず、close 済み object への再アクセスとして failure を返す。close 開始時点で対象 object cascade を `Closing` へ遷移させ、callback cleanup と domain cleanup hook を実行し、cleanup 成功時に `Closed`、cleanup 失敗時に `CleanupFailed` へ遷移する。callback cleanup や Binder callback は runtime lock 内で実行せず、object table 上の `Closing` によって並行 public method を遮断する。frontend 固有の worker停止 / LNB owner-loss cleanup、および LNB public close の backend/runtime cleanup もこの domain cleanup hook として接続し、service_runtime 側で先に begin close しない。

frontend tune / scan / stop / close / setLnb の境界は、frontend id / registry entry を AIDL 層で先に組み立てる thin wrapper ではなく、service_runtime の public frontend use-case façade へ接続する。AIDL object handle から public runtime id / owner relation / object lifecycle を解決する処理は service_runtime query façade、`object_lifecycle` façade、または object-handle based use-case transaction を通す。

通常の supported public API planning は `AidlMethodCall::PublicApi` を使う。unsupported-by-design API の戻り値生成だけ `AidlMethodCall::UnsupportedPublicApi` を使う。query / open / 状態取得系の supported API を unsupported planning に流用しない。

### 2.2 service_runtime 層

#### 2.2.1 `boot.rs` の責務

`service_runtime/src/boot.rs` は `TunerServiceRuntime` 定義、boot/probe、object table / callback registry / diagnostic accessor、command dispatch を所有する。`service_runtime/src/boot.rs` に通常 operation を追加してはならない。`TunerServiceRuntime` の field は private のままとし、operation module へ渡す目的で `pub(crate)` 化しない。

#### 2.2.2 top-level `*_ops.rs` の責務

公開 operation wrapper は top-level `service_runtime/src/*_ops.rs` へ置く。top-level `*_ops.rs` は `TunerServiceRuntime` の private field に直接触れない。単純な単一 runtime transaction は `boot` child module の `transact_*` helper を直接呼び、複数stepの owner/object/open rollback を所有する use-case だけ domain transaction context を呼ぶ。read-only は query wrapper を呼ぶ。

| ファイル | 所有する公開 wrapper | 呼び出す context |
|---|---|---|
| `service_runtime/src/frontend_ops.rs` | AIDL/service_runtime 境界に必要な frontend public use-case façade のみ。object handle から public runtime id / lifecycle / dispatch proof consumption / domain transaction へ接続する tune / scan / stop tune / stop scan / close cleanup / setLnb / clear live data など、phase order を所有する use-case 境界だけを残す | `set_frontend_lnb_object_use_case()`。frontend worker 内部操作は `FrontendTxn<'_>` を直接使い、同じ引数を横流しするだけの one-line wrapper、callback rollback だけを包む public wrapper、profile validation だけを包む public wrapper を置かない |
| `service_runtime/src/demux_filter_dvr_ops.rs` | demux/filter/DVR allocation/register/configure/start/stop/flush/source/DVR。`IFilter.setDataSource(source)` では sink/source object の lifetime・generation・kind 確認後に owner demux 同一性と自己参照を検証し、同一demux内のfilter接続グラフだけをcommit対象にする | 単純 operation は `transact_*` helper。child open / rollback だけ `DemuxFilterDvrTxn<'_>` |
| `service_runtime/src/descrambler_ops.rs` | descrambler allocation/demux/key/PID/unregister/owner-loss cleanup。clear-key / replace-key は外部 caller が plan / prepared token / commit を組み立てられない full transaction façade だけを公開する | `DescramblerTxn<'_>` |
| `descrambler/src/runtime/key_table.rs` | descrambler key token から key slot id への登録・参照・参照数管理 | slot id allocation は key table 内で fail-closed とし、上限到達時に既存 slot id を再発行してはならない。token table / slot table / refcount を部分更新しない |
| `descrambler/src/runtime/session_txn.rs` | descrambler session の demux binding、key replacement / clear、PID claim、cleanup | `setKeyToken(non-VOID)` は public split API で plan / commit を外へ出さず、service_runtime registry の full transaction façade だけから実行する。transaction 内では session replace plan を作り、新 token acquire 成功後に session replace を commit し、その後で old token release を行う。old token release を session replace より前に実行してはならない。session replace failure では acquired new token を rollback release し、rollback release failure は composed failure として返す。`setKeyToken(VOID)` も public split API や externally observable prepared token を置かず、full transaction façade だけから実行する。clear path は transaction 内で old token snapshot と stale plan 検証を行い、session clear を commit してから old token release を行う。old token / old key slot を caller が accessor で観測して独自 cleanup order を組める surface を置かない。old token release failure は `KeyTokenReleaseFailed` diagnostic として返し、session は既に keyless 状態として扱う。demux binding の rebind は既存 PID claims と同時に整合しなければならず、demux id / generation が変わる場合は stale PID claims を clear する |
| `service_runtime/src/root_object_ops.rs` | ITuner root object open の public façade / transaction境界 | AIDL層に runtime allocation / AIDL object table registration / rollback をコピーしない |
| `service_runtime/src/root_method_txn.rs` | ITuner root query / root command の method planning、dispatch preflight、DTO request / response 境界 | `query_api.rs` に planning、unsupported / unavailable status helper、mutable precedence を置かない。AIDL method body へ `AidlMethodAdapter::plan()` / `RuntimeExecutableRequest` 抽出を戻さない |
| `service_runtime/src/error_mapping.rs` | service_runtime 内の typed error enum -> `HalError` 共通写像 | object table / registry / dispatch error を各use-caseで自由に `Internal` へ丸めない |
| `service_runtime/src/method_dispatch.rs` | object method transaction の dispatch planning 共通入口 | `plan_command_dispatch(...).map_err(command_dispatch_error_to_hal)` を各 domain ops にコピーしない。dispatch missing の status分類は共通 mapper に通す |
| `service_runtime/src/method_validation.rs` | `RuntimeExecutableRequest` の profile / supported-value validation 正本 | AIDL executor / service_runtime use-case ごとに `profile_support()` / `validate_supported_values()` を個別実装しない。直接呼び出しは `method_dispatch::plan_object_method_dispatch()` に集約する |
| `service_runtime/src/transaction_registry.rs` | runtime transaction -> dispatch target の正本表 | production dispatch で使う target mapping だけを持つ。第2の runtime handler / status 判定層を置かない |
| `service_runtime/src/open_rollback.rs` | open registration rollback、runtime cleanup、primary failure と cleanup failure の composed failure 合成規則 | root / child open transaction ごとに早期return処理を複製しない。object rollback failure 後も runtime cleanup を必ず試行し、primary と cleanup の両方を保持する |
| `service_runtime/src/object_close_txn.rs` | ObjectCloseTxn の close開始遮断 / cleanup failed 記録 / close commit / drop leak quarantine | close transaction ごとに begin_close / mark_cleanup_failed / commit_close / quarantine を手書きしない |
| `service_runtime/src/object_lifecycle.rs` | AIDL object table live確認 / public runtime binding lookup の service_runtime façade | AIDL helper から `RuntimeObjectTable` / `object_table()` / `object_table_mut()` を直接呼ばない |
| `service_runtime/src/packet_ops.rs` | packet ingress / demux binding | `PacketTxn<'_>` |
| `service_runtime/src/lnb_ops.rs` | LNB binding / apply / lifecycle / callback / drop leak | `LnbTxn<'_>` |

LNB public operation の状態遷移正本は `service_runtime/src/boot/lnb_txn.rs` の `LnbTxn<'_>`、`lnb/src/apply_txn.rs` の `LnbApplyTxn`、`lnb/src/lifecycle_txn.rs` の `LnbLifecycleTxn`、および `lnb/src/runtime.rs` の `LnbRuntimeState` とする。production 経路に接続されていない active operation ledger / guard token を LNB transaction invariant として置いてはならない。

#### 2.2.3 `boot/*_txn.rs` の責務

状態変更 transaction は `service_runtime/src/boot/*_txn.rs` へ置く。`boot/*_txn.rs` は domain transaction context または `transact_*` helper を定義し、registry / frontend worker / diagnostics / key table / stream boundary などの private field 操作を所有する。top-level `*_ops.rs` は、単純 operation では `transact_*` helper を直接呼び、複数stepの所有権移管・rollback use-case では domain transaction context の method を呼ぶ。

| ファイル | 所有する transaction context | 所有する状態変更 |
|---|---|---|
| `service_runtime/src/boot/frontend_txn.rs` | `FrontendTxn<'a>` | frontend runtime / scan / live reader / worker lifecycle の状態変更 |
| `service_runtime/src/boot/demux_filter_dvr_txn.rs` | `transact_*` helper + `DemuxFilterDvrTxn<'a>` | 単純 demux/filter/DVR 状態変更は `transact_*` helper が所有し、複数stepの child open / rollback だけ `DemuxFilterDvrTxn` が所有する |
| `service_runtime/src/boot/descrambler_txn.rs` | `DescramblerTxn<'a>` | descrambler allocation/demux/key/PID/unregister/owner-loss cleanup の状態変更 |
| `service_runtime/src/boot/packet_txn.rs` | `PacketTxn<'a>` | frontend TS packet ingress、demux source boundary、descrambler packet policy、packet diagnostics の状態変更 |
| `service_runtime/src/boot/lnb_txn.rs` | `LnbTxn<'a>` | LNB binding、runtime state apply、callback registration commit、lifecycle close、drop leak recording の状態変更 |

demux/filter/DVR の単純 operation は thin wrapper 増殖を避けるため、top-level `demux_filter_dvr_ops.rs` から `transact_*` helper を直接呼んでよい。複数stepの child open / rollback、frontend/packet/descrambler/LNB の context-owned transaction は domain transaction context を通す。`query_api.rs` から mutating transaction context または `transact_*` を呼んではならない。

#### 2.2.4 `query_api.rs` の責務

状態を変更しない参照系 API は `service_runtime/src/boot/query_api.rs` へ置く。`query_api.rs` は `RuntimeQuery<'a>` を定義し、read-only query は `RuntimeQuery<'a>` の method として実装する。`RuntimeQuery<'a>` は必要な read-only source だけを immutable reference として保持する。参照対象を追加する場合も immutable reference に限定し、状態変更、cleanup、rollback、quarantine、worker stop/start を行わない。AIDL object handle から public runtime id / owner relation を解決する read-only query はここへ置く。

`TunerServiceRuntime` に残る参照系 method は `self.query()` で `RuntimeQuery<'_>` を生成し、その method へ委譲する wrapper とする。`boot.rs` に `self.registry` を直接読む read-only wrapper を追加しない。複数の read-only 値を同一 AIDL query で使う場合は、AIDL 層で複数回 lock/query せず、`RuntimeQuery` に single-lock snapshot façade を追加する。

LNB profile/backend policy は `ServiceRuntimeLnbProfileAdapter` が `LnbBackendOps` へ適合させる。これは実 backend I/O ではなく、service_runtime の registry/profile 状態を domain transaction へ渡す adapter である。

#### 2.2.5 境界条件

- `service_runtime/src/boot.rs` は通常 operation を所有しない。
- `TunerServiceRuntime` field は domain transaction context が必要最小の範囲で扱う。
- top-level `service_runtime/src/*_ops.rs` は public use-case façade に留め、private field 操作や domain transaction 本体を所有しない。
- AIDL 層は service_runtime の private transaction 実体を所有しない。
- `query_api.rs` は read-only query 専用とし、cleanup、rollback、quarantine、worker stop/start を所有しない。
- `RuntimeQuery<'a>` は immutable reference のみを保持する。
- production code の file split / module visibility 規約は `CODE_CONVENTION.md` に置く。

## 3. 共通部品分類と責務境界

`tuner_hal2` で共通部品と呼ぶものは、既存 `tuner_hal/DESIGN_JA.md` の「共通部品の定義条件」を満たすものに限る。薄い helper、adapter、単純委譲 wrapper は共通部品ではない。

本書では所有関係の語を次に固定する。

| 用語 | 意味 |
|---|---|
| transaction 正本 | 状態遷移、commit、rollback、cleanup failed、quarantine、failure composition のいずれかを所有する実体 |
| use-case façade | AIDL/service_runtime 境界で public API の phase order を固定する入口。domain state は所有しない |
| executor façade | AIDL method identity、runtime lock、Binder status 変換、service_runtime use-case への橋渡しを所有する入口。transaction 本体は所有しない |
| implementation helper | transaction / use-case / executor の内部補助。単独では状態、寿命、failure precedence を所有しない |

| 分類 | tuner_hal2の実体 | 責務 | 所有関係 |
|---|---|---|---|
| 論理契約 | `ObjectCloseTxn` | close開始遮断、callback cleanup、domain cleanup、cleanup failed、final close | transaction 正本 |
| 論理契約 | `ObjectMethodTxn` / object method use-case | fallible request-builder、child open、source relation、callback registration、unavailable / unsupported / plan-only など status precedence を壊しやすい経路の phase order | transaction 正本。全 object method を機械的に包む層ではない |
| 論理契約 | `SourceBoundaryTxn` / `GenerationBoundaryTxn` / `PacketPipeline` | source切替、stream境界、packet assembler / continuity / partial payload の破棄境界 | transaction 正本。`SourceBoundaryTxn` は boundary cleanup だけでなく non-null source commit までを同一 snapshot rollback / quarantine 境界で所有する。`GenerationBoundaryTxn` が production stream boundary の正本であり、未接続 skeleton を別名 transaction として公開しない。実装対応は各 demux / packet pipeline 正本に閉じる |
| 実装正本 | `service_runtime::object_close_txn` | close preflight と close lifecycle の service_runtime 側正本 | transaction 正本 |
| 実装正本 | `service_runtime::object_method_txn` | fallible request-builder 系 method の object live / generation / kind 確認、request build、request validation、dispatch planning preflight | request-builder / request validation / dispatch planning 境界の transaction 正本。domain commit / rollback / quarantine は domain transaction 側が所有する |
| 実装正本 | `service_runtime::method_dispatch` | request validation と command dispatch planning | validation / dispatch planning 正本。状態commitは所有しない |
| executor façade | `aidl_service::object_runtime` の public façade `execute_*_use_case*` / `plan_unavailable_object_method_use_case` | AIDL method identity の静的 plan 化、Binder status 変換、runtime lock、DTO query request、service_runtime transaction への橋渡し | AIDL executor 境界だけを所有する。domain commit / rollback / quarantine / callback artifact registration state transition の transaction 正本ではない。query 用任意 closure executor は置かない |
| use-case façade | top-level `service_runtime/src/*_ops.rs` | AIDL/service_runtime から見える型境界、domain naming 隠蔽、public API phase order の入口 | domain state は所有しない。phase order を固定する use-case façade は許可し、同名単純委譲の thin façade は増殖させない |
| adapter | profile/backend adapter | backend trait へ product profile を適用する | 状態寿命正本ではない |

AIDL method body は、fallible な AIDL input -> domain request 変換、callback retain、source filter relation validation、unsupported / unavailable status mapping を個別に組み立ててはならない。これらは object lifetime / generation の phase 後に、上表の該当 use-case / transaction 境界で実行する。

共通部品の品質判定は、名前の有無ではなく、phase order、所有状態、失敗時遷移、呼び出し境界、最低テストが設計と実装で一致しているかで行う。

## 4. AIDL object lifecycle と method transaction

本節は既存 `tuner_hal/DESIGN_JA.md` の close / cleanup failed / Drop leak / quarantine 契約を `tuner_hal2` の実体名へ接続し、`tuner_hal2` で必要になる failure precedence / composed failure / callback cleanup / open rollback の補足を固定する。既存契約と差分がある場合は本節の記載を `tuner_hal2` の正本とする。

| 既存契約上の境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| public close lifecycle / ObjectCloseTxn | `aidl_service::object_runtime::{close_object_after_close_preflight_with_domain_cleanup, close_object_after_close_preflight}` と `service_runtime::object_close_txn` | public close の開始遮断、callback cleanup、domain cleanup hook、cleanup failed 記録、final close / runtime unregister は汎用 close transaction が所有する。close開始遮断は正本で1回だけ行う。final close では public runtime unregister 対象 entry の存在を全件 preflight し、preflight failure 時に一部 runtime unregister を開始してはならない。`RuntimeObjectTable` の finalization / cleanup-failed marking helper は root と descendant を区別する。root の unexpected `Closed` / `Quarantined` は内部不整合として表面化し、既に terminal な descendant は親 close retry / finalization 対象から除外する。cleanup-failed marking の `CleanupStep` は既存 ledger step に限定する。pre-finalization close cleanup failure は実際の失敗 phase へ写像し、callback artifact / RuntimeCallbackRegistry cleanup は `UnregisterRuntime`、domain cleanup hook failure は `ReleaseBackend`、descendant DVR status notifier stop failure は `StopWorker` として記録する。final close entry lookup / close commit failure は `ReleaseLedger`、public runtime unregister failure は `UnregisterRuntime` として記録する。`DomainCleanup` のような未定義 step を導入して close cleanup phase を増やしてはならない |
| Drop leak terminalization | `aidl_service::object_runtime::drop_leak_object()` と `RuntimeObjectTable::quarantine_cascade()` | Drop は通常 cleanup の代替にせず、live object と descendant を quarantine へ落とす入口 |
| 単一 object quarantine 遷移 | `RuntimeObjectTable` 内の private helper | 外部から直接呼ばせない。object lifecycle の公開入口は cascade 経路へ統一する |
| callback実体 cleanup | `aidl_service::callback_store` と `RuntimeCallbackRegistry` | callback object実体の保持・削除はAIDL層が正本。backend trait に callback cleanup を持たせない |
| callback registration transaction | `object_runtime` façade、callback artifact registration core、typed callback retain closure、`callback_store`、`RuntimeCallbackRegistry`、domain runtime callback commit | setCallback 主経路は `object_runtime` の AIDL façade 入口を通る。callback artifact registration core、callback store retain、`RuntimeCallbackRegistry` registration、domain runtime commit、失敗時 rollback は上記の正本分担に従う。child object open は child runtime/object registration 成功後、typed AIDL object 生成前に同じ callback artifact registration core を呼び、callback failure 時は child open rollback へ進む |
| child object open transaction | `aidl_service::child_object_open::{open_filter_child_for_owner_object_with_request_builder, open_dvr_child_for_owner_object_with_request_builder}` + `service_runtime::{open_filter_child_runtime_for_demux_object, open_dvr_child_runtime_for_demux_object, rollback_filter_child_open_after_aidl_failure, rollback_dvr_child_open_after_aidl_failure}` | demux配下 filter / DVR child object open の共通入口。AIDL側は owner `AidlObjectHandle` を受け取り、`AidlMethodCall` と request-builder result を service_runtime transaction helper へ渡す。service_runtime は同一 runtime critical section 内で owner live / generation / kind 確認、request-builder、AIDL method planning、`RuntimeExecutableRequest` 抽出・validation、dispatch planning、service_runtime child-open use-case 実行までを進める。runtime allocation、domain runtime registration、AIDL object table registration は service_runtime の owner object id / generation + dispatch proof based child-open use-case が所有する。service_runtime は typed child runtime id と `RuntimeObjectEntry` を同一 child-open result として返す。AIDL側は `RuntimeObjectEntry` から child handle を作り、typed callback retain / rollback と typed Binder object生成だけを扱う。AIDL側で `RuntimeObjectEntry.ledger_id` を filter / DVR id へ再変換しない。callback artifact registration は typed AIDL object 生成前に完了させ、callback 失敗時に明示 child rollback と Drop leak が同じ object へ連続しない順序にする。child-open request-builder / dispatch proof / runtime open を別 lock 区間に分離しない。`openFilter()` / `openDvr()` に同等手順をコピーしない。open request 構築が失敗し得る場合は request-builder 版を使い、owner demux の lifetime / generation 確認後に request を構築する |
| open rollback completion | `service_runtime::open_rollback` | root / child object open の post-registration failure は、object-table rollback が失敗しても runtime unregister / close を必ず試行し、primary failure と cleanup failure を composed failure 方針で扱う。各 open transaction はこの方針に接続し、独自に早期 return して後続 cleanup を飛ばしてはならない。 |
| primary / cleanup failure composition | `maleicacid_tuner_hal2_common` の failure composition helper 群 | primary failure が既に確定した後に cleanup / rollback / 必須診断 failure が発生した場合、primary と cleanup の両方を保持する composed failure を作る共通部品。open rollback、callback registration rollback、public close cleanup、Drop leak、owner-loss cleanup、frontend worker rollback / cleanup、descrambler key-token rollback release はこの方針へ接続する。各 transaction は status detail 文字列化や local-only 合成を正本にしない。 |
| multi-step cleanup first-error collector | `maleicacid_tuner_hal2_common::FirstErrorCollector` | close / owner-loss / worker stop / scan cancel / callback cleanup など、同一 cleanup phase 内の複数 cleanup step をすべて試行し、最初に発生した cleanup error を保持する共通部品。各 step はこの collector へ結果を投入し、途中 return で後段 cleanup を飛ばしてはならない。collector は状態遷移、診断記録、rollback 本体、primary failure と cleanup failure の合成を所有しない。primary failure が既に存在する経路では、failure composition helper 群で primary と cleanup を合成する。 |
| root object open transaction | `service_runtime::root_object_ops` と `aidl_service::tuner_service::{*_object_from_entry, rollback_root_object_open_after_aidl_failure}` | ITuner root open (`openFrontendById` / `openDemux` / `openDemuxById` / `openDescrambler` / `openLnbById` / `openLnbByName`) の runtime allocation、availability query、method planning、AIDL object table registration、runtime open、失敗時rollbackを service_runtime の root object open use-case 境界へ寄せる。AIDL側は returned entry から typed Binder object を生成し、Binder object 生成失敗時は service_runtime rollback helper を呼ぶだけにする。rollback は `finish_open_rollback()` を通し、object-table 側 rollback が失敗しても runtime unregister / close を必ず試行する。object table failure は共通 `object_table_error_to_hal()` で `RuntimeObjectTableError` の意味を保ち、duplicate / lifecycle / owner / kind mismatch を `HalError::Internal` に丸めて `UNKNOWN_ERROR` へ落とさない。generation overflow は内部カウンタ枯渇として `Internal` を維持する。runtime registry allocation / commit failure は共通 `registry_commit_error_to_hal()` を使い、duplicate は `INVALID_STATE`、missing/mismatch は対象APIの invalid input、id exhausted は `UNKNOWN_ERROR` へ分離する。`RuntimeObjectEntry` 取得後の public id 変換失敗も後段失敗として root object open rollback 対象にする。 |
| AIDL object method planning | `aidl_service::object_runtime::{plan_unavailable_object_method_use_case, execute_object_runtime_use_case, execute_object_runtime_use_case_with_request_builder, execute_shared_object_runtime_use_case, execute_shared_object_runtime_use_case_with_request_builder, execute_object_query_use_case}` + `service_runtime::{object_method_txn, method_dispatch}` | AIDL method plan、status precedence、runtime dispatch planning の共通入口。状態変更commit本体は `execute_object_runtime_use_case()` または frontend worker など共有runtime所有の use-case 用 `execute_shared_object_runtime_use_case()` から、`ObjectMethodExecutionToken` を消費する service_runtime 統一 `*_for_object` use-case / domain transaction へ委譲する。object pure query は `ObjectQueryRequest` / `ObjectQueryResponse` の DTO 境界で扱い、query closure や `&mut TunerServiceRuntime` を AIDL 側へ渡さない。AIDL executor は `aidl_service::object_runtime` が所有し、個別 AIDL method body は先行 validation / runtime lock を所有しない。AIDL入力からdomain requestを作る処理が失敗し得る場合は request-builder 版 helper を使い、`service_runtime::object_method_txn` が object live / generation / kind 確認後、かつ同じ runtime critical section 内で builder を実行し、builder 成功後に `RuntimeExecutableRequest` validation と dispatch planning preflight を一度だけ行う。builder failure では dispatch planning / domain operation を実行せず、dispatch planning failure では domain operation を実行しない。dispatch planning 成功後は `ObjectMethodDispatchProof` を `object_method_txn` 内部で即時消費し、service_runtime 統一 `*_for_object` use-case へは `ObjectMethodExecutionToken` だけを渡す。受け側は token を消費してから domain operation に進む。同じ `plan_object_method_dispatch()` を再実行してはならない。unavailable / unsupported / plan-only 経路は `plan_unavailable_object_method_use_case()` へ寄せ、AIDL method body で `ensure_open()` と plan-only status 写像を手書きしない。旧 service_runtime 側低レベル executor module は廃止し、低レベル runtime lock executor は `aidl_service::object_runtime` の private helper に閉じる。これにより status precedence の object lifetime / profile unsupported / input validation / dispatch 順を崩さない |
| public close helper | `aidl_service::object_runtime::{close_object_after_close_preflight_with_domain_cleanup, close_object_after_close_preflight}` + `service_runtime::object_close_txn` | close系 public method の入口。public `close()` は `Closed` を成功 no-op にせず、close 済み object への再アクセスとして AOSP-facing failure にする。begin close が必要な場合だけ、service_runtime 側で object kind / closeable lifecycle（`Live | CleanupFailed`）/ generation / dispatch planning を行い、同じ runtime critical section で close対象 cascade を `Closing` へ遷移させる。通常 method 用の `Live` 限定判定とは分離する。domain runtime cleanup を伴う close は `ObjectCloseTxn` 方針に従い、`Closing` へ遷移した後に runtime lock 外で domain cleanup hook を実行し、cleanup/finalize境界をこの入口に接続する。 |
| runtime unregister | `TunerServiceRuntime::unregister_public_runtime_for_closed_aidl_entry()` | close finalization では closing entries の runtime unregister を object table `Closed` commit 前に行い、runtime unregister が成功した後にだけ object table close commit を行う。Drop leak の quarantine terminalization は別経路で扱う。runtime unregister failure 時に object table を `Closed` に進めて retry 不能にしてはならない |

### 4.1 AIDL method category と責務正本
| AIDL method category | Required 正本 | 備考 |
|---|---|---|
| request-builder を伴う mutating method | `ObjectMethodTxn` + request-builder use-case | AIDL入力を domain request に変換する前に object live / generation / kind を確認する |
| source relation method | `ObjectMethodTxn` + source relation use-case | `IFilter.setDataSource()` 等。sink/source/owner demux 関係を service_runtime 側で確認する |
| callback registration | callback registration use-case | `ObjectMethodTxn` preflight + callback artifact retain + runtime registry record + domain commit |
| child open | `child_object_open` use-case | owner live / dispatch preflight / runtime child open / callback artifact retain / rollback |
| close | `ObjectCloseTxn` | close preflight / closing cascade / domain cleanup / commit or cleanup failed |
| root open | `root_object_ops` + `open_rollback` | root object registration / runtime allocation / rollback |
| object pure query | `ObjectQueryRequest` / `ObjectQueryResponse` + `ObjectMethodTxn` query path | object live / generation / kind、method planning、runtime validate、dispatch preflight を通し、proof は発行せず、`RuntimeQuery<'_>` だけで read-only snapshot を作る |
| root read-only query | `RootQueryRequest` / `RootQueryResponse` + `root_method_txn` + `RuntimeQuery<'_>` | root method planning と dispatch preflight 後に single-lock read-only snapshot を返す。query_api.rs は planning を所有しない |
| unavailable / unsupported | `plan_unavailable_object_method_use_case` | 実行しないが public API method として lifecycle / dispatch を記録する |

使用境界は次で固定する。direct import、public thin wrapper、helper 呼び出しに関する具体的な実装規約は `CODE_CONVENTION.md` に置く。

| 層 | 責務境界 |
|---|---|
| AIDL method implementation files | AIDL executor façade、callback registration use-case、service_runtime use-case、profile validation 境界へ接続する。低レベル executor helper や dispatch planning 本体は所有しない |
| `aidl_service/src/object_runtime/` | AIDL object runtime façade。`mod.rs` は実行・close façade を保持し、drop leak 隔離処理は `drop_leak.rs` に分割する。transaction 本体は所有しない |
| service_runtime domain use-case | public runtime id 解決、dispatch planning、domain transaction 接続を use-case 境界で扱う |
| new mutating method | 本表の required 正本へ照合してから実装する |


Lifecycle helper の用途は次に固定する。

| helper | 許可 lifecycle | 用途 |
|---|---|---|
| `aidl_object_live()` | `Live` | 通常 public method / query / callback registration |
| `aidl_object_closeable()` | `Live | CleanupFailed` | close preflight。retry は許可するが二重 begin は許可しない |
| `aidl_object_entry_for_close_cleanup()` / `aidl_public_runtime_id_for_close_cleanup()` / `aidl_object_for_close_cleanup_runtime()` | `Live | Closing | CleanupFailed` | close begin 後の domain cleanup / owner-loss cleanup / runtime cleanup |
| quarantine cascade | `Live | Closing | CleanupFailed` 相当 | Drop leak / forced quarantine |

`aidl_object_entry_for_close_cleanup*` 系は `Closing` を許すため、close preflight へ使ってはならない。close preflight は必ず `aidl_object_closeable()` を使う。通常 method 用の `aidl_object_live()` へ `CleanupFailed` 例外を追加してはならない。


設計上許容しない構造差分:

- `IFilter.setDataSource(source)` の non-null source 経路では、sink/source filter の lifetime / generation / kind を確認した後、両者の owner demux が同一 object / generation であることを service_runtime が検証する。cross-demux source、自己参照、閉鎖済み source は AIDL method body ではなく service_runtime の demux/filter use-case で拒否する。
source 切替の demux runtime 内処理は `SourceBoundaryTxn` を通す。`SourceBoundaryTxn` は sink endpoint、queue 存在、generation 増分可能性を検証した後に sink queue clear、demux generation boundary、packet pipeline reset を行い、その後で既存 upstream を disconnect する。既存 upstream disconnect、sink filter queue clear、demux generation boundary、packet pipeline reset を `set_filter_source_non_null()` に手書きしない。source boundary 失敗時は sink source を新 source へ commit せず、precondition / generation validation failure では既存 source も disconnect しない。mutation 開始後の generation boundary / downstream disconnect failure では boundary 開始前 snapshot へ rollback し、rollback failure 時は demux を quarantine する。`setDataSource(null)` も source boundary を通し、queue clear / generation boundary / packet pipeline reset と source disconnect を同じ source boundary として扱う。
TS main type の `linkCaps` を広告するため、AIDL/VTS が TS subtype `UNDEFINED` または `TS` で開いた filter は `TsRaw` として扱う。`setDataSource(source)` の non-null source 経路では、source が `TsRaw` であり、sink が `TsRaw` または `TsRecord` の場合、同一 demux / lifecycle / PID 条件を満たす限り sink subtype として拒否してはならない。`TsRecord` sink は record DVR へ attach される終端 sink として扱い、source filter origin からの TS packet も record index / record DVR mirror の対象にする。その他未分類 sink は `setDataSource` の sink として unsupported / unavailable 系へ落とす。

- AIDL object 種別ごとに Drop cleanup 処理をコピー実装しない。
- 単一 object quarantine を外部公開入口として残さない。
- Drop 経路で public close と同じ通常 cleanup を実行しない。
- callback store cleanup を LNB backend / profile backend / device backend の責務へ戻さない。
- callback registration の retain / runtime registry record / domain commit / rollback を AIDL method body や object wrapper へ個別コピーしない。typed callback Strong の保持は object wrapper / child open helper が closure として渡してよいが、frontend / LNB setCallback 主経路の ObjectMethodTxn dispatch preflight、rollback、runtime registry record は `execute_callback_registration_runtime_use_case()` に閉じる。child object open は child runtime/object registration 後、typed AIDL object 生成前に `register_callback_artifact_after_owner_ready()` を呼び、callback artifact registration 失敗時に child open rollback へ進む。
- child object open の allocation / registration / callback rollback 手順を `openFilter()` / `openDvr()` にコピーしない。既存 `child_object_open.rs` の共通入口を使う。
- close / Drop leak の runtime unregister を object table 終端前に実行しない。

### 4.2 root / child open rollback の統一
Root object open rollback と child object open rollback は、同じ rollback / cleanup failure composition 方針に接続する。root object open rollback は frontend / demux / descrambler / LNB すべてに適用し、LNB root object open を例外にしない。

runtime open failure 後の object table rollback failure は、primary runtime open failure と composed failure にする。child object open の runtime allocation / object table registration / callback artifact registration / typed Binder object construction 後段失敗も同じ方針に従う。rollback に使う runtime unregister / close / callback cleanup は、失敗を表面化できる operation として扱い、結果を観測できない best-effort-only operation を rollback transaction の正本にしない。

### 4.3 close cleanup / cleanup-failed marking
Close begin 後の callback cleanup、domain cleanup、cleanup-failed marking、final close / runtime unregister は close transaction の方針に従う。cleanup-failed marking 自体が失敗した場合、その failure は必須診断 failure として扱う。cleanup-failed marking failure が primary cleanup failure を無診断で上書きしてはならない。object table error kind を generic internal error へ潰さない。

### 4.4 AIDL method category 別の完了条件
| AIDL method category | 完了条件 |
|---|---|
| request-builder mutating method | object live / generation / kind 確認前に domain request を確定しない。builder failure で dispatch planning / domain operation を実行しない |
| source relation method | sink/source object の lifetime / generation / kind 確認後に owner demux 同一性と自己参照を検証する |
| callback registration | retain / runtime record / domain commit / rollback の各 failure point が callback registration transaction 方針に接続されている |
| child open | runtime allocation、object table registration、typed child runtime id return、callback artifact registration、typed Binder object construction、rollback failure が child open rollback 方針に接続されている。AIDL側で `RuntimeObjectEntry.ledger_id` を filter / DVR id へ再変換しない |
| root open | allocation、availability query、method planning、object table registration、runtime open、typed Binder object construction、public id conversion、runtime cleanup failure が root open rollback 方針に接続されている |
| close | callback cleanup、domain cleanup、cleanup-failed marking、final close、runtime unregister、cleanupFailed retry が ObjectCloseTxn 方針に接続されている |
| unavailable / unsupported | lifecycle / generation / kind と dispatch planning を通し、domain operation は実行しない |
| pure query | read-only snapshot だけを使い、cleanup / rollback / quarantine を呼ばない |

## 5. failure / cleanup / diagnostic contract

### 5.1 primary failure / cleanup failure / composed failure
`primary failure` は、public API / worker / open / close / callback registration の本来処理で最初に確定した失敗である。`cleanup failure` は、その失敗後または close / owner-loss / Drop leak terminalization 中に、rollback、cleanup、quarantine、cleanup-failed marking、callback health marking、runtime unregister / close で発生した失敗である。

primary failure と cleanup failure が同時に存在する場合、戻り値と診断の正本は `composed failure` とする。composed failure は抽象的な失敗契約であり、Rust の具体型名や構文規則は `CODE_CONVENTION.md` と実装側で定める。composed failure は primary failure と cleanup failure の両方を保持し、cleanup failure が primary failure を無診断で上書きしてはならない。primary failure と cleanup failure を文字列 detail だけの generic internal error に潰してはならない。AIDL status mapping と diagnostic は composed failure から一貫して導出する。

composed failure 方針の対象は次である。

| 対象 | primary failure | cleanup failure |
|---|---|---|
| root object open rollback | runtime allocation / runtime open / object table registration / typed Binder object generation / public id conversion の失敗 | object table rollback、runtime unregister / close、cleanup diagnostic の失敗 |
| child object open rollback | runtime allocation / object table registration / typed child runtime id return / callback artifact registration / typed Binder object construction の失敗 | child runtime rollback、callback artifact rollback、runtime callback registry clear / unhealthy、cleanup diagnostic の失敗 |
| callback registration rollback | callback artifact retain / runtime callback registry record / domain runtime commit の失敗 | callback artifact rollback、runtime callback registry rollback / unhealthy の失敗 |
| public close cleanup | callback cleanup または domain cleanup の失敗 | cleanup-failed marking、final close、runtime unregister の失敗 |
| Drop leak terminalization | Drop leak 経路での quarantine / diagnostic / runtime unregister の失敗 | Drop leak 中の追加 callback cleanup / registry clear / unhealthy / diagnostic failure |
| owner-loss cleanup | owner relation 喪失後の cleanup 本体失敗 | cleanup failed state / callback health / diagnostic 記録の失敗 |
| frontend worker start / body / rollback / stop cleanup | backend tune / scan session open / worker body / commit の失敗 | worker stop/join、snapshot restore、bound demux snapshot restore、failure marking、live-pump cleanup の失敗 |
| cleanup-failed marking | cleanup-failed marking 対象の primary cleanup failure | cleanup-failed marking 自体の失敗 |

同一 cleanup phase 内の複数 cleanup step は all-attempt とする。first-error collector は cleanup step 間の first cleanup error を集める部品であり、primary + cleanup failure composition の正本ではない。primary failure が既に存在する経路では、failure composition helper 群を composition 正本として使い、各 transaction が local-only の precedence / 文字列 detail 合成を正本化しない。

戻り値としてどの status を選ぶかは AIDL status bridge の責務だが、選ばれなかった failure を消してはならない。cleanup failure を優先して返す場合でも primary failure は composed failure の primary 要素または必須診断に残す。primary failure を優先して返す場合でも cleanup failure は composed failure の cleanup 要素または必須診断に残す。

### 5.2 diagnostic failure の二分類
Diagnostic failure は、必須診断と best-effort telemetry に分ける。

| 分類 | 意味 | precedence |
|---|---|---|
| 必須診断 | lifecycle、retry、cleanup accounting、callback health、quarantine、scan terminal state に影響する記録 | failure composition の対象にする |
| best-effort telemetry | 統計、packet count、補助ログ、観測用 counter など、状態正本に影響しない記録 | primary failure を上書きしない。失敗しても telemetry diagnostic に留める |

必須診断には、close cleanup failed marking、callback registry unhealthy marking / owner unhealthy marking、Drop leak quarantine / Drop leak record、owner-loss cleanup failed state、scan session terminal failure record、callback delivery failure accounting を含める。best-effort telemetry には、packet count、malformed count、throughput / diagnostic counter、retry / state transition に使わない補助ログを含める。

### 5.3 missing target failure の適用範囲
missing target を全域で機械的に failure 化しない。rollback、public close、owner-loss cleanup では missing target を failure として扱う。これらの範囲では、対象 missing が state corruption、二重 cleanup、owner relation 破損、cleanup 漏れを意味し得るためである。

read-only query、idempotent stop、best-effort telemetry、unsupported / unavailable defensive path、API契約上 already-gone を許容する処理では、missing target を機械的に failure 化しない。

## 6. callback ownership

### 6.1 callback registration 正本分担
callback registration は複数の正本に分かれる。`object_runtime` は AIDL façade 入口であり、callback artifact / lifecycle / domain commit の状態正本ではない。

| 正本 | 所有するもの | 所有しないもの |
|---|---|---|
| `aidl_service::object_runtime` | AIDL façade 入口、runtime lock 境界、Binder status bridge、typed retain glue 呼び出し | Binder callback artifact、registration health、domain operation 可否、rollback 本体 |
| `AidlServiceContext` owned `callback_store` | Binder callback artifact の正本。`Strong<dyn IFrontendCallback>` / `Strong<dyn ILnbCallback>` / child callback 実体を保持する唯一の場所。`TunerServiceRuntime` と同じ service instance lifetime で `AidlServiceContext` が所有する | registration health、domain operation 可否 |
| `RuntimeCallbackRegistry` | callback registration lifecycle / health / cleanup accounting の正本。`registered` / `unhealthy` を状態として管理し、clear は registry entry removal として扱う。close / rollback / Drop leak / delivery failure の診断に使う | Binder callback `Strong` 実体 |
| domain runtime state | callback registration commit 後の domain operation 可否の正本 | callback 実体、registration accounting |

`callback_store`、DVR status notifier store、filter event dispatcher bridge、drop-leak diagnostic store を process-global `OnceLock` / `static Mutex` に置いてはならない。callback artifact、DVR notifier cancel / join handle、filter event delivery bridge、drop-leak 診断記録、`TunerServiceRuntime` は同じ service instance lifetime に閉じる。Binder callback artifact / notifier thread / filter event dispatcher / drop-leak 診断記録が runtime object table と別寿命で残ってはならない。filter event dispatcher は `TunerServiceRuntime` instance の field として所有し、AIDL service 起動時に `AidlServiceContext` へ弱参照する dispatcher をその runtime instance へ登録する。`service_runtime` の process-global dispatcher slot を使ってはならない。

runtime 再初期化は `TunerServiceRuntime::boot_from_probe_results()` を AIDL service entry から直接呼んではならない。AIDL service 側で runtime を再構成する場合は `AidlServiceContext::reset_runtime_from_probe_results()` を唯一の入口とし、DVR notifier 全停止、callback artifact 全 clear、drop-leak 診断記録 clear、runtime boot の順で同一 owner に閉じて実行する。drop-leak 診断記録 clear の lock poison は boot 前 cleanup failure として返し、`poisoned.into_inner()` で吸収してはならない。test も production と同じ context-owned callback store / drop-leak diagnostic store を使い、test-only global callback store や process-global drop-leak diagnostic store を置いてはならない。

AIDL service crate の外部 API に `SharedTunerRuntime`、raw runtime lock accessor、callback artifact retain / lookup / clear helper を公開してはならない。AIDL object / tuner service wrapper が内部処理用に runtime handle や callback artifact store を取得する helper は crate 内部に閉じ、crate 外へ公開する owner は `AidlServiceContext` / `SharedAidlServiceContext` と Binder object wrapper に限定する。これにより、runtime reset と callback artifact / notifier cleanup の所有者を public API 上も `AidlServiceContext` に固定する。
frontend close の LNB owner-loss callback cleanup を含む callback artifact cleanup は、`SharedTunerRuntime` ではなく `SharedAidlServiceContext` を受け取る context-owned callback store helper へ接続する。runtime lock / domain cleanup 用に取得した raw runtime handle を callback artifact cleanup API へ渡してはならない。

`service_runtime` は Binder `Strong<dyn ...Callback>` を直接保持しない。callback lifecycle / health accounting は `RuntimeCallbackRegistry` が所有し、Binder artifact 実体と notifier thread は `AidlServiceContext` が所有する。filter event dispatcher は `service_runtime` から見える trait object だが、process-global ではなく `TunerServiceRuntime` instance field として所有し、実体は `AidlServiceContext` への `Weak` 参照だけを保持する。

callback registration order は次で固定する。

```text
object live / generation / kind check
  -> ObjectMethodTxn dispatch preflight
  -> callback artifact retain
  -> RuntimeCallbackRegistry record
  -> domain commit
  -> domain失敗時 callback rollback
```

失敗時の扱いは次で固定する。

| 失敗点 | 必須処理 |
|---|---|
| callback retain 失敗 | runtime record / domain commit へ進まない |
| retain 成功 / runtime record 失敗 | callback artifact rollback を実施する |
| runtime record 成功 / domain commit 失敗 | callback artifact rollback を実施し、runtime registry を unhealthy または entry removal に揃える |
| callback delivery 失敗 | 登録済み callback への Binder delivery failure のみ runtime registry を unhealthy にする。unhealthy marking は必須診断として扱い、失敗を黙殺しない |
| callback 未登録 / callback store failure | delivery failure ではなく callback artifact absence / store failure として返す。scan END などの domain session 側 failure は記録してよいが、runtime registry missing を primary failure に置き換えない |
| close / Drop leak | callback artifact clear と runtime registry entry removal / unhealthy を同期する。片方失敗時は必ず unhealthy / quarantine / diagnostic / returned error のいずれかへ落とす。`CallbackRegistryUpdate::Missing` を空分岐で吸収してはならない。Drop leak では object quarantine を必ず試行した後、registry missing を戻り値または診断で表面化する |

scan END delivery の失敗分類は次に固定する。callback store が `None` を返す未登録状態は「registered callback delivery failure」ではないため、scan session の callback failure は記録してよいが、`RuntimeCallbackRegistry::mark_unhealthy()` を呼んではならない。callback store lock poison も callback artifact store failure として扱い、registry missing へ置き換えてはならない。実際に callback `Strong` を取得した後の Binder delivery failure だけを runtime registry unhealthy marking 対象とする。

## 7. Drop leak / callback cleanup の共通部品境界
Drop 経路は public close の代替ではない。Drop leak は通常 close cleanup を実行する経路ではなく、quarantine / diagnostic / runtime unregister によって leak を終端させる経路である。全 AIDL object の `Drop` は `drop_leak_object_from_drop()` だけを呼び、戻り値を返せない `Drop` 上の error は `AidlServiceContext` owned drop-leak 診断記録へ保存する。drop-leak 診断記録を process-global `OnceLock` / `static Mutex` へ置いてはならない。drop-leak 診断記録は service lifetime 中に bounded とし、上限超過で古い record を捨てた件数と、診断 store lock poison により記録できなかった件数を context-owned counter として観測可能にする。object 種別固有の追加記録が必要な場合も、`Drop` 実装へ個別手順を書かず、`DropLeakDomainAction` と service_runtime 側 domain hook に閉じ込める。Drop leak 経路で行う quarantine / diagnostic / runtime unregister の failure は Drop leak terminalization failure として composed failure 方針に従う。Drop leak で callback store cleanup を行う場合、callback store lock を `TunerServiceRuntime` lock の内側で取得してはならない。

callback cleanup は次の規則に従う。

- public close / rollback では `clear_owner_callback_registration()` を使い、callback store cleanup 失敗を `RuntimeCallbackRegistry` の unhealthy と Binder error へ接続する。
- Drop leak では `drop_leak_object()` が callback store clear、runtime callback registry clear/unhealthy、object table quarantine、runtime unregister をまとめて扱う。runtime lock poison / quarantine 失敗 / registry missing は `drop_leak_object()` の戻り値として返す。実際の `Drop` 実装は `drop_leak_object_from_drop()` を呼び、返された error を stderr ではなく `AidlServiceContext` owned drop-leak 診断記録へ保存する。Drop 実装では通常 cleanup の代替を行わず、明示 close / owner-loss 経路で検出できる状態遷移へ寄せる。無言 return で成功扱いにしない。drop-leak 診断 store lock poison 時は `poisoned.into_inner()` で継続せず、記録失敗 counter を増やす。
- AIDL object wrapper には、状態・寿命・phase order・rollback・error precedence を所有しない public thin wrapper を置かない。許容するのは constructor、object identity / runtime accessor、typed callback retain を含む非薄い use-case glue に限る。具体的な実装規約は `CODE_CONVENTION.md` に置く。
- callback cleanup failure は必須診断または returned error へ接続し、成功扱いにしない。
- LNB だけを例外にして Drop cleanup 手順を持たせない。LNB固有の drop leak 記録は domain hook として表現する。

## 8. worker / scan / live path

### 8.1 worker構造差分
`tuner_hal2` では、frontend単位のworker slotを `FrontendWorkerRegistry` が所有する。これは既存契約名である `WorkerExit` を置き換える正本ではなく、tuner_hal2内部でfrontend workerを探すためのslot所有構造である。

| 境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| worker種別 | `FrontendWorkerKind::{Tune, Scan}` | `WorkerOwnerKind` のfrontend系所有者をtuner_hal2内で分けるための構造差分 |
| 停止要求 | `FrontendWorkerCancelReason` | `WorkerStopReason` へ写像するtuner_hal2内部入力 |
| 停止操作結果 | `FrontendWorkerStopOutcome` | stop要求APIの戻り値。終了分類の正本ではない |
| 終了分類 | `WorkerExit` | 既存契約名をそのまま使う |
| 失敗分類 | `WorkerFailureClassifier` | 既存契約名をそのまま使う |

### 8.2 ScanSession構造差分
`tuner_hal2` では、既存 `ScanSessionTxn` 論理契約に対応する内部状態正本として `FrontendScanSession` を置く。

| 境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| session owner | `FrontendScanSession` | active session、generation、fingerprintをfrontend runtime内で所有する構造差分 |
| 候補進行 | `current_candidate()` / `advance_after_candidate()` | scan candidate進行をsession状態へ閉じる |
| 置換 | `SupersededByNewRequest` terminal化 | 旧scan停止後に新generationを開始する契約をtuner_hal2のworker slotへ接続する |
| 停止 | `StopRequested` terminal化 | `stopScan()` 由来の停止理由をsessionへ残す |
| 終端理由 | `FrontendScanTerminalReason` | `END` / cancel / backend失敗 / callback失敗 / panicをScanSession内で区別する構造差分 |

### 8.3 live path構造差分
`tuner_hal2` では、device側descriptor、pump owner、packet sinkを分ける。

| 境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| descriptor | `FrontendLiveReaderDescriptor` | 読取元fd/pathの説明だけを持つ |
| pump owner | `FrontendLivePumpOwner` | thread handle、cancel、join、reportを所有する |
| packet sink | `FrontendLivePacketSink` | demux側配送先を抽象化する |
| stop結果 | `FrontendLivePumpReport` | stop/join後のpacket数、malformed byte、cancel/EOFを返す |

### 8.4 Frontend worker thread result ownership
`device::runtime::thread_result_owner::ThreadResultOwner` は frontend worker / live pump の thread result cell と `JoinHandle` の正本である。cancel signal と `WorkerExit` / `FrontendWorkerCancelReason` の domain 意味は caller 側が所有し、この部品へ入れない。worker closure は `catch_unwind(AssertUnwindSafe(...))` で囲み、panic を process abort や missing report ではなく `HalError::internal(InvariantViolation, ...)` として owner へ記録する。`AssertUnwindSafe` は worker closure が panic 後に再利用されない one-shot closure であり、panic 後の state 継続を保証するためではなく、panic を terminal failure として報告するためだけに使う。producer 側の result lock poison は `ThreadResultProducer::record_or_capture_failure()` が producer failure side channel へ記録し、caller 側の最終判定は `ThreadResultOwner::collect_if_finished()` / `join_after_stop()` が行い、poison / missing report / panic / join failure を success / finished に丸めてはならない。

### 8.5 Frontend worker poison / stop outcome 境界
`FrontendWorkerContext::cancel_reason()` は lock poison を `None` や正常終了へ丸めない。cancel reason lock poison は `HalError::Internal(InvariantViolation)` と `WorkerExit::RuntimeFailure(Signal)` に写像する。

`FrontendWorkerRegistry::request_stop()` / `request_stop_for_join()` は、cancel reason lock に書けなかった場合、cancel flag だけを立てて成功扱いしない。`FrontendWorkerStopOutcome::StopRequestFailed` を返し、所有側は対象 frontend / scan session / live data の状態を未停止または failed として扱う。

frontend worker の blocking join は `TunerServiceRuntime` lock を保持したまま実行してはならない。runtime lock 内では worker slot の取り外し、cancel reason 設定、cancel flag 設定、join ticket 作成までに限定する。`FrontendWorkerStopTicket::complete()` による `JoinHandle` 待ちは runtime lock 外で実行し、join outcome の scan cancel / live data idle / failed state 反映は必要に応じて runtime lock を再取得して行う。scan start で旧 scan worker を supersede する場合も同じ二段階停止を使い、旧 worker の join 中に runtime lock を保持しない。

frontend worker start 後に fallible commit を置く場合、commit failure は起動済み worker を二段階停止で stop/join し、frontend snapshot と bound demux snapshot の rollback を試行してから返す。worker start 成功後の commit failure を単に返して worker slot / thread を残してはならない。rollback 中の cleanup failure は primary commit failure を無音で上書きせず、composed failure 方針に従って primary/cleanup の関係が分かる形で表面化する。

frontend close では tune/scan worker stop、scan cancel record、live-data close/unbind をすべて試行する。ただし live-data close/unbind の error は、先に検出済みの worker stop / scan cancel record error を上書きしてはならない。複数 cleanup step 間では first cleanup error を保持し、primary failure がある場合は composed failure 方針に従って primary と cleanup の両方を表面化する。

## 9. demux / source boundary / product profile

### 9.1 demux依存境界
`tuner_hal2` のfrontend runtimeは、demux runtimeを所有しない。bound demux quarantine、demux unbind、attached demux stop notification、demux sinkの実体はdemux側runtimeの責務であり、frontend側では構造境界だけを持つ。

### 9.2 TS-only profile / monitor event policy
`tuner_hal2` の product profile は TS-only とする。monitor event feature は宣言しない。

AOSP Tuner HAL API として `IFilter.configureMonitorEvent()` は存在し、framework から HAL へ転送され得る。ただし本 product profile では monitor event を要求しない。

- `configureMonitorEvent(0)` は監視解除または未設定 no-op として成功させる。これは非対応機能を有効化しないため、TS-only profile と矛盾しない。
- `configureMonitorEvent(nonzero)` は monitor event feature を要求するため、profile 非宣言 feature として `UNAVAILABLE` を返す。
- VTS / product config では nonzero monitor event を必須要求しない。nonzero monitor event を要求する product へ切り替える場合は、別 WP で monitor event 実装と profile 宣言を追加する。

この方針の実体は `service_runtime::capability_profile::configure_monitor_event_result()` と `IFilter.configureMonitorEvent()` の supported no-op / unsupported profile 分岐である。

## 10. AIDL status bridge の所有者
AIDL status 変換は次に固定する。

| 層 | 責務 |
|---|---|
| `binder_adapter::status` / `AidlStatusMapper` | `HalError` / domain failure から `TunerStatusCode` への純粋写像 |
| `aidl_service::error_bridge` | `TunerStatusCode` / `HalError` から `binder::Status` への唯一の変換点 |

所有規則:

- Binder status 生成境界は `aidl_service::error_bridge` に集約する。
- `HalError` / domain failure から `TunerStatusCode` への写像は `binder_adapter::status` / `AidlStatusMapper` が所有する。
- `TunerStatusCode` / `HalError` から `binder::Status` への変換は `aidl_service::error_bridge` が所有する。
- production 未接続の bridge / slot / mapper 型は、設計正本に明記して用途を固定しない限り公開境界に置かない。
- `control_core` は production 接続済みの typed worker result / FMQ delivery transaction に限定する。production 未接続の lifecycle / stream boundary / worker signal skeleton を public 共通部品として置かない。

## 11. AOSP-facing runtime responsibility anchors

本節は AOSP-facing 契約補正を実装境界へ接続するため、現行ランタイム部品の責務を固定する。failure composition 共通 helper 規律は引き続き維持し、primary failure と cleanup / rollback failure の合成責務を局所実装へ戻さない。

### 11.1 FMQ queue runtime

`fmq` / `fmq_shim` / `demux::runtime::queue_runtime` は filter / DVR queue のランタイム境界である。Rust AIDL backend だけで official FMQ 本体を再実装するのではなく、target では `system/libfmq` と接続する薄い native shim を正方向とする。

現行 Rust 側の queue runtime は次を所有する。

- queue descriptor snapshot の保持
- queued bytes / readiness / clear / drain の runtime state
- filter / DVR の queue descriptor export 境界
- EventFlag wake / wait へ接続するための shim 境界

queue runtime は AIDL method body に局所実装しない。AIDL 層は descriptor query / queue handle export の façade に留め、実 queue state と drain readiness は demux runtime / service_runtime 側が所有する。

### 11.2 filter delay / callback delivery

filter delay hint は media / AV filter には適用しない。`setDelayHint()` は non-media filter の queue readiness だけに接続する。

`aidl_service/src/filter_callback_delivery.rs` は filter callback delivery の façade であり、pipeline generated event から AIDL event へ変換する境界を所有する。callback failure は filter runtime の unhealthy / diagnostic state へ記録し、domain commit 後の callback failure を public method の post-commit failure として扱わない。callback binder failure、callback registry missing、filter runtime unhealthy marking failure は `TunerServiceRuntime` の bounded filter callback delivery diagnostic store に記録する。

### 11.3 DVR queue / status notifier / callback delivery

`aidl_service/src/dvr_callback_delivery.rs` と DVR queue runtime は、DVR status event と queue descriptor export のランタイム境界である。

- record DVR `start()` は filter 未 attach だけでは失敗しない。
- playback DVR `attachFilter()` / `detachFilter()` は unsupported operation として扱い、state invalid と混同しない。
- DVR `start()` 後の status callback delivery / notifier 起動 failure は best-effort notification failure とし、started commit 済みの public `start()` を後段 failure へ反転させない。DVR `stop()` 後の notifier 停止 failure も、stopped commit 済みの public `stop()` を後段 failure へ反転させない。ただし、これは failure を捨ててよいという意味ではない。callback delivery failure / notifier 起動 failure / notifier 停止 failure は DVR post-commit notification diagnostic と callback unhealthy state へ記録する。DVR status notifier は `AidlServiceContext` owned notifier store に cancel handle / join handle を登録する。spawn 済み thread を notifier store 未登録のまま残してはならない。notifier store の fallible lock は thread spawn 前に取得し、spawn 成功後は同じ critical section 内で cancel handle / join handle を登録する。DVR post-commit failure の accounting 自体が runtime lock poison / callback registry missing / unhealthy marking failure で成立しない場合は service invariant failure として呼び出し元へ返し、silent return してはならない。
- pipeline generated event から filter queue payload へ enqueue する境界では、filter queue missing / filter missing などの enqueue failure を捨ててはならない。public packet push 成否と分離する場合でも `PipelineReport` diagnostic へ接続する。

AOSP IDvr は `read()` / `write()` method を公開しない。AOSP/VTS 上の DVR データ方向は、record DVR が demux output buffer であり HAL が record FMQ へ生成データを書き、client / VTS callback が読む方向、playback DVR が demux input buffer であり client / VTS callback が playback FMQ へ書き、HAL が読む方向である。したがって HAL 内部の queue operation 名で表す場合は record DVR = HAL write、playback DVR = HAL read とし、client 側 helper 名で表す場合は record DVR = client read、playback DVR = client write と明記する。視点を明記しない `record = write` / `playback = read` のような短縮表現を公開契約として使ってはならない。

### 11.4 AV shared backing

AV shared backing は media / AV filter event の shared memory backing を表すランタイム部品である。shared handle 未 export 中に AV payload を通常 MediaEvent として配送してはならない。未 export 中の drop / overflow は診断 counter として保持する。

AV shared backing の slot allocation / release / release_all は、active set と free slot set の片側だけを更新して成功扱いにしてはならない。release 時に backing marker と runtime backing 実体が乖離している場合、transient backing を生成して release outcome を作ってはならず、backing failure として扱う。部分 failure を検出した場合は diagnostic に残し、次回 release / close cleanup で再試行できる状態を保つ。

### 11.5 closed object public access after close

closed object に対する public method 呼び出しは、再 `close()` を含めて AOSP-facing API 境界では failure として扱う。特に `IDvr.close()` 後は AOSP `IDvr` 契約に従い、当該 DVR instance の全 method が failure を返す前提で設計する。ただし、`CleanupFailed` と `Closed` を混同してはならない。cleanup が未完了の object は再 close で cleanup を再試行可能でなければならず、Closed failure によって cleanup retry を隠してはならない。

close cascade finalization / cleanup-failed marking では、root object が unexpected terminal lifecycle である場合と、descendant が既に terminal lifecycle である場合を分ける。root が `Closed` / `Quarantined` の状態で finalization helper へ渡された場合は close preflight 境界の不整合として error にする。一方、descendant が既に `Closed` / `Quarantined` の場合は、親 close の再試行や部分 cleanup 後の cascade で起こり得るため、runtime unregister / close commit / cleanup-failed marking 対象から除外する。terminal descendant を理由に root close retry を失敗させてはならない。public runtime unregister preflight は destructive unregister の前に registry entry と runtime state の両方を確認し、descrambler でも registry entry と runtime の片側欠落を cleanup failure として表面化する。

### 11.6 demux-input descrambler PID claim

`IDescrambler.addPid(pid, NULL)` / `removePid(pid, NULL)` は、source filter を持たない demux input PID 操作として成功対象である。descrambler PID claim は `SourceFilter` と `DemuxInput` を区別する。

- `SourceFilter` claim は source filter id / generation と PID を保持する。
- `DemuxInput` claim は owner demux id / demux generation と PID を保持する。
- packet path は同一 demux generation の `DemuxInput` claim を active descrambler PID として拾う。
- demux-input claim に source filter accessor を強制してはならない。source filter を必要とする処理は `source_filter_ref()` の `Some` のみを対象にする。
- packet path で `SourceFilter` claim を active descrambler PID として拾う場合、source filter の owner demux、demux generation、filter lifecycle、PID、subtype、claim に保存した source filter generation を検証する。検証失敗または generation mismatch は `packet_policy` に丸めず、`PacketPipeline` phase の descrambler diagnostic として `filter_id` と `HalError` を保持する。packet delivery 全体は継続してよいが、該当 claim を active snapshot へ含めてはならない。`.ok()` で source-filter validation failure を無診断破棄してはならない。

### 11.7 A/V sync 最小契約と精度改善境界

`DemuxTsFilterType::PCR` は `FilterOpenType::TsPcr` として受理する。`getAvSyncHwId(media filter)` は、media filter と同じ demux に属する live PCR filter id を返す。渡された media filter の id を sync id として返してはならない。

`getAvSyncTime(pcrFilterId)` は、指定 id が同じ demux に属する live PCR filter であることを確認したうえで API として成功可能にする。PCR 未観測時は時刻未確定値として `0` を返してよいが、これは高精度 clock を表すものではない。

以下は後続の精度改善対象であり、現行のAOSP-facing最小契約とは分離する。

- PCR PID 明示管理
- PCR timestamp 抽出と monotonic clock 補間
- jitter smoothing
- PLL / clock discipline
- 複数 clock source の品質評価

PCR filter id association と pre-PCR API success の AOSP-facing 最小契約は現行責務であり、精度改善対象へ先送りしない。

## 12. 型付き境界の保守契約

本節は追記メモではなく、2章から11章の責務境界を型付き DTO、capability token、diagnostic 型へ落とすための設計正本である。旧リリース名は履歴識別子として扱い、現行仕様の正本は本節の本文とする。

### 12.1 Root / object query DTO boundary

Root query / command の DTO 境界は次で固定する。`RootQueryResponse::FrontendInfo` は `FrontendRegistryEntry` を返してはならず、`RootFrontendInfoSnapshot` のような専用 snapshot DTO だけを返す。`RootQueryRequest::MaxNumberOfFrontends` と `RootCommandRequest::SetMaxNumberOfFrontends` は AIDL 入力の `frontend_type` を捨てず、service_runtime 側 DTO に保持する。`RootDemuxCapabilitiesSnapshot` と `RootDemuxInfoSnapshot` は `TsOnly` marker だけに縮退せず、AIDL 変換に必要な field を service_runtime 側 snapshot として保持する。AIDL 層は snapshot DTO から AIDL 型へ変換するだけで、registry entry、capability policy、existence policy の正本を所有しない。

Object query の DTO 境界は次で固定する。`ObjectQueryResponse` は `FrontendRegistryEntry` を返してはならず、frontend status / readiness は `ObjectFrontendStatusValue` / `ObjectFrontendStatusReadinessValue` のような専用 DTO として service_runtime 側で policy を確定する。AIDL 層は requested status type を `ObjectFrontendStatusType` に変換し、返却 DTO を AIDL 型へ変換するだけに留める。`IDemux.getAvSyncHwId(filter)` の local Binder downcast のような fallible AIDL object conversion は、demux object live / generation / kind 確認と dispatch preflight の後に実行する専用 AIDL input conversion 境界へ置く。これは任意 query closure または `&mut TunerServiceRuntime` を query façade へ渡すことを許すものではない。



`ObjectMethodTxnTarget` は service_runtime の private target とし、AIDL façade が自由生成できる public constructor を置かない。AIDL 層は object id / generation / kind を DTO 入力として渡すだけにし、target construction、live/generation/kind 確認、method planning、dispatch validation は `ObjectMethodTxn` 内で行う。

Descrambler clear-key / replace-key は plan / validate / prepared token / commit を個別 public API として露出しない。外部 caller が独自 phase order を組めないよう、key table 操作まで含む full transaction façade だけを公開し、transaction 内で snapshot 再検証、session commit、token release / rollback release を固定する。

`SourceBoundaryTxn`、`DescramblerSessionTxn`、`LnbLifecycleTxn` は constructor / plan / prepared token / commit / reason を外部 caller が任意に組み立てられる surface にしない。`DescramblerReplaceKeyPlan` / clear-key prepared token は private に閉じ、外部 caller が stale plan を偽造したり old token / old key slot を観測したりできない形にする。`LnbLifecycleReason` は public close / owner-loss だけを選べる入力 enum とし、Drop leak は `record_lnb_drop_leak_lifecycle()` のような専用入口だけから記録する。Drop leak reason を通常 close façade へ渡せる形にしてはならない。

`PipelineDiagnostic` は typed enum とし、failure 種別ごとの必須 context を variant field で固定する。SourceFilter validation / descramble policy / record DVR mirror / filter queue delivery / AV backing failure / AV delivery non-delivered outcome は、文字列 detail ではなく typed error、typed outcome、PID、filter id、DVR id などの必須 context を保持する。`AvDeliveryState { detail: String }` のような fallback variant を置いてはならない。`PipelineDiagnosticKind` のような別 enum を production diagnostic 生成入力として残さない。集計・表示が必要な場合も `PipelineDiagnostic` typed enum の pattern match から派生させる。

`service_runtime/src/transaction_registry.rs` は runtime transaction -> dispatch target mapping だけを持つ。coverage、接続済み表示、stale 未接続表示、status precedence はこの表の責務ではない。`RuntimeCommandDispatcher` は dispatch target だけを消費する。

### 12.2 Capability token / diagnostic hardening

`ObjectMethodDispatchProof` は `object_method_txn` 内部の proof であり、AIDL closure、top-level `*_ops.rs` façade、domain transaction へ渡してはならない。`execute_object_method_call_after_live()` / `execute_shared_object_method_call_after_live()` は dispatch planning 成功直後に proof を内部消費し、後続には `ObjectMethodExecutionToken` だけを渡す。`*_for_object` use-case は `ObjectMethodExecutionToken` を最初の runtime-critical operation として consume してから public runtime id、owner relation、frontend/descrambler/LNB relation などを解決する。

Descrambler clear-key / replace-key の公開境界は key table 操作まで含む full transaction façade に限定する。`prepare_clear_key_with_session_txn()`、`commit_prepared_clear_key_with_session_txn()`、`plan_replace_key_with_session_txn()`、`commit_replace_key_with_session_txn()` は owning module 内 private helper であり、crate root / runtime module から re-export しない。clear-key は stale plan 検証後に session clear を commit し、その後に old token release を行う。外部 caller は old token / old key slot を観測できない。

LNB apply の公開境界は `apply_lnb_state_with_txn()` に限定し、caller-supplied generation を受け取る `LnbApplyTxn` constructor / `apply_with_generation()` を crate public surface に出さない。

Packet path diagnostic は validated TS packet から得た `PacketPid` を必須 context として保持する。validated packet path で PID を `Option` として扱う場合は、診断格納前に validation failure として扱い、record-DVR / filter-queue / AV delivery diagnostic の required PID を欠落させてはならない。
