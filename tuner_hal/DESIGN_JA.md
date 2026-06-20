# Tuner HAL 設計判断


## DESIGN_JA.md の責務境界

`DESIGN_JA.md` は Tuner HAL の設計正本である。本書は、AOSP 公開契約、ARIB/ISDB 入力処理、本製品の対応範囲、状態所有者、資源寿命、失敗時遷移、quarantine 条件、未対応機能の返却方針を定義する。

本書は、作業履歴、リリース履歴、ビルド手順、atest手順、VTS手順、静的検索手順、成果物命名規則、完了宣言テンプレートを定義しない。それらは `開発規則.md`、`タスク完了判定の実施方法.md`、`CHANGELOG.md`、各作業計画を正とする。

本書と他文書で、状態遷移、資源寿命、戻り値、capability、失敗時波及範囲が矛盾する場合は、本書を正として他文書を修正する。ただし、作業完了判定、成果物名、build / atest / VTS 手順については `タスク完了判定の実施方法.md` を正とする。


### 共通部品の定義条件

本書で共通部品と呼ぶものは、単なる関数名・ファイル名・薄い委譲 wrapper ではない。共通部品として設計正本へ置く場合は、次を定義する。

| 項目 | 必須内容 |
|---|---|
| 論理契約名 | 例: `ObjectCloseTxn`、`ObjectMethodTxn`、`StreamBoundaryTxn` |
| 実装正本 | 状態・寿命・失敗時遷移を所有する module / file / type |
| 公開入口 | AIDL層または service_runtime 層から呼んでよい entry point |
| 所有する状態 | lifecycle、registry、callback、worker、FMQ、packet assembler など、当該部品が正本として変更する状態 |
| 所有しない状態 | 呼び出し元または別 transaction が所有する状態 |
| phase order | lifetime / request build / validation / dispatch / commit / rollback / quarantine の順序 |
| 失敗時処理 | 戻り値、rollback、cleanup継続、cleanup failed、quarantine の扱い |
| 呼び出し許可層 | AIDL method body、object wrapper、service_runtime façade、domain transaction のうち許可する層 |
| 呼び出し禁止層 | 誤用を避けるため直接呼んではならない層 |
| 最低テスト | status precedence、rollback、retry、cleanup failure、quarantine などを固定する test |

上記を満たさないものは、共通部品ではなく helper、façade、adapter、または implementation detail と呼ぶ。helper / façade が transaction 正本を名乗ってはならない。

`transaction` という名前は、少なくとも状態変更の開始条件、commit、rollback または cleanup failure / quarantine のいずれかを所有する部品にだけ使う。runtime lock を取って closure を呼ぶだけの部品、引数を同名 method へ横流しするだけの wrapper、domain naming を隠さない薄い façade は transaction ではない。


## 外部文書参照: no-panic / 劣化起動 / 閉鎖側失敗境界

この項目は実装規約であるため、詳細な禁止事項、エラー写像、劣化起動、mutex汚染、ワーカー生成・join 方針は `tuner_hal/CODE_CONVENTION.md` を正とする。本書では Tuner HAL が no-`panic` / 劣化起動 / 閉鎖側失敗 を設計上必須とすることだけを固定する。


## 正本・移動済み情報の読み方

本書の正本階層は次の順とする。

1. `DESIGN_JA.md の責務境界`、`製品スコープ / AOSP capability / VTS profile 境界`、`AIDL 契約境界`、`Tuner HAL 状態遷移表SSOT` を最上位正本とする。
2. `0-S. 状態所有・寿命・失敗時遷移設計`、`表1`〜`表20`、`ARIB/ISDB入力処理契約`、`Stream boundary 契約`、`Packet pipeline 正本契約`、`AV shared handle 入出力契約` を、現在の設計契約の正本とする。
3. 旧 `補足契約:` 章は本体正本章へ吸収済みであり、本書内に二重正本として残さない。
4. 個別リリースの履歴、作業経緯、ビルド/atest/VTS/静的検索/成果物命名/完了宣言は本書では定義しない。履歴は `CHANGELOG.md`、完了判定は `タスク完了判定の実施方法.md` を正とする。

削除・移動した旧記載の追跡表は現行リリース物に置かない。現行仕様は本書、実装規約は `tuner_hal/CODE_CONVENTION.md`、統合手順は `tuner_hal2/INTEGRATION.md`、変更履歴は `tuner_hal/CHANGELOG.md` を正とする。存在しない trace 文書を正本参照にしてはならない。

## 製品スコープ / AOSP capability / VTS profile 境界

製品全体のリリース到達点、日本向け scan 候補、サービス検出、channel key の実装データ保持者は tv 直下の `開発規則.md` を正とする。本節では、Tuner HAL の capability、VTS/profile、AIDL戻り値に閉じる境界だけを固定する。HAL は渡された tune request を処理し、BLIND_SCAN や HAL-generated Japanese scan plan は capability / VTS profile で対応宣言しない。

`config/tuner_vts_config_aidl_V2.xml` は 明示選局点、AV filter、録画DVR経路 の接続確認に限定する。descrambler オブジェクト は Tuner HAL AIDL 面として実装するが、VTS/profile では本番経路のスクランブル解除成功を対応宣言しない。

Tuner HAL の capability / VTS profile では TS 入力だけを宣言する。製品全体の TS-only スコープは `開発規則.md` を正とし、本書では Tuner HAL の宣言値と返却値を固定する。MMTP、TLV、ALP、IP CID は capability と VTS profile に宣言しない。`IFilter.configureIpCid()` は filter種別にかかわらず `UNAVAILABLE` とする。CID を保存だけして 照合、経路制御、配送 に使わない成功扱いの無処理 を残してはならない。


### export ID と VTS profile の固定

Tuner HAL が framework へ export する frontend ID は backend の単純な numeric index だけに依存しない。`px4video0` と `pxmlt5video0` のように異なる device family が同じ unit index を持つ場合でも、HAL の frontend ID と physical group ID は衝突してはならない。device family code と unit index を組み合わせ、1,000,000 番台の px4 frontend ID として export する。DVB frontend ID はハッシュではなく固定ビット割当で生成し、`2,000,000 + (adapter_id << 12) + (frontend_index << 4) + variant` とする。`adapter_id` と `frontend_index` は 8 bit、`variant` は 4 bit で、variant は ISDB-T=0、ISDB-S=1 に固定する。範囲外の DVB probe は export しない。生成後の duplicate ID 検出は最終保険として残す。px4 frontend の `exclusiveGroupId` は unit index 単独値ではなく、device family code と unit index を含む packed physical group id として返す。

VTS設定は `profiles/*.yaml` から `tools/render_vts_config.py` で生成する。LabProfile は ISDB-T、BS、CS110 をすべて持ち、ProductProfile や DiagnosticProfile と混ぜない。VTS検査用プロファイルは代表 PID による 188-byte TS 録画/再生経路 接続確認に使うが、設計上の対応宣言は 1サービスTS録画 であり、8 PID 前提の 検査専用 実装に縮退させてはならない。TIS 録画 UI や予約スケジューラとは結びつけない。製品向け復号フローは VTS検査用プロファイル で対応宣言せず、ECM filter と `<descramblers>` は生成しない。

### VTS profile / capability / 実装済み機能 対応表

VTS XML/profileで使う機能、capabilityで宣言する機能、実装済み機能は一致させる。VTS profileで使用する機能をcapability非宣言または未実装扱いにしてはならない。capabilityで宣言する機能をVTS/profileから到達不能にして検査を回避してはならない。

| 領域 | capability / profile 方針 | 設計契約 |
|---|---|---|
| `setDataSource(NULL)` | AOSP意味論として存在。ただしAndroid 14 AIDL Rust 生成境界では現行実装済み扱いにしない | 本書の「nullable Binder 境界」節を正とし、現行実装済み対象に含めない |
| `IDescrambler.addPid/removePid(NULL)` | AOSP意味論として存在。ただしAndroid 14 AIDL Rust 生成境界では現行実装済み扱いにしない | 本書の「nullable Binder 境界」節を正とし、現行実装済み対象に含めない |
| AV shared handle release | media filter shared memory profileでは到達する | `releaseAvHandle(fd付き handle, 0)` を成功させる |
| monitor event | `monitorEventTypes > 0` を使うprofileだけ対応宣言 | 非対応profileでは非0 mask を使わない |
| AV passthrough | 対応宣言しない | profileでは `isPassthrough=false` に固定する |
| `linkCaps` | main type 粒度 | subtype 成否は `setDataSource()` 検証表を正とする |


### Tuner HAL 固定境界

- CS110 は周波数のみで選局する。ISDB-S settings で `streamIdType=UNDEFINED` かつ `streamId=0` の明示未指定、または AOSP SDK の default 表現である `streamIdType=STREAM_ID` かつ `streamId=INVALID_STREAM_ID(0xFFFF)` だけを selector なしとして扱う。CS110 tune request に TSID / relative stream-number selector が指定された場合は `INVALID_ARGUMENT` とする。`streamIdType=RELATIVE_STREAM_NUMBER` の負値、`streamIdType=UNDEFINED` の負値、その他の負値 selector は未指定へ丸めない。
- BS は TSID 指定を要求する。px4 backend だけ relative stream number を受け付け、DVB backend では relative stream number を `INVALID_ARGUMENT` とする。BS `STREAM_ID` の 0..11 は全backendで `INVALID_ARGUMENT` とする。
- コールバック失敗、ワーカー異常終了、FMQ / EventFlag 失敗の状態遷移、診断、後続処理停止条件は表7・表8を正とする。本節では再定義しない。
- DVR 状態 interval はコールバックワーカーの周期にだけ使う。ワーカーの wait は stop signal で wake 可能な cancellable wait とし、close / Drop / shutdown は interval 満了を待たない。
- `getAvSharedHandle()`、AV filter `start()`、`releaseAvHandle()` の状態別契約は、本書の「表4. AV共有メモリ資源寿命表」を正とする。
- device未検出 / open失敗 は `UNAVAILABLE`、device が存在する状態での runtime ioctl / read失敗 は `UNKNOWN_ERROR` とする。クライアント由来の不正入力と runtime I/O失敗 を同じエラー経路に入れない。
- filter monitor event は profile / capability 依存とする。monitor event 非対応 profile では `configureMonitorEvent(0)` のみ成功し、非0 mask は `UNAVAILABLE` とする。monitor event 対応 profile で `monitorEventTypes > 0` を使う場合は、`configureMonitorEvent(nonzero)` を成功させ、要求 mask に対応する monitor event を配送する。通常の `DATA_READY` / `OVERFLOW` / `onFilterEvent()` delivery は monitor mask で抑止しない。
- soft demux の section / PES assembler と filter `stop()` / `flush()` / `configure()` / `close()` の状態別契約は、本書の「表1. IFilter 状態表」を正とする。
- `setMaxNumberOfFrontends()` は `0 <= max_number <= default_max` だけを成功させる。負値と `default_max` 超過はどちらも `INVALID_ARGUMENT` とする。
- 製品実行時 の frontend registry は実在 probe できた backendエントリ だけで構成する。probe 失敗は 診断情報レコード に残し、劣化 frontendエントリ / テスト劣化補助関数 / 診断劣化補助関数 は作らない。


### nullable Binder 境界

AOSP意味論として NULL binder 入力を持つ境界は、`IFilter.setDataSource(NULL)`、`IDescrambler.addPid(NULL)`、`IDescrambler.removePid(NULL)`、`IFrontend.setCallback(NULL)`、`ILnb.setCallback(NULL)` とする。Android 14 AIDL Rust 生成 trait の現行公開メソッドはこれらのうち一部を non-null `Strong<...>` として受けるため、現行実装済み対象には含めない。

この境界は本書で管理する。`future_work` を現行リリース契約の正本として参照してはならない。non-null 経路の状態遷移、戻り値、資源寿命、失敗時遷移は本書の各表を正とする。この nullable 境界を解く場合は、正式な `DESIGN_JA.md` へ吸収してから完了扱いにする。

### Android 14 AIDL filter source 境界の現行処理

`IFilter.setDataSource()` は AOSP意味論では `source == NULL` により sink filter の入力元を demux input へ戻す。ただし Android 14 AIDL Rust 生成境界では現行 公開メソッド が non-null `Strong<dyn IFilter>` を要求するため、`setDataSource(NULL)` は 現行実装済み対象に含めない。non-null source filter を指定する場合の互換性、閉鎖済み source、別 demux source、自己参照、sink 開始中の扱いは、本書の「表1-D. `setDataSource()` 互換表」を正とする。`configure()` は既存上流接続を必ず解除する。nullable Binder 境界は本書の「nullable Binder 境界」節を正とし、現行実装済み対象に含めない。

`IDescrambler.addPid()` / `removePid()` は、AOSP意味論では `optionalSourceFilter == NULL` を demux input 全体に対する PID 登録 / 解除として扱う。ただし Android 14 AIDL Rust 生成境界では現行 公開メソッド が non-null `Strong<dyn IFilter>` を要求するため、NULL 経路は 現行実装済み対象に含めない。non-null source filter 経路は、本書の「表D-1. IDescrambler PID 操作表」を正とし、同一 demux、非閉鎖、世代一致を検証する。nullable Binder 境界は本書の「nullable Binder 境界」節を正とし、現行実装済み対象に含めない。


## AIDL 契約境界

`IFilter`、`IDvr`、`IFrontend`、`IDemux`、`ILnb`、`IDescrambler` の 公開メソッド は、AIDL HAL の契約面として close 後状態を必ず検査する。状態別の戻り値、次状態、維持する内部状態、破棄・無効化する内部状態は、本書の「Tuner HAL 状態遷移表SSOT」を正とする。

`IFrontend.getStatus(statusTypes)` は、要求された `statusTypes` の各要素に対して、同じ順序で1つの `FrontendStatus` を返す。未対応 状態 type を黙ってdropして短い配列を返してはならない。未対応 状態 type が要求された場合、`getStatus()` は呼び出し全体を `INVALID_ARGUMENT` として失敗させる。`getFrontendStatusReadiness(statusTypes)` は AOSP VTS 期待に合わせ、要求された全 状態 type と同じ長さの readiness 配列を返す。`statusCaps` 外の type は要素ごとに `UNSUPPORTED`、`statusCaps` 内で backend が現在利用不可または 状態 word / telemetry を現在取得できない場合は `UNAVAILABLE`、tune/probe 中なら `UNSTABLE`、有効値を返せる状態なら `STABLE` とする。`statusCaps`、`getStatus()`、`getFrontendStatusReadiness()` は同一の 状態 support 判定 SSOT を使うが、戻り方は API ごとの AOSP 契約に従って分ける。`statusCaps` には起動時列挙時点で値の取得根拠を固定できる 状態 type だけを含め、read 時に失敗し得る optional ioctl 由来の 状態 type は含めない。telemetry 未取得値を `0` として成功返却してはならない。

`IFilter.setDataSource(source)` は、AOSP意味論では `source != NULL` の場合に指定 filter output を入力元とし、`source == NULL` の場合に sink filter の入力元を demux input へ戻す。ただし 現行実装では Android 14 AIDL Rust 生成 trait 上の nullable binder 受け口が未固定であるため、`setDataSource(NULL)` は実装済み対象に含めない。AOSP frozen/stable AIDL の vendor 独自改変、raw Binder transaction parser による公開契約を通さない実装は採用しない。non-null source filter 経路では、旧 `SourceFilter(filter_id, generation)` origin に属する section / PES assembler、continuity、flush generation、downstream partial state を切断し、旧 source 由来の未完了 payload を新 source 由来 payload へ連結してはならない。

`IFrontend.tune()` は binder thread 上で ロック 完了まで待ち続けない。前回 tune / scan の ワーカーを generation で無効化し、backend へ tune request を投入し、非同期 ワーカー が ロック timeout と event 通知を行う。`stopTune()`、`close()`、次回 `tune()`、`scan()` は該当 generation を cancel し、古い ワーカー からの `LOCKED` / `NO_SIGNAL` 通知を捨てる。

`IFrontend.scan()` は、同一条件の再 scan であっても成功扱いの無処理 にしない。AOSP 契約に従い、未完了の scan がある場合は既存 scan generation を停止し、新しい scan generation を開始する。既存 scan の callback から来る古い terminal event は generation mismatch として捨てる。

`IFrontend.close()` は frontend backend の critical cleanup を成功扱いで握り潰さない。公開 close では、scan cancel、tune ワーカー stop、ライブ pump stop、backend close、コールバック解除、demux unbind、frontend lease release を step runner として扱い、途中 step が失敗しても後続 cleanup を継続し、最初に観測した critical error を AIDL 状態 として返す。cleanup failure 後の frontend オブジェクト は通常操作へ戻さず、close retry だけを通常の復旧経路として許可する。戻り値を返せない Drop 経路は通常 cleanup の代替にせず、未 close または cleanup 未完了を DropLeakTxn に記録し、対象を漏えい診断 / 隔離診断へ落とす。

DVB / earth_pt1 backend では、`DTV_CLEAR` は明示的な tune 停止操作である `stop_tune()` の責務とする。DVB backend の `close()` は reader stop と fd release を行うが、`DTV_CLEAR` の実行を close の必須条件とはしない。したがって、DVB `close()` が `DTV_CLEAR` を発行しないことを release blocker または bug と扱わない。

`IFrontend.removeOutputPid(pid)` は、frontend 出力段で PID を除去できる実装が存在しない限り `UNAVAILABLE` とする。soft demux 後段の block list だけで PID を捨てる実装は、frontend-level output PID removal を実装したことにしない。

DVR playback は 対応宣言対象とする。DVR playback の水位通知は AIDL `PlaybackSettings.lowThreshold` / `highThreshold` の説明に合わせ、playback input FMQ の unused space size in bytes を基準に判定する。`SPACE_EMPTY`、`SPACE_ALMOST_EMPTY`、`SPACE_ALMOST_FULL` は threshold 到達時だけ通知し、中間水位では新規状態通知を行わない。used bytes を threshold として直接比較してはならない。標準閾値は buffer 容量比で low 25%、high 75% とし、VTS検査用プロファイル では XML 生成時に明示値へ展開する。

### DVR playback status の空き領域基準

DVR playback status は playback input buffer の空き領域、すなわち space を基準に判定する。

| PlaybackStatus | 意味 |
|---|---|
| `SPACE_EMPTY` | playback input buffer の空き領域が空、すなわち書き込み余地がない |
| `SPACE_ALMOST_EMPTY` | 空き領域が少ない |
| `SPACE_ALMOST_FULL` | 空き領域が多い |
| `SPACE_FULL` | 空き領域が満杯、すなわち buffer は空に近い |

使用済み量基準と空き領域基準を混同してはならない。


## Tuner HAL 状態遷移表SSOT

本節の表は、Tuner HAL の状態を持つ公開API、内部事象、資源寿命、戻り値、副作用のSSOTである。表に記載した状態別契約は、後続の散文で再定義しない。後続本文は、表だけでは読み取れない背景、製品方針、能力宣言、実装上の補足に限定する。


### 0-S. 状態所有・寿命・失敗時遷移設計

#### 0-S-1. 設計原則

Tuner HAL は、内部状態の正本を1つに固定する。複数の構造体が同じ状態を独立に保持してはならない。

公開API 主経路は、必ず正本所有者を経由する。共通部品を追加しただけ、一部経路が共通部品を呼ぶだけ、旧名が消えただけでは、設計適合とはみなさない。

各APIは次を固定する。

| 項目 | 内容 |
|---|---|
| 状態所有者 | どの構造体が正本か |
| 成功時commit | 何が確定するか |
| 失敗時rollback | 何を戻すか |
| rollback失敗時 | quarantine / failed / retry のどれに落とすか |
| cleanup失敗時 | 再試行可能に残すか、quarantineするか |
| Drop時動作 | Dropで何をしてよいか |
| 診断 | 何を残すか |
| 公開API戻り値 | どのAIDL状態を返すか |

rollback不能、cleanup不能、正本不一致、backend実状態とregistry不一致、ワーカー状態不一致は、通常状態として扱ってはならない。通常状態へ戻せない場合は quarantine または failed 状態へ落とす。

#### 0-S-2. 状態所有者表

| 資源 / 状態 | 正本所有者 | 補助所有者 | 禁止事項 |
|---|---|---|---|
| service lifecycle | `TunerHal` | なし | 子資源が service 全体状態を直接変更しない |
| frontend lease | `FrontendLedger` | `FrontendRuntime` | open count、physical group、runtime binding を別々に確定しない |
| frontend backend state | `FrontendRuntime` | backend adapter | backend実状態とruntime状態を通常状態で乖離させない |
| demux id / refcount | `DemuxLedger` | `DemuxHal` | live id、registry record、refcount を別々に確定しない |
| demux データ経路 | `DemuxRuntime` | `RuntimeIoRegistry` | FMQ/DVR/AV境界処理をAPIごとに重複実装しない |
| filter lifecycle | `FilterLedger` | `FilterHal`, `soft_demux` | soft demux filter record と binder object の片方だけを残さない |
| filter queue | `FilterQueueBacking` | `FilterHal` | write成功後のwake失敗を完全失敗として黙殺しない |
| DVR lifecycle | `DvrLedger` | `DvrHal`, `soft_demux` | DVR object と soft demux DVR record の片方だけを残さない |
| playback queue | `PlaybackQueueBacking` | playback ワーカー | ワーカー失敗だけでDVRをclosed扱いにしない |
| descrambler session | `DescramblerSession` | `DescramblerLedger` | demux binding、PID、source filter、key tokenを別々に閉じない |
| key token | `DescramblerKeyTable` | `DescramblerSession` | refcount不整合のまま成功扱いしない |
| LNB state | `LnbRegistry` | backend adapter | backend状態とregistry状態の乖離を通常状態にしない |
| ワーカー | `WorkerRuntime` | 所有オブジェクト | 所有者不明ワーカーを作らない |
| stream boundary | `StreamBoundaryTxn` | demux/filter/DVR/AV | tune/scan/source切替で境界処理を重複実装しない |
| packet pipeline | `PacketPipeline` | `soft_demux` | continuity、origin、generationを複数箇所で更新しない |
| AV shared memory | `AvSharedBacking` | `FilterHal` | fd番号一致をshared handle同一性条件にしない。fd付きhandle + `avDataId == 0` はclient側shared handle使用終了通知として扱う |

#### 0-S-3. 公開API transaction（状態遷移）契約

公開API の状態変更は、原則として次の段階で扱う。

```text
validate
  -> reserve
  -> prepare
  -> apply
  -> commit
```

| 段階 | 許可 | 禁止 |
|---|---|---|
| validate | 引数、状態、capability、owner一致、closed状態の確認 | 状態変更 |
| reserve | ledger / id / slot / ワーカーslot の仮確保 | backend実状態の変更 |
| prepare | ワーカー生成準備、コールバック経路準備、rollback snapshot取得 | 旧公開状態の破壊 |
| apply | backend / soft_demux / queue / registry への変更 | commit不能な変更をsnapshotなしに行うこと |
| commit | 正本状態の確定 | 後段で失敗し得る処理を未処理のまま成功扱いすること |
| rollback | commit前変更の取り消し | 失敗を握りつぶして通常状態へ戻すこと |
| quarantine | rollback不能資源の隔離 | 成功扱いで通常操作を許すこと |

commit前失敗では、成功戻りを返してはならない。commit後cleanup失敗では、APIの戻り値方針を各API表で固定し、必ず診断に残す。rollback失敗時は、対象資源を quarantine または failed 状態へ落とす。


#### 0-S-3A. 共通部品適用表

本表は、既存の状態表を再定義しない。公開API 主経路がどの共通部品を必ず通るかを固定し、APIごとの個別実装や正本を経由しない実装を禁止するための対応表である。既存共通部品で表現できない場合は、対象APIだけに同型処理を再実装せず、共通部品の責務を拡張してから接続する。

| 対象処理 | 所有共通部品 | 必ず通す経路 | 禁止する実装 |
|---|---|---|---|
| Filter / DVR `start()` の commit 後 コールバック失敗 | `PostCommitCallbackFailureTxn` | start commit 後の callback 失敗は rollback せず `callback_unhealthy` に固定。既存実装名が異なる場合でも、この責務を単一の callback health 正本へ移管する | APIごとに `stop_*()` を直接呼んで rollback すること、またはAPI別に同型の コールバック失敗 処理を再実装すること |
| Filter / DVR `flush()` の queue cleanup | `QueueCleanupTxn` | demux flush 後の FMQ / AV / playback cleanup 失敗を runtime failed または cleanup failed に接続 | `clear_best_effort()` で 公開API 成功に丸めること |
| Filter / DVR / Demux / Frontend / LNB / Descrambler `close()` | `CloseLifecycleTxn` | close開始時に通常API遮断を確定し、cleanup failed は再close可能に残す | `closed` だけを個別に立て、cleanup途中状態を隠すこと |
| ワーカー起動 / 停止 / join / wake | `WorkerFailureClassifier` | ワーカー制御失敗、コールバック失敗、backend failure を enum / domain error で分類 | `reason.contains(...)` など文字列分類で失敗種別を決めること |
| frontend / demux / source filter / flush 境界 | `StreamBoundaryTxn` / `SourceBoundaryTxn` | origin、generation、assembler、FMQ、AV shared、record queue を対象単位で処理 | 各APIが assembler / queue / generation を個別に直接操作すること |
| descrambler demux / PID / key cleanup | `DescramblerSessionCleanupTxn` / `DescramblerKeyTxn` | session と key table の更新・release・失敗集約を一体で扱う | 1件の stale cleanup 失敗で後続 session を未処理のまま抜けること |
| FMQ write / EventFlag wake | `FmqDeliveryTxn` | write と wake の結果を delivery commit として扱い、失敗分類を表6へ写像 | write成功後 wake失敗を単なるログで成功扱いすること |
| DVR playback read / inject | `PlaybackConsumeTxn` | FMQ read、TS parse、注入結果、消費確定を1つの状態機械で扱う | read済み入力を 注入結果未確認のまま一律消費済みにすること |


#### 0-S-4. 失敗分類と波及範囲

| 失敗種別 | 例 | 戻り値 | 波及範囲 | 禁止事項 |
|---|---|---|---|---|
| クライアント誤用 | 引数不正、owner不一致 | `INVALID_ARGUMENT` | 呼び出し対象のみ | backend/データ経路 failureへ昇格しない |
| invalid state | closed、closing、failed | `INVALID_STATE` | 呼び出し対象のみ | 引数不正と混同しない |
| unsupported | capability外、恒久非対応 | `UNAVAILABLE` | なし | callback/ワーカー状態を先に見て別エラーにしない |
| コールバック失敗 | Binder コールバック失敗 | API表に従う | コールバック所有者 | データ経路全体を即failedにしない |
| ワーカー失敗 | panic、join失敗、wake失敗 | `UNKNOWN_ERROR` またはAPI表 | ワーカー owner | コールバック失敗と混同しない |
| backend failure | ioctl/read/tune失敗 | `UNKNOWN_ERROR` | frontend/backend | demux全体へ無条件波及させない |
| データ経路 failure | FMQ/shared memory破損 | `UNKNOWN_ERROR` | 対象filter/DVR/AV | frontend backend failureと混同しない |
| ledger failure | 正本台帳不整合 | `UNKNOWN_ERROR` | 対象資源 | 通常状態に戻さない |
| rollback failure | 旧状態復元失敗 | `UNKNOWN_ERROR` | 対象資源をquarantine | 成功扱いにしない |
| cleanup failure | close/drop後片付け失敗 | API表に従う | 対象資源 | Dropだけに逃がさない |

#### 0-S-5. API別設計適合条件

各API表は、少なくとも次の列を持つ。既存表に同じ情報が分散している場合も、各項目への対応を追跡できることを必須とする。

| 項目 | 必須理由 |
|---|---|
| 事前状態 | closed / closing / failed / quarantined を区別するため |
| validate内容 | 引数不正と状態不正を分けるため |
| commit対象 | 成功時に何が確定するかを固定するため |
| rollback対象 | 失敗時に何を戻すかを固定するため |
| cleanup失敗時 | close / Drop / best-effort の扱いを固定するため |
| 次状態 | API戻り値と内部状態を一致させるため |
| 診断 | 失敗原因を追跡するため |

### Tuner HAL runtime 設計契約

Tuner HAL runtime の公開API状態、内部事象、資源寿命、失敗時波及範囲を以下の設計契約として固定する。

- 対象 tuner device が見つからない場合も HAL サービスは起動する。probe 結果が空の場合、存在しない frontend を registry に登録せず、`getFrontendIds()` と `getFrontendInfo()` で device absent の frontend を advertise しない。サービス 起動自体は継続し、device missing の縮退理由 を診断に残す。対象 資源への操作要求が来た場合は `UNAVAILABLE` と診断へ 異常時閉鎖済み する。
- filter ID は HAL 外部へ返す値を demux-local ID のまま維持する。DVR attach/detach、filter データ入力元、AV sync ID 取得では、渡された filter オブジェクト の内部 owner demux を検証し、owner demux が一致しない filter を `INVALID_ARGUMENT` で拒否する。
- ワーカー は handle 保存先の mutex を確保してから spawn する。保存先を確保できない場合は spawn しない。ワーカー `panic` は `WorkerHandle::join_from_owner()` 経由で診断へ残し、detached ワーカーを作らない。
- 長寿命 ワーカー の待機は `Mutex` + `Condvar` を基本とし、stop request → wake → join の順で停止する。`AtomicBool` は close済み / stop要求 / export済みなどの単純 flag に限定し、複合状態同期の代替にしない。`loom` は テスト専用 候補であり、通常 単体テスト と静的ロジック確認の代替にはしない。

