# tuner_hal2 DESIGN_JA.md


## 1. 文書スコープと既存契約との関係

この文書は、tv直下 `開発規則.md` が許可した `tuner_hal2` 固有の構造差分、および既存 `tuner_hal/DESIGN_JA.md` の公開契約を `tuner_hal2` の実体名へ接続するために必要な補足だけを記載する。既存契約を再定義する場合は、既存契約との差分理由と、どちらを正とするかを同じ節で明記する。

DESIGN_JA.md は責務境界、状態遷移、phase order、failure precedence、resource lifetime を扱う。`let _`、`?`、`Option::None`、`format!(...)` のような具体的な実装規約は `CODE_CONVENTION.md` に置く。

本書に出てくるファイル名、module名、型名は、LLM/作業者が責務正本を取り違えないための実装境界アンカーである。単なる例示ではない。rename / split / merge を行う場合は、同じ変更で本書のアンカーも同期し、状態遷移、戻り値、資源寿命、failure precedence の正本がどこへ移ったかを明記する。ただし、syntax、import、visibility、禁止APIなどの実装規約は `CODE_CONVENTION.md` へ置き、本書へ重複定義しない。

## 2. レイヤ構造とファイル分割境界

本節は巨大制御層を再発させないための tuner_hal2 固有の配置規則である。ファイル分割は Rust の通常 `mod` による module 分割を前提とする。

### 2.0 crate 間 domain API と外部公開境界の責務分担

本書でいう「公開境界」は、状態遷移、phase order、resource lifetime、failure precedence を外部 caller が組み替えられるかどうかを意味する。Rust crate 間の接続に必要な domain 型、DTO、typed request、token、one-shot handle、full transaction façade の具体的な公開範囲規約は `CODE_CONVENTION.md` に置く。本書では、それらの型がどの責務正本に属し、どの状態遷移または resource lifetime を固定するかだけを扱う。

公開面は次の三層に分ける。

| 層 | 例 | 許可条件 | 禁止事項 |
|---|---|---|---|
| AOSP / Binder facing public API | AIDL trait implementation、Binder status bridge | AOSP/AIDL 契約、lifecycle precedence、Binder status mapping を守る | runtime state mutation、rollback policy、domain cleanup policy を AIDL method body へ持つ |
| crate 間 domain API | `service_runtime` から `demux` / `device` を呼ぶ domain operation、typed request、snapshot DTO、one-shot descriptor export handle | 呼び出し元と責務を文書化し、typed request / token / capability / owner relation / one-shot handle のいずれかで phase order を固定する | service_runtime transaction を迂回して authority を得られる surface、prepared token の外部再利用、raw registry/session mutator、arbitrary closure executor、同名薄 wrapper による正本不明化 |
| crate 内 helper / test helper | parser primitive、failure injection helper、loom helper | crate 内または test cfg に閉じる。production 経路の正本にしない | production caller が validation / lifecycle / cleanup を迂回できる形で公開する |

`service_runtime` は AIDL object lifecycle、object table、registry、root/object method transaction、failure composition の正本である。`demux` は demux domain runtime、filter/DVR queue、TS packet validation、packet pipeline、record index、AV/shared memory domain の正本である。`service_runtime` が `demux` crate の domain operation を呼ぶために DemuxRuntime、domain request、snapshot、queue descriptor export 用の型を crate 間接続面へ出すことは許容する。ただし、公開された domain operation は次の責務境界を満たす。

- lifecycle / object generation / owner relation / capability を `service_runtime` 側で検証済みであること、または demux 側 operation が typed request / token によって検証済みであることを型または owning use-case 境界で表す。
- raw runtime id、registry entry、session map、queue mutable state を任意 caller が直接書き換えられる API にしない。
- crate 間 domain API の typed request は、capability token / transaction proof / rollback token と同義ではない。違反条件は request が薄いことではなく、AIDL / binder / domain_request などが `service_runtime` transaction を迂回して runtime mutation / rollback / export authority を得られることである。`&mut DemuxRuntime` 取得自体が `service_runtime` registry ownership に閉じている場合、method 名が操作類型を固定し、request が raw runtime id や設定値を運ぶだけでも許容する。
- query / descriptor export は DTO または one-shot handle を返す。demux crate 内の queue descriptor export plan は demux-local target と one-shot export lifetime を保持すればよい。AIDL object id / object generation / owner relation は `service_runtime::RuntimeQuery` が wrapper plan で保持し、AIDL DTO 変換直前までその wrapper 境界に閉じる。handle は同じ descriptor を任意回数 export できる汎用 mutable accessor にしない。
- low-level primitive を公開する場合は、その primitive が state mutation を行わない、または caller が typed request / token / capability を提示しない限り mutation できない形にする。
- wrapper を置く場合は、責務名、phase order、failure composition、status precedence のいずれかを固定する場合に限る。名前だけ違う単純委譲 wrapper は置かない。

したがって、`configure_filter_runtime()`、`open_filter_runtime()`、queue descriptor export、packet ingress のような関数は、公開範囲そのものではなく、上記の typed boundary を満たしているかで判定する。設計上の違反は「公開範囲が広いこと」ではなく、「phase-less direct mutation が可能であること」「正本 use-case を迂回できること」「query / export / mutation の責務が同じ surface に混在すること」である。


#### 2.0.1 DemuxRuntime mutation authority boundary

`DemuxRuntime` の crate 間 mutation 境界は、production caller が service_runtime transaction、typed request、capability token、transaction proof、transaction-owned rollback token のいずれかを経由せずに state mutation を実行できない形にする。arbitrary closure executor で `&mut DemuxRuntime` と mutation authority を同時に貸与してはならない。capability token は owning transaction が発行し、状態検証または予約済み resource lifetime を表す。token / handle の重複利用や再利用を可能にする具体的実装禁止事項は `CODE_CONVENTION.md` に置く。

typed request は次で判定する。

- request 自体が authority / proof / transaction plan / reusable handle として外部 caller に扱われる場合は、操作類型、owner relation、generation、rollback 範囲を request または token に保持する。
- `service_runtime` が object live / generation / owner relation / kind / dispatch を検証済みで、demux crate へ `&mut DemuxRuntime` を渡せる caller が `service_runtime` に閉じている場合、request は demux-local operation DTO であってよい。この場合の操作類型は method 名、owner/generation は service_runtime registry と `DemuxRuntime` / child runtime state、rollback 範囲は service_runtime use-case と demux runtime 内部 ledger が保持する。request が raw filter id / dvr id / config / reason だけを運ぶこと自体を違反にしない。

Snapshot は read-only query DTO として許容する。rollback token は snapshot 本体を保持しない opaque id とし、snapshot 本体は `DemuxRuntime` 内部 ledger に保持する。restore は token id を one-shot consume し、demux id 不一致、ledger missing、snapshot generation 不一致、消費済み token reuse を拒否する。rollback prepare request は rollback authority ではなく demux-local request DTO であり、authority は `service_runtime` が所有する `&mut DemuxRuntime` 到達経路と、prepare が発行する one-shot token で固定する。rollback token の read-only generation accessor は、service_runtime の worker / rollback consistency check 用に限り許容し、old-token cleanup order や restore 対象切替に使える mutable authority accessor とは扱わない。crate 間 domain API として rollback prepare / restore の typed façade が存在しても、それを product/public API や AIDL 入口として扱ってはならない。AIDL/外部境界からは service_runtime rollback use-case を通す。queue descriptor export は read-only query authority として扱い、export authority を持つ handle/plan は one-shot lifetime を持つ。`QueueDescriptorSnapshot::into_parts()` と grantor accessor は AIDL DTO 変換用の read-only DTO surface であり、state mutation / export authority / queue mutable accessor を与えない限り許容する。

### 2.1 AIDL 層


`aidl_service::object_runtime` は AIDL object method executor façade であり、transaction 正本を新規に所有しない。置いてよいものは、AIDL method plan 入口、runtime lock / shared runtime executor 境界、Binder status 変換境界、service_runtime use-case 呼び出し façade、close / unavailable / query / callback registration 入口の façade に限る。

`object_runtime` に root / child open rollback 本体、callback artifact registration の状態遷移本体、domain cleanup policy、object table lifecycle commit 本体、registry / callback / domain cleanup の個別 rollback 手順、AIDL method ごとの例外的 phase order を追加してはならない。`object_runtime` に残せる callback registration 関連処理は façade 入口、runtime lock 境界、status bridge、callback artifact bridge の実行結果を service_runtime outcome / finish use-case へ渡す処理に限る。callback retain 成否後の rollback command 生成、unhealthy marking、primary+cleanup failure composition は service_runtime 側へ置く。