- 現行仕様で管理対象となる長寿命ワーカーは、`WorkerHandle` が owner id、`JoinHandle`、owner `ConcreteWorkerSignal` を所有し、owner signal の `Mutex<WorkerSignalState> + Condvar` で stop/work generation を wake する。`WorkerExit` は `Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` を正式名とする。
- `frontend_tune_worker` / `frontend_scan_worker` の停止は、`AtomicBool + thread::sleep()` polling ではなく、`WorkerHandle::request_stop()` → `WorkerHandle::wake()` → `WorkerHandle::join_from_owner()` の順に行う。
- Demux close / ライブ pump failure / ワーカー spawn failure は子 Filter / DVR / runtime I/O を 異常時閉鎖済み にし、close後の既存 child オブジェクト の `configure()` / `start()` / `getQueueDesc()` などを成功扱いしない。
- frontend source transition は transactional に扱い、new bind / old unbind / record更新 / stream 境界 reset の途中失敗時には新 binding をrollbackし、rollback不能なら demux を 異常時閉鎖済み にする。
- 公開 `close()` は致命的な後片付け失敗を成功扱いしない。Drop は公開 `close()` の代替ではなく、未close / 後片付け未完了を診断し、隔離状態へ落とす最後の安全網に限定する。公開 Binder close は後片付け完了後に閉鎖済み状態を確定する。
- DVR start は 状態 interval 分だけ Binder thread を sleep しない。状態 interval は コールバック ワーカー の周期だけに使う。
- playback consumer は no data と fatal error を分離する。FMQ read error、demux mutex汚染、fatal demux error は ワーカー fatal stop として 診断情報と オブジェクト state に反映し、後続操作を成功扱いしない。
- px4 close は control FD だけでなく TS reader FD と reader state も解放する。
- px4 の CNR 取得は optional telemetry であり、`PTX_GET_CNR` 失敗だけで ロック/状態 query を fatal error にしない。
- セクションフィルター は condition の必要 byte 幅が payload 長を超える場合に match しない。prefix だけ一致した短い payload を match としない。
- セクションフィルターの `repeat=false` は重複抑止ではなく、同一 `start()` 世代内の配送停止条件である。`SectionBits` は最初に一致した section を1件配送した後、version や section number が異なる後続 section も配送しない。`TableInfo` は最初に一致した table id / table id extension / version を処理対象 table として固定し、その table の `0..last_section_number` を1回ずつ配送して table 完了後に停止する。table 完了前の別 version は配送しない。`repeat=true` の場合だけ同一条件の section / table を繰り返し配送する。section filter の配送可否状態は demux 入力から直接組み立てた section にだけ適用する。source filter 経由で section payload を再配送する経路は本製品では対応しない。この配送停止は公開 `IFilter.stop()` 呼び出しと同じ状態遷移ではない。filter object の公開状態は Started のまま維持し、利用側が明示的に `stop()` / `flush()` / `configure()` / `close()` を呼べる状態を保つ。
- `TableInfo.version` は `-1` または `0..31` だけを受け付ける。`-1` は wildcard、範囲外は `INVALID_ARGUMENT` とする。
- PES `streamId` は `0..=255` を明示 `stream_id` として照合し、`-1` だけを wildcard として扱う。その他の負値と `256` 以上は `INVALID_ARGUMENT` とする。`streamId=0` は wildcard ではなく、8-bit 値 `0x00` の明示照合である。
- `IFilter.setDataSource()` の互換性は本書の「表1-D. `setDataSource()` 互換表」を正とする。`setDataSource(NULL)` は AOSP意味論としては demux input 復帰であるが、現行 Android 14 AIDL Rust 生成境界では実装済み対象に含めない。filter source を指定する場合は、表1-D-3の subtype 別成立条件を正とする。source filter として指定できるのは TS生データフィルタだけである。下流として成功させるのは TS生データフィルタと record フィルタだけである。section / PES / AV への raw TS 再parse chain、および section payload、PES payload、AV payload、record payload を直接 source として再配送する経路は作らない。非対応の linkage は `UNAVAILABLE` とし、ペイロードなしフィルタを source または sink にする接続は `INVALID_ARGUMENT` とする。
- `IFilter.setDataSource(source)` の non-null source 経路は 同一demux内のfilter接続グラフ の接続だけを正式対象とする。`linkCaps` は同一 demux 内で開いた source / sink filter の main type 対応可否を表し、別 demux に属する filter を source に指定する経路を capability / VTS profile 対象に含めない。source / sink object の lifetime、generation、kind を先に確認し、その後に owner demux 不一致と自己参照を `INVALID_ARGUMENT` で拒否する。AOSP API 文面上の「another filter」は本製品では同一 demux の filter graph 内の別 filter として扱い、別demux間のfilter接続グラフは作らない。
- `IFilter.getQueueDesc()` の成否は configure 済みかどうかではなく、open時フィルタ種別が通常FMQを持つかどうかで決める。通常FMQ対象フィルタは未configureでも記述子取得を成功させる。
- 表1 / 表2 の `getQueueDesc()` 行は、対象オブジェクトが close開始後、閉鎖済み、後片付け未完了、異常時閉鎖済み、隔離済み、runtime failed、callback_unhealthy のいずれにも該当しない通常可用状態に限って適用する。これらの横断gateに該当する場合は、表5および失敗分類表を優先する。
- `IDescrambler.addPid()` / `removePid()` の source filter は AOSP意味論では optional であり、`NULL` は demux 入力全体の PID 指定である。ただし 現行 Android 14 AIDL Rust 生成境界では実装済み対象に含めない。
- AV共有メモリの slot size は filter `bufferSize` に依存させず、製品定数 `AV_SHARED_SLOT_SIZE_BYTES` で固定する。
- 入力値不正は `INVALID_ARGUMENT`、未対応 capability は `UNAVAILABLE`、オブジェクト state 不整合は `INVALID_STATE`、mutex汚染 や内部整合性崩壊は `UNKNOWN_ERROR` / `HalError::Internal` に写像する。
- CHANGELOG と ログ message を除き、source comment は日本語に統一する。
- AV filter の `start()`、shared backing、MediaEvent、`releaseAvHandle()` の状態別契約は、本書の「表4. AV共有メモリ資源寿命表」を正とする。
- A/V sync の状態別契約は本書の「A/V sync 方針」と「A/V sync 非採用範囲」を正とする。


### 0. 総則

#### 0.1 本製品の固定方針

| 項目 | 固定内容 |
|---|---|
| 入力範囲 | 製品全体の入力方式スコープは `開発規則.md` を正とする。本書では Tuner HAL の capability / VTS profile として TS 入力だけを宣言し、MMTP、TLV、ALP、IP CID を宣言しないことを固定する |
| ライブAV正式経路 | non-passthrough `MediaEvent` + 共有メモリ + `dataId` 経路だけを正式対応とする |
| AVペイロードとFMQ | AVペイロードは通常FMQへ書き込まない。EventFlag は FMQ対象経路の通知にだけ使う |
| AV共有メモリ解放 | `releaseAvHandle(avMemory, dataId)` は表1-C-AVHの判定優先順位に従う。`getAvSharedHandle()` が返した fd付き shared handle は、`releaseAvHandle(fd付き handle, 0)` により client 側 shared handle 使用終了通知として受理する。`dataId=0` は利用者側 AV handle 使用終了通知であり、shared backing、公開済みハンドル、既存`dataId`、使用中領域を破棄しない |
| AV passthrough | 本製品では恒久的に対応しない。passthrough capability は宣言せず、passthrough要求は configure時 `UNAVAILABLE` とする |
| 監視イベント配送 | profile / capability 依存とする。非対応 profile では `configureMonitorEvent(0)` は成功、非0マスク値は `UNAVAILABLE`。対応 profile で `monitorEventTypes > 0` を使う場合は非0マスク値も成功し、要求eventを配送する |
| PCR | ペイロードキューとして公開しない。AV同期の内部状態として扱う |
| 未対応機能 | capability と VTS profile に宣言しない。要求された場合は configure時、専用API呼び出し時、対応する公開API呼び出し時のいずれかで `UNAVAILABLE` とする |
| close | `closed` は公開API遮断ゲート、`cleanup_complete` は後片付け完了根拠として別管理する |
| ABI不整合 | AIDL ABI、Rust/C 接続層の関数シグネチャ、リンク不整合は実行時状態表に入れない。ビルド、リンク、AIDL確認、VINTF確認で弾く対象とする |

#### 0.2 状態圧縮の許可条件

状態遷移表で複数の状態を1行へ圧縮してよいのは、次の4条件を全て満たす場合だけである。

| 条件 | 固定内容 |
|---|---|
| 条件1 | 選択式の戻り値、選択式の次状態、未固定語をセル内に書かない |
| 条件2 | 対象状態集合を表内に明記し、集合のヌケモレを許さない |
| 条件3 | 戻り値、次状態関数、副作用、診断、資源寿命が対象状態集合内で完全に同じである |
| 条件4 | 同値性根拠を表内に明記する |

次状態は固定値だけでなく、`入力状態を維持`、`共有ハンドル軸だけ公開済みに変更` のような関数で固定してよい。関数で固定する場合は、変更する状態軸と維持する状態軸を表内に書く。

#### 0.3 文書間の責務境界

| 文書 | 正とする内容 | 禁止事項 |
|---|---|---|
| `tuner_hal/DESIGN_JA.md` | Tuner HAL の公開API状態、内部事象、資源寿命、戻り値、副作用、確定点、巻き戻し、閉鎖側失敗の対象 | 同じ状態遷移契約を他文書で再定義すること |
| `tuner_hal/CODE_CONVENTION.md` | Tuner HAL 固有の実装規約、禁止構文、補助関数 使用規則、静的確認観点 | DESIGN_JA.md の状態遷移、戻り値、資源寿命を別内容で定義すること |
| `GLOBAL_CODE_CONVENTION.md` | Rust / Kotlin 全体に共通する実装規約 | Tuner HAL 固有の状態遷移を定義すること |
| `タスク完了判定の実施方法.md` | 検査手順、証跡の取り方、判定時の確認順序 | 設計契約や実装規約を新規定義すること |
| `tuner_hal/CHANGELOG.md` | 変更履歴、リリース履歴、過去の作業理由 | 現行設計の正本として扱うこと。CHANGELOG にしかない方針で実装を正当化すること |

状態遷移、資源寿命、失敗時の戻り値、閉鎖側失敗の対象について文書間に重複または差分がある場合は、本節の表を正として他文書を修正する。

### 表0-F. IFrontend scan 状態表

`scan()` が成功した場合は、常に新しい scan generation を開始する。同一条件の再 scan を成功扱いの無処理 にしてはならない。

| No | 事前状態 | 呼び出し | AIDL戻り値 | 次状態 | 副作用 | 設計上の成立条件 |
|---:|---|---|---|---|---|---|
| FR-001 | Idle | `scan(settings, type)` | 成功 | Scanning(generation+1) | 新 scan generation を開始 | backend へ新 scan request が投入される |
| FR-002 | Scanning | `scan(same settings, same type)` | 成功 | Scanning(generation+1) | 既存 scan を停止し、新 scan を開始 | 同一条件でも 無処理 にならない |
| FR-003 | Scanning | `scan(different settings/type)` | 成功 | Scanning(generation+1) | 既存 scan を停止し、新 scan を開始 | 古い callback は generation mismatch で捨てる |
| FR-004 | Scanning | `stopScan()` | 成功 | Idle | 現 scan generation を停止 | terminal reason を Cancelled として診断へ残す |
| FR-005 | Idle | `stopScan()` | 成功 | Idle | なし | 重複 stop は冪等成功 |
| FR-006 | Closing / Closed | `scan(...)` | `INVALID_STATE` | 入力状態を維持 | なし | 閉鎖中または閉鎖後に scan を開始しない |

### 表1. IFilter 状態表

#### 表1-A. IFilter 状態コード

| 状態コード | 状態名 | 意味 |
|---|---|---|
| F0 | 未設定 | `openFilter()` 後、`configure()` 未完了 |
| F1 | FMQ設定済み | section、PES、TS生データ、録画補助情報の FMQ対象フィルタが configure 済み |
| F2 | FMQ開始済み | FMQ対象フィルタが start 済み |
| F3 | FMQ停止済み | FMQ対象フィルタが stop 済み |
| F4 | ペイロードなし設定済み | PCR、監視、状態通知専用など、通常FMQへ公開するペイロードを持たないフィルタが configure 済み |
| F5 | ペイロードなし開始済み | ペイロードなしフィルタが start 済み。監視イベント配送は発生しない |
| F6 | ペイロードなし停止済み | ペイロードなしフィルタが stop 済み |
| A0 | AV設定済み・補助種別未設定・ハンドル未公開 | live AV filter が configure 済み、`configureAvStreamType()` hint 未設定、共有ハンドル未公開。audio/video routing 種別は open subtype から導出済み |
| A1 | AV設定済み・補助種別設定済み・ハンドル未公開 | live AV filter が `configureAvStreamType()` 済み、共有ハンドル未公開。routing 種別は open subtype と一致する場合だけ設定成功 |
| A2 | AV設定済み・補助種別未設定・ハンドル公開済み | A0 を基底状態として共有ハンドルを公開済み |
| A3 | AV設定済み・補助種別設定済み・ハンドル公開済み | A1 を基底状態として共有ハンドルを公開済み |
| A4 | AV開始済み・補助種別未設定・ハンドル未公開 | A0 から start 済み |
| A5 | AV開始済み・補助種別設定済み・ハンドル未公開 | A1 から start 済み |
| A6 | AV開始済み・補助種別未設定・ハンドル公開済み | A2 から start 済み |
| A7 | AV開始済み・補助種別設定済み・ハンドル公開済み | A3 から start 済み |
| A8 | AV停止済み・補助種別未設定・ハンドル未公開 | A4 から stop 済み |
| A9 | AV停止済み・補助種別設定済み・ハンドル未公開 | A5 から stop 済み |
| A10 | AV停止済み・補助種別未設定・ハンドル公開済み | A6 から stop 済み |
| A11 | AV停止済み・補助種別設定済み・ハンドル公開済み | A7 から stop 済み |
| F15 | 閉鎖済み | `close()` 後片付け完了済み |
| F16 | 異常時閉鎖済み | 作業スレッド致命停止、FMQ/共有メモリ致命失敗、後片付け未完などで公開APIを遮断した状態 |

AV filter の audio/video routing 種別は open subtype を正とする。TsAudio は Audio、TsVideo は Video である。`configureAvStreamType()` は codec / stream type hint を保存する補助APIであり、未実行であっても `setDataSource()`、`start()`、PES/AV routing、MediaEvent 配送の必須条件にはしない。

#### 表1-B. IFilter 基本API状態契約

| No | API / 入力 | 対象状態集合 | AIDL戻り値 | 次状態関数 | 副作用 | 診断 | 同値性根拠 / 設計上の成立条件 |
|---:|---|---|---|---|---|---|---|
| F-B-001 | `configure()` FMQ対象設定 | F0 | 成功 | F1 | queue世代を更新し旧一過性状態を消去 | `filter_configure_success` | 未設定からFMQ対象へ進む |
| F-B-002 | `configure()` ペイロードなし設定 | F0 | 成功 | F4 | queueを公開しない種別として設定 | `filter_configure_success` | 未設定からペイロードなしへ進む |
| F-B-003 | `configure()` live AV non-passthrough | F0 | 成功 | A0 | AV世代を進め、旧AV資源を全破棄。TsAudio は Audio、TsVideo は Video の routing 種別を open subtype から導出する | `filter_configure_success` | AVはハンドル未公開で開始する。`configureAvStreamType()` 未実行でも routing 種別は存在する |
| F-B-004 | `configure()` AV passthrough | F0 | `UNAVAILABLE` | F0 | なし | `unsupported_passthrough_configure` を増やす | 本製品では passthrough を恒久非対応とする |
| F-B-005 | `configure()` MMTP / TLV / ALP / IP CID | F0 | `UNAVAILABLE` | F0 | なし | `unsupported_filter_configure` を増やす | Tuner HAL capability / VTS profile では宣言しない方式を成功扱いにしない |
| F-B-006 | `configure()` 再設定 | F1, F3 | 成功 | F1 | queue世代を更新し旧データを破棄 | `filter_reconfigure_success` | 開始中でない FMQ対象状態は再設定に関して同値 |
| F-B-007 | `configure()` 再設定 | F4, F6 | 成功 | F4 | 一過性状態を破棄 | `filter_reconfigure_success` | 開始中でないペイロードなし状態は再設定に関して同値 |
| F-B-008 | `configure()` 再設定 | A0, A1, A2, A3, A8, A9, A10, A11 | 成功 | A0 | AV世代を進め、共有ハンドル、使用中領域、`dataId`、`configureAvStreamType()` hint を全無効化。routing 種別は open subtype から再導出する | `filter_reconfigure_success` | 開始中でないAV状態は再設定に関して同値。再設定後は補助種別未設定へ戻るが routing は可能 |
| F-B-009 | `configure()` 開始中 | F2, F5, A4, A5, A6, A7 | `INVALID_STATE` | 入力状態を維持 | なし | `configure_while_started` を増やす | 開始中再設定を禁止する |
| F-B-010 | `start()` FMQ対象 | F1, F3 | 成功 | F2 | FMQ作業スレッドを開始し、停止済みなら再開 | `filter_start_success` | F1 と F3 は start に関して戻り値、副作用、次状態が同一 |
| F-B-011 | `start()` ペイロードなし | F4, F6 | 成功 | F5 | 状態だけ開始済みにする。監視イベント配送は発生しない | `filter_start_success` | F4 と F6 は start に関して同値 |
| F-B-012 | `start()` AV | A0, A1, A2, A3, A8, A9, A10, A11 | 成功 | 実行状態軸だけ開始済みに変更。他軸は維持 | 新規配送可能状態へ進む。ハンドル未公開中はAVペイロードを配送しない | `filter_start_success` | 戻り値、診断、状態軸変換規則、資源寿命が同一。配送可否はハンドル軸から導出する |
| F-B-013 | `start()` 既に開始済み | F2, F5, A4, A5, A6, A7 | 成功 | 入力状態を維持 | なし | `start_idempotent` を増やす | 重複 start は冪等成功 |
| F-B-014 | `start()` 未設定 | F0 | `INVALID_STATE` | F0 | なし | `start_invalid_state` を増やす | 未設定では開始対象が存在しない |
| F-B-015 | `stop()` FMQ対象 | F2 | 成功 | F3 | 新規FMQ書き込みを停止 | `filter_stop_success` | FMQ開始状態を停止状態へ進める |
| F-B-016 | `stop()` ペイロードなし | F5 | 成功 | F6 | 状態だけ停止済みにする | `filter_stop_success` | 配送資源を持たない |
| F-B-017 | `stop()` AV | A4, A5, A6, A7 | 成功 | 実行状態軸だけ停止済みに変更。他軸は維持 | 新規AV配送を停止。既存 `dataId` は release / flush / close まで維持 | `filter_stop_success` | 戻り値、診断、状態軸変換規則、資源寿命が同一 |
| F-B-018 | `stop()` 非開始設定済み状態 | F1, F3, F4, F6, A0, A1, A2, A3, A8, A9, A10, A11 | 成功 | 入力状態を維持 | なし | `stop_idempotent` を増やす | 停止済み相当の状態で stop は冪等成功 |
| F-B-019 | `stop()` 未設定 | F0 | `INVALID_STATE` | F0 | なし | `stop_invalid_state` を増やす | 未設定では停止対象が存在しない |
| F-B-020 | `close()` | 全非閉鎖状態 | 表5に従う | 表5に従う | 後片付け開始 | 表5に従う | close の戻り値と後片付け完了判定は表5を正とする |
| F-B-021 | 閉鎖後の公開API | F15, F16 | `INVALID_STATE` | 入力状態を維持 | なし | `closed_access` を増やす | 閉鎖後は `close()` 以外の公開APIを成功させない |

#### 表1-C. IFilter 補助API状態契約

| No | API / 入力 | 対象状態集合 | AIDL戻り値 | 次状態関数 | 副作用 | 診断 | 同値性根拠 / 設計上の成立条件 |
|---:|---|---|---|---|---|---|---|
| F-C-001 | `getQueueDesc()` | F0 かつ open時フィルタ種別が通常FMQ対象、F1, F2, F3 | 成功 | 入力状態を維持 | 通常FMQ記述子を返す | `queue_desc_success` | `getQueueDesc()` の成否は configure 済みではなく通常FMQ有無で決める |
| F-C-002 | `getQueueDesc()` | F0 かつ open時フィルタ種別が通常FMQ非対象 | `UNAVAILABLE` | F0 | なし | `queue_desc_unavailable` を増やす | 未configureでも非FMQ対象は記述子を公開しない |
| F-C-003 | `getQueueDesc()` | F4, F5, F6, A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 | `UNAVAILABLE` | 入力状態を維持 | なし | `queue_desc_unavailable` を増やす | 通常FMQを公開しない状態として同値 |
| F-C-004 | `configureAvStreamType()` 正常入力 | A0, A1, A8, A9 | 成功 | 補助種別軸を設定済みに変更。他軸は維持 | stream type hint を指定値で保存する。TsAudio には Audio、TsVideo には Video だけを許可する | `av_stream_type_configured` | ハンドル未公開の非開始AV状態として同値。routing 種別は open subtype 由来であり、このAPIの有無に依存しない |
| F-C-005 | `configureAvStreamType()` 正常入力 | A2, A3, A10, A11 | 成功 | 補助種別軸を設定済みに変更。他軸は維持 | stream type hint を指定値で保存し、全`dataId`を無効化。TsAudio には Audio、TsVideo には Video だけを許可する | `av_generation` を進める | ハンドル公開済みの非開始AV状態として同値。旧`dataId`を使わせない。routing 種別は open subtype 由来である |
| F-C-006 | `configureAvStreamType()` 開始中 | A4, A5, A6, A7 | `INVALID_STATE` | 入力状態を維持 | なし | `av_stream_type_while_started` を増やす | 開始中の種別変更は禁止 |
| F-C-007 | `configureAvStreamType()` 未設定 | F0 | `INVALID_STATE` | F0 | なし | `av_stream_type_invalid_state` を増やす | 未設定ではAV補助APIの対象が存在しない |
| F-C-008 | `configureAvStreamType()` 非AV | F1, F2, F3, F4, F5, F6 | `UNAVAILABLE` | 入力状態を維持 | なし | `av_stream_type_unavailable` を増やす | 非AV状態は全て同値 |
| F-C-009 | `configureAvStreamType()` passthrough要求 | A0, A1, A2, A3, A8, A9, A10, A11 | `UNAVAILABLE` | 入力状態を維持 | なし | `unsupported_passthrough_configure` を増やす | 本製品では passthrough を恒久非対応とする |
| F-C-010 | `getAvSharedHandle()` 初回 | A0, A1, A4, A5, A8, A9 | 成功 | 共有ハンドル軸だけ公開済みに変更。他軸は維持 | shared backing を生成しハンドルを返す | `av_shared_memory_create` を増やす | 種別軸と実行状態軸を維持し、ハンドル軸だけ変更する |
| F-C-011 | `getAvSharedHandle()` 再取得 | A2, A3, A6, A7, A10, A11 | 成功 | 入力状態を維持 | 既存ハンドルを返す | `av_shared_handle_reuse` を増やす | 再取得は冪等成功 |
| F-C-012 | `getAvSharedHandle()` 未設定 | F0 | `INVALID_STATE` | F0 | なし | `av_handle_invalid_state` を増やす | 未設定ではAV共有ハンドル対象が存在しない |
| F-C-013 | `getAvSharedHandle()` 非AV | F1, F2, F3, F4, F5, F6 | `UNAVAILABLE` | 入力状態を維持 | なし | `av_handle_unavailable` を増やす | 非AV状態は全て同値 |
| F-C-014 | `releaseAvHandle()` | 全状態 | 表1-C-AVHに従う | 表1-C-AVHに従う | 表1-C-AVHに従う | 表1-C-AVHに従う | `releaseAvHandle()` は `avDataId` 符号、fd付き `avMemory`、closed、filter種別、shared handle公開状態、active/stale `dataId` の優先順位を表1-C-AVHで一意に固定する |
| F-C-020 | `flush()` FMQ対象 | F1, F2, F3 | 成功 | 入力状態を維持 | FMQ未消費データと一過性状態を破棄 | `filter_flush_success` | FMQ対象状態は flush に関して同値 |
| F-C-021 | `flush()` ペイロードなし | F4, F5, F6 | 成功 | 入力状態を維持 | 一過性状態を破棄 | `filter_flush_success` | ペイロードなし状態は flush に関して同値 |
| F-C-022 | `flush()` AVハンドル未公開 | A0, A1, A4, A5, A8, A9 | 成功 | 入力状態を維持 | 一過性状態を破棄 | `filter_flush_success` | ハンドル未公開AV状態では共有ハンドル資源を触らない |
| F-C-023 | `flush()` AVハンドル公開済み | A2, A3, A6, A7, A10, A11 | 成功 | 入力状態を維持 | 使用中領域と全`dataId`を破棄し、公開済みハンドルと shared backing は維持 | `filter_flush_success` | flush は全解放と異なり、共有ハンドル公開状態を維持する |
| F-C-024 | `flush()` 未設定 | F0 | `INVALID_STATE` | F0 | なし | `filter_flush_invalid_state` を増やす | 未設定では破棄対象が存在しない |
| F-C-025 | `configureMonitorEvent(0)` | F0, F1, F2, F3, F4, F5, F6, A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 | 成功 | 入力状態を維持 | なし | `monitor_event_mask_zero` を増やす | mask 0 は無処理成功で同値 |
| F-C-026 | `configureMonitorEvent(nonzero)` | F0, F1, F2, F3, F4, F5, F6, A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 | profile 非対応では `UNAVAILABLE`、profile 対応では成功 | 入力状態を維持 | profile 対応では要求 mask を保存し monitor event 配送対象にする | `monitor_event_unavailable` または `monitor_event_configured` を増やす | VTS/profile で `monitorEventTypes > 0` を使う場合は成功と event 配送を必須とする |
| F-C-027 | `configureIpCid()` | F0, F1, F2, F3, F4, F5, F6, A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 | `UNAVAILABLE` | 入力状態を維持 | なし | `ip_cid_unavailable` を増やす | IP CID は Tuner HAL の視聴経路 / capability 対象外 |
| F-C-028 | `setDelayHint()` 正常入力 | F0, F1, F2, F3, F4, F5, F6, A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 | 成功 | 入力状態を維持 | hint 値だけ保存 | `delay_hint_set` | 資源寿命を変えない |
| F-C-029 | `setDelayHint()` 不正入力 | F0, F1, F2, F3, F4, F5, F6, A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 | `INVALID_ARGUMENT` | 入力状態を維持 | なし | `delay_hint_invalid` を増やす | 不正入力は全非閉鎖状態で同じ拒否 |
| F-C-030 | `getId()` / `getId64Bit()` | F0, F1, F2, F3, F4, F5, F6, A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 | 成功 | 入力状態を維持 | IDを返す | なし | 読み取り専用APIで資源寿命を変えない |
| F-C-031 | `setDataSource()` 成功組み合わせ | 表1-Dで成功と定義した組み合わせ | 成功 | 入力状態を維持 | source 参照を保持 | `set_data_source_success` | 詳細は表1-Dを正とする |
| F-C-032 | `setDataSource()` 拒否組み合わせ | 表1-Dで拒否と定義した組み合わせ | 表1-Dに従う | 入力状態を維持 | なし | 表1-Dに従う | 詳細は表1-Dを正とする |

##### 表1-C-AVH. `releaseAvHandle()` 判定優先順位表

`releaseAvHandle(avMemory, avDataId)` は次の優先順位で判定する。共有ハンドル経路では、`DemuxFilterMediaEvent.avMemory` は empty handle とし、個別 AV 領域の解放は empty handle + 非0 `avDataId` で扱う。`getAvSharedHandle()` が返した fd 付き handle 本体は、`releaseAvHandle(fd付き handle, 0)` による client 側 shared AV handle 使用終了通知として受理する。

`getAvSharedHandle()` が返す `NativeHandle` は fd 1個と `ints[0]=0` を持つ。`ints[0]` は Android framework/JNI が memory index として参照するため、HAL 内部識別子として使わない。`slot_size`、`slot_count`、magic、generation、filter id は `NativeHandle.ints` に公開しない。fd 付き handle の同一性確認では fd 番号そのものを比較してはならない。client が dup した fd を返す可能性があるため、fd番号一致を契約条件にしない。

| No | 優先 | 状態 | 条件 | AIDL戻り値 | 資源変化 | 診断 | 設計上の成立条件 |
|---:|---:|---|---|---|---|---|---|
| AVH-001 | 1 | any | `avDataId < 0` | `INVALID_ARGUMENT` | なし | `av_data_id_invalid_release` | 負の `avDataId` が他条件より先に拒否される |
| AVH-002 | 2 | open AV | fd付き `avMemory` + `avDataId == 0` | 成功 | slot解放なし。client release済みにする | `av_handle_client_release` | VTS互換の shared AV handle 使用終了通知として受理する。fd番号一致を要求しない |
| AVH-003 | 3 | closed | fd付き `avMemory` + `avDataId == 0` | 成功扱いの無処理 | なし | `av_handle_client_release_after_close` | close後に遅れて届いた shared handle release が状態を壊さない |
| AVH-004 | 4 | any | fd付き `avMemory` + `avDataId > 0` | `INVALID_ARGUMENT` | なし | `av_handle_slot_release_invalid` | fd付きhandleはslot releaseには使わない |
| AVH-C01 | 5 | closed | empty `avMemory` + `avDataId >= 0` | 成功 | なし | `av_data_id_stale_release_after_close` | close後に遅れて届いたAV release通知が状態を壊さない |
| AVH-S01 | 6 | open | empty `avMemory` + `avDataId > 0` + current filter が AV ではない + 過去に AV shared handle 公開済み | 成功 | なし | `av_data_id_stale_release` | configure 後に非AVへ再設定されても、旧AV MediaEventの遅延releaseが状態を壊さない |
| AVH-S02 | 7 | open | empty `avMemory` + `avDataId > 0` + current filter が AV ではない + AV shared handle 公開履歴なし | `UNAVAILABLE` | なし | `av_handle_unavailable` | 非AV filterで新規 release が成功しない |
| AVH-S03 | 8 | open AV | empty `avMemory` + `avDataId > 0` + `getAvSharedHandle()` 未実行 | `INVALID_STATE` | なし | `av_handle_release_without_handle` | shared handle未公開で個別slot release が成功しない |
| AVH-S04 | 9 | open AV または未公開 | empty `avMemory` + `avDataId == 0` | 成功。ただし export済みかつ二重通知なら `INVALID_ARGUMENT` | slot解放なし。shared handle export済みかつ client release未済みなら client release済みにする。export未済みなら既存互換 無処理 | `av_handle_client_release` またはなし | `avDataId=0` は利用者側 AV handle 使用終了通知であり、全解放にならない。export未済みなら 無処理 成功。二重通知は拒否する |
| AVH-S05 | 10 | open AV | empty `avMemory` + active `avDataId > 0` | 成功 | 指定slotを解放 | `av_data_id_release` | 指定slotだけ解放される |
| AVH-S06 | 11 | open AV | empty `avMemory` + stale `avDataId > 0` | 成功 | なし | `av_data_id_stale_release` | configure / configureAvStreamType / flush 後に遅れて届いた旧 `avDataId` release が状態を壊さない |

衝突入力の優先順位は次で固定する。

- 任意状態 + 任意 `avMemory` + `avDataId < 0` は AVH-001 とする。
- open AV + fd付き `avMemory` + `avDataId == 0` は AVH-002 とする。
- closed + fd付き `avMemory` + `avDataId == 0` は AVH-003 とする。
- fd付き `avMemory` + `avDataId > 0` は AVH-004 とする。
- empty `avMemory` + `avDataId == 0` は、filter種別や export 有無にかかわらず AVH-S04 とする。ただし export済みかつ client release済みの場合は二重通知として `INVALID_ARGUMENT` とする。
- open non-AV filter + empty `avMemory` + `avDataId > 0` + 過去に AV shared handle 公開済み は AVH-S01 とする。
- open non-AV filter + empty `avMemory` + `avDataId > 0` + AV shared handle 公開履歴なし は AVH-S02 とする。

受け入れ条件は次で固定する。

- `releaseAvHandle()` は shared-mode empty `avMemory` を拒否しない。
- `releaseAvHandle(returnedSharedHandle, 0)` は成功する。
- fd付き handle の二重 release は、client release済みであれば `INVALID_ARGUMENT` にしてよい。
- `releaseAvHandle(fd付き avMemory, dataId > 0)` は `INVALID_ARGUMENT` になる。
- `releaseAvHandle(fd付き avMemory, dataId < 0)` は `INVALID_ARGUMENT` になり、診断は `av_data_id_invalid_release` になる。
- `releaseAvHandle(fd付き avMemory, 0)` は fd番号一致を要求せずに shared handle release 通知として受理する。
- `releaseAvHandle(empty, 0)` は、export済みなら client release済みにし、slot を解放しない。
- `releaseAvHandle(empty, 0)` は、export未済みなら 無処理 成功する。
- `releaseAvHandle(empty, 0)` は、export済みかつ client release済みなら二重通知として `INVALID_ARGUMENT` になる。
- `releaseAvHandle(empty, active dataId)` は成功し、指定slotを解放する。
- `releaseAvHandle(empty, stale dataId)` は成功扱いの無処理 になる。
- `releaseAvHandle(empty, negative dataId)` は `INVALID_ARGUMENT` になる。
- `configureAvStreamType()` 後の旧 `avDataId` release は成功扱いの無処理 になる。
- `configure()` で非AV設定へ再設定した後の旧AV `avDataId` release は成功扱いの無処理 になる。
- `close()` 後の `releaseAvHandle(empty/fd付き, avDataId >= 0)` は成功扱いの無処理 になる。
- `close()` 後の `releaseAvHandle(empty/fd付き, negative avDataId)` は `INVALID_ARGUMENT` になる。


#### 表1-D. `setDataSource()` 互換表

`setDataSource()` は sink 側公開APIである。実装は、表1-D-1の判定順序を先に適用し、通常の source / sink 種別互換は表1-D-3の行列で判定する。

##### 表1-D-1. `setDataSource()` 判定順序表

| 優先 | 条件 | AIDL戻り値 | 次状態 | 固定理由 |
|---:|---|---|---|---|
| 1 | sink が閉鎖済み | `INVALID_STATE` | sink 状態を維持 | `setDataSource()` は sink 側公開APIであり、sink 自身の閉鎖状態を最優先で判定する |
| 2 | sink が実行時失敗状態 | `INVALID_STATE` | sink 状態を維持 | fail-closed 状態の filter は再配線しない |
| 3 | sink が開始中 | `INVALID_STATE` | sink 状態を維持 | 開始中に入力元参照を変更しない |
| 4 | source が `NULL` | 現行 Android 14 AIDL Rust 生成境界では到達不能 | sink 状態を維持 | AOSP意味論では demux input 復帰だが、Android 14 AIDL Rust 生成 trait で null を受け取る経路が未固定であるため、現行実装済み対象に含めない |
| 5 | source と sink が同一 object | `INVALID_ARGUMENT` | sink 状態を維持 | 自己参照を禁止する |
| 6 | source が閉鎖済みまたは実行時失敗状態 | `INVALID_STATE` | sink 状態を維持 | source の lifecycle 異常であり、引数形式不正として扱わない |
| 7 | source が別 demux 所属 | `INVALID_ARGUMENT` | sink 状態を維持 | demux 境界をまたいだ接続を禁止する |
| 8 | 上記に該当しない | 表1-D-3に従う | 表1-D-3に従う | 通常の種別互換判定を行う |

source は非閉鎖かつ非実行時失敗であれば、設定済み、開始済み、停止済みのいずれでも入力元として利用可能とする。sink は非閉鎖、非実行時失敗、かつ非開始の状態だけ、表1-D-3の互換判定へ進む。source が `NULL` の場合は AOSP契約上 filter object ではないため、自己参照、source閉鎖、別demux所属の判定対象にしない。ただし 現行 Android 14 AIDL Rust 生成境界では public HAL method で null を受け取れないため、実装済み対象に含めない。

##### 表1-D-2. `setDataSource()` endpoint分類表

| 分類名 | 含むもの | 通常FMQ payload | AV共有メモリ | 備考 |
|---|---|---:|---:|---|
| demux input | source が `NULL` の場合のAOSP契約上の標準入力元 | 対象sinkに従う | 対象sinkに従う | filter object ではない。現行 Android 14 AIDL Rust 生成境界では実装済み対象に含めない |
| section フィルタ | section payload を出す FMQ対象フィルタ | あり | なし | source にはしない。SourceFilter 経由の section sink としても扱わない |
| PES フィルタ | PES payload を出す FMQ対象フィルタ | あり | なし | source にはしない。SourceFilter 経由の PES sink としても扱わない |
| TS生データフィルタ | TS raw payload を出す FMQ対象フィルタ | あり | なし | `SourceFilter` 経由で再投入できる唯一の source 種別。下流として成功させるのは TS生データフィルタと record フィルタだけである |
| AV フィルタ | live audio / video フィルタ | なし | あり | source にはしない。SourceFilter 経由の AV sink としても扱わない |
| record フィルタ | DVR record buffer と `TsRecordEvent` 用の終端フィルタ | あり | なし | source にはしない。SourceFilter 経由では record sink としてだけ扱う |
| ペイロードなしフィルタ | PCR、監視、状態通知専用フィルタ | なし | なし | source / sink にしない |

##### 表1-D-3. `setDataSource()` 通常組み合わせ行列

この行列は、表1-D-1の優先1〜7を通過した場合だけ適用する。つまり、sink は非閉鎖かつ非開始、source は非閉鎖、同一 demux 所属、source と sink は別 object である。source が `NULL` の場合は AOSP契約上は優先4の対象であり、この行列には入らない。

| source \ sink | section フィルタ | PES フィルタ | TS生データフィルタ | AV フィルタ | record フィルタ | ペイロードなしフィルタ |
|---|---|---|---|---|---|---|
| section フィルタ | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` |
| PES フィルタ | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` |
| TS生データフィルタ | `UNAVAILABLE` | `UNAVAILABLE` | 成功 | `UNAVAILABLE` | 成功 | `INVALID_ARGUMENT` |
| AV フィルタ | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` |
| record フィルタ | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` |
| ペイロードなしフィルタ | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` |

表1-D-3 に条件付き成功セルは置かない。source filter として指定できるのは TS生データフィルタだけである。下流として成功させるのは TS生データフィルタと record フィルタだけである。section / PES / AV への raw TS 再parse chain、および section payload、PES payload、AV payload、record payload を直接 source として再配送する経路は作らない。非対応の linkage は `UNAVAILABLE` とする。source と sink の `tpid` が一致しない場合は、対応 linkage の場合だけ `INVALID_ARGUMENT` とする。record フィルタと AV フィルタは終端sinkであり、他 filter の source にはしない。

##### 表1-D-4. `setDataSource()` 行列セルの副作用

| 行列結果 | AIDL戻り値 | 次状態 | 副作用 | 診断 | 設計上の成立条件 |
|---|---|---|---|---|---|
| demux input 復帰 | 成功 | sink 状態を維持 | AOSP契約に従い既存 source 参照を解除し、sink の入力元を demux input に戻す | `set_data_source_demux_input` | source が `NULL` で、sink が非閉鎖かつ非開始である |
| 成功 | 成功 | sink 状態を維持 | sink が source 参照を保持する。登録済み source がある場合は新しい source 参照で置換する | `set_data_source_success` | source / sink の組み合わせが表1-D-3の成功セルに一致する |
| `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | sink 状態を維持 | source 参照を変更しない | `set_data_source_invalid_pair` を増やす | source / sink の組み合わせが表1-D-3の拒否セルに一致する |


### Filter data source の source lifecycle エラー

`setDataSource()` の source filter が closed または runtime failed の場合、HAL は lifecycle 異常として `INVALID_STATE` を返す。存在しない filter、別 demux の filter、local HAL filter ではない object、source と sink の自己参照、種別不一致、PID不一致は引数不正として `INVALID_ARGUMENT` を返す。source lifecycle 異常を `INVALID_ARGUMENT` に丸めてはならない。


### 表2. IDvr 状態表

#### 表2-A. IDvr 状態コード

| 状態コード | 状態名 | 意味 |
|---|---|---|
| D0R | 録画DVR未設定 | `openDvr(record)` 後、`configure()` 未完了 |
| D0P | 再生DVR未設定 | `openDvr(playback)` 後、`configure()` 未完了 |
| D1 | 録画設定済み | record DVR が configure 済み |
| D2 | 録画開始済み | record DVR が start 済み |
| D3 | 録画停止済み | record DVR が stop 済み |
| D4 | 再生設定済み | playback DVR が configure 済み |
| D5 | 再生開始済み | playback DVR が start 済み |
| D6 | 再生停止済み | playback DVR が stop 済み |
| D7 | 閉鎖済み | `close()` 後片付け完了済み |
| D8 | 異常時閉鎖済み | DVR作業スレッド致命停止、FMQ致命失敗、後片付け未完などで公開APIを遮断した状態 |

#### 表2-B. IDvr API別状態契約

| No | API / 入力 | 対象状態集合 | AIDL戻り値 | 次状態関数 | 副作用 | 診断 | 同値性根拠 / 設計上の成立条件 |
|---:|---|---|---|---|---|---|---|
| DVR-001 | `configure(record settings)` | D0R | 成功 | D1 | 録画DVR queue を設定 | `dvr_configure_success` | DVR種別と settings 種別が一致 |
| DVR-002 | `configure(playback settings)` | D0P | 成功 | D4 | 再生DVR queue を設定 | `dvr_configure_success` | DVR種別と settings 種別が一致 |
| DVR-003 | `configure()` 種別不一致 | D0R, D1, D3, D0P, D4, D6 | `INVALID_ARGUMENT` | 入力状態を維持 | なし | `dvr_configure_kind_mismatch` を増やす | 対象は record DVR への playback settings と playback DVR への record settings とする |
| DVR-004 | `configure()` 同一DVR種別の非開始再設定 | D1, D3, D4, D6 | 成功 | record DVR は D1、playback DVR は D4 | DVR queue世代を更新 | `dvr_reconfigure_success` | 同一DVR種別の非開始再設定として同値 |
| DVR-005 | `configure()` 開始中 | D2, D5 | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_configure_while_started` を増やす | 開始中再設定を禁止 |
| DVR-006 | `getQueueDesc()` | D1, D2, D3, D4, D5, D6 | 成功 | 入力状態を維持 | DVR FMQ記述子を返す | `dvr_queue_desc_success` | configured DVR は種別に関係なく記述子を持つ |
| DVR-007 | `getQueueDesc()` | D0R, D0P | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_queue_desc_invalid_state` を増やす | 未設定DVRは記述子を公開しない |
| DVR-008 | `start()` record / record filter attach 済み | D1, D3 | 成功 | D2 | 録画作業スレッドを開始 | `dvr_start_success` | record DVR は attached record filter を入力源として録画を開始する |
| DVR-008a | `start()` record / record filter 未attach | D1, D3 | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_start_missing_record_filter` を増やす | AOSP の録画DVR実用フローは record filter attach 後の start であり、入力源なしの record DVR start を成功扱いしない |
| DVR-009 | `start()` playback | D4, D6 | 成功 | D5 | 再生入力受付を開始 | `dvr_start_success` | playback DVR の非開始状態は start に関して同値 |
| DVR-010 | `start()` 開始済み | D2, D5 | 成功 | 入力状態を維持 | なし | `dvr_start_idempotent` を増やす | 重複 start は冪等成功 |
| DVR-011 | `start()` 未設定 | D0R, D0P | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_start_invalid_state` を増やす | 未設定DVRでは開始対象が存在しない |
| DVR-012 | `stop()` record | D2 | 成功 | D3 | 録画作業スレッドを停止 | `dvr_stop_success` | record開始済みを停止済みにする |
| DVR-013 | `stop()` playback | D5 | 成功 | D6 | 再生入力受付を停止 | `dvr_stop_success` | playback開始済みを停止済みにする |
| DVR-014 | `stop()` 設定済み非開始 | D1, D3, D4, D6 | 成功 | 入力状態を維持 | なし | `dvr_stop_idempotent` を増やす | 非開始設定済み状態で stop は冪等成功 |
| DVR-015 | `stop()` 未設定 | D0R, D0P | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_stop_invalid_state` を増やす | 未設定DVRでは停止対象が存在しない |
| DVR-016 | `flush()` configured DVR | D1, D2, D3, D4, D5, D6 | 成功 | 入力状態を維持 | DVR queue と一過性状態を破棄 | `dvr_flush_success` | record/playback とも queue と一過性状態を破棄する点で同値 |
| DVR-017 | `flush()` 未設定 | D0R, D0P | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_flush_invalid_state` を増やす | 未設定DVRでは破棄対象が存在しない |
| DVR-018 | `read()` on record DVR | D1, D2, D3 | 成功 | 入力状態を維持 | DVR record queue から読み出す | `dvr_read_success` | queue状態に応じた読み出し結果を返す |
| DVR-019 | `read()` on unconfigured record DVR | D0R | `INVALID_STATE` | D0R | なし | `dvr_read_invalid_state` を増やす | 未設定record DVRでは読み出し対象queueが存在しない |
| DVR-020 | `read()` on playback DVR | D0P, D4, D5, D6 | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_read_wrong_kind` を増やす | playback DVR は read 対象外 |
| DVR-021 | `write()` on playback DVR | D4, D5, D6 | 成功 | 入力状態を維持 | 入力可能分を書き込む。空き不足時は内部結果を 0 byte written とする | `dvr_write_success` | AIDL戻り値は成功に固定し、入力抑制は内部結果で表現 |
| DVR-022 | `write()` on unconfigured playback DVR | D0P | `INVALID_STATE` | D0P | なし | `dvr_write_invalid_state` を増やす | 未設定playback DVRでは入力queueが存在しない |
| DVR-023 | `write()` on record DVR | D0R, D1, D2, D3 | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_write_wrong_kind` を増やす | record DVR は write 対象外 |
| DVR-024 | `attachFilter()` valid filter | D1, D2, D3 | 成功 | 入力状態を維持 | 未登録なら登録する | `dvr_attach_filter_success` | record DVR だけ filter attach を受ける |
| DVR-025 | `attachFilter()` 同一filter重複 | D1, D2, D3 | 成功 | 入力状態を維持 | 登録数を増やさない | `dvr_attach_filter_idempotent` を増やす | 重複attachは冪等成功 |
| DVR-026 | `attachFilter()` 未設定record DVR | D0R | `INVALID_STATE` | D0R | なし | `dvr_attach_invalid_state` を増やす | 未設定record DVRでは attach 対象queueが存在しない |
| DVR-027 | `attachFilter()` playback DVR | D0P, D4, D5, D6 | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_attach_wrong_kind` を増やす | playback DVR では attach しない |
| DVR-028 | `attachFilter()` 不正filter | D1, D2, D3 | `INVALID_ARGUMENT` | 入力状態を維持 | なし | `dvr_attach_invalid_filter` を増やす | 閉鎖済み、別demux、録画非対応filterを attach しない |
| DVR-029 | `detachFilter()` 登録済みfilter | D1, D2, D3 | 成功 | 入力状態を維持 | 登録を解除する | `dvr_detach_filter_success` | record DVR だけ filter detach を受ける |
| DVR-030 | `detachFilter()` 未登録filter | D1, D2, D3 | 成功 | 入力状態を維持 | なし | `dvr_detach_filter_idempotent` を増やす | 未登録 detach は冪等成功 |
| DVR-031 | `detachFilter()` 未設定record DVR | D0R | `INVALID_STATE` | D0R | なし | `dvr_detach_invalid_state` を増やす | 未設定record DVRでは detach 対象登録が存在しない |
| DVR-032 | `detachFilter()` playback DVR | D0P, D4, D5, D6 | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_detach_wrong_kind` を増やす | playback DVR では detach しない |
| DVR-033 | `setStatusCheckIntervalHint()` 正常入力 | D0R, D0P, D1, D2, D3, D4, D5, D6 | 成功 | 入力状態を維持 | hint 値だけ保存 | `dvr_status_hint_set` | 資源寿命を変えない |
| DVR-034 | `setStatusCheckIntervalHint()` 不正入力 | D0R, D0P, D1, D2, D3, D4, D5, D6 | `INVALID_ARGUMENT` | 入力状態を維持 | なし | `dvr_status_hint_invalid` を増やす | 不正入力は全非閉鎖状態で同じ拒否 |
| DVR-035 | `close()` | 全非閉鎖状態 | 表5に従う | 表5に従う | 後片付け開始 | 表5に従う | close の戻り値と後片付け完了判定は表5を正とする |
| DVR-036 | 閉鎖後の公開API | D7, D8 | `INVALID_STATE` | 入力状態を維持 | なし | `dvr_closed_access` を増やす | 閉鎖後は `close()` 以外の公開APIを成功させない |

### 表3. フィルタ種別別データ経路表

configure 非受理後は IFilter 状態が F0 のままである。その後に `getQueueDesc()` が呼ばれた場合は open時フィルタ種別の通常FMQ有無に従い、`start()`、`flush()` 等が呼ばれた場合は表1の F0 行に従う。

| No | フィルタ種別 / 要求 | 本製品での扱い | capability / VTS profile | configure時 / 専用API戻り値 | 後続公開APIの扱い | ペイロード配送 | 固定根拠 |
|---:|---|---|---|---|---|---|---|
| DP-001 | section | 受理 | 宣言する | 成功 | 表1の FMQ対象状態に従う | 通常FMQ | PSI/SI section 取得に必要 |
| DP-002 | PES | 受理 | 宣言する | 成功 | 表1の FMQ対象状態に従う | 通常FMQ | 字幕、音声補助、検査用途に必要 |
| DP-003 | TS生データ | 受理 | 宣言する | 成功 | 表1の FMQ対象状態に従う | 通常FMQ | lab / raw TS 検査用 |
| DP-004 | record filter | 受理 | 宣言する | 成功 | 表1の FMQ対象状態に従う | 通常FMQ、DVR record mirror | 1サービスTS録画の入力 |
| DP-005 | live AV audio/video non-passthrough | 受理 | AV filter と共有メモリ経路を宣言する。通常FMQからのAVペイロード読み出しを VTS profile に入れない | 成功 | 表1の AV状態に従う | `MediaEvent` + 共有メモリ + `dataId` | 本製品のライブAV正式経路 |
| DP-006 | AV passthrough | 恒久非対応 | 宣言しない | `UNAVAILABLE` | 状態は未設定のまま。後続APIは F0 に従う | なし | 本製品では対応しない |
| DP-007 | PCR / AV同期用情報 | 内部状態として受理 | payload queue として宣言しない | 成功 | 表1のペイロードなし状態に従う | ペイロードなし。AV同期内部状態へ反映 | PCRを通常FMQへ出さない |
| DP-008 | 監視 / 状態通知専用 | profile依存 | 非対応profileでは監視イベント配送を宣言しない。対応profileでは monitor event capability とVTS configを一致させる | 非対応profileでは `configureMonitorEvent(0)` は成功、非0は `UNAVAILABLE`。対応profileでは非0も成功 | start は状態遷移だけ成功。対応profileでは要求maskに応じてmonitor eventを配送 | monitor event | VTS/profileで `monitorEventTypes > 0` を使う場合だけ正式対応対象にする |
| DP-009 | MMTP / TLV / ALP | Tuner HAL capability / VTS profile 対象外 | 宣言しない | `UNAVAILABLE` | 状態は未設定のまま。後続APIは F0 に従う | なし | 製品全体の入力方式スコープは `開発規則.md` を正とし、本書では Tuner HAL の返却値だけを固定する |
| DP-010 | IP CID | Tuner HAL capability / VTS profile 対象外 | 宣言しない | `configureIpCid()` は `UNAVAILABLE` | 入力状態を維持 | なし | IP filter を Tuner HAL の視聴経路に含めない |


#### raw section / raw PES event 生成契約

raw section filter は、FMQ data に加えて `DemuxFilterEvent::Section` を生成する。section header を parse できない raw payload でも、raw delivery の event 自体を欠落させてはならない。parse 不能時の table metadata は意味値として扱わず、raw payload 到着を示す event としてだけ扱う。

raw PES filter は、FMQ data に加えて `DemuxFilterEvent::Pes` を生成する。PES header を parse できない raw payload でも、raw delivery の event 自体を欠落させてはならない。parse 不能時の stream id は意味値ではなく、raw PES payload 到着を示す event として扱う。

### raw section / raw PES event の metadata

raw section / raw PES event では、payload arrival を通知することを主目的とする。parse不能時に補助的に設定する `tableId=0`、`version=0`、`sectionNum=0`、`PES_STREAM_ID_UNKNOWN` は意味値ではない。

上位は、これらの値を section / PES 意味解析の成功を示す値として使ってはならない。意味解析が必要な場合は、FMQ payload本体を上位で再解析する。


### 表4. AV共有メモリ資源寿命表

shared backing は表4の資源寿命列で管理する。`releaseAvHandle(dataId=0)` は client 側 AV handle 使用終了通知として扱い、shared backing、公開済みハンドル、既存`dataId`、使用中領域を破棄しない。ただし client release済みの間は新しい AV payload を `MediaEvent` として出さず、`getAvSharedHandle()` 再取得で client release未済みに戻してから配送を再開する。

#### 表4-A. AV共有メモリ容量固定表

AV共有メモリの slot size は filter `bufferSize` から算出してはならない。`bufferSize` は通常FMQ対象フィルタの queue 容量であり、AV共有メモリの単位領域サイズとは別定数にする。

| 項目 | 固定内容 |
|---|---|
| slot size | `AV_SHARED_SLOT_SIZE_BYTES` という製品定数で固定する |
| slot count | `AV_SHARED_SLOT_COUNT` という製品定数で固定する |
| `bufferSize` との関係 | filter `bufferSize` を AV slot size に流用しない |
| oversized AV payload | slot size を超える AV payload は配送せず、`av_payload_oversize_drop` を増やす |
| MediaEvent 発行条件 | payload が slot に収まり、共有ハンドル公開済み、client release未済みで、有効な `dataId` を発行できる場合だけ発行する |
| VTS/profile 条件 | AVペイロードの通常FMQ読み出しを前提にしない |

#### 表4-B. AV共有メモリ資源寿命表

| No | 操作 / 事象 | 対象状態集合 | AIDL戻り値 | shared backing | 公開済みハンドル | 使用中領域 | `dataId` | 一過性状態 | 累積カウンタ | 新規配送可否 | 次状態関数 | 設計上の成立条件 | 同値性根拠 |
|---:|---|---|---|---|---|---|---|---|---|---|---|---|---|
| AVM-001 | `configure(AV)` | F0 | 成功 | 未生成 | 未公開 | なし | 未発行 | `configureAvStreamType()` hint を消去し、routing 種別を open subtype から導出 | `av_generation` を進める | 不可 | A0 | configure 境界で旧AV資源が残らないこと。TsAudio/TsVideo は hint 未設定でも routing 可能であること | AV初期状態を一意にする |
| AVM-002 | `configureAvStreamType()` | A0, A1, A8, A9 | 成功 | 未生成 | 未公開 | なし | 未発行 | stream type hint を保存 | 変化なし | 不可 | 補助種別軸を設定済みに変更。他軸は維持 | stream type hint が指定値で保存されること。routing 種別は open subtype と一致していること | ハンドル未公開の非開始AV状態として同値 |
| AVM-003 | `configureAvStreamType()` | A2, A3, A10, A11 | 成功 | 維持 | 公開済み | 全破棄 | 全無効化 | stream type hint を保存 | `av_generation` を進める | 不可 | 補助種別軸を設定済みに変更。他軸は維持 | 旧`dataId`が使えないこと。routing 種別は open subtype と一致していること | ハンドル公開済みの非開始AV状態として同値 |
| AVM-004 | `getAvSharedHandle()` 初回 | A0, A1, A4, A5, A8, A9 | 成功 | 未生成なら生成 | 公開済み | なし | 未発行 | なし | `av_shared_memory_create` を増やす | 開始済み状態だけ可 | ハンドル軸だけ公開済みに変更。他軸は維持 | 初回取得でハンドルが返ること | ハンドル未公開状態として同値 |
| AVM-005 | `getAvSharedHandle()` 再取得 | A2, A3, A6, A7, A10, A11 | 成功 | 維持 | 公開済み | 維持 | 維持 | client release済みなら未済みに戻す | `av_shared_handle_reuse` を増やす | 開始済み状態だけ可 | 入力状態を維持 | 再取得で既存資源を維持し、client release 後の配送を再開可能にすること | 再取得は配送再開の合図として扱う |
| AVM-006 | `start()` | A0, A1, A2, A3, A8, A9, A10, A11 | 成功 | 入力状態に従う | 入力状態のハンドル軸に従う | 入力状態に従う | 入力状態に従う | なし | `av_start` を増やす | ハンドル公開済み状態だけ可 | 実行状態軸だけ開始済みに変更。他軸は維持 | ハンドル未公開中にAVペイロードを配送しないこと | 戻り値、診断、状態軸変換規則、資源寿命が同一 |
| AVM-007 | AV payload 到着 | A4, A5 | 公開APIなし | 維持 | 未公開 | 作らない | 発行しない | drop状態更新 | `av_before_export_drop` を増やす | 不可 | 入力状態を維持 | ハンドル未公開中に MediaEvent を出さないこと | ハンドル未公開開始済み状態は同値 |
| AVM-008 | AV payload 到着 | A6, A7 + client release未済み | 公開APIなし | 維持 | 公開済み | 割当 | 発行 | MediaEvent 生成 | `av_delivered` を増やす | 可 | 入力状態を維持 | `dataId` と共有メモリ領域が対応すること | ハンドル公開済み開始済みかつ client release未済み状態は同値 |
| AVM-008B | AV payload 到着 | A6, A7 + client release済み | 公開APIなし | 維持 | 公開済み | 作らない | 発行しない | drop状態更新 | `av_shared_handle_client_released_drop` を増やす | 不可 | 入力状態を維持 | 利用者側使用終了後に MediaEvent を出さないこと | 再取得されるまで配送しない |
| AVM-009 | `releaseAvHandle(empty, dataId=0)` | A2, A3, A6, A7, A10, A11 | 成功。ただし client release済みなら `INVALID_ARGUMENT` | 維持 | 公開済み | 維持 | 維持 | client release済みにする | `av_handle_client_release` を増やす | 不可。再取得後だけ再開可 | 入力状態を維持 | dataId、shared backing、使用中領域を破棄せず、利用者側使用終了だけを記録すること | empty handle の 0 通知だけを lifetime 通知として扱う |
| AVM-010 | `releaseAvHandle(active dataId)` | A2, A3, A6, A7, A10, A11 | 成功 | 維持 | 公開済み | 指定領域だけ破棄 | 指定`dataId`無効化 | なし | `av_data_id_release` を増やす | 開始済み状態だけ可 | 入力状態を維持 | 指定領域だけが解放されること | 部分解放は状態軸を変えない |
| AVM-011 | `releaseAvHandle(stale dataId)` | A2, A3, A6, A7, A10, A11 | 成功 | 維持 | 公開済み | 維持 | 維持 | なし | `av_data_id_stale_release` を増やす | 入力状態に従う | 入力状態を維持 | stale `dataId` release が状態を壊さないこと | AOSP framework/JNI の finalize 遅延を吸収するため |
| AVM-012 | `flush()` | A0, A1, A4, A5, A8, A9 | 成功 | 未生成 | 未公開 | なし | 未発行 | 消去 | 累積値維持 | 入力状態に従う | 入力状態を維持 | ハンドル未取得で flush が失敗しないこと | ハンドル未公開AV状態は同値 |
| AVM-013 | `flush()` | A2, A3, A6, A7, A10, A11 | 成功 | 維持 | 公開済み | 全破棄 | 全無効化 | 消去 | 累積値維持 | 入力状態に従う | 入力状態を維持 | 公開済みハンドルを維持し、active slot だけ破棄すること | ハンドル公開済みAV状態は同値 |
| AVM-014 | `stop()` | A4, A5, A6, A7 | 成功 | 維持 | 入力状態のハンドル軸に従う | 維持 | 維持 | なし | `av_stop` を増やす | 不可 | 実行状態軸だけ停止済みに変更。他軸は維持 | 停止しても既存`dataId`は release / flush / close まで維持 | 戻り値、診断、状態軸変換規則、資源寿命が同一 |
| AVM-015 | `close()` | 全AV状態 | 表5に従う | 解放 | 無効化 | 全破棄 | 全無効化 | 消去 | close診断へ反映 | 不可 | 表5に従う | close後にAV資源が残らないこと | close は表5を正とする |