`aidl_service/src/tuner_service.rs` は root `ITuner` service の AIDL DTO 変換、root object open/query/command DTO の service_runtime 呼び出し、Binder status 変換だけを所有する。root method planning、dispatch preflight、unsupported / unavailable status precedence、read-only snapshot 取得は `service_runtime::root_method_txn` と `RuntimeQuery` 側へ閉じる。AIDL object lookup、local binder downcast、source filter handle 検証などの service-level helper は `aidl_service/src/tuner_service/support.rs` へ分ける。child object の公開 AIDL trait 実装は `aidl_service/src/tuner_service/*_methods.rs` へ分ける。

| ファイル | 所有する実装 | 所有外の実装 |
|---|---|---|
| `aidl_service/src/tuner_service.rs` | `TunerAidlService`、`ITuner`、root open/query/command の DTO 変換、service_runtime root façade 呼び出し、AIDL 型変換 | root method planning、dispatch preflight、unsupported / unavailable status precedence、read-only snapshot 本体、child trait 実装、service-level helper を戻さない |
| `aidl_service/src/tuner_service/support.rs` | AIDL object lookup、local binder downcast、AIDL method call DTO 生成、source filter owner/public id helper | AIDL trait 実装、root method planning、unsupported / unavailable status precedence、runtime状態遷移、Binder status helper再定義を置かない |
| `aidl_service/src/child_object_open.rs` | デマルチプレクサ配下のフィルターまたはDVRを開く際のBinderオブジェクト生成、コールバック実体の保持、`service_runtime`への接続 | 子オブジェクトの割り当て、コールバックの後片付け、巻き戻し、状態確定は`service_runtime::object_method_txn`が一括して担当する。AIDL層は`ObjectMethodExecutionToken`を生成せず、Binderオブジェクトの生成結果とコールバック実体の操作結果だけを渡す |
| `aidl_service/src/tuner_service/frontend_methods.rs` | `impl IFrontend for FrontendAidlObject` | runtime registry の直接所有を増やさない |
| `aidl_service/src/tuner_service/demux_methods.rs` | `impl IDemux for DemuxAidlObject` | filter/DVR/descrambler 状態遷移を直接所有しない |
| `aidl_service/src/tuner_service/filter_methods.rs` | `impl IFilter for FilterAidlObject` | callback/FMQ/AV cleanup failure を空消費しない |
| `aidl_service/src/tuner_service/dvr_methods.rs` | `impl IDvr for DvrAidlObject` | FMQ/EventFlag commit 条件を局所実装しない |
| `aidl_service/src/tuner_service/descrambler_methods.rs` | `impl IDescrambler for DescramblerAidlObject` | token / PID lifetime を AIDL 層で所有しない |
| `aidl_service/src/tuner_service/lnb_methods.rs` | `impl ILnb for LnbAidlObject` | LNB backend safe-state apply を Drop 経路へ戻さない |

AIDLメソッドは、対象オブジェクトの識別情報とAIDL入力を`service_runtime`へ渡し、返却結果をBinder状態へ変換する。生存、世代、種類、入力変換、実行可否、状態変更、巻き戻しの順序は、`service_runtime::object_method_txn`が1個のトランザクションとして担当する。状態変更を許可する`ObjectMethodExecutionToken`も同トランザクションが発行し、対象の処理で一度だけ消費する。AIDL層は別の実行権限や状態遷移を設けない。

ルート問い合わせと命令は`RootQueryRequest`、`RootQueryResponse`、`RootCommandRequest`を介し、`service_runtime::root_method_txn`が実行可否と戻り値の優先順位を決める。オブジェクト問い合わせは`ObjectQueryRequest`と`ObjectQueryResponse`を介し、`RuntimeQuery<'_>`から読み取り専用のスナップショットを生成する。問い合わせ処理では`ObjectMethodExecutionToken`を発行しない。

`IFilter.setDataSource(source)`では、AIDL層がローカルBinderオブジェクトをドメイン要求へ変換する。入力元と出力先の資源寿命、世代、種類、所有デマルチプレクサ、自己参照、実行可否、確定、巻き戻しは、`service_runtime`のデマルチプレクサ、フィルター、DVR用トランザクションが担当する。所有デマルチプレクサの不一致または自己参照は、出力先オブジェクトの生存と世代を確認した後に判定する。

状態変更を伴うAIDLメソッドでは、生存確認、公開IDの解決、入力変換、実行可否の判定、状態確定をAIDL層で分割して組み立てない。`aidl_service::object_runtime`をAIDL側の入口とし、`service_runtime::object_method_txn`が同一の排他区間で検証、予約、確定、巻き戻し、隔離を担当する。現在の実行時状態が入力変換に必要な場合も、その取得と要求の確定を同じトランザクション内で行う。

`ObjectMethodTxn`は、入力変換が失敗し得る操作、子オブジェクトの生成、入力元との関連付け、コールバック登録など、戻り値の優先順位が競合し得る操作に使用する。単純な状態変更処理まで機械的に包まない。トランザクションは検証後に複製不能な`ObjectMethodExecutionToken`を1個だけ発行し、対象ID、世代、種類が一致する処理で一度だけ消費する。未対応または利用不能として拒否する経路と、読み取り専用の問い合わせ経路では同トークンを発行しない。


通常の supported public API planning は `AidlMethodCall::PublicApi` を使う。unsupported-by-design API の戻り値生成だけ `AidlMethodCall::UnsupportedPublicApi` を使う。query / open / 状態取得系の supported API を unsupported planning に流用しない。

### 2.2 service_runtime 層

#### 2.2.1 `boot.rs` の責務

`service_runtime/src/boot.rs` は `TunerServiceRuntime` 定義、boot/probe、object table / callback registry / diagnostic accessor、command dispatch を所有する。`service_runtime/src/boot.rs` に通常 operation を追加してはならない。`TunerServiceRuntime` の内部 field は boot/runtime 正本が所有し、operation module は owning use-case / transaction API を通じて接続する。field storage を operation module の状態遷移正本にしない。

#### 2.2.2 top-level `*_ops.rs` の責務

公開 operation wrapper は top-level `service_runtime/src/*_ops.rs` へ置く。top-level `*_ops.rs` は `TunerServiceRuntime` の private field に直接触れない。単純な単一 runtime transaction は `boot` child module の `transact_*` helper を直接呼び、複数stepの owner/object/open rollback を所有する use-case だけ domain transaction context を呼ぶ。read-only は query wrapper を呼ぶ。

| ファイル | 所有する公開 wrapper | 呼び出す context |
|---|---|---|
| `service_runtime/src/frontend_ops.rs` | AIDL境界からフロントエンドの選局、走査、停止、閉鎖、LNB接続、受信データ消去へ接続し、処理順序を固定する | フロントエンド内部の状態変更は`FrontendTxn<'_>`が担当する。引数を横流しするだけの別入口や、コールバックの巻き戻しだけを分離した公開入口は設けない |
| `service_runtime/src/demux_filter_dvr_ops.rs` | demux/filter/DVR allocation/register/configure/start/stop/flush/source/DVR。`IFilter.setDataSource(source)` では sink/source object の lifetime・generation・kind 確認後に owner demux 同一性と自己参照を検証し、同一demux内のfilter接続グラフだけをcommit対象にする | 単純 operation は `transact_*` helper。child open / rollback だけ `DemuxFilterDvrTxn<'_>` |
| `service_runtime/src/descrambler_ops.rs` | descrambler allocation/demux/key/PID/unregister/owner-loss cleanup。clear-key / replace-key は外部 caller が plan / prepared token / commit を組み立てられない full transaction façade だけを公開する | `DescramblerTxn<'_>` |
| `service_runtime/src/descrambler_key_table.rs` | descrambler key token から key slot id への登録・参照・参照数管理 | key table は service_runtime 所有とし、descrambler crate から public export しない。slot id allocation は key table 内で fail-closed とし、上限到達時に既存 slot id を再発行してはならない。token table / slot table / refcount を部分更新しない |
| `service_runtime/src/descrambler_session.rs` | descrambler runtime session state、demux binding、key replacement / clear、PID claim、cleanup | descrambler crate は domain value / DTO / validation の公開に限定し、runtime transaction state は service_runtime が所有する。`setKeyToken(non-VOID)` / `setKeyToken(VOID)` / PID add-remove / cleanup は service_runtime の registry-owned use-case 内で key table 操作と一体で実行する。old token / old key slot を caller が accessor で観測して独自 cleanup order を組める surface を置かず、token release failure と session commit failure は service_runtime structured result に合成する。demux binding の rebind は既存 PID claims と同時に整合し、demux id / generation が変わる場合は stale PID claims を clear する |
| `service_runtime/src/root_object_ops.rs` | ITuner root object open の public façade / transaction境界 | AIDL層に runtime allocation / AIDL object table registration / rollback をコピーしない |
| `service_runtime/src/root_method_txn.rs` | ITuner root query / root command の method planning、dispatch preflight、DTO request / response 境界 | `query_api.rs` に planning、unsupported / unavailable status helper、mutable precedence を置かない。AIDL method body へ `AidlMethodAdapter::plan()` / `RuntimeExecutableRequest` 抽出を戻さない |
| `service_runtime/src/error_mapping.rs` | service_runtime 内の typed error enum -> `HalError` 共通写像 | object table / registry / dispatch error を各use-caseで自由に `Internal` へ丸めない |
| `service_runtime/src/method_dispatch.rs` | object method transaction の dispatch planning 共通入口 | `plan_command_dispatch(...).map_err(command_dispatch_error_to_hal)` を各 domain ops にコピーしない。dispatch missing の status分類は共通 mapper に通す |
| `service_runtime/src/method_validation.rs` | `RuntimeExecutableRequest` の profile / supported-value validation 正本 | AIDL executor / service_runtime use-case ごとに `profile_support()` / `validate_supported_values()` を個別実装しない。直接呼び出しは `method_dispatch::plan_object_method_dispatch()` に集約する |
| `service_runtime/src/transaction_registry.rs` | runtime transaction -> dispatch target の正本表 | production dispatch で使う target mapping だけを持つ。第2の runtime handler / status 判定層を置かない |
| `service_runtime/src/open_rollback.rs` | open registration rollback、runtime cleanup、primary failure と cleanup failure の composed failure 合成規則 | root / child open transaction ごとに早期return処理を複製しない。object rollback failure 後も runtime cleanup を必ず試行し、primary と cleanup の両方を保持する |
| `service_runtime/src/object_close_txn.rs` | ObjectCloseTxn の close開始遮断 / cleanup failed 記録 / close commit / drop leak quarantine | close transaction ごとに begin_close / mark_cleanup_failed / commit_close / quarantine を手書きしない |
object close / Drop leak の public runtime unregister は `ObjectRuntimeCleanupCommand` で表現し、AIDL 側へ `FnOnce` closure として runtime mutation を注入しない。AIDL executor は command を受け取り runtime lock を取得して `service_runtime` command の `execute()` を呼ぶだけとし、unregister 対象選択、preflight、failure composition は `service_runtime/src/object_close_txn.rs` に閉じる。domain cleanup は `service_runtime` の typed command が executor trait method を選択し、AIDL 側 executor は command kind の policy match を持たず、個別 bridge method だけを実装する。