### AV shared handle 入出力契約

`getAvSharedHandle()` は、AV shared memory を表す fd付き `NativeHandle` と共有メモリ総サイズを返す。client は、共有ハンドル使用終了時に、`getAvSharedHandle()` で受け取った fd付き `NativeHandle` を `releaseAvHandle(avMemory, 0)` に渡してよい。

`releaseAvHandle()` の正規入力は次に固定する。

| 入力 | 結果 | 意味 |
|---|---|---|
| fd付き handle + `avDataId == 0` | 成功 | client側 shared AV handle 使用終了通知 |
| empty handle + `avDataId == 0` | 成功 | fdを返せないclient経路の shared AV handle 使用終了通知 |
| empty handle + active `avDataId > 0` | 成功 | MediaEvent slot release |
| empty handle + stale `avDataId > 0` | 成功扱いの無処理 | 遅延finalize吸収 |
| empty handle + unknown `avDataId > 0` | `INVALID_ARGUMENT` | 不正dataId |
| fd付き handle + `avDataId > 0` | `INVALID_ARGUMENT` | fd付きhandleはslot releaseには使わない |
| 任意handle + `avDataId < 0` | `INVALID_ARGUMENT` | 不正dataId |

fd付き shared handle の同一性確認では、fd番号そのものを比較してはならない。client が dup した fd を返す可能性があるため、fd番号一致を契約条件にしない。

fd付きhandle + `avDataId == 0` の成功は、shared backing、公開済みhandle、既存slot、active `avDataId` を破棄することを意味しない。以後のAV payload配送を継続するには、client release済み状態を解除するために `getAvSharedHandle()` 再取得を必要としてよい。


### 表5. `close()` / 後片付け完了状態表

| No | 対象 | 呼び出し元 / 事象 | 後片付け手順 | 手順分類 | 閉鎖ゲート | 後片付け完了フラグ | 公開API戻り値 | Drop挙動 | 再試行条件 | 後続公開API | 診断保持 | 設計上の成立条件 | 固定根拠 |
|---:|---|---|---|---|---|---|---|---|---|---|---|---|---|
| CL-001 | Filter / DVR | 公開`close()`開始 | 公開API遮断開始 | 公開API遮断 | true | false | 後続手順結果で決定 | 該当なし | 後片付け未完の間は再試行対象 | `close()`以外は`INVALID_STATE` | close開始 | `close()`開始直後から他APIが成功しないこと | 閉鎖ゲートと後片付け完了を分離 |
| CL-002 | Filter / DVR | 公開`close()` | 作業スレッド停止 | 致命的、再試行対象 | true | false | 失敗時`UNKNOWN_ERROR` | Dropでは通常後片付けを再試行しない。DropLeakTxnへ未完診断を記録 | 作業スレッド停止未完 | `close()`以外は`INVALID_STATE` | 作業スレッド停止結果 | 作業スレッド停止失敗が成功扱いにならないこと | 動作中スレッドを残さない |
| CL-003 | Filter / DVR | 公開`close()` | キュー停止 / キュー解放 | 致命的、再試行対象 | true | false | 失敗時`UNKNOWN_ERROR` | Dropでは通常後片付けを再試行しない。DropLeakTxnへ未完診断を記録 | キュー後片付け未完 | `close()`以外は`INVALID_STATE` | キュー後片付け結果 | キュー後片付け失敗が記録されること | データ経路資源を残さない |
| CL-004 | Filter / DVR | 公開`close()` | AV / DVR 資源解放 | 致命的、再試行対象 | true | false | 失敗時`UNKNOWN_ERROR` | Dropでは通常後片付けを再試行しない。DropLeakTxnへ未完診断を記録 | 資源解放未完 | `close()`以外は`INVALID_STATE` | 資源解放結果 | 共有メモリやDVRキューが残らないこと | 資源リーク防止 |
| CL-005 | Filter / DVR | 公開`close()` | 未生成資源の解放 | 安全な無処理成功 | true | 既存値を維持 | 成功扱い | 該当なし | 不要 | `close()`以外は`INVALID_STATE` | 安全な無処理成功手順 | 未生成資源の解放が`close()`失敗にならないこと | lazy allocation と整合 |
| CL-006 | Filter / DVR | 公開`close()` | 登録解除 / コールバック切断 | ベストエフォート | true | 致命的手順の結果で決定 | 致命的手順が全成功なら成功 | Dropでは通常後片付けを再試行しない。DropLeakTxnへ未完診断を記録 | 登録解除未完 | `close()`以外は`INVALID_STATE` | 登録解除結果 | コールバック残存が診断へ残ること | 呼び出し元へ成功返却できる手順と致命的手順を分離 |
| CL-007 | Filter / DVR | 公開`close()`全手順成功 | 完了確定 | 完了確定 | true | true | 成功 | Dropで何もしない | 不要 | `close()`以外は`INVALID_STATE`。二重`close()`は CL-009 に従う | close成功 | cleanup_complete が true になること | 完全閉鎖 |
| CL-008 | Filter / DVR | 公開`close()`致命的手順失敗 | 未完確定 | 異常時閉鎖 | true | false | `UNKNOWN_ERROR` | Dropでは通常後片付けを再試行しない。DropLeakTxnへ未完診断を記録 | 失敗手順が残る間 | `close()`以外は`INVALID_STATE`。二重`close()`は CL-010 に従う | `failed_step`, `error_kind`, `remaining_steps` | 失敗が成功扱いにならないこと | fail-closed |
| CL-009 | Filter / DVR | 二重`close()` | 後片付け完了済み | 無処理成功 | true | true | 成功 | 何もしない | 不要 | `close()`以外は`INVALID_STATE` | `close_idempotent` | 二重closeが資源を壊さないこと | 冪等性 |
| CL-010 | Filter / DVR | 二重`close()` | 後片付け未完 | 再試行 | true | false | 再試行結果に従う | Dropでは通常後片付けを再試行しない。DropLeakTxnへ未完診断を記録 | 失敗手順が残る間 | `close()`以外は`INVALID_STATE` | `close_retry` | 未完cleanupを成功扱いで隠さないこと | cleanup_complete を正にする |


#### 表5-A. close開始遮断 実装所有表

表5の `閉鎖ゲート` は実装上も `CloseLifecycleTxn` が正本として所有する。各 資源オブジェクト が保持する `closed` / `closing` / `cleanup_complete` 相当の値は、`CloseLifecycleTxn` の状態を反映する派生値または保存先に限定し、別意味の閉鎖状態を再定義してはならない。

| Resource | close開始時の状態 | close中に許可する操作 | close中に拒否する操作 | cleanup失敗時状態 | 再試行条件 |
|---|---|---|---|---|---|
| Filter | `closing=true`, `cleanup_complete=false` | `close()` の再試行または同一 txn への合流 | `configure/start/stop/flush/read/getQueueDesc/getAvSharedHandle/releaseAvHandle` | `cleanup_failed` | `close()` 再試行可 |
| DVR | `closing=true`, `cleanup_complete=false` | `close()` の再試行または同一 txn への合流 | `configure/start/stop/flush/attachFilter/detachFilter/getQueueDesc/read/write` | `cleanup_failed` | `close()` 再試行可 |
| Demux | `closing=true`, `cleanup_complete=false` | `close()` の再試行 | `openFilter/openDvr/openDescrambler/setFrontendDataSource` | `cleanup_failed` または quarantine | `close()` 再試行可 |
| Frontend | `closing=true`, `cleanup_complete=false` | `close()` の再試行、所有者喪失 cleanup | `tune/scan/stopTune/stopScan/setCallback/linkLnb` | `cleanup_failed` または failed | `close()` または 所有者喪失 経路で再試行 |
| LNB | `closing=true`, `cleanup_complete=false` | `close()`、所有者喪失 cleanup | `setVoltage/setTone/setSatellitePosition/sendDiseqc/setCallback` | failed / quarantined | reopen または 所有者喪失 cleanup |
| Descrambler | `closing=true`, `cleanup_complete=false` | `close()` の再試行 | `setDemuxSource/setKeyToken/addPid/removePid` | `cleanup_failed` | `close()` 再試行可 |

Drop は公開 `close()` の代替正本ではない。Filter / DVR でも Drop から通常後片付けを再試行しない。Drop は未closeまたは後片付け未完了を `DropLeakTxn` に記録し、対象を漏えい診断 / 隔離診断へ落とすだけに限定する。公開 `close()` と所有者喪失経路で実施すべき backend apply、queue clear、ledger commit を Drop だけへ移してはならない。


### 表6. FMQ / EventFlag / 接続層失敗写像表

| No | 発生箇所 | 発生文脈 | 失敗条件 | 失敗分類 | 対象 | AIDL戻り値 | 作業スレッド挙動 | 一過性状態 | 累積カウンタ | あふれ通知 | 異常時閉鎖条件 | 再試行可否 | ペイロード扱い | 設計上の成立条件 | 固定根拠 |
|---:|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| FMQ-001 | 記述子公開 | 公開API | grantor数 / grantor取得失敗 | 記述子生成失敗 | Filter / DVR | `UNKNOWN_ERROR` | 該当なし | なし | `descriptor_export_error` を増やす | なし | なし | 可 | ペイロード未公開 | 失敗後オブジェクトが設定済み状態を維持すること | 記述子未公開状態へ戻す |
| FMQ-002 | 記述子公開 | 公開API | ファイル記述子複製失敗 | 記述子生成失敗 | Filter / DVR | `UNKNOWN_ERROR` | 該当なし | なし | `descriptor_fd_error` を増やす | なし | なし | 可 | ペイロード未公開 | ファイル記述子複製失敗後に再取得を試せること | 一時失敗扱い |
| FMQ-003 | 記述子公開 | 公開API | FMQ記述子の grantor配置、quantum、flags、ints の内部値不整合 | FMQ記述子内部不整合 | Filter / DVR | `UNKNOWN_ERROR` | 該当なし | 致命的状態 | `descriptor_internal_error` を増やす | なし | 1回で異常時閉鎖済み | 不可 | ペイロード未公開 | 記述子内部不整合で異常時閉鎖になること | ABI問題ではなく実行時の記述子安全検査 |
| FMQ-003A | FMQ生成 | 内部初期化 | AidlMessageQueue が無効、EventFlag word取得失敗、EventFlag生成失敗 | FMQ生成失敗 | Filter / DVR | `UNKNOWN_ERROR` | 該当なし | 作成失敗 | `fmq_create_error` を増やす | なし | 公開前なので対象なし | 再試行可 | 記述子未公開 | 無効queueをRust側に返さないこと | native薄層は create 成功条件として `isValid()` と EventFlag生成成功を確認する |
| FMQ-004 | Filter FMQ書き込み | 作業スレッド | 接続層の書き込み処理が失敗を返す | 致命的I/O | section / PES / TS生データ / 録画補助情報 | 公開APIなし | 作業スレッド致命停止 | 致命的状態 | `filter_fmq_write_error` を増やす | なし | 1回で異常時閉鎖済み | 不可 | 対象ペイロード破棄 | 書き込み接続層エラーが空データ扱いにならないこと | 採用済み方針 |
| FMQ-005 | Filter FMQ書き込み | 作業スレッド | キュー満杯 / 書き込み余地不足 | あふれ | section / PES / TS生データ / 録画補助情報 | 公開APIなし | 継続 | `overflow_pending=true` | `fmq_overflow` を増やす | 次callbackで通知 | なし | 可 | 新規ペイロード破棄 | キュー満杯が空データ成功に潰れないこと | overflow として扱う |
| FMQ-006 | EventFlag wake | 作業スレッド | wake システムコール失敗 | 致命的通知失敗 | FMQ対象 filter / DVR | 公開APIなし | 作業スレッド致命停止 | 致命的状態 | `eventflag_wake_error` を増やす | なし | 1回で異常時閉鎖済み | 不可 | 対象ペイロード破棄 | wake失敗が無視されないこと | 採用済み方針 |
| FMQ-007 | FMQ消去 | 公開API `flush()` / DVR `configure()` の再設定境界 / frontendストリーム境界初期化 | キュー消去処理の不整合、read不足、再生残りバッファのロック失敗 | 致命的queue破損 | Filter / DVR | `UNKNOWN_ERROR` | 該当なし | 致命的状態 | `fmq_clear_error` を増やす | なし | 1回で異常時閉鎖済み | 不可 | 未消費ペイロード破棄不可 | flush / configure / stream boundary reset 失敗後に通常継続しないこと | queue整合性を優先 |
| FMQ-008 | DVR record 書き込み | 作業スレッド | 接続層の書き込み処理が失敗を返す | 致命的I/O | DVR record | 公開APIなし | 作業スレッド致命停止 | 致命的状態 | `dvr_fmq_write_error` を増やす | なし | 1回で異常時閉鎖済み | 不可 | 対象ペイロード破棄 | record出力失敗が空データ扱いにならないこと | 採用済み方針 |
| FMQ-009 | DVR record 読み出し | 公開API | 読み出し処理が失敗扱いを返し、キューも空である | 通常空読み | DVR record | 成功 | 該当なし | なし | 変化なし | なし | なし | 可 | 0 byte read | 空読みをエラーにしないこと | DVR read契約 |
| FMQ-010 | DVR playback 入力 | 公開API | 書き込み側の空き不足 | 入力抑制 | DVR playback | 成功 | 継続 | `playback_backpressure=true` | `playback_backpressure` を増やす | 状態通知 | なし | 可 | 0 byte written | 古い入力を HAL 内部で勝手に捨てないこと | no-eviction 方針 |
| FMQ-011 | EventFlag wait timeout | 作業スレッド | 待機timeout | 通常待機timeout | Filter / DVR | 公開APIなし | 継続 | なし | 増やさない | なし | なし | 可 | なし | timeoutが異常診断を汚さないこと | 採用済み方針 |
| FMQ-012 | EventFlag wait error | 作業スレッド | 待機システムコールエラー | 致命的通知失敗 | Filter / DVR | 公開APIなし | 作業スレッド致命停止 | 致命的状態 | `eventflag_wait_error` を増やす | なし | 1回で異常時閉鎖済み | 不可 | なし | wait error が黙殺されないこと | 採用済み方針 |
| FMQ-013 | AV共有メモリ割当 | 作業スレッド | 領域割当失敗 | あふれ | live AV | 公開APIなし | 継続 | `av_overflow_pending=true` | `av_shared_memory_overflow` を増やす | 次callbackで通知 | なし | 可 | 新規AVペイロード破棄 | 共有メモリ不足をFMQ空データ扱いにしないこと | AV経路専用 |
| FMQ-014 | AV共有メモリ破損 | 作業スレッド | backing破損、offset範囲外、領域管理不整合 | 致命的AV資源破損 | live AV | 公開APIなし | 作業スレッド致命停止 | 致命的状態 | `av_shared_memory_internal_error` を増やす | なし | 1回で異常時閉鎖済み | 不可 | 対象AVペイロード破棄 | 不正offsetをMediaEventで出さないこと | 安全性優先 |

Filter `configure()` と DVR `configure()` は、旧一過性状態の破棄を先に行い、破棄が成功した後だけ demux 側設定を変更する。Filter では旧通常FMQ出力、AV用FMQ出力、AV shared backing / exported handle / active slot を先に破棄する。DVR record では旧 output queue を先に消去し、DVR playback では playback input queue と packet assembler の残りを先に破棄する。破棄失敗時は `UNKNOWN_ERROR` を返し、`configure_*_with_summary_result()` を呼ばない。これにより「戻り値は失敗だが内部設定だけ新状態」という部分成功を禁止する。


#### checked FMQ shim 入力契約


#### 表6-A. FMQ / EventFlag commit 細分表

表6の失敗写像を実装へ落とすため、FMQ delivery の commit 点を次で固定する。記述子公開、payload write、clear、playback read は同じ成功条件で扱わない。

| 処理 | commit前 | commit点 | commit後失敗 | 公開API戻り値 / worker挙動 | 内部状態 |
|---|---|---|---|---|---|
| FMQ descriptor export | grantor / fd / ints / flags の検証 | descriptor を AIDL へ返す直前 | fd duplicate 失敗、grantor配置不整合 | transient export failure は Err 後も再取得可。structural failure は runtime failed | 表6 FMQ-001〜003 に従う |
| FMQ write | capacity確認、payload境界確認 | native write成功 + EventFlag wake成功 | wake失敗 | ワーカー delivery failure。成功扱い不可 | delivery failed または runtime failed |
| FMQ clear | clear開始、対象queue確定 | native clear / residual discard 成功 | clear不足、lock失敗 | public `flush()` / cleanup は Err | runtime failed または cleanup failed |
| DVR playback read | FMQ read実行 | TS parse と inject の扱いを `PlaybackConsumeTxn` が確定 | 内部注入失敗 | 消費確定しない | playback runtime failed |
| EventFlag wait | wait対象確認 | signal観測またはtimeout | wait error | ワーカー制御失敗 | ワーカー failed / retryable stop state |


checked FMQ shim は、`queue == null` または `out_written == null` を `INVALID_ARGUMENT` とする。`size == 0` は `data == null` でも成功扱いの無処理 とする。`size > 0 && data == null` は `INVALID_ARGUMENT` とする。この契約は FMQ 実体の read/write 契約より前に適用する。

### 表7. 操作別 確定点 / 巻き戻し / 閉鎖側失敗表

本表は、公開APIまたは作業スレッドが複数資源を変更する場合の確定点を固定する。成功を返すには、確定点までに列挙した変更が全て完了していなければならない。確定点前の失敗は、表に記載した巻き戻しを実施する。巻き戻せない場合は、表に記載した対象を閉鎖側失敗へ遷移させる。

| No | 操作 / 事象 | 変更順序 | 成功の確定点 | 確定点前の失敗 | 巻き戻し不能時の対象 | 公開戻り値 / 作業スレッド終了 | 設計上の成立条件 |
|---:|---|---|---|---|---|---|---|
| AT-001 | `IFrontend.tune()` | 入力検証 → 旧generation無効化 → bound demux stream boundary reset → backend tune submit → tune ワーカー 起動 | backend tune submit と tune ワーカー 起動が両方成功した時点 | demux reset 失敗では backend へ新tuneを投入しない。backend submit 後の ワーカー 起動失敗では backend stop を試す | frontend runtime、bound demux、配下 filter / DVR | `UNKNOWN_ERROR`。次の tune / scan へ進まない | 「戻り値は失敗だが実機だけ新tune済み」を残さない |
| AT-002 | scan ワーカーの per-request tune | scan generation確認 → bound demux stream boundary reset → backend scan/tune submit → scan コールバック配送 | backend submit と必要な scan callback 配送が成功した時点 | callback 失敗後は次の scan request や backend tune へ進まない | frontend runtime、scan generation | `WorkerExit::RuntimeFailure`、scan reason は `FailedCallback` または `FailedBackend` | callback 失敗を scan 継続成功にしない |
| AT-003 | tune ワーカーの `LOCKED` / `NO_SIGNAL` 通知 | generation確認 → frontend callback通知 → runtime状態更新 → live pump 起動判定 | callback 成功後にだけ live pump 起動判定へ進む | callback 未登録またはBinder失敗時は コールバック登録を解除し、live pumpを開始しない | frontend runtime、live path、bound demux配下 | `WorkerExit::RuntimeFailure` | 通知失敗後に映像経路を開始しない |
| AT-004 | scan `END` 通知 | terminal reason確定 → `END` コールバック配送 → terminal通知済み記録 | `END` callback 成功時 | `END` 失敗は追加診断へ残し、失敗理由を `FailedCallback` にできる | frontend scan generation | `WorkerExit::RuntimeFailure` | terminal通知失敗を `let _ =` で捨てない |
| AT-005 | Filter `configure()` | 旧作業スレッド停止 → 旧FMQ / AV資源破棄 → demux filter設定更新 | 旧資源破棄と demux設定更新が全て成功した時点 | 旧資源破棄失敗では demux設定を変更しない | filter | `UNKNOWN_ERROR` | 失敗戻り値で新設定だけ残さない |
| AT-006 | DVR `configure()` | 旧作業スレッド停止 → 旧record/playback queue破棄 → demux DVR設定更新 | 旧資源破棄と demux設定更新が全て成功した時点 | 旧資源破棄失敗では demux設定を変更しない | DVR | `UNKNOWN_ERROR` | 失敗戻り値で新設定だけ残さない |
| AT-007 | FMQ対象 payload delivery | queue write → EventFlag wake | write 成功と wake 成功の両方が成立した時点 | write失敗、short write、wake失敗はいずれも成功にしない | 対象 filter / DVR | `WorkerExit::RuntimeFailure` | ペイロード格納済みなのに通知不能、または通知成功扱いなのに未格納を残さない |
| AT-008 | DVR playback consumer | playback状態確認 → `playback_consume_lock`取得 → FMQ read → packet assembly → inject | read済み payload が inject まで成功した時点 | 停止中、方向違い、DVR不在では FMQ を読まない。read後の inject拒否は消費済み成功にしない | DVR playback | `WorkerExit::RuntimeFailure` | stop/close競合で入力を黙って捨てない |
| AT-009 | demux close / demux generation invalidation | demux公開API遮断 → filter/DVR停止 → descrambler PID claim無効化 → demux generation無効化 | descrambler側と demux側の無効化が両方完了した時点 | descrambler無効化失敗を Drop / best-effort 経路でも破棄しない | demux、該当 descrambler | 公開経路は `UNKNOWN_ERROR`、戻り値不能経路は `descrambler_demux_invalidate_error` 診断記録 | 閉鎖済みdemuxにPID claimを残さない |
| AT-010 | `IDescrambler.addPid()` / `removePid()` | source filter検証 → demux generation確認 → PID claim更新 → backend packet経路反映 | 台帳更新と packet経路反映が両方成功した時点 | source不正、世代不一致、閉鎖済みは台帳を変更しない | descrambler、必要に応じて demux | 入力不正は `INVALID_ARGUMENT` / `INVALID_STATE`、内部失敗は `UNKNOWN_ERROR` | PID claim と実packet経路を乖離させない |
| AT-011 | `ILnb.setVoltage()` / `setTone()` / `setSatellitePosition()` | `operation_lock`取得 → 旧状態取得 → 新状態候補作成 → backend反映 → registry確定 | backend反映と registry確定が両方成功した時点 | backend反映失敗では registry を変更しない。registry確定失敗時に backend rollback apply は行わない | LNB、関連 satellite frontend | `UNKNOWN_ERROR`、LNBは失敗状態。以後の公開制御APIも `UNKNOWN_ERROR` | registryとbackendの二重巻き戻し失敗を作らない |
| AT-012 | `ILnb.close()` / owner loss | `operation_lock`取得 → 安全状態作成 → backend初期化戻し → registry安全状態確定 → callback解放 → 閉鎖確定 | backend初期化戻しと registry確定と callback解放が完了した時点 | 初期化戻し失敗は close成功にしない。Rust `Drop` は通常cleanupを実行せず、未close診断・quarantine・コールバック参照の局所clearだけを行う | LNB、関連 satellite frontend | 公開closeは `UNKNOWN_ERROR`。owner lossは診断付き lifecycle close。Dropは `DropLeakTxn::record_unclosed_drop(ResourceKind::Lnb)` 相当の診断・隔離のみ | 閉鎖後に電圧 / tone / position の実状態を不定にしない。Dropを通常closeの代替にしない |
| AT-013 | ワーカー 起動 / 停止待ち | `WorkerRuntime::spawn_owned_with_exit_hook()` → owner signal 停止要求 → `WorkerHandle::join_from_owner()` | `Normal` または `StopRequested` を確認した時点 | spawn失敗、Condvar / Mutex失敗、`RuntimeFailure`、`PanicOrJoinFailure` は成功にしない | ワーカー所有 object | 公開経路は `UNKNOWN_ERROR`、非同期経路は所有 object を閉鎖側失敗 | 作業スレッド異常終了やDVR callback workerの待機失敗を close成功・通常timeout 扱いにしない |
| AT-014 | コールバック配送 | コールバック登録確認 → Binder callback呼び出し → 結果検査 | Binder callback成功時 | コールバック未登録、Binder失敗、戻り値失敗を `let _ =` で捨てない | コールバック所有 object | 公開経路は該当 error、作業スレッドは `RuntimeFailure` | コールバック失敗後に後続副作用へ進まない |

ただし、Filter / DVR `start()` のように callback が commit 後に実行されるAPIでは、callback失敗を理由に commit済み状態を rollback してはならない。この場合、対象オブジェクトは開始済み状態を維持したまま `callback_unhealthy` に固定し、表1 / 表2の commit 後コールバック失敗契約に従う。復旧操作は `stop()` / `flush()` / `close()` に限定する。

### 表8. 資源寿命・所有権・破棄失敗表

本表は、Tuner HAL 内の資源について、所有者、通常破棄、異常時破棄、破棄失敗時の扱いを固定する。表7の操作別契約と矛盾する場合は、表7の操作別契約を優先し、本表を更新する。

| No | 資源 | 所有者 | 作成 / 取得 | 通常破棄 | 異常時破棄契機 | 破棄失敗時 | 設計上の成立条件 |
|---:|---|---|---|---|---|---|---|
| RL-001 | frontend backend state | `FrontendHal` | frontend open / backend probe | `IFrontend.close()` | tune / scan ワーカー異常、backend ioctl失敗 | frontendを閉鎖側失敗。bound demux配下を停止 | backend状態と frontend runtime state が乖離しない |
| RL-002 | scan / tune generation | `FrontendHal` | `tune()` / `scan()` | stopTune / stopScan / close / 次generation | コールバック失敗、ワーカー異常 | 古いgenerationの通知を捨て、現generationを失敗状態にする | 古いワーカーが新状態を上書きしない |
| RL-003 | demux generation | `DemuxHal` | demux open / stream boundary reset | demux close | frontend tune boundary、demux fail-closed | demuxを閉鎖側失敗。診断に失敗対象を残す | closed demux向けの後続配送が残らない |
| RL-004 | filter FMQ / EventFlag | `FilterHal` | `configure()` / `getQueueDesc()` | `flush()` / `configure()` / `close()` | write失敗、wake/wait失敗、queue破損 | filterをF16へ遷移 | 失敗後にDATA_READY成功扱いを返さない |
| RL-005 | DVR record / playback queue | `DvrHal` | `configure()` | `flush()` / `configure()` / `close()` | read/write失敗、playback 注入失敗、wait失敗 | DVRを異常時閉鎖 | 入力・出力データを silent drop しない |
| RL-006 | ワーカー thread | 各 所有オブジェクト | `WorkerRuntime::spawn_owned_with_exit_hook()` | stop / close。Dropでは通常後片付けを実行せず未close診断に限定 | panic、runtime failure、join failure | 所有オブジェクトを閉鎖側失敗 | 異常停止が診断と状態へ反映される |
| RL-007 | callback object | frontend / filter / DVR / LNB | `setCallback()` 等 | close / 再設定 / コールバック失敗後cleanup | Binder失敗、登録先死亡 | コールバック登録を解除し owner を失敗状態へ遷移 | dead callback に後続通知しない |