| ファイル | 所有する公開 wrapper | 呼び出す context |
|---|---|---|
| `service_runtime/src/object_lifecycle.rs` | AIDL object table live確認 / public runtime binding lookup の service_runtime façade | AIDL helper から `RuntimeObjectTable` / `object_table()` / `object_table_mut()` を直接呼ばない |
| `service_runtime/src/packet_ops.rs` | packetの入口とdemuxへの接続 | `PacketTxn<'_>` |
| `service_runtime/src/lnb_ops.rs` | LNBの接続、反映、存続期間、callback、解放漏れ | `LnbTxn<'_>` |

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
| callback registration transaction | `object_runtime` façade、callback artifact bridge、service_runtime callback registration / cleanup use-case、`callback_store`、`RuntimeCallbackRegistry`、domain runtime callback commit | setCallback 主経路は `object_runtime` の AIDL façade 入口を通る。ただし AIDL façade が行うのは typed callback Strong の artifact retain / clear bridge と Binder status 変換だけであり、artifact retain bridge は service_runtime の object-method live / dispatch preflight 後にだけ実行する。`RuntimeCallbackRegistry` registration、domain runtime commit、rollback command 生成、unhealthy marking、primary+cleanup failure composition は service_runtime の callback registration / cleanup use-case が所有する。child object open は child runtime/object registration 成功後、typed AIDL object 生成前に callback artifact bridge 結果を service_runtime finish use-case へ渡す。callback artifact retain failure 時または typed Binder object 生成 failure 時の child open rollback 要否と failure composition は service_runtime child-open finish use-case が決める |
予約・事前確認と外部準備を分離し、ロック保持中に行う処理を所有者の再検証と確定処理に限定する。重複する分岐表や例外規則は設けない。

| 既存契約上の境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| open rollback completion | `service_runtime::open_rollback` | root / child object open の post-registration failure は、object-table rollback が失敗しても runtime unregister / close を必ず試行し、primary failure と cleanup failure を composed failure 方針で扱う。各 open transaction はこの方針に接続し、独自に早期 return して後続 cleanup を飛ばしてはならない。 |
| cleanup execution report / shared cleanup diagnostics | `service_runtime::cleanup_execution::{CleanupExecutionReport<TStepOutcome, TFailure>, CleanupExecutionDiagnosticSnapshot<TRecord>, SharedCleanupDiagnostics<TRecord>}` | cleanup 系 top-level use-case が all-attempt step outcome を保持し、public failure 用 first-error projection、bounded diagnostic snapshot、dropped count、shared diagnostic sink を共通化する上位部品。object close / drop-leak terminalization と frontend worker cleanup はこの pattern に接続する。共通化するのは report/snapshot/shared-store の execution pattern であり、対象 id、generation、step kind、domain failure 型は domain-specific typed adapter に残す。object cleanup は `ObjectCleanupStepOutcome::{Artifact,Domain,Runtime}` と `ObjectCleanupObjectTarget`、frontend worker cleanup は `FrontendWorkerCleanupStepOutcome` の step-specific variants と `FrontendWorkerCleanupTarget` / `FrontendWorkerCleanupWorkerGeneration` で context を表現し、`Option` field bag や `String` detail へ丸めて object cleanup と frontend worker cleanup を無理に同一 record 化してはならない。 |
| multi-step cleanup first-error collector | `maleicacid_tuner_hal2_common::FirstErrorCollector` | owner-loss / callback cleanup など、cleanup execution report の leaf step 内で必要になる複数 cleanup stepをすべて試行し、最初に発生した cleanup error を保持する補助部品。object close / drop-leak terminalization および frontend worker cleanup の top-level step 集約は `CleanupExecutionReport<_, _>` が所有し、FirstErrorCollector はその top-level report の代替にならない。collector は状態遷移、診断記録、rollback 本体、primary failure と cleanup failure の合成を所有しない。primary failure が既に存在する経路では、failure composition helper 群で primary と cleanup を合成する。 |
| root object open transaction | `service_runtime::root_object_ops` と `aidl_service::tuner_service::{*_object_from_entry, rollback_root_object_open_after_aidl_failure}` | ITuner root open (`openFrontendById` / `openDemux` / `openDemuxById` / `openDescrambler` / `openLnbById` / `openLnbByName`) の runtime allocation、availability query、method planning、AIDL object table registration、runtime open、失敗時rollbackを service_runtime の root object open use-case 境界へ寄せる。AIDL側は returned entry から typed Binder object を生成し、Binder object 生成失敗時は service_runtime rollback helper を呼ぶだけにする。rollback は `finish_open_rollback()` を通し、object-table 側 rollback が失敗しても runtime unregister / close を必ず試行する。object table failure は共通 `object_table_error_to_hal()` で `RuntimeObjectTableError` の意味を保ち、duplicate / lifecycle / owner / kind mismatch を `HalError::Internal` に丸めて `UNKNOWN_ERROR` へ落とさない。generation overflow は内部カウンタ枯渇として `Internal` を維持する。runtime registry allocation / commit failure は共通 `registry_commit_error_to_hal()` を使い、duplicate は `INVALID_STATE`、missing/mismatch は対象APIの invalid input、id exhausted は `UNKNOWN_ERROR` へ分離する。`RuntimeObjectEntry` 取得後の public id 変換失敗も後段失敗として root object open rollback 対象にする。 |
振り分けから実行までの権限は、複製できない一回限りの `ExecutionToken` で表す。このトークンは `object_id`、`object_generation`、`owner_id`、`method_domain`、`nonce` を保持する。検証・予約用ロックの保持中に `Prepared` 状態で生成し、実行部へ一度だけ移し、外部から観測可能な最初の副作用の直前に一度だけ消費する。状態は `Prepared`、`Transferred`、`Consumed`、`Cancelled`、`Invalidated` とする。二重消費、古い世代、所有者またはメソッドの不一致は、副作用を生じさせず型付きの権限エラーとして返す。消費前の取り消し、所有者の消滅、オブジェクトの閉鎖では未消費トークンを無効化する。実行失敗後にトークンを戻してはならず、再試行時は検証からやり直して新しいトークンを発行する。この状態機械とは別の実行権限を設けない。

| 既存契約上の境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| 公開 `close()` の補助処理 | `aidl_service::object_runtime::{close_object_after_close_preflight}` + `service_runtime::object_close_txn` | `tuner_hal/DESIGN_JA.md` の公開 `close()` 契約を正とする。`tuner_hal2` はその契約を再定義せず、この実体へ接続する |


### 4.1 AIDL method category と責務正本
| AIDL method category | Required 正本 | 備考 |
|---|---|---|
| request-builder を伴う mutating method | `ObjectMethodTxn` + request-builder use-case | AIDL入力を domain request に変換する前に object live / generation / kind を確認する |
| source relation method | `ObjectMethodTxn` + source relation use-case | `IFilter.setDataSource()` 等。sink/source/owner demux 関係を service_runtime 側で確認する |
| callback登録 | callback登録処理 | `ObjectMethodTxn`による事前確認、callback実体の保持、実行時台帳への記録、domain状態の確定 |
| 子オブジェクトのopen | `child_object_open`とservice_runtimeの子open完了処理 | 所有者の生存確認、dispatchの事前確認、実行時の子open、callback実体の保持、service_runtimeが所有するrollback完了処理 |
| close | `ObjectCloseTxn` | close preflight / closing cascade / domain cleanup / commit or cleanup failed |
| rootオブジェクトのopen | `root_object_ops`と`open_rollback` | rootオブジェクトの登録、実行時資源の割り当て、rollback |
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

| 補助処理 | 許可するライフサイクル | 用途 |
|---|---|---|
| `aidl_object_live()` | `Live` | 通常の公開メソッド、問い合わせ、callback登録 |
| `aidl_object_closeable()` | `Live`または`CleanupFailed` | closeの事前確認。再試行は許可するが、開始処理を重複させない |
| `aidl_object_entry_for_close_cleanup()` / `aidl_public_runtime_id_for_close_cleanup()` / `aidl_object_for_close_cleanup_runtime()` | `Live`、`Closing`、`CleanupFailed` | close開始後のdomain、所有者消滅、実行時資源の後片付け |
| 隔離の連鎖処理 | `Live`、`Closing`、`CleanupFailed`相当 | 解放漏れまたは強制隔離 |

`aidl_object_entry_for_close_cleanup*` 系は `Closing` を許すため、close preflight へ使ってはならない。close preflight は必ず `aidl_object_closeable()` を使う。通常 method 用の `aidl_object_live()` へ `CleanupFailed` 例外を追加してはならない。


設計上許容しない構造差分:

- `IFilter.setDataSource(source)` の non-null source 経路では、sink/source filter の lifetime / generation / kind を確認した後、両者の owner demux が同一 object / generation であることを service_runtime が検証する。cross-demux source、自己参照、閉鎖済み source は AIDL method body ではなく service_runtime の demux/filter use-case で拒否する。
source 切替の demux runtime 内処理は `SourceBoundaryTxn` を通す。`SourceBoundaryTxn` は sink endpoint、queue 存在、generation 増分可能性を検証した後に sink queue clear、demux generation boundary、packet pipeline reset を行い、その後で既存 upstream を disconnect する。既存 upstream disconnect、sink filter queue clear、demux generation boundary、packet pipeline reset を `set_filter_source_non_null()` に手書きしない。source boundary 失敗時は sink source を新 source へ commit せず、precondition / generation validation failure では既存 source も disconnect しない。mutation 開始後の generation boundary / downstream disconnect failure では boundary 開始前 snapshot へ rollback し、rollback failure 時は demux を quarantine する。`setDataSource(null)` も source boundary を通し、queue clear / generation boundary / packet pipeline reset と source disconnect を同じ source boundary として扱う。
TS main type の `linkCaps` を広告するため、AIDL/VTS が TS subtype `UNDEFINED` または `TS` で開いた filter は `TsRaw` として扱う。`setDataSource(source)` の non-null source 経路では、source が `TsRaw` であり、sink が `TsRaw` または `TsRecord` の場合、同一 demux / lifecycle / PID 条件を満たす限り sink subtype として拒否してはならない。`TsRecord` sink は record DVR へ attach される終端 sink として扱い、source filter origin からの TS packet も record index / record DVR mirror の対象にする。その他未分類 sink は `setDataSource` の sink として unsupported / unavailable 系へ落とす。

安定版Tuner AIDLは変更しない。DVR再生とDVR記録では、FMQの開始予約から確定までが公開 `flush()` の境界をまたぎ得るため、プロセス内の `QueueEpochProtocol` を使用する。FilterとSharedFilterでは、HAL内部の `FilterProducerDrainGate` を使用する。両機構の共通契約は `tuner_hal/DESIGN_JA.md` を正とし、この文書では `tuner_hal2` の実体への接続だけを定義する。


- 単一 object quarantine を外部公開入口として残さない。
- Drop 経路で public close と同じ通常 cleanup を実行しない。
- callback store cleanup を LNB backend / profile backend / device backend の責務へ戻さない。
- callback registration の retain / runtime registry record / domain commit / rollback を AIDL method body や object wrapper へ個別コピーしない。typed callback Strong の保持または削除は AIDL service context の artifact bridge が実行してよいが、frontend / LNB setCallback 主経路の ObjectMethodTxn dispatch preflight、domain commit、runtime registry record、rollback command 生成、unhealthy marking、primary+cleanup failure composition は service_runtime の callback registration / cleanup use-case に閉じる。child object open は child runtime/object registration 後、typed AIDL object 生成前に callback artifact bridge を実行し、その bridge 結果を service_runtime finish use-case へ返す。callback artifact retain failure 時の child open rollback、および typed Binder object 生成 failure 後の callback cleanup / child open rollback の要否と failure composition は service_runtime child-open finish use-case が所有する。
- child object open の allocation / registration / callback cleanup policy / rollback failure composition を `openFilter()` / `openDvr()` にコピーしない。既存 `child_object_open.rs` の共通入口を使うが、この入口は Binder object construction と callback artifact bridge glue に限定し、callback cleanup command 生成と rollback failure composition は service_runtime use-case が所有する。
- close / Drop leak の runtime unregister を object table 終端前に実行しない。

### 4.2 root / child open rollback の統一
Root object open rollback と child object open rollback は、同じ rollback / cleanup failure composition 方針に接続する。root object open rollback は frontend / demux / descrambler / LNB すべてに適用し、LNB root object open を例外にしない。

runtime open failure 後の object table rollback failure は、primary runtime open failure と composed failure にする。child object open の runtime allocation / object table registration / callback artifact registration / typed Binder object construction 後段失敗も同じ方針に従う。child-open rollback の command 生成、runtime unregister / close、callback cleanup、primary+cleanup failure composition は service_runtime の child-open finish use-case が所有し、AIDL 側は callback artifact bridge と Binder object construction の結果だけを渡す。rollback に使う runtime unregister / close / callback cleanup は、失敗を表面化できる operation として扱い、結果を観測できない best-effort-only operation を rollback transaction の正本にしない。

### 4.3 close cleanup / cleanup-failed marking
Close begin 後の callback cleanup、domain cleanup、cleanup-failed marking、final close / runtime unregister は close transaction の方針に従う。cleanup-failed marking 自体が失敗した場合、その failure は必須診断 failure として扱う。cleanup-failed marking failure が primary cleanup failure を無診断で上書きしてはならない。object table error kind を generic internal error へ潰さない。

### 4.4 AIDL method category 別の完了条件
| AIDL method category | 完了条件 |
|---|---|
| request-builder mutating method | object live / generation / kind 確認前に domain request を確定しない。builder failure で dispatch planning / domain operation を実行しない |
| source relation method | sink/source object の lifetime / generation / kind 確認後に owner demux 同一性と自己参照を検証する |
| callback registration | retain / runtime record / domain commit / rollback の各 failure point が callback registration transaction 方針に接続されている |
| child open | runtime allocation、object table registration、typed child runtime id return、callback artifact registration、typed Binder object construction、rollback failure が service_runtime の child-open rollback 方針に接続されている。AIDL側で `RuntimeObjectEntry.ledger_id` を filter / DVR id へ再変換せず、child rollback command 生成や primary+cleanup failure composition も持たない |
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
| 確定に必須の状態 | 世代、資源寿命、入力元との接続、バックエンドの所有権、PID台帳、後片付け権限、queue位置の確定、走査の`terminal_reason` | トランザクションの失敗合成対象とする。保存できない場合は確定自体を成立させない |
| 確定後の配送と集計 | コールバック配送、`end_delivery_outcome`、診断文 | 確定済みの公開結果を変更しない。飽和カウンターと実装内部の型付き有界リングへ記録する |
| best-effort telemetry | 統計、packet count、補助ログ、観測用 counter など、状態正本に影響しない記録 | primary failure を上書きしない。失敗しても telemetry diagnostic に留める |