`IFrontend.setCallback(non-null)` は active tune / scan 中でも callback 実体の差し替えを許可する。frontend の通知経路は各通知時点で callback slot から最新の callback を取得し、差し替え後に旧 callback へ後続通知しない。差し替え自体は scan generation / tune generation / backend state を停止または巻き戻さない。callback Binder 失敗時は表7 AT-014 と表8 RL-007 に従い callback 登録を解除し、対象 generation を コールバック失敗 として扱う。
| RL-008 | AV shared backing / exported handle / active slot | AV filter | AV configure / `getAvSharedHandle()` / payload割当 | `configure()` / `close()` / `flush()` / `releaseAvHandle()` の表4契約 | backing破損、範囲不整合、割当管理破損 | AV filterをF16へ遷移 | 不正offsetや古いdataIdをMediaEventで出さない |
| RL-009 | descrambler PID claim | `DescramblerRegistry` | `addPid()` | `removePid()` / descrambler close / demux close | demux generation失効、key token失効 | descramblerと該当demuxを失敗状態へ遷移 | closed demuxにPID claimを残さない |
| RL-010 | key token binding | 復号鍵台帳 | `setKeyToken()` | session close / service切替 / demux generation失効 / 明示失効 | registry lock失敗、token解決不能 | descramblerを失敗状態へ遷移 | raw keyをBinderへ出さず、失効済みと未知を区別する |
| RL-011 | LNB registry state | `LnbRegistry` / `LnbHal` | LNB open / set系API | `ILnb.close()` | backend反映失敗、registry確定失敗、mutex汚染 | LNBを失敗状態。関連frontendへ診断反映 | registry状態とbackend状態を成功扱いで乖離させない |
| RL-012 | LNB backend state | satellite frontend backend | `setLnb()` / set系API / close初期化戻し | `ILnb.close()` / frontend close | registry確定失敗、close初期化戻し失敗 | LNBと関連frontendを失敗状態 | 閉鎖後に給電状態を不定にしない |

### 表9. 固定表現要約表

本表は、表1から表8に固定した主要事項の要約である。状態遷移、戻り値、資源寿命、閉鎖側失敗対象は表1から表8を正とし、本表だけを根拠に実装完了と判定してはならない。

| No | 固定表現 | 関連箇所 |
|---:|---|---|
| 1 | 製品全体の入力方式スコープは `開発規則.md` を正とする。本書では Tuner HAL の capability / VTS profile として TS入力だけを宣言し、MMTP、TLV、ALP、IP CID を宣言しないことを固定する | 方式・capability 説明 |
| 2 | 本製品のライブAVフィルタは、non-passthrough `MediaEvent` + 共有メモリ + `dataId` 経路だけを正式対応とする | AV経路説明 |
| 3 | AVペイロードは通常FMQへ書き込まない。EventFlag は FMQ対象経路の通知にだけ使う | AV / FMQ 説明 |
| 4 | 本製品では AV passthrough を恒久的に対応しない。passthrough capability は宣言せず、passthrough要求は configure時 `UNAVAILABLE` とする | AV passthrough 説明 |
| 5 | `getQueueDesc()` は横断gateに該当しない通常可用状態で、対象オブジェクトが通常FMQ記述子を持つ場合だけ成功する。IFilterでは configure 済みかどうかではなく open時フィルタ種別の通常FMQ有無を正とする | IFilter / DVR状態表 |
| 6 | `flush()` は共有ハンドル未公開のAVフィルタでも成功する。共有ハンドル未公開中は無処理成功として扱う | AV flush 説明 |
| 7 | `releaseAvHandle(dataId=0)` は全解放ではない。client 側 AV handle 使用終了通知として扱い、shared backing、公開済みハンドル、既存`dataId`、使用中領域を破棄しない | AV資源寿命説明 |
| 8 | Filter / DVR の `close()` は、公開API遮断ゲートと後片付け完了状態を分離する。致命的な後片付け失敗は `UNKNOWN_ERROR` と異常時閉鎖済み状態に反映する | close説明 |
| 9 | ABI不整合、関数シグネチャ不整合、リンク不整合は実行時状態表に入れない | FMQ / 接続層説明 |
| 10 | 状態行を圧縮してよいのは、対象状態集合、戻り値、次状態関数、副作用、診断、資源寿命の同値性を表内に明記できる場合だけとする | 表の記載規則 |
| 11 | EventFlag はペイロード格納先ではない。EventFlag は FMQ対象経路の通知機構として扱う | EventFlag説明 |
| 12 | close の成功固定に読める既存行は表5に合わせる。致命的後片付け失敗は成功扱いしない | close説明 |
| 13 | AV flush / release の既存行は表4に合わせる。flush は shared backing と公開済みハンドルを維持し、使用中領域と全`dataId`を破棄する。`releaseAvHandle(dataId=0)` は shared backing を破棄しない | AV資源寿命説明 |
| 14 | `setDataSource(NULL)` は AOSP意味論として sink の入力元を demux input に戻す。ただし 現行 Android 14 AIDL Rust 生成境界では実装済み対象に含めない | setDataSource説明 / nullable Binder 境界 |

### 10. 設計表の自己整合条件

| No | 整合観点 | 設計上の条件 |
|---:|---|---|
| 1 | 未固定語検査 | 設計値セルに未固定語が残っていない。互換表の種別名では具体種別名を列挙する |
| 2 | 選択式表現検査 | 戻り値セルと次状態セルに二者択一の表現がない |
| 3 | 状態軸検査 | DVR種別、DVR未設定状態、AVストリーム種別設定有無、共有ハンドル公開有無、開始/停止状態が行で失われていない。shared backing は表4の資源寿命列で管理する |
| 4 | 同値圧縮検査 | 圧縮行には対象状態集合と同値性根拠がある |
| 5 | capability整合検査 | 未対応機能が capability と VTS profile に宣言されていない |
| 6 | AOSP境界検査 | AIDL ABI、リンク、関数シグネチャ不整合を実行時状態表に入れていない |
| 7 | AV経路検査 | AVペイロードを通常FMQへ書き込む経路が表に存在しない |
| 8 | EventFlag表現検査 | EventFlag をペイロード格納先として扱う表現がない |
| 9 | close検査 | `closed` と `cleanup_complete` が分離され、致命的後片付け失敗を成功扱いにしていない |
| 10 | AOSP releaseAvHandle 検査 | `releaseAvHandle(dataId=0)` は client 側 AV handle 使用終了通知として扱い、shared backing、公開済みハンドル、既存`dataId`、使用中領域を破棄しない |
| 11 | AOSP setDataSource 検査 | `setDataSource(NULL)` は AOSP意味論として存在するが、現行 Android 14 AIDL Rust 生成境界では nullable Binder 境界の現行実装済み対象外 として扱う |
| 12 | 実装反映検査 | 表1〜表8の各行に対応する単体テストや状態遷移テストを作成できる |


### 表10. 失敗領域と波及範囲

失敗分類と波及範囲は、本書冒頭の「0-S-4. 失敗分類と波及範囲」を正本とする。本節では再定義しない。

各API表で異なる戻り値または波及範囲を採る場合は、API表側にその差分だけを記載する。コールバック失敗、ワーカー失敗、backend failure、データ経路 failure、ledger failure、rollback failure、cleanup failure を同じ失敗として丸めてはならない。

### 表11. 同一条件呼び出し 無処理 契約

同一条件の再指定は、破壊的操作にしてはならない。破壊的操作が必要な場合は、状態比較により条件差分を確定してから実行する。

| API | 同一条件 | 破壊的処理の可否 | 異なる条件 |
|---|---|---:|---|
| `IDemux.setFrontendDataSource(frontend)` | 現在と同じ frontend / generation | stream boundary reset を行わない | 旧frontend unbind、新frontend bind、boundary reset |
| `IFrontend.tune(settings)` | normalized tune settings が現在条件と同一 | backend stop、live pump停止、demux boundary reset を行わない | 旧tune停止、新tune投入、boundary reset |
| `IFilter.configure(settings)` | 現在設定と同一 | queue / AV backing を破棄しない | validate後にcommitし、必要時だけqueue境界処理 |
| `IDvr.configure(settings)` | 現在設定と同一 | queueを破棄しない | validate後にcommitし、record/playback種別変更時だけqueue境界処理 |

### 表12. 公開API transaction（状態遷移）契約

公開API transaction の共通契約は、本書冒頭の「0-S-3. 公開API transaction（状態遷移）契約」を正本とする。本節では validate / reserve / prepare / apply / commit / rollback / quarantine を再定義しない。

個別APIの確定点と巻き戻し対象は「表7. 操作別 確定点 / 巻き戻し / 閉鎖側失敗表」を正とする。表7が0-S-3と矛盾する場合は、0-S-3の原則に合わせて表7を更新する。

### 表13. best-effort 使用範囲

`best_effort` の使用範囲は、「0-S-3. 公開API transaction（状態遷移）契約」と「0-S-4. 失敗分類と波及範囲」を正本とする。本節では表を重複定義しない。

設計上、`best_effort` は Drop、process teardown、公開API の補助診断のように戻り値へ反映できない経路に限る。公開API の主状態変更、registry unregister、queue clear / discard を `best_effort` で握りつぶしてはならない。必要な場合は rollback、error return、quarantine のいずれかに写像する。

### 表14. 寿命ID・世代ID・token 規則

寿命ID、世代ID、token ID に `saturating_add()` を使って固定値で継続してはならない。上限到達時は対象を失敗状態へ移すか、新規発行を失敗させる。

| 対象 | 加算規則 | 上限到達時 | 禁止事項 |
|---|---|---|---|
| filter delivery generation | `checked_add(1)` | 対象filterをquarantine | `saturating_add()` で固定値継続 |
| section / PES assembler generation | `checked_add(1)` | 対象filterをquarantine | flush判定不能なまま継続 |
| ワーカー signal generation | `checked_add(1)` | 対象ワーカーをfailed停止 | wake generation固定化 |
| LNB state generation | `checked_add(1)` | 対象LNBをquarantine | 世代固定化 |
| AV `avDataId` | 正数だけ発行。0と負数は予約 | AV経路 failed | wrapして負値IDを発行 |
| descrambler key token | wrap禁止。expired token は削除 | 新規token発行失敗 | expired token を永久保持 |

### 表15. backend state model

DVB と px4 の状態、診断名前空間、失敗扱いは分離する。DVB backend failure を px4 診断へ記録してはならず、px4 backend failure を DVB 診断へ記録してはならない。

| backend | 状態 | 意味 | 診断名前空間 |
|---|---|---|---|
| DVB | `Idle` | fdあり、tuneなし | DVB |
| DVB | `Tuning` | tune ioctl中 | DVB |
| DVB | `Locked` | lock確認済み、reader稼働可 | DVB |
| DVB | `Stopping` | `stop_tune()` 中 | DVB |
| DVB | `Closed` | reader停止、fd release済み | DVB |
| DVB | `Failed` | ioctl/read/clear等で復旧不能 | DVB |
| px4 | `Idle` | device open済み、streamingなし | px4 |
| px4 | `Streaming` | px4 streaming中 | px4 |
| px4 | `Stopping` | streaming停止中 | px4 |
| px4 | `Closed` | device release済み | px4 |
| px4 | `Failed` | px4固有ioctl/read失敗 | px4 |

frontend 共通処理から backend failure を記録する場合は、backend種別を受け取り、対応する診断名前空間だけへ記録する。

### 表16. source filter downstream 契約

source filter downstream の対応範囲は、「表18. source filter origin / downstream 状態所有契約」を正本とする。本節では同じ行列を重複定義しない。

本製品の source filter linkage は raw TS packet を下流 raw TS / record 系へ配送する範囲だけを正式対応とする。section payload / PES payload / AV payload / record payload を別filterへ直接再投入する linkage は対応しない。非対応組み合わせは成功扱いの無処理 にせず、設定時または接続時に `UNAVAILABLE` とする。

### 表17. key token 所有権・参照カウント契約

key token は HAL 内部では refcount 付き共有資源として管理する。同一 token bytes を複数 `IDescrambler` が `setKeyToken()` してよい。

HAL 内 refcount は、HAL が保持する token 解決結果の寿命だけを管理する。CAS session の本来の寿命、CAS HAL 側の失効、ECM更新方針を代替しない。

key token table は token bytes 単位で key material を保持し、descrambler session 単位の参照数を持つ。token slot は refcount が 0 になった時だけ削除する。

| No | 操作 | 入力状態 | AIDL戻り値 | key table 変更 | session 変更 | 設計上の成立条件 |
|---:|---|---|---|---|---|---|
| KT-001 | `setKeyToken(non-VOID)` | token malformed | `INVALID_ARGUMENT` | なし | なし | 長さ・形式不正を未知tokenと混同しない |
| KT-002 | `setKeyToken(non-VOID)` | token unknown / expired | `INVALID_STATE` | なし | なし | 未登録または失効済みkeyを有効化しない |
| KT-003 | `setKeyToken(non-VOID)` | 現在tokenなし、新token有効 | 成功 | 新token refcount +1 | sessionに新token設定 | key material解決とrefcount増加が両方成功 |
| KT-004 | `setKeyToken(non-VOID)` | 現在token A、新token A | 成功 | 変更なし | 変更なし | 同一token再設定は 無処理。release しない |
| KT-005 | `setKeyToken(non-VOID)` | 現在token A、新token B | 成功 | B refcount +1 後に A refcount -1 | sessionをBへ変更 | B確保成功前にAを失効しない |
| KT-006 | `setKeyToken(VOID)` | 現在token A | 成功 | A refcount -1 | session keyを空へ変更 | refcount減少とsession clearが両方完了 |
| KT-007 | descrambler close | 現在token A | close表に従う | A refcount -1 | session closed | key release失敗時はdescramblerを異常時閉鎖へ移す |
| KT-008 | token refcount 0 | active sessionなし | - | token slot削除 | - | expired tokenを永久保持しない |


#### 表17-B. Descrambler cleanup / key lifetime transaction（状態遷移）表

`DescramblerSession` と `DescramblerKeyTable` の更新は `DescramblerKeyTxn` / `DescramblerSessionCleanupTxn` が所有する。session と key table をAPIごとに別々に個別更新してはならない。cleanup 中に1件失敗しても、同じ demux / close 対象に属する後続 session の cleanup を未試行のまま終了してはならない。

| 操作 | session更新順序 | key table更新順序 | 失敗時 | 後続session処理 | 共通部品 |
|---|---|---|---|---|---|
| `setKeyToken(non-VOID)` validate | session未変更 | 新key ref取得 | 取得失敗ならsession変更なし | 継続 | `DescramblerKeyTxn` |
| new key commit | pending→current | 旧key release | 旧key release失敗は cleanup failed 診断。新keyがcommit済みならrollbackしない | 継続 | `DescramblerKeyTxn` |
| `setKeyToken(VOID)` | key pending clear | 旧key release | release失敗なら session は no-key、cleanup failed 診断 | 継続 | `DescramblerKeyTxn` |
| `snapshots_for_demux()` | stale binding検出 | 必要key release | 1件失敗しても全session走査継続 | 失敗一覧を返す | `DescramblerSessionCleanupTxn` |
| `invalidate_demux()` | 全affected sessionを走査 | key release/expire | 1件失敗しても全件試行 | 失敗一覧を返す | `DescramblerSessionCleanupTxn` |
| `close()` | closing gate | key release | 失敗時 cleanup_failed、再close可能 | retry可 | `CloseLifecycleTxn` |


```mermaid
flowchart LR
    CAS[CAS bridge / token issuer] -->|token register| KT[Key Token Table]
    KT -->|resolved key material| DS[Descrambler Session]
    DS -->|PID claim + key ref| DR[Descrambler Runtime]
    DR -->|descramble| TS[TS packet経路]

    DS -->|close / set VOID| REL[release ref]
    REL -->|refcount > 0| KT
    REL -->|refcount = 0| DEL[token slot delete]
```

### 表18. source filter origin / downstream 状態所有契約

Tuner HAL は AOSP Tuner HAL の filter linkage 構造のうち、capability と本表で固定した範囲だけを受理する。

本製品の source filter linkage は、raw TS packet を下流 raw TS / record 系へ配送する範囲だけを正式対応とする。section payload / PES payload / AV payload を別filterへ直接再投入する linkage は対応しない。

AOSP `DemuxCapabilities.linkCaps` は main type 粒度であり、TS subtype の raw TS / record だけを精密に表現できない。そのため本製品は TS→TS main type linkage の宣言を維持し、subtype 別の正式対応範囲を本表と `setDataSource()` 検証で固定する。対応しない subtype linkage が要求された場合は `UNAVAILABLE` を返し、`INVALID_ARGUMENT` とはしない。

VTS は linkCaps の main type bit から subtype `UNDEFINED` の `DemuxFilterType` を生成し得る。そのため、TS→TS main type linkage を宣言する場合、`UNDEFINED` subtype による source filter 接続要求に対する扱いを固定する。本製品では、`UNDEFINED` subtype は payload配送元として成立する具体subtypeではないため、実際に成功させる source / sink subtype は表1-Dの検証表だけを正とする。

source filter は配送元であり、downstream filter の continuity / assembler 状態を未接続時に進めてはならない。source filter flush / reconfigure / close では、source origin generation を進め、接続済みdownstreamのpartial stateを破棄する。

| No | 事象 | 状態所有者 | 許可する副作用 | 禁止する副作用 | 設計上の成立条件 |
|---:|---|---|---|---|---|
| SF-001 | frontend input TS | `TsInputOrigin::Frontend` | frontend origin の continuity / assembler 更新 | source filter origin への混入 | frontend直入力として処理 |
| SF-002 | DVR playback input TS | `TsInputOrigin::Playback(dvr_id)` | playback origin の continuity / assembler 更新 | frontend origin への混入 | playback入力として処理 |
| SF-003 | source filter raw TS delivery | `TsInputOrigin::SourceFilter(filter_id, generation)` | 接続済みdownstreamに限り、そのdownstream用状態を更新 | downstream未接続時のassembler更新 | 未接続なら状態を汚染しない |
| SF-004 | source filter flush | source filter + downstream接続表 | source origin generation更新、接続済みdownstream partial破棄 | 古いpartialの保持 | flush後の旧payloadを配送しない |
| SF-005 | source filter reconfigure | source filter | source generation更新、既存downstream接続解除 | 旧条件でのdownstream継続 | reconfigure後は再接続必須 |
| SF-006 | source filter close | source filter | downstream接続解除、source origin破棄 | downstreamに閉鎖済みsourceを残す | close後source由来配送なし |

| source filter 出力 | downstream | 対応 | 配送内容 | 状態所有者 | flush時処理 | 非対応時 |
|---|---|---:|---|---|---|---|
| raw TS packet | raw TS filter | 可 | 同一TS packet view | downstream raw TS filter | source origin generation更新 | - |
| raw TS packet | record filter | 可 | record event / record packet | record filter | record partial破棄 | - |
| raw TS packet | section filter | 不可 | 再parse section は行わない | なし | なし | `UNAVAILABLE` |
| raw TS packet | PES filter | 不可 | 再parse PES は行わない | なし | なし | `UNAVAILABLE` |
| section payload | 任意downstream | 不可 | 直接再配送しない | なし | なし | `UNAVAILABLE` |
| PES payload | 任意downstream | 不可 | 直接再配送しない | なし | なし | `UNAVAILABLE` |
| AV payload | 任意downstream | 不可 | 直接再配送しない | なし | なし | `UNAVAILABLE` |


#### 表18-B. source filter boundary 補足表

source filter boundary は downstream lifecycle、queued payload、pending event、assembler state、DVR attach を分けて扱う。source filter の接続変更だけで downstream filter の公開 lifecycle を暗黙に stopped / failed へ変えてはならない。failed 化する条件は本表または表6/表5に明記された異常に限定する。

| 操作 | downstream lifecycle | queued payload | pending event | assembler state | DVR attach | public状態 |
|---|---|---|---|---|---|---|
| source filter接続 | 変更しない | 既存queueは generation 境界で区別 | 既存eventは旧generationとして扱う | source origin generation更新 | 変更なし | started維持可 |
| source filter切断 | 変更しない | source origin generationを進め、旧generationのsource由来queueを配送対象から外す。物理破棄は `SourceBoundaryTxn` の共通処理でだけ行い、API別に分岐させない | source由来event抑止 | source origin partial state reset | attach維持。明示detachは別APIだけ | started維持 |
| source filter close | downstreamは source lost 境界を観測 | source由来queueは `SourceBoundaryTxn` が物理破棄できるentryを破棄し、残るentryを旧generationとして配送禁止にする。この組み合わせを唯一の共通方針とし、API別に分岐させない | source由来event抑止 | source origin reset | DVR attach解除は `FilterUnregisterTxn` / `SourceBoundaryTxn` が診断へ残す | downstreamを自動failedにしない。閉鎖済みsourceを参照する再配送だけ拒否 |
| downstream reconfigure | reconfigure表に従う | reconfigure前データ破棄 | reconfigure前event破棄 | downstream origin reset | attach維持可否を明記 | configured状態 |
| upstream generation mismatch | 変更しない | 配送しない | event抑止 | reset | 変更なし | runtime failedにはしない |


### 表19. `IFrontend.tune()` transaction（状態遷移）契約

`IFrontend.tune()` は、validate / prepare が完了するまで旧tune状態を破壊しない。

validate には、settings型、周波数範囲、frontend capability、LNB候補を含める。prepare には、ワーカー生成準備、コールバック経路 準備可能性、バックエンドロールバック経路 準備可能性を含める。

backend tune submit 後に ワーカー生成 が失敗した場合は、旧tune復旧を試みる。旧tune復旧に失敗した場合は、frontend failed とし、bound demux は quarantine へ移す。

ワーカー生成 失敗時に `LOCKED` / `NO_SIGNAL` / scan message を送ってはならない。

| No | 段階 | 処理 | 失敗時 | 旧tune維持 |
|---:|---|---|---|---:|
| TN-001 | validate | settings正規化、capability、周波数範囲、LNB候補検証 | `INVALID_ARGUMENT` / `UNAVAILABLE` | 必須 |
| TN-002 | prepare | worker/コールバック経路準備、ロールバック経路準備 | `UNKNOWN_ERROR` | 必須 |
| TN-003 | pre-boundary | 同一tune判定 | 無処理成功 | 維持 |
| TN-004 | commit開始 | 旧generation無効化、boundary reset、新backend tune submit | 失敗時は旧tune維持を試す | 努力義務 |
| TN-005 | ワーカー start | tune worker起動 | backend rollbackを試す | 努力義務 |
| TN-006 | rollback成功 | backend旧tune復旧、demux状態維持 | `UNKNOWN_ERROR` | 維持 |
| TN-007 | rollback失敗 | frontend failed、bound demux quarantine | `UNKNOWN_ERROR` | 不可 |
| TN-008 | worker起動成功 | 非同期LOCK/NO_SIGNAL待ち | 成功 | 新tuneへ遷移 |

```mermaid
flowchart TD
    A[validate settings / LNB candidate] -->|fail| B[return error, old tune kept]
    A --> C[prepare ワーカー / callback / ロールバック経路]
    C -->|fail| B
    C --> D{same tune?}
    D -->|yes| E[無処理成功]
    D -->|no| F[boundary reset + backend submit]
    F -->|submit fail| G[rollback old tune attempt]
    F -->|submit ok| H[start tune worker]
    H -->|ok| I[new tune pending]
    H -->|spawn fail| G
    G -->|rollback ok| J[return error, old tune restored]
    G -->|rollback fail| K[frontend failed + bound demux quarantine]
```

### 表20. counter / generation overflow 契約

寿命IDは wrap / saturating reuse を禁止し、`checked_add()` 失敗時に対象を failed / quarantine する。

診断counterは `saturating_add()` を許可する。ただし、上限到達時は `diagnostic_counter_saturated` を記録し、本体データ経路を停止しない。

診断counter overflowを、filter / DVR / demux / frontend の runtime failure に昇格してはならない。診断counterは成功/失敗判定に使ってはならない。

| 分類 | 対象 | 加算規則 | overflow時 | データ経路 への波及 | 禁止事項 |
|---|---|---|---|---|---|
| 寿命ID | filter generation | `checked_add(1)` | filter failed / demux quarantine | あり | wrap / saturating reuse |
| 寿命ID | section generation | `checked_add(1)` | filter failed | あり | wrap / saturating reuse |
| 寿命ID | PES generation | `checked_add(1)` | filter failed | あり | wrap / saturating reuse |
| 寿命ID | source filter origin generation | `checked_add(1)` | source filter failed | あり | wrap / saturating reuse |
| 寿命ID | AV `avDataId` | 正数範囲で `checked_add(1)` | AV経路 failed | あり | 0 / 負数発行、wrap |
| 寿命ID | key token ID | `checked_add(1)` | token発行失敗 | なし、対象token発行だけ失敗 | expired token永久保持、wrap |
| 寿命ID | ワーカー wake generation | `checked_add(1)` | 所有オブジェクト failed | あり | wake generation固定化 |
| 診断counter | malformed packet count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| 診断counter | drop count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| 診断counter | ioctl error count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| 診断counter | queue clear failure count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| debug統計 | dump用累計 | `saturating_add(1)` | saturated表示 | なし | 成功/失敗判定に使う |

| 表示項目 | 値 |
|---|---|
| `counter_value` | `u64::MAX` |
| `counter_saturated` | `true` |
| `last_increment_result` | `Saturated` |
| API戻り値 | 原則として変えない |
| 本体状態 | 維持 |
| 追加診断 | `diagnostic_counter_saturated:<counter_name>` |


## ワーカー abnormal exit と scan terminal state の固定方針

ワーカー `panic` はログ-only にしない。`WorkerRuntime::spawn_owned_with_exit_hook()` / `WorkerHandle::join_from_owner()` が `WorkerExitReason` を返し、`panic` は診断情報と表7・表8で定義した対象状態へ反映する。公開API経路で `stop_tune_worker()` または `stop_live_pump()` が `RuntimeFailure` / `PanicOrJoinFailure` を観測した場合は、表7・表8に従って戻り値と次状態を決め、次の tune / scan / stopTune 処理へ進まない。best-effort 経路では戻り値を返せないが、異常を成功扱いにせず実行時診断へ残す。

scan ワーカー は次の terminal reason を保持する。

```text
Running
Completed
Cancelled
FailedBackend
FailedCallback
FailedPanic
```

scan の normal / stopScan / backend error / コールバック error / `panic` は区別して 診断情報に残す。コールバック 登録済みで scan が開始済みの場合、terminal 時に可能な限り END を送る。ただし END 送信は成功扱いを意味しない。

### scan END 通知失敗の固定

scan END 通知失敗は コールバック失敗 の固定契約であり、Stream boundary / データ経路 failure の正本を変更しない。

scan ワーカー 内の `END` 通知は、`PROGRESS_PERCENT`、`FREQUENCY`、`LOCKED`、`INPUT_STREAM_IDS`、`LOCKED` / `NO_SIGNAL` event と同じく callback 契約の一部として扱う。
`notify_scan_end_with_callback()` の戻り値を `let _ = ...` で捨ててはならない。

- `END` 通知成功時だけ、scan terminal 通知済みとして扱う。
- `END` 通知が callback 未登録または Binder 失敗で失敗した場合、`ScanPhase::FailedCallback` に遷移する。
- 失敗理由は コールバック失敗 診断に記録する。コールバック失敗 だけで `mark_live_path_failed()` を呼んではならない。
- scan ワーカーの最終 `WorkerExit` は コールバック失敗 として診断できる値にする。backend/データ経路 failure と混同しない。
- すでに backend failure / panic failure へ遷移している場合でも、`END` 通知失敗を無視してはならない。追加診断として記録し、`FailedCallback` へ遷移してよい。

この固定は HAL 内部の失敗伝播であり、AOSP AIDL 公開面は変更しない。


## ARIB/ISDB入力処理契約

本書は、Tuner HALが扱うARIB/ISDB入力処理のうち、HAL内に置く処理だけを固定する。

| 項目 | 所有者 |
|---|---|
| 日本向けscan候補表 | TIS |
| channel key / service候補 | TIS |
| 明示選局要求の検証 | Tuner HAL |
| TS packet validation | Tuner HAL `PacketPipeline` |
| section/PES assembly | Tuner HAL `soft_demux` |
| record index | Tuner HAL `soft_demux` |
| MULTI2 payload復号中核 | Tuner HAL descrambler |
| ECM/EMM処理 | CAS HAL / CAS bridge |
| card I/O | CAS HAL / CAS bridge |
| EPG / SI意味解釈 | TIS / arib_si_engine_rs |

Tuner HALは、日本向けscan候補表、BS TSID表、CATV周波数表、service candidate tableを独自生成しない。TISが生成した 明示選局候補 を検証・変換・実行する。

### HAL責務境界

モジュール間の責務境界は `開発規則.md` を正とする。本章の設計は、AOSP Tuner HAL の公開契約に対し、Tuner HAL 内部の寿命、所有権、失敗時状態、配送境界、capability、AIDL戻り値だけを固定する。

## エラー写像 / scan lifecycle / section overflow / DVR close の契約

`IDescrambler`、`IFilter.setDataSource()`、Filter / DVR / Frontend / LNB の状態別 エラー写像 は、本書の「Tuner HAL 状態遷移表SSOT」を正とする。本節では、表セルだけでは表現しきれない診断保持、scan terminal 保存、section overflow 通知、DVR cleanup 補助関数 の補足だけを固定する。

frontend scan lifecycle では、`scan_session` は active `Running` scan だけを表す。`Completed` / `Cancelled` / `FailedBackend` / `FailedCallback` / `FailedPanic` は terminal 診断として `scan_last_terminal` / `scan_terminal_debug` に保存し、保存後は `scan_session` を `None` にする。`stopTune()` は `scan_session.is_some()` を active scan 判定として使い続けるため、terminal scan が残存して `stopTune()` を `INVALID_STATE` にしてはならない。

section assembler が ARIB table 種別別上限を超える section drop または stale partial discard を検出した場合、該当 セクションフィルター の 診断情報 counter を増やし、`pending_overflow` を立てる。コールバック ワーカー は既存 `pending_overflow` 経路を使い、payload が空でも `DemuxFilterStatus::OVERFLOW` を通知する。CRC mismatch と malformed section syntax は filter 条件不成立または section event 不成立として非 delivery を維持し、overflow 状態 へ写像しない。

`DvrHal` の `closed` は外部操作を止める gate であり、cleanup 完了状態ではない。DVR cleanup 完了は `cleanup_complete` で別管理する。通常 cleanup は `close_internal()` の明示 close 経路だけが実行する。Drop および ワーカー失敗 から queue clear、runtime unregister、queue stop、demux unregister の通常 cleanup を best-effort 実行してはならない。Drop は未close診断だけを残し、ワーカー失敗 は runtime failed と callback停止要求だけを記録する。`close_internal()` は step runner を介して コールバック ワーカー stop、queue clear、runtime unregister、queue stop、demux unregister、ledger commit を実行し、全 step が `Success` または `SafeNoOp` と確認できた場合だけ `cleanup_complete=true` とする。

## lab profile のサービス対応

代表ゲートは次の サービス 対応で固定する。

| 系統 | frontend | 周波数 | ONID | TSID | service_id | PMT PID | PCR PID | video PID | audio PID | record PID |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| ISDB-T | `FE_ISDBT_0` | 557142857 Hz | 32736 | 32736 | 1024 | 256 | 272 | 272 | 273 | 272 |
| BS | `FE_ISDBS_0` | 1049480000 Hz | 4 | 16400 | 101 | 256 | 272 | 272 | 273 | 272 |
| CS110 | `FE_ISDBS_0` | 1613000000 Hz | 6 | 0 | 301 | 256 | 272 | 272 | 273 | 272 |

固定 PID は lab profile の代表値であり、実機検証時は同じ サービス 対応表に合わせる。製品 scan では PMT から得た PID を使う。

## BS と CS110 の選局契約

BS は IF 周波数と stream selector を併用する。HAL外部契約では、earth_pt1/DVB backend と px4 backend のいずれも TIS の BS TSID 表から渡された TSID を受け付ける。px4 backend に限り、周波数帯と相対TS番号の併用も受け付ける。px4 backend は BS `STREAM_ID` の TSID 値をそのまま legacy `slot` へ渡し、BS `RELATIVE_STREAM_NUMBER` の相対TS番号値をそのまま legacy `slot` へ渡す。ただし、BS `STREAM_ID` の 0..11 は px4_drv で相対TS番号として解釈されるため受け付けない。earth_pt1/DVB backend は相対TS番号を受け付けず、BS `STREAM_ID` の 0..11 も absolute TSID ではなく相対TS番号レンジとして拒否する。CS110 の実運用は周波数のみで選局し、backend 変換では streamId/relative stream number を使わない。AOSP VTS XML は schema 上 `streamId` と `streamIdType` が必須であるため、VTS用 profile では schema を満たす値を明示するが、これは CS110 の実運用 selector 対応宣言ではない。


## scan / tune の責務分担

この節は Tuner HAL から見た責務分担を説明するものであり、日本向け scan 候補表のSSOTではない。選局対象範囲と除外条件の設計契約は tv 直下の `開発規則.md`、候補表の具体値と実行時候補生成は TIS の実装データを正とする。

Tuner HAL は、TIS が生成した 明示選局候補 を検証・変換・実行するだけであり、日本向け候補表、BS TSID 表、CATV周波数表、サービス candidate table を独自に生成せず保持しない。

日本向け周波数表、CATV周波数表、BS/CS110のTSID表、channel key、サービス検出 の実装データ保持者は TIS とする。選局対象、周波数帯、BS/CS110 selector 境界、CATV 候補範囲の設計契約は tv 直下の開発規則.mdを正とする。Tuner HAL は HAL-generated Japanese scan plan を持たず、TIS が作った explicit candidate を `Tuner.tune()` で受ける。HAL の `scan()` は AOSP/VTS互換の最小実装に限定し、製品の通常 channel scan は TIS の周波数表 + `tune()` ループに寄せる。

TIS が持つ候補範囲は、地上波UHF、CATV、BS、CS110を含める。地上波UHFとCATVは周波数候補をそのまま試す。CS110は周波数帯だけで試し、frontend stream id / relative stream number を要求しない。BSだけは同一周波数に複数TSが存在するため、TIS が持つBS TSID表に含まれる同一IF周波数上のTSID候補をすべて試す。px4 backend は BS `STREAM_ID` の TSID 値と BS `RELATIVE_STREAM_NUMBER` の相対TS番号値をそのまま legacy `slot` へ渡し、BS `STREAM_ID` の 0..11 は拒否する。earth_pt1/DVB backend はTSIDをそのまま `DTV_STREAM_ID` に渡すが、BS `STREAM_ID` の 0..11 は absolute TSID ではないため拒否する。

実行時候補生成では、TIS が持つ BS TSID 表だけを正とする。px4 backend 側に TSID から legacy slot への変換表を持たない。TIS から渡された absolute TSID はそのまま px4 legacy API の `slot` へ渡し、px4 専用の相対TS番号もそのまま `slot` へ渡す。absolute TSID として 0..11 が渡された場合は、全backendで相対TS番号レンジとして拒否する。TSID 直渡しにより、TIS 候補表と px4 側 TSID 表の一致確認は 現行設計上の成立条件から削除する。

この px4 BS `STREAM_ID` direct-slot 契約は、対象 kernel driver が本プロジェクトで採用する px4_drv `feat/android-ddk` 系、すなわち BS legacy `slot >= 8` reject が無効化され、`slot` 値を absolute TSID として `set_stream_id()` へ渡せる実装であることを前提にする。公開 `nns779/px4_drv` develop 相当のように BS `slot >= 8` reject が有効な driver では、absolute TSID direct-slot 経路は使用不可であり、その product で px4 BS `STREAM_ID` 対応を 対応宣言 してはならない。HAL は互換 代替処理 として TSID→relative slot 変換表を復活させない。driver 前提が満たせない場合は、TIS/profile/VTS 設定側で px4 BS absolute TSID 経路を使わない構成にする。

CATV も TIS の製品 scan 候補表に実装データとして追加する。CATV候補表は C13〜C63 に固定する。MID band は C13〜C22、SHB band は C23〜C63 とし、中心周波数は ARIB STD-B21 Appendix 10 の `+1/7 MHz` オフセット込みで保持する。C22 は `167 + 1/7 MHz`、C23 は `225 + 1/7 MHz` であり、C21からC22、C22からC23は単純な6MHz連続として計算しない。地上UHF候補表とCATV候補表はどちらもTIS側が正であり、Tuner HAL はCATV scan planを自前生成しない。TIS はCATV候補を 明示選局候補 としてHALへ渡し、px4 backend は渡されたCATV frequencyをlegacy `freq_no/addfreq` へ変換するだけにする。

この節に現れる UHF、CATV、BS、CS110 の範囲説明は、Tuner HAL の独立した候補表定義ではない。値の更新が必要になった場合は、まず `開発規則.md` の設計契約と TIS の候補表実装を更新し、Tuner HAL 側は 明示選局要求 の validation と backend adapter だけを追従させる。


CATVをスコープに含めるため、TIS の製品 scan table は地上UHFだけを前提にしてはならず、CATV C13〜C63 も候補として保持する。

Tuner HAL 側に置いてよい周波数・サービス関連データは、次に限定する。

- VTS / lab profile 用の代表点
- TIS から渡された 明示選局要求 を backend ioctl へ落とすための backend adapter
- px4 legacy API 用の `freq_no / slot / addfreq` 変換
- 明示選局要求 の validation に必要な最小境界値

これらは product scan candidate table、サービス検出 SSOT、channel display number、BS/CS110 TSID table、TvProvider メタデータの SSOT ではない。製品 scan 候補表、BS/CS110 TSID 表、CATV 中心周波数表、display number、channel key、TvProvider 登録用 メタデータは TIS 側を正とする。

VTS / lab profile は代表点だけでよく、全 CATV 候補の実波存在を VTS pass 条件にはしない。

`Tuner.scan(AUTO_SCAN)` を実装する場合も、HALが日本向け候補列を生成しない。TISが明示した1候補に対する一回限りのscanとして扱い、継続探索はTISが次のcandidateを投入する。


## セクションフィルター / EIT schedule 上限

`numBytesInSectionFilter` は section payload の最大長ではなく、セクションフィルター condition の byte幅として扱う。mask / filter byte 幅は16 bytesを維持する。

`bitWidthOfLengthField` は本製品の TS 入力対象では `0` と `12` だけを受理し、内部的に `12` へ正規化する。その他の値は `INVALID_ARGUMENT` として configure 時点で拒否する。section assembly、CRC、section condition 判定は同じ正規化済み length フィールド width を使い、condition 判定だけが隠れ 12bit 固定になる実装を残してはならない。


EIT schedule を扱うため、section assembler と セクションフィルター delivery は ARIB STD-B10 の table 種別別 section_length 上限に従う。`section_length` は section_length field 直後から CRC_32 末尾までの byte 数であり、section total length は `3 + section_length` とする。EIT p/f と EIT schedule、すなわち table_id `0x4e..=0x6f` は `section_length <= 4093`、section total length `<= 4096` を受け入れる。その他の正式対応 PSI/SI table は `section_length <= 1021`、section total length `<= 1024` を既定上限とする。未分類 table を EIT と同じ 4093 扱いへ拡大してはならない。table 種別別上限を超える section は破損または対象外としてdropし、診断counterへ記録する。

### PSI/SI section CRC_32

CRC_32 は MPEG-2 PSI/SI section CRC_32 を用いる。CRC対象範囲は `table_id` から CRC_32 直前までとし、受信section末尾4 byteを期待CRCとして比較する。

CRC計算の初期値、生成多項式、bit order は ISO/IEC 13818-1 / ARIB STD-B10 の PSI/SI section CRC_32 に従う。

`isCheckCrc=false` の場合でも、section length、reserved bit、syntax構造検証は省略しない。CRC不一致はdelivery不成立であり、queue overflowに写像しない。


PUSI到達時の `pointer_field` は、直前の未完了sectionに対して pointer バイト列の範囲だけを合法なtailとして扱う。pointer bytesで直前sectionが完了しない場合、または `pointer_field == 0` で未完了sectionが残っている場合は、旧partial sectionを新section本文へ連結してはならない。旧partial sectionは破棄し、stale partial discard 診断counterへ記録してから `1 + pointer_field` の位置を新section開始として扱う。


### ARIB section validator 契約

section validator は ARIB table 種別別上限に従う。EIT table_id `0x4e..=0x6f` は `section_length <= 4093`、その他の正式対応 PSI/SI table は `section_length <= 1021` とする。syntaxありsectionでは、`section_length` が header、syntax領域、CRCを格納できる最小長を満たすことを検証する。

section length field 周辺および version byte 周辺の reserved bit は、ARIB / MPEG-TS の reserved bit として検証する。reserved bit が仕様値から外れる section は malformed として扱う。`isCheckCrc=false` の場合でも、長さ・reserved bit・syntax構造の検証を省略してはならない。

## queue overflow / drop 通知方針

internal queue overflow を first-class event として扱う。soft demux 内部 queue、filter delivery queue、DVR record output queue、AV shared buffer、FMQ write のいずれで payload drop または write failure が起きても、無通知破棄 にしてはならない。queue push API は成功、旧データ破棄、新データ破棄、full/backpressure、閉鎖済み を区別できる結果型を返し、破棄バイト数 / drop packets を診断カウンター に必ず反映する。

filter runtime state と DVR runtime state は pending overflow を持つ。コールバック ワーカー は FMQ write failure だけでなく internal queue drop も overflow 通知対象にし、次回 コールバック 周期で `OVERFLOW` / overflow 状態 を必ず上位へ通知する。section / PES / record / DVR raw TS で payload が欠落した場合、上位から欠落を観測できない正常短縮として扱ってはならない。

用途別 drop policy は次で固定する。

| path | 方針 |
|---|---|
| ライブ AV | 低遅延優先。filter queue overflow では古い AV payload の 旧データ破棄 を許容する。ただし overflow event と drop counter は必須。shared memory slot 不足では active slot を eviction せず overflow 診断に落とす。 |
| TS raw | filter FMQ payload は新データ破棄方針とする。古い TS raw payload を暗黙に捨てて時系列を詰めてはならない。 |
| section | 新データ破棄方針とし、overflow event と drop counter を必須にする。EIT / PMT / CAT 等の欠落を上位が検知可能にする。 |
| PES | 新データ破棄方針とし、overflow event と診断カウンター を必須にする。raw PES と ES payload の表現を混在させてはならない。 |
| record metadata event | filter FMQ payload bytes は0とし、entry数上限を持つ新データ破棄方針とする。`TsRecordEvent` 生成用の 188 byte TS packet は metadata として保持し、通常 FMQ watermark / data-size delay の対象にしない。 |
| record / DVR raw TS | 大容量化して極力 drop を避ける。DVR record output queue は新データ破棄方針とし、drop した場合は record 状態 / 診断情報に必ず出す。 |
| DVR playback input | framework producer から playback input FMQ へ書き込まれ、HAL consumer が再注入する入力方向である。HAL 内部の drop-old queue として扱わず、producer-backpressure / no-eviction として model 化する。 |

ライブ AV shared memory slot size と oversized payload の分類は次で固定する。

| 項目 | 固定内容 |
|---|---|
| `AV_SHARED_SLOT_SIZE_BYTES` | 通常product profile では 1 MiB 以上にする。VTS/lab overflow test profile だけ小容量化を許可する |
| slot size と `bufferSize` | AV shared memory slot size は framework が渡す filter `bufferSize` だけから縮小算出しない。product profile の下限を下回ってはならない |
| oversize 診断 | slot size 超過は `DroppedOversizePayload` とし、malformed / empty payload と混同しない |
| overflow 状態 | oversize drop は `pending_overflow` または AV 専用 overflow pending を立て、次 callback 周期で `OVERFLOW` を通知する |
| 通常視聴条件 | 通常product profile では、正常な live AV access unit を 256 KiB 固定値で恒常 drop しないこと |

AV payload delivery result は、少なくとも `Delivered`、`DroppedBeforeHandleExport`、`DroppedNoFreeSlot`、`DroppedOversizePayload`、`DroppedMalformedPayload` を区別する。slot size 超過を `DroppedInvalidPayload` に丸めてはならない。

queue 容量は profile 依存にできる構造にする。VTS/lab profile の小容量で overflow test を行えることと、product profile で record / DVR raw TS を大容量化できることの両方を満たす。overflow 時に古いデータを捨てるか新しいデータを捨てるかは用途別に固定し、ライブ AV の 旧データ破棄 方針を TS raw / section / PES / 録画経路 に流用してはならない。`filter_queue_model()`、`dvr_queue_model()`、`QueuePolicy.overflow_policy`、`QueuePolicy.bounded_entries` はこの用途別方針を診断モデルとしてそのまま表す。未公開リリース候補のため、後方互換目的の alias、boolean 互換 field、旧モデル API は残さず削除する。`QueueOverflowPolicy` を唯一の overflow 方針表現とする。


`QueuePushOutcome` は 受理バイト数、破棄バイト数、破棄要素数、旧データ破棄/新データ破棄、overflow を区別する。filter queue で overflow した場合は runtime state の `pending_overflow` を立て、コールバック ワーカー が payload 有無にかかわらず次周期で `DemuxFilterStatus::OVERFLOW` を通知する。record DVR output queue は 1サービスTS録画 用に 新データ破棄 方針を採り、full 時に新規 TS packet を 無通知破棄 せず `RecordStatus::OVERFLOW` へ伝播する。


## Stream boundary 契約

stream boundary は、次の事象で発生する。

```text
tune
stopTune
scan candidate 切替
frontend close
frontend unbind
setFrontendDataSource
setDataSource
filter flush
DVR flush
filter close
DVR close
descrambler demux/key/PID invalidate
```

boundary処理は `StreamBoundaryTxn` を正本とする。各APIが個別に FMQ、AV shared memory、section/PES assembler、DVR queue、callback generation を直接処理してはならない。

| 対象 | 処理 |
|---|---|
| FMQ | 旧generation payloadは `StreamBoundaryTxn` が破棄する。物理破棄前に観測されるentryはgeneration判定で無効化し、配送しない |
| EventFlag | wake失敗を診断 |
| AV shared memory | 未release slotを診断し再利用可能化 |
| section assembler | 対象origin/PID/generationだけ破棄 |
| PES assembler | 対象origin/PID/generationだけ破棄 |
| continuity tracker | 対象origin/PIDだけreset |
| source filter origin | frontend/playback/source-filterを混在させない |
| record queue | 旧boundary payloadを新boundaryとして扱わない |
| callback | old generationのコールバックを抑止 |

1つのfilter flushが、同じsource origin/PIDを共有する無関係なfilterのassemblerまたはcontinuityを壊してはならない。


#### 表SB-1. 複数 demux boundary 一部失敗表

複数 demux を跨ぐ stream boundary は、全体成功/全体失敗だけでなく、demux 単位の一部成功を第一級状態として扱う。成功済み demux を rollback して通常状態へ戻そうとしてはならない。

| 状態 | 成功demux | 失敗demux | 公開API戻り値 | 後続操作 | 診断 |
|---|---|---|---|---|---|
| 全件成功 | boundary generation 更新済み | なし | `OK` | 通常継続 | boundary success |
| 一部成功・一部失敗 | 成功済みとして維持 | runtime failed + quarantine | Err | 成功demuxは通常継続、失敗demuxはclose/reopen | demux別結果を保持 |
| 全件失敗 | なし | 全件 runtime failed + quarantine | Err | frontendまたは呼び出し元は失敗状態へ遷移 | 全件失敗 |
| demux record lock失敗 | lock成功分だけ処理 | lock失敗demuxは quarantine + 再試行計画保存 | Err | 再試行計画 を close/reopen で消化 | lock failure と対象demux id |
| boundary txn内部失敗 | commit済み分は維持 | 未commit分は failed/quarantine | Err | 成功済みdemuxをrollbackしない。未処理対象は再試行計画またはquarantineへ接続する | failed step と remaining step |


## Packet pipeline 正本契約

### TS resync buffer 末尾 packet 契約

TS resync buffer は、入力末尾に完全な188 byte packetがある場合、次入力のsync byteを待たずにそのpacketを返す。次入力のsync byte確認が必要なのは resync候補が完全packet境界として確定できない場合だけである。

完全な188 byte packetを次入力待ちで保留し続けてはならない。DVR playback / chunked input の最後のpacketを永久保留しないことを設計契約とする。


`PacketPipeline` は、次を正本として持つ。

```text
TS packet validation
source origin
PID continuity
discontinuity
section generation
PES generation
filter delivery generation
flush generation
record index input
```

source origin は次の名前空間で分離する。

| origin | 意味 |
|---|---|
| Frontend | backend live TS |
| PlaybackDvr | playback DVR input |
| SourceFilter(filter_id, generation) | source filterからのraw TS再投入 |

Frontend と SourceFilter を同じ continuity / generation 名前空間に入れてはならない。

malformed TS、TEI、adaptation field不整合、PES header不整合、section長不整合は正常payloadとして配送しない。dropしたpacketを投入成功として数えない。

### record index packet boundary 契約

record index は、TS packet境界をまたぐ以下の構造を検出できなければならない。

| 構造 | 要件 |
|---|---|
| `00 00 01` start code prefix | 3 byte prefixがTS packet境界で分割されても検出する |
| PES header | headerがTS packet境界で分割されても継続解析する |
| PTS field | PTS 5 byteがTS packet境界で分割されても抽出する |
| malformed PES後の復帰 | 次PUSIまたは次start codeから復帰する |

record indexは、現在payload単体だけで完結する前提にしてはならない。必要な最小carry stateをrecord index側が持つ。



### filter delivery delay 条件

`FilterDelayHint` は、コールバック配送頻度を抑制するための遅延ヒントである。media filter には適用しない。

有効な時間条件と有効な byte 数条件が両方ある場合、どちらか一方を満たした時点で配送可能とする。すなわち、time delay と data-size delay は OR 条件である。

| 条件 | 配送可能条件 |
|---|---|
| time delayのみ有効 | 指定時間経過 |
| data-size delayのみ有効 | 指定byte数以上 |
| time delay + data-size delay | 指定時間経過、または指定byte数以上 |
| 両方無効 | delayなし |

時間条件は queue-empty から non-empty へ遷移した payload のまとまりごとに再armする。巨大な時間値は `Instant::checked_add()` で検証し、overflow する値を受理しない。

### unbounded PES の上限超過境界

`PES_packet_length == 0` の unbounded PES は、次の payload unit start indicator 付き TS packet で前PESを完成できる範囲だけ正式対応とする。assembler の保持量が `MAX_PES_BUFFER_BYTES` を超えた場合は 上限超過 PES として当該 PID / 入力元世代キーの PES assembler state を破棄し、診断 counter を増やす。上限超過した PES を配送単位として分割配送する経路は作らない。flush、stop、close、source unlink 境界では未完了 unbounded PES を完成扱いにしない。

## 失敗時状態・境界処理の設計固定

この節は、Tuner HAL の公開 API、soft demux、frontend backend、worker、Filter / DVR close、AV 共有メモリの間で、成功時状態、失敗時状態、再試行条件を一意に固定する。ここに記載する処理は、Tuner HAL の TS packet processing、section assembly、PES / AV / DVR delivery、FMQ / EventFlag、callback、backend I/O、資源寿命 の範囲に閉じる。SI/EIT 意味解析、EPG生成、TvProvider反映、予約追従判断は Tuner HAL の責務ではない。

### TS 入力元と flush 境界

soft demux に入る TS packet の入力元は次の三種類だけとする。

| 入力元 | 意味 | 世代キー |
|---|---|---|
| `Frontend` | frontend backend から来るライブ TS | `Frontend(frontend_generation)` |
| `Playback` | `IDvr.write()` から投入される playback TS | `Playback(dvr_generation)` |
| `SourceFilter` | `IFilter.setDataSource()` により、上流 filter の raw TS 出力を下流 filter へ再投入する TS | `SourceFilter(filter_id, filter_generation)` |

`SourceFilter` は raw TS packet の再投入経路だけを表す。section payload、PES payload、AV payload、record payload を `SourceFilter` 経由で再配送する経路は作らない。上流 filter が raw TS を出力できない種別である場合、`setDataSource()` は接続を拒否する。

section assembler と PES assembler は、上記の世代キー単位で flush generation を保持する。`flush()`、`setDataSource()`、filter close、source unlink、stream boundary reset のいずれかが発生した場合、対象入力元の assembler state と carry state を破棄し、flush generation を更新する。古い generation で組み立て開始された section / PES は配送しない。新しい generation で開始された section / PES だけを配送する。

上流 filter が close、flush、configure、`setDataSource(NULL)`、`setDataSource(別source)` のいずれかで source として使えなくなった場合、当該上流 filter に対応する `SourceFilter(filter_id, filter_generation)` の assembler state と carry state を破棄する。下流 filter の設定は維持してよいが、旧 source 由来の組立途中データを新 source 由来データへ連結してはならない。

本製品の多段 filter は、上流の raw TS filter から `SourceFilter` 経由で raw TS packet を再投入し、下流の TS raw / record filter へ配送する経路だけを正式対応とする。

```text
Frontend / Playback -> raw TS filter -> SourceFilter -> TS raw / record filter
```

この制限は暫定的なリリース範囲ではなく、本製品の正式仕様である。次の経路は非対応とし、`setDataSource()` 時点で `UNAVAILABLE` として拒否する。

```text
section filter -> SourceFilter -> 任意 filter
PES filter     -> SourceFilter -> 任意 filter
AV filter      -> SourceFilter -> 任意 filter
record filter  -> SourceFilter -> 任意 filter
```

### PES assembler の異常系状態表

PES assembler は正常 PES だけを配送対象とする。malformed PES、continuation-only PES、上限超過 PES は配送しない。異常検出時は、当該 PID と入力元世代キーに対応する PES assembler state を破棄し、次の payload unit start indicator 付き TS packet から再同期する。

| 入力状態 | 判定 | assembler 動作 | 配送 |
|---|---|---|---|
| PUSI あり、PES start code 正常 | 新規 PES 開始 | 既存未完了 PES を破棄し、新規 PES を開始 | まだ配送しない |
| PUSI なし、既存 PES あり | continuation | buffer へ追加 | 完成条件を満たせば配送 |
| PUSI なし、既存 PES なし | continuation-only | state 破棄 | 配送しない |
| PES start code 不正 | malformed | state 破棄 | 配送しない |
| optional header marker 不正 | malformed | state 破棄 | 配送しない |
| `PTS_DTS_flags == 0b01` | malformed | state 破棄 | 配送しない |
| PTS / DTS marker bit 不正 | malformed | state 破棄 | 配送しない |
| `PES_packet_length` と header 長が矛盾 | malformed | state 破棄 | 配送しない |
| buffer が `MAX_PES_BUFFER_BYTES` を超過 | oversized | state 破棄 | 配送しない |
| flush / stop / close / source unlink | boundary | state 破棄 | 未完了 PES は配送しない |

`PES_packet_length == 0` の unbounded PES は、映像または音声 PES として受け付ける。ただし、access unit 境界または次の PUSI でのみ完成扱いにする。flush、stop、close 境界で未完了の unbounded PES を完成扱いにしてはならない。

### ワーカー失敗と所有権境界

ワーカー はデータ処理と通知だけを担当し、資源寿命 の所有者ではない。ワーカー失敗 発生時、ワーカー は demux、filter、DVR、descrambler を直接 unregister してはならない。

ワーカー が行ってよい処理は次だけとする。

```text
- runtime failure reason の記録
- 対象 object の ワーカー unhealthy 状態設定
- waiters / コールバック待機 の起床
- 診断 counter の更新
```

cleanup は 公開API object の close 経路に集約する。

playback ワーカー で異常が発生した場合、DVR runtime state を `Failed` に遷移させる。ワーカー は `demux.unregister_dvr()` を直接呼ばない。DVR の demux unregister、queue clear、コールバックワーカー stop、ledger close は `IDvr.close()` の `DvrHal::close_internal()` で行う。

filter コールバックワーカー で異常が発生した場合、filter runtime state を `Failed` に遷移させる。ワーカー は `demux.unregister_filter()` を直接呼ばない。filter の demux unregister、queue clear、コールバックワーカー stop、ledger close は `IFilter.close()` の `FilterHal::close_internal()` で行う。

ワーカー失敗 後の公開 API 動作は次に固定する。

| API | 動作 |
|---|---|
| `start()` | `INVALID_STATE` |
| `stop()` | 停止可能な範囲で停止し、後片付け失敗時は cleanup failed |
| `flush()` | 復旧操作として demux flush、queue boundary clear、AV shared cleanup を可能な範囲で実行する。後片付け失敗時は cleanup failed |
| `read()` / `write()` | `INVALID_STATE` |
| `close()` | 必ず cleanup 経路へ進む。ワーカー失敗 済みでも直接成功扱いしない |

### close / unregister / quarantine 条件

close は、公開 object の lifetime を閉じる唯一の正規経路である。close 中に demux 側 unregister が missing を返した場合、通常は成功扱いしない。missing を成功扱いできるのは、同じ object の runtime failure 経路で事前 unregister 済みと明示記録されている場合だけである。

`IFilter.close()` は次の順序で処理する。