確定処理の成否を左右する状態は、世代、ライフサイクル、入力元の関連付け、バックエンドの所有権、PID台帳、後片付け権限、キューポインターの確定に限定する。コールバックの配送・集計と診断文は確定後の処理とし、保存失敗によって確定済みの公開結果を覆してはならない。診断には飽和カウンターと、型付きコールバック失敗を保持する実装内部の有界リングを1個使用する。容量は1件以上とし、診断用メモリー枠と想定する失敗集中量から実装時に決める。設計文書や `ProductProfile` で固定値を定めない。満杯時は最古の記録を上書きし、`overwritten_record_count` を飽和加算する。読み取りによって記録を削除してはならない。永続報告、操作ID、有効期限、確認・削除手順、公開HAL APIは追加しない。


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

実行時状態の再初期化は`AidlServiceContext::reset_runtime_from_probe_results()`を唯一の入口とする。再初期化前に、DVR通知処理とワーカーの停止、コールバック実体の消去、解放漏れ診断の消去をすべて試し、結果を`ServiceResetPreflightReport`へ保存する。残存処理がサービス全体の台帳またはバックエンドを変更できる場合、排他的な資源を保持する場合、世代で遮断できない場合、または新しい初期化と同じ資源を競合する場合は、サービス全体に影響する失敗として初期化を開始せず隔離する。それ以外の残存失敗は所有者、世代、依存資源の範囲だけを隔離し、無関係な所有者を維持したまま初期化を続行できる。

結果は`boot_not_started`または`boot_committed`と、後片付け失敗とを別々に保持する。ロック汚染を含む後片付け失敗を吸収せず、事前確認報告へ残す。



frontend close の LNB owner-loss callback cleanup を含む callback artifact cleanup は、service_runtime の close cascade plan が生成する typed artifact cleanup command に接続する。AIDL は `SharedAidlServiceContext` 上の callback artifact bridge を実行して結果を返すだけにし、runtime lock / domain cleanup 用に取得した raw runtime handle を callback artifact cleanup API へ渡してはならない。

`service_runtime` は Binder `Strong<dyn ...Callback>` を直接保持しない。callback lifecycle / health accounting は `RuntimeCallbackRegistry` が所有し、Binder artifact 実体と notifier thread は `AidlServiceContext` が所有する。filter event dispatcher は `service_runtime` から見える trait object だが、process-global ではなく `TunerServiceRuntime` instance field として所有し、実体は `AidlServiceContext` への `Weak` 参照だけを保持する。

callback registration order は service_runtime の callback registration use-case が次で固定する。AIDL は service_runtime object-method preflight / dispatch 成功後に callback artifact retain bridge を実行し、その結果を渡すだけで、この順序・rollback command 生成・failure composition を所有しない。

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
| callback artifact retain bridge 失敗 | service_runtime finish use-case は runtime record / domain commit へ進まない |
| artifact retain 成功 / runtime record 失敗 | service_runtime が callback artifact cleanup command を生成し、AIDL は bridge 結果だけを返す |
| runtime record 成功 / domain commit 失敗 | service_runtime が callback artifact cleanup command、runtime registry unhealthy / entry removal、primary+cleanup failure composition を所有する |
| callback delivery 失敗 | AIDL delivery façade は callback artifact lookup、AIDL event conversion、Binder callback execution、primary `HalError` 生成だけを行う。callback delivery failure diagnostic、scan-session callback failure marking、runtime callback registry unhealthy marking、filter / DVR runtime callback unhealthy marking、primary+cleanup failure composition は原則 `service_runtime::finish_callback_delivery_failure_use_case()` が所有する。ただし runtime lock poison で finish use-case に到達できない場合は、AIDL service context が runtime instance と同等の typed fallback diagnostic record を保存し、accounting failure を silent return しない。AIDL delivery façade は通常経路で `compose_primary_cleanup_failure()`、generic callback unhealthy marking、delivery diagnostic record を直接呼ばない |
| コールバック未登録またはコールバック台帳の失敗 | コールバック実体の不在または台帳失敗を`end_delivery_outcome`と診断集計へ記録し、走査の`terminal_reason`を上書きしない。`Strong`取得後のBinder呼び出しに失敗した場合だけ、登録済みコールバックを不健全状態へ移す。コールバックに依存しない操作、実行時台帳、公開結果には波及させない |
| close / Drop leak | callback artifact clear と runtime registry entry removal / unhealthy を同期する。片方失敗時は必ず unhealthy / quarantine / diagnostic / returned error のいずれかへ落とす。`CallbackRegistryUpdate::Missing` を空分岐で吸収してはならない。Drop leak では object quarantine を必ず試行した後、registry missing を戻り値または診断で表面化する |


## 7. Drop leak / callback cleanup の共通部品境界


- public close / rollback では service_runtime の typed callback artifact cleanup command / owner callback cleanup use-case を使う。AIDL は callback artifact bridge を実行して結果を返すだけにし、callback store cleanup 失敗、runtime registry clear、unhealthy marking、primary+cleanup failure composition の policy 分岐を持たない。
- AIDL object wrapper には、状態・寿命・phase order・rollback・error precedence を所有しない public thin wrapper を置かない。許容するのは constructor、object identity / runtime accessor、callback artifact bridge result を service_runtime use-case へ渡す非薄い façade に限る。具体的な実装規約は `CODE_CONVENTION.md` に置く。


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
| 終端理由 | `FrontendScanTerminalReason` | `terminal_reason`は`Completed` / `Cancelled` / `FailedBackend` / `FailedPanic`だけを所有する。`end_delivery_outcome`（`Delivered` / `CallbackMissing` / `StoreFailure` / `BinderFailure`）は別fieldとし、END delivery失敗でterminal reasonを上書きしない |

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

## 9. デマルチプレクサ・入力元・製品設定の境界

### 9.1 demux依存境界
`tuner_hal2` のfrontend runtimeは、demux runtimeを所有しない。bound demux quarantine、demux unbind、attached demux stop notification、demux sinkの実体はdemux側runtimeの責務であり、frontend側では構造境界だけを持つ。

### 9.2 TS限定製品設定と監視イベント
`tuner_hal2`の製品設定はTS限定とし、monitor eventの対応能力を宣言しない。公開APIの戻り値は`tuner_hal/DESIGN_JA.md`を正とする。`tuner_hal2`では、その判断を`service_runtime::capability_profile::configure_monitor_event_result()`へ接続する。VTS設定と製品設定では0以外のmonitor eventを必須にせず、必要な製品では対応する実装とprofileを定義してから有効化する。

## 10. AIDLステータス変換の責務
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

## 11. AOSP公開契約と実行時部品の接続

本節は AOSP-facing 契約補正を実装境界へ接続するため、現行ランタイム部品の責務を固定する。failure composition 共通 helper 規律は引き続き維持し、primary failure と cleanup / rollback failure の合成責務を局所実装へ戻さない。

### 11.1 FMQキュー

`fmq` / `fmq_shim` / `demux::runtime::queue_runtime` は filter / DVR queue のランタイム境界である。Rust AIDL backend だけで official FMQ 本体を再実装するのではなく、target では `system/libfmq` と接続する薄い native shim を正方向とする。

現行 Rust 側の queue runtime は次を所有する。

- queue descriptor snapshot の保持
- queued bytes / readiness / clear / drain の runtime state
- filter / DVR の queue descriptor export 境界
- EventFlag wake / wait へ接続するための shim 境界

queue runtime は AIDL method body に局所実装しない。AIDL 層は descriptor query / queue handle export の façade に留め、実 queue state と drain readiness は demux runtime / service_runtime 側が所有する。

### 11.2 フィルター遅延とコールバック配送


`aidl_service/src/filter_callback_delivery.rs` は filter callback delivery の façade であり、pipeline generated event から AIDL event へ変換する境界を所有する。callback failure は service_runtime の callback delivery failure use-case へ渡し、filter runtime の unhealthy / diagnostic state へ記録する。domain commit 後の callback failure を public method の post-commit failure として扱わない。callback binder failure、callback registry missing、filter runtime unhealthy marking failure は `TunerServiceRuntime` の bounded filter callback delivery diagnostic store に記録する。runtime lock poison により service_runtime finish use-case に到達できない場合も、AIDL service context 側の fallback bounded diagnostic store へ typed diagnostic record を残し、記録不能を silent return しない。

### 11.3 DVRキュー・状態通知・コールバック配送

`aidl_service/src/dvr_callback_delivery.rs` と DVR queue runtime は、DVR status event と queue descriptor export のランタイム境界である。

DVRの公開契約と戻り値は`tuner_hal/DESIGN_JA.md`を正とする。`tuner_hal2`では、状態通知をBinder配送、実行時方針による抑止、コールバック実体の検索、通知処理の事前確認・実行・後片付けに分けて診断する。コールバックの不健全状態は、`Strong`を取得した後のBinder配送失敗、または`service_runtime`がコールバック状態の破損を明示した場合だけに設定する。

DVR状態通知処理の停止・待機用handleは`AidlServiceContext`が所有する通知台帳へ登録し、起動済みthreadを未登録のまま残さない。起動失敗時は以前の通知処理を台帳へ戻せるようにする。待機済みまたはpanic終了済みのhandleを再試行用として台帳へ戻してはならない。service resetと解放漏れの後片付けでは、通知台帳の全項目に終端処理を試す。