```text
1. FilterLedger begin_close
2. コールバックワーカー stop
3. runtime unregister
4. queue / AV backing clear
5. demux.unregister_filter(filter_id, generation)
6. FilterLedger commit_close
7. cleanup_complete = true
```

`demux.unregister_filter()` が missing を返した場合の動作は次に固定する。

| 条件 | 動作 |
|---|---|
| runtime に `pre_unregistered_by_worker_failure` がある | close 継続可能 |
| それ以外 | cleanup failed / quarantine |

`IDvr.close()` は次の順序で処理する。

```text
1. DvrLedger begin_close
2. playback / record ワーカー stop
3. queue clear
4. demux.unregister_dvr(dvr_id, generation)
5. DvrLedger commit_close
6. cleanup_complete = true
```

`demux.unregister_dvr()` が missing を返した場合の動作は次に固定する。

| 条件 | 動作 |
|---|---|
| runtime に `pre_unregistered_by_worker_failure` がある | close 継続可能 |
| それ以外 | cleanup failed / quarantine |

cleanup failed になった object は quarantine 状態に遷移する。quarantine 状態の object は通常 API では利用不可とする。同じ generation の close retry は許可する。新規 open は同じ ID / generation を再利用しない。

### `IFrontend.stopTune()` の失敗時状態

`IFrontend.stopTune()` は backend tune を停止し、当該 frontend に接続された demux の stream boundary を閉じる操作である。backend stop 後に demux boundary reset が失敗した場合、古いデータが通常配送可能状態として残ってはならない。

`stopTune()` は次の順序に固定する。

```text
1. 対象 frontend に接続された demux 一覧を確定する
2. backend stop を実行する
3. tune ワーカー / scan ワーカー を停止する
4. 各 demux に stream boundary reset を実行する
5. 全 demux reset 成功後、frontend state を Idle にする
```

backend stop 成功後、demux boundary reset が失敗した場合の動作は次に固定する。

```text
- stopTune() は失敗を返す
- backend は停止済みとして扱う
- reset 失敗した demux は quarantine へ遷移する
- quarantine demux の filter / DVR / AV backing は通常配送不可とする
- 該当 demux の close retry は許可する
```

backend 停止済みなのに、旧 FMQ、AV shared backing、DVR queue、packet pipeline が通常利用可能状態として残ることは禁止する。

backend stop が失敗した場合、demux boundary reset は実行しない。frontend state は backend 実状態と一致する状態へ残し、`stopTune()` は backend error を返す。

### AV 共有メモリの原子性不変条件

AV shared backing は、MediaEvent 用 shared memory slot の lifetime を所有する。slot の `active`、`reserved`、`free`、`next_generation` は、一つの原子的状態として扱う。

`clear_result()`、`release()`、`release_all()` は、失敗時に部分更新してはならない。内部状態は次を一つの mutex 配下に置く。

```text
AvSharedState {
  active_slots
  reserved_slots
  free_slots
  next_generation
  diagnostics
}
```

複数 mutex に分けて順次更新してはならない。lock 取得に失敗した場合、状態は呼び出し前から変化しない。

`clear_result()` の動作は次に固定する。

| 条件 | 動作 |
|---|---|
| lock 取得失敗 | Err を返す。状態は不変 |
| lock 取得成功 | active / reserved を空にし、free を全 slot へ戻し、generation を更新して commit |
| generation 枯渇 | Err を返す。状態は不変 |

`release(avDataId)` の動作は次に固定する。

| 条件 | 動作 |
|---|---|
| lock 取得失敗 | Err。状態不変 |
| active に存在しない | `INVALID_ARGUMENT` |
| active に存在する | active から削除し、同一 commit で free へ戻す |
| free 復帰に失敗 | 状態不変で Err |

active から削除した後に free へ戻せない状態は禁止する。`release_all()` は `clear_result()` と同じ原子性を持つ。途中まで slot を戻してから失敗する状態は禁止する。


### TS continuity / adaptation-only packet 固定

- adaptation-only packet は MPEG-TS continuity counter の組立進行条件に含めない。payloadなし packet は continuity tracker の次期待値を進めず、section/PES assembler へ入力しない。
- adaptation-only packet に `discontinuity_indicator` が立つ場合だけ、当該 PID の continuity 状態と section/PES assembler を切断する。


## フィルタ状態破棄境界と遅延通知方針

filter の `stop()`、`flush()`、`configure()`、上流フィルタ登録解除の状態別契約は、本書の「表1. IFilter 状態表」を正とする。本節では、遅延通知の再arm条件だけを補足する。

`FilterDelayHint::時間遅延指定` は queue-empty → non-empty の各まとまりごとに再armする。start/configure直後の1回限りdelayではない。payload queue が空の filter に新規 payload が入った時点で期限を再設定し、最初のまとまり delivery 後に queue が空になった場合、次まとまりは再び time delay を受ける。time delay と data-size delay が両方有効な場合は OR 条件であり、どちらか一方を満たした時点でコールバック配送可能とする。

## CAS と descrambler の境界

CAS HAL / TIS / Tuner HAL のリリース段階ごとのスクランブル解除スコープは `開発規則.md` を正とする。本節では、CAS 本体未接続時でも Tuner HAL の `IDescrambler` AIDL面、key token検証、PID登録、packet単位デスクランブル中核、診断境界をどう扱うかだけを固定する。

復号鍵台帳には、Rust 単体テスト 専用の deterministic トークン と、CAS bridge 接続口から登録された トークン を別 origin として登録する。product 経路で CAS bridge が未接続の場合は、Tuner HAL の key token 解決を 異常時閉鎖済み とし、未登録 トークン、不正 トークン、空 トークン、失効 key slot を復号成功として扱わない。Rust 単体テスト 専用 トークン 登録 API は `#[cfg(test)]` に閉じ、VTS補助関数 や 本番経路 binary から到達できる設計にしない。

## descramble 失敗時 packet policy

対象 PID の descramble に失敗した場合でも、DVR / raw TS 録画経路 では scrambled TS packet を後段へ pass-through してよい。これは録画済み TS を後からデスクランブルできるようにするための意図的な設計である。

ただし pass-through は 平文 成功ではない。packet経路 は少なくとも次を区別する。

- 平文 packet
- descrambled packet
- scrambled pass-through packet
- descramble 失敗 packet

Live/AV経路、診断、recording メタデータ、VTS 判定では、scrambled pass-through を `notifyVideoAvailable()` や 平文 success と混同しない。診断カウンター は `NO_KEY`、`BAD_TOKEN`、`CAS_BRIDGE_UNCONNECTED`、`INVALID_TSC`、`MULTI2_FAIL`、`SCRAMBLED_PASSTHROUGH`、`SCRAMBLED_WITHOUT_DESCRAMBLER` を分離し、debug dump 文字列で demux/PID ごとに観測できるようにする。

## px4_drv ロック 方針

px4_drv は userspace から RF/carrier ロック や demod ロックを個別取得できる API を持たない。開発規則.md の既存方針どおり、px4 backend の `DEMOD_LOCK` は `PTX_SET_SYSTEM_MODE`、`PTX_SET_CHANNEL`、`PTX_START_STREAMING` の tune ioctl 系がすべて成功したことだけを 真値 とする。TS packet 到着、PAT/PMT 到着、AV 到着は px4 frontend の `DEMOD_LOCK` 条件に含めない。

この方針は px4 の frontend 状態 だけの設計であり、視聴可能状態の判定ではない。TIS は `notifyVideoAvailable()` を出す前に、section 到達、PMT/ES PID 解決、AV filter data、decoder/surface の成立を別途確認する。px4 backend は `RF_LOCK` を advertise しない。

## px4_drv chardev open / ライブ TS reader 方針

px4_drv の legacy chardev は同一 device node の二重 open を許さないため、px4 backend は control 用 fd と ライブ TS reader 用 fd を別々に `open()` してはならない。`/dev/px4video*` family は `PTX_SET_SYSTEM_MODE`、`PTX_SET_CHANNEL`、`PTX_START_STREAMING`、TS read を同一 open instance から扱う前提にする。

px4 backend は control fd を一度だけ open し、ライブ TS reader はその `File` を `try_clone()` / fd duplicate 相当で複製して使う。TS pump は nonblocking fd と `poll()` の組み合わせで動かし、reader 作成のために同じ chardev path を再 open しない。これにより、px4_drv の single-open 制約下でも tune 後に ライブ TS、section、AV、record/DVR経路 へ packet を流せることを保証する。

tune / scan ロック timeout は、backend 種別、ISDB-T、BS、CS110 を問わず一律 5 秒に固定する。timeout は非同期 ワーカー 側で扱い、binder method を5秒間占有しない。

## DVR 方針

Tuner HAL は `IDvr`を対応宣言対象とする。DVR は 188-byte MPEG-TS のみを受け入れ、192-byte / 204-byte TS、MMT、TLV は扱わない。DVR record gate は ISDB-T、BS、CS110 のすべてに掛ける。HAL の `IDvr` record / playback 面は完成状態に固定する。

`DemuxCapabilities.numRecord` と `DemuxCapabilities.numPlayback` は、本製品では恒久的に demux 数と同数を広告する。これは HAL 全体で同時に開ける record DVR / playback DVR の最大数であり、各 demux につき同一方向 DVR は1本までとする。別 demux であれば record DVR は demux 数ぶん同時 open 可能、playback DVR も demux 数ぶん同時 open 可能でなければならない。同一 demux 内の record 2本目または playback 2本目は、現在状態による容量超過として `INVALID_STATE` に倒す。

表明する録画範囲は**1サービスTS録画** とする。サービスPID集合の SSOT は TIS に置く。TIS は PMT と サービス検出結果から、PAT、PMT、PCR、video、audio、caption、data、必要な CA 関連 PID を record filter として接続する。Tuner HAL は service_id を理解して record 対象を自動生成しない。HAL は attach された複数 record filter の 188-byte TS packet を、受信 TS順序に近い順序を保って record DVR へ multiplex する。

record filter capacity は32を標準値とする。8 PID 前提の VTS/lab PID-record だけに最適化してはならない。PMT 変更時の PID attach/detach は TIS が行い、HAL は started 中の合法的な attach/detach、重複 attach、detach 後 packet delivery 停止、overflow 通知を state machine として扱う。full transport recording mode は 対応宣言対象外とし、診断または full TS dump feature として追加する場合は別途設計正本へ固定してから扱う。

record DVR / raw TS filter経路 は受信した 188-byte TS packet を製品の録画品質方針として保持する。TEI が立った packet、duplicate continuity counter の packet、scrambled pass-through packet は、録画・診断・後段デスクランブルのために 録画経路 へ到達させる。一方で、section / PES / AV assembly は破損 packet や duplicate packet による二重組み立てを避けるため、TEI packet と duplicate continuity packet を assembly 入力から除外する。これは AOSP が TEI / duplicate の drop/keep policy を明示しているためではなく、日本向け製品の録画品質と parser 安定性を両立するための固定設計である。

DVR playback は 対応宣言対象とする。playback は client から HAL へ TS を入れる入力方向であり、playback injection payload を record/output DVR queue に積んではならない。`inject_playback_payload()` は playback 専用 stats を更新し、playback 起源の TS として demux/filter 入力へ渡すだけにする。frontend/ライブ 起源 TS と playback 起源 TS は routing origin を分離し、playback 起源 TS では direct record filter delivery でも 下流フィルタ propagation でも record DVR mirror を行わない。record/output queue への mirror、record DVR stats の更新、record コールバック の wake は行わない。DVR playback input は producer-backpressure / no-eviction の入力FMQとして扱い、HAL内部で旧playback入力を破棄して新規入力を押し込む drop-old queue とは model 化しない。

playback 専用 stats は少なくとも injected bytes、injected packets、malformed packets、dropped bytes を持つ。malformed TS は drop + 診断 を標準方針とし、1 packet の malformed input で playback stream 全体を fail させない。playback input FMQ の `PlaybackStatus` は start 直後・周期 コールバック ともに playback input FMQ の実 fill / unused write space を唯一の水位 source とし、record/output queue の `queued_bytes` を流用しない。playback consumer ワーカー は `WorkerHandle` / owner `ConcreteWorkerSignal` に接続し、close / Drop / 異常時閉鎖済み で `request_stop()` → `wake()` → `join_from_owner()` の順に停止する。

playback input FMQ の stream 境界 方針は次のとおり固定する。start 前に client が prefill した bytes は保持し、start 後に playback TS として読む。started=false 中は ワーカー が FMQ を読まない。stop 時は playback input FMQ と packet assembler residual を維持し、次 start で既存 stream の続きとして読む。flush 時は playback input FMQ と packet assembler residual を drain/discard し、dropped bytes 診断カウンター と ログ に記録する。flush 後に client が新たに書いた bytes は started=false 中には読まず、直前の flush で既存 stream 境界が drain 済みであることを前提に、次 start の prefill として扱う。playback flush は playback input FMQ、packet assembler、playback stats だけを reset し、record/output queue を破壊しない。record DVR flush は record output queue と record stats だけを reset し、playback input queue と playback stats を破壊しない。


### playback consumer commit（消費確定）表

本表は `ConsumedNoDelivery` と内部注入失敗を混同しないための補足である。DVR playback は入力方向のFMQであり、filter未接続や未startedによる配送先なしを即時致命失敗にしてはならない。一方で、TS parse後の内部注入処理そのものが失敗した場合は、FMQ read 済み入力を成功消費扱いにしてはならない。

| 入力状態 | FMQ read | TS parse | 注入結果 | 消費扱い | public/diagnostic |
|---|---|---|---|---|---|
| valid TS + delivery成功 | 成功 | 成功 | `Consumed` | 消費済み | 正常 |
| valid TS + delivery先なし | 成功 | 成功 | `ConsumedNoDelivery` | 設計上、診断付き消費済み可 | no delivery diagnostic。filter未接続/未startedをfatalにしない |
| valid TS + filter未started | 成功 | 成功 | `ConsumedNoDelivery` | 診断付き消費済み可 | playback入力をqueue内で無期限保留しない |
| malformed TS | 成功 | malformed | `MalformedOnly` | 消費済み可 | malformed diagnostic。1 packet でstream全体をfailしない |
| partial TS | 成功 | pending | 未commit | residual保持 | 次readへ持ち越し |
| 内部注入失敗 | 成功 | 成功 | Err | 消費済みにしない | playback runtime failed または retryable failure |



### playback consumer ワーカー 起動順序

DVR playback consumer ワーカー は、DVR が soft demux と `RuntimeIoRegistry` の両方へ登録され、queue と ワーカー signal の所有権が `DvrHal` へ確定した後にだけ開始する。登録前に playback ワーカー が DVR state を観測してはならない。

ワーカー生成 後に registry commit する構造は禁止する。spawn 後に後段登録が失敗した場合は、ワーカー stop / join、queue cleanup、soft demux unregister、ledger rollback を一体で行う。

## Frontend capability / 状態 方針

ISDB-T / ISDB-S の frontend capability bitmask は Android 14 AIDL enum 名に基づく固定値とする。ISDB-T は `AUTO | MODE_3`、`AUTO | BANDWIDTH_6MHZ`、`AUTO | MOD_DQPSK | MOD_QPSK | MOD_16QAM | MOD_64QAM`、`AUTO | CODERATE_1_2 | CODERATE_2_3 | CODERATE_3_4 | CODERATE_5_6 | CODERATE_7_8`、`AUTO | INTERVAL_1_32 | INTERVAL_1_16 | INTERVAL_1_8 | INTERVAL_1_4`、`AUTO | INTERLEAVE_3_0 | INTERLEAVE_3_1 | INTERLEAVE_3_2 | INTERLEAVE_3_4` を advertise する。ISDB-S は `AUTO | MOD_BPSK | MOD_QPSK | MOD_TC8PSK` と `AUTO | CODERATE_1_2 | CODERATE_2_3 | CODERATE_3_4 | CODERATE_5_6 | CODERATE_7_8` を advertise する。

`RF_LOCK` は backend が RF/carrier acquisition を別途取得できる場合だけ advertise する。DVB / earth_pt1 backend は Linux DVB `FE_READ_STATUS` が返す `FE_HAS_CARRIER` を `RF_LOCK`、`FE_HAS_LOCK` を `DEMOD_LOCK` に対応させる。px4_drv backend は RF/carrier ロックを返す API を持たないため、px4 の擬似 ロック は `DEMOD_LOCK` のみに使い、`RF_LOCK` には使わない。

`SNR` と `SIGNAL_STRENGTH` は、現行Tuner HAL capability / VTS profile では `statusCaps` に含めない。DVB / earth_pt1 の `FE_READ_SNR` と `FE_READ_SIGNAL_STRENGTH`、px4 の `PTX_GET_CNR` は target driver / device 状態によって read 時に失敗し得る optional telemetry であり、起動時列挙時点で frontendエントリ の固定 capability として証明できないためである。これらの optional telemetry は 診断内部値として保持してよいが、AOSP statusCaps 上の supported 状態として advertise してはならない。

`SIGNAL_QUALITY` は、backend ごとに根拠ある合成値を返せる場合だけ `statusCaps` に含める。DVB / earth_pt1 backend の `SIGNAL_QUALITY` は Linux DVB `FE_READ_STATUS` 状態 bit の ロック 進捗を 0〜100 に正規化した値とする。px4 backend は `PTX_GET_CNR` を安定取得できることを frontendエントリ の capability として固定できない限り、`SNR` と `SIGNAL_QUALITY` を advertise しない。いずれも `DEMOD_LOCK` や `RF_LOCK` の代替ではなく、UI/診断 用の合成指標である。未取得 telemetry を `SIGNAL_QUALITY=0` として成功返却してはならない。


### frontend settings validation の固定方針

Tuner HAL が advertise する frontend capability は、Android 14 AIDL enum 名に基づく固定 bitmask とする。capability 値の詳細は本書の「Frontend capability / 状態 方針」を正とし、本節では重複定義しない。

public `FrontendSettings` validation は、advertised capability と矛盾してはならない。`AUTO` だけを受け付け、advertise 済みの具体 enum 値を拒否する実装は禁止する。

explicit 範囲スキャン は ISDB-T / ISDB-S 共通で対応宣言しない。`endFrequency` が `frequency` と異なる場合は、共通 validation で `UNAVAILABLE` とする。

### ISDB-T validation

- `bandwidth` は `AUTO` または `BANDWIDTH_6MHZ` だけを受け付ける。
- `mode` は `AUTO` または `MODE_3` だけを受け付ける。
- `modulation` / `coderate` / `guardInterval` / `timeInterleave` は、advertised capability に含まれる値だけを受け付ける。
- `timeInterleave` は mode 3 用の `INTERLEAVE_3_0`、`INTERLEAVE_3_1`、`INTERLEAVE_3_2`、`INTERLEAVE_3_4` だけを受け付け、mode 1 / mode 2 用の値は拒否する。
- ブラインドスキャン は `UNAVAILABLE` とする。

対象 driver は modulation / coderate / guard interval / time interleave を userspace から細かく強制設定するモデルではない。したがって、これらの具体値は「driverへ個別プログラムする knob」ではなく、Android 14 AIDL 上 advertise した運用可能値として検証する。backend は frequency / bandwidth / stream selector を主入力として tune し、demod の自動検出に委ねる。

### ISDB-S validation

- `modulation` は `AUTO`、`MOD_BPSK`、`MOD_QPSK`、`MOD_TC8PSK` だけを受け付ける。
- `coderate` は `AUTO`、`CODERATE_1_2`、`CODERATE_2_3`、`CODERATE_3_4`、`CODERATE_5_6`、`CODERATE_7_8` だけを受け付ける。
- public settings の `symbolRate` は `0` / 未指定相当のみ成功とする。
- BS は `streamId` を必須とする。
- CS110 は stream selector を指定してはならない。
- ブラインドスキャン は `UNAVAILABLE` とする。

共通 validation は binder 層の `settings_to_request()` に集約し、backend 固有 validation は `Px4FrontendBackend::validate_tune_request()` / `DvbFrontendBackend::validate_tune_request()` を通す。public `tune()` / `scan()` は validation 済み request だけを backend へ渡す。


## ライブ AV filter / FMQ 方針

ライブ AV filter を正式スコープに含める。本製品のライブ AV filter は non-passthrough の `MediaEvent` + 共有メモリ + `dataId` 経路だけを正式対応とする。AVペイロードは通常FMQへ書き込まない。EventFlag は FMQ対象経路の通知にだけ使い、AVペイロードの格納先として扱わない。

AV passthrough は本製品では恒久的に対応しない。`DemuxFilterAvSettings.isPassthrough=true` は configure 時点で `UNAVAILABLE` とし、passthrough capability は宣言しない。成功扱いの無処理 または無配送の AV filter として受け入れてはならない。

VTS/profileでは、AV filterを使用する場合でも `isPassthrough=false` に固定する。`isPassthrough=true` を含むprofileは本製品の対応profileとして扱わない。

AV filter の状態別契約、shared backing、公開済みハンドル、使用中領域、`dataId`、`releaseAvHandle()`、`flush()`、`configure()`、`close()` の副作用は、本書の「表4. AV共有メモリ資源寿命表」を正とする。本節では、allocator、NativeHandle形式、payload配置、診断方針だけを補足する。

Android 14 系 framework/JNI が受理する `MediaEvent` + 共有ハンドルは、Codec2 が `MediaCodec.LinearBlock` として import できる ION / dma-buf 系共有メモリ fd を `NativeHandle` の先頭 fd として持つ形式に固定する。`IFilter.getAvSharedHandle()` は1個の fd を持つ `NativeHandle` と共有メモリ総サイズを返す。shared handle方式では、各 `DemuxFilterMediaEvent.avMemory` は empty handle とする。通常の AV payload delivery に対応する `avDataId` は、0 以外の領域寿命 ID とする。`avDataId == 0` は shared handle lifetime / release 通知用に予約し、AV payload delivery には使わない。`offset` と `dataLength` は、共有メモリ内の AV access unit 範囲を示す。payload 範囲は半開区間 `[offset, offset + dataLength)` として扱う。そのため、有効な範囲条件は `offset + dataLength <= shared memory total size` である。`offset + dataLength == shared memory total size` は、共有メモリの最後の byte までちょうど使用する正常境界である。範囲外として拒否するのは、`offset + dataLength > shared memory total size` の場合である。Android 14 framework/JNI では、`avDataId == 0` かつ event 内 fd なしの fallback 経路に限り `dataLength + offset < avSharedMemSize` が native context 作成条件に含まれる。本 HAL の通常 AV payload delivery は `avDataId != 0` に固定するため、この fallback 条件を通常 payload の境界条件として扱わない。`releaseAvHandle(fd付き handle, 0)` と `releaseAvHandle(empty handle, 0)` は shared backing、公開済みハンドル、既存 `dataId`、使用中領域を破棄しない。zero-length AV payload は MediaEvent として出さず、`DroppedMalformedPayload` / `av_malformed_payload` として診断し、`OVERFLOW` 状態に反映する。slot size 超過 payload は `DroppedOversizePayload` / `av_oversize_payload` として診断し、malformed payload と同じ診断名に丸めない。平文メディア経路であり、`isSecureMemory=false` に固定する。

### AV shared handle の `NativeHandle` 形式

| 項目 | 固定値 | 理由 |
|---|---|---|
| fd数 | 1 | shared backing fd を framework/JNI へ渡すため |
| ints数 | 1 | Android framework/JNI が参照する memory index だけを公開するため |
| `ints[0]` | 0 | 単一 shared memory の index。HAL内部識別子ではない |
| `ints[1..]` | 出さない | HAL内部識別子を framework/JNI へ公開しないため |
| `slot_size` / `slot_count` | 出さない | HAL内部の領域管理値であり、`NativeHandle.ints` ではないため |
| magic / generation / filter id | 出さない | JNI が int を memory index として読むため |

### 利用者側 AV handle 使用終了後の AV payload 処理

| 状態 | AV payload 到着時の動作 |
|---|---|
| shared handle export済み + client release未済み | shared memory に配置し、`MediaEvent` を出す |
| shared handle export済み + client release済み | `MediaEvent` を出さず破棄し、`av_shared_handle_client_released_drop` を増やす |
| shared handle export未済み | `MediaEvent` を出さず破棄し、`av_drop_unexported` を増やす |
| `getAvSharedHandle()` 再取得後 | client release未済みに戻し、配送を再開可能にする |

## A/V sync 方針


### AV sync hardware ID 所有契約

AV sync hardware ID は `filter_id & 0xffff` から導出しない。demux 内の `filter_id -> hw_id` と `hw_id -> filter_id` の双方向表で固定し、filter ID 65536周期の衝突を禁止する。

filter unregister、non-AV configure、AV filter close、demux close では、双方向表の両方向を同一commitで削除する。片方向だけ残る場合は demux の AV sync 状態を通常状態として扱わない。


AV filterを対応宣言する demux は AOSP の `getAvSyncHwId(Filter)` と `getAvSyncTime(int)` の契約に沿って A/V sync ID と 90kHz timestamp を返す。`getAvSyncHwId()` は同一 demux 内の audio/video main filter にだけ deterministic ID を返し、section、PES、record、閉鎖済み filter には `UNAVAILABLE` を返す。

`getAvSyncHwId()` は、対象 filter が audio/video main filter であり、かつ soft demux が PCR 由来の source clock を既に保持している場合だけ sync ID を返す。AOSP CTS は `INVALID_AV_SYNC_ID` を許容する一方、valid ID を返した場合は `getAvSyncTime(id)` が valid timestamp を返すことを期待するため、PCR 未観測時に valid ID を先出ししない。

`getAvSyncTime()` は sync ID が指す AV filter を検証し、soft demux が最後に観測した PCR base を基準に、観測時点からの経過時間を 90kHz clock に換算して加算した current timestamp を返す。PCR が未観測の場合は PTS を代用せず `UNAVAILABLE` を返す。PTS は presentation timestamp であり、AOSP が要求する current A/V sync clock の代替にしない。PCR の 33-bit wrap は内部で extended 90kHz 値へ伸長して単調性を保つ。

## A/V sync 非採用範囲

AV filter の `start()`、共有ハンドル、MediaEvent、`releaseAvHandle()` の状態別契約は、本書の「表4. AV共有メモリ資源寿命表」を正とする。本節では A/V sync の現行境界と非採用範囲だけを固定する。

- A/V sync は、PCR が未観測であれば 有効な同期ID を返さない。有効な同期ID を返す場合は、`getAvSyncTime(id)` が valid 90kHz timestamp を返せる状態に限る。
- PTS は current A/V sync clock の 代替処理 として使わない。
- PCR と monotonic clock の対応付けによる最小 wallclock 補間は維持する。
- PCR PID 明示管理、サービス clock、jitter smoothing、PLL / clock discipline を追加する場合は、clock source、reset 条件、戻り値、診断、実機確認条件を本書へ固定してから扱う。

以下は現行実装範囲外にする。

- PCR PID 明示管理。
- サービス clock モデル。
- jitter smoothing。
- PLL / clock discipline。
- 複数 clock source の品質評価。
- より厳密な CTS / VTS / 実波ベース補正。

## LNB 固定 profile

対象driverの大枠は `開発規則.md` を正とする。本節では Tuner HAL の LNB capability / profile / AIDL戻り値だけを固定する。px4_drv 系で LNB 電源を成功扱いにするのは、対応デバイス仕様で 15V 出力が確認できる `px4video*` family のみとし、`pxmlt5video*`、`pxmlt8video*`、`isdb6014video*` は安全側に倒して `NONE` のみ成功にする。earth_pt1 系は `NONE`、`11V`、`15V` だけを受け付ける。tone、DiSEqC、satellite position switching は恒久的に未対応であり、`POSITION_UNDEFINED` 以外の satellite position、tone ON、自動 tone、DiSEqC message は `UNAVAILABLE` とする。LNB 用の汎用 DVB profile は作らない。

LNB は satellite frontend の所有物として扱い、shared LNB の余地は置かない。`setLnb(lnb_id)` は当該 satellite frontend に紐付いた LNB ID だけを受け付け、別 frontend の LNB ID、地上波 frontend への LNB attach、不明な LNB ID は失敗させる。

`ILnb.setCallback(non-null)` は、受け取ったコールバック実体を `LnbHal` 内に保持する。再設定時は新しいコールバック実体で置換する。`ILnb.close()` と未閉鎖 `LnbHal` の破棄経路では保持中のコールバック実体を解放する。現行 Android 14 AIDL Rust 生成境界では `setCallback(null)` を Rust HAL 公開メソッドで受ける実装方式がないため、null解除を実装済み扱いにしない。nullable callback 境界は 本書の「nullable Binder 境界」節を正とする。AOSP frozen/stable AIDL の vendor 独自改変、生の Binder transaction 解析器による公開契約を通さない実装は採用しない。

`setVoltage()`、`setTone()`、`setSatellitePosition()` は `update_lnb_state()` を唯一の状態更新入口にする。LNB状態更新は registry を先に変更しない。旧状態から新状態候補を作り、frontend backend への反映が成功した場合だけ registry を新状態へ更新する。backend 反映に失敗した場合は registry を変更せず、backend rollback apply は行わず、`UNKNOWN_ERROR` と `lnb_backend_apply_error` 診断へ落とす。これにより HAL内部台帳と実 backend 状態の二重 rollback 失敗を作らない。