確定後診断は、service_runtimeが発行した共有診断格納先へ記録する。通常の記録ができない場合は代替記録へ移し、それも失敗した場合は記録失敗数を増やす。実運用のsnapshotでは、診断記録、破棄件数、記録失敗数を取得できるようにする。
- pipeline generated event から filter queue payload へ enqueue する境界では、filter queue missing / filter missing などの enqueue failure を捨ててはならない。public packet push 成否と分離する場合でも `PipelineReport` diagnostic へ接続する。

DVRの公開データ方向は`tuner_hal/DESIGN_JA.md`を正とする。`tuner_hal2`の内部名では、record DVRをHAL書き込み、playback DVRをHAL読み取りとして明示し、呼び出し側から見た向きと混同しない。

### 11.4 AV共有領域

AV shared backing は media / AV filter event の shared memory backing を表すランタイム部品である。shared handle 未 export 中に AV payload を通常 MediaEvent として配送してはならない。未 export 中の drop / overflow は診断 counter として保持する。

AV shared backing の slot allocation / release / release_all は、active set と free slot set の片側だけを更新して成功扱いにしてはならない。release 時に backing marker と runtime backing 実体が乖離している場合、transient backing を生成して release outcome を作ってはならず、backing failure として扱う。部分 failure を検出した場合は diagnostic に残し、次回 release / close cleanup で再試行できる状態を保つ。

### 11.5 閉鎖後の公開操作


close cascade finalization / cleanup-failed marking では、root object が unexpected terminal lifecycle である場合と、descendant が既に terminal lifecycle である場合を分ける。root が `Closed` / `Quarantined` の状態で finalization helper へ渡された場合は close preflight 境界の不整合として error にする。一方、descendant が既に `Closed` / `Quarantined` の場合は、親 close の再試行や部分 cleanup 後の cascade で起こり得るため、runtime unregister / close commit / cleanup-failed marking 対象から除外する。terminal descendant を理由に root close retry を失敗させてはならない。public runtime unregister preflight は destructive unregister の前に registry entry と runtime state の両方を確認し、descrambler でも registry entry と runtime の片側欠落を cleanup failure として表面化する。

### 11.6 デスクランブラーのPID登録への接続

nullable引数と公開結果は`tuner_hal/DESIGN_JA.md`の契約を正とする。`tuner_hal2`では、NULLを`DescramblerPidClaimSource::DemuxInput`、filter指定を`DescramblerPidClaimSource::SourceFilter`へ変換し、いずれもservice_runtimeの同じPID登録処理へ接続する。


- `SourceFilter` claim は source filter id / generation と PID を保持する。
- packet path で `SourceFilter` claim を active descrambler PID として拾う場合、source filter の owner demux、demux generation、filter lifecycle、PID、subtype、claim に保存した source filter generation を検証する。検証失敗または generation mismatch は `packet_policy` に丸めず、`PacketPipeline` phase の descrambler diagnostic として `filter_id` と `HalError` を保持する。packet delivery 全体は継続してよいが、該当 claim を active snapshot へ含めてはならない。`.ok()` で source-filter validation failure を無診断破棄してはならない。

### 11.7 A/V同期契約への接続

A/V同期の公開契約は`tuner_hal/DESIGN_JA.md`を正とする。`tuner_hal2`では`DemuxTsFilterType::PCR`を`FilterOpenType::TsPcr`へ対応付け、同一デマルチプレクサの有効なPCRフィルターIDと最新PCR時刻を`service_runtime`の問い合わせ用DTOへ接続する。AIDL層では別の戻り値規則を設けない。


次の内部精度改善は、公開契約と分離する。

- PCR PID 明示管理
- PCR timestamp 抽出と monotonic clock 補間
- jitter smoothing


- 複数 clock source の品質評価


## 12. 型付き境界の保守契約

本節は追記メモではなく、2章から11章の責務境界を型付き DTO、capability token、diagnostic 型へ落とすための設計正本である。

### 12.1 ルート・オブジェクト問い合わせのDTO境界


ルート問い合わせと命令のDTO境界は次のとおりとする。`RootQueryResponse::FrontendInfo`は、台帳項目そのものではなく専用の`RootFrontendInfoSnapshot`を返す。`RootQueryRequest::MaxNumberOfFrontends`と`RootCommandRequest::SetMaxNumberOfFrontends`は、AIDL入力の`frontend_type`を`service_runtime`側のDTOへ保持する。`RootDemuxCapabilitiesSnapshot`と`RootDemuxInfoSnapshot`は、AIDL変換に必要な項目を保持する。AIDL層はスナップショットをAIDL型へ変換するだけとし、台帳、対応能力、存在判定の規則を所有しない。

### 公開契約とtuner_hal2内部型の接続

null許容引数、フロントエンド数、状態、準備状態、A/V同期の公開契約と戻り値は`tuner_hal/DESIGN_JA.md`を正とし、本書では再定義しない。`tuner_hal2`内の接続は次のとおりとする。

- `IFilter.setDataSource()`のnull許容引数は、`None`を`disconnect_filter_data_source_for_object()`へ渡す。
- フロントエンドとLNBのnull許容コールバックは、`None`を`service_runtime`の登録解除処理へ渡す。AIDL層はコールバック実体の操作結果を`service_runtime`へ返すだけとする。
- デスクランブラーのnull許容フィルターは、`DescramblerPidClaimSource`で入力元を区別する。
- フロントエンド数のAPIは、`frontend_type`を`service_runtime`のDTOへ保持したまま渡す。
- フロントエンドの状態と準備状態は、`service_runtime`がそれぞれ専用の`ObjectFrontendStatusSnapshot`、`ObjectFrontendStatusValue`、`ObjectFrontendStatusReadinessValue`を生成する。AIDL層は要求型と戻り値の変換だけを行う。
- `IDemux.getAvSyncHwId(filter)`のように失敗し得るAIDLオブジェクト変換は、対象オブジェクトの生存、世代、種類、および実行可否の事前確認後に行う。


`ObjectMethodTxnTarget` は service_runtime が生成・所有する transaction target とし、AIDL façade が target construction を所有しない。AIDL 層は object id / generation / kind を DTO 入力として渡すだけにし、target construction、live/generation/kind 確認、method planning、dispatch validation は `ObjectMethodTxn` 内で行う。

Descrambler clear-key / replace-key は plan / validate / prepared token / commit を外部 caller が個別に組み替えられる境界にしない。key table 操作まで含む full transaction façade だけを外部入口とし、transaction 内で snapshot 再検証、session commit、token release / rollback release を固定する。

`SourceBoundaryTxn`、service_runtime-owned descrambler session transaction、`LnbLifecycleTxn` は construction / plan / prepared token / commit / reason を外部 caller が任意に組み立てられる境界にしない。Descrambler replace / clear-key の prepared state は service_runtime が所有し、外部 caller が stale plan を偽造したり old token / old key slot を観測したりできない形にする。LNB lifecycle reason は public close / owner-loss と Drop leak 専用記録を分け、Drop leak reason を通常 close façade へ渡せる形にしてはならない。

`PipelineDiagnostic` は typed enum とし、failure 種別ごとの必須 context を variant field で固定する。SourceFilter validation / descramble policy / record DVR mirror / filter queue delivery / AV backing failure / AV delivery non-delivered outcome は、文字列 detail ではなく typed error、typed outcome、PID、filter id、DVR id などの必須 context を保持する。`AvDeliveryState { detail: String }` のような fallback variant を置いてはならない。`PipelineDiagnosticKind` のような別 enum を production diagnostic 生成入力として残さない。集計・表示が必要な場合も `PipelineDiagnostic` typed enum の pattern match から派生させる。

`service_runtime/src/transaction_registry.rs` は runtime transaction -> dispatch target mapping だけを持つ。coverage、接続済み表示、stale 未接続表示、status precedence はこの表の責務ではない。`RuntimeCommandDispatcher` は dispatch target だけを消費する。

### 12.2 対応能力トークンと診断の境界


Descrambler clear-key / replace-key の公開境界は service_runtime の key table 操作込み full transaction use-case に限定する。clear-key は stale plan 検証後に session clear を commit し、その後に old token release を行う。外部 caller は old token / old key slot を観測できない。具体的な helper 公開範囲と module 接続規約は `CODE_CONVENTION.md` に置く。

LNB apply の公開境界は service_runtime-owned transaction に限定し、caller-supplied generation で stale state を適用できる境界を作らない。

Packet path diagnostic は validated TS packet から得た `PacketPid` を必須 context として保持する。validated packet path で PID を `Option` として扱う場合は、診断格納前に validation failure として扱い、record-DVR / filter-queue / AV delivery diagnostic の required PID を欠落させてはならない。


### 12.3 共通部品境界閉鎖契約

この節の「閉鎖」は責務正本を閉じることを意味する。閉鎖対象は、外部 caller が transaction phase、runtime state mutation、callback cleanup / rollback、packet policy、descriptor export、key/session material resolution を任意順序で組み替えられる境界である。具体的な Rust 公開範囲規約は `CODE_CONVENTION.md` に置く。

- callback cleanup は service_runtime の typed artifact cleanup command / callback registry use-case を正本 entry とし、domain callback state clear、runtime callback registry clear、callback artifact store clear を all-attempt で処理する。
  AIDL object-runtime helper は callback artifact bridge を実行し、その結果を service_runtime の command finish use-case へ返すだけにする。artifact store clear / runtime registry clear / unhealthy marking / primary+cleanup failure composition を AIDL 側 policy として分岐しない。
- descrambler key clear / replace / cleanup は service_runtime の key table 操作込み full transaction use-case のみを entry とし、descrambler crate は raw runtime/session mutator、old token、old key slot、runtime transaction façade を crate 外へ公開しない。close / owner-loss cleanup では service_runtime が token release と session cleanup を all-attempt で所有する。
  descrambler crate は domain value / DTO / validation の公開に限定し、runtime/session state と key table transaction は service_runtime が所有する。
- packet path が descrambler claim / key-slot 状態を必要とする場合でも、packet transaction は key table accessor、key-slot-id lookup helper、raw `(claims, key_slot_id)` tuple を直接読まない。`RuntimeRegistry` が resolved claim snapshot と keyless-claim predicate を返し、key table resolution と stale source-filter generation 判定を registry-owned façade に閉じる。
- service_runtime の descrambler runtime state が packet path 向け claim set を返す場合、raw key-slot-id snapshot を返してはならない。runtime state は key table owner と同じ service_runtime 内で resolved claim set を生成し、packet consumer は resolved key material の有無だけを観測する。
- object close cascade の policy 判断は service_runtime の `close_object_use_case()` / typed command executor 正本へ寄せる。service_runtime は close preflight、begin close、cascade entries、Binder artifact cleanup command、domain cleanup command、public runtime unregister、commit close、cleanup-failed marking、failure composition を所有する。AIDL façade は executor adapter、Binder artifact bridge、error bridge に限定する。
  Binder artifact cleanup と domain cleanup の phase ordering は service_runtime plan executor が所有し、AIDL façade は domain cleanup closure を注入せず、計画内 phase を読み分岐してはならない。
- queue descriptor export は service_runtime query use-case から demux domain API へ依頼してよい。低レベル export handle を公開する場合は、対象 queue、owner object、generation、消費状態を保持する one-shot handle とし、queue lifecycle mutation、fd duplication、descriptor 再 export を任意 caller が直接実行できる汎用 accessor にしない。AIDL façade は DTO response への変換だけを行い、queue runtime state や export policy を所有しない。
- `SourceBoundaryTxn` の construction / step recording / outcome 操作は source boundary transaction 本体が所有し、外部観測は immutable `SourceBoundaryReport` のみにする。
  `SourceBoundaryReport` / `FilterConfigureReport` / `DvrConfigureReport` / `FilterRuntimeOperationReport` は service_runtime の bounded production diagnostic store に typed record として接続する。demux transaction diagnostic の production accessor は records と dropped count を返す snapshot とし、bounded store overflow を観測不能にしてはならない。Filter/DVR configure の validation-only failure は rollback を実行していないため `Failed` outcome とし、rollback 成功 path の `RolledBack` outcome は failed step と rollback step を保持する。 DVR configure commit 後の status reporting 設定 failure は、configure 済み状態をそのまま public failure として残さず、pre-configure DVR snapshot へ rollback を試行し、rollback failure は primary/cleanup composition と demux quarantine で表面化する。filter runtime stop / flush の pipeline step 後に queue clear が失敗する場合は、queue clear failure、pipeline rollback 成否、queued payload clear / AV backing flush / stopped marking の実行・skip decision を `FilterRuntimeOperationReport` に残す。AIDL object method 経由の `IFilter.stop()` / `IFilter.flush()` も service_runtime の transaction façade を通し、typed report を破棄する raw demux runtime helper を直接呼ばない。demux transaction typed diagnostic record は production で取得可能な単調増加 diagnostic id を持ち、public `HalError` detail が source-boundary / configure / filter runtime operation report を要約する場合は同じ diagnostic id を含める。public `HalError` detail は Binder/AIDL status bridge 用の要約であり、typed report の唯一の保持場所にしてはならないが、public failure と typed record の対応を追えない状態にしてはならない。`Unsupported` 相当の public status を維持する必要がある動的 source-boundary failure では、static-only `Unsupported(&'static str)` ではなく diagnostic id を持てる dynamic detail variant を使う。
- packet-bearing production path は、demux ingress boundary で raw TS bytes を受けてよい。ただし raw ingress は frontend / DVR / source-filter adapter から demux へ入る最初の境界、resync / malformed packet diagnostic、または preflight-only helper に限定する。ingress 直後に `ValidatedTsPacket` / `PacketPid` へ変換し、以後の delivery / section / PES / record index / AV / descrambler planning は `ValidatedTsPacket` と `PacketPid` を正本にする。validation failure は NULL PID へ丸めず、raw packet boundary が malformed / TEI / length / sync / PID validation diagnostic を所有する。
  `ValidatedTsPacket` が保持する元 TS packet bytes は、record/DVR mirror、AV shared memory / FMQ write、raw TS forwarding、bounded diagnostic prefix のような byte output 用に限り読み出してよい。この読み出しは validated packet の付随 payload 参照であり、別個の packet identity、validation source、PID source、descramble policy source、section/PES planning source として扱ってはならない。
  `ValidatedTsPacket::packet_bytes()` 相当の accessor を置く場合は read-only borrow に限定し、caller がその bytes から `TsPacketView` を作り直す、PID を再抽出する、scrambling / source-filter / generation / diagnostic policy を再判定する、または validation 済み packet と異なる raw packet を組み合わせる設計にしてはならない。
- record index event construction も packet-bearing production path として扱い、record event 用の TS parser は `ValidatedTsPacket` ingress を通る。`TsPacketView` の直接 validate / parse は `ValidatedTsPacket` 内部、同一 crate 内 parser primitive、または test に限定し、crate 外の production caller へ公開しない。
  record index 内部 event model は `PacketPid` を保持し、AIDL DTO 変換が必要な境界でだけ数値 PID へ射影する。
  record index parser は raw byte wrapper を持つ場合でも、共通部品間の正本接続では `push_validated_ts_packet()` / `build_validated_event()` のように `ValidatedTsPacket` を直接受け取る入口を使える形にし、record event 構築側で raw `TsPacketView` を再正本化しない。


- object close cascade は callback 登録を持ち得る object にだけ callback cleanup command を生成する。callback 未登録は close cleanup / unregister cleanup の成功扱いとし、callback store failure だけを cleanup failure として扱う。
- LNB owner-loss callback cleanup は service_runtime の close cascade plan が生成する callback artifact cleanup command に統合し、AIDL 側に個別の runtime registry clear / artifact store clear / unhealthy marking 手順を持たない。
- close cascade の低レベル begin / commit / mark / entries helper は service_runtime 内部 helper とし、public API 主経路は close_object_use_case / finish_object_close_use_case に限定する。
- `TsPacketView` は raw packet preflight view であり、production path の packet identity 正本にしない。raw byte 入口は demux ingress validation 境界、resync / malformed diagnostic 境界、または preflight-only helper に限定する。validation 成功後の production path は `ValidatedTsPacket` を正本とし、raw `TsPacketView` / raw PID を再正本化しない。
- query surface は DTO response を正本とし、frontend runtime/signal 中間 state helper を AIDL façade の policy source にしない。


- callback cleanup は service_runtime が artifact cleanup command を生成し、AIDL は callback artifact bridge の実行結果だけを返す。cleanup failure 時は runtime registry entry を先に clear せず、unhealthy marking が意味を持つ状態を保持する。
- LNB Drop leak の記録要否は AIDL object の `Drop` 実装や AIDL 側 action enum で選ばない。service_runtime の drop-leak quarantine plan が `LnbDropLeakRecord` typed domain cleanup command を生成した場合だけ、AIDL executor adapter がその command を実行する。
- `ValidatedTsPacket` は crate 外に `TsPacketView` を返さない。packet-bearing helper の production 境界は `PacketPid` を受け、raw `i32` / `u16` PID は ts_core など低レベル部品への最終変換点に限定する。元 TS packet bytes の accessor は output 用 payload 参照としてのみ許可し、PID / policy / validation の再正本化入口にしてはならない。
- AIDL input / filter config 由来 PID は packet-derived `PacketPid` と混同せず、`AidlInputPid` / `ConfigInputPid` の validation boundary を通す。