`ILnb.close()` は終了時の初期化戻しとして扱う。公開 `close()` はコールバックを消すだけでは成功扱いにせず、LNB 台帳の voltage を `NONE`、tone を `NONE`、satellite position を `UNDEFINED` に戻し、世代番号を進め、当該 LNB を選択中の frontend backend へ初期化戻し状態を反映してから閉鎖済み状態を確定する。初期化戻しの反映に失敗した場合は `close()` を成功扱いにしない。binder death などの明示的な所有者喪失処理も `LnbLifecycleTxn::close_from_owner_loss()` 相当の lifecycle 正本を通り、公開 `close()` と同じ安全状態反映を試みる。Rust `Drop` は AOSP 公開契約ではなく戻り値を返せない最後の安全網であるため、通常cleanupや backend 初期化戻しの代替にしない。未閉鎖の `LnbHal` が破棄された場合、Drop は `DropLeakTxn::record_unclosed_drop(ResourceKind::Lnb)` 相当の未close診断、quarantine、コールバック参照の局所clear だけを行い、`safe_state_for_close()`、backend apply、registry safe commit を呼ばない。


### LNB 状態更新の失敗時整合性

LNB backend へ新状態を適用した後に registry commit が失敗した場合、HAL は backend rollback apply を行わない。registry を成功扱いで新状態へ進めず、当該 LNB を `quarantined` または `failed` に固定し、通常の `setVoltage()` / `setTone()` / `setSatellitePosition()` を拒否する。以後は `close()` / cleanup 経路だけを許可し、close で voltage none、tone none、position undefined の安全状態を backend へ再投入してから registry 解放を試みる。registry commit 失敗後に通常操作を継続させて backend 実状態と HAL 台帳の乖離を隠してはならない。旧状態 rollback と安全状態再投入を二重に試す構造は作らない。


## 復号鍵台帳

`IDescrambler.setKeyToken()` が受け取る値は復号鍵そのものではなく、不透明な参照値である。Tuner HAL はこの参照値で復号鍵台帳を引き、内部の `DescramblerKeySlot` に変換する。Binder 境界を越える バイト列に MULTI2 の system key、CBC 初期値、偶数鍵、奇数鍵を入れてはならない。

復号鍵台帳の key slot 状態は次で固定する。

| 状態 | 意味 | resolve結果 | 復号可否 | 設計上の成立条件 |
|---|---|---|---|---|
| `Registered` | CAS bridge または test 専用登録により、内部鍵参照が有効である。refcount は 0 以上 | 成功 | 可 | `setKeyToken()` が acquire ref に成功し、packet経路 が key slot を参照できる |
| `Unknown` | 台帳に存在しない token。未登録、refcount 0 到達による削除、refcount 0 の未使用 slot revoke 済みを含む | `UnknownToken` | 不可 | 削除済み token を復号可能として扱わない |
| `RegistryUnavailable` | 台帳 lock 失敗、内部状態破損、CAS bridge registry 不在などで解決不能 | `RegistryUnavailable` または AIDL `UNKNOWN_ERROR` 相当 | 不可 | 内部障害を復号成功にしない |

現行の key token table は persistent `Expired` slot を保持しない。通常 release により refcount が 0 になった token slot は削除する。CAS bridge 側の session close / service switch / PMT 変更 / 明示 revoke は、refcount 0 の slot だけを削除し、refcount > 0 の slot は active session が release するまで保持する。削除済み token を後から `setKeyToken()` された場合は `UnknownToken` とする。`ExpiredKeySlot` は stale release / refcount underflow 検出用の診断名であり、通常の `setKeyToken()` resolve 結果として要求しない。

## デスクランブル gate

VTS/lab config には descrambling flow を置かない。VTS 用 XML に ECM filter や `<descramblers>` を生成せず、平文ライブ視聴 / DVR / 明示選局 の接続確認に限定する。Tuner HAL は PMT/CAT/SDT/ECM/EMM 等の section payload delivery、`IDescrambler`、`setKeyToken()`、`addPid()` / `removePid()`、トークン lookup 境界、未接続・bad トークン・expired トークン 診断までを確認対象とする。本番経路スクランブル解除成功のリリーススコープと、CA情報 / サービス メタデータの意味解析、ECM/EMM filter 開始方針、MediaCas/CAS bridge 呼び出し、不透明な参照値の取得試行、Tuner descrambler への接続判断、未接続診断の上位制御の責務境界は `開発規則.md` を正とする。Tuner HAL の packet 単位のデスクランブル中核は、単体テスト内で復号鍵台帳へ既知鍵を登録して確認する。


## IDescrambler optionalSourceFilter 境界

AOSP意味論では、`IDescrambler.addPid(pid, optionalSourceFilter)` および `removePid(pid, optionalSourceFilter)` の `optionalSourceFilter == NULL` は demux input 全体に対する PID 登録 / 解除である。ただし 現行 Android 14 AIDL Rust 生成境界では public HAL method が non-null `Strong<dyn IFilter>` を要求するため、NULL経路は実装済み対象に含めない。non-null 経路は指定 filter output、すなわち upper stream を対象にした PID 登録 / 解除であり、source filter検証後に成功対象とする。nullable Binder 境界は本書の「nullable Binder 境界」節を正とし、現行実装済み対象に含めない。

### 表D-1. IDescrambler PID 操作表

| No | API | source filter | 条件 | AIDL戻り値 | 副作用 | 設計上の成立条件 |
|---:|---|---|---|---|---|---|
| DS-001 | `addPid(pid, NULL)` | なし | AOSP意味論上の正式入力 | 現行 Android 14 AIDL Rust 生成境界では到達不能 | なし | `本書の「nullable Binder 境界」節` で管理する。実装済み対象に含めない |
| DS-002 | `addPid(pid, filter)` | あり | filter が同一 demux、非閉鎖、generation 有効、pid valid | 成功 | source filter に紐づく PID として登録 | source filter id と generation を保存する |
| DS-003 | `addPid(pid, filter)` | あり | filter が別 demux、foreign filter、dangling filter、閉鎖済み | `INVALID_ARGUMENT` | なし | 不正 source filter を登録しない |
| DS-004 | `addPid(pid, filter)` | あり | invalid PID | `INVALID_ARGUMENT` | なし | PID 範囲外を登録しない。NULL経路は現行実装対象外 |
| DS-005 | `addPid(pid, filter)` | あり | descrambler 閉鎖済み、demux 未設定、別 active descrambler が同一 demux generation / PID を所有 | `INVALID_STATE` | なし | 状態衝突を引数不正として扱わない。key token 未設定は PID 登録拒否条件ではない。NULL経路は現行実装対象外 |
| DS-006 | `removePid(pid, NULL)` | なし | AOSP意味論上の正式入力 | 現行 Android 14 AIDL Rust 生成境界では到達不能 | なし | `本書の「nullable Binder 境界」節` で管理する。実装済み対象に含めない |
| DS-007 | `removePid(pid, filter)` | あり | 登録済み source-filter 紐づき PID | 成功 | 紐づく PID 登録を解除 | source filter id と generation が一致する登録だけ解除する |
| DS-008 | `removePid(pid, filter)` | あり | 未登録 PID | 成功 | なし | cleanup として冪等成功にする。NULL経路は現行実装対象外 |
| DS-009 | `removePid(pid, filter)` | あり | invalid PID | `INVALID_ARGUMENT` | なし | PID 範囲外を解除対象にしない。NULL経路は現行実装対象外 |
| DS-010 | `addPid()` / `removePid()` | あり | unsupported `DemuxPid` variant | `UNAVAILABLE` | なし | product capability 未対応に限定する。NULL経路は現行実装対象外 |

同一 descrambler 内では PID 登録表の主キーを PID とし、同一PIDに対する `addPid(pid, sourceFilter)` は既存登録を新しい source filter generation で置換する。別 descrambler 間では、同一 demux / demux generation / PID を二重に復号対象へ登録しないため、既に他の active descrambler が同一PIDを保持している場合は `INVALID_STATE` とする。

エラー写像:
- `INVALID_STATE`: descrambler 閉鎖済み、demux 未設定、demux generation 消失、再検査時 state 不整合、別 active descrambler による同一 demux / demux generation / PID 所有衝突。key token 未設定は `addPid()` / `removePid()` の `INVALID_STATE` 理由にしない。
- `INVALID_ARGUMENT`: invalid PID、foreign filter、別 demux filter、not-open / dangling local filter handle、閉鎖済み source filter。
- `UNAVAILABLE`: unsupported `DemuxPid` variant、product capability 未対応に限定する。

## DVB backend の対応表

DVB backend は frontend index と同じ demux index / dvr index を使う。`adapterN/frontendM` は `adapterN/demuxM` と `adapterN/dvrM` に対応する。demux が別 frontend の TS を読む構成は advertise しない。source 選択 ioctl が失敗した場合は tune / scan / record を成功扱いにしない。

## 診断可観測性の固定

本番経路トークンの用語、リリース段階、TIS から `setKeyToken()` へ渡してよい値のスコープは `開発規則.md` を正とする。本節では、Tuner HAL が受け取ったトークンの検証、AIDL戻り値、診断、副作用だけを固定する。`register_from_cas_bridge()` は CAS bridge 接続口であり、非 test product 経路からの到達可否は `開発規則.md` の本番経路スコープに従う。

`IDescrambler.setKeyToken()` に到達する non-VOID トークン は、HAL key token table が発行した 8 byte の opaque byte array だけを有効とする。Android 14 系の `Tuner.VOID_KEYTOKEN` は 1 byte トークン `[0x00]` として扱い、current key removal 用の有効 トークン とする。空 トークン `[]` は VOID トークン ではなく、常に `INVALID_ARGUMENT` と内部診断 `BAD_TOKEN` に落とす。non-VOID で 8 byte 以外の トークン は registry lookup 前に `INVALID_ARGUMENT` / `BAD_TOKEN` とする。

`maleicacid-cas-desc-token-*`、`maleicacid-placeholder-desc-token*`、既存 TIS 側の `maleicacid-kari-token-*` は、設計文書上の診断名またはログ上のラベルであり、Tuner SDK API 経由で渡す実 トークン ではない。単体テスト、fake CAS、診断注入で同等のケースを表現する場合も、`setKeyToken()` に渡す non-VOID byte array は HAL key token table が発行した 8 byte fixed テストトークン とし、長い診断名は テストケース 名、lookup table の説明、診断 dump の表示名に限定する。

これらの診断 トークン origin を受け取った場合は、復号成功ではなく `CAS_BRIDGE_UNCONNECTED`、`BAD_TOKEN`、`EXPIRED_KEY_SLOT` など該当する診断へ落とす。

`IDescrambler.setKeyToken()` は、最初に `[0x00]` を `Tuner.VOID_KEYTOKEN` として処理し、registry lookup に流さず current key slot のみ解除する。PID 登録は維持する。次に空 トークン `[]` と 8 byte 以外の non-VOID トークン を registry lookup 前に拒否し、`INVALID_ARGUMENT` と内部診断 `BAD_TOKEN` に固定する。8 byte だが未登録の トークン と CAS bridge 未接続 トークン は通常 トークン として registry lookup 後に区別して診断する。診断を通さない トークン 解決 API は 本番経路へ公開しない。

`IDescrambler.setKeyToken()` の失敗時は、現在の鍵スロット、現在のトークン、demux 紐付け、PID登録を変更しない。空 トークン、長さ超過、未登録、失効済み、台帳異常のどれで失敗しても、成功扱いにせず固定された AIDL 戻り値と診断だけを返す。PID 登録を消す操作は `removePid()` だけであり、`VOID_KEYTOKEN` と 鍵参照の解決失敗は PID 登録削除を伴わない。

デスクランブル診断は、`dump_descrambler_diagnostics_for_debug()` の dump 文字列と `maleicacid-tuner-hal-descrambler-diagnostic` ログで観測する。dump には demux、PID、`CLEAR_PACKET`、`DESCRAMBLED`、`SCRAMBLED_PASSTHROUGH_FOR_RECORDING`、`MALFORMED_PACKET_FOR_RECORDING`、`DESCRAMBLE_FAILED`、`INVALID_PACKET_SIZE`、`BAD_SYNC_BYTE`、`INVALID_AFC`、`INVALID_ADAPTATION_FIELD`、`INVALID_TSC`、`SCRAMBLED_WITHOUT_PAYLOAD`、`NO_KEY`、`BAD_TOKEN`、`CAS_BRIDGE_UNCONNECTED`、`EXPIRED_KEY_SLOT`、`MULTI2_FAIL`、`SCRAMBLED_WITHOUT_DESCRAMBLER` を含める。`SCRAMBLED_PASSTHROUGH_FOR_RECORDING` は後段デスクランブル可能な録画 TS を残すための pass-through であり、平文 成功を意味しない。malformed / undefined な TS-frame-like packet の録画保存は `MALFORMED_PACKET_FOR_RECORDING` で別管理し、`InvalidPacketSize` / `BadSyncByte` は record-DVR raw TS に保存しない。

`MALEICACID_TUNER_HAL_DESCRAMBLER_DIAGNOSTIC_FILE` を設定した デバッグビルドまたは立ち上げ検証環境では、Tuner HAL サービスが 5 秒間隔で同じ descrambler 診断 dump を指定ファイルへ書き出す。Stable AIDL には vendor 独自メソッドを追加しない。


### 失効 トークン 診断

`maleicacid-expired-desc-token-*` は診断名であり、`setKeyToken()` に渡す実 トークン ではない。現行仕様では persistent expired state を持たないため、失効または revoke 済み token の `setKeyToken()` は unknown token として扱う。`EXPIRED_KEY_SLOT` は stale release / refcount underflow 検出用の診断名としてだけ使う。

`setKeyToken()` は、空 トークン、8 byte 以外の non-VOID トークン、未登録 トークン、CAS bridge 未接続 トークン を区別して診断カウンターに記録する。`[0x00]` は `Tuner.VOID_KEYTOKEN` として扱い、`BAD_TOKEN`、unknown トークン、CAS bridge 未接続には混ぜず、key 未設定状態でも 成功扱いの無処理 とする。空 トークン `[]` は registry lookup、current key slot 変更、PID 登録変更を行わない。

## B25 packet デスクランブル中核の範囲

現行 Tuner HAL は、libaribb25 相当の B25 全体実装であるとは主張しない。Tuner HAL に実装済みなのは、188 byte TS packet の payload に対する MULTI2 復号中核、odd/even key 選択、adaptation フィールドを壊さない payload offset 判定、復号成功時の scrambling_control 正規化、復号失敗時の録画向け scrambled pass-through 診断である。

### MULTI2 / B25 境界

Tuner HAL の descrambler は、key token で与えられた鍵を用いて、188 byte TS packet の payload 部分だけを復号する。

| 項目 | 契約 |
|---|---|
| TS header | 変更しない |
| adaptation field | 変更しない |
| PCR / OPCR | 変更しない |
| continuity counter | 変更しない |
| payload | MULTI2復号対象 |
| scrambling_control | 復号成功時に clear 化する |
| odd/even key | scrambling_control に従い選択する |

ECM / EMM、CAS権利判定、card I/O、CW取得は Tuner HAL の責務ではない。CAS HAL または CAS bridge が責務を持つ。Tuner HAL は、取得済み key token を使う payload 復号中核だけを担当する。


ECM / EMM 処理、カード I/O、CAS 権利判定、CW 取得、不透明 トークン 発行、B25 system key / CBC 初期値 / data key を CAS 側から安全に供給する経路は CAS HAL または CAS bridge の責務である。CAS / TIS / Tuner HAL のリリース段階ごとの統合スコープは `開発規則.md` を正とする。本節の OK 判定は「Tuner HAL の packet 単位のデスクランブル中核と診断境界が静的に整った」という意味であり、「CAS 通信部だけを除いて libaribb25 の TS→TS B25 処理系が全て完成した」という意味ではない。

## LNB profile 判定表

LNB profile は sysfs `DEVNAME` または `/dev` basename と earth_pt1 の sysfs driver basename で決定する。HAL は以下の表を実装に持つ。

| device node prefix | LNB profile | 成功する voltage |
|---|---|---|
| `px4video*` | `Px4Device15VOnly` | `NONE`, `15V` |
| `pxmlt5video*` | `NoPower` | `NONE` |
| `pxmlt8video*` | `NoPower` | `NONE` |
| `isdb6014video*` | `NoPower` | `NONE` |
| `isdb2056video*` | `NoPower` | `NONE` |
| `pxm1urvideo*` | `NoPower` | `NONE` |
| `pxs1urvideo*` | `NoPower` | `NONE` |
| `isdbt2071video*` | `NoPower` | `NONE` |


`pxmlt5video*` は対応デバイス仕様で LNB 電源非対応のため `15V` を advertise しない。`pxmlt8video*` と `isdb6014video*` は LNB 電源仕様が未確定のため、現行設計では product profile による明示 opt-in を作らず `NoPower` に固定する。未確認デバイスを 15V 成功扱いにする silent overclaim は禁止する。

DVB frontend は sysfs driver basename が `earth-pt1` の場合だけ `EarthPt1FixedLnb` として採用する。frontend name に `tc90522` が含まれるだけでは採用しない。

`EarthPt1FixedLnb` は `NONE`、`11V`、`15V` だけを成功にする。`13V`、`18V`、tone、DiSEqC、satellite position switching は成功扱いしない。


## 恒久仕様

### Filter / DVR 開始 commit 境界

Filter と DVR の開始状態は、内部 start commit が成功した後に確定する。初回 callback / status callback は commit 後に送信する。commit 前の検証または start commit が失敗した場合、対象オブジェクトは開始済み状態へ遷移せず旧状態を維持する。commit 後の コールバック配送 だけが失敗した場合、開始済み状態は rollback せず、対象 callback 状態を `callback_unhealthy` に固定し、継続配送を必要とする通常 `start()` / `read()` / `write()` は拒否する。ただし `stop()` / `flush()` / `close()` は復旧操作として許可する。継続利用が必要な場合は当該 filter / DVR object を `close()` して新規 `openFilter()` / `openDvr()` で作り直す。cleanup が失敗した場合は成功扱いにせず、診断、失敗状態、再試行可能な cleanup 状態のいずれかへ反映する。

### PES 解析境界

record index は、PES と raw elementary stream を区別する。共有 PES parser が PES 形式として拒否した入力を、元 payload 全体の raw elementary stream として再走査してはならない。raw elementary stream として扱うのは、PES stream id として解釈しない入力だけとする。

### packet origin

source filter 由来の TS packet は frontend 由来の TS packet と同じ packet pipeline を通る。ただし origin namespace は frontend と source filter で分離し、assembler generation、carry state、flush state を相互に消してはならない。

### ワーカー 停止失敗

scan ワーカー と tune ワーカー は、join 失敗時に ワーカーslot を破棄してはならない。停止失敗は診断に残し、後続 close または stop で再試行できる状態を保持する。

### AV shared backing

AV shared backing は、検証が成功するまで旧 backing を保持する。設定変更の後段失敗で旧 backing、公開済み handle、stream type を破棄してはならない。release、flush、clear は active/free map を中間不整合のまま公開してはならない。

### test と release API の境界

テストの都合で release経路 の API 可視性を広げない。テスト補助関数は `#[cfg(test)]` 内に閉じる。旧 補助関数、互換 alias、互換 wrapper を release経路 に戻してはならない。


## product 統合手順

product makefile、BoardConfig、ueventd、SELinux、VINTF/init、VTS設定、通常 vendor binary 統合、二重登録禁止の具体手順は `tuner_hal2/INTEGRATION.md` を正とする。本書には統合手順を重複定義せず、Tuner HAL の設計判断だけを置く。旧 `tuner_hal` は参照用ソースであり、product default serviceとして組み込まない。

px4 probe prefix を変更する場合は、frontend_px4系実装、`tuner_hal2/config/ueventd.tuner_hal2.rc`、`tuner_hal2/sepolicy/file_contexts` を同時に更新し、静的確認 と ロジック確認で一致を確認する。この整合条件の実機組込手順は `tuner_hal2/INTEGRATION.md` に従う。


## 契約確認観点

本節は設計契約に対する確認観点を列挙する。実行手順、atest名、VTSコマンド、成果物名、完了判定は `タスク完了判定の実施方法.md` または個別テスト計画を正とし、本書では定義しない。

### AOSP / AIDL / VTS 系

| No | 確認観点 | 目的 |
|---:|---|---|
| T-AOSP-1 | `setDataSource(sourceFilter)` 成功 | 同一 demux 内の source filter接続 |
| T-AOSP-2 | `setDataSource(nullptr)` | AOSP意味論確認。現行 Android 14 AIDL Rust 生成境界では nullable Binder 境界の現行実装済み対象外 |
| T-AOSP-3 | `setDataSource(nullptr)` 後の再start/data出力 | nullable Binder 境界解決後の検証項目 |
| T-AOSP-4 | source filter owner demux不一致 | `INVALID_ARGUMENT` |
| T-AOSP-5 | source filter closed/failed | `INVALID_STATE` |
| T-AOSP-6 | unsupported source/sink subtype | `UNAVAILABLE` |
| T-AOSP-7 | `addPid(pid, nullptr)` | AOSP意味論確認。現行 Android 14 AIDL Rust 生成境界では nullable Binder 境界の現行実装済み対象外 |
| T-AOSP-8 | `removePid(pid, nullptr)` | AOSP意味論確認。現行 Android 14 AIDL Rust 生成境界では nullable Binder 境界の現行実装済み対象外 |
| T-AOSP-9 | `addPid(pid, sourceFilter)` 成功 | upper stream識別ありPID登録 |
| T-AOSP-10 | `removePid(pid, sourceFilter)` 成功 | upper stream識別ありPID解除 |
| T-AOSP-11 | optionalSourceFilter owner demux不一致 | `INVALID_ARGUMENT` |
| T-AOSP-12 | optionalSourceFilter closed/failed | `INVALID_STATE` |
| T-AOSP-13 | `linkCaps` main type matrix | VTS main type期待と一致 |
| T-AOSP-14 | `linkCaps`非宣言main type接続 | `UNAVAILABLE` |
| T-AOSP-15 | TS main type `UNDEFINED` subtype source filter | VTS互換上の扱い確認 |
| T-AOSP-16 | `getAvSharedHandle()` 成功 | fd付きNativeHandleとsize取得 |
| T-AOSP-17 | `releaseAvHandle(fd付きhandle, 0)` 成功 | VTS互換shared handle release |
| T-AOSP-18 | `releaseAvHandle(empty, 0)` | fdなし通知経路 |
| T-AOSP-19 | `releaseAvHandle(empty, activeAvDataId)` | MediaEvent slot release |
| T-AOSP-20 | `releaseAvHandle(fd付きhandle, activeAvDataId)` | `INVALID_ARGUMENT` |
| T-AOSP-21 | `releaseAvHandle(any, negativeAvDataId)` | `INVALID_ARGUMENT` |
| T-AOSP-22 | `getAvSharedHandle()` 複数回取得 + release | fd duplicate寿命確認 |
| T-AOSP-23 | `configureMonitorEvent(0)` | 成功、通常event抑止なし |
| T-AOSP-24 | `configureMonitorEvent(nonzero)` profile有効時 | monitor event発生 |
| T-AOSP-25 | `configureMonitorEvent(nonzero)` profile無効時 | `UNAVAILABLE` |
| T-AOSP-26 | AV `isPassthrough=false` | shared memory AV経路成功 |
| T-AOSP-27 | AV `isPassthrough=true` | `UNAVAILABLE` |
| T-AOSP-28 | `getStatus()` 要求順・同長 | AIDL配列契約 |
| T-AOSP-29 | `getFrontendStatusReadiness()` 要求順・同長 | AIDL配列契約 |
| T-AOSP-30 | unsupported status type | 設計固定値確認 |
| T-AOSP-31 | `tune()` 中の再`tune()` | 旧tune停止、新tune開始 |
| T-AOSP-32 | `scan()` 中の再`scan()` | 旧scan停止、新scan開始 |
| T-AOSP-33 | `stopTune()` | tune停止、attached demuxへdata停止 |
| T-AOSP-34 | `stopScan()` | scan停止 |
| T-AOSP-35 | active scan中の`stopTune()` | 製品設計通りの挙動固定 |
| T-AOSP-36 | DVR playback watermark | 空き領域基準 |
| T-AOSP-37 | DVR record watermark | record callback基準 |
| T-AOSP-38 | `FilterDelayHint` timeのみ | time条件 |
| T-AOSP-39 | `FilterDelayHint` dataのみ | data条件 |
| T-AOSP-40 | `FilterDelayHint` time+data | OR条件 |
| T-AOSP-41 | unsupported API precedence | `UNAVAILABLE` 優先 |
| T-AOSP-42 | VTS XML/profile full run | `VtsHalTvTunerTargetTest` |
| T-AOSP-43 | VTS config audit | monitor / descrambler / AV shared / linkCaps / passthrough整合 |

### ARIB TS packet 系

| No | 確認観点 | 目的 |
|---:|---|---|
| T-TS-1 | sync byte不正 | reject |
| T-TS-2 | 187/189 byte | reject |
| T-TS-3 | TEI set packet | section/PES/AV assemblyへ入れない |
| T-TS-4 | TEI set packet record/raw TS | 方針通り保持/診断 |
| T-TS-5 | adaptation_field_control reserved | reject |
| T-TS-6 | adaptation length overflow | reject |
| T-TS-7 | PCR flagありPCR不足 | reject |
| T-TS-8 | OPCR flagありOPCR不足 | reject |
| T-TS-9 | splicing/private/extension長不足 | reject |
| T-TS-10 | duplicate CC | assemblyへ入れない |
| T-TS-11 | discontinuity_indicator | continuity/assembler reset |
| T-TS-12 | adaptation-only packet | continuityを進めない |
| T-TS-13 | TS resync末尾完全188byte | 次入力sync待ちせず返す |
| T-TS-14 | false `0x47` resync | 誤同期しない |
| T-TS-15 | scrambling_control set + keyなし | record pass-through / assembly drop |

### ARIB section 系

| No | 確認観点 | 目的 |
|---:|---|---|
| T-SEC-1 | section_length最小未満 | reject |
| T-SEC-2 | syntaxあり最小長不足 | reject |
| T-SEC-3 | reserved bit不正 | reject |
| T-SEC-4 | CRC good | accept |
| T-SEC-5 | CRC bad | reject / overflowに写像しない |
| T-SEC-6 | `isCheckCrc=false` + reserved bit不正 | CRC無効でも構文不正はreject |
| T-SEC-7 | EIT `section_length == 4093` | accept |
| T-SEC-8 | EIT `section_length == 4094` | reject |
| T-SEC-9 | 非EIT `section_length == 1021` | accept |
| T-SEC-10 | 非EIT `section_length == 1022` | reject |
| T-SEC-11 | 未分類table長大section | EIT扱いしない |
| T-SEC-12 | section total 1024/4096境界 | accept/reject |
| T-SEC-13 | `SectionBits repeat=false` | one-shot |
| T-SEC-14 | `TableInfo repeat=false` | 1 table / 1 version |
| T-SEC-15 | `repeat=true` version更新 | 継続監視 |
| T-SEC-16 | raw section parse不能event | metadata非意味値 |

### PES / record index 系

| No | 確認観点 | 目的 |
|---:|---|---|
| T-PES-1 | PES start code不正 | malformed |
| T-PES-2 | optional header marker不正 | malformed |
| T-PES-3 | `PTS_DTS_flags == 0b01` | malformed |
| T-PES-4 | PTS marker bit不正 | malformed |
| T-PES-5 | DTS marker bit不正 | malformed |
| T-PES-6 | `PES_packet_length` とheader長矛盾 | malformed |
| T-PES-7 | bounded PES complete | delivery |
| T-PES-8 | unbounded PES next PUSI | 前PES完成 |
| T-PES-9 | unbounded PES flush/stop/close | 未完成を完成扱いしない |
| T-PES-10 | `MAX_PES_BUFFER_BYTES` 超過 | oversized drop + reset |
| T-PES-11 | PES header TS packet境界分割 | 正しく組立 |
| T-PES-12 | PTS field TS packet境界分割 | PTS抽出 |
| T-PES-13 | start code `00 00 01` TS packet境界分割 | record index検出 |
| T-PES-14 | malformed PES後の復帰 | 次PUSIから正常復帰 |
| T-PES-15 | raw PES parse不能event | stream id非意味値 |

### MULTI2 / B25 descrambler 系

| No | 確認観点 | 目的 |
|---:|---|---|
| T-B25-1 | MULTI2既知ベクトル | 復号中核確認 |
| T-B25-2 | payload-only復号 | TS header/adaptation/PCR/CC非破壊 |
| T-B25-3 | even key `10` | even key選択 |
| T-B25-4 | odd key `11` | odd key選択 |
| T-B25-5 | key未設定 | record pass-through + 診断 |
| T-B25-6 | bad token | `INVALID_ARGUMENT` / 診断 |
| T-B25-7 | expired token | `INVALID_STATE` / 診断 |
| T-B25-8 | 復号成功 | scrambling_control clear |
| T-B25-9 | 復号失敗 | 方針通りpass-through/drop/診断 |
| T-B25-10 | ECM/EMM/card I/O不在 | Tuner HALへ持ち込まない |