- RuntimeRegistry の descrambler key/session transaction façade は crate-local とし、外部 crate から registry transaction を分解して呼べる surface を広げない。

### callback artifact lookup と delivery failure の境界

callback artifact lookup failure は Binder delivery failure と同一に扱わない。artifact lookup failure は callback artifact が存在しない、または callback artifact store を読めない境界 failure であり、service_runtime は diagnostic を記録するが、Binder callback が失敗した場合と同じ unhealthy marking を実行しない。Binder delivery failure / event conversion failure / post-commit notification failure は、service_runtime の `finish_callback_delivery_failure_use_case()` が diagnostic、unhealthy marking、primary+cleanup failure composition を所有する。ただし DVR notifier cleanup / runtime policy skip / artifact lookup は phase として記録しても、Binder delivery failure と同じ unhealthy marking 対象にしない。AIDL 側 delivery module は Binder artifact lookup、event conversion、Binder callback execution、primary `HalError` 生成だけを行う。callback artifact store の production clear は owner callback cleanup では `OwnerCallbackCleanupArtifactCommand`、service boot reset では `CallbackArtifactResetCommand` を受ける artifact bridge だけを許可し、runtime callback registry mutation は service_runtime finish use-case だけが所有する。

### worker / callback / query / packet 境界補強

frontend worker の停止・置換は prepare / external join / complete の三相で扱う。public tune / scan request の object 解決、request validation、scan candidate calculation のような worker 停止前に判定可能な precondition は旧 worker の supersede stop より前に完了し、invalid request で既存 worker を破壊してはならない。prepare は runtime lock 内で対象 frontend、AIDL object、object generation、worker kind、停止対象 worker generation、事前算出した new worker generation candidate、bound demux rollback token / generation snapshot を固定した lifecycle ticket を発行する。candidate は runtime state へ予約 commit された generation ではなく、stop 前に overflow / availability を検査済みの値である。external join は runtime lock 外で実行する。complete は ticket を消費し、object generation、frontend id、停止対象 worker generation、live reader、scan session、bound demux snapshot を再検証してから、candidate を使う worker install / cleanup / rollback を実行する。install / begin step は `commit_generation(candidate)` 相当で candidate がまだ単調増加条件を満たすことを再検証し、競合で失効した candidate は post-complete install failure として start rollback diagnostic へ記録する。join 後に frontend id だけを根拠として runtime mutation してはならない。

frontend worker replacement では、旧 worker を stop/join した後に、新 worker generation candidate の算出、bound demux rollback token preparation、request validation、scan candidate calculation、または新 request 開始に必須で旧 worker stop 前に実行可能な fallible pre-start preparation を初めて実行してはならない。旧 worker stop 前に実行できる generation candidate calculation / rollback-token preparation / request preflight は prepare phase へ移し、ticket に含める。new worker generation candidate を実 runtime state へ commit する install / begin step は complete success 後に実行してよいが、その入力は ticket 内の candidate、frontend snapshot、bound demux rollback token / generation snapshot から導出し、ここで新たに generation candidate calculation や rollback-token preparation を行ってはならない。complete 失敗は replacement cleanup report と同じ transaction outcome に `CompleteReplacement` 相当の typed step として primary failure を記録し、旧 worker が停止済みであること、新 worker generation が未開始であること、rollback / restart を試行したかまたは意図的に試行しない理由を typed diagnostic に残す。complete success 後の install / begin / worker start failure は TuneStartRollback / ScanStartRollback / WorkerStartRollback 等の start rollback diagnostic に、generation candidate と旧 worker stop 済みの文脈を失わない形で記録する。この条件を満たさず、旧 worker stop 成功後の `prepare_frontend_worker_generation()` などの fallible preparation failure だけで public tune / scan を failure へ返す設計は不可とする。

DVR status callback delivery では runtime registry 上の callback_present と AidlServiceContext の callback artifact store lookup 結果を照合する。artifact missing、artifact store failure、notifier preflight skip は DVR post-commit notification diagnostic に必ず記録する。artifact missing は Binder delivery failure ではなく、registered callback unhealthy marking 対象にしない。Strong を取得した後の Binder failure だけを unhealthy marking 対象にする。

callback artifact cleanup、callback registration rollback、object close callback cleanup、service boot reset callback cleanup は runtime prepare -> artifact command execution -> runtime finish の all-attempt transaction とする。runtime registry は callback lifecycle accounting の正本であり、artifact mutation command id を発行する。AIDL 層は command を実行し、outcome を runtime finish へ返す。artifact failure、runtime finish failure、registry missing は callback artifact/runtime split diagnostic に必ず保持する。primary failure と cleanup / finish failure が同時に存在する場合は failure composition helper 群で composed failure を作成し、同時に split diagnostic も記録する。FirstErrorCollector は同一 cleanup phase の cleanup step first error 収集だけを担当する。object close / drop-leak terminalization、frontend worker cleanup、frontend tune/scan rollback cleanup、frontend close owner-loss cleanup は all-attempt 実行後、各 step outcome を domain-specific adapter (`ObjectCleanupStepOutcome` / `FrontendWorkerCleanupStepOutcome`) として保持し、共通 `CleanupExecutionReport<TStepOutcome, TFailure>` から public failure 用の first-error を射影する。object cleanup の artifact/domain/runtime 差分と frontend worker cleanup の object-target/frontend-target 差分は variant-specific target/step outcome で表現し、nullable field 群で後から意味を復元する形にしてはならない。public failure を first-error に畳む場合でも、plan-owned result から成功 step、失敗 step、cleanup kind、対象 id / generation、cascade target detail、worker kind / worker generation、scan cancel / live-data cleanup result、frontend snapshot restore result、bound demux rollback result、owned LNB close result を失ってはならない。AIDL façade や service_runtime use-case が public `BinderResult` / `HalError` へ変換する前に report を `.into_result()` だけで破棄してはならず、`SharedCleanupDiagnostics<TRecord>` を基盤にした shared diagnostic sink に typed diagnostic record として記録する。production accessor は `CleanupExecutionDiagnosticSnapshot<TRecord>` として records だけでなく bounded store の dropped count も返し、overflow 監査を可能にする。object close / drop-leak terminalization report 記録は runtime lock 再取得に依存してはならず、frontend worker cleanup も可能な限り runtime instance から先に clone した shared sink へ記録する。finish/terminalization/worker cleanup result と診断記録失敗は failure composition で両方表面化する。artifact mutation 成功後に runtime finish lock が失敗した場合も、AIDL が独自診断 store を持つのではなく、service_runtime instance が発行した shared diagnostic sink へ `CallbackArtifactRuntimeSplitDiagnosticRecord` を記録し、artifact/runtime split failure として診断に残す。

root/object query は RootQueryRequest / RootQueryResponse / ObjectQueryRequest / ObjectQueryResponse の DTO 境界に集約する。query_api.rs は registry entry、runtime state、signal state、filter open type helper、PCR filter lookup helper を AIDL façade の policy source として返さない。AIDL 層は DTO response から AIDL 型へ変換するだけで、registry entry や runtime state の policy を所有しない。

PacketPid は production packet routing、validation、diagnostic construction では raw integer PID に戻して使わない。raw PID が必要な AIDL 変換は terminal AIDL presentation boundary として扱う。service_runtime が descrambler claim と packet path を照合する場合は、既に検証済みの `DescramblerPid` から `PacketPid` への一方向 typed bridge だけを使い、packet-derived `PacketPid` から raw PID を取り出して照合しない。ログと診断表示は diagnostic typed context から生成する。PacketPid の Display は表示整形だけに限定し、routing、validation、diagnostic classification の入力にしない。PipelineDiagnostic は PID を Option<i32> field bag として返さず、PacketPid を持つ typed accessor と PID非適用を表す typed outcome に分ける。

Descrambler diagnostic は Option field bag を使わず、set-key-token failure、PID claim rejection、packet policy failure、packet source-filter validation failure、cleanup key release failure のような事象別 record として保持する。PID は typed id とし、欠落を Option field で表すのではなく、欠落し得る事象は別 record / typed context として表現する。bounded diagnostic store は維持する。

後片付け診断は内部情報として扱う。所有者ごとに、直近の後片付け試行を集約した有界のメモリースナップショットを1個保持する。更新は全体を一括で置き換え、主処理の失敗と後片付けの失敗を型で区別し、`omitted_failure_count` を飽和加算する。読み取りによって内容を削除してはならない。記録件数は設計で固定せず、1件以上の容量を承認済みの診断用メモリー枠から実装時に選ぶ。上書きまたは省略の有無によって公開結果を変えてはならない。永続化、操作ID、有効期限、確認・削除手順、`ProductProfile` の容量項目は設けない。
