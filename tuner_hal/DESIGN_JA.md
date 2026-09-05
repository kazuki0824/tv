# Tuner HAL 設計判断


## DESIGN_JA.md の責務境界

`DESIGN_JA.md` は Tuner HAL の設計正本である。本書は、AOSP 公開契約、ARIB/ISDB 入力処理、本製品の対応範囲、状態所有者、資源寿命、失敗時遷移、quarantine 条件、未対応機能の返却方針を定義する。

本書は、作業履歴、リリース履歴、ビルド手順、atest手順、VTS手順、静的検索手順、成果物命名規則、完了宣言テンプレートを定義しない。それらは `開発規則.md`、`タスク完了判定の実施方法.md`、`CHANGELOG.md`、各作業計画を正とする。

本書と他文書で、状態遷移、資源寿命、戻り値、capability、失敗時波及範囲が矛盾する場合は、本書を正として他文書を修正する。ただし、作業完了判定、成果物名、build / atest / VTS 手順については `タスク完了判定の実施方法.md` を正とする。


### 共通部品の定義条件

本書で共通部品と呼ぶものは、単なる関数名・ファイル名・薄い委譲 wrapper ではない。共通部品として設計正本へ置く場合は、次を定義する。

| 項目 | 必須内容 |
|---|---|
| 論理契約名 | 例: `ObjectCloseTxn`、`ObjectMethodUseCase`、`StreamBoundaryTxn` |
| 実装正本 | 物理 module / file / type は `tuner_hal2/DESIGN_JA.md` の同名論理契約行を単一正本として参照し、本書では複製しない |
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

`Txn` は分類Bの別名として使わない。状態変更の開始条件、確定、取消し・巻戻し、後始末失敗・隔離のいずれも所有せず、複数の正規所有者を接続して結果を集約する共通手順は `UseCase` とする。呼出し単位の値を束ねるだけで正規手順を所有しない非公開補助型は `Context` とし、`Txn` を付けない。`UseCase` が内部で取引を呼ぶこと、または取引境界の一部を調停すること自体は、その `UseCase` を `Txn` と呼ぶ根拠にはしない。


## 外部文書参照: no-panic / 劣化起動 / 閉鎖側失敗境界

この項目のうち、プロジェクト共通の禁止構文、低レベル失敗の型付き検出、mutex汚染、一般的なworker / lock / callback規約は`../GLOBAL_CODE_CONVENTION.md`を正とする。product default `tuner_hal2`の公開status変換の集約方法、ワーカー生成・join、typed entry、現行helper使用規則は`../tuner_hal2/CODE_CONVENTION.md`を正とする。旧`tuner_hal/CODE_CONVENTION.md`は旧参照実装だけの規約であり、現行実装規約の正本として参照しない。公開AIDL戻り値、status precedence、次状態、資源寿命、閉鎖側失敗対象は本書だけを正本とし、実装規約側で再定義しない。


## 正本・移動済み情報の読み方

本書の正本階層は次の順とする。

1. `DESIGN_JA.md の責務境界`、`製品スコープ / AOSP capability / VTS profile 境界`、`AIDL 契約境界`、`Tuner HAL 状態遷移表SSOT` を最上位正本とする。
2. `0-S. 状態所有・寿命・失敗時遷移設計`、`0-S-2. 状態所有者表`、0-S-3Bの名前付きcanonical contract、各公開APIの名前付き契約、`TS continuity / adaptation-only packet 固定`、`AV shared handle の NativeHandle 形式`、`AV転送方式とクライアント側の存続期間`、`AOSP / AIDL / VTS 系`、`ARIB TS packet 系`、`ARIB section 系`、`PES / record index 系`、`MULTI2 / B25 descrambler 系`を現在の設計契約の正本とする。移送済みの旧表番号・旧節名は現行正本参照に使用しない。
3. 旧 `補足契約:` 章は本体正本章へ吸収済みであり、本書内に二重正本として残さない。
4. 個別リリースの履歴、作業経緯、ビルド/atest/VTS/静的検索/成果物命名/完了宣言は本書では定義しない。履歴は `CHANGELOG.md`、完了判定は `タスク完了判定の実施方法.md` を正とする。

削除・移動した旧記載の追跡表は現行リリース物に置かない。現行仕様は本書、project-wide実装規約は`../GLOBAL_CODE_CONVENTION.md`、product default `tuner_hal2`の実装規約は`../tuner_hal2/CODE_CONVENTION.md`、現行実装owner / anchorは`../tuner_hal2/DESIGN_JA.md`、統合手順は`../tuner_hal2/INTEGRATION.md`、変更履歴は`tuner_hal/CHANGELOG.md`を正とする。`tuner_hal/CODE_CONVENTION.md`は旧参照実装固有の規約だけを保持する。存在しない trace 文書または移送済み旧表番号を正本参照にしてはならない。

## 製品スコープ / AOSP capability / VTS profile 境界

製品全体のリリース到達点、日本向け scan 候補、サービス検出、channel key の実装データ保持者は tv 直下の `開発規則.md` を正とする。本節では、Tuner HAL の capability、VTS/profile、AIDL戻り値に閉じる境界だけを固定する。HAL は渡された tune request を処理し、BLIND_SCAN や HAL-generated Japanese scan plan は capability / VTS profile で対応宣言しない。

Tuner VTS用XMLは実行環境に依存する静的構成であり、既定では導入しない。使用するVTS artifact/tag/commitと、試験実行機の`ro.vendor.vts_tuner_configuration_variant`を`VtsEnvironmentProfile`の入力として固定し、XML filenameは選択したVTS実装が実際に行う解決規則から決定する。Android 14 AIDL VTSでbase名`/vendor/etc/tuner_vts_config_aidl_V1`へnon-empty variantを`.`区切りで付加して`.xml`を読む実装を選択した場合は、`/vendor/etc/tuner_vts_config_aidl_V1[.<variant>].xml`として解決する。AOSP branch、VTS artifact/tag/commit、variant property、受信元、周波数、stream ID、PID、実行手順、filter/DVR queue容量、製品memory予算を宣言し、必要な全資源を起動前に予約できる場合だけ、その値を持つ静的XMLを解決済みpathへinstallする。VTS artifactまたはvariant propertyを含む環境入力が未確定な場合はfilename自体を`DESIGN_HOLD_VTS_ENVIRONMENT_UNDECLARED`とし、推測したpathへXMLをinstallせず、VTS成功を宣言しない。descrambler AIDL objectを実装していても、試験設定だけで本番のスクランブル解除成功を宣言しない。

Tuner HAL の capability / VTS profile では TS 入力だけを宣言する。製品全体の TS-only スコープは `開発規則.md` を正とし、本書では Tuner HAL の宣言値と返却値を固定する。MMTP、TLV、ALP、IP CID は capability と VTS profile に宣言しない。`IFilter.configureIpCid()` は filter種別にかかわらず `UNAVAILABLE` とする。CID を保存だけして 照合、経路制御、配送 に使わない成功扱いの無処理 を残してはならない。


### export ID と VTS profile の固定

Tuner HAL が framework へ export する frontend ID は backend の単純な numeric index だけに依存しない。`px4video0` と `pxmlt5video0` のように異なる device family が同じ unit index を持つ場合でも、HAL の frontend ID と physical group ID は衝突してはならない。device family code と unit index を組み合わせ、1,000,000 番台の px4 frontend ID として export する。DVB frontend ID はハッシュではなく固定ビット割当で生成し、`2,000,000 + (adapter_id << 12) + (frontend_index << 4) + variant` とする。`adapter_id` と `frontend_index` は 8 bit、`variant` は 4 bit で、variant は ISDB-T=0、ISDB-S=1 に固定する。範囲外の DVB probe は export しない。生成後の duplicate ID 検出は最終保険として残す。px4 frontend の `exclusiveGroupId` は unit index 単独値ではなく、device family code と unit index を含む packed physical group id として返す。

DVB frontend の `exclusiveGroupId` は公開 frontend ID や `(adapter_id, frontend_index)` の一意性から生成せず、backend topology が検証した「同時に機能できない物理 frontend 群」を正本として決める。同じ物理 frontend から公開する ISDB-T/ISDB-S variant は必ず同じ group に属する。異なる `(adapter_id, frontend_index)` を別 group にできるのは、RF/tuner/demod を含む同時利用不可資源を共有せず同時利用できることを topology で確認できる場合だけとし、共有が確認された別 tuple は同じ group にする。global group ID は符号付き32 bitの非負範囲に収め、上位4 bitの backend class と下位28 bitの backend 固有 group payload は、検証済み排他群へ衝突しない識別子を割り当てるための名前空間にだけ使う。現行 backend class は px4=`0x10000000`、DVB=`0x20000000` に固定する。payload が28 bitへ収まらない候補、異なる排他群の group ID 重複、同一排他群内で group ID が不一致になる候補は `CapabilitySnapshot` へ commit せず、その frontend を公開しない。resource arbitration は frontend ID の数値近接や DVB node tuple ではなく、この検証済み物理排他群と `exclusiveGroupId` の対応だけを正本とする。

現行 Linux DVB profile は `earth-pt1` に限定する。Linux v6.6 `drivers/media/pci/pt1/pt1.c` の `PT1_NR_ADAPS=4`、adapterごとのdemod/tuner、`pt1_start_feed()` / `pt1_stop_feed()` が `adap->index` 単位でstreamを個別制御する実装を同時利用可能性の証跡とする。probeはcanonical sysfs物理device identityごとにfrontend index 0の4 adapterが完全かつ重複なく存在する場合だけ、4つのdriver streamを別の検証済み物理group keyへ写像する。同じdriver streamから作る公開variantは同一keyを共有する。4-stream profileが不完全、sysfs identity不明、別driver、またはgroup key割当が衝突する候補はtopology不明として公開しない。将来、別tuple間で同時利用不可資源を共有するbackendを追加する場合は、その複数tupleへ同じ物理group keyをprobe結果として与えなければならない。


`DvrLeasePool`は確定済みで不変の`CapabilitySnapshot`を参照し、`getDemuxCaps()`応答と`openDvr()`受付可否を決める唯一の情報源とする。再生・記録DVRの全体上限は`snapshot.playback_count`と`snapshot.record_count`、demuxごとの上限は各1個とする。`openDvr()`はlifecycle・入力・用途別/ demux別容量を満たす場合だけobjectを公開し、容量枯渇は`UNAVAILABLE`とする。使用枠reservation、FMQ / Binder artifact prepare、commit、公開前failure時のrollback / cleanup順序は「公開transactionのphase・確定点・失敗処理契約」の`root/child open`を唯一の正本とする。`CleanupPending`または`Quarantined`は最終解放まで使用中と数える。Tuner VTSはruntime能力から無条件に導出せず、起動前`VtsEnvironmentProfile`にVTS artifact/tag/commit、variant property、入力元、PID、経路、queue容量、memory予算が定義されるまで`DESIGN_HOLD`としてXML filenameを解決せず、XMLをinstallしない。使用する静的設定は確定済み`CapabilitySnapshot`に収まり、必要queue容量を正確に予約できなければならない。


### VTS profile / capability 対応契約

VTS XML/profileで使用する機能とcapabilityで宣言する機能は一致させる。VTS profileで使用する機能をcapability非宣言にしてはならず、capabilityで宣言する機能をVTS/profileから到達不能にして検査を回避してはならない。実装適用状況そのものは実装を事実源とし、完了・未達判定は `../タスク完了判定の実施方法.md` に従う判定側で管理する。

| 領域 | capability / profile 方針 | 設計契約 |
|---|---|---|
| `IFilter.setDataSource(filter)`、`filter == NULL` | AOSP意味論として存在する必須契約であり、現行設計の成功対象 | sink filter の入力元を demux input へ戻す |
| `IDescrambler.addPid(pid, optionalSourceFilter)` / `removePid(pid, optionalSourceFilter)`、`optionalSourceFilter == NULL` | AOSP意味論として存在する必須契約であり、現行設計の成功対象 | 指定PIDについてdemux input全体への登録 / 解除として扱う |
| AV shared handle release | media filter shared memory profileでは到達する | `releaseAvHandle(fd付き handle, 0)` を成功させる |
| monitor event | 本製品のTS-only `ProductProfile`では対応宣言しない | `configureMonitorEvent(0)`だけを監視停止として成功させ、非0 maskは`UNAVAILABLE`とする。monitor event用の状態、worker、queue、能力値を生成しない |
| AV passthrough | 対応宣言しない | profileでは `isPassthrough=false` に固定する |
| `linkCaps` | main type 粒度 | 広告した main type pair は VTS が生成する subtype `UNDEFINED` 接続も成功対象に含める。成功させない pair は広告しない |
| TS `RECORD` Filter FMQ | canonical RECORD data-flow profileは `useFMQ=false` とする。Filter FMQ descriptorを明示検査する `record-filter-fmq` probe variantだけ `useFMQ=true` を使用できる | `useFMQ=true` は `IFilter.getQueueDesc()` のdescriptor取得coverageだけを追加し、RECORD payloadの配送先をFilter FMQへ変更しない。実record payloadはRecord DVR FMQへ正確に1回だけ配送し、Filter FMQへ二重配送しない |


#### `DemuxCapabilities.linkCaps` / `numBytesInSectionFilter` 固定契約

Android 14 AIDL V2 の `DemuxCapabilities.linkCaps` は `DemuxFilterMainType` の各bit位置をsource rowとし、各要素をsink main typeのbitmaskとして返す。本製品の `ProductProfile` はTS main typeだけを公開し、公開するmain-type linkageは **TS -> TS の1組だけ** とする。Android 14 V2で定義済みのmain type順 `TS, MMTP, IP, TLV, ALP` に対する具体値は `linkCaps = [0x01, 0, 0, 0, 0]` とし、`filterCaps`にもTS以外のmain type bitを立てない。`linkCaps` はmain type間の接続能力を表すものであり、TS内部の具体的subtype組み合わせを独自の第二capability matrixとして符号化しない。将来main typeが追加されても、現行profileで対応を証明していないrow / sink bitを推測して1にしない。

`linkCaps`で1を返したpairは、VTSがsource / sink双方をsubtype `UNDEFINED`でopenして`setDataSource()`を呼ぶ場合も成功対象に含める。実データ経路では、完全なMPEG-TS packet列を出力するTS raw filterをsourceとし、同じTS main typeのうちMPEG-TS packetを入力として処理する `TS` / `SECTION` / `PES` / `AUDIO` / `VIDEO` / `PCR` / `RECORD` filterをdownstreamとして成功対象に含める。各接続は、対象subtype自体が本製品で公開済みであること、同一demux・owner、lifecycle、cycle禁止、既存settingsとの整合、資源条件などsource filter契約の通常validationを満たす場合にだけ成功する。Section/PES/AV/Record等の加工済みoutputを完全なTS packet列へ暗黙再解釈してsourceにする経路は追加しない。`UNDEFINED`はVTSのmain-type linkage endpointとしての役割を維持し、具体filter subtypeへ暗黙昇格させない。

`DemuxCapabilities.numBytesInSectionFilter` は **16** を返す。AIDL上、この値はSection Filterの`mask`でサポートする最大byte数を表し、section payload最大長、`filter`配列長、`mode`配列長の上限ではない。したがって `sectionBits.mask.size() > 16` は `INVALID_ARGUMENT` とし、状態を変更しない一方、`filter` / `mode` に16-byte上限をこのcapability値から派生させない。`sectionBits.filter`、`mask`、`mode` はAOSPが独立したbyte arrayとして渡すため、長さ不一致だけを理由に拒否せず、各arrayを改変・padding・truncationせず保持する。matching domainは`mask`とし、各byte位置`i < mask.size()`について`mask[i]`の0 bitは比較対象外とする。`mask[i]`に1のactive bitが1つでも存在する場合は、同じbyte位置の`filter[i]`と`mode[i]`の双方が存在しなければ、そのconditionは意味的に不完全として`INVALID_ARGUMENT`とし、値0その他で補完しない。`mask.size()`を超える`filter` / `mode`末尾要素はmatchingに使用しないが、入力shape自体は保持する。active bitの比較はAIDLどおり、`mode` bitが0ならsection bitと`filter` bitの一致、1なら不一致を要求する。runtime sectionがactive comparison位置まで存在しない場合はconfigure/runtime failureへ昇格させず、そのsectionをnon-matchとする。`bitWidthOfLengthField`はMMTP section messageのlength-field幅であり、TSの`section_length`、CRC、condition幅の代用にしない。本製品はMMTP main typeを公開しないため、TS section処理をこの値で変化させない。

### Tuner HAL 固定境界

- CS110 は周波数のみで選局する。ISDB-S settings で `streamIdType=UNDEFINED` かつ `streamId=0` の明示未指定、または AOSP SDK の default 表現である `streamIdType=STREAM_ID` かつ `streamId=INVALID_STREAM_ID(0xFFFF)` だけを selector なしとして扱う。CS110 tune request に TSID / relative stream-number selector が指定された場合は `INVALID_ARGUMENT` とする。`streamIdType=RELATIVE_STREAM_NUMBER` の負値、`streamIdType=UNDEFINED` の負値、その他の負値 selector は未指定へ丸めない。

ISDB-S selectorはAOSPの`FrontendIsdbsStreamIdType`を正とし、`STREAM_ID`と`RELATIVE_STREAM_NUMBER`を別domainとして受理・検証する。Linux DVB / earth_pt1は`STREAM_ID 0..65534`を`DTV_STREAM_ID`へ渡す。px4 legacy ABIは`slot < 12`を相対番号、`slot >= 12`をabsolute TSIDとして解釈するため、px4では`RELATIVE_STREAM_NUMBER 0..7`と`STREAM_ID 12..65534`をlegacy `slot`へ直接渡す。absolute `STREAM_ID 0..11`はAOSP上有効だが同ABIで相対値と区別できないため、副作用なしの`UNAVAILABLE`とする。`65535`は明示TSIDとして`INVALID_ARGUMENT`とする。selector kindを数値域から推測せず、TISへ`EffectiveCapabilities`、driver名、relative slotを公開しない。`ProductProfile`は検証済み能力を抑止できるが、新設または拡張してはならない。


- post-commit callback failureの写像は0-S-3Bの`PostCommitCallbackFailureTxn`、generic worker lifecycleは`WorkerRuntime`、worker failure分類は`WorkerFailureClassifier`を正とする。FMQ / EventFlagのdata-path結果と診断は「失敗影響範囲」および`FilterProducerDrainGate` / `QueueEpochProtocol` / `QueueCleanupUseCase`等の各queue owner契約、API固有の公開状態・戻り値は各APIの名前付き契約を正とし、本節ではfailure state machineを再定義しない。
- `IDvr.setStatusCheckIntervalHint(milliseconds)`で受理したhintは、playback / record DVRが以後のdata/status evaluation cadenceを決定する入力として使用する。hint変更自体はqueue内容、DVR lifecycle、playback/record stateを変更せず、Binder threadをhint時間sleepさせない。callback workerのwait / wake / cancel、close / Drop / shutdown時のgeneric終端は0-S-3Bの`WorkerRuntime`を正とし、本節では再定義しない。
- `getAvSharedHandle()`、AV filter `start()`、`releaseAvHandle()`の公開資源寿命と解放条件は、0-S-2のAV active token台帳、後段の「AV 共有メモリの原子性不変条件」「AV転送方式とクライアント側の存続期間」「AV割り当て」を正とする。

backendのエラーは、呼び出し側の不正値・値域違反を`INVALID_ARGUMENT`、不存在・使用中・容量不足・規格上は有効だが未対応を`UNAVAILABLE`、不正なライフサイクルを`INVALID_STATE`、依存資源の未初期化を`NOT_INITIALIZED`、割り当て失敗を`OUT_OF_MEMORY`、権限・入出力・設定破損・不変条件違反を`UNKNOWN_ERROR`へ対応付ける。


- 本製品のTS-only `ProductProfile`はfilter monitor eventを対応能力として採用しない。`configureMonitorEvent(0)`は監視停止として成功し、未配送monitor event、保存mask、種別ごとの最終観測値を消去する。非0 maskは常に`UNAVAILABLE`とし、monitor event用の状態、worker、queueを生成しない。通常の`DATA_READY` / `OVERFLOW` / `onFilterEvent()` deliveryはmask 0または非0要求の拒否によって抑止しない。
- soft demux の section / PES assembler と filter `stop()` / `flush()` / `configure()` / `close()` は、後段の「フィルタ状態破棄境界と遅延通知方針」、0-S-3Bの`FilterProducerDrainGate` / `QueueCleanupUseCase` / `StreamBoundaryTxn`、および各公開Filter APIの名前付き契約を正とする。
- `setMaxNumberOfFrontends(type, maxNumber)`は同じ`FrontendType`の`0 <= maxNumber <= defaultMax(type)`だけを成功させる。負値、未知type、同typeの既定上限超過は`INVALID_ARGUMENT`とし、別typeの上限を変更しない。
- 製品実行時 の frontend registry は実在 probe できた backendエントリ だけで構成する。probe 失敗は 診断情報レコード に残し、劣化 frontendエントリ / テスト劣化補助関数 / 診断劣化補助関数 は作らない。


### TS PID 共通値域契約

TS PIDの構文上有効な値域は13 bitの`0x0000..0x1FFF`とする。`0xFFFF`はAOSP `INVALID_TS_PID`であり入力PIDとして無効、`0x2000..0xFFFE`、負値、生成binding上の範囲外値・不正union encodingも`INVALID_ARGUMENT`として状態を変更しない。予約PID/特殊PIDも13 bit値域内なら、予約されているという理由だけで構文上無効とはしない。Filter設定とDescrambler `tPid`は同じ値域契約を使う。未対応の`DemuxPid` union variantは既存のcapability契約どおり`UNAVAILABLE`とし、数値不正と混同しない。

### nullable Binder 境界

LineageOS 22.1 / Android 15 で採用する Tuner AIDL の null 入力は、API ごとに根拠と製品契約を分けて扱う。

- `IFilter.setDataSource(filter)` は、Tuner HAL AIDL の説明が `filter == NULL` を demux 入力元への復帰と明記し、AIDL VTS も別 Filter を入力元に設定した後で `setDataSource(nullptr)` の成功を要求する。Rust 生成物でこの入力を表現できるよう、unfrozen current V3 では `filter` を `@nullable` とし、Rust 公開境界は `Option` で受ける。LineageOS 22.1 の `frameworks/base` と `frameworks/av` には null を HAL まで運べない箇所があるため、製品統合時に両方を補正する。
- `IDescrambler.addPid()` / `removePid()` は、Tuner HAL AIDL が source Filter を optional と説明し、Java API が `@Nullable Filter` を受け、`frameworks/base` と `frameworks/av` が null を HAL まで保持し、AIDL VTS も `nullptr` での add/remove 成功を要求する。`optionalSourceFilter == null` は上流 Filter による入力元限定を付けない PID 登録・解除として扱う。unfrozen current V3 では当該 Filter 引数を `@nullable` とし、Rust 公開境界は `Option` で受ける。
- `IFrontend.setCallback(callback)` は、AIDL コメントに null 可との記載がある一方、機械的 AIDL 宣言は非 null、LineageOS 22.1 の `frameworks/base` は非 null Binder callback を生成し、`frameworks/av` と AOSP 参照 HAL は null を `INVALID_ARGUMENT` とし、AIDL VTS も非 null 登録だけを要求する。本製品では `setCallback()` を非 null 登録契約とし、callback の寿命終了は `close()` に従う。V3 で `@nullable` を追加しない。
- `ILnb.setCallback(callback)` は、AIDL コメントが null を許容し、AOSP 参照 HAL も null を保存して以後の callback 配送を停止する。一方、LineageOS 22.1 のフレームワーク経路と AIDL VTS は HAL への null 登録を使用しない。これはフレームワーク必須経路ではなく、Hardware AIDL の説明と参照 HAL に既に存在する null 値を Rust 実装でも表現するための補正として扱う。unfrozen current V3 では `callback` を `@nullable` とし、Rust 公開境界は `Option` で受ける。

生成言語bindingの表現は公開契約ではない。実装適用状況そのものは実装を事実源とし、判定結果・未達理由は本書に保持せず`../タスク完了判定の実施方法.md`に従う判定側で管理する。実装状態を理由に本節のAOSP契約を弱めたり、frozen AIDLをvendor独自改変したりしてはならない。

### `IFrontend.setCallback()` 登録契約

frontend runtimeはcallback slotを`Empty(callback_generation)`または`Registered(callback_identity, callback_generation)`として所有する。`callback_generation`は単調増加し、古い値を再利用しない。tune/scan workerはcallback実体を保持せず、frontend operation generationとeventだけを配送キューへ渡す。配送開始時に現在のcallback slotを解決し、置換または解除済みgenerationの未配送entryは配送しない。置換前にBinder配送を開始済みの呼出結果は診断へ記録し、新callbackへ重複配送しない。

| API / 入力状態 | AIDL戻り値 | 公開上の結果 | 失敗時の公開状態 |
|---|---|---|---|
| `setCallback(non-NULL)` / Live / `Empty` | 成功 | 新callbackを登録し、新generation以後の配送先とする | 登録失敗は`UNKNOWN_ERROR`とし、従前の`Empty`状態を維持 |
| `setCallback(non-NULL)` / Live / `Registered(old)` | 成功 | 同一identityを含む再設定を受理し、新callbackへ置換する。置換後は旧generationへの新規配送を行わない | 登録失敗は`UNKNOWN_ERROR`とし、旧callbackを配送先として維持 |
| `setCallback(NULL)` / Live | 成功 | callback登録を解除し、新generation以後の配送を停止する。既に`Empty`なら成功no-op | 解除失敗は`UNKNOWN_ERROR`とし、旧登録を維持 |
| `setCallback(any)` / LogicalClosed、CleanupPending、Quarantined | `INVALID_STATE` | 入力状態を維持 | callback状態を変更しない |

Binder artifact、runtime registry、domain callback logical stateのprepare / composite commit / rollback、旧artifact cleanupは0-S-3Bの`CallbackRegistrationUseCase`を唯一の正本とし、本節では再定義しない。callback delivery failureは`PostCommitCallbackFailureTxn`に従う。

current callbackのBinder deathは、死亡したcallbackがcurrent registrationに対応する場合だけ登録解除として扱う。置換済みcallbackの遅延death通知は現在の登録へ影響させない。`close()`後は旧generation由来の未配送entryを配送せず、callback registrationのcleanupは`ObjectCloseTxn`から`CallbackRegistrationUseCase`のtyped cleanupへ接続する。

### Android 14 AIDL filter source 境界契約

AOSP Frameworkの`Filter.setDataSource()`は、成功済みのnon-NULL sourceを保持している状態から別のnon-NULL sourceへ再設定する標準経路を持たない。したがって本製品の公開成功契約として「任意のnon-NULL→別non-NULL replacement」を要求しない。`SourceBoundaryTxn`は初回source接続、AIDLのNULL/demux-input意味論、cleanupによるunlink、cycle/owner/generation検証を正規責務とし、内部実装がより一般的なrelation mutation primitiveを持つこと自体はimplementation detailとする。

`IFilter.setDataSource(non-NULL)`はcandidate edgeをcommitした場合のsource relation graph全体を検証し、self-loopを含む任意の有向cycleを作る要求を`INVALID_ARGUMENT`で拒否して状態を変更しない。2-node / 3-node cycleも同じ規則で拒否する。cycle検証とrelation commitは0-S-3B `SourceBoundaryTxn`の同一transactionに属する。



`configure()`は入力元との接続を変更しない。新しい設定が既存の接続と両立しない場合は`INVALID_STATE`で拒否し、以前の設定と接続を保持する。切断は`setDataSource(null)`で明示する。不正な設定には`INVALID_ARGUMENT`を返す。


`IDescrambler.addPid()` / `removePid()` は、`optionalSourceFilter == NULL` を demux input 全体に対する PID 登録 / 解除として扱い、`optionalSourceFilter != NULL` を指定 filter output、すなわち upper stream に対する PID 登録 / 解除として扱う。NULL 経路は現行AOSP契約上の必須成功対象として設計対象に含める。non-null source filter 経路は、本書の「表D-1. IDescrambler PID 操作表」を正とし、同一 demux、非閉鎖、世代一致を検証する。


### 公開transactionのphase・確定点・失敗処理契約

この表は`../tuner_hal2/DESIGN_JA.md`から責務移管した公開transactionのphase、確定点、失敗処理を保持する。公開AIDLの意味、状態、戻り値、確定点、rollback / cleanupは本書が唯一の正本である。実装owner、module anchor、呼び出し禁止入口は`../tuner_hal2/DESIGN_JA.md`を正とする。

object methodでは、呼出対象のlifecycle/generation不整合を引数値の詳細検証より先に`INVALID_STATE`へ確定する。呼出対象の生存検証後のtag、列挙値、nullable入力、値域の不正は`INVALID_ARGUMENT`とし、状態を変更しない。別object引数のlifecycle/generation不整合は`INVALID_STATE`、foreign owner、別demux、wrong kind、非互換関係は`INVALID_ARGUMENT`とし、呼出対象objectのowner検証と引数objectのownership検証を同じ判定へ丸めない。

| 契約 | 必須phase order | 確定点・失敗処理 |
|---|---|---|
| object method | 呼出対象live・自身の登録owner・generation・kind確認 → request変換 → 引数object live/generation確認 → 引数object owner/demux/kind/関係検証 → dispatch計画 → 一回限り権限消費 → domain実行 | domain commit前は無変更。呼出対象lifecycle不整合と引数object lifecycle不整合は`INVALID_STATE`、foreign/wrong関係は`INVALID_ARGUMENT`、commit後失敗は型付き診断と契約別cleanupへ接続 |
| root/child open | 公開ID・能力確認 → 全使用権仮予約 → runtime/object-tableを`Prepared`で登録 → callback/Binder artifact準備 → Binder object準備 → private callback artifact stage → runtime `Live` publish | **AIDLから観測可能な公開確定点はruntime/object-tableの`Live` publishだけ**とする。callback artifactは`Live`前にprivate storeへstageしてよいが、child objectはまだcallerへ返さず通常delivery sourceにもならない。後段`Live` publish失敗時はstage済みartifactと全仮予約を逆順cleanupしてerrorを返し、artifactだけを公開成功状態として残さない。AOSP/VINTF/VTSにvendor内部のcallback storeとruntime tableを同一物理mutationでcommitせよという契約は追加しない |
| public close / owner loss / Drop | `ObjectCloseTxn`の`begin_close`とtyped cleanup契約を正とし、本行では再定義しない | 確定点、`CloseCleanupAuthority`、`CleanupPending`、回収移管は`ObjectCloseTxn`契約を正とする |
| descrambler key / PID / session cleanup | key変更は`DescramblerKeyTxn`、PID変更は`DescramblerPidTxn`、session cleanupは`DescramblerSessionCleanupTxn`を正とし、本行ではphaseを再定義しない | 各契約の確定点、rollback / cleanup、失敗時状態をそのまま適用する |
| source relation / stream boundary | Filter source relationは`SourceBoundaryTxn`、Demux-Frontend relationは`DemuxFrontendSourceTxn`、stream state境界は`StreamBoundaryTxn`を正とし、本行では一体transactionを再定義しない | 各relation契約と`StreamBoundaryTxn`の確定点、rollback / cleanup、失敗時状態をそのまま適用する |
| frontend tune/scan | request検証 → tuneでは同一条件・healthy snapshot判定、scanではrequest fingerprint確定 → worker/callback/rollback準備 → 非破壊tune re-entry、同一`LockedReported`のscan継続、または旧session遮断後のbackend要求・新generation commitへ分岐。`stopTune()` / `stopScan()`は対象operationのgenerationをfenceし、該当backend/worker停止と必要なstream boundary cleanupを同じownerで完了させる。複数demuxへboundaryが必要な場合は、破壊的処理の対象demux一覧をこのownerで固定し、各対象へtyped `StreamBoundaryTxn`を実行して結果を集約する | 同一健全tuneは`request_sequence`と現lockの`LOCKED`配送予約だけを確定し、現generation・worker・backend・demux境界・AVを維持する。scan継続は旧scan generationをfenceし、backend再探索なしに新callback generationからENDを1回配送する。それ以外のfull tune/scanだけが旧session遮断、backend要求、新generation commitへ進む。旧session遮断後の新要求拒否では旧要求を再投入せず、backend停止・境界終端を確認できれば`Untuned`、backend結果不明は`FailedBackend`、境界不明でfence成立は`FailedBoundary`、fence不成立は`Quarantined`とする。複数demuxのboundary結果集約では、確定済みdemux結果を巻き戻さず、pre-commit失敗または未処理対象は変更せず、commit結果を確定できないdemuxだけを隔離する。frontendのcaller-visibleな結果は後段の`IFrontend.tune()` / `IFrontend.scan()` / `IFrontend.stopTune()` / `IFrontend.stopScan()`の名前付き契約へ写像する。commit済みoperationのcallback配送失敗ではdomain stateをrollbackせず`PostCommitCallbackFailureTxn`へ渡す |
| callback registration | `CallbackRegistrationUseCase`を正とし、AIDL façadeのartifact prepare/releaseとservice_runtime側composite commitのphaseを本行では再定義しない | artifact、runtime registry、domain logical stateの確定点とrollback / cleanupは同契約をそのまま適用する |
| worker終端 | generic lifecycle mechanismは`WorkerRuntime`を正とし、本行では共通phaseを再定義しない | domain固有の終了意味は該当API契約、failure分類は`WorkerFailureClassifier`契約を適用する |

## AIDL 契約境界

`IFilter`、`IDvr`、`IFrontend`、`IDemux`、`ILnb`、`IDescrambler` の 公開メソッド は、AIDL HAL の契約面として close 後状態を必ず検査する。状態別の戻り値、次状態、維持する内部状態、破棄・無効化する内部状態は、本書の「Tuner HAL 状態遷移表SSOT」を正とする。

通常のメモリ割り当て、FMQの作成・領域確保、共有メモリまたはdma-bufの割り当てについて、要求を満たす容量を確保できないことが確定した場合は`OUT_OF_MEMORY`へ写像する。`UNKNOWN_ERROR`は、容量不足ではない内部不整合、allocator/backendから原因を確定できない異常、または割り当て結果・副作用を確定できない障害に限定する。既知の容量不足を`UNKNOWN_ERROR`へ丸めず、低レベル実装名やerrnoにより公開結果を変えない。個別APIのlifecycle、入力、未対応、commit後失敗が優先される場合は各状態表のpriorityを正とする。

### ITunerルートAPIの固定契約

| API | 成功条件と結果 | 失敗時 |
|---|---|---|
| `getFrontendIds()` | 起動時に確定したfrontend IDを昇順で返す。ID集合はサービス世代中不変であり、`setMaxNumberOfFrontends()`で増減させない | snapshotを読み出せない内部障害は`UNKNOWN_ERROR`、部分結果は返さない |
| `openFrontendById(id)` | 公開済みIDでtype別の現在上限と使用可能枠を満たす場合に、指定IDの`IFrontend` objectだけを返す。内部open transactionは「公開transactionのphase・確定点・失敗処理契約」の`root/child open`を正とする | 未公開IDは`INVALID_ARGUMENT`、公開済みだが現在上限または使用枠により開けない場合は`UNAVAILABLE`。その他の公開前failure / rollback / cleanupは`root/child open`に従いobjectを返さない |
| `getFrontendInfo(id)` | 公開済みIDに対応する起動時確定済みの不変な`FrontendInfo`を返す | 未公開IDは`INVALID_ARGUMENT`、内部snapshot障害は`UNKNOWN_ERROR`、部分情報は返さない |
| `getDemuxIds()` | `CapabilitySnapshot.publicDemuxes`のkeyを昇順で返す。ID集合はサービス世代中不変とする | snapshotを読み出せない内部障害は`UNKNOWN_ERROR`、部分結果は返さない |
| `openDemux(out demuxId)` | 公開済みdemux ID集合から使用可能な1 IDを選び、成功時だけ`IDemux` objectと要素数1の`demuxId`配列を同一応答で返す。内部open transactionは`root/child open`を正とする | 使用可能な公開IDまたは容量がない場合は`UNAVAILABLE`。その他の公開前failure / rollback / cleanupは`root/child open`に従い、objectもIDも部分公開しない |
| `openDemuxById(id)` | 公開済みの指定IDが利用可能な場合に、その`IDemux` objectだけを返す。入力IDを出力として返さない。内部open transactionは`root/child open`を正とする | 未公開IDは`INVALID_ARGUMENT`、公開済みだが使用中または容量不足は`UNAVAILABLE`。その他の公開前failure / rollback / cleanupは`root/child open`に従いobjectを返さない |
| `getDemuxCaps()` | `CapabilitySnapshot.publicDemuxes`と同じper-demux能力集合から`numDemux`と`filterCaps`を導出し、その他の不変な`DemuxCapabilities`項目と一括で返す | snapshotを読み出せない内部障害は`UNKNOWN_ERROR`、部分的な能力値は返さない |
| `getDemuxInfo(id)` | `CapabilitySnapshot.publicDemuxes[id].filterTypes`を不変の`DemuxInfo.filterTypes`として返す | 未公開IDは`INVALID_ARGUMENT`、内部snapshot障害は`UNKNOWN_ERROR` |
| `openDescrambler()` | descrambler能力とobject/session枠が利用可能な場合に、未結合の`IDescrambler` objectだけを返す。demux ID、demux generation、`DescramblerCapacityPool`は選択しない。内部open transactionは`root/child open`を正とする | 対応するdescrambler能力またはobject/session枠がない場合は`UNAVAILABLE`。その他の公開前failure / rollback / cleanupは`root/child open`に従いobjectを返さない |
| `getLnbIds()` | 起動時probeとoperation/value capability対応表から公開対象と確定したLNB IDを昇順で返す。`aidl_baseline_eligible=false`だけを理由に除外しない | snapshotを読み出せない内部障害は`UNKNOWN_ERROR`、部分結果は返さない |
| `openLnbById(id)` | 公開済みIDのendpointが利用可能な場合に、その`ILnb` objectだけを返す。公開判定は実証済みoperation/value capabilityに従い、内部open transactionは`root/child open`を正とする | 未公開IDは`INVALID_ARGUMENT`、公開済みだが使用中、`CleanupPending`、`Quarantined`のendpointは`UNAVAILABLE`。`aidl_baseline_eligible=false`だけを理由に拒否しない。その他の公開前failure / rollback / cleanupは`root/child open`に従う |
| `openLnbByName(name, out lnbId)` | 本製品は名前付き外部LNBを公開しない | 空文字は`INVALID_ARGUMENT`、その他の名前は`UNAVAILABLE`。LNB ID、object、leaseを生成せず、出力を部分公開しない |
| `isLnaSupported()` | `false`を返す | 内部状態へ依存させない |
| `setLna(enable)` | 本製品はLNA制御を公開しない | `UNAVAILABLE`。frontend、backend、capabilityを変更しない |

### `FrontendInfo` scalar capability 契約

公開frontendごとに、`CapabilitySnapshot`は`FrontendInfo`へ返す`minFrequency`、`maxFrequency`、`minSymbolRate`、`maxSymbolRate`、`acquireRange`を`FrontendScalarCapability`として変更不能に保持する。`getFrontendInfo(id)`はこのsnapshotをコピーするだけとし、呼出し時のbackend probe、推測値、別の周波数表から値を合成しない。

- `minFrequency`と`maxFrequency`は、同じfrontendの公開`tune()`/`scan()` validationが受理し得る周波数集合の外側境界と一致させ、`0 <= minFrequency <= maxFrequency`を満たす。境界外を`UNAVAILABLE`で拒否する実装が、それより広い範囲を`FrontendInfo`で広告してはならない。逆に、明示選局で受理する周波数をこの範囲外へ置いてはならない。
- `minSymbolRate`と`maxSymbolRate`は当該frontendが受信可能なsymbol rate rangeをsymbols per secondで表し、`FrontendSettings`でcallerが明示symbol rateを指定できるかどうかとは分離する。backend/device/profileの能力証跡から実際の受信可能範囲を決め、明示指定非対応だけを理由に`0/0`へ固定したり、独自sentinelとして扱ったりしない。settings側の`symbolRate`受付可否は別のvalidation契約に従う。
- `acquireRange`は、要求周波数の周囲でbackendが探索可能と製品profileで検証した非負の範囲だけを返す。製品profileで検証済みの非0範囲がなければ`0`とする。`acquireRange`を`minFrequency`/`maxFrequency`の外側を受理する根拠にしてはならない。
- 起動時のsnapshot候補について、上記scalarとfrontend validation、backend設定経路、`ProductProfile`のいずれかが矛盾する場合は候補をcommitしない。scalarだけを後からclampして整合したことにしてはならない。


### demux能力の横断不変条件

`CapabilitySnapshot.publicDemuxes`は、公開demux IDをkey、当該demuxで実際にopenできる`DemuxFilterMainType`のbit ORを`filterTypes`とする単一の順序付きmapである。`getDemuxIds()`、`getDemuxInfo()`、`getDemuxCaps()`、`openDemux()`、`openDemuxById()`はこのmap以外からIDまたはfilter main typeを合成してはならない。

| 公開値 | 唯一の導出規則 |
|---|---|
| `getDemuxIds()` | `sort(keys(publicDemuxes))` |
| `DemuxCapabilities.numDemux` | `size(publicDemuxes)` |
| `getDemuxInfo(id).filterTypes` | `publicDemuxes[id].filterTypes` |
| `DemuxCapabilities.filterCaps` | 全`publicDemuxes[id].filterTypes`のbit OR。集合が空なら0 |

snapshot確定時に、ID重複、未定義bit、公開filter数と矛盾するmain type、`numDemux != size(publicDemuxes)`、または`filterCaps != OR(filterTypes)`を検出した候補はcommitせず、候補vector全体を戻す。確定後に`DemuxInfo`と`DemuxCapabilities`を別々に補正してはならない。これによりAndroid 14 VTSの全demux横断一致を構造的に保証する。

`setMaxNumberOfFrontends(type, maxNumber)`の上限は`FrontendType`ごとに独立して保持する。`defaultMax(type)`は起動時probeに成功し、非空のhardware infoまで準備できた同じtypeのfrontend数、`currentMax(type)`はその初期値を持つ。未知のtype、負値、`defaultMax(type)`超過は`INVALID_ARGUMENT`とする。`0..defaultMax(type)`への変更は成功し、既存leaseを強制closeしない。新規openだけを`activeLeaseCount(type) < currentMax(type)`で制限し、0では同typeの新規openを`UNAVAILABLE`とする。`getMaxNumberOfFrontends(type)`は同じtypeの`currentMax`を返す。`getFrontendIds()`の不変な機器ID集合は上限変更で増減させない。

### 非対応のTimeFilterとCI CAM

本製品の`DemuxCapabilities.bTimeFilter`は`false`に固定する。`IDemux.openTimeFilter()`はdemuxが閉鎖済みなら`INVALID_STATE`、それ以外では`UNAVAILABLE`を返し、`ITimeFilter` object、lease、workerを生成しない。したがって`setTimeStamp()`、`clearTimeStamp()`、`getTimeStamp()`、`getSourceTime()`、`close()`へ到達可能な公開objectは存在しない。

VTS製品設定の`canConnectToCiCam`は`false`に固定する。CI CAM系APIはAIDLシグネチャどおり次の契約へ分け、CAM ID、接続状態、backend状態を変更しない。対応しない機能を成功扱いの無処理にしてはならない。

| API / 入力 | 閉鎖済みobject | Live object |
|---|---|---|
| `IFrontend.linkCiCam(ciCamId)` / `unlinkCiCam(ciCamId)` / `IDemux.connectCiCam(ciCamId)`、`ciCamId < 0` | `INVALID_STATE` | `INVALID_ARGUMENT` |
| 同3 API、`ciCamId >= 0` | `INVALID_STATE` | `UNAVAILABLE` |
| `IDemux.disconnectCiCam()` | `INVALID_STATE` | `UNAVAILABLE` |

`disconnectCiCam()`には入力引数がないため、CAM ID検証を適用しない。

### Filter / DVR query API 固定契約

`IFilter.getQueueDesc()`、`IFilter.getId()`、`IFilter.getId64Bit()`、`IDvr.getQueueDesc()`は読み取り専用queryであり、成功時にobject lifecycle、queue identity、generation、設定、relation、FMQ read/write positionを変更しない。論理閉鎖開始後は0-S-4の公開close gateに従い`INVALID_STATE`とする。

| API | Live objectでの成功条件と結果 | 非対応 / failure |
|---|---|---|
| `IFilter.getQueueDesc()` | open時filter種別がFilter FMQ resourceを所有する場合、`configure()`前後を問わずopen時に確保した同一FMQ descriptorを返す | Filter FMQ resourceを所有しないfilterは`UNAVAILABLE`とし、FMQ descriptorを後付け生成しない |
| `IFilter.getId()` / `getId64Bit()` | open時に確定したdemux-local filter IDを対応するAIDL返却型で返す。両queryは同じfilter object identityを表す | 読み取り専用queryとして別IDを再発行せず、状態を変更しない |
| `IDvr.getQueueDesc()` | record / playbackとも、`configure()`前後およびstart/stop状態を問わず、open時に確保した同一DVR FMQ descriptorを返す | `configure()`または再設定でqueue identity、容量、descriptorを置換しない |

Filter FMQ resourceを所有するFilterは、未configureを理由に`getQueueDesc()`を拒否しない。Filter FMQ resourceの所有とpayload data planeとしての使用は次の契約で分離する。audio/video mediaは`MediaEvent`の共有領域またはイベント固有fdへ配送し、PCRその他callback-only eventにはFilter FMQ resourceを設けない。DVR FMQは`openDvr()`の`bufferSize`から生成し、`configure()`はしきい値等のDVR設定だけを変更する。

#### Filter FMQ ownership / payload data-plane 分離契約

Filter FMQの「Filter objectがresourceとして所有し`IFilter.getQueueDesc()`でdescriptorをexportできること」と、「そのFMQを当該Filterのpayload data planeとして使用すること」は独立した2軸とする。resource ownershipをpayload配送能力の別名として扱わない。

現行TS-only profileでは、TS `TS`（raw TS）、`SECTION`、`PES`、`RECORD` Filterがopen時にFilter FMQ resourceを所有する。これらは有効な`openFilter(type, bufferSize, callback)`の`bufferSize`をFilter FMQ byte容量として`CapabilitySnapshot.fmqRuntimeBudgetBytes`から予約し、Filter object lifetime中は同じqueue identity / descriptorを維持する。`AUDIO`、`VIDEO`、`PCR`、`UNDEFINED`はFilter FMQ resourceを所有せず、`IFilter.getQueueDesc()`は`UNAVAILABLE`とする。

Filter FMQをpayload data planeとして使用するのはTS `TS`、`SECTION`、`PES`だけとする。これらのpayload commit、EventFlag wake、`DATA_READY` / `LOW_WATER` / `HIGH_WATER` / `OVERFLOW`は後段の通常Filter status契約を正とする。

TS `RECORD` FilterはFilter FMQ resourceを所有しdescriptorをexportできるが、RECORD TS payloadをそのFilter FMQへ書き込まない。RECORD Filterの実payload data planeは、`RecordDvrFilterRelationTxn`でattachされたRecord DVRのRecord DVR FMQだけとし、同じ188-byte TS packetをRECORD Filter FMQとRecord DVR FMQへ二重配送してはならない。RECORD Filter FMQのread/write position、occupancy、EventFlagはRECORD payload、Record status、`record_output_byte_offset`、`DemuxFilterTsRecordEvent.byteNumber`の入力にしない。

RECORDの`record_output_byte_offset`と`DemuxFilterTsRecordEvent.byteNumber`は「TS RECORD filter settings / event 固定契約」と0-S-3Bの`FilterProducerDrainGate`を正とし、Record DVR FMQへの実output commit成功時だけ実commit byte数に従って進める。Record DVR output commit失敗時、RECORD Filter FMQのdescriptor取得、Filter FMQのqueue position変化だけでは進めない。

VTS/profileの`useFMQ`はこのownership/data-plane境界を変更しない。canonical RECORD data-flow profileはAOSPのRECORD経路に合わせ`useFMQ=false`とし、`record-filter-fmq` probe variantの`useFMQ=true`は`IFilter.getQueueDesc()` descriptor contractを追加検査するためだけに使用する。probe variantでもRECORD payloadの唯一のdata planeはRecord DVR FMQのままとする。

### Filter / DVR lifecycle API 固定契約

Filter / DVR の `configure()` / `start()` / `stop()` / `flush()` と Record DVR の `attachFilter()` / `detachFilter()` について、caller-visibleな事前状態、戻り値、次状態、維持する公開資源は本節を正とする。旧状態表の行番号や旧状態コードを現行正本として復活させず、内部のprepare / commit / rollback、producer drain、queue epoch、playback consume、relation mutationは0-S-3Bの既存canonical contractを唯一の正本とする。論理閉鎖開始後の優先判定は0-S-4の公開close gateを正とし、本節のLive状態行より先に適用する。

本節では公開lifecycleを `OpenUnconfigured`（open成功後、対象settings未確定）、`ConfiguredStopped`（有効settings確定済みかつ非開始）、`Started`（開始済み）の3射影で記述する。AV filterのshared handle、補助stream type、active `avDataId`等の直交軸は「AV転送方式とクライアント側の存続期間」およびAV allocation契約を正とし、本節の射影へ吸収しない。

#### IFilter lifecycle

| API / 条件 | `OpenUnconfigured` | `ConfiguredStopped` | `Started` | caller-visibleな確定内容 |
|---|---|---|---|---|
| `configure(valid settings)` | 成功 → `ConfiguredStopped` | 成功 → `ConfiguredStopped` | `INVALID_STATE`、状態不変 | open時のqueue identity、source relation、callback登録、monitor設定、delay hintを勝手に置換しない。既存source relationと新settingsが両立しない場合は`INVALID_STATE`で旧settings/relationを維持する。一度start済みのFilterに対するstop後の有効な`configure()`成功は、settingsが同一か否かにかかわらず「Filter event startId / FilterDelayHint 契約」に従って次回start用pending `startId`をprepareし、同一settingsを単なるno-opとしてこの契約を省略しない |
| `start()` | `INVALID_STATE`、状態不変 | 成功 → `Started` | 成功、状態不変 | 対応producer / deliveryを開始または再開する。AVはshared handle未取得でもevent-local fd経路があるためhandle取得をstart成功条件にしない |
| `stop()` | 成功、状態不変 | 成功、状態不変 | 成功 → `ConfiguredStopped` | 新規producer / deliveryを停止するが、既にFilter outputへcommitした未消費FMQ data、配送済みAV allocation / active token等のcaller-visible資源を破棄しない。`stop()`を`flush()`の代用にしない |
| `flush()`（non-AV） | `INVALID_STATE`、状態不変 | 成功、lifecycle維持 | 成功、`Started`を維持 | `FilterProducerDrainGate`と`QueueCleanupUseCase`を通し、受付済みproducerを排出してから未消費Filter FMQ data、未配送event、parser / assembler partial stateを当該Filterの契約に従って破棄する。source relation、callback登録、monitor設定、delay hint、queue identity、配送済みclient資源は維持する |
| `flush()`（AV） | 成功、設定/実行軸を維持 | 成功、設定/実行軸を維持 | 成功、`Started`を維持 | shared handleの取得有無を成功条件にしない。未配送AV payload / transient stateだけを破棄し、shared backing、client handle lease、配送済みactive `avDataId` allocationはAV資源寿命契約に従って維持する |

`configure()`の入力値・subtype・passthrough・PES/section等の能力判定は各既存の名前付き入力契約を正とする。本節は有効入力に対するlifecycle結果を固定するだけであり、未対応入力を成功へ昇格させない。`flush()`成功はFilter lifecycleそのものを停止状態へ変更する操作ではなく、開始済みFilterでは開始状態を維持する。

#### IDvr lifecycle

| API / 条件 | `OpenUnconfigured` | `ConfiguredStopped` | `Started` | caller-visibleな確定内容 |
|---|---|---|---|---|
| `configure(valid same-kind settings)` | 成功 → `ConfiguredStopped` | 成功 → `ConfiguredStopped` | `INVALID_STATE`、状態不変 | `DvrSettings`契約で全fieldを一括validate / commitし、open時queue identity、FMQ read/write position、既存Record Filter relationをconfigureだけで置換・flushしない。record/playback settingsのDVR kind不一致は`INVALID_ARGUMENT`で状態不変 |
| `start()` | `INVALID_STATE`、状態不変 | 成功 → `Started` | 成功、状態不変 | Record DVRはrecord Filter未attachでもstart自体を成功させ、attachされるまでdata productionがないだけとする。Playback DVRは既存queueを入力としてconsumeを開始する |
| `stop()` | 成功、状態不変 | 成功、状態不変 | 成功 → `ConfiguredStopped` | Recordは既にcommit済みの未消費Record DVR FMQと`record_output_byte_offset`を維持する。Playbackは未読FMQと`PlaybackConsumeTxn`が保持するprocessing buffer / parse-inject cursorを維持し、再startで継続可能にする。いずれも`stop()`でqueueをflushしない |
| `flush()`（Record） | `INVALID_STATE`、状態不変 | 成功、lifecycle維持 | `INVALID_STATE`、`Started`維持 | 非開始時だけRecord DVR FMQのclient未消費byte列とRecord-pathの未確定partial / statsを破棄し、Filter relation、queue identity、Filter側`record_output_byte_offset`を維持する |
| `flush()`（Playback） | `INVALID_STATE`、状態不変 | 成功、lifecycle維持 | 成功、`Started`維持 | `QueueEpochProtocol` / `QueueCleanupUseCase` / `PlaybackConsumeTxn`を通し、新規read commitをfenceして受付済みtokenを完了または取消した後、未読Playback DVR FMQと保持中processing / assembler residualを破棄する。relation、queue identityは維持する |

Record DVRの`attachFilter()` / `detachFilter()`は、Record DVRがLiveであればconfigure前、`ConfiguredStopped`、`Started`のいずれでも受理対象とする。liveかつ同一demux・同一owner・record対応Filterへの初回attachは成功、同一Filterの重複attachは状態不変で成功、登録済みFilterのdetachは成功、未登録の同一Filterのdetachも状態不変で成功とする。Playback DVRに対するattach/detach、foreign owner、別demux、record非対応Filterは`INVALID_ARGUMENT`、同一サービス内で論理閉鎖済みのFilter引数は`INVALID_STATE`とする。relationのprepare / commit / abortとexactly-once route切替は`RecordDvrFilterRelationTxn`だけを正本とする。

`IDvr.setStatusCheckIntervalHint(milliseconds)`はRecord / Playbackの全Live lifecycle状態で受理し、DVR lifecycle、queue内容、relationを変更しない。値0をproduct defaultへの独自resetと解釈せず、受理済みhintを以後のdata/status evaluation cadence決定へ使用する詳細は既存の「`IDvr.setStatusCheckIntervalHint()` 契約」を正とする。


#### `DvrSettings` configure 完全契約

`IDvr.configure()` はunion tagがopen時のDVR kindと一致することを確認した後、`statusMask`、`lowThreshold`、`highThreshold`、`dataFormat`、`packetSize`を同一の `DvrSettingsSnapshot` としてvalidateし、全項目が成功条件を満たした場合だけ設定世代を一括commitする。いずれか1項目の拒否時は以前のsettings、FMQ read/write位置、queue identity、Record Filter relation、worker状態を変更しない。開始中のconfigureは前段lifecycle契約どおり`INVALID_STATE`を優先する。

- `statusMask=0` はstatus callbackを要求しない有効値として成功させ、データ経路は通常どおり動作させる。AIDLで既知のstatus bitのうち現行profileが生成できないbitは`UNAVAILABLE`、予約bit・未知bitは`INVALID_ARGUMENT`とする。成功後はmaskで選択したstatusだけをcallback対象にする。
- `lowThreshold` と `highThreshold` はbyte単位とし、`0 <= lowThreshold <= highThreshold <= openDvr(bufferSize)`を満たす場合だけ受理する。負値、大小逆転、FMQ容量超過は`INVALID_ARGUMENT`とし、clampしない。`lowThreshold == highThreshold` はAOSPが禁止していない有効入力として受理し、等値だけを理由に本製品固有の拒否条件を追加しない。
- 水位判定は、同一queue snapshotとcommit済み `DvrSettingsSnapshot` だけから決定し、別のwatermark state machineを設けない。Playbackは同一snapshotから `readableBytes = availableToRead` と `writableBytes = availableToWrite` を取得し、AOSP default HAL / VTSの歴史的behaviorに合わせて、`writableBytes == 0` なら `PlaybackStatus::SPACE_FULL`、それ以外で `readableBytes > highThreshold` なら `SPACE_ALMOST_FULL`、それ以外で `readableBytes < lowThreshold` なら `SPACE_ALMOST_EMPTY`、それ以外で `readableBytes == 0` なら `SPACE_EMPTY`、それ以外は直前のPlayback statusを維持して新しいstatus遷移を生成しない。したがってPlaybackでは `readableBytes == lowThreshold` / `readableBytes == highThreshold` をALMOST側の新規遷移条件にしない。Recordは `q = unconsumedBytes` とし、`q > highThreshold` なら `RecordStatus::HIGH_WATER`、それ以外で `q < lowThreshold` なら `LOW_WATER`、それ以外は直前のRecord statusを維持して新しいwatermark status遷移を生成しない。Recordでは `lowThreshold == highThreshold == T` かつ `q == T` の場合もLOW/HIGHのどちらにも新規遷移しない。Playbackのthreshold判定へunused/free bytesを代用せず、Recordの判定へunused bytesを代用しない。`RecordStatus::DATA_READY` はwatermark分類ではなく、Record outputが実際に成功commitされ読み取り可能になった事象として既存のdata-ready契約に従う。
- `dataFormat` は現行TS-only `ProductProfile`が扱うTS formatだけを成功させる。AIDL上既知だが現行profileで扱わないformatは`UNAVAILABLE`、予約値・未知値・未定義値は`INVALID_ARGUMENT`とする。
- `packetSize` は正のbyte数だけを構文上受理し、現行TS formatでは188だけを成功させる。正の別packet sizeは`UNAVAILABLE`、0以下は`INVALID_ARGUMENT`とし、`dataFormat`との組として検証する。
- `statusMask` は上記のsemantic status分類後に適用し、選択されているbitのstatusだけをcallback対象とする。分類されたstatusのbitがmaskされている場合、同じsnapshotで条件が重なる別statusへfallback / remapしない。開始直後の初期評価、threshold crossing / status変化、および受理済み`setStatusCheckIntervalHint()`による周期評価は同じ分類式と同じ`DvrSettingsSnapshot`を参照し、callback失敗は`PostCommitCallbackFailureTxn`へ接続する。

### `openFilter()` type/subtype acceptance と AV 入力固定契約

`IDemux.openFilter(type, bufferSize, callback)` の公開可否は、Android 14 AIDL V2 の main type / subtype と確定済み `CapabilitySnapshot` から一意に決める。AIDL上有効だが本製品が能力として採用しない main type / subtype は副作用なしの `UNAVAILABLE`、未知の列挙値、不正union encoding、main typeとsubtypeの組として成立しない入力は `INVALID_ARGUMENT` とする。拒否時はfilter ID、object、FMQ、callback artifact、使用枠を生成・消費しない。成功時の child-open reserve / prepare / commit / rollback は既存の `root/child open` 契約だけを正本とする。

| main type | subtype | `openFilter()` | `configure()` の対応settings | 通常payload / 補足 |
|---|---|---|---|---|
| TS | `UNDEFINED` | `linkCaps` で TS→TS を広告するため成功 | linkage検査用のsubtypeであり、具体filter settingsの成功対象を持たない。`configure()` は `UNAVAILABLE` | 通常payload FMQなし。`setDataSource()` のTS→TS linkageとdemux input復帰だけを成功対象にし、`start()` は未configure契約どおり `INVALID_STATE` |
| TS | `SECTION` | 成功 | `section` | 通常Filter FMQ |
| TS | `PES` | 成功 | `pesData` | 通常Filter FMQ |
| TS | `TS` | 成功 | `noinit` | raw TSの通常Filter FMQ |
| TS | `AUDIO` | AV filterの公開能力と既存child-open資源条件が成立する場合に成功 | `av` | `MediaEvent`。通常Filter FMQなし |
| TS | `VIDEO` | AV filterの公開能力と既存child-open資源条件が成立する場合に成功 | `av` | `MediaEvent`。通常Filter FMQなし |
| TS | `PCR` | 成功 | `noinit` | callback-only。通常Filter FMQなし |
| TS | `RECORD` | 成功 | `record` | Filter FMQ resourceをopen時に所有し`getQueueDesc()`でexport可能。payloadはRecord DVR FMQ、index metadataはcallback。Filter FMQへRECORD payloadを二重配送しない |
| TS | `TEMI` | `UNAVAILABLE` | なし | 本製品はTEMI処理能力を採用せず、object / queue / workerを生成しない |
| MMTP / IP / TLV / ALP | AIDL上の任意の既知subtype | `UNAVAILABLE` | なし | TS-only `ProductProfile`の固定能力外。main-type objectを生成しない |

上表の成功行でも、`bufferSize`、callback、個数枠、FMQ / AV / PES予算その他の既存child-open条件を満たさない場合は既存契約の原因別errorへ写像する。成功行を理由に資源不足を成功へ丸めない。TS `UNDEFINED` は `linkCaps` のmain-type接続試験を成立させるための公開subtypeであり、SECTION/PES/TS/AV/PCR/RECORDのいずれかへ暗黙昇格させない。

#### `DemuxFilterAvSettings.isSecureMemory` / `isPassthrough`

製品ライブAVアーキテクチャをclear-memoryかつnon-passthroughへ固定するproduct-level invariantは `../開発規則.md` を唯一の正本とし、本節では再定義しない。そのinvariantをAIDL入力契約へ写像し、非開始AV filterの `configure()` では `isSecureMemory=false && isPassthrough=false` だけをAV能力判定へ進める。`isSecureMemory=true` または `isPassthrough=true` はAIDL上有効だが本製品非対応の要求として、副作用なしの `UNAVAILABLE` とし、旧settings、shared backing、stream-type hint、source relation、generation、active `avDataId` を変更しない。開始済みfilterでは前段のlifecycle判定を適用し、有効だが未対応の値を評価して既存 `INVALID_STATE` を覆さない。未知tag / malformed settingsは `INVALID_ARGUMENT` とする。

`MediaEvent`出力側の `isSecureMemory=false` はこの入力validationの代替ではない。secure allocator、secure backing、secure handle種別、secure専用state machineを生成してはならない。

#### `IFilter.configureAvStreamType()`

AV stream type hintはopen subtypeが `AUDIO` / `VIDEO` のFilterだけが持つ直交設定軸とし、既存Filter ownerが所有する。routing種別そのものはopen subtypeを正とし、hint未設定または `UNDEFINED` でも `configure()`、`setDataSource()`、`start()`、PES/AV routing、`MediaEvent`配送の必須条件にはしない。

- TsAudio filterでは `AvStreamType.audio` tag、TsVideo filterでは `AvStreamType.video` tagだけを受理する。反対tagは `INVALID_ARGUMENT` とし、旧hintと全状態を維持する。
- matching tagの値が `UNDEFINED` なら成功し、既存codec hintを未指定へ置き換える。routing subtypeは変更しない。
- matching tagのAndroid 14 AIDLで定義済みのstream typeは、`UNDEFINED`を含めstream-type hintとして成功させる。stream typeはrouting subtypeそのものでもdemux codec capability advertisementでもないため、非公開のcodec別capability matrixを追加して公開APIの成功可否を狭めない。予約値・未知値は `INVALID_ARGUMENT` とし、旧hintと全状態を維持する。
- `OpenUnconfigured` / `ConfiguredStopped` では成功時にstream-type hintだけを原子的に置換し、configure状態、source relation、shared handle/backing、active allocation、queue/delivery generationを変更しない。同じ値の再指定は成功no-opとする。
- `Started` では、tag/value自体が有効な要求に `INVALID_STATE` を返して旧hintと全状態を維持する。close gateと共通入力検証優先順位は既存契約に従う。
- non-AV filterでは `UNAVAILABLE`、論理閉鎖開始後は `INVALID_STATE` とする。

stream-type hintの保存は、open subtypeから決まるAudio/Video routing、AV filterの公開可否、実payloadの検証・配送能力を代替しない。実際のAV data pathで未対応の入力を受理したふりをする根拠にも使わず、反対にprivateなcodec capability表だけを理由としてAIDL定義済みhintを拒否しない。

### `IFrontend.getHardwareInfo()`

公開するfrontendは、probe時に非空のhardware info文字列を確定できたものに限る。文字列はbackend種別、物理機器識別子、driver revisionなど、秘密情報を含まない不変情報から生成し、同じfrontend objectの寿命中は同じ値を返す。objectがLiveなら必ず成功して非空値を返し、部分値や空文字を返さない。閉鎖開始後は`INVALID_STATE`とする。probe時に生成できないfrontendは`getFrontendIds()`へ公開せず、VTSがopen済みfrontendで失敗する状態を作らない。


`IFrontend.getStatus(statusTypes)` と `getFrontendStatusReadiness(statusTypes)` では、返却件数の契約が異なる。`getStatus()` は、`FrontendInfo.statusCaps` で公開した種類だけを要求順に返し、公開済み種類の重複も維持する。未公開の既知値と、この実装がまだ認識しない将来の列挙数値は無視するため、要求がすべて非対応種類であれば空の配列を返して成功する。種類ごとの「取得不能」を表す架空値を生成してはならない。要求された公開済み種類の取得に1件でも失敗した場合は、部分結果を返さず `UNAVAILABLE` とする。`getFrontendStatusReadiness()` は、既知・未知にかかわらず要求された各要素に対し要求順で1件ずつ返す。公開済み種類には `UNAVAILABLE`、`UNSTABLE`、`STABLE` のいずれか、未公開の既知値と将来の列挙数値には `UNSUPPORTED` を返す。stable AIDLへ将来追加された列挙値だけを理由に要求全体を`INVALID_ARGUMENT`へ落とさず、現行Android 14の公開済み値に対するVTSの順序・件数・値の契約は維持する。出力件数を決める処理は両APIで共用しない。

公開済みstatusの値は、frontend runtimeが所有する世代付き`FrontendStatusSnapshot`だけから返す。tune/scan workerまたはbackend監視処理が、backend I/Oの完了後に同じfrontend generationを再検証してsnapshotを更新する。`getStatus()`と`getFrontendStatusReadiness()`はsnapshotを読むだけとし、backend I/O、probe、worker起動、状態変更を行わない。snapshotはstatus種別ごとに値、readiness、取得元、更新generation、単調増加する更新番号を持つ。新しいtune/scan、`stopTune()`、`stopScan()`、backend切断、fatal backend failure、`close()`では旧generationの値を無効化する。公開中の`DEMOD_LOCK`と`RF_LOCK`は、現generationで未取得または無効化済みなら`false`かつ`UNSTABLE`とし、fatal backend failureで値の正当性を保証できない場合は要求全体を`UNAVAILABLE`とする。時刻だけで同generationのlock値を失効させず、世代境界またはbackend事象で更新する。任意telemetryは、起動時に安定取得と更新経路を証明できない限り`statusCaps`へ公開しない。


`IFilter.setDataSource(source)` は、AOSP意味論どおり`source != NULL`では指定filter outputを入力元とし、`source == NULL`ではsink filterの入力元をdemux inputへ戻す。`setDataSource(NULL)`は現行設計の成功対象に含める。AOSP frozen/stable AIDLのvendor独自改変、raw Binder transaction parserによる公開契約を通さない実装は採用しない。source relationの変更は0-S-3Bの`SourceBoundaryTxn`、旧入力origin由来のpartial dataを新入力originへ連結しないstream/parser boundaryは`StreamBoundaryTxn`を唯一の正本とし、本節ではrelation phase、generation更新、parser mutationを再定義しない。

TS main typeの具体的linkageでは、実データを供給するsourceとして完全な188-byte MPEG-TS packet列を出力するTS `TS` subtypeを成功対象とする。そのpacket列は、各sinkの通常のPID/settings/filter semanticsで再処理するため、公開済みのTS `TS` / `SECTION` / `PES` / `AUDIO` / `VIDEO` / `PCR` / `RECORD` subtypeへ接続できる。source/sinkのconfigure前後にrelationを設定できる範囲は既存lifecycle契約に従い、後続`configure()`が既存relationと両立しない場合は前段契約どおり`INVALID_STATE`で旧settings/relationを維持する。TS `UNDEFINED`はVTSが`linkCaps`の広告closureを構成するために使用するAIDL定義済みsubtypeであり、TS→TSを広告する場合は同一demuxのTS packet source relationとして成功させる。内部packet mechanicsは`TS`/raw sourceと共有してよい。AOSP/VTSが定義しないprobe-only制約は追加せず、非広告main-type pair、加工済みSection/PES/AV/Record outputのTS source化は従来どおり`UNAVAILABLE`とする。Section/PES/AV/Record等の加工済みoutputをTS packet sourceとして再解釈する接続は、別の公開capabilityを定義しない限り成功対象へ追加しない。

`IFrontend.tune()`はbinder thread上でlock完了まで待ち続けない。同一の正規化settings、typed selector、LNB/power条件で既存lockを安全に継続できる場合は、既存streamを中断せず当該requestに対応する`LOCKED`を正確に1回配送する。それ以外の要求ではfull retuneとして扱い、破壊的遷移後に旧service由来のdataを新要求の出力として復元しない。入力分類、caller-visibleな結果、失敗後の公開状態は本節のfrontend各API契約を正とし、同値性snapshot、generation、worker/backend停止、boundary、commit / rollbackの内部semanticsはcanonical `frontend tune/scan`だけを正本とする。

無応答backendの製品watchdogはbackend別`ProductProfile.tuneTerminalDeadlineMs`を正とし、本製品ProductProfileではearth_pt1=`4000 ms`、px4=`7000 ms`とする。これはAIDL規定値ではなく、正常なbackend処理列を期限前に打ち切らないための製品値である。lockまたは明示失敗がないまま期限へ達した場合のcaller-visibleな結果は`NO_SIGNAL`を正確に1回通知して`Idle`とし、期限と既に確定したlockが競合する場合は`LOCKED`を優先する。cancel / stale通知fence等の内部処理はcanonical `frontend tune/scan` / `WorkerRuntime`参照とする。Android 14 AIDL VTSへ結び付けるprofileは、実信号でVTSの`WAIT_TIMEOUT=3秒`より前に`LOCKED`を通知できることを別の受入条件とし、VTS待機値を製品watchdogへ流用しない。

`IFrontend.scan()`は、最初の`scan(K)`で`LOCKED`を配送した後、同じsettings / scan typeの次の`scan(K)`に対して`END`を正確に1回配送し、2回目の`LOCKED`で補償しない。異なるrequestは新しいscanとして扱い、`stopScan()`はactive scanを停止する。`tune()` / `close()`等で継続条件を失った後に旧scanのterminal callbackを新operationの結果として配送しない。request fingerprint、scan generation、continuation state、worker/callback commit等の内部semanticsはcanonical `frontend tune/scan`だけを正本とする。最低試験は`scan(K)→LOCKED→scan(K)→END`、2回目の`LOCKED`がないこと、および異なるrequest / `stopScan()` / `tune()` / `close()`後に旧continuation結果が現れないことを確認する。

`IFrontend.close()` は、scan / tune worker、live pump、frontend backend、callback registration、demux relation、frontend leaseを当該frontend固有のcleanup対象とする。公開`close()`の戻り値、再`close()`時の結果、論理閉鎖後に許可する操作は0-S-4の公開close契約を正とする。cleanup authority、全対象試行、retry / handoff、quarantineの内部semanticsは0-S-3Bの`ObjectCloseTxn`を唯一の正本とし、本節では再定義しない。

DVB / earth_pt1 backend では、`DTV_CLEAR` は明示的な tune 停止操作である `stop_tune()` の責務とする。DVB backend の `close()` は reader stop と fd release を行うが、`DTV_CLEAR` の実行を close の必須条件とはしない。したがって、DVB `close()` が `DTV_CLEAR` を発行しないことを release blocker または bug と扱わない。

`IFrontend.removeOutputPid(pid)` は、本製品では frontend-level output PID removal を対応能力として採用しないため、常に `UNAVAILABLE` とする。soft demux 後段の block list だけで PID を捨てる経路を、frontend-level output PID removal の成功として扱わない。


### DVR playback status のAOSP互換判定

HIDL 1.0およびAndroid 14 AIDLの`PlaybackSettings`コメントはlow/high thresholdをplaybackのunused spaceとして記載する一方、AOSPのHIDL VTSとdefault HALの歴史的behaviorはqueue内のreadable data量を`SPACE_EMPTY` / `SPACE_FULL`およびALMOST判定の基準としている。本製品はAndroid互換性を優先し、後者のAOSP実装/VTS behaviorへ合わせる。

同一FMQ snapshotから `readableBytes = availableToRead` と `writableBytes = availableToWrite` を取得し、次の順に最初に成立したstatusを採用する。

1. `writableBytes == 0` → `PlaybackStatus::SPACE_FULL`
2. `readableBytes > highThreshold` → `PlaybackStatus::SPACE_ALMOST_FULL`
3. `readableBytes < lowThreshold` → `PlaybackStatus::SPACE_ALMOST_EMPTY`
4. `readableBytes == 0` → `PlaybackStatus::SPACE_EMPTY`
5. いずれも成立しない → 直前のPlayback statusを維持し、新しいstatus遷移 / callbackを生成しない

`readableBytes == lowThreshold` / `readableBytes == highThreshold` はALMOST側への新規遷移条件にしない。status名をunused/free space量へ読み替えてEMPTY/FULLを反転させず、同一snapshotで上記の優先順位を変更しない。


## Tuner HAL 状態遷移表SSOT

本節は、Tuner HAL の状態を持つ公開API、内部事象、資源寿命、戻り値、副作用のSSOTである。現行契約は以下の名前付き節および0-S-3Bの論理契約名で参照し、移送済み旧表番号を参照名として使用しない。


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
| Record Filter output position | `FilterProducerDrainGate` | なし | `record_output_byte_offset`を`PacketPipeline` / `RecordDvrFilterRelationTxn` / `QueueEpochProtocol` / `StreamBoundaryTxn`から直接変更しない |
| DVR lifecycle | `DvrLedger` | `DvrHal`, `soft_demux` | DVR object と soft demux DVR record の片方だけを残さない |
| playback queue | `PlaybackQueueBacking` | playback ワーカー | ワーカー失敗だけでDVRをclosed扱いにしない |
| descrambler session | `DescramblerSession` | `DescramblerLedger` | demux binding、PID、source filter、key tokenを別々に閉じない |
| key token | `DescramblerKeyTable` | `DescramblerSession` | refcount不整合のまま成功扱いしない |
| LNB永続制御・物理レール状態 | `LnbRegistry` | backend adapter | 電圧・トーン・衛星位置、LNB状態世代、失敗・隔離状態、共有レールのリース・参照数、物理LNB・レール単位のI/O直列化権限の正本を一箇所に保持し、backend状態とregistry状態の乖離を通常状態にしない |
| ワーカー | `WorkerRuntime` | 所有オブジェクト | 所有者不明ワーカーを作らない |
| stream boundary | `StreamBoundaryTxn` | demux/filter/DVR/AV | tune/scan/source切替で境界処理を重複実装しない |
| packet pipeline | `PacketPipeline` | `soft_demux` | packet validation・origin分類・data-path dispatchを担当し、continuity / generation stateを各canonical ownerを迂回して更新しない |
| AV shared memory | `AvSharedBacking` | `FilterHal` | fd番号一致をshared handle同一性条件にしない。fd付きhandle + `avDataId == 0` はclient側shared handle使用終了通知として扱う |

未解放AV allocationの正本は、正の`avDataId`をkeyとするactive token台帳に記録した`{owner_filter_id, filter_generation, transfer_kind, backing_id, allocation_id, avDataId, lease_state}`とする。ファイル記述子番号や`fstat`の`{st_dev, st_ino, size}`をallocation identityの正本にしてはならない。`fstat`などの記述子メタデータは、採用した記憶領域実装で同一性が保証される場合だけ補助検証と診断に用いる。共有ハンドルの`dataId=0`解放は呼出先IFilterが所有するboundedなクライアント使用権を対象とし、正の`dataId`解放はactive token台帳で当該IFilterの未解放allocationと転送方式を特定して行う。allocation解放成功時はtoken entryを削除し、その後activeでない正のtokenは`INVALID_ARGUMENT`として資源を変更しない。解放済みduplicate、foreign、never-issuedを永久に識別するためのtombstoneは持たない。台帳の信頼性を確認できない場合は`UNKNOWN_ERROR`とし、安全を確認できない記憶領域を隔離する。


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

各操作は準備、主要状態の確定、確定後処理の順に実行する。主要状態の確定では正本状態、所有権、backendへの反映を扱い、確定後処理ではcallback、状態通知、診断、後片付けの集計を扱う。確定後処理の失敗は型付きの副次結果として保存し、主処理の戻り値を変更しない。ただし、API別状態表がコールバック経路の準備または登録を成功確定条件に含める操作では、その処理を確定前に実行する。この共通則を使ってAPI別の成功条件を確定後処理へ移してはならない。


| 段階 | 許可 | 禁止 |
|---|---|---|
| rollback | commit前変更の取り消し | 失敗を握りつぶして通常状態へ戻すこと |
| quarantine | rollback不能資源の隔離 | 成功扱いで通常操作を許すこと |

commit前失敗では、成功戻りを返してはならない。commit後cleanup失敗では、APIの戻り値方針を各API表で固定し、必ず診断に残す。rollback失敗時は、対象資源を quarantine または failed 状態へ落とす。


#### 0-S-3A. 共通部品適用表

本表は、処理領域と0-S-3Bのcanonical contractの対応、および統合してはならない責務境界だけを示す。state、phase、commit point、rollback / cleanup、failure semantics、relation cardinalityは0-S-3Bだけを正本とし、本表では再定義しない。

| 対象処理 | canonical contract | 統合してはならない責務 |
|---|---|---|
| public close / owner loss / Drop | `ObjectCloseTxn` | API / Drop / Reaperごとに別のclose契約を持たない |
| Filter source relation | `SourceBoundaryTxn` | demux/frontend relationを統合しない |
| Demux frontend source relation | `DemuxFrontendSourceTxn` | stream boundary内部を所有しない |
| stream data boundary | `StreamBoundaryTxn` | relation、Filter/DVR queue内部、A/V sync、PCR、callback、descramblerを所有しない |
| callback registration | `CallbackRegistrationUseCase` | Binder artifact、runtime registry、domain logical stateのownerを統合しない |
| Frontend / LNB assignment relation | `FrontendLnbRelationTxn` | LNB persistent control stateを統合しない |
| LNB永続制御手順 | `LnbControlTxn` | 永続状態・世代・失敗・隔離・物理I/O直列化は`LnbRegistry`を正本とし、DiSEqC一時送信を同手順へ統合しない |
| descrambler PID mutation | `DescramblerPidTxn` | key mutation / session cleanupを統合しない |
| descrambler key mutation | `DescramblerKeyTxn` | PID mutation / session cleanupを統合しない |
| descrambler session cleanup | `DescramblerSessionCleanupTxn` | normal PID / key mutationのownerにしない |
| Record DVR / Filter relation | `RecordDvrFilterRelationTxn` | DVR側とFilter側に別のrelation正本を持たない |
| worker lifecycle mechanism | `WorkerRuntime` | domain固有のstart / stop state machineを統合しない |
| worker failure classification | `WorkerFailureClassifier` | lifecycle、retry / cleanup、公開状態遷移を所有しない |
| domain commit後callback failure | `PostCommitCallbackFailureTxn` | commit済みdomain stateを所有しない |
| Filter producer drain | `FilterProducerDrainGate` | Filter `flush()`全体またはDVR queue stateを所有しない |
| DVR queue epoch | `QueueEpochProtocol` | Filter stateまたはDVR `flush()`全体を所有しない |
| Filter / DVR `flush()` cleanup orchestration | `QueueCleanupUseCase` | 下位protocol内部stateを所有しない |
| DVR playback read/inject | `PlaybackConsumeTxn` | worker / FMQ helperへconsume state machineを分散しない |
| A/V sync relation | `AvSyncRegistry` | PCR anchorとownerを統合しない |
| PCR clock anchor | `PcrClockAnchorStore` | A/V sync relationとownerを統合しない |

#### 0-S-3B. 共通部品の規範定義

次表の10項目を満たしたものだけを共通部品の設計正本とする。物理 module / file / type アンカーは`tuner_hal2/DESIGN_JA.md`の「共通transaction / use-caseの規範実装アンカー」を単一正本とし、本書の「実装正本」列はその同名論理契約行への参照だけを持つ。

| 論理契約名 | 実装正本 | 公開入口 | 所有する状態 | 所有しない状態 | phase order | 失敗時処理 | 呼び出し許可層 | 呼び出し禁止層 | 最低テスト |
|---|---|---|---|---|---|---|---|---|---|
| `ObjectCloseTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | public `close()`、owner loss/Drop、shutdown/reaper retryはいずれも同じ`begin_close`入口 | `CloseCleanupAuthority`、未完step、cleanup report、retry/reaper handoff、早期再開要求、完了確定 | API固有入力、backend内部状態、queue parser内部 | `begin_close` atomic commit（logical close確定 + 新規通常操作遮断 + authority取得） → authority下でtyped cleanup全件試行 → unregister/release → complete/pending | 全stepを試行し、retryableは`CleanupPending`。再`close()`、owner消滅、依存資源の完了通知、service初期化は未完cleanupの早期再開を要求できるが、これらは唯一の進行契機ではなく、`WorkerRuntime`の自律retryを停止しない。authority取得後にcaller/ownerが消滅した場合は未完authorityを回収機構へ一度だけ移管し、実状態不明/遮断不能だけquarantine。主障害とcleanup障害を別保持 | object close façade、owner-loss/Drop façade、reaper | AIDL method body、Drop、worker、backendが独自cleanup authorityを持つこと、logical closeとauthority取得の分離commit | close-vs-Drop race、`begin_close` atomicityと中間状態不存在、owner-loss/Drop handoffの一回性、途中失敗後も後続cleanup実行、retry、二重release防止、quarantine |
| `SourceBoundaryTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | `IFilter.setDataSource()` use-case、`ObjectCloseTxn`/source unlink cleanupからのtyped relation mutation | Filter source/sink relationとそのrelation generation、committed source relation graphの非巡回不変条件 | demux/frontend relation、DVR queue、A/V sync/PCR内部 | validate（candidate edge追加後も有向cycleを作らないことを含む） → relation prepare → source boundary prepare → commit / rollback | cycleを作るcandidateは`INVALID_ARGUMENT`で状態不変。pre-commitは旧relation維持、確定不明だけ対象relationを隔離 | Filter source use-case、`ObjectCloseTxn` typed cleanup command | API wrapper/workerのgraph直接変更、Demux frontend use-case | 初回non-NULL接続、NULL/demux-input経路、self-loop、2-node/3-node cycle拒否、wrong demux/owner、closed/generation、source close/unlink cleanup、prepare/commit fault |
| `DemuxFrontendSourceTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | `IDemux.setFrontendDataSource()` use-case、`ObjectCloseTxn`/Frontend・Demux cleanupからのtyped unbind mutation | demux/frontend relationのorchestration | stream parser内部、Filter source graph | validate → relation prepare + `StreamBoundaryTxn.prepare()` → composite commit（新relation・stream generation・旧relation logical detach） → old relation physical cleanup | pre-commitは両prepared stateをabortし旧relation/旧generation維持。commit結果不明だけ対象demuxを隔離。composite commit成功後のold relation physical cleanup失敗では新relationをrollbackせず、旧relation cleanupだけをretryable cleanupへ移管し、旧資源の実状態不明時だけ旧relation資源をquarantine | Demux frontend-source use-case、`ObjectCloseTxn` typed cleanup command | `SourceBoundaryTxn`への吸収、relation/stream別commit、post-commit cleanup失敗による新relation rollback | same-source no-reset、switch/unbind、Frontend/Demux close cleanup、cleanup idempotence、boundary prepare failure、composite commit fault、post-commit old relation cleanup failureで新relation維持 |
| `StreamBoundaryTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | typed stream-boundary use-case、上位transactionからの`prepare()` | `stream_boundary_generation`、`PreparedStreamBoundary`、各data-path ownerへのtyped reset / invalidate dispatch | steady-state PID continuity、section/PES/TableInfo/record-index parser・assembler・tracker state、relation table、Filter/DVR queue内部、A/V sync/PCR内部、callback、descrambler | validate → 次`stream_boundary_generation`を`checked_add()`でprepare → 各steady-state ownerからtyped reset tokenをprepare → `PreparedStreamBoundary` commit / abort | abortでは旧boundary generationと各steady-state stateを維持。generationを発行できない場合はwrap / reuseせず当該stream boundaryを局所`Quarantined`とする。commit不明時だけ対象streamをfail/quarantine | service_runtime packet/boundary use-case、上位relation transaction | API/worker/helperによるboundary generation直接変更、`StreamBoundaryTxn`によるsteady-state parser/continuity state直接所有 | standalone commit、prepared abort/commit、stale generation、generation exhaustion、continuity/parser/TableInfo/record-index ownerへのtyped reset、relation composite atomicity |
| `CallbackRegistrationUseCase` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | Frontend/LNB等のset/clear/replace callback。AIDL façadeはartifact prepare/releaseだけを行い、prepared artifact handleをservice_runtime ownerへ渡す | service_runtime側registration transactionのorchestration、prepared runtime registry mutation、domain callback logical stateのcommit/rollback policy。Binder artifact本体はcallback storeが所有 | Binder artifact storage/strong ref、callback配送後のdomain state、backend state | AIDL artifact prepare（非公開） → service_runtime runtime registry prepare → domain callback state prepare → service_runtime composite commit（prepared artifact handle採用 + runtime mutation + domain logical state） → AIDL old artifact cleanup | composite commit前はservice_runtime ownerがruntime/domain prepared stateをabortし、AIDL façadeへprepared artifact releaseを指示して旧callbackを維持。commit後のold artifact cleanup失敗では新registrationをrollbackせずcleanup/診断へ接続し、callback delivery失敗は`PostCommitCallbackFailureTxn` | service_runtime callback registration owner、AIDL artifact prepare/release façade | AIDL façadeによるdomain state/rollback policy所有、LNB/domain/backend/resource ledgerのBinder strong ref直接保持、artifact/runtime/domainの片側commit | set/replace/NULL、各prepare failure、composite commit fault、old artifact cleanup failureで新registration維持、Binder death/generation |
| `FrontendLnbRelationTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | `IFrontend.setLnb()` use-case、Frontend close時は`ObjectCloseTxn`からのtyped assignment release | frontend→LNB assignment relation、prepared assignment lease-reference mutation、transaction authority、旧assignment cleanup record | LNB endpoint lease pool内部、共有railの物理状態、`LnbRegistry`の永続制御状態・共有レール物理状態、`ILnb` object lifecycle | frontendとLNB endpointのlive/owner/generation/type/connectability・shared rail互換性を同一snapshotでvalidate → LNB resource ownerからassignment lease-reference mutationをprepare → relation prepare → composite commit（新relation + 新lease参照 + 旧relation logical detach） → 旧assignment lease参照cleanup | commit前失敗はprepared relation/lease mutationをabortして旧relation/旧lease参照を維持。commit結果不明時だけ当該frontend assignmentと関係claimをquarantine。commit後の旧lease cleanup失敗では新relationをrollbackせず旧lease cleanupだけをretry/quarantineへ接続。Frontend closeは同ownerのtyped releaseでassignmentを解放 | frontend object-method use-case、`ObjectCloseTxn` typed cleanup command、LNB resource ownerのprepared lease入口 | AIDL wrapper/frontend use-case/LNB registryによるrelation・leaseの別commit、`LnbControlTxn`への吸収、LNB lease pool内部の直接変更 | 初回assignment、同一assignment再設定、別LNBへの更新、non-satellite/wrong owner/stale generation/incompatible rail/capacity、prepare/commit fault、post-commit旧lease cleanup failureで新assignment維持、Frontend closeでassignment release |
| `LnbControlTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | `setVoltage()` / `setTone()` / `setSatellitePosition()` | 1回の制御要求の調停、`LnbRegistry`が発行する準備済み制御変更と物理I/O権限を一回だけ消費する呼出し単位の進行状態 | LNB永続制御状態、LNB状態世代、失敗・隔離状態、共有レールのリース・参照数、物理I/O直列化状態、DiSEqC一時送信、コールバック、endpoint lease | 検証 → `LnbRegistry`の状態読取り値から物理I/O競合単位を特定 → registry状態ロックを保持せず当該I/O権限取得 → 寿命・世代・リース再検証 → 次世代と候補値の準備済み変更を`LnbRegistry`で確保 → backendへ適用 → 成功なら準備済み変更を確定、明示拒否なら取消し、結果不明なら失敗・隔離を`LnbRegistry`へ確定 → I/O権限解放 | 世代を発行できない場合はbackend適用前に`LnbRegistry`が対象LNBを`Quarantined`とする。backend適用成功後の準備済み変更の確定は、I/O権限と準備済み変更権限の保持中に追加の資源割当またはbackend I/Oを行わず失敗不能とする。明示拒否または機器状態不変を確認できる失敗は準備済み変更を取り消してregistry状態不変とする。backend反映結果が不明な場合だけ`LnbRegistry`へ失敗・隔離状態を確定する。`LnbControlTxn`自身は呼出しを越える失敗状態または世代を保持しない | LNB object use-case | 3 APIの個別state machine、`LnbRegistry`を迂回するbackend I/O、DiSEqCの手順統合 | 3操作、invalid/unavailable、generation exhaustion、backend rejected/indeterminate、close race、固定給電・safe-state・DiSEqCとの物理I/O直列化 |
| `DescramblerPidTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | `addPid()` / `removePid()` use-case | PID tuple、pool PID claim、backend packet-path apply、compensation | key refcount、session close/pool session lifetime | validate → claim/prepare → backend apply → PID ledger commit → compensation on failure | pre-commit rollback、backend適用後commit失敗はcompensation、compensation不能/実状態不明だけquarantine | descrambler PID use-case | AIDL/packet helperのclaim/backend/ledger直接変更 | add/remove idempotence、NULL/non-NULL source、wrong owner/generation、capacity、backend/commit/compensation fault |
| `DescramblerKeyTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | `setKeyToken()` use-case | key token/refcount/session-key mutation | PID relation、session cleanup | validate → new key acquire/prepare → backend apply → session/key-table commit → old ref release | pre-commit rollback、refcount/commit不整合は対象session/key tableをfail/quarantine | descrambler key use-case | PID/cleanup path、AIDL direct key table mutation | valid/invalid/VOID/same/replacement、backend fault、commit/refcount fault |
| `DescramblerSessionCleanupTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | descrambler closeは`ObjectCloseTxn`からtyped cleanup command、demux invalidationはdemux invalidation ownerからtyped cleanup request | sessionに属するPID/key/pool帰属のcleanup進捗 | public close authority、normal key/PID mutation、他session | trigger（close authorityまたはdemux invalidation generation）確認 → session cleanup直列化 → backend detach全件 → claims/key refs/pool session release → report | 全対象を試行し、close起因retryableは`ObjectCloseTxn`の`CleanupPending`へ、invalidation起因retryableはdemux invalidation ownerへtyped pending結果を返してcleanup/reaperで再試行。状態不明だけ対象sessionをquarantine | `ObjectCloseTxn` typed cleanup command、demux invalidation owner | public API/workerによる個別release、demux invalidationをpublic close authorityとして扱うこと | close/invalidateの別入口、partial cleanup、retry、idempotence、quarantine |
| `RecordDvrFilterRelationTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | attach/detach、Filter/DVR close、demux cleanup | Record DVR/Filter relationの単一正本 | Filter/DVR lifecycle本体、queue payload | validate both objects → relation prepare → union-route prepare → single commit / abort | pre-commit旧relation維持、commit不明時だけrelation/routeをfail | DVR/Filter relation use-case、close cleanup command | DVR/Filter両側のshadow relation別commit | duplicate attach、absent detach、wrong owner/demux/kind、close/detach race、commit fault |
| `WorkerRuntime` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | 各domain worker ownerのspawn/stop/wake/join/reaper | owner generation / signal generation、stop signal、JoinHandle、fence、reaper handoff mechanism、有界`ReaperSupervisor` work queue、retry schedule / coalesce state、typed worker terminal result（`Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure`） | domain start/stop state、backend semantic failure、queue payload | owner handle slot prepare → 次generationを`checked_add()`でprepare → fenced worker spawn / handle bind → signal stop → wake/cancel → observe/join または one-shot reaper handoff → handoff済みworkを既存retry scheduleで外部API再呼出しなしに自律再試行 → worker実終了と依存cleanup完了後にlease/slot release | handle slot準備失敗ではspawnしない。owner/signal generationを発行できない場合はwrap / saturating reuseや存在しない次generationでのreplacement spawnを行わず、現generationをfenceして停止・回収し、影響するowner/generation/resourceだけを`Quarantined`とする。取消generationごとのstop/wake通知は各1回までとし、終了済みworkerは直ちに回収する。failureはtyped reportしleaseを早期再利用しない。`ReaperSupervisor`へのenqueueおよび早期再開要求は `(owner, generation, dependency resource)` ごとにcoalesceし、同一未完workを重複実行しない。外部APIの再呼出しがなくても有界work queueがretry scheduleに従って進行する。terminal budgetは`cleanupRetryScheduleMs=[0,10,100,1000]`後1000 ms間隔、`cleanupTerminalDeadlineMs=30000`、`workerIoDeadlineMs=2000`、`workerReaperDeadlineMs=10000`。deadline到達後もowner generation無効化で副作用を遮断できる場合は対象owner/generation/resourceだけを`Quarantined`とする。無効化後もservice-global stateを変更可能、遮断不能なservice-wide exclusive resourceを保持、owner/generation/resource tokenで遮断不能、または同一資源のreplacement/restartと競合可能というtyped evidenceがある場合だけ`ServiceCritical`とする | domain worker owner、cleanup/reaper | 別のgeneric worker lifecycle ownerの追加、AIDLからの直接join | handle-slot failureでno-spawn、generation exhaustionでwrap/reuse/replacementなし、typed terminal result、stop/wake一回性、generation fence、join/one-shot reaper、外部API再呼出しなしの自律retry、早期再開要求のcoalesce、panic、no early reuse、deadline branch、local quarantine対ServiceCritical判定 |
| `WorkerFailureClassifier` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | worker owner / cleanup managerからのtyped failure | stop/wake/join/EventFlag/Reaper/backend-control/callback等の失敗種別分類だけ | worker lifecycle、停止順序、retry/cleanup、quarantine、公開状態遷移 | typed/raw failure受理 → source/domainをtyped分類 → ownerへ分類結果返却 | 文字列推測・API別errno推測を禁止し、unknownもtyped分類として返す。分類器自身はstate mutationしない | worker owner、cleanup manager、callback/backend failureを扱うowner | classifierからdomain/public stateを直接変更すること、owner側で同型分類を再実装すること | stop/wake/join/EventFlag/Reaper/backend-control/callback分類、owner間同一分類、state不変 |
| `PostCommitCallbackFailureTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | domain commit済みcompletion use-caseから`WorkerFailureClassifier`で分類済みのtyped callback failureを受け取る | callback health、delivery outcome、診断への写像と確定 | commit済みdomain state、backend state、failure category分類 | verify post-commit → classified typed callback failure受領 → delivery outcome / health / diagnostic commit | failure categoryを再分類せず、domain rollback禁止・public結果維持。分類済みcategoryからcallback health/delivery outcome/診断だけを確定 | Frontend/Filter/DVR等のcompletion use-case | API別rollback handler、文字列/errno再分類、failure categoryの再分類 | Frontend tune、Filter/DVR start、分類済みmissing/store/Binder failureのcategory維持、domain unchanged、classifier二重呼出しなし |
| `FilterProducerDrainGate` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | Filter/SharedFilter producer、`QueueCleanupUseCase`からのtyped drain request | `Open`/`Draining`/`Closed`、`filter_delivery_generation`、`parser_state_generation`、`admitted_producer_count`、bounded pending event queue、`FilterProducerPermit(g)`、Record Filter output lifetime単位の`record_output_byte_offset` | FMQ内容、DVR token/epoch、flush全体のorchestration | `Open`でadmit/permit発行 → producer commit/finishでpermit解放。Record FilterではRecord DVR output commit成功と同じ確定点で実commit byte数だけ`record_output_byte_offset`を進め、commitしなかったdataでは不変とする → drain開始を`Draining`へ線形化し新規admit拒否 → admitted producerとpending eventを排出 → 次generationを`checked_add()`でprepareしてgeneration/parser stateを確定し`Open`へ戻す、またはcloseで`Closed`。drain / generation更新では`record_output_byte_offset`をresetしない | panic/returnでもpermit解放。generationを発行できない場合はwrap / saturating reuseせず対象Filterを`Quarantined`として`Open`へ戻さない。drain中は旧generationのproducer/eventを新generationへ確定せず、その他の遮断不能failureだけFilter fail。Record DVR output commit失敗では`record_output_byte_offset`と対応するRecord eventを確定しない。`QueueCleanupUseCase`はtyped入口の結果だけを集約 | data producer、`QueueCleanupUseCase` | Binder callback/IO/joinをpermit内に保持、`QueueCleanupUseCase`/API/workerがgate内部stateを直接変更、DVR stateの吸収、他ownerによる`record_output_byte_offset`直接変更 | Open/Draining/Closed遷移、flush中の新規permit拒否、全permit/pending event排出、generation/parser更新、Record output初期offset 0、commit成功時だけ実commit byte数分進行、commit失敗時不変、flush/reconfigure/source/stream boundary・Record DVR relation変更でoffset維持、新Filter output lifetimeで0、generation exhaustionで非再利用・quarantine、panic/drop、commit前失敗時の旧state維持、共通orchestratorからのdrain |
| `QueueEpochProtocol` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | DVR data path、`QueueCleanupUseCase`からのtyped flush request | `Open(g)`/`Draining(g)`/`Closed`、`queue_epoch`、一回限りのread/write transaction token、受付中transaction数 | `queue_identity`（`PlaybackQueueBacking`所有）、Filter producer、DVR parser/stats、flush orchestration | `Open(g)`でbegin/token発行 → commit/cancel/dropでtokenを一回消費 → flush開始を`Draining(g)`へ線形化して新規begin拒否 → 受付中transaction排出 → epoch prepare/commitで`Open(g+1)`、closeで`Closed` | stale token・二重token消費を拒否し、flush commit前失敗は旧`Open(g)`/epoch/positionを維持 | DVR data path、`QueueCleanupUseCase` | Filter path、API別token state machine、orchestratorの内部state直接変更 | Open/Draining/Closed遷移、一回性token、flush race、commit前状態不変、stale token、identity ABA |
| `QueueCleanupUseCase` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | Filter / DVR `flush()` use-case | cleanup orchestration plan、typed下位protocol呼出順序、共通失敗集約/result composition | Filter producer permit/state、DVR queue token/epoch、API固有eligibility/公開状態 | API ownerが対象確定 → typed drain/cleanup request → 全対象結果集約 → API ownerへtyped result返却 | 下位protocol失敗を成功へ丸めず全対象を試行し、API固有state transitionは各ownerへ返す | Filter/DVR flush use-case | 下位protocol内部stateの直接変更、non-flush API、API別orchestration複製 | Filter/DVR双方が同じorchestratorを通る、下位state独立、partial cleanup failure、result aggregation |
| `PlaybackConsumeTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | playback workerの1 consume step | FMQ read transaction、processing buffer、parse/inject cursor、consume result | worker lifetime、queue epoch owner | beginRead → copy → commitRead → parse → inject incrementally → finish/retain | retryable injectはbuffer/cursor保持、stop保持、flush/close/fatalは損失診断して破棄 | playback worker | FMQ/helperの独自consume state machine | partial TS、partial inject、retry、stop→start preserve、flush discard |
| `AvSyncRegistry` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | AV/PCR filter configure/unregister/close、demux close | `media_filter_id -> hw_sync_id`のmany-to-one relation。reverse indexを物理保持する場合だけ`hw_sync_id -> Set<media_filter_id>`を同ownerで持ち、injective / bijectiveは要求しない | PCR clock anchor、filter lifecycle本体 | validate → forward relation mutation prepare → optional reverse-index mutation prepare → outer transaction commit/abort | abortでforward relationとreverse indexを不変に保ち、片側だけの確定を通常状態にしない。1 filterのunregisterで同一`hw_sync_id`を共有する他filterのrelationを消さない | filter/demux lifecycle transaction | API/Filter wrapper/StreamBoundaryのrelation/reverse index直接更新、reverse indexを`hw_sync_id -> media_filter_id`へ縮退 | register、複数media filterによる同一hw sync ID共有、reconfigure、1 filterだけのunregister、filter/demux close、abort、forward/reverse整合、non-injective relation |
| `PcrClockAnchorStore` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | PCR観測、stream/filter boundaryからprepared invalidation | generation-scoped `PcrClockAnchor { raw_pcr_base_33, unwrapped_pcr_90k, monotonic_base_ns, generation }` とその観測・無効化状態 | A/V sync ID relation、stream generation本体 | current generationの最初の有効PCR観測でanchorを生成 → 同一generationかつ`discontinuity_indicator`なしの後続PCRは33-bit baseを前向きにunwrapして観測monotonic時刻へanchor更新。`discontinuity_indicator`、前向きwrapとして解釈不能なPCR逆行、PCR PID/source filterの置換・再設定・`flush()`・`stop()`・`close()`、demux input generation変更、frontend retune・`stopTune()`・`close()`、playback sourceの`flush()`/resetはprepared invalidation → outer commit / abort | stale generationを拒否する。`now_monotonic_ns < monotonic_base_ns`の時計異常ではcurrent anchorを無効化する。prepared invalidationのabortでは旧anchor維持、commit後は旧generation anchorを再利用せず、新しい有効PCR観測までanchorなしとする | PCR data path、StreamBoundary/filter lifecycle | API/StreamBoundaryによる内部直接変更 | 初回PCR observe、同一generation update、33-bit wrap、時計逆行、discontinuity/PCR逆行、flush/stop/close/input-gen/retune/playback flush/reset invalidation、stale generation、prepared abort/commit |

#### 0-S-4. 失敗分類と波及範囲

| 失敗種別 | 例 | 戻り値 | 波及範囲 | 禁止事項 |
|---|---|---|---|---|
| クライアント誤用 | 引数不正、owner不一致 | `INVALID_ARGUMENT` | 呼び出し対象のみ | backend/データ経路 failureへ昇格しない |

公開 `close()` の状態別結果は本段落を正とする。`Live` objectへの最初の`close()`は0-S-3Bの`ObjectCloseTxn`へ接続し、logical close確定後は回復用入口を除く通常methodを拒否する。`LogicalClosed+CleanupComplete`では`IFrontend.close()` / `ILnb.close()`は状態を変えず`SUCCESS`、`IDvr.close()` / `IFilter.close()`は`INVALID_STATE`とし、Filterの遅延`releaseAvHandle()`は別の解放台帳操作として扱う。`LogicalClosed+CleanupPending`の再`close()`は`ObjectCloseTxn`のrecovery入口へ接続し、そのtyped resultを公開戻り値へ写像する。`CleanupPending` / `Quarantined`への遷移条件、未完cleanupのretry、authority handoff、reaper移管、quarantine判定は0-S-3Bの`ObjectCloseTxn`を唯一の正本とし、本節では再定義しない。


| 失敗種別 | 例 | 戻り値 | 波及範囲 | 禁止事項 |
|---|---|---|---|---|
| unsupported | capability外、恒久非対応 | `UNAVAILABLE` | なし | callback/ワーカー状態を先に見て別エラーにしない |
| コールバック失敗 | Binder コールバック失敗 | API表に従う | コールバック所有者 | データ経路全体を即failedにしない |

ワーカー関連の失敗種別は`WorkerFailureClassifier`だけがtyped分類する。対象にはstop/wake/join/EventFlag/Reaper/backend-control/callback等の発生源を含めるが、分類器が所有するのは分類結果だけであり、停止順序、retry、cleanup、quarantine、公開状態遷移は各worker owner/API契約に残す。FMQ payload commit後のEventFlag起床失敗についても、payload保持・再起床というdata-path状態機械はqueue runtimeが所有し、classifierは失敗種別を分類するだけとする。


| 失敗種別 | 例 | 戻り値 | 波及範囲 | 禁止事項 |
|---|---|---|---|---|
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


公開APIの戻り値は、不存在または別所有者のIDを`INVALID_ARGUMENT`、同一サービス内の閉鎖済みオブジェクトを`INVALID_STATE`とする。通常のメモリ割り当て、FMQ領域、共有メモリ、dma-bufなど要求byte容量の不足が確定した場合は`OUT_OF_MEMORY`、frontend・demux・filter・DVR・worker slot・leaseなど論理資源の使用枯渇は`UNAVAILABLE`、依存資源の未初期化は`NOT_INITIALIZED`、内部不整合・破損、原因不明、または割り当て結果・副作用を確定できない内部障害は`UNKNOWN_ERROR`とする。個別API表がlifecycle、入力、未対応、確定前後のpriorityを定める場合はその表を優先する。実体のないオブジェクトを生成せず、自動的な隔離も行わない。


- filter ID は HAL 外部へ返す値を demux-local ID のまま維持する。DVR attach/detach、filter データ入力元、AV sync ID 取得では、渡された filter オブジェクト の内部 owner demux を検証し、owner demux が一致しない filter を `INVALID_ARGUMENT` で拒否する。
- generic workerのhandle/spawn ownership、typed terminal result、stop / wake / join、generation fence、reaper handoff、lease returnは0-S-3Bの`WorkerRuntime`を唯一の正本とする。failure categoryは`WorkerFailureClassifier`を正とする。mutex / condvarのproject-wide規約は`../GLOBAL_CODE_CONVENTION.md`、product defaultのspawn / join等の実装規約は`../tuner_hal2/CODE_CONVENTION.md`を正とし、本節では再定義しない。

ワーカーが利用するbackend pathは、停止通知またはFD closeで有限時間に復帰すること、driver/kernel契約から導出した内部I/O上限内に復帰すること、または副作用をowner境界で遮断できる隔離実行であることのいずれかを能力公開前に証明する。いずれも証明できないbackend pathは能力として公開しない。選局成否を固定時間で覆す専用timing profileは設けない。取消、回収、reaper、lease return、terminal budget、`Quarantined` / `ServiceCritical`分岐のgeneric semanticsは0-S-3Bの`WorkerRuntime`を唯一の正本とし、本節では再定義しない。

generic worker failureの隔離範囲と`ServiceCritical`昇格条件は0-S-3Bの`WorkerRuntime`を唯一の正本とし、failure categoryは`WorkerFailureClassifier`を正とする。各domain節は分類済みterminal resultをAPI固有状態・公開結果へ写像する責務だけを持ち、service-wide failure判定条件を再定義しない。


- frontend source transitionでは、API成功時に要求したfrontend source assignmentが成立していることを公開意味として固定する。relation / stream boundaryのprepare、composite commit、rollback、post-commit cleanup、commit不明時処理は0-S-3Bの`DemuxFrontendSourceTxn` / `StreamBoundaryTxn`を唯一の正本とし、本節では再定義しない。


- DVR `start()` は`setStatusCheckIntervalHint()`で受理済みのhint時間だけBinder threadをsleepしない。受理済みhintは以後のdata/status evaluation cadenceの決定に使用し、queue内容、DVR lifecycle、playback/record stateのmutation条件には使用しない。

キュー、機器、パケットの各読み取り結果は、本書の「失敗影響範囲」に従って分類する。非ブロッキング読み取りでデータがない場合と `WouldBlock` は `NoData`、`EINTR` は `Interrupted` とし、状態を変えずに再試行する。明示的な停止または所有中の入力に対するEOFは `Closed` とする。`InfrastructureCorrupt` はFMQの記述子、制御情報、トランザクションの不変条件違反に限定し、影響を受ける経路を隔離する。不正な188バイトTSパケットは、そのパケットだけを破棄して型付き診断を残し、基盤破損として扱わない。TEI付きパケットはTS生データ出力と記録出力には保持し、意味解析には使用しない。連続性の不連続ではTS生データと記録データを保持し、境界前の意味解析結果を境界後へ連結しない。steady-state continuityは`PacketPipeline`、parser / assembler stateは各per-filter parser owner、Filter parser fenceは`FilterProducerDrainGate.parser_state_generation`を正本とする。stream/source boundaryでは`StreamBoundaryTxn`からtyped reset / invalidate dispatchだけを受ける。SectionまたはPESの解析失敗では対象の意味単位を破棄し、正しい境界から再開する。所有中の入出力に恒久障害が生じても、遮断されていない全体状態の変更を示す型付き証跡がない限り、影響を受けるランタイムだけを終了する。破損または致命的失敗を無言で `NoData` に変換してはならない。


- px4 close は control FD だけでなく TS reader FD と reader state も解放する。
- px4 の CNR 取得は optional telemetry であり、`PTX_GET_CNR` 失敗だけで ロック/状態 query を fatal error にしない。
- セクションフィルター は condition の必要 byte 幅が payload 長を超える場合に match しない。prefix だけ一致した短い payload を match としない。
- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内のone-shot配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に自動配送を停止する。
- `TableInfo`の公開照合条件は、TS filter settingsのPID、table id、versionである。明示versionではそのversionだけを照合し、`version=-1`では最初のtarget選択時にversionを照合条件から外す。callerが指定していないtable種別一覧、送出周期、`ProductProfile`の私的一覧で受理対象を狭めない。
- Android 14 `SectionSettings`の`repeat=false`が明記するのは、`TableInfo`でtable IDとversionに基づくall sectionsを配送した後に停止することまでである。同一PID上で公開条件に一致する複数の`table_id_extension`、actual version、`current_next_indicator`のどれをone-shot対象にするか、および候補全体の有限終端はAOSP公開契約では規定されない。ISO/IEC 13818-1／ARIBの拡張section構文では、`section_number=0..last_section_number`の完結性は1個のtable instance内で成立し、同じtable IDでも`table_id_extension`、actual version、`current_next_indicator`が異なれば別のsection番号空間を持つ。本設計では`TableInstanceKey={input_origin_generation, filter_generation, PID, table_id, table_id_extension, actual_version, current_next_indicator}`を内部同一性とし、別instanceの同じsection番号を一つのtableへ混成しない。
- AOSP公開面は`table_id_extension`または全subtable集合の列挙・終端通知を持たない。`TableInfo repeat=false`のfirst-instance解決はAOSP未規定の複数候補に対する製品内規則として、次項のtarget選択で定義する。
- 本製品は、AOSP未規定の複数候補解決として、公開条件に一致して入力順で最初に受理した構造上完全なsectionが属する1個の`TableInstanceKey`をone-shot targetに選ぶ。first-instanceはAOSPの明文要求ではなく、有限なsnapshotを決定的に選択する製品内規則である。これはcaller-visible filter条件を追加するものでも、AOSPがall sectionsを1個のinstanceと定義したと主張するものでもない。`version=-1`はtarget選択時のwildcardであり、全actual versionを1回で配送する指定ではない。target確定後のactual version固定は設定値の書換えではなくtable instance identityである。全serviceのEIT等、複数instanceを包括的・継続的に取得するcallerは`repeat=true`を使用し、SI engineがinstance別の完成を管理した後に明示的に`stop()`する。Tuner HALは未知の全instance集合の一巡または終端を推測しない。
- 対象instanceの構造上完全なsectionは、最初の出現順に各section番号を正確に1回だけ逐次配送する。FMQ書込みまたはevent登録が確定した後にだけ対応bitを配送済みbitmapへ立て、重複sectionは再配送しない。`0..last_section_number`の全bitが確定した時点で自動配送を停止する。全payloadをtable完成まで保持せず、section番号順への並べ替えも行わない。短形式でversion、extension、section番号を持たないtableは、公開条件に一致した最初の完全sectionを1 sectionのtableとして配送して停止する。
- target確定後は、別extension、別actual version、別current/nextのsectionを対象へ混成または配送しない。`version=-1`でtarget完成前に別versionが到着してもtargetを先着instanceから切り替えず、明示versionでは他versionを無視する。target内で`last_section_number`が矛盾するsectionはmalformedとして破棄し、誤完了させない。`repeat=true`では公開条件に一致する全instanceを継続配送する。
- `TableInfo repeat=false`の完了に時間窓、再送一巡、最初に完成したcandidate、非公開table一覧を使用しない。不完全なtargetでは有限時間で停止することを推測しない。`stop()`はdeliveryだけを停止し、target metadata・配送済みbitmap・partial sectionを保持して再`start()`で継続する。`flush()`、再設定、stream boundaryでは境界前target / sectionを境界後のtarget / sectionへ連結・配送しない。stream boundaryでは`StreamBoundaryTxn`が各steady-state ownerへのtyped reset / invalidateをprepare / dispatchする。target metadata / 配送済みbitmapは対応するper-filter TableInfo tracker owner、parser / assembler stateは各per-filter parser owner、Filter parser fence generationは`FilterProducerDrainGate.parser_state_generation`がそれぞれmutation ownerであり、`StreamBoundaryTxn`はこれらのstate自体を所有・直接mutationしない。
- SECTION能力閉包がone-shot用に確保する追加状態は、1 filter当たり1個の`TableInstanceKey`、`last_section_number`等の固定metadata、および256-bit（32 byte）の配送済みbitmapだけとする。FMQ overflow/backpressureでcommitできない新規sectionはAOSP `DemuxFilterStatus::OVERFLOW`のdrop-new意味論に従って破棄し、commit前にbitmapを更新しない。最大256 section分のpayloadを別領域へ常時予約せず、通常のsection組立て・FMQ予算とone-shot追跡状態を二重計上しない。後続放送入力として同一sectionが再到来した場合は、bitmap未確定のため通常のmatching対象として受理できる。
- `TableInfo.version`は`-1`または`0..31`だけを受け付ける。`-1`はtarget選択時にversionを無視する指定であり、caller-visibleな設定をruntime観測値へ書き換えない。範囲外は`INVALID_ARGUMENT`とする。
- PES `streamId`は`0..=255`を明示`stream_id`として照合し、AOSP `Constant.INVALID_STREAM_ID`の`0xFFFF`をwildcardとして扱う。負値、`256..=65534`、`65536`以上は`INVALID_ARGUMENT`とする。PES能力を広告するdemuxは、全ての有効な明示stream IDとwildcardを通常のPES filter設定として受理し、`0xBD`その他の私的部分集合へ制限しない。ARIB字幕を利用するTIS profileは`0xBD`を指定してよいが、それは利用側の選択でありHAL capabilityの制限ではない。`PES_packet_length=0`はH.222.0で許可される映像stream ID `0xE0..0xEF`のruntime組立てとして扱い、その他のstream IDで受信した長さ0 PESはmalformedとして当該意味単位を破棄する。

#### raw section / raw PES event 生成契約

Section/PES処理は外形抽出、設定されたCRC検査、typed event生成に必要な構文検証を独立段階として扱う。外形不完全、宣言長不成立、設定上限超過、境界不明は配送しない。

| filter設定 | FMQ payload | FMQ commit後の必須callback | `onFilterEvent()` |
|---|---|---|---|
| Section `raw=true` | 完全なsection bytes | `IFilterCallback.onFilterStatus(DATA_READY)` | `DemuxFilterSectionEvent`を生成しない |
| PES `raw=true` | 完全なPES bytes | `IFilterCallback.onFilterStatus(DATA_READY)` | `DemuxFilterPesEvent`を生成しない |
| Section `raw=false` | event契約に対応するsection data | event配送規則に従う | `DemuxFilterSectionEvent` |
| PES `raw=false` | event契約に対応するPES data | event配送規則に従う | `DemuxFilterPesEvent` |

raw=trueではFMQ payloadをcommitした後に`DATA_READY` status callbackを配送する。EventFlagはFMQ consumerを追加で起床させる同期手段であり、`onFilterStatus(DATA_READY)`の代替ではない。raw/nonraw切替前のcallback/eventを切替後として配送しないfenceは`FilterProducerDrainGate.filter_delivery_generation`を使う。

nonraw Section eventの`tableId`は実sectionのtable_id、long sectionでは実際の5-bit `version`と`section_number`、short sectionでは`version=0` / `sectionNum=0`とする。`dataLength`は対応する完全section byte数と一致させる。nonraw PES eventの`streamId`はparseした実stream_id 0..255、`dataLength`は対応する完全PES byte数、TS-only productの`mpuSequenceNumber`は0とする。推測値でmetadataを生成しない。

### PES assembler の異常系状態表

次表は一般PES filterが満たす構文・再同期条件を表す。設定は有効な明示stream IDまたはwildcardを受理し、受信したstream IDごとに宣言長ありPESと映像の長さ0 PESを区別する。

| 入力状態 | 判定 | assembler 動作 | 配送 |
|---|---|---|---|
| PUSI あり、PES start code 正常 | 新規 PES 開始 | 既存未完了 PES を破棄し、新規 PES を開始 | まだ配送しない |
| PUSI なし、既存 PES あり | continuation | buffer へ追加 | 完成条件を満たせば配送 |
| PUSI なし、既存 PES なし | continuation-only | state 破棄 | 配送しない |
| PES start code 不正 | malformed | state 破棄 | 配送しない |
| `stream_id`が`0xBC,0xBE,0xBF,0xF0,0xF1,0xF2,0xF8,0xFF` | ordinary optional headerを持たないspecial syntax | start code、stream id、宣言長と当該special payload境界だけを検証し、optional-header marker、`PTS_DTS_flags`、`header_data_length`、PTS/DTSを要求しない | 完全長とspecial syntax検証成功時だけ配送 |
| 上記以外の`stream_id`でoptional header marker不正 | malformed ordinary PES | state 破棄 | 配送しない |
| ordinary PESで`PTS_DTS_flags == 0b00` | timestampなしの有効PES | timestamp fieldを要求せず収集を継続 | 完全長で配送 |
| ordinary PESで`PTS_DTS_flags == 0b01` | malformed | state 破棄 | 配送しない |
| ordinary PESで`PTS_DTS_flags == 0b10`かつPTS marker正常 | PTSあり | PTSを内部検証して収集を継続 | 完全長で配送 |
| ordinary PESで`PTS_DTS_flags == 0b11`かつPTS/DTS marker正常 | PTS/DTSあり | PTS/DTSを内部検証して収集を継続 | 完全長で配送 |
| ordinary PESのPTS / DTS marker bit不正 | malformed | state 破棄 | 配送しない |
| ordinary PESで`PES_packet_length`とheader長が矛盾 | malformed | state 破棄 | 配送しない |
| 映像以外の`stream_id`で`PES_packet_length == 0` | malformed | state 破棄 | 配送しない |
| 有効`stream_id`かつ`PES_packet_length > 0` | supported bounded PES | stream id別の構文分岐後、宣言長+6 byteを共通台帳からclaimし、1 filter 1 assemblerで収集 | 対応する完全長・構文検証成功時だけ配送 |
| `stream_id=0xE0..0xEF`かつ`PES_packet_length == 0` | supported zero-length video PES | 次PUSIまで収集し、`MAX_PES_BUFFER_BYTES`超過時はoversize破棄 | 完成境界とordinary PES検証成功時だけ配送 |
| `stop()` | delivery pause | 未完了PESと対応claimを保持し、再`start()`で同じPES組立てを継続する |
| `flush()` / close / source unlink | boundary | `StreamBoundaryTxn`参照 | 境界前の未完了PESを境界後のPESとして配送せず、partial stateと対応claimを破棄・返却する |


PESの組み立て状態はPIDごとに分離する。`PES_packet_length > 0` の場合は、宣言されたPESバイト数を正確に収集した時点だけを完了とする。同じPIDで宣言長に達する前にPUSIを受信した場合は破損とし、未完のPESを破棄する。`PES_packet_length == 0` の場合は、同じPIDの後続TSペイロードで `payload_unit_start_indicator=1` かつ、ペイロード先頭に構造上有効な `0x000001` のPES開始コードと最低限有効なPESヘッダーがある場合に限り、その直前で現在のPESを完了する。境界となるパケットは次のPESの先頭とし、前のPESへ追加しない。同じPIDのPUSIと有効なPESヘッダーを伴わないエレメンタリーストリーム内の `0x000001` 開始コードによって、現在のPESを終了してはならない。同じPIDのPUSIがあってもPES開始部またはヘッダーが構造上不正な場合は、伝送破損として未完PESを破棄し、型付き診断を記録して、完了PESとして通知しない。別PIDのPUSIは影響させない。`stop()`では未完PESと対応claimを保持して再`start()`で継続する。TEI、連続性の不連続、`flush()`、`close()`では、境界前の未完PESを境界後の完了PESへ連結・配送せず、対応する型付き診断を記録してpartial stateと対応claimを破棄・返却する。steady-state parser / assembler stateは対応するper-filter parser ownerを正本とし、境界時のreset / invalidate要求と`stream_boundary_generation`だけを`StreamBoundaryTxn`のtyped boundaryとして扱う。


### ワーカー失敗と所有権境界

generic worker lifecycleは0-S-3Bの`WorkerRuntime`、worker failure classificationは`WorkerFailureClassifier`を唯一の正本とする。本節ではstop / wake / join、generation fence、Reaper handoff、retry schedule、lease return、`ServiceCritical`判定のmechanismを再定義しない。

ワーカーはデータ処理と通知を担当し、demux、filter、DVR、descrambler等の資源寿命または登録relationのmutation ownerにはならない。worker failureはtyped resultとしてdomain ownerへ返し、domain ownerが各API状態表に従って公開状態へ反映する。worker自身が対象objectを直接unregisterしたり、別ownerのresourceを解放したりしてはならない。

Frontend / Filter / DVR / Playback固有のterminal meaningとworker failure後の公開結果 / data-path効果は各APIの名前付き契約と「失敗影響範囲」を正とする。generic lifecycleは`WorkerRuntime`、failure categoryは`WorkerFailureClassifier`、post-commit callback failureは`PostCommitCallbackFailureTxn`を唯一の正本とし、本節では第二のfailure contractを持たない。

### close / unregister / quarantine 条件

公開`close()`、owner loss、Dropのcleanup実行authority、`begin_close`、`CleanupPending`、retry / handoff / reaper、`Quarantined`判定は0-S-3Bの`ObjectCloseTxn`だけを正本とする。本節はFilter/DVR固有のcleanup対象と依存順序、caller-visibleな結果だけを定義し、独立したclose state machineを持たない。

Filterのtyped cleanup commandは、Filter producer / worker依存の停止・drain要求、未配送queue / 保留eventのcleanupと配送済みAV割り当ての`ReleaseOnly`台帳への移管、source/downstream relationとRecord DVR relationの解除、`demux.unregister_filter(filter_id, generation)`、runtime object登録解除、Filter ledgerの最終releaseを依存順に`ObjectCloseTxn`へ渡す。worker lifecycle自体は`WorkerRuntime`、queue drainは`FilterProducerDrainGate` / `QueueCleanupUseCase`、relation mutationは各0-S-3B ownerを正本とする。

DVRのtyped cleanup commandは、DVR worker / queue依存の停止・cleanup要求、接続済みfilter relation解除、queue cleanup、`demux.unregister_dvr(dvr_id, generation)`、runtime object登録解除、DVR ledgerの最終releaseを依存順に`ObjectCloseTxn`へ渡す。worker lifecycle、queue epoch / cleanup、relation mutationの内部semanticsは各0-S-3B ownerを正本とする。

`demux.unregister_filter()` / `demux.unregister_dvr()` のmissingを成功相当に扱えるのは、同一object / generationについてruntime failure経路で事前unregister済みというtyped記録が存在する場合だけとする。それ以外のmissingは公開close成功へ丸めない。

cleanup commandの途中失敗後の後続command実行、未完step記録、同generationのrecovery `close()`、自律retry、authority移管、reaper進行、quarantine、公開操作拒否、ID / generation再利用可否は0-S-3Bの`ObjectCloseTxn`を唯一の正本とし、本節では再定義しない。

### `IFrontend.stopTune()` の失敗時状態

`IFrontend.stopTune()`はactive tuneを停止し、当該frontendに接続されたdemuxについて旧tune由来data / callbackを後続の成功結果として公開しない。backend停止結果を確定できない場合は`FailedBackend`、backend停止済みだが必要なdemux boundaryを確定できず旧generation fenceが成立する場合は`FailedBoundary`、stale callback / queue / backend resultを遮断できない場合だけ`Quarantined`とする。

対象demux一覧の固定、frontend operation generation fence、worker / backend停止、per-demux `StreamBoundaryTxn`、結果集約、retry / quarantineの内部phase・failure semanticsは「公開transactionのphase・確定点・失敗処理契約」のcanonical `frontend tune/scan`と0-S-3Bの`StreamBoundaryTxn`だけを正本とする。本節では別の複数demux batch state machineを定義しない。active scanだけが存在する場合の`stopTune()`はscan generation、backend scan、attached demux boundaryを変更せず成功する。

新しい配送の停止と、クライアントが保持する記憶領域の存続期間は分けて管理する。


### AV 共有メモリの原子性不変条件

AV shared backingはMediaEvent用allocationのlifetimeを論理的に一括管理し、allocation/release操作で部分更新を公開しない。物理lock、内部struct、helper名はこの公開・論理契約の規範対象ではなく、product default実装に特定の型名・helper名を要求しない。

### TS continuity / adaptation-only packet 固定

- adaptation-only packet は MPEG-TS continuity counter の組立進行条件に含めない。payloadなし packet は continuity tracker の次期待値を進めず、section/PES assembler へ入力しない。
- adaptation-only packet に `discontinuity_indicator` が立つ場合は、境界前のsection / PESを境界後へ連結しない。steady-state continuityは`PacketPipeline`、parser / assembler stateは各per-filter parser ownerを正本とし、境界時は`StreamBoundaryTxn`のtyped reset / invalidate dispatchを受ける。


## Filter event startId / FilterDelayHint 契約

### `DemuxFilterEvent.startId`

一度start済みのFilterに対するstop後の有効な`configure()`成功は、settingsが直前と同一か否かにかかわらず、次回start用のpending `startId`をprepareする。`startId`は0以外の再利用しないpositive idをFilterごとの発行状態からcommit前に予約し、発行できない場合は設定変更をcommitせず`UNKNOWN_ERROR`として従前の設定とpending状態を維持する。次回start前に複数回の`configure()`が成功した場合は、最後に成功したreconfigureに対応するpending `startId`だけを次回startへ引き継ぎ、置換された未配送IDを後から配送または再利用しない。

pending `startId`は対応する`filter_delivery_generation`と関連付けてstale配送を防ぐが、AIDL-visibleな`startId`の識別子空間そのものを内部`filter_delivery_generation`と同一視しない。Filterを再startした後、最初のevent callbackは`startId`だけを含むcallbackとして正確に1回配送し、その後に通常eventを配送する。startId-only callbackに別eventを同梱しない。新規open Filterの最初のstartだけはAOSP予約値0を使用してよく、それ以外は0を使用しない。stale `filter_delivery_generation`に関連付いたpending `startId`は配送しない。`startId`専用のID発行状態は既存Filter delivery ownerが所有し、stream / queue / parser用の新しいgeneration軸として扱わない。

### `FilterDelayHint`

`FilterDelayHint`は`TIME_DELAY_IN_MS`と`DATA_SIZE_DELAY_IN_BYTES`だけを受理する。unknown type、負の`hintValue`、表現不能値は`INVALID_ARGUMENT`で状態不変とする。typeごとに独立したhint値を保持し、0はそのtypeのhintだけをreset、positive値は対応typeの閾値として保持する。media filterは本製品capability契約どおり`UNAVAILABLE`とする。

non-mediaのevent-producing filterではpending `onFilterEvent()` batchを既存Filter delivery ownerが保持し、有効な時間閾値または有効な累積data-size閾値の**いずれか**に達した時点でbatchを配送可能とする。data-sizeはpending batch内eventのdata length合計で評価する。hintはeventを失わせる権限ではなく、stop / flush / close / reconfigure時のpending aggregateは`filter_delivery_generation`でfenceし、旧generationのbatchを新generationとして配送しない。別のscheduler state machineは導入しない。

## フィルタ状態破棄境界と遅延通知方針

filter の `stop()`、`flush()`、`configure()`、上流フィルタ登録解除の状態別契約は、本節、0-S-3Bの`FilterProducerDrainGate` / `QueueCleanupUseCase` / `StreamBoundaryTxn`、および各Filter公開APIの名前付き契約を正とする。本節では、遅延通知の再arm条件を補足する。

`FilterDelayHint::時間遅延指定` は queue-empty → non-empty の各まとまりごとに再armする。start/configure直後の1回限りdelayではない。payload queue が空の filter に新規 payload が入った時点で期限を再設定し、最初のまとまり delivery 後に queue が空になった場合、次まとまりは再び time delay を受ける。time delay と data-size delay が両方有効な場合は OR 条件であり、どちらか一方を満たした時点でコールバック配送可能とする。

## CAS と descrambler の境界

CAS HAL / TIS / Tuner HAL のリリース段階ごとのスクランブル解除スコープは `開発規則.md` を正とする。本節では、CAS 本体未接続時でも Tuner HAL の `IDescrambler` AIDL面、key token検証、PID登録、packet単位デスクランブル中核、診断境界をどう扱うかだけを固定する。


## descramble 失敗時 packet policy

対象 PID の descramble に失敗した場合でも、DVR / raw TS 録画経路 では scrambled TS packet を後段へ pass-through してよい。これは録画済み TS を後からデスクランブルできるようにするための意図的な設計である。

ただし pass-through は 平文 成功ではない。packet経路 は少なくとも次を区別する。

- 平文 packet
- descrambled packet
- scrambled pass-through packet
- descramble 失敗 packet

Live/AV経路、診断、recording メタデータ、VTS 判定では、scrambled pass-through を `notifyVideoAvailable()` や 平文 success と混同しない。診断カウンター は `NO_KEY`、`BAD_TOKEN`、`CAS_BRIDGE_UNCONNECTED`、`INVALID_TSC`、`MULTI2_FAIL`、`SCRAMBLED_PASSTHROUGH`、`SCRAMBLED_WITHOUT_DESCRAMBLER` を分離し、debug dump 文字列で demux/PID ごとに観測できるようにする。

## px4_drv ロック 方針

px4_drv backendはRF/carrier lockを直接返すuserspace APIを持たないため、`RF_LOCK`をadvertiseしない。一方、product採用driverは `kazuki0824/px4_drv` `feat/android-ddk` commit `c2236cc761d9d02e38a728f8cad0519610cd891b`（`px4_drv#3` merge。`PTX_GET_LOCK_STATUS` と `PTX_GET_TMCC_TSID_LIST` を含む）に固定し、同commitのUAPIはread-only `PTX_GET_LOCK_STATUS = _IOR(0x8d, 0x0c, __u32)`を持つ。driverはcurrent system未設定時を`-EAGAIN`とし、それ以外は既存`ops->check_lock()`で現在のdemodulator lockを観測し、I/O/device failureを正常unlocked値へ丸めずerrnoで返す。

HALは同ABIを`device/src/px4/abi.rs::PTX_GET_LOCK_STATUS`として固定し、active backend sessionの`observe_signal_state()`から副作用なくcurrent lockを取得する。したがってpx4の`FrontendInfo.statusCaps`には`DEMOD_LOCK`をadvertiseし、`getStatus(DEMOD_LOCK)`およびtune/scan workerのlock/lost-lock/relock判定は同一generationのfresh readbackから導出する。`PTX_GET_CNR`、TS packet到達、過去の`PTX_SET_CHANNEL`成功履歴だけをcurrent `DEMOD_LOCK`の代替にしてはならない。readback I/O failure、`EAGAIN`、古いgenerationは正常な`false`へ捏造せず既存backend failure/pending契約へ接続する。

`PTX_SET_CHANNEL`が選局時に内部`check_lock()`を使用する既存動作は維持するが、その一回の成功履歴は後続current statusの代替にしない。transport health（188-byte境界、sync、TEI、continuity、無受信時間等）も`DEMOD_LOCK`/`RF_LOCK`の真値へ写像しない。採用driver commitを変更する場合は、新commitに同等のread-only lock ABIとfailure分離が存在することをproduct integration証跡として更新するまでpx4 `DEMOD_LOCK` capabilityを維持してはならない。

## px4_drv ISDB-S TMCC TSID list 方針

product採用px4 driverは、ISDB-Sのcurrent TMCCからtransponder内TSID集合を取得するread-only UAPIとして `PTX_GET_TMCC_TSID_LIST = _IOR(0x8d, 0x0e, struct ptx_tmcc_tsid_list)` を持つ。UAPI payloadはpointer-free / fixed-sizeの `__u32 num; __u16 tsid[12];` とし、`num <= 12`、返却prefixの各TSIDは非0でなければならない。driverは既存 `tc90522_tmcc_get_tsid_s()` を唯一のTMCC authorityとして0..11のslotを読み、確定した非0 TSIDをTMCC slot順でcompactに返す。userspace側に固定BS TSID表または別TMCC parserを正本として持たない。

HALのpx4 device-adaptation層は同ABIを `device/src/px4/abi.rs::PTX_GET_TMCC_TSID_LIST` として固定し、active `FrontendBackendSession` が既に所有するcontrol fdだけからreadbackする。同一exclusive chardevをTSID取得のために再openしてはならない。`num > 12`、compact prefix内の0、ABI shape不整合はfail-closedのbackend I/O/invariant failureとし、値をtruncation・補完・推測しない。driverの `EAGAIN` は「current TMCC list未確定」のtyped pending observation、その他のerrnoはbackend failureとして保持する。非ISDB-Sの `EOPNOTSUPP` を空listへ変換しない。

このdevice observation自体は公開AIDL capabilityの別名ではない。`FrontendInfo.statusCaps`、`getStatus()`、`getFrontendStatusReadiness()`、scan callbackへ投影する場合は、current frontend generationの正本所有者へcommitした値だけを使用し、公開契約は本書のAOSP frontend status / scan契約に従う。VTS/profile tooling、TIS、host resolverがpx4 ioctlを直接呼ぶ経路は設けない。

### px4 ISDB-S `STREAM_ID_LIST` / `INPUT_STREAM_IDS` 公開契約

px4 ISDB-S frontendは、上記TMCC TSID readbackをproductionで利用できる構成に限り `FrontendInfo.statusCaps`へ `FrontendStatusType::STREAM_ID_LIST` をadvertiseする。px4 ISDB-T、Linux DVB、または同readbackを持たないbackendへこのcapabilityを横展開しない。公開listはcurrent frontend generationでdemod lock成立後にdriver TMCCから取得し `FrontendRuntime`へcommitした同一の非0 16-bit TSID列だけを正とし、固定表、前generation、別frontend、tune request中のselectorをlistの代用品にしない。

`getFrontendStatusReadiness(STREAM_ID_LIST)` は、未広告frontendでは `UNSUPPORTED`、current tune/scan generationが進行中でlist未確定なら `UNSTABLE`、current generationがlockedかつlist commit済みの場合だけ `STABLE` とする。操作外、lock loss後、stop/close/failure後は `UNAVAILABLE` とする。`getStatus(STREAM_ID_LIST)` はcommit済みlistがある場合だけそのlistを返し、advertise済みだがcurrent list未確定の場合に空配列を観測済み値として捏造せず、既存getStatus契約どおり要求全体を `UNAVAILABLE` とする。

listはgeneration変更、lock loss、scan candidate遷移、`stopTune()` / `stopScan()`、close、backend/fatal failureで失効させる。失効後の旧listを新generationのreadinessまたはstatusへ再利用しない。

ISDB-S scanでcurrent locked candidateのTMCC listをauthoritativeに取得・commitできた場合は、同一listを `FrontendScanMessageType::INPUT_STREAM_IDS` / 対応union tagとして、そのcandidateの `LOCKED(isLocked=true)` より先に配送する。TMCC readbackが `EAGAIN` pendingの間は固定値・空listを生成せず、`INPUT_STREAM_IDS` のためだけに `LOCKED` を遅延・失敗させない。pendingのままcandidate lockが成立した場合は追加messageを省略して既存の最低保証 `LOCKED` 契約を維持し、追加message専用の第二scan state machineを設けない。

tune中はlock後の既存worker監視周期でpendingを再観測してよいが、pendingだけをtune failureへ昇格させない。`EAGAIN`以外のdriver/I/O failureは空listや正常pendingへ丸めず、既存backend failure契約へ接続する。VTS/profile toolingはこの値を得るためにpx4 ioctlを直接呼ばずpublic AIDLを試験する。

## px4_drv chardev open / ライブ TS reader 方針

px4_drv の legacy chardev は同一 device node の二重 open を許さないため、px4 backend は control 用 fd と ライブ TS reader 用 fd を別々に `open()` してはならない。`/dev/px4video*` family は `PTX_SET_SYSTEM_MODE`、`PTX_SET_CHANNEL`、`PTX_START_STREAMING`、TS read を同一 open instance から扱う前提にする。

px4 backendは同一device nodeを二重openせず、1回のbackend openからcontrol経路とlive TS readerを派生させる。二重open回避のproduct default実装規約は`../tuner_hal2/CODE_CONVENTION.md`の「px4 single-open backend 実装規約」を正とし、旧参照実装の具体API選択を現行実装の根拠にしない。single-open制約下でもtune後にlive TS、section、AV、record/DVR経路へpacketを流せることを公開設計上の不変条件とする。


フロントエンドの存在と対応能力は、機器、versioned backend manifest、functional probe、有限の選局終端を実装できることから導出する。選局は非同期操作とし、バックエンドが選局要求を受理した後は、`LOCKED`、backendの明示失敗、明示的停止、再選局、閉鎖、またはbackend別`ProductProfile.tuneTerminalDeadlineMs`到達時の`NO_SIGNAL`のいずれかで現generationを必ず終端する。現行profileはearth_pt1を`4000 ms`、px4を`7000 ms`とする。px4値はRT710設定、PLL確認、demod lock、absolute TSID一致、およびrelative selectorのTMCC解決からなる正常な有限経路を期限前に打ち切らないための上限である。期限到達はbinder呼出しの成功を後から失敗へ反転させず、非同期終端eventとして扱う。VTS既知信号経路はVTS自身の待機内でLOCKEDへ到達できる入力を別途要求し、製品deadlineをVTS待機値へ短縮しない。正の有限期限と取消可能なbackend I/Oを実装できないfrontendは公開しない。停止した`ioctl`、read、USB control transferから復帰する内部期限は別の`workerIoDeadlineMs`で管理し、px4の`ctrl_timeout=0`を禁止する。個別I/O期限は検証済みcontrol transfer上限より短くせず、正常処理列の合計がbackendのterminal deadline内に収まるよう固定する。


## TS input origin namespace 固定契約

TS packetのcontinuity / parser / assembler状態を異なる入力経路間で共有しないため、論理入力originの正本namespaceを次の3種類に固定する。

| origin | identity | generation / boundary owner | 禁止事項 |
|---|---|---|---|
| `TsInputOrigin::Frontend` | demuxに現在接続されているfrontend入力 | frontend tune/scan/stop/closeに伴うstream boundaryは`StreamBoundaryTxn` | Playback DVRまたはSourceFilter由来stateとの混成 |
| `TsInputOrigin::PlaybackDvr(dvr_id, queue_identity, queue_epoch)` | Playback DVR object + 再利用しないqueue incarnation + 同一queue内flush epoch | `queue_identity`は`PlaybackQueueBacking`、`queue_epoch`は`QueueEpochProtocol`、境界dispatchは`StreamBoundaryTxn` | fd番号・descriptor内容・別`dvr_generation`をorigin identityに使用 |
| `TsInputOrigin::SourceFilter(filter_id, generation)` | source raw-TS Filter + 当該Filterのcurrent delivery generation | generation mutationは`FilterProducerDrainGate`、relationは`SourceBoundaryTxn`、境界dispatchは`StreamBoundaryTxn` | 未接続downstreamのcontinuity/parser stateを進めること |

`PacketPipeline`は受理したpacketを必ず1個の `TsInputOrigin` とともに処理し、steady-state continuityをorigin別に分離する。per-filter parser / assembler ownerもoriginを含むkeyでpartial stateを分離する。`StreamBoundaryTxn`はboundaryのprepare / dispatchを所有するが、steady-state continuityやper-filter parser stateそのものを第二のownerとして保持しない。origin変更前のpartial stateを新originへ連結してはならない。

## Filter / DVR queue full・overflow 固定契約

通常Filter FMQおよびRecord DVR FMQでは、既にcommit済みでclient未消費のdataを新規dataのために暗黙evictionしない。payload / packet全体をcommitできる空きがない場合は部分writeせず、既存queue内容とread/write positionを維持したまま当該新規単位をdrop-newとして扱う。

| path | full / insufficient space時 | caller-visible通知 | state / counter |
|---|---|---|---|
| TS raw Filter FMQ | 新規TS packetを部分commitせず破棄 | `DemuxFilterStatus::OVERFLOW`をpendingにしcallback delivery対象とする | drop packet/byte診断を飽和加算。既存未消費FMQは維持 |
| section / PES Filter FMQ | 新規の完全section / PESを論理単位で破棄し、先頭だけを書かない | `DemuxFilterStatus::OVERFLOW` | drop診断を更新し、未commit単位を配送済み・one-shot完了扱いにしない |
| Record DVR FMQ | 新規188-byte TS packetを部分commitせず破棄 | 当該write attemptのsemantic outcomeを `RecordStatus::OVERFLOW` とする。`statusMask`で`OVERFLOW` bitが選択されている場合だけcallbackし、maskされている場合に`HIGH_WATER`その他へ代替通知しない | `record_output_byte_offset`を進めず、既存未消費record dataとqueue位置を維持 |
| Playback DVR input FMQ | HAL consumerはclient producerの未読dataをevictしない | client producer側のFMQ空き領域によるbackpressure | HALはdrop-old/drop-new queueとして扱わない |

Filter runtimeとRecord DVR runtimeは、drop原因を通常のmalformed/CRC rejectionと区別できるpending overflow診断を保持する。Record DVRでは、FMQのavailable space不足により188-byte packetの原子的commitを拒否したwrite attemptについて `OVERFLOW` をwatermarkより優先し、そのwrite attemptを契機に `HIGH_WATER` / `LOW_WATER` / `DATA_READY` を代替生成しない。失敗したwriteではqueue occupancyが変わらないため、その後の通常の初期評価・status変化・周期評価は不変のqueue snapshotに対して前段のwatermark分類を独立に適用してよいが、`OVERFLOW`がmaskされていることを理由に同じ失敗を別statusへ再分類してはならない。FMQ write成功後のEventFlag wake失敗はpayload commit済みであり、同じpayloadを再writeしたりoverflow dropへ書き換えたりしない。AV allocation不足は既存AV allocation契約を正とし、この表のTS/section/PES/Record方針を流用しない。

## 通常Filter status 公開方針

Filter FMQをpayload data planeとして使用するFilter（現行TS-only profileではTS raw / `SECTION` / `PES`）の `IFilterCallback.onFilterStatus()` は、Android 14 AIDL V2がfilter buffer statusとして定義する `DATA_READY`、`LOW_WATER`、`HIGH_WATER`、`OVERFLOW` を生成対象とする。`DATA_READY` は各payload/event契約の成功commit条件、`OVERFLOW` は本書のqueue full / allocation overflow契約を正とし、LOW/HIGH watermarkを理由に既存のcommit / drop契約を変更しない。TS `RECORD`はFilter FMQ resourceを所有していてもpayload data planeには使用しないため、このFilter FMQ status / watermarkの生成対象に含めず、Record statusはRecord DVR FMQの契約だけを正とする。

`LOW_WATER` / `HIGH_WATER` はAIDLが規定する既定25% / 75%水位をそのまま用いる。open時のFilter FMQ byte容量を `capacity`、clientから現在読み出し可能なbyte数を `available` とし、`low = ceil(capacity * 0.25)`、`high = ceil(capacity * 0.75)` とする。同一queue snapshotに対して `available < low` なら `LOW_WATER`、`available > high` なら `HIGH_WATER` とし、`available == low` / `available == high` では新たなLOW/HIGH遷移を生成しない。これはAOSP default Tuner HALの既定watermark計算と比較境界に合わせる。LOW/HIGH判定は`FilterQueueBacking`が持つ既存FMQ occupancy snapshotから導出し、独立したwatermark owner / queue / worker / timerを追加しない。重複callback抑止に必要な直前のderived watermark statusはFilter callback配送の補助状態に限り、payload ownership、lifecycle、generation、queue positionを変更しない。

成功したpayload commitでreadable dataが生じた場合の`DATA_READY`と、そのcommit後snapshotがLOW/HIGH条件を満たした場合のwatermark statusは別の公開事象として扱う。一方、論理単位を原子的にcommitできず本書のoverflow条件に入ったwrite attemptは`OVERFLOW`を正とし、その失敗をLOW/HIGHへ代替写像しない。

`NO_DATA` はAIDL上「filterへdataが来ていない」状態として定義されるが、AIDLはその判定時間・timeout・周期を規定しておらず、AOSP default Tuner HALの通常Filter watermark処理にも独自のinactivity timerはない。本製品も経過時間だけを根拠とするNO_DATA timerを新設せず、`available == 0`だけで`NO_DATA`へ写像しない。現行の通常Filter経路では、AOSP契約または入力domainからNO_DATAを一意に確定できる別の明示事象を定義していないため`NO_DATA`を生成しない。将来のAIDL enum値も、明示契約なしに既存statusへ丸めて生成しない。

このFilter watermark契約はDVRの `PlaybackStatus` / `RecordStatus` と `DvrSettings.lowThreshold/highThreshold` には適用せず、DVR水位契約を変更しない。

## MPEG-TS section transport 固定契約

TS `section_length`は12 bit構文として扱い、table IDごとの外形上限を次で固定する。1021区分は`section_length <= 1021`かつsection全体`<= 1024`、拡張区分は`section_length <= 4093`かつsection全体`<= 4096`とする。表固有の意味解析はHALへ持ち込まず、ここではtransport外形だけを検証する。

| `table_id` / 範囲 | 表 | `section_length`上限 | section全体上限 |
|---|---|---:|---:|
| `0x00` | PAT | 1021 | 1024 |
| `0x01` | CAT | 1021 | 1024 |
| `0x02` | PMT | 1021 | 1024 |
| `0x03` | TSDT | 1021 | 1024 |
| `0x40-0x41` | NIT actual/other | 1021 | 1024 |
| `0x42`, `0x46` | SDT actual/other | 1021 | 1024 |
| `0x4A` | BAT | 1021 | 1024 |
| `0x4E-0x6F` | EIT p/f, schedule | 4093 | 4096 |
| `0x70` | TDT | 5 | 8 |
| `0x71` | RST | 1021 | 1024 |
| `0x72` | ST | 4093 | 4096 |
| `0x73` | TOT | 1021 | 1024 |
| `0x4C`, `0xC2`, `0xC4-0xC7`, `0xD0-0xD2` | INT, PCAT, BIT, NBIT, LDT, LIT, ERT, ITT | 4093 | 4096 |
| `0xFE` | AMT | 4093 | 4096 |
| その他の予約/private/未割当table ID | raw transport | 4093 | 4096 |

PUSI=1のTS payloadでは、payload先頭の`pointer_field`を検証してから `1 + pointer_field` を新section開始位置とする。`pointer_field` bytesは直前の未完了sectionのtailとしてだけ使用できる。pointer bytesで旧sectionが完全になった場合はそのsectionを確定後に新sectionへ進む。pointer bytesで旧sectionが完了しない場合、または`pointer_field == 0`なのに旧partialが残る場合は旧partialを破棄し、新section本文へ連結しない。`1 + pointer_field`が当該TS payload範囲を超える入力はmalformedとして新section開始を作らない。旧partialの破棄はstale-partial診断へ記録し、queue overflowへ写像しない。

## DVR 方針


DVRの同時利用上限は確定済み`CapabilitySnapshot`で定める。`P=snapshot.playback_count`、`R=snapshot.record_count`、demuxごとの上限は各1個とする。用途別全体枠とdemux別枠に空きがあり、要求queueと正確な通知枠をtransactionとして準備できる場合だけ受け付ける。検証順序はlifecycleと引数、用途別容量、demux別上限、失敗し得る準備処理とする。失敗時は`INVALID_ARGUMENT`、`UNAVAILABLE`、`UNKNOWN_ERROR`を原因別に返し、確定状態を変更しない。能力報告、受付、cleanup、最終解放は同じsnapshotを参照する。VTS設定を実行時生成せず、無条件の既定XMLも設けない。起動前環境profileでVTS artifact/tag/commit、variant property、入力元、経路、PID、queue予算を定義し、選択したVTS実装の規則でXML filenameを解決し、その要求全体がsnapshotに収まる場合だけ解決済みpathへ静的XMLをinstallする。それ以外はruntime保証を弱めずVTSを`DESIGN_HOLD`とする。

### `IDvr.setStatusCheckIntervalHint()` 契約

Android 14 AIDL V2の`setStatusCheckIntervalHint(milliseconds)`はPlayback / Recordの双方で有効なDVR objectに対して受理し、成功後のhintを以後のdata/status evaluation cadenceを決める入力として使用する。method成功を「値を保存するだけ」で完了させず、次回以後のstatus評価scheduleは受理済みhintを参照する。

hint変更自体はDVR queue内容、FMQ read/write position、playback/record state、`start()` / `stop()` lifecycle、Filter relationを変更しない。Binder methodはhint時間待機しない。started中の変更を既存workerへ反映する場合は`WorkerRuntime`の既存signal mechanismでwaitを再評価させ、別のworker lifecycle/state machineを導入しない。

Android 14 AIDL本文は`milliseconds=0`に特別なreset意味を規定せず、positive値についても厳密deadlineまたは最大遅延を保証していないため、本設計は0を独自の「product defaultへreset」と読み替えず、受理したhint値としてevaluation cadence決定へ渡す。AOSPが保証する以上の時刻意味を公開契約へ追加しない。


### TS RECORD filter settings / event 固定契約

`DemuxFilterRecordSettings` は `tsIndexMask`、`scIndexType`、tagged `scIndexMask` を1個の設定snapshotとして検証し、全項目が成功した場合だけFilter設定へcommitする。拒否時は旧settings、parser/tracker、`record_output_byte_offset`、source relation、Record DVR relation、generationを変更しない。

- `tsIndexMask=0` はTS header/adaptation indexを要求しない有効値とする。Android 14 `DemuxTsIndex` のうちMPEG-TS packet header / adaptation fieldを直接表す既知bitだけを本製品のTS record index候補とし、要求maskの各bitはその既知bit集合のsubsetでなければならない。MMTP/MPT固有としてAIDL上定義された既知bitはTS-only productで `UNAVAILABLE`、予約bit・未知bitは `INVALID_ARGUMENT` とする。
- `scIndexType=NONE` はstart-code indexなしを意味し、effective `scIndexMask` は0でなければならない。NONE時に非0 maskを要求した場合は `INVALID_ARGUMENT` とし、unionの未使用tagからcodecを推測しない。event側のcanonical zeroは `scIndex` tag + 0とする。
- `SC` / `SC_HEVC` / `SC_AVC` / `SC_VVC` はそれぞれ `scIndex` / `scHevc` / `scAvc` / `scVvc` union tagと一致しなければならず、tag不一致は `INVALID_ARGUMENT` とする。各maskは選択tagでAndroid 14 AIDLが定義するbitのsubsetだけを受理し、予約bit・未知bitは `INVALID_ARGUMENT` とする。
- non-NONEの `scIndexType` は、同じ確定済み `CapabilitySnapshot` に当該index parser / codec能力が存在する場合だけ成功する。AIDL上既知だが能力非採用のtypeは `UNAVAILABLE` とし、値を保存して無視しない。未知の `scIndexType` は `INVALID_ARGUMENT` とする。

`DemuxFilterTsRecordEvent` は設定要求のechoではなく、同じFilter/source/delivery generationで実際に検出し、対応するRecord DVR output commitが成功した事実だけから生成する。

- `pid` はindexを検出した実TS packetの13-bit PIDとする。別PIDまたは設定値の固定echoを使用しない。
- `tsIndexMask` は要求maskと当該位置で実検出したTS index bitの積集合だけを設定し、未検出の要求bitを立てない。
- `scIndexMask` は設定した `scIndexType` に対応するunion tagを使用し、要求maskと実検出indexの積集合だけを設定する。`NONE`では前記canonical zeroを使用する。
- `byteNumber` は既存のRecord Filter output offset契約を正とし、対応output commit成功後だけ公開する。
- `pts` は同じ検出位置へ結び付く構造上有効なPTSをparserが取得できた場合だけその値を設定し、取得できない場合はAndroidの `Constant64Bit.INVALID_PRESENTATION_TIME_STAMP` を返す。別PES / 別generationのPTSを流用しない。
- `firstMbInSlice` は `SC_AVC` で当該sliceから構造上有効な値を取得できた場合だけ設定し、それ以外は `Constant.INVALID_FIRST_MACROBLOCK_IN_SLICE` を返す。別codecへ0を意味値として捏造しない。

flush、reconfigure、source変更、stream boundaryでは旧generationの未配送Record eventとpartial index parser stateをfenceし、新generationのeventとして配送しない。`record_output_byte_offset`自体のreset規則は前段の固定契約を正とし、index parser generationとoffset lifetimeを同一視しない。

Record DVRへ接続中の記録フィルター条件について、caller / data-path上の不変条件は、各188-byte TS packetを単一の確定済みroute snapshotに対して正確に1回評価し、いずれかの記録条件に一致した場合は到着順にRecord DVRへ正確に1回commitすることとする。フィルターごとの索引状態とコールバック状態は独立して保持する。relation / union-routeのprepare、commit / abort、設定変更時のsnapshot更新、relation generation、切替確定点、lockingは0-S-3Bの`RecordDvrFilterRelationTxn`だけを正本とし、本節では再定義しない。各フィルターへ一度分配してから全体を並べ替える、重複排除する、または`ingress_sequence`で欠落を推測する構成にしてはならない。

Record Filterの`record_output_byte_offset`はFilter output lifetimeごとに`FilterProducerDrainGate`が唯一のmutation ownerとして所有し、次にRecord Filter outputへcommitされるbyteのoffsetを0起点で表す。Record DVRへのoutput commit成功時だけ、実際にcommitしたbyte数だけ単調に進め、commitしなかったdata、cancel、失敗したwriteでは変更しない。`DemuxFilterTsRecordEvent.byteNumber`は当該indexが位置するcommitted Record Filter output上のbyte offsetとし、対応するoutput commitが成功した後にだけ当該Record eventを配送可能にする。TS packet先頭を示すindexではcommit前の`record_output_byte_offset`、packet内のindexではそのoutput内位置を加えた値を使用する。`start()` / `stop()`、configure / reconfigure、Filter / DVR `flush()`、source / stream boundary、Record DVRのattach / detachではresetせず、新しいFilter object / output lifetimeだけ0から開始する。`PacketPipeline`、`RecordDvrFilterRelationTxn`、`QueueEpochProtocol`、`StreamBoundaryTxn`はこのoffsetを直接変更しない。

Record DVR FMQへ成功commitした188-byte packetは公開済みとして扱い、後続のrecord filter接続・切断、source変更、source generation変更によって遡及変更しない。relation更新前後の個々のpacketを旧新routeの混在条件で評価せず、Record DVR FMQのclient未消費byte列を明示的に破棄するのは`IDvr.flush()`だけとする。個別source/filter境界の代替として共有queue全体をflushしてはならない。

開始済みの録画フィルターを接続または切断する場合も、各packetの配送はexactly-onceとし、切断後のrouteへ旧relation由来の新規配送を行わない。重複接続と未接続フィルターの切断は状態を変えず成功する。route locking、relation generation、packet間の切替確定処理は`RecordDvrFilterRelationTxn`を正とする。


record DVR / raw TS filter経路 は受信した 188-byte TS packet を製品の録画品質方針として保持する。TEI が立った packet、duplicate continuity counter の packet、scrambled pass-through packet は、録画・診断・後段デスクランブルのために 録画経路 へ到達させる。一方で、section / PES / AV assembly は破損 packet や duplicate packet による二重組み立てを避けるため、TEI packet と duplicate continuity packet を assembly 入力から除外する。これは AOSP が TEI / duplicate の drop/keep policy を明示しているためではなく、日本向け製品の録画品質と parser 安定性を両立するための固定設計である。

payloadを持つ同一PIDで直前と同じcontinuity counterを受信した場合、同じ入力元・世代に保存した直前の188バイトTS packetと全バイトが一致するときだけ再送重複と判定する。この場合はraw TSと録画へ保持し、section/PES/AV assemblerへは二重投入しない。同じcounterで1バイトでも異なるpacketは重複ではなく連続性破損である。raw TSと録画には保持するが、境界前のsection / PES / AV partialを当該packet以後の意味単位へ継続結合しない。steady-state continuityは`PacketPipeline`、parser / assembler / partial stateは各per-filter parser ownerを正本とし、境界時は`StreamBoundaryTxn`のtyped reset / invalidate dispatchを受ける。adaptation-only packetは次期待counterを進めず、`discontinuity_indicator`はpacket一致判定とは別に明示境界として処理する。



playback 専用 stats は少なくとも injected bytes、injected packets、malformed packets、dropped bytes を持つ。malformed TS は drop + 診断 を標準方針とし、1 packet の malformed input で playback stream 全体を fail させない。playback input FMQ の `PlaybackStatus` は start 直後・周期 コールバック ともに playback input FMQ の実 fill / unused write space を唯一の水位 source とし、record/output queue の `queued_bytes` を流用しない。playback consumer ワーカー は domain worker ownerから0-S-3Bの`WorkerRuntime`へ接続する。stop / wake / join / reaper / retry / lease returnのgeneric lifecycleは同契約を唯一の正本とし、本節ではPlayback固有のstatsとdata-path結果だけを定める。

playback input のstream境界について、本節はcaller/domain-visibleなdata-path結果だけを固定する。start前にclientがprefillしたbytesはstart後のplayback TSとして扱い、started=false中はplayback consumerが入力を消費しない。stop成功後は同じplayback streamを次startで継続できる。flush成功後にclientが新たに書いたbytesは次startのprefillとなり、flush前のstreamと連結しない。flushで破棄された入力量はdropped bytes診断カウンターとログへ反映する。playback flushはrecord/output側のstream/queue/statsを破壊せず、record DVR flushもplayback側のstream/queue/statsを破壊しない。playback input FMQのdrain/resetとqueue epochは0-S-3Bの`QueueEpochProtocol`、consume stateとpacket assembler residualは`PlaybackConsumeTxn`を正本とする。stream boundaryでは`StreamBoundaryTxn`がtyped reset / invalidateをprepare / dispatchし、residualの保持・破棄を含む各stateのmutation自体は対応するownerが行う。本節では内部mutation順序を再定義しない。playback/record固有statsのreset結果は各DVR公開契約に従う。


### playback consumer commit（消費確定）表

本節は DVR playback の caller / data-path から観測できる結果と診断だけを固定する。FMQ read admission、`beginRead()` / `commitRead()`、processing buffer、parse / inject cursor、consume result、retain / discard、residual、retry の内部 phase・state・failure semantics は 0-S-3B の `PlaybackConsumeTxn` だけを正本とし、本節では再定義しない。

| caller / data-path 状況 | caller-visible / data-path結果 | 診断 | 内部mutation正本 |
|---|---|---|---|
| valid TSの配送成功 | 対象outputへ正確に1回配送する | 正常 | `PlaybackConsumeTxn` |
| 受付後から配送確定までの間に配送先が消滅 | stream全体を致命失敗にせず、消滅した配送先へ重複配送しない | no-delivery raceを型付きで記録 | `PlaybackConsumeTxn` |
| malformed TS | 当該malformed単位を配送せず、後続stream処理を継続する | malformed diagnostic | `PlaybackConsumeTxn` |
| partial TS | 完全なTS packet / semantic unitとして成立する前に配送済み扱いにしない | 必要に応じpartial入力診断 | `PlaybackConsumeTxn` |

定常的に配送先がない状態を即時fatal stream failureへ昇格しない。FMQの背圧・容量状態は既存のplayback queue公開契約に従い、read admissionや入力保持方法を本節で追加定義しない。

Playback DVR の `configure()` 時は、FMQ descriptor / queue capacity 自体は要求された全容量を保持する一方、1回の `PlaybackConsumeTxn` が同時に保持する processing-buffer は `min(FMQ容量, 188 * 256)` bytes に有界化する。`CapabilitySnapshot.playbackProcessingBudgetBytes` から予約する使用権と実領域の確保量は、この同一 `required_playback_processing_bytes(queue_capacity)` 契約から導出し、FMQ全容量を第二の processing buffer として二重予約しない。予約または確保に失敗した場合は `OUT_OF_MEMORY` を返し、FMQ descriptor と DVR 設定を部分公開しない。この領域は第二の queue ではなく、1 consume transaction のための予約済み storage であり、使用権は DVR の最終 cleanup 完了時に返す。processing-buffer の read / parse / inject / retain / discard semantics は 0-S-3B の `PlaybackConsumeTxn` だけを正本とする。

### playback consumer ワーカー 起動順序

DVR playback consumer ワーカー は、DVR が soft demux と `RuntimeIoRegistry` の両方へ登録され、queue と worker signal の所有権がdomain ownerへ確定した後にだけ開始する。登録前にplayback workerがDVR stateを観測してはならない。worker handle slot準備・spawn・stop / wake / join / reaperは0-S-3Bの`WorkerRuntime`、公開前のregistry / queue / ledger rollbackはroot/child open契約を正とし、本節では失敗時cleanup phaseを再定義しない。

### Frontend scan callback message 固定契約

本製品のscan callbackは、公開する `FrontendScanMessageType` と `FrontendScanMessage` union tag/valueを一体の契約として生成する。message typeだけを正しくして別tagまたは、当該事象を表さない値を入れてはならない。

| scan事象 | message type | union tag / value | 生成条件 |
|---|---|---|---|
| current scan generationでcandidate成立 | `LOCKED` | `isLocked=true` | backend lockと、要求に含まれる追加検証条件が同一generationで成立したcandidateだけ。candidateごとに既存scan契約の回数制約に従う |
| scanの正常終了 | `END` | `isEnd=true` | current generationの終了をcommitした後に正確に1回 |
| current candidate / scan進行についてAIDL定義済みの追加情報をauthoritativeに取得済み | 対応するAIDL定義済みmessage type | 当該message typeに対応するunion tag / 実観測値 | current generationに属するbackend結果、正規化済みrequest、または同generationで確定したparser結果から値を一意に構成できる場合だけ |

`LOCKED` / `END` は上表の条件で最低保証する。`PROGRESS_PERCENT`、`FREQUENCY`、`SYMBOL_RATE`、`HIERARCHY`、`ANALOG_TYPE`、`PLP_IDS`、`GROUP_IDS`、`INPUT_STREAM_IDS`、`DVBS_STANDARD`、`DVBT_STANDARD`、`ATSC3_PLP_INFO` などAndroid 14 AIDLで定義済みのmessage typeは、単に現行の最低保証集合に含まれないことを理由に禁止しない。一方、別generationの値、固定値、別frontend由来値、未取得値、推測値をcallback生成のために捏造してはならず、値をauthoritativeに確定できないmessageは生成しない。追加messageのためだけに公開scan lifecycleと独立した第二のscan state machineを設けず、必要な補助値はcanonical `frontend tune/scan` ownerのcurrent-generation stateへ従属させる。

本製品は現行profileでblind scanを対応宣言しないため、blind scan要求を成功へ拡張しない。将来blind scan能力を公開する場合は、その能力を使用するAndroid 14 VTSが要求する`FREQUENCY`等のcallback closureを同じprofile変更で満たし、`LOCKED` / `END`だけを返す部分対応を広告してはならない。

callback entryは `FrontendTuneScanTxn` が確定したcurrent scan generationをfenceし、旧generationのいかなるscan messageも新generationへ配送しない。同一scan終了について `END` を重複配送せず、type/tag/valueのいずれかが不一致のentryは公開callbackへ渡さず内部不変条件違反として扱う。`LOCKED`では`isLocked=true`、`END`では`isEnd=true`だけを公開し、false variantを別事象の代替表現として生成しない。

## フロントエンドの対応能力と状態


ISDB-Tの列挙値域は、今回精読したARIB公式英訳STD-B31 2.2-E1本文の2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7を証拠本文とする。現行日本語版2.3との差分は「ARIB規範本文との静的照合」の`差分未証明`管理に従う。規格上の有効値と対象ドライバーで設定可能な値は分けて扱う。本製品の対象バックエンドは、モード、変調方式、符号化率、ガードインターバル、時間インターリーブについて `AUTO` だけを対応能力として採用し、公開・受理する。


`RF_LOCK` は backend が RF/carrier acquisition を別途取得できる場合だけ advertise する。DVB / earth_pt1 backend は Linux DVB `FE_READ_STATUS` が返す `FE_HAS_CARRIER` を `RF_LOCK`、`FE_HAS_LOCK` を `DEMOD_LOCK` に対応させる。px4固有の`RF_LOCK` / `DEMOD_LOCK` capability、`PTX_SET_CHANNEL`成功と`LOCKED`通知の関係は後段の「px4_drv ロック 方針」を唯一の正本とし、本節では再定義しない。

`SNR` と `SIGNAL_STRENGTH` は、起動時に安定取得と更新経路をfrontendエントリの固定capabilityとして証明できる場合だけ `statusCaps` に含める。DVB / earth_pt1 の `FE_READ_SNR` と `FE_READ_SIGNAL_STRENGTH`、px4 の `PTX_GET_CNR` は target driver / device 状態によって read 時に失敗し得る optional telemetry であり、その証明がないfrontendではadvertiseしない。これらの optional telemetry は診断内部値として保持してよいが、証明なしにAOSP `statusCaps` 上のsupported状態としてadvertiseしてはならない。

`SIGNAL_QUALITY` は、backend ごとに根拠ある合成値を返せる場合だけ `statusCaps` に含める。DVB / earth_pt1 backend の `SIGNAL_QUALITY` は Linux DVB `FE_READ_STATUS` 状態 bit の ロック 進捗を 0〜100 に正規化した値とする。px4 backend は `PTX_GET_CNR` を安定取得できることを frontendエントリ の capability として固定できない限り、`SNR` と `SIGNAL_QUALITY` を advertise しない。いずれも `DEMOD_LOCK` や `RF_LOCK` の代替ではなく、UI/診断 用の合成指標である。未取得 telemetry を `SIGNAL_QUALITY=0` として成功返却してはならない。


### ISDB-T segment capability 契約

Android 14 AIDL V2の`FrontendIsdbtCapabilities.isSegmentAuto`と`isFullSegment`は、ISDB-T frontendごとの変更不能な`IsdbtSegmentCapability`として`CapabilitySnapshot`へ保持し、`FrontendInfo.frontendCaps`とsettings validationの両方を同じ値から導出する。

- layerの`numOfSegment=0`はAOSP builderの未指定値として扱い、segment数の明示制約を付けず成功させる。`isSegmentAuto`の真偽を`0`の受付条件にしてはならない。
- `isSegmentAuto=true`にできるのは、対象backend/device/profileでsegment構成を明示指定せず自動判定して実際に選局できることを検証済みの場合だけとする。Android framework APIは`numOfSegment`用のnamed AUTO定数を公開していないが、Android 14 CTSは`isSegmentAutoSupported()==true`のISDB-T frontendに対して`numOfSegment=0xFF`を設定して`tune()`成功を要求する。このため`0xFF`はCTS互換のAUTO要求として受理し、`isSegmentAuto=true`のfrontendではbackend/demodulatorのsegment自動判定へ写像する。`isSegmentAuto=false`では`0xFF`を`UNAVAILABLE`とし、独自の明示segment数へ読み替えない。
- `isFullSegment=true`にできるのは、対象backend/device/profileで13-segmentの通常受信が成立することを機器能力として検証済みの場合だけとする。単にlockを取得できたこと、またはARIB上13 segmentが存在することだけから`true`を推測しない。
- callerが指定する明示`numOfSegment=1..13`を成功させるには、その値をlayerごとにbackendへ反映する経路または固定値として検証する経路が必要である。本製品のpx4/earth_pt1はlayerごとの明示segment数を反映または固定値検証する能力を採用しないため、値域内の明示segment数を`UNAVAILABLE`とし、値を捨てて成功しない。
- CTS対象として公開するISDB-T frontendは、`isSegmentAuto` / `isFullSegment` と `numOfSegment` 受付の閉包条件を満たさなければならない。`isSegmentAuto=true`ならCTSが送る`0xFF`を実現できること、`isSegmentAuto=false && isFullSegment=true`なら`13`を実現できること、`isSegmentAuto=false && isFullSegment=false`なら`1`を実現できることを、同じ`CapabilitySnapshot`の生成時に検証する。対応するCTS入力を実現できないcapability pairを公開してはならず、3分岐のいずれも成立しないbackend/device/profileはCTS対象ISDB-T frontendとしてexportしない。segment能力の証跡がない場合にbooleanを単に`false`へ倒すだけでこの閉包条件を回避してはならない。能力boolean、`numOfSegment`の受付、`ProductProfile`、VTS選局入力の間に矛盾がある候補は`CapabilitySnapshot`へcommitしない。


### ISDB-T `layerSettings[]` 配列契約

`FrontendIsdbtSettings.layerSettings` は配列形状を保持したままvalidationし、空配列を **layer個別制約なし** として成功対象に含める。空配列を3個のAUTO要素へ書き換えたり、backendへ存在しないlayer要求を生成したりしない。

AOSP側はこの配列の物理layer数上限を定義しない。Frameworkの `IsdbtFrontendSettings.Builder.setLayerSettings()` はcallerの配列長を保持してcopyし、JNIも同じ長さでAIDL `FrontendIsdbtSettings.layerSettings` vectorをresizeして全要素を順に写像するため、4要素以上という配列長それ自体をAOSP malformed inputとは扱わない。

本製品が通常の13-segment ISDB-Tとして受理する物理階層数の上限はAndroid vectorではなく放送方式側から導出する。現行の「標準テレビジョン放送等のうちデジタル放送に関する送信の標準方式」第21条は、通常の地上デジタルテレビジョン放送について階層を「13個のOFDMセグメントを最大3個に区分したもの」と規定する。ARIB STD-B31 2.3の現行公開サンプルでも対応する規範領域は第2章2.1「階層伝送」および第3章3.4「階層分割」として維持されている。ただし公開サンプルは当該本文を収録していないため、そのサンプルだけから2.3本文の具体文言を推測せず、最大3という具体値の現行一次根拠には前記省令第21条を使用する。ARIB本文との版差分確認は本書「ARIB規範本文との静的照合」の証拠境界に従う。

したがって非空配列は入力順序を保持したまま最大3要素を物理階層要求として検証し、4要素以上はAOSP vector encodingの異常ではなく、対象ISDB-T方式に存在しない4番目以降の物理階層を要求するsemantic inputとして `INVALID_ARGUMENT` で要求全体を状態不変のまま拒否する。AOSPがlayer名やvector indexにA/B/Cという追加意味を固定していないため、本書もindex 0 / 1 / 2へ独自のA/B/C名称を再定義せず、callerが与えた先頭からの階層順を保持する。

各要素の `modulation`、`codeRate`、`timeInterleave`、`numOfSegment` は既存のISDB-T element-level契約で独立に検証する。1要素でもmalformed / 値域外なら要求全体を `INVALID_ARGUMENT`、AIDL/放送方式上有効でも対象backendが実処理・検証できない明示値を含む場合は要求全体を `UNAVAILABLE` とし、旧tune/scan request、backend、generationを変更しない。現行px4 / earth_pt1のAUTO/未指定中心の能力方針を、配列要素数を理由に拡張しない。

### frontend settings validation の固定方針

フロントエンドの対応能力、AIDL入力の受付可否、`ProductProfile`、VTSの選局入力は、本書の「フロントエンド設定の反映表」から生成する。ARIBが定義する放送パラメーター集合と、対象バックエンドが明示的に設定できる入力集合を混同しない。具体値を対応可能として公開または受理できるのは、ドライバーへ設定する経路、または読み戻して検証する経路が存在する場合だけとする。値を検証するだけでバックエンドへの要求から捨て、成功を返す経路は禁止する。

対象の px4 / earth_pt1 による ISDB-T の設計上の backend capability は、設定表に従い、周波数と 6 MHz または `AUTO` の帯域幅に対応する。モード、階層ごとの変調方式と符号化率、ガードインターバル、階層ごとの時間インターリーブについては、対象 backend に具体値を設定する経路または読み戻して検証する経路の証跡がないため `AUTO` だけを対応能力として宣言する。`AUTO` は成功とし、規格上既知でも具体値を実処理・検証できない要求は `UNAVAILABLE` を返して、backend と直前の要求を変更しない。不正なタグまたは値域には `INVALID_ARGUMENT` を返す。対応能力、AIDL 入力検証、`ProductProfile`、VTS 選局入力は同じ設定表から生成する。ARIB STD-B31 2.2-E1 の 2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7 は放送パラメーターの値域と伝送上の意味を定めるが、`AUTO` のみという制限は ARIB 上の制約ではなく、本製品が採用する backend capability である。


`endFrequency`はAOSPのblind scan範囲終端としてだけ解釈する。`IFrontend.tune()`およびblind以外のscanでは選局条件ではないため、`endFrequency`が`frequency`と異なっていても拒否せず、正規化済みrequest fingerprint、backend tune request、選局結果の適合条件へ含めない。本製品はblind scanを対応宣言しないため、blind scan要求は正常な`endFrequency`を含めて`UNAVAILABLE`とし、既存tune/scan stateを変更しない。blind以外の操作で`endFrequency`差分を独自のexplicit範囲scanとして再解釈してはならない。

### ISDB-T validation

- `frequency`はtarget channel mappingへ変換可能な値だけを受け付ける。
- `bandwidth=UNDEFINED`はcallerがbandwidth制約を指定していないAOSP sentinelとして成功させ、capability bitとしてadvertiseしない。`AUTO`または`BANDWIDTH_6MHZ`は対応値として受理し、既知の`BANDWIDTH_7MHZ` / `BANDWIDTH_8MHZ`は`UNAVAILABLE`とする。予約値・未知値は`INVALID_ARGUMENT`とする。
- `mode`、layer `modulation`、layer `codeRate`、`guardInterval`、layer `timeInterleave`の`UNDEFINED`はcaller constraintなしとして成功させ、capability bitとしてadvertiseしない。`AUTO`だけを対応値としてadvertise・受理し、既知の具体値は`UNAVAILABLE`とする。予約値・未知値は`INVALID_ARGUMENT`とする。
- `UNDEFINED`と`AUTO`を同義に正規化してはならない。`UNDEFINED`は制約なし、`AUTO`はhardwareによる自動検出・設定を要求する既知の明示値としてtyped requestで区別し、拒否時はバックエンドと直前の要求を変更しない。
- `inversion`は未指定・自動を表すAIDL値だけを、明示制約なしとして成功させる。本製品の対象backendは明示inversionを設定または固定値検証する能力を採用しないため、規格上有効な明示inversionは`UNAVAILABLE`とする。予約値・未知値は`INVALID_ARGUMENT`とする。
- `serviceAreaId=0`は未指定として成功させる。本製品の対象backendは正の`serviceAreaId`をbackend requestまたは選局結果検証へ反映する能力を採用しないため、構文上有効な正の値は`UNAVAILABLE`、負値は`INVALID_ARGUMENT`とする。
- `partialReceptionFlag=UNDEFINED`はcaller constraintなしを表すAOSP sentinelとして成功させる。`AUTO`はhardwareによる自動判定を求める既知の明示要求だが、現行product profileではその要求を設定・検証する契約を持たないため、backend副作用前に`UNAVAILABLE`とする。`TRUE` / `FALSE`は規格上有効な明示要求である。`IFrontend.tune()`同期戻り値は、要求の構文・capability・資源・backend開始可否を検証して選局処理を受理できたことだけを表し、lock後のTMCC照合結果を後から同期戻り値へ反映しない。px4は採用するread-only `PTX_GET_TMCC_PARTIAL_RECEPTION`を用い、対象demodulatorが自動判定した同一tune generationのfreshなTMCC readbackが要求値と一致した場合だけ、その要求で指定されたsignalへlockしたものとして`FrontendEventType::LOCKED`を通知する。不一致は要求されたsignalへlockできなかったものとして`NO_SIGNAL`とし、readback未確定・I/O失敗・古いgenerationでは`LOCKED`を生成せず既存のbackend failure契約に従う。scanでは同じfresh readback一致を当該candidateの成立条件とし、不一致または未確定をlock済みcandidateとして通知しない。earth_pt1 / TC90522は`../future_work/not_planned/earth_pt1_tc90522_tmcc_readback_error_propagation_blocker.md`が未解決の間、readback成立を偽装せず明示`TRUE` / `FALSE`を`UNAVAILABLE`とする。予約値・未知値は`INVALID_ARGUMENT`とする。
- layer `numOfSegment=0`は未指定として成功させる。`0xFF`はAndroid 14 CTSが`isSegmentAutoSupported()==true`のfrontendへ送る互換AUTO要求として扱い、`isSegmentAuto=true`ならbackend/demodulatorのsegment自動判定を使用して成功させ、`false`なら`UNAVAILABLE`とする。本製品の対象backendはlayerごとの明示segment数を反映または固定値検証する能力を採用しないため、構文上有効な`1..13`は`UNAVAILABLE`とする。`14..254`、負値、255を超える値は`INVALID_ARGUMENT`とする。
- 上記4項目を含むsettingsは、成功時だけ正規化済みrequest fingerprintへ含める。`UNAVAILABLE`または`INVALID_ARGUMENT`では旧tune/scan、backend、generationを変更せず、入力値を黙って捨てて成功してはならない。
- blind scanは`UNAVAILABLE`とする。

ISDB-T設定値の規格上の妥当性は、今回精読したARIB公式英訳STD-B31 2.2-E1本文の2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7を証拠本文とし、現行日本語版2.3との差分未証明を別管理する。一方、対象ドライバーで設定可能かどうかは独立した根拠で判定する。本製品の対象バックエンドは、モード、変調方式、符号化率、ガードインターバル、時間インターリーブについて `AUTO` だけを対応能力として採用し、明示要求として公開・受理する。`UNDEFINED` は対応能力値ではなくcaller constraintなしを表すAOSP sentinelとして別途受理する。規格上の具体値を解析や試験のため内部表現に保持してよいが、制御可能な設定として公開または受理してはならない。


ARIB STD-B31 2.2-E1は、モードを2.3、内符号化率を3.8と3.15.6.6、搬送波変調を3.9と3.15.6.5、時間インターリーブを3.11.1と3.15.6.7、ガードインターバルを3.14.2で定義する。本製品の対象バックエンドで明示要求として`AUTO`だけを対応値として受け付け、`UNDEFINED`はcaller constraintなしのAOSP sentinelとして別途受理することは、ARIB上の値を否定するものではない。明示的な設定経路がない対象について、対応能力を過大に表明しないための制限である。

### ISDB-S validation

- public settingsの`symbolRate=0`はAOSPの未指定sentinelとして成功させ、広告する`minSymbolRate..maxSymbolRate`の値域外であっても拒否しない。正値は、同じ不変`CapabilitySnapshot`が広告する範囲内であり、かつbackendへ適用する経路がある場合だけ成功させる。px4と現行Linux DVB / earth_pt1の製品profileは固定値28,860,000だけを広告・受理する。Linux v6.6 `tc90522_ops_sat.info`はsymbol-rate capabilityを設定せず`FE_GET_INFO`では0/0となる一方、同driverのISDB-S readbackと採用`qm1d1b0004` module経路は28,860,000を固定の実効値とするため、earth_pt1の`CapabilitySnapshot`はdriver名・完全topology・周波数範囲のprobe成功を前提に`minSymbolRate=maxSymbolRate=28,860,000`を製品profile証跡として確定する。public入力0はBinder境界で未指定のまま保持し、backend mappingで固定実効値へ正規化して`DTV_SYMBOL_RATE=28,860,000`を必ず投影する。その他の正値または`u32`へ収まらない値はbackend副作用、旧request、generationの変更前に`INVALID_ARGUMENT`とする。AOSPの`IsdbsFrontendSettings.Builder.setSymbolRate()`と`FrontendInfo.minSymbolRate/maxSymbolRate`はこの明示値経路を公開しており、CDD/VTSに`0`限定の契約は置かれていない。将来`FE_GET_INFO`由来へ戻す条件とupstream改善候補は`../future_work/not_planned/linux_tc90522_isdbs_symbol_rate_capability_metadata.md`に記録し、現行runtime契約の根拠にはしない。
- AOSP SDK defaultの`STREAM_ID + INVALID_STREAM_ID(0xFFFF)`は、BS/CS110を問わず明示TSIDの値域検証より先に`Unspecified`へ正規化する。通常の日本向けBS scan、channel保存、ライブ再選局ではTISが検出・保存したabsolute TSIDを明示し、`Unspecified` fallbackをサービス選択に使用しない。px4 BSの`Unspecified`は現行ABI上の互換fallbackとしてrelative slot `0`へ写像するが、callerがslot 0を指定したとは扱わない。Linux DVB / earth_pt1の`Unspecified`は`DTV_STREAM_ID=NO_STREAM_ID_FILTER`へ明示写像し、前回のselectorをproperty cacheへ残さない。CS110は従来どおりselectorなしのfrequency-only選局を使用する。
- modulationとcodeRateの`UNDEFINED`はcallerが当該制約を指定していないAOSP sentinelとして成功させ、capability bitとしてadvertiseしない。`AUTO`はhardwareによる自動検出・設定を求める既知の明示要求としてtyped requestに保持し、対応値としてadvertise・受理する。既知具体値は`UNAVAILABLE`、予約値・未知値は`INVALID_ARGUMENT`とする。
- `rolloff`は未指定を表すAIDL値を明示制約なしとして成功させる。本製品の対象backend/deviceは明示rolloffを設定または固定値検証する能力を採用しないため、規格上有効な明示rolloffは`UNAVAILABLE`、予約値・未知値は`INVALID_ARGUMENT`とする。入力`rolloff`をbackend requestから捨てたまま成功してはならず、拒否時は旧tune/scan、backend、generationを変更しない。
- blind scanは`UNAVAILABLE`とする。

対象のpx4/earth_pt1によるISDB-Sでは、変調方式と符号化率は `AUTO` だけを対応能力として採用する。`UNDEFINED` は対応能力値ではなくcaller constraintなしを表すAOSP sentinelとして成功させ、`AUTO` はhardwareによる自動検出・設定を求める明示要求として成功させる。規格上既知の具体値には状態を変えず `UNAVAILABLE`、予約値・未知値には `INVALID_ARGUMENT` を返す。相対TS番号とTS_IDを別のselector domainとして扱う根拠はARIB STD-B20 3.0の2.9（別記第2・第3）と2.10、周波数の根拠はSTD-B21 5.12-E2とし、セレクター設定表で動作を別に定める。

対象バックエンドのISDB-S変調方式は `AUTO` だけを対応能力として採用する。BPSK、QPSK、TC8PSKの明示指定には状態を変えず `UNAVAILABLE` を返す。

対象バックエンドのISDB-S符号化率は `AUTO` だけを対応能力として採用する。符号化率の明示指定には状態を変えず `UNAVAILABLE` を返す。


共通検証はBinder層の要求変換と`service_runtime`の事前確認で実施する。ただし、設定権限を持たない層が具体値を成功扱いにしてはならない。検証済みの要求だけをバックエンドへ渡し、未対応の入力では以前のワーカーと選局状態を破壊しない。

## ライブ AV filter / FMQ 方針

ライブAVフィルターを正式な対象範囲に含める。本製品は、パススルーではない`MediaEvent`について2種類の伝送方式に対応する。第一選択は公開済みの共有領域と正の`dataId`の組、代替方式はイベント固有の正確な長さを持つ1個のファイル記述子と正の`dataId`の組とする。AVペイロードは通常のFMQへ書き込まない。`EventFlag`はFMQを使用する経路の通知だけに使用する。

AV passthrough は本製品では恒久的に対応しない。`DemuxFilterAvSettings.isPassthrough=true` は configure 時点で `UNAVAILABLE` とし、passthrough capability は宣言しない。成功扱いの無処理 または無配送の AV filter として受け入れてはならない。

VTS/profileでは、AV filterを使用する場合でも `isPassthrough=false` に固定する。`isPassthrough=true` を含むprofileは本製品の対応profileとして扱わない。

AV filter の状態別契約、shared backing、公開済みハンドル、使用中領域、`dataId`、`flush()`、`configure()`、`close()` の副作用は、0-S-2のAV active token台帳、本節の「AV転送方式とクライアント側の存続期間」および後段の「AV割り当て」を正とする。本節では、allocator、NativeHandle形式、payload配置、診断方針だけを補足する。

AndroidフレームワークとJNIが受理する`MediaEvent`の表現は、本書の「AV割り当て」を正とする。共有モードでは、`IFilter.getAvSharedHandle()`がdma-bufまたはION系のファイル記述子1個を持つハンドルを返す。各イベントの`avMemory`は空とし、正の`avDataId`と`offset/dataLength`で共有領域内の半開区間を識別する。イベント固有モードでは、各イベントが正確な長さのファイル記述子1個を持つ`avMemory`と、正の`avDataId`を持つ。共有ハンドルの未取得、使用権の解放済み、収容可能な空き領域なし、またはAUが領域長を超える場合は、イベント固有モードを正式な代替方式とする。過大なAUを破棄して、2方式対応という能力表明と矛盾させてはならない。

両モードの`avDataId`は、同じ上限付き割り当て台帳から発行する。メモリー、台帳、`MediaEvent`の準備がすべて成功した後に割り当てを確定し、失敗時はコールバックまたは`dataId`を公開しない。`offset + dataLength <= backing size`を正常範囲とし、上限超過を検出できる加算を用いる。長さ0は不正としてイベントを発行しない。`isSecureMemory=false`に固定する。

解放要求の形状、active `avDataId` tokenのowner・generation・transfer kind検証、inactive/unknown tokenの拒否、ファイル記述子の補助検証、論理閉鎖後の解放は、0-S-2のAV active token台帳と「AV転送方式とクライアント側の存続期間」を正とする。`releaseAvHandle(fd,0)`を共有記憶領域全体の破棄と解釈してはならない。イベント固有モードでは、受領したハンドルの使用権だけを先に閉じ、正のactive tokenとallocationを後続解放まで維持できる。

### clear non-passthrough `DemuxFilterMediaEvent` 公開field契約

本製品が成功対応として表明するTS入力のclear / non-passthrough live AVでは、Android 14 Tuner AIDL V2で`DemuxFilterMediaEvent`に公開されるfieldを、同一media outputへauthoritativeに関連付けられる入力metadataまたは以下で明示した未使用/default表現から構成する。別PES、別generation、別media outputのmetadataを混成せず、入力から確定できない値を推測して生成しない。

| field | TS live AVでの値契約 |
|---|---|
| `streamId` | 当該media outputを生成したPESの実`stream_id` `0..255`。別PES、filter設定値、固定値から推測しない |
| `isPtsPresent` / `pts` | 後段の「clear non-passthrough MediaEvent presentation timestamp 契約」を正とする |
| `isDtsPresent` / `dts` | 元PES headerに有効なDTSが存在する場合だけ`isDtsPresent=true`とし、その33-bit 90 kHz DTSを`dts`へ設定する。DTSが存在しない場合は`isDtsPresent=false`かつ`dts=0`とし、PTS、PCR、wallclock等からDTSを生成しない |
| `dataLength` / `offset` / `avMemory` / `avDataId` | 本節のAV allocation / transfer契約を正とする |
| `isSecureMemory` | `false` |
| `mpuSequenceNumber` | TS入力ではMMTP用sequenceを生成せず`0` |
| `isPesPrivateData` | 当該media outputに対応する元PESの構文からprivate dataであることをauthoritativeに確定できる場合だけ`true`、それ以外は`false`。stream IDや用途名だけから推測しない |
| `extraMetaData` | audioでauthoritativeな`AudioExtraMetaData`を構成できる場合だけ`audio` tag/valueを設定し、それ以外は`noinit`。videoは`noinit`。存在しないaudio metadataを生成しない |
| `scIndexMask` | 当該media outputに対応するstart-code indexをauthoritativeに確定できる場合だけ対応するAndroid 14 AIDL union tag/valueを設定し、それ以外は`scIndex=0`。record indexや別outputのindex情報を流用しない |

PES parserがDTSまたはprivate-data presenceを有効入力として既に確定したmedia outputについて、それを「未対応」として虚偽のabsenceへ変換してはならない。producer側で成功capabilityに必要な公開fieldのauthoritative associationを成立させられないbackend/profileは、そのlive media-filter capabilityを公開しない。

### AV shared handle の `NativeHandle` 形式

| 項目 | 固定値 | 理由 |
|---|---|---|
| fd数 | 1 | shared backing fd を framework/JNI へ渡すため |
| ints数 | 1 | Android framework/JNI が参照する memory index だけを公開するため |
| `ints[0]` | 0 | 単一 shared memory の index。HAL内部識別子ではない |
| `ints[1..]` | 出さない | HAL内部識別子を framework/JNI へ公開しないため |
| `slot_size` / `slot_count` | 出さない | HAL内部の領域管理値であり、`NativeHandle.ints` ではないため |
| magic / generation / filter id | 出さない | JNI が int を memory index として読むため |

### AV転送方式とクライアント側の存続期間

| 状態 | AV payload到着時の動作 |
|---|---|
| 共有ハンドル公開済み、クライアント使用権が有効、収容可能な空き領域あり | 共有領域へ配置し、空のハンドルと正の`dataId`を持つ`MediaEvent`を発行する |
| 共有ハンドル未取得、またはクライアント使用権を解放済み | イベント固有の正確な長さを持つファイル記述子を割り当て、当該ハンドルと正の`dataId`を持つ`MediaEvent`を発行する |
| shared slotなしまたはAU > slot size | event-local exact-size fdへfallbackする |
| allocation lease pool exhausted | 当該イベントを破棄し、filter status callbackで`OVERFLOW`を通知する。`av_allocation_drop`と`av_allocation_pool_exhausted`を増やし、filterは開始状態を維持する。既存allocationをevictしない。後続payloadごとに再度割り当てを試す |
| イベント固有領域の割り当て失敗 | 当該イベントを破棄し、filter status callbackで`OVERFLOW`を通知する。`av_allocation_drop`と`av_event_local_allocation_failure`を増やし、filterは開始状態を維持する。AIDL戻り値は存在しないため`UNAVAILABLE`を返したことにしない。実体のない`MediaEvent`または`dataId`を公開せず、後続payloadごとに再度割り当てを試す |
| `getAvSharedHandle()`再取得 | 新規または現在の共有クライアント使用権を有効にし、後続イベントで共有モードを再選択できるようにする |

## A/V sync 方針


### AV sync hardware ID 所有契約

AV sync hardware ID は `filter_id & 0xffff` のような media filter ID の単純変換から導出しない。media filter と hardware sync ID の relation cardinality、reverse index の有無と形、register / unregister / close 時の mutation・commit / abort semantics は 0-S-3B の `AvSyncRegistry` を唯一の正本とし、本節では再定義しない。


AV filterを対応宣言する demux は AOSP の `getAvSyncHwId(Filter)` と `getAvSyncTime(int)` の契約に沿って A/V sync ID と 90kHz timestamp を返す。`getAvSyncHwId(media filter)` は AV filter 固有IDではなく、対応する PCR filter ID を返す。section、PES、record、閉鎖済み filter、対応する PCR filter が存在しない media filter には契約に従った失敗を返す。

`getAvSyncHwId()` は、対象 media filter に対応する PCR filter が configure 済みであれば、PCR 観測前でもその PCR filter ID を返す。PCR 観測済みかどうかを sync ID 返却の前提にしない。PCR 未観測状態は `getAvSyncTime(id)` の戻り値側で未確定値として表現する。

同一demuxに属する稼働中のPCRフィルターを示す有効なA/V同期IDについて、0-S-3Bの`PcrClockAnchorStore`に当該generationの有効anchorがない場合は`getAvSyncTime()`を成功させ、`Tuner.INVALID_TIMESTAMP`を返す。anchorの初回生成、後続PCRによる33-bit unwrap / 更新、discontinuity・PCR逆行・filter/source/stream/frontend/playback境界による無効化、stale generation拒否、時計逆行時の無効化は`PcrClockAnchorStore`だけを正本とし、本節ではmutationを再定義しない。

有効anchorがある場合のcaller-visibleな時刻計算は、`current_90k = (unwrapped_pcr_90k + floor((now_monotonic_ns - monotonic_base_ns) * 90000 / 1000000000)) mod 2^33`とし、PCR到着間隔中もmonotonic clockで進行させる。計算は符号なしオーバーフローを起こさない拡張精度で行う。`PcrClockAnchorStore`がanchorを無効と判定した場合は`Tuner.INVALID_TIMESTAMP`を返す。別demuxのID、PCR以外のフィルターID、閉鎖済みID、不明なIDには`INVALID_ARGUMENT`を返す。値0を未観測時の特別値として公開してはならない。


## A/V sync 非採用範囲

AV filter の `start()`、共有ハンドル、MediaEventの状態別契約と`releaseAvHandle()`の資源寿命・解放契約は、0-S-2のAV active token台帳、前節の「AV転送方式とクライアント側の存続期間」および後段の「AV割り当て」を正とする。本節では A/V sync の非採用範囲だけを恒久契約として固定する。


- PTS は current A/V sync clock の 代替処理 として使わない。
- PCR anchorの観測・33-bit wrap・更新・無効化条件は0-S-3Bの`PcrClockAnchorStore`を唯一のmutation正本とし、90 kHzへのcaller-visibleな整数変換は直前の`getAvSyncTime()`契約を正とする。
- 本製品の canonical A/V sync contract は `PcrClockAnchorStore` が所有する観測済み PCR anchor と monotonic clock の対応だけを用いる。PCR PID 明示管理、サービス clock モデル、jitter smoothing、PLL / clock discipline、複数 clock source の品質評価、CTS / VTS / 実波ベースの補正モデルは本製品契約として導入しない。

## LNB能力と固定給電


LNBは機器単位の終端資源とし、本書の「LNB機器の資源規則」と事象駆動の「ワーカー終了契約」だけで管理する。`aidl_baseline_eligible`は、Android 14 CTSがnon-nullの公開LNB objectへ要求する基礎操作一式、すなわち対応電圧、`setTone(TONE_NONE)`、`setSatellitePosition(POSITION_A)`、2バイトの`sendDiseqcMessage()`、登録済みcallbackへの受信通知を、成功扱いの無処理ではなくbackend契約として実処理できるかを表すCTS baseline適合分類とする。この分類は公開`ILnb` endpoint全体の生成可否を決めるgateではない。

本製品はAndroid 14 CTSのLNB試験合格より、hardware / driverが実処理できることを証跡で確認したLNB operation / valueをframeworkへ公開することを優先する。LNB endpointは、probeに成功し、satellite frontendへ接続可能で、endpoint lease条件を満たし、かつ少なくとも1つの公開operation / valueに実処理証跡がある場合に`CapabilitySnapshot`へ公開対象としてcommitできる。`aidl_baseline_eligible=false`だけを理由にendpoint全体を隠してはならない。証跡のないoperation / valueを能力として生成してはならず、有効だが対象backendで未対応の要求は副作用なしの`UNAVAILABLE`とする。未知の列挙値その他の不正要求は`INVALID_ARGUMENT`とし、backend未対応と区別する。

versioned `SupportedDeviceCapabilityCatalog` のpx4 / earth_pt1項目は、公開LNB operation / value capabilityとして電圧制御だけを保持し、tone、satellite position、DiSEqCの実処理能力を保持しないため`aidl_baseline_eligible=false`とする。px4項目は0 V / 15 V、earth_pt1項目は11 V / 15 Vのcaller制御可能な電圧operationを持つendpointとして公開できる。`getLnbIds()`はこれらの公開対象endpointを列挙し、`openLnbById()`はendpoint leaseを取得して`ILnb` objectを生成する。tone、satellite position、DiSEqC等の未対応要求をCTS合格目的の成功no-op、擬似成功、callback echoにしてはならない。本製品は部分LNB公開によりAndroid 14 CTSのLNB試験が失敗し得ることを既知compatibility deltaとして受容し、CTS LNB適合を宣言しない。hardware / driver能力が変化した場合は、既存のcatalog再評価条件に従ってversioned項目を更新する。

公開`ILnb` operation能力とsatellite frontendの電源トポロジは別能力として扱う。`SupportedDeviceCapabilityCatalog`の機器項目は、`InternalFixed15V`、`ExternalOrShared`、`UnknownOrDisabled`のいずれかを保持する。`InternalFixed15V`は、物理rail owner、15 Vの適用確認方法、停止時の安全状態、共有互換条件を同じ項目に持ち、frontend generation開始前に既存の機器単位rail leaseを取得して15 Vを実適用できる場合だけ成立する。`ExternalOrShared`は、給電主体、HALが電圧を変更しないこと、共有互換条件、選局中の給電継続を製品配線として確認できる場合だけ成立する。

`InternalFixed15V`または`ExternalOrShared`が検証済みでruntime LNB切替を必要としないISDB-S frontendは、公開`ILnb` endpointの有無と独立に公開可否を判断する。前者ではHAL内部で選局前に固定15 Vを適用し、後者ではHALは電圧操作を行わない。固定給電だけをcaller制御可能な`ILnb.setVoltage()`能力として広告してはならない。`UnknownOrDisabled`、トポロジ証跡不一致、給電継続または共有互換性を確認できない場合はsatellite frontendを公開しない。給電、リース、選局準備失敗時の巻き戻し、安全状態復帰、共有レール参照管理、実状態不明時の隔離は、「公開transactionのphase・確定点・失敗処理契約」、0-S-2の`LnbRegistry`状態所有、0-S-3Bの`FrontendLnbRelationTxn` / `LnbControlTxn` / `ObjectCloseTxn` / `WorkerRuntime`を適用する。`LnbControlTxn`は公開永続制御の手順だけを所有し、物理レール状態またはI/O直列化状態を所有しない。固定給電だけのための専用profileや別状態機械を設けない。

`getLnbIds()`は起動時probeとoperation/value capability対応表から公開対象として確定したendpoint IDを列挙する。`openLnbById()`は公開済みendpoint 1個の使用権を取得する。不明なIDには`INVALID_ARGUMENT`、使用中、`CleanupPending`、`Quarantined`のendpointには状態を変えず`UNAVAILABLE`を返す。`openLnbByName()`は本製品で名前付き外部LNBを公開しないため成功対象を持たず、空文字を`INVALID_ARGUMENT`、その他の名前を`UNAVAILABLE`とする。LNB ID、object、leaseを生成せず、出力を部分公開しない。`ILnb.close()`の公開結果とendpoint leaseの最終解放条件は0-S-4の公開close契約と「LNB機器の資源規則」を正とする。cleanup authority、retry / handoff、worker回収、quarantineの内部semanticsは0-S-3Bの`ObjectCloseTxn`と`WorkerRuntime`を正本とし、本節では再定義しない。

公開するLNB IDはsatellite frontendへ接続できる論理endpointとして扱い、1個のendpoint leaseを複数frontendへ同時接続しない。`setLnb(lnb_id)`は当該satellite frontendへ接続可能なLNB IDだけを受け付け、別の物理機器に属するLNB ID、地上波frontendへのLNB接続、不明なLNB IDは失敗させる。同一の物理LNBまたは電圧レールへ到達し得るbackend I/Oは、公開LNB制御、HAL内部固定給電、DiSEqC送信、安全状態復帰の入口にかかわらず、`LnbRegistry`が物理競合単位ごとに所有する同じI/O権限で直列化する。I/O権限待ちの間は`LnbRegistry`の状態ロックを保持せず、権限取得後に寿命・世代・リースを再検証する。共有レールの互換要求と参照数は`LnbRegistry`だけを正本とする。

`IFrontend.setLnb(lnb_id)`はsatellite frontendへhardware LNB resource assignmentを設定し、複数回の呼出しでassignmentを更新できる公開操作とする。Frontend closeでは当該assignment resourceを解放する。frontend-LNB relation、assignment lease参照、atomic commit、rollback / cleanupの論理契約は0-S-3Bの`FrontendLnbRelationTxn`を唯一の正本とし、本節では内部mutation semanticsを再定義しない。CI CAM系は本書「非対応のTimeFilterとCI CAM」の契約どおり非対応であり、`FrontendLnbRelationTxn`の成功対象へ含めない。

`ILnb.setCallback(callback)` のBinder callback artifact / strong refは共通callback storeだけが所有し、runtime登録は`RuntimeCallbackRegistry`、LNB domainはlogical callback stateだけを所有する。set / replace / `callback == NULL` による解除 / close / owner lossは`CallbackRegistrationUseCase`の登録・cleanup契約へ接続し、`LnbHal`またはLNB個別use-caseがBinder callback実体、runtime registry、rollback方針を第二の正本として直接所有してはならない。`callback == NULL` はAOSP契約上の登録解除として成功対象に含める。AOSP frozen/stable AIDL のvendor独自改変、生のBinder transaction解析器による公開契約を通さない実装は採用しない。

### ILnb公開操作

公開操作は、閉鎖状態、入力妥当性、製品対応能力、backend適用の順に判定する。

| API | 有効入力 | 本製品の結果 | 内部失敗契約 |
|---|---|---|---|
| `setVoltage(voltage)` | AIDL列挙値であり、対象profileの対応電圧 | 対応表に従って実機へ適用する。profile非対応の有効電圧は`UNAVAILABLE` | 検証からbackend適用、準備済み変更の確定・取消しまでの手順は0-S-3Bの`LnbControlTxn`、永続状態・世代・失敗・隔離・物理I/O直列化は0-S-2の`LnbRegistry`を正本とする |
| `setTone(tone)` | AIDLの有効列挙値 | 対応表でbackend実処理が確認された値だけを適用して成功する。有効だが対象backendで未対応の値は副作用なしの`UNAVAILABLE`。成功扱いの無処理は禁止 | 検証からbackend適用、準備済み変更の確定・取消しまでの手順は0-S-3Bの`LnbControlTxn`、永続状態・世代・失敗・隔離・物理I/O直列化は0-S-2の`LnbRegistry`を正本とする |
| `setSatellitePosition(position)` | AIDLの有効列挙値 | 対応表でbackend実処理が確認された値だけを適用して成功する。有効だが対象backendで未対応の値は副作用なしの`UNAVAILABLE`。成功扱いの無処理は禁止 | 検証からbackend適用、準備済み変更の確定・取消しまでの手順は0-S-3Bの`LnbControlTxn`、永続状態・世代・失敗・隔離・物理I/O直列化は0-S-2の`LnbRegistry`を正本とする |
| `sendDiseqcMessage(message)` | 非空byte列。実処理可能なbackendでは宣言済み上限内とし、Android 14 CTSが使う2バイト要求を能力対応時は受理できる | 対応表でDiSEqC実処理が確認されたbackendだけ全byteを送信し、送信完了後に成功する。有効だが対象backendで未対応なら副作用なしの`UNAVAILABLE`。送信していないmessageのcallback echoは禁止 | 一時送信固有契約として、`LnbRegistry`の同一物理I/O権限を取得して送信し、部分送信を成功にせず、送信開始後に実状態不明なら`LnbRegistry`で当該LNBを隔離する。未対応判定だけなら状態不変 |

閉鎖開始後は全操作を`INVALID_STATE`とする。不明な列挙値、空メッセージ、またはDiSEqC実処理を宣言したbackendの上限を超えるメッセージは`INVALID_ARGUMENT`とする。DiSEqC未対応backendでは、上限検証以前に有効な非空messageを副作用なしの`UNAVAILABLE`とする。2バイトを長さだけで拒否してはならない。妥当だが個別profileで非対応のoperation / valueは`UNAVAILABLE`とする。`aidl_baseline_eligible=false`はCTS baseline適合分類であり、endpoint全体の公開可否を単独では決めない。



`setVoltage()` / `setTone()` / `setSatellitePosition()` の検証、準備済み変更取得、backend適用、確定・取消し、公開結果への写像は0-S-3Bの`LnbControlTxn`を正本とする。永続制御状態、LNB状態世代、失敗・隔離状態、共有レールのリース・参照数、物理I/O直列化権限は0-S-2の`LnbRegistry`を唯一の正本とする。backend適用成功後の準備済み変更の確定は追加の資源割当やbackend I/Oを含めず失敗不能とし、backend成功後に別の失敗し得るregistry確定段階を設けない。

LNB固有の安全状態復帰は後始末対象として`ObjectCloseTxn`へ型付き後始末指示で渡し、backend I/Oは公開制御と同じ`LnbRegistry`の物理I/O権限を通す。public `close()`、owner loss、Dropのcleanup開始authority、`begin_close`、handoff、reaper、実行方式は0-S-3Bの`ObjectCloseTxn`だけを正本とし、LNB個別節では再定義しない。


## IDescrambler demux結合契約

`ITuner.openDescrambler()`にはdemux入力がないため、生成時にdemuxまたはdemux依存の復号poolを推測してはならない。source-call状態は`NeverCalledUnbound`、`CallConsumedUnbound(failure)`、`Bound(demux_id, demux_generation, pool_id)`のいずれか一つとする。論理閉鎖状態は別軸であり、閉鎖gateをsource-call状態より先に判定する。

`IDescrambler.setDemuxSource(demuxId)`のLiveな初回呼出しは、成功・失敗にかかわらず一回性を消費する。session transaction lock内で`NeverCalledUnbound`を確認した時点で`source_call_consumed=true`を不可逆に確定し、その呼出しだけがdemux検証とpool予約へ進む。以後は同じIDを含む全ての再呼出しを`INVALID_STATE`とする。検証または予約失敗ではdemux/poolへ結合せず`CallConsumedUnbound(failure)`に残すため、利用を続けるには当該descramblerをcloseして新しいobjectをopenする。

| 操作 / 入力状態 | 検証と確定 | AIDL戻り値 | 次状態 / 副作用 |
|---|---|---|---|
| `openDescrambler()` | descrambler能力とobject/session枠を満たす場合に未結合objectを公開する。内部reservation / runtime・Binder prepare / commit / rollbackは`root/child open`を正とし、demux ID、demux generation、pool IDは記録しない | 成功 | `NeverCalledUnbound`。demux pool、鍵組、PID claimは消費しない |
| `setDemuxSource(id)` / `NeverCalledUnbound` / 公開済みで生存する対応demux | 一回性を先に消費し、demux ID、同じサービスのlive generation、対応する復号経路、共有poolのsession受付可否を検証する。`{demux_id, demux_generation, pool_id}`とpool session帰属を同一transactionで確定する | 成功 | `Bound`。以後sourceを変更しない |
| `setDemuxSource(id)` / `NeverCalledUnbound` / 未公開ID | 一回性を消費し、poolを予約しない | `INVALID_ARGUMENT` | `CallConsumedUnbound(InvalidDemuxId)` |
| `setDemuxSource(id)` / `NeverCalledUnbound` / 公開IDだがdemuxが閉鎖済みまたはgenerationが無効 | 一回性を消費し、poolを予約しない | `INVALID_STATE` | `CallConsumedUnbound(InvalidDemuxState)` |
| `setDemuxSource(id)` / `NeverCalledUnbound` / 有効なdemuxだが復号経路非対応またはpool session枯渇 | 一回性を消費する。仮予約済みのpool帰属は返却する | `UNAVAILABLE` | `CallConsumedUnbound(UnsupportedOrCapacity)` |
| `setDemuxSource(any)` / `CallConsumedUnbound`または`Bound` | AOSPの一回限り契約を入力検証より先に適用し、再設定を受け付けない | `INVALID_STATE` | 既存のfailureまたはdemux ID、generation、pool帰属を維持 |
| `setDemuxSource(any)` / 論理閉鎖済み | 閉鎖状態を入力より先に判定する | `INVALID_STATE` | 状態と資源を変更しない |

`setKeyToken(non-VOID)`、`addPid()`、`removePid()`は`Bound` sessionだけを対象とする。`NeverCalledUnbound`と`CallConsumedUnbound`では`INVALID_STATE`を返し、鍵参照とPID claimを作らない。source demuxのgenerationが消失した場合も新しい操作は`INVALID_STATE`とし、別demuxへ再結合せず、保持中のclaimはcloseまたはdemux無効化の後片付けで同じpoolへ返す。`close()`は未結合の2状態ならobject/session枠だけ、`Bound`ならpool session帰属、鍵参照、PID claimを含む全後片付けを試行し、0-S-4の公開close契約と0-S-3Bの`DescramblerSessionCleanupTxn` / `ObjectCloseTxn`に従う。

## 復号鍵台帳

`IDescrambler.setKeyToken()` が受け取る値は復号鍵そのものではなく、不透明な参照値である。Tuner HAL はこの参照値で復号鍵台帳を引き、内部の `DescramblerKeySlot` に変換する。Binder 境界を越える バイト列に MULTI2 の system key、CBC 初期値、偶数鍵、奇数鍵を入れてはならない。r52のCAS/Tuner内部鍵資源の完全なmaterial構成、供給・更新・revoke境界は`../future_work/r52/b25_key_slot_registry_contract.md`を正とし、本書では公開AIDL境界とTuner側のtoken解決意味だけを定義する。

復号鍵台帳の key slot 状態は次で固定する。

| 状態 | 意味 | resolve結果 | 復号可否 | 設計上の成立条件 |
|---|---|---|---|---|
| `Registered` | CAS bridge または test 専用登録により、内部鍵参照が有効である。refcount は 0 以上 | 成功 | 可 | `setKeyToken()` が acquire ref に成功し、packet経路 が key slot を参照できる |
| `Unknown` | 台帳に存在しない token。未登録、refcount 0 到達による削除、refcount 0 の未使用 slot revoke 済みを含む | `UnknownToken` | 不可 | 削除済み token を復号可能として扱わない |
| `RegistryUnavailable` | 台帳 lock 失敗、内部状態破損、CAS bridge registry 不在などで解決不能 | `RegistryUnavailable` または AIDL `UNKNOWN_ERROR` 相当 | 不可 | 内部障害を復号成功にしない |


失効時は直ちに無効化し、新規および既存の解決処理を停止して鍵素材を使用不能にする。


## デスクランブル gate

### STD-B25デコード能力台帳

STD-B25デコード能力とSTD-B25 Part 1 §4.9への適合宣言を分離する。`開発規則.md` のproduct-level invariantに従い、Part 1 §4.9の受信機システム最小8鍵組容量は本製品全体として恒久的に適合対象外とし、同条項への適合を宣言しない。`StdB25DecodeCapability`、1鍵組の保証、実鍵組数または実PID数を根拠に、同条項適合、Part 1 CAS-R全体への適合、またはSTD-B25全面準拠と表現してはならない。

実装がSTD-B25で定める対象方式のTS payloadを実際に復号できる場合は、限定した事実を`StdB25DecodeCapability`として製品profileへ記録してよい。この能力は、対応するPart・方式・payload処理、物理tuner/backend復号経路ごとの実同時鍵組数、実同時PID数、pool共有単位、枯渇時の`UNAVAILABLE`を一体で定義する。値が未確定、または復号経路が利用不能の場合は能力を公開しない。AOSPの`DemuxCapabilities`には鍵組数またはPID数の欄がなく、`IDescrambler`は1 sessionを1 key slotへ関連付けて複数PIDを登録する契約までなので、frozen AIDLへ独自fieldを追加しない。鍵組数を外部へ表示する必要がある場合は、AIDL能力ではなく製品profileの設計メタデータとして扱う。

実行時は、同じ物理tuner/backend復号経路に属する共有`DescramblerCapacityPool`へprofileの実鍵組数と実PID数、pool共有単位を登録し、全sessionの合計使用量が実容量を超えないことをcapacity contractとして保証する。個別sessionのclaim / release順序、commit / rollback / cleanupは下表のcanonical mutation ownerだけを正本とし、本能力台帳では再定義しない。

| 容量次元 | capability / capacity契約 | caller-visibleな枯渇結果 | 内部mutation正本 |
|---|---|---|---|
| 能力公開 | 物理tuner/backend単位の実鍵組数、実PID数、pool共有単位を製品profileから確定する。未確定、0、または実体と不一致の値を能力として公開しない | 能力非公開 | `CapabilitySnapshot` / product profile契約 |
| descrambler object/session枠 | 公開したobject/session上限を超えて生成しない | `openDescrambler()`は`UNAVAILABLE` | root/child open契約 |
| pool session容量 | 同じ物理復号経路を共有するsession総数をpool容量内に制限する | 対応demuxへの初回`setDemuxSource()`でもsession容量枯渇なら`UNAVAILABLE` | `IDescrambler demux結合契約` |
| 鍵組容量 | 同じpoolで同時利用する鍵組数をprofileの実鍵組数以下にする | 新規鍵組を必要とする`setKeyToken(non-VOID)`で枯渇なら`UNAVAILABLE` | `DescramblerKeyTxn` |
| PID容量 | 同じpoolで同時利用するPID数をprofileの実PID数以下にする | `addPid()`で枯渇なら`UNAVAILABLE` | `DescramblerPidTxn` |
| cleanup中容量 | cleanup完了を確認できないsession / key / PID / pool帰属は再利用可能容量へ戻さない | 必要容量を確保できない新規要求は`UNAVAILABLE` | `DescramblerSessionCleanupTxn` / `ObjectCloseTxn` |

製品profileで公開demuxのいずれにもSTD-B25デコード能力を有効にしない構成では`openDescrambler()`を`UNAVAILABLE`とし、VTS製品設定へdescrambling flowを含めない。一部のdemux経路だけで能力を有効にする構成では、未結合objectの生成後、対象外demuxへの`setDemuxSource()`を`UNAVAILABLE`とする。能力を有効にする場合も、実鍵組数または実PID数を、本製品全体として恒久的に適合対象外であるPart 1 §4.9への適合、Part 1 CAS-R適合、またはSTD-B25全面準拠の宣言へ読み替えない。鍵素材はslot数だけを台帳化し、公開AIDLまたは診断へ出さない。

VTS/lab config の descrambling flow は、`VTS profile / capability 対応契約`と`開発規則.md`のrelease到達点を正とする。r51ではCAS HALがplaceholderで本番の実CAS tokenを成立させないため、VTS product profileで本番descrambling成功を表明するflowを宣言しない。r52の正式リリース到達点でCAS HAL / MediaCas / vendor key bridgeが実tokenを成立させ、対象demuxの`StdB25DecodeCapability`を有効化し、使用するVTS artifact / variant / input / CAS system / filter / PID / queue / memory等の`VtsEnvironmentProfile`入力と必要資源を起動前に確定・予約できるprofileでは、AOSP VTSのdescrambling flowを到達可能にする。descrambler能力を宣言したprofileからflowを隠して検査を回避してはならない。Tuner HAL は PMT/CAT/SDT/ECM/EMM 等の section payload delivery、`IDescrambler`、`setKeyToken()`、`addPid()` / `removePid()`、token lookup境界、未接続・bad token・expired token診断を契約対象とする。本番経路スクランブル解除成功のrelease scopeと、CA情報 / service metadataの意味解析、ECM/EMM filter開始方針、MediaCas/CAS bridge呼出し、実token取得、Tuner descramblerへの接続判断、未接続診断の上位制御は`開発規則.md`を正とする。Tuner HALのpacket単位デスクランブル中核は単体テスト内で既知鍵を登録して確認してよい。


## IDescrambler optionalSourceFilter 境界

AOSP意味論では、`IDescrambler.addPid(pid, optionalSourceFilter)` および `removePid(pid, optionalSourceFilter)` の `optionalSourceFilter == NULL` は demux input 全体に対する PID 登録 / 解除である。NULL経路は現行AOSP契約上の成功対象として扱う。non-null 経路は指定 filter output、すなわち upper stream を対象にした PID 登録 / 解除であり、source filter検証後に成功対象とする。

### 表D-1. IDescrambler PID 操作表

| 番号 | API | source filter | 条件 | AIDL戻り値 | 副作用 | 設計上の成立条件 |
|---:|---|---|---|---|---|---|
| DS-001 | `addPid(pid, NULL)` | なし | valid PID、descrambler非閉鎖、demux設定済み、PID未衝突 | 成功 | demux input 全体に対する PID として登録 | NULL filter は demux input を表す。source filter id / generation は持たない |
| DS-002 | `addPid(pid, filter)` | あり | filter が同一 demux、非閉鎖、generation 有効、pid valid | 成功 | source filter に紐づく PID として登録 | source filter id と generation を保存する |

同一サービス内の閉鎖済みオブジェクトには`INVALID_STATE`を返す。


| 番号 | API | source filter | 条件 | AIDL戻り値 | 副作用 | 設計上の成立条件 |
|---:|---|---|---|---|---|---|
| DS-004 | `addPid(pid, filter)` | あり | invalid PID | `INVALID_ARGUMENT` | なし | PID 範囲外を登録しない |
| DS-005 | `addPid(pid, filter)` | あり | descrambler 閉鎖済み、demux 未設定、別 active descrambler が同一 demux generation / PID を所有 | `INVALID_STATE` | なし | 状態衝突を引数不正として扱わない。key token 未設定は PID 登録拒否条件ではない |
| DS-006 | `removePid(pid, NULL)` | なし | demux input 全体に登録済みPID、または未登録PID | 成功 | demux input 全体に対する PID 登録を解除。未登録なら無処理 | NULL filter は demux input を表す。cleanup として冪等成功にする |
| DS-007 | `removePid(pid, filter)` | あり | 登録済み source-filter 紐づき PID | 成功 | 紐づく PID 登録を解除 | source filter id と generation が一致する登録だけ解除する |
| DS-008 | `removePid(pid, filter)` | あり | 未登録 PID | 成功 | なし | cleanup として冪等成功にする |
| DS-009 | `removePid(pid, filter)` | あり | invalid PID | `INVALID_ARGUMENT` | なし | PID 範囲外を解除対象にしない |
| DS-010 | `addPid()` / `removePid()` | あり/なし | unsupported `DemuxPid` variant | `UNAVAILABLE` | なし | product capability 未対応に限定する。NULL filterかどうかではなくPID variantで判定する |


`addPid(pid, source)`はPIDを置換キーとする。同じPID・同じsourceの再指定は冪等成功とし、同じPIDに異なるsourceが既に登録されている場合は、AOSP Framework契約どおり旧sourceを新sourceへ置換する。新sourceのdemux/generation/lifecycleをcommit前に検証し、検証またはprepareに失敗した場合は旧登録を維持する。置換中もPID claimの容量は1件として扱い、`removePid()`を先行させる空白期間を要求しない。


エラー写像:
- `INVALID_STATE`: descrambler 閉鎖済み、demux 未設定、demux generation 消失、再検査時 state 不整合、別 active descrambler による同一 demux / demux generation / PID 所有衝突。key token 未設定は `addPid()` / `removePid()` の `INVALID_STATE` 理由にしない。

閉鎖済みの入力元filterには`INVALID_STATE`を返す。


- `UNAVAILABLE`: unsupported `DemuxPid` variant、product capability 未対応に限定する。

## DVB backend の対応表

DVB backend は frontend index と同じ demux index / dvr index を使う。`adapterN/frontendM` は `adapterN/demuxM` と `adapterN/dvrM` に対応する。demux が別 frontend の TS を読む構成は advertise しない。source 選択 ioctl が失敗した場合は tune / scan / record を成功扱いにしない。

## 診断可観測性の固定

本番経路トークンの用語、リリース段階、TIS から `setKeyToken()` へ渡してよい値のスコープは `開発規則.md` を正とする。本節では、Tuner HAL が受け取ったトークンの検証、AIDL戻り値、診断、副作用だけを固定する。CAS bridgeからのtoken登録は標準MediaCas session ID bytesと内部key resourceの対応を成立させる論理契約に従う。具体helper名、内部登録API、debug出力方法は本契約で規範化しない。product defaultの`IDescrambler.setKeyToken()`に伴うkey table mutationのowner / entryは`../tuner_hal2/DESIGN_JA.md`の`DescramblerKeyTxn` / descrambler key table規範実装アンカーを正とする。

`IDescrambler.setKeyToken()` に到達する non-VOID トークンは、標準MediaCas経路では `MediaCas.Session.getSessionId()` と同一byte sequenceをTuner key tokenとして用い、CAS / vendor key bridgeがそのbytesと内部`DescramblerKeySlot`の対応をHAL key token tableへ登録したものを解決対象とする。TIS向けに別形式のvendor-private tokenを設けない。入力形式はTuner SDK `Descrambler.isValidKeyToken()` に合わせ、1 byte以上16 byte以下を有効なtoken形式とする。ただしAndroid 14系の `Tuner.VOID_KEYTOKEN` は1 byteトークン `[0x00]` としてcurrent key removal用に予約する。空トークン `[]` はVOIDトークンではなく、常に `INVALID_ARGUMENT` と内部診断 `BAD_TOKEN` に落とす。16 byteを超えるnon-VOIDトークンはregistry lookup前に `INVALID_ARGUMENT` / `BAD_TOKEN` とする。

`maleicacid-cas-desc-token-*`、`maleicacid-placeholder-desc-token*`、既存 TIS 側の `maleicacid-kari-token-*` は、設計文書上の診断名またはログ上のラベルであり、Tuner SDK API 経由で渡す実 トークン ではない。単体テスト、fake CAS、診断注入で同等のケースを表現する場合も、`setKeyToken()` に渡すtest用non-VOID byte arrayは1 byte以上16 byte以下（`[0x00]`を除く）のtest-only tokenをHAL key token tableへ事前登録したものとし、長い診断名はテストケース名、lookup tableの説明、診断dumpの表示名に限定する。

これらの診断 トークン origin を受け取った場合は、復号成功ではなく `CAS_BRIDGE_UNCONNECTED`、`BAD_TOKEN`、`EXPIRED_KEY_SLOT` など該当する診断へ落とす。

`IDescrambler.setKeyToken()` は、最初に `[0x00]` を `Tuner.VOID_KEYTOKEN` として処理し、registry lookup に流さず current key slot のみ解除する。PID 登録は維持する。次に空トークン `[]` と16 byteを超えるnon-VOIDトークンをregistry lookup前に拒否し、`INVALID_ARGUMENT` と内部診断 `BAD_TOKEN` に固定する。1 byte以上16 byte以下（`[0x00]`を除く）だが未登録のトークンとCAS bridge未接続トークンは通常トークンとしてregistry lookup後に区別して診断する。診断を通さない トークン 解決 API は 本番経路へ公開しない。

`IDescrambler.setKeyToken()` の失敗時は、現在の鍵スロット、現在のトークン、demux 紐付け、PID登録を変更しない。空 トークン、長さ超過、未登録、失効済み、台帳異常のどれで失敗しても、成功扱いにせず固定された AIDL 戻り値と診断だけを返す。PID 登録を消す操作は `removePid()` だけであり、`VOID_KEYTOKEN` と 鍵参照の解決失敗は PID 登録削除を伴わない。

デスクランブル診断は公開AIDL意味を変えない内部診断として保持する。product defaultの診断record / storeは`../tuner_hal2/CODE_CONVENTION.md`のfailure / rollback / cleanup規約およびquery / packet / diagnostic境界にあるtyped・bounded診断規約に従う。具体helper名、debug出力先、ファイル書き込み周期はproduct defaultの公開・論理契約として要求しない。

### 診断counter飽和契約

診断専用counterはlifetime ID、generation、token、startId等の再利用禁止識別子とは別の値域として扱う。診断counterは上限で飽和してよいが、上限到達時は`diagnostic_counter_saturated`をcounter種別と対象ownerを識別できる必須診断として記録する。

診断counterの飽和または診断記録のdropだけを理由に、filter / DVR / demux / frontendをruntime failure、cleanup failure、`Quarantined`へ遷移させず、本体data pathを停止しない。診断取得APIを除くbusiness APIの成功/失敗、AIDL戻り値、capability、資源寿命を変更してはならず、診断counterをこれらの判定入力に使用しない。

lifetime ID、generation、token、startId等の識別子発行は本契約の対象外であり、各canonical ownerが定めるchecked increment、wrap/reuse禁止、発行不能時の局所failure / quarantine契約をそのまま適用する。

### 失効 トークン 診断

`maleicacid-expired-desc-token-*` は診断名であり、`setKeyToken()` に渡す実 トークン ではない。本製品仕様は persistent expired state を持たないため、失効または revoke 済み token の `setKeyToken()` は unknown token として扱う。`EXPIRED_KEY_SLOT` は stale release / refcount underflow 検出用の診断名としてだけ使う。

`setKeyToken()` は、空トークン、16 byteを超えるnon-VOIDトークン、未登録トークン、CAS bridge未接続トークンを区別して診断カウンターに記録する。`[0x00]` は `Tuner.VOID_KEYTOKEN` として扱い、`BAD_TOKEN`、unknown トークン、CAS bridge 未接続には混ぜず、key 未設定状態でも 成功扱いの無処理 とする。空 トークン `[]` は registry lookup、current key slot 変更、PID 登録変更を行わない。

## B25 packet デスクランブル中核の範囲

本設計は、libaribb25 相当の B25 全体処理系を規定しない。本設計が Tuner HAL descrambler 中核として規定する範囲は、188 byte TS packet の payload に対する MULTI2 復号、odd/even key 選択、adaptation フィールドを変更しない payload offset 判定、復号成功時の scrambling_control 正規化、復号失敗時の録画向け scrambled pass-through 診断である。

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


ECM / EMM 処理、カード I/O、CAS 権利判定、CW 取得、不透明 トークン 発行、B25 system key / CBC 初期値 / data key を CAS 側から安全に供給する経路は CAS HAL または CAS bridge の責務であり、r52内部鍵資源の詳細は`../future_work/r52/b25_key_slot_registry_contract.md`を正とする。CAS / TIS / Tuner HAL のリリース段階ごとの統合スコープは `開発規則.md` を正とする。本節が規定するのはTuner HALのpacket単位デスクランブル中核と診断境界であり、libaribb25相当のTS→TS B25処理系全体の完成条件または作業完了判定を定義しない。

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


DVB frontend は sysfs driver basename が `earth-pt1` の場合だけ `EarthPt1FixedLnb` として採用する。frontend name に `tc90522` が含まれるだけでは採用しない。

`EarthPt1FixedLnb` は `NONE`、`11V`、`15V` だけを成功にする。`13V`、`18V`、tone、DiSEqC、satellite position switching は成功扱いしない。


## 恒久仕様

### Filter / DVR 開始 commit 境界


AIDLの公開面は、callbackに依存する操作だけがcallback状態の影響を受けるようにする。


### PES 解析境界

record index は、PES と raw elementary stream を区別する。共有 PES parser が PES 形式として拒否した入力を、元 payload 全体の raw elementary stream として再走査してはならない。raw elementary stream として扱うのは、PES stream id として解釈しない入力だけとする。

### packet origin

source filter由来のTS packetはfrontend由来のTS packetと同じpacket pipelineを通る。ただしorigin namespaceはfrontendとsource filterで分離し、`PacketPipeline`のper-origin continuity stateと各per-filter parser ownerのcarry/tracker stateを別origin間で共有・相互resetしない。stream/source boundaryのreset要求だけを`StreamBoundaryTxn`のtyped dispatchとして受ける。

### scan / tune worker terminal mapping

scan / tune workerのgeneric join failure、retry、reaper handoff、worker slot / lease保持は0-S-3Bの`WorkerRuntime`を唯一の正本とする。本節では分類済みterminal resultをscan / tune固有状態と公開結果へ写像する責務だけを持ち、generic停止失敗state machineを再定義しない。

### AV shared backing

AV shared backing は、検証が成功するまで旧 backing を保持する。設定変更の後段失敗で旧 backing、公開済み handle、stream type を破棄してはならない。release、flush、clear は active/free map を中間不整合のまま公開してはならない。

### test と release API の境界

release AIDL経路からテスト専用入口へ到達してはならず、テスト専用入口は製品runtimeのcapability・状態・戻り値を変更しない。この実装方法は`../GLOBAL_CODE_CONVENTION.md`の「テスト専用入口の本番隔離」を正とし、runtime flagだけの隔離を本番経路からの除外根拠にしない。

### AOSP / AIDL / VTS 系

| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-AOSP-1 | `setDataSource(sourceFilter)` 成功 | 同一demuxのTS raw sourceから、公開済みTS `TS` / `SECTION` / `PES` / `AUDIO` / `VIDEO` / `PCR` / `RECORD` sinkへの接続を、通常のlifecycle・owner・cycle・settings整合条件を満たす場合に成功させる |
| T-AOSP-2 | `setDataSource(nullptr)` | demux input 復帰として成功 |
| T-AOSP-3 | `setDataSource(nullptr)` 後の再start/data出力 | demux inputからの再start/data出力成功 |
| T-AOSP-4 | source filter owner demux不一致 | `INVALID_ARGUMENT` |
| T-AOSP-5 | source filter closed/failed | `INVALID_STATE` |
| T-AOSP-6 | unsupported source/sink semantics | 加工済みSection/PES/AV/Record outputのTS packet source化など、公開capabilityを持たない接続は`UNAVAILABLE` |
| T-AOSP-7 | `addPid(pid, nullptr)` | AOSP意味論確認。demux input 全体へのPID登録として成功 |
| T-AOSP-8 | `removePid(pid, nullptr)` | AOSP意味論確認。demux input 全体へのPID解除として成功 |
| T-AOSP-9 | `addPid(pid, sourceFilter)` 成功 | upper stream識別ありPID登録 |
| T-AOSP-9a | 同じPIDへ異なる`sourceFilter`で再`addPid()` | 旧sourceを新sourceへ原子的に置換し、先行`removePid()`を要求しない。新source検証失敗では旧登録を維持 |
| T-AOSP-10 | `removePid(pid, sourceFilter)` 成功 | upper stream識別ありPID解除 |
| T-AOSP-11 | optionalSourceFilter owner demux不一致 | `INVALID_ARGUMENT` |
| T-AOSP-12 | optionalSourceFilter closed/failed | `INVALID_STATE` |
| T-AOSP-13 | `linkCaps` main type matrix | 広告したmain type pairはVTS生成のUNDEFINED subtype接続も成功 |
| T-AOSP-14 | `linkCaps`非宣言main type接続 | `UNAVAILABLE` |
| T-AOSP-15 | TS main type `UNDEFINED` subtype source filter | linkCapsでTS→TSを広告する場合は接続成功 |
| T-AOSP-16 | `getAvSharedHandle()` 成功 | fd付きNativeHandleとsize取得 |
| T-AOSP-17 | `releaseAvHandle(fd付きhandle, 0)` 成功 | VTS互換shared handle release |
| T-AOSP-18 | `releaseAvHandle(empty, 0)` | fdなし通知経路 |
| T-AOSP-19 | `releaseAvHandle(empty, activeAvDataId)` | MediaEvent slot release |
| T-AOSP-20 | `releaseAvHandle(event-local fd handle, matching activeAvDataId)` | 成功。foreign/mismatchは`INVALID_ARGUMENT` |
| T-AOSP-21 | `releaseAvHandle(any, negativeAvDataId)` | `INVALID_ARGUMENT` |
| T-AOSP-22 | `getAvSharedHandle()` 複数回取得 + release | fd duplicate寿命確認 |
| T-AOSP-23 | `configureMonitorEvent(0)` | 成功。未配送monitor stateだけをresetし、通常event、callback、FMQ、parser状態を維持 |
| T-AOSP-24 | `configureMonitorEvent(nonzero)` | `UNAVAILABLE`。本製品のTS-only `ProductProfile`はmonitor eventを対応能力として採用せず、monitor state、worker、queueを生成しない |
| T-AOSP-26 | AV `isPassthrough=false` | shared memory AV経路成功 |
| T-AOSP-27 | AV `isPassthrough=true` | `UNAVAILABLE` |
| T-AOSP-28a | `openDemux(out demuxId)` | objectと要素数1のID配列を同一成功応答で取得し、失敗時はどちらも公開されない |
| T-AOSP-28b | `openDemuxById(id)` | 指定IDのobjectだけを取得し、別のout IDを生成しない |
| T-AOSP-28c | `openDescrambler()` → `setDemuxSource(demuxId)` | 生成時は未結合、source設定時にdemux generationと共有poolへ一回だけ原子的に結合 |
| T-AOSP-28d | 結合済みdescramblerの再`setDemuxSource()` | 同じIDを含めて`INVALID_STATE`、既存結合を維持 |
| T-AOSP-28e | TsAudio + Video tag、TsVideo + Audio tagの`configureAvStreamType()` | `INVALID_ARGUMENT`、hintと全状態を維持 |
| T-AOSP-28f | `setDemuxSource(invalid_or_unavailable)`後の再`setDemuxSource()` | 初回は原因別エラー、二回目は`INVALID_STATE`。closeして新objectをopenした場合だけ新たな初回呼出しが可能 |
| T-AOSP-28g | `getDemuxIds()` / 全`getDemuxInfo()` / `getDemuxCaps()` | ID数が`numDemux`と一致し、全`filterTypes`のORが`filterCaps`と完全一致 |
| T-AOSP-28h | `IFrontend.setCallback(non-NULL → non-NULL → NULL)` | 各呼出しが成功し、置換後の新規eventは新callbackだけへ配送。新callback準備失敗では旧callbackを維持 |
| T-AOSP-29 | `getFrontendStatusReadiness()` 要求順・同長 | AIDL配列契約 |
| T-AOSP-30a | 未公開の既知値または将来のstatus数値を含む`getStatus()` | 対応済み要素だけを要求順で返し、非対応要素を無視して成功 |
| T-AOSP-30b | `getFrontendStatusReadiness()` unsupported status type | 要求順・同長で要素ごとにUNSUPPORTED |
| T-AOSP-31 | `tune()` 中の再`tune()` | 旧tune停止、新tune開始 |
| T-AOSP-32a | `scan(K) -> LOCKED -> scan(K)` | 2回目はbackend再探索なしに`END`を正確に1回配送し、2回目の`LOCKED`を配送しない |
| T-AOSP-32b | active scan中に異なるrequestで`scan(K2)` | 新scanへ移り、旧scan由来の後続結果を新scan結果として公開しない |
| T-AOSP-33 | `stopTune()` | tune停止、attached demuxへdata停止 |
| T-AOSP-34 | `stopScan()` | scan停止 |
| T-AOSP-35 | active scan中の`stopTune()` | 成功。scan generationとbackend scanを停止せず、scan callbackを継続し、frontendは`Scanning`のまま。tune世代は存在しないためbackend tune-stopを呼ばず、attached demuxのstream boundaryも変更しない |
| T-AOSP-36 | DVR playback watermark | T-AOSP-50のPlayback watermark判定と一致 |
| T-AOSP-37 | DVR record watermark | record callback基準 |
| T-AOSP-38 | `FilterDelayHint` timeのみ | time条件 |
| T-AOSP-39 | `FilterDelayHint` dataのみ | data条件 |
| T-AOSP-40 | `FilterDelayHint` time+data | OR条件 |
| T-AOSP-44 | `FrontendInfo` scalar境界とtune validation | min/max frequency、symbol rate、acquire rangeは同一`CapabilitySnapshot`を正本とする。固定離散raster backendのfrequencyはscalar envelopeに加えて同snapshotのraster profileを適用し、Android Hz要求を隣接center間の半間隔以内で一意なnearest centerへ正規化する。BS/CS間gap、range外、曖昧化できない値は副作用前に拒否し、CTS個別値の特例を置かない |
| T-AOSP-45 | DVBの検証済み物理group key、同一streamのvariant、共有資源、独立stream、topology不明 | 同一物理streamのvariantと同時利用不可資源を共有するtupleは同じ`exclusiveGroupId`、同時利用可能性を検証した独立streamだけ別group、別backend namespaceは衝突せず、topology不明候補は公開しない |
| T-AOSP-46 | ISDB-T segment capabilityとlayer `numOfSegment` | `0`は未指定、`0xFF`はCTS互換AUTO、`1..13`は明示値として分離する。さらに`isSegmentAuto=true`→`0xFF`、`false && isFullSegment=true`→`13`、`false && isFullSegment=false`→`1`のCTS分岐ごとに、公開capability pairが対応入力を必ず実現できる閉包条件を満たすこと。成立しないcandidateはfrontendとしてexportしない |
| T-AOSP-47 | ISDB-T V2 `inversion` / `serviceAreaId` / `partialReceptionFlag` / `numOfSegment` | 成功・`UNAVAILABLE`・`INVALID_ARGUMENT`をフィールド別に固定する。`partialReceptionFlag`明示値では同期`tune()`受付と非同期lock成立を分離し、px4ではfresh TMCC readback一致時だけ`LOCKED`、不一致は`NO_SIGNAL`、未確定・I/O失敗・旧generationでは`LOCKED`を生成しない。earth_pt1 / TC90522は既知readback blocker未解決中にsilent ignore-successしない |
| T-AOSP-51 | ISDB-S SDK default selector未指定 | `STREAM_ID + INVALID_STREAM_ID(0xFFFF)`を明示TSID検証より先に`Unspecified`へ正規化し、px4 BSは互換slot 0、px4 CS110はfixed slot 0、Linux DVB / earth_pt1は`NO_STREAM_ID_FILTER`へ写像する。通常のBS TIS経路では明示absolute TSIDを維持する |
| T-AOSP-48 | ISDB-S `rolloff` | 未指定、既知未対応、malformedを分類し、未適用値を成功させない |
| T-AOSP-49 | RECORD index settings/event | request mask/typeを無損失検証し、event mask、`pts`、`firstMbInSlice`をcurrent parser/delivery fenceに一致させる。`byteNumber`は当該indexが位置するcommitted Record Filter output上の0起点offsetとする。最初のoutputは0から始まり、Record DVR output commit成功時だけ実commit byte数分`record_output_byte_offset`を進め、commit失敗では不変とする。flush/reconfigure/source/stream boundary、Record DVR attach/detachで維持し、新Filter output lifetimeだけ0へ戻す |
| T-AOSP-50 | DVR `statusMask` / threshold / `dataFormat` / `packetSize` | Playbackは同一FMQ snapshotの`availableToRead` / `availableToWrite`を用い、`SPACE_FULL` → `SPACE_ALMOST_FULL` → `SPACE_ALMOST_EMPTY` → `SPACE_EMPTY` → 前status維持の順で判定する。threshold等値ではALMOSTへ新規遷移しない。Recordはunconsumed bytesで`q > highThreshold`を`HIGH_WATER`、`q < lowThreshold`を`LOW_WATER`として判定し、threshold等値では新規遷移しない。`low == high == T` / `q == T` でも直前statusを維持する。semantic status確定後に`statusMask`を適用し、maskされたstatusを別statusへremapしない。無効・未対応設定は状態不変で拒否 |
| T-AOSP-52 | `endFrequency`の操作別意味 | `tune()`とblind以外のscanでは差分を理由に拒否せずfingerprint/backend要求へ含めず、blind scanだけ`UNAVAILABLE`にする |
| T-AOSP-53 | 同一settingsでのFilter reconfigure後startId | 一度start済みFilterをstopし、同一settingsで`configure()`成功後にrestartした場合も、最初に新しいstartId-only callbackを正確に1回配送してから通常eventを配送する |
| T-AOSP-54 | TS live `DemuxFilterMediaEvent`公開field | `streamId`、PTS/DTS presence/value、`mpuSequenceNumber`、private-data、`extraMetaData`、`scIndexMask`を同一media outputの入力由来または定義済みdefaultと一致させ、別PES/generationを混成しない |
| T-AOSP-55 | DVR `setStatusCheckIntervalHint()` | playback / record双方で受理したhintを以後のdata/status evaluation cadence決定に使用し、保存だけのno-op、Binder thread sleep、queue/lifecycle変更にしない |
| T-AOSP-56 | Filter lifecycle closure | non-AV未設定`start()`/`flush()`は`INVALID_STATE`、未開始`stop()`と重複`start()`は冪等成功、開始中`configure()`は`INVALID_STATE`。`stop()`はcommit済みoutput/client資源を維持し、`flush()`だけが対象の未消費FMQ・未配送event・partial stateを破棄する。開始済みFilterのflushは開始状態を維持し、一度start済みFilterのstop後同一settings再configureはT-AOSP-53の新`startId`契約を満たす |
| T-AOSP-57 | DVR lifecycle closure | 未設定`start()`/`flush()`は`INVALID_STATE`、未開始`stop()`と重複`start()`は冪等成功、開始中`configure()`は`INVALID_STATE`。Record開始中flushは`INVALID_STATE`、Playback開始中flushは成功して開始状態を維持する。stopではRecord未消費outputおよびPlayback未読input/processing residualを維持し、configureでqueue identity/relationを置換しない |
| T-AOSP-58 | Record DVR Filter relation lifecycle | configure前/停止中/開始中のRecord DVRでvalid attach/detachを成功させ、duplicate attach / absent detachは状態不変成功。Playback DVR、foreign owner、別demux、record非対応Filterは`INVALID_ARGUMENT`、閉鎖済みFilter引数は`INVALID_STATE`。route mutationは`RecordDvrFilterRelationTxn`だけを通る |
| T-AOSP-59 | `linkCaps` concrete matrix | TS-only profileでは`[0x01, 0, 0, 0, 0]`を返し、TS->TSのUNDEFINED subtype linkageを成功させ、非広告pairを成功させない |
| T-AOSP-60 | `numBytesInSectionFilter` / SectionBits boundary | capabilityはmask最大幅16。filter/mask/modeは独立shapeを保持し、maskだけ16 bytes超を`INVALID_ARGUMENT`で拒否する。active mask bitが参照するbyte位置ではfilter/mode双方の存在を必須とし、missing valueを補完しない。mask外のfilter/mode末尾はmatchingに使わず、短いruntime sectionはnon-matchとする |
| T-AOSP-61 | `DvrSettings` atomic configure | statusMask / low/high threshold / dataFormat / packetSizeを同一snapshotでvalidateし、1 field拒否時にsettings・queue位置・identity・relationを変更しない |
| T-AOSP-62 | Filter / Record DVR FMQ full | 既存未消費dataをevictせず新規論理単位を部分commitしない。Filterは`DemuxFilterStatus::OVERFLOW`、Recordの188-byte commit拒否は`RecordStatus::OVERFLOW`へ一意に写像する。Recordは`OVERFLOW` bit選択時だけ通知し、mask時にHIGH/LOW/DATA_READYへ代替しない。commit失敗ではqueue位置と`record_output_byte_offset`不変 |
| T-AOSP-63 | section table length / `pointer_field` | PAT/CAT/PMT等1021区分、EIT等4093区分、TDT=5、TOT=1021を固定し、PUSI pointerで旧partial未完了を新sectionへ連結しない |
| T-AOSP-64 | `TsInputOrigin` namespace | Frontend / PlaybackDvr(dvr_id, queue_identity, queue_epoch) / SourceFilter(filter_id, generation)でcontinuity・partial stateを分離し、origin boundary前後を連結しない |
| T-AOSP-65 | `configureAvStreamType()` 全域 | matching Audio/Video tagは非開始状態でAndroid 14 AIDL定義済みstream typeの全enum member（Videoの`RESERVED`、Audio/Videoの`UNDEFINED`を含む）をhintとして成功commitし、private codec capability gateを設けない。再設定はhintだけを置換し、同値はno-op成功、tag不一致/enum未定義numeric値は`INVALID_ARGUMENT`、valid Started要求は`INVALID_STATE`、non-AVは`UNAVAILABLE` |
| T-AOSP-66 | AV secure-memory input | non-started AVで`isSecureMemory=false && isPassthrough=false`だけを通常能力判定へ進め、`isSecureMemory=true`または`isPassthrough=true`は`UNAVAILABLE`かつ旧settings/backing/hint/relationを維持。Startedはlifecycle契約を優先 |
| T-AOSP-67 | scan callback union | candidate成立は`LOCKED + isLocked=true`、正常終了は`END + isEnd=true`をcurrent generationだけから生成する。その他のAIDL定義済みmessageはcurrent generationのauthoritativeな実値を一意に構成できる場合に対応tagで生成可能とし、未取得/推測値、type/tag/value不一致、false variant、stale generation、重複ENDを公開しない。blind scanを将来広告する場合はVTSが要求する`FREQUENCY`等のclosureも同時に満たす |
| T-AOSP-68 | ISDB-T `layerSettings[]` | AOSPはvector長を制限せず全要素を保持することを確認する。空配列はlayer制約なしで成功し、1〜3要素は入力順を保持する。通常13-segment ISDB-Tの最大3階層は現行省令第21条を根拠とし、4要素以上は存在しない4番目以降の物理階層要求として`INVALID_ARGUMENT`。全要素を既存field契約で原子的に検証し、unsupported explicit valueを無視成功しない |
| T-AOSP-69 | normal Filter status set | Filter FMQをpayload data planeとして使うTS raw / SECTION / PESだけを`DATA_READY` / `LOW_WATER` / `HIGH_WATER` / `OVERFLOW`の生成対象とする。LOW/HIGHはFMQ容量の`ceil(25%)` / `ceil(75%)`を既定thresholdとし、`available < low` / `available > high`だけで遷移し、等値では新規遷移しない。`NO_DATA`はAIDL未定義の独自inactivity timeoutを設けず、`available == 0`だけでは生成しない。Filter FMQを所有するだけのRECORDとDVR watermark contractには波及させない |
| T-AOSP-70 | TS RECORD settings/event closure | `tsIndexMask`、`scIndexType`、union tag/maskを原子的に検証し、known unsupportedは`UNAVAILABLE`、malformed/unknownは`INVALID_ARGUMENT`。eventのPID/mask/PTS/firstMb/byteNumberを同一generationの実検出・成功commitに一致させる |
| T-AOSP-71 | `openFilter()` acceptance matrix | TSのUNDEFINED/SECTION/PES/TS/AUDIO/VIDEO/PCR/RECORDは表の条件で開き、TEMIおよびMMTP/IP/TLV/ALPは`UNAVAILABLE`。wrong main/subtype/unionと未知値を`INVALID_ARGUMENT`とし、拒否時にobject/queue/leaseを生成しない |
| T-AOSP-72 | RECORD Filter FMQ ownership / data-plane split | RECORD Filterはopen時にFilter FMQ resourceを所有し`getQueueDesc()`で有効descriptorを返す。一方、RECORD TS投入でFilter FMQ payloadは増加せず、attach済みRecord DVR FMQへ正確に1回だけ188-byte packetをcommitする。`record_output_byte_offset` / `byteNumber`はRecord DVR output commitだけから進む |
| T-AOSP-73 | RECORD VTS canonical / probe variant | canonical RECORD profileは`useFMQ=false`、`record-filter-fmq` probe variantだけ`useFMQ=true`とする。probe variantでも`useFMQ`はFilter descriptor取得coverageだけを変更し、RECORD payload routeはRecord DVR FMQのまま維持する |

`close()` 以外の公開メソッドでは、`LogicalClosed`、`InvalidArgument`、`WrongLifecycle`、`ResourceUnavailable`、`BackendFailure`、`Success` の順で判定を優先する。`close()` 自体の結果はこの共通優先順位で決めず、0-S-4の公開close契約に従う。遅延して呼ばれる `IFilter.releaseAvHandle()` はAV解放台帳に従う独立操作であり、閉鎖後の共通メソッドとして扱わない。


| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-AOSP-42 | VTS XML/profile full run | `VtsHalTvTunerTargetTest` |
| T-AOSP-43 | VTS config audit | monitor / descrambler / AV shared / linkCaps / passthrough整合 |

### ARIB TS packet 系

| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-TS-1 | sync byte不正 | reject |
| T-TS-2 | 187/189 byte | reject |
| T-TS-3 | TEI set packet | section/PES/AV assemblyへ入れない |

188バイトで構造上完全なTSパケットに `TEI=1` が設定されている場合、TS生データ出力とTS記録出力には入力順のまま保持する。HALはTEIカウンターを飽和加算する。記録の`byteNumber`と`record_output_byte_offset`は前段のRecord Filter output offset契約に従い、TEI packetもRecord DVRへ成功commitした実188 byteとしてoffsetを進める。


| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-TS-5 | adaptation_field_control reserved | reject |
| T-TS-6 | adaptation length overflow | reject |
| T-TS-7 | PCR flagありPCR不足 | reject |
| T-TS-8 | OPCR flagありOPCR不足 | reject |
| T-TS-9 | splicing/private/extension長不足 | reject |
| T-TS-10 | 同一CC・188バイト全一致 | raw/recordへ保持し、assemblyへは入れない |
| T-TS-10a | 同一CC・packet不一致 | raw/recordへ保持し、境界前の意味解析結果を後続へ連結しない。continuityは`PacketPipeline`、parser stateは各per-filter parser ownerを更新し、`StreamBoundaryTxn`はtyped reset dispatchだけを行う |
| T-TS-11 | discontinuity_indicator | 境界前後の意味解析結果を連結しない。continuityは`PacketPipeline`、parser stateは各per-filter parser ownerを更新し、`StreamBoundaryTxn`はtyped reset dispatchだけを行う |
| T-TS-12 | adaptation-only packet | continuityを進めない |
| T-TS-13 | TS resync末尾完全188byte | 次入力sync待ちせず返す |
| T-TS-14 | false `0x47` resync | 誤同期しない |
| T-TS-15 | scrambling_control set + keyなし | record pass-through / assembly drop |

### ARIB section 系

| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-SEC-1 | section_length最小未満 | reject |
| T-SEC-2 | syntaxあり最小長不足 | reject |
| T-SEC-3 | reserved bit不正 | reject |
| T-SEC-4 | CRC good | accept |
| T-SEC-5 | `isCheckCrc=true` + CRC bad | reject / overflowに写像しない |
| T-SEC-5a | `isCheckCrc=false` + CRC bad + 構文正常 | CRCを配送条件にせず、rawはFMQ commit後に`IFilterCallback.onFilterStatus(DATA_READY)`を配送する。EventFlagは追加wakeだけに使い、non-rawは型付きevent規則に従う |
| T-SEC-6 | raw + `isCheckCrc=false` + reserved bit不正 | 生バイト列を配送し、型付きeventは生成しない |
| T-SEC-6a | non-raw + reserved bit不正 | reject |
| T-SEC-7 | EIT `section_length == 4093` | accept |
| T-SEC-8 | EIT `section_length == 4094` | reject |
| T-SEC-13 | `SectionBits repeat=false` | 最初の一致sectionを1件配送してone-shot停止 |










| T-SEC-14 | 明示versionの`TableInfo repeat=false`、sectionが順不同 | 最初に選択した`TableInstanceKey`の各sectionを初出順に1回配送し、`0..last_section_number`の配送済みbitが全て立った後に停止 |
| T-SEC-14a | `version=-1`の`TableInfo repeat=false` | target選択まではwildcardを維持し、先着sectionのactual versionをinstance identityとして固定。設定値は書き換えない |
| T-SEC-14b | 同一table ID/versionで複数extension/current-nextが並行 | 最初の構造上完全なmatching sectionが属するinstanceだけをtargetとし、他instanceの同じsection番号を混成・配送しない |
| T-SEC-14c | wildcard target完成前に別actual version到着 | targetを切り替えず、先着instanceの未配送sectionを待つ。別versionを配送しない |
| T-SEC-14d | target sectionの`last_section_number`不一致 | 不一致sectionをmalformedとして破棄し、bitmapまたは停止判定を進めない |
| T-SEC-14e | short syntax + wildcard + `repeat=false` | 最初の完全sectionを1 section tableとして1回配送後停止 |
| T-SEC-14f | 最大`last_section_number=255` | 256-bit（32 byte）bitmapと固定metadataだけで追跡し、各section payloadは逐次配送してtable全体を保持しない |
| T-SEC-14g | target未完成、`stop()`／`flush()`／再設定／stream boundary | `stop()`ではtarget metadata・配送済みbitmap・partial sectionを保持して再`start()`で継続する。`flush()`／再設定／stream boundaryでは境界前targetを境界後へ継承・完了扱いせず、0-S-3Bのowner境界に従い各per-filter ownerが対象stateを更新する |
| T-SEC-14h | 各section配送時のFMQ overflow/backpressure | AOSP `DemuxFilterStatus::OVERFLOW` の契約どおり、filter bufferがfullでcommitできない新規sectionは内部第二queueへ複製・再試行せず破棄して`OVERFLOW`を通知する。FMQ payload/event commitが成功するまでは配送済みbitを立てないため、`repeat=false` targetも未達のまま維持し、放送入力として後続に再到来した同一sectionは通常のmatching対象として再び受理できる |
| T-SEC-14i | 複数extension/versionが並行する`TableInfo repeat=true` | table id/version条件に一致する全instanceのsectionを継続配送する |
| T-SEC-15 | `repeat=true` version更新 | 継続監視 |




### PES / record index 系

| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-PES-1 | PES start code不正 | malformed |
| T-PES-2 | optional header marker不正 | malformed |
| T-PES-3 | `PTS_DTS_flags == 0b01` | malformed |
| T-PES-4 | PTS marker bit不正 | malformed |
| T-PES-5 | DTS marker bit不正 | malformed |
| T-PES-6 | `PES_packet_length` とheader長矛盾 | malformed |
| T-PES-7 | bounded PES complete | delivery |
| T-PES-8 | bounded PESが宣言長到達前に次PUSI | 未完PESを破棄し、次PESから再開 |
| T-PES-9 | bounded PESの`stop()`／`flush()`／`close()` | `stop()`は未完成PESと対応claimを保持して再`start()`で継続する。`flush()`／`close()`は未完成を完成扱いせず破棄してclaimを返却する |
| T-PES-10 | 同時PES filterが各`MAX_PES_BUFFER_BYTES`までclaim可能 | `pesRuntimeBudgetBytes`内で公開数全filterを受理 |
| T-PES-11 | PES header TS packet境界分割 | 正しく組立 |
| T-PES-12 | PTS field TS packet境界分割 | PTS抽出 |
| T-PES-13 | start code `00 00 01` TS packet境界分割 | record index検出 |
| T-PES-14 | malformed PES後の復帰 | 次PUSIから正常復帰 |
| T-PES-15 | 映像以外の`stream_id`で`PES_packet_length=0` | malformedとして破棄 |
| T-PES-16 | `streamId=0xBD`以外の有効な明示stream ID | configure成功し、指定IDだけを照合・配送 |
| T-PES-17 | wildcard `streamId=0xFFFF` | configure成功し、有効な全stream IDを配送対象にする |
| T-PES-18 | 映像`stream_id 0xE0..0xEF`の長さ0 PES | 次PUSIで完成し、`MAX_PES_BUFFER_BYTES`超過時だけoversize破棄 |
| T-PES-19 | ordinary PESの`PTS_DTS_flags=00` | timestampなしの有効PESとして配送 |
| T-PES-20 | ordinary optional headerを持たないspecial stream id | 通常header検証を適用せず、special syntaxの完全長を配送 |
| T-PES-21 | PES event生成 | `streamId`、`dataLength`、`mpuSequenceNumber`だけを設定し、PTS有無を捏造しない |

PES filterは、外形検証の後に`stream_id`で通常optional-header構文とspecial syntaxを分岐する。明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理する。受信するPES packetの`stream_id`は8 bit値として構文分岐し、ヘッダーが複数TSパケットに分割される場合にも対応する。通常構文では`PTS_DTS_flags=00`をtimestampなしの有効PESとして受理し、PTSまたはPTS/DTSが存在する場合だけflag、marker、`header_data_length`とtimestamp fieldを内部検証する。special syntaxへ通常optional-header検証を適用しない。完全PES bytesを通常FMQへ書き込み、`DemuxFilterPesEvent`ではAIDL公開フィールドの`streamId`、`dataLength`、`mpuSequenceNumber`だけを通知する。PES eventへPTS有無またはPTS値を追加しない。Media eventのPTS公開契約とは分離する。宣言長ありPESは宣言長で完成し、映像`stream_id 0xE0..0xEF`の長さ0 PESは同一PIDの次PUSIで完成する。その他のstream IDで長さ0を受信した場合はruntime malformedとして破棄する。


### MULTI2 / B25 descrambler 系

| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-B25-1 | MULTI2既知ベクトル | 復号中核確認 |
| T-B25-2 | payload-only復号 | TS header/adaptation/PCR/CC非破壊 |
| T-B25-3 | even key `10` | even key選択 |
| T-B25-4 | odd key `11` | odd key選択 |
| T-B25-5 | key未設定 | record pass-through + 診断 |
| T-B25-6 | bad token | `INVALID_ARGUMENT` / 診断 |
| T-B25-8 | 復号成功 | scrambling_control clear |

デスクランブラーとTS経路の失敗は、本書の「失敗影響範囲」に従って扱う。影響経路を隔離するのは、データ枠を管理する基盤が破損した場合に限る。不正TSはパケット単位で破棄し、TEIと連続性異常は各経路の規則に従う。構造上有効だがスクランブルが残るパケットはTS生データ経路と記録経路に残してよいが、復号済みの意味イベントを生成してはならない。ARIB STD-B25 6.7-E1 第1部の2.2.2.4、2.2.2.10〜2.2.2.11、3.1.5〜3.1.7、3.2.3〜3.2.4、4.3.3.3の表4-11〜4-14、4.8を精読基準とする。これらの条項から、TSペイロードをパケット単位でスクランブルすること、受信側でECMとEMMをCAモジュールへ渡すこと、Ksを受信側へ返すこと、スクランブル状態を検出することを、限定したSTD-B25デコード能力の設計条件とする。`開発規則.md` のproduct-level invariantどおり、Part 1 §4.9の受信機システム最小8鍵組容量は本製品全体として恒久的に適合対象外であり、実鍵組数と実PID数は製品profileの事実としてSTD-B25デコード能力台帳で予約・受付・解放を強制する。ECM、EMM、KsをTuner HALの公開面へ出さない境界は、AOSPの公開面と情報露出を最小化する設計から定めるものであり、STD-B25の文言そのものとは主張しない。HAL内部の隔離方法とエラー対応は、AOSP契約に基づく内部設計とする。


| 番号 | 確認観点 | 目的 |
|---:|---|---|
| T-B25-10 | ECM/EMM/card I/O不在 | Tuner HALへ持ち込まない |


## 対応能力ごとの設計正本

- 機器の事実は`DeviceProbeCapability`で確定する。frontendは公開API全体が成立するものだけを公開する。LNBは検出成功、endpoint/lease条件、operation/valueごとの実処理証跡から公開可否を決め、`aidl_baseline_eligible`はAndroid 14 CTS baseline適合分類としてだけ保持する。px4/earth_pt1は電圧制御の証跡があるため該当operation/valueを公開し、tone、position、DiSEqC等の有効だが未対応の要求は副作用なしの`UNAVAILABLE`とする。
- demux、filter、DVRの個数は本書「サービスオブジェクトの上限」で定め、同じ使用権台帳で強制する。
- AVの転送、割り当て、解放は、本書「AV割り当て」、0-S-2のAV active token台帳、「AV転送方式とクライアント側の存続期間」で定める。共有領域方式は最適化手段とし、要求サイズどおりのイベント固有ファイル記述子方式を正式な代替経路とする。`dataId=0`のhandle lease終了だけはboundedなclient lease stateにより冪等化し、正の`avDataId`はactive token台帳に存在する場合だけ解放を成功させる。
- ワーカーとLNBの停止・後片付けは、本書「ワーカー終了契約」と「LNB機器の資源規則」で定める。選局成否を固定時間で覆す専用timing profileや、公開経路で上限なく `join` を待つ処理を設けない。
- パケット異常と基盤異常の影響範囲は、本書「失敗影響範囲」で定める。不正TS、TEI、連続性異常を基盤隔離へ昇格させない。
- frontendで公開・受理する値は、本書「フロントエンド設定の反映表」で定める。ARIB B31の値域根拠は本書「フロントエンドの対応能力と状態」、現行日本語版との差分管理は本書「ARIB規範本文との静的照合」を正とする。
- 個別の対応能力で失敗した場合は、その能力または要求だけを抑止・拒否する。無関係な `ITuner` の公開を妨げない。


## 対応能力・キュー・ARIB境界

- Filter / SharedFilter の producer drain は 0-S-3B の `FilterProducerDrainGate`、DVR の queue epoch / transaction token は `QueueEpochProtocol`、Filter / DVR `flush()` の共通 cleanup orchestration は `QueueCleanupUseCase` を唯一の正本とする。本節では対象 domain、公開結果、資源要求だけを定め、内部 state、permit / token、phase、commit / rollback を再定義しない。
- demux、型別filter、DVRの個数とbyte予算は、frontend/backend/電源、demux base、main type別filter/FMQ、PES、AV、playback/record DVR、worker/callback/reaper/cleanup共有枠の`CapabilityClosure`ごとに原子的に検証・予約する。各閉包の失敗は、その閉包を必要とする能力だけを非公開にし、依存しないfrontend、filter種別、DVR種別へ波及させない。選択済み閉包を合成した後、query/openの同一性、`numDemux`、`filterCaps`、用途別個数、全byte台帳の横断不変条件を一括検証し、変更不能な`CapabilitySnapshot`として確定する。PES assemblerは全ての有効な明示PES `streamId` 0..255とwildcard `0xFFFF`を同じPES閉包で扱い、宣言長ありPESと映像stream IDの長さ0 PESを`MAX_PES_BUFFER_BYTES`および`pesRuntimeBudgetBytes`内で保持する。Tuner VTSは別途起動前環境へ結び付け、入力元、PID、経路、queue容量、memory予算が定義されるまで`DESIGN_HOLD_VTS_ENVIRONMENT_UNDECLARED`とする。
- AVの共有方式とイベント固有方式は、同じ実行時台帳を共有する。各filterでは`CapabilitySnapshot.avPerFilterLiveBytes`、サービス全体では`CapabilitySnapshot.avRuntimeBudgetBytes`を未解放payloadバイト数の上限とし、イベントの実サイズだけを割り当てる。`openFilter(type, bufferSize, cb)`の`bufferSize`はFMQ容量として別に予約する。固定スロット数や1 MiB単位をAOSPまたはコーデック上限として規範化せず、使用中の割り当てを追い出さない。
- ARIB STD-B10 5.13-E1 Part 1 5.2.4〜5.2.17・Part 3 5.1.1〜5.1.3を表ごとのsection上限1021/4093の根拠とし、STD-B32 3.11-E1 Fascicle 3 Chapter 3 3.1をPES構文、Fascicle 1 Chapter 5 5.1.1・Attachment 2 Chapter 5 5.1・Attachment 5 Chapter 5 5.1.1を製品対象video PESのPTS明示、Fascicle 2 Chapter 5.2.2をMPEG-2 AAC LC ADTSのsampling frequencyと1 raw-data-block/frameというexact frame duration、Fascicle 2 Attachment Chapter 2 2.1をaudioでは特定境界の先頭frameにPTSを要求するだけで全PESへの明示保証ではないことの証拠本文とする。Fascicle 3がoptional PES headerを委ねるITU-T H.222.0 2.4.3.7は、audio PTSが当該PES内で開始する最初のaudio access unitへ対応することの根拠とする。B32を4093の独立した上限根拠として使用しない。B25は公式英訳6.7-E1全文を精読基準とするが、`開発規則.md` のproduct-level invariantどおり、Part 1 §4.9の受信機システム最小8鍵組容量は本製品全体として恒久的に適合対象外とし、同条項への適合を宣言しない。STD-B25デコード能力は、対応するPart・方式・payload処理と、物理tuner/backend復号経路ごとの実鍵組数、実PID数、pool共有単位、枯渇時の`UNAVAILABLE`を製品profileの事実として定義する。AOSPに公開欄は追加せず、session間で共有する同じ内部台帳で受付と解放を強制する。
- 対象ドライバーと上流Linuxの証跡は、AOSP契約とは独立した根拠として扱う。

### ARIB規範本文との静的照合

ARIB依存の規範主張は、**現行日本語版の版番号**と、**今回実際に条項本文を精読した証拠本文**を分離して管理する。証拠本文と現行日本語版の版が一致しない規格は`差分未証明`とし、その規格について現行版まで条項内容が同一である、または現行版へ完全適合を検証済みであるとは主張しない。改定概要・版一覧・紹介ページは版管理の一次資料として使えるが、条項本文の代替にはしない。

| 規格 | 現行日本語版 | 今回精読した証拠本文 | 版差分状態 | 精読条項 / 本PRで使う主張 | 所有文書 |
|---|---|---|---|---|---|
| STD-B10 | 5.14 | 5.13-E1 英語版 | `差分未証明` | Part 1 5.2.4〜5.2.17・Annex B、Part 2 Table 6-5・6.2.12・6.2.26・Annex E、Part 3 5.1.1〜5.1.3 / PSI/SI Table ID・表別section長・CRC、parental rating、codec signaling | 本書、`arib_si_engine_rs/DESIGN_JA.md`、`tis/DESIGN_JA.md` |
| STD-B20 | 3.0 | 3.0 日本語版 | `版一致` | 2.9別記第2・別記第3、2.10 / 相対TS番号0〜7とTS_IDの別domain | 本書 |
| STD-B21 | 5.14 | 5.12-E2 英語版 | `差分未証明` | Appendix 10 Table 10-3/10-4 / CATV C13〜C63中心周波数 | 本書、`tis/DESIGN_JA.md` |
| STD-B24 | 6.5 | 6.4-E1 英語版 Fascicle 1 | `差分未証明` | 7.1.1.1〜7.1.2.4、9.1.1、9.2、9.3、9.5、9.6 / SI/EPG文字、字幕/data group、PTS、PMT descriptor | `arib_si_engine_rs/DESIGN_JA.md`、`tis/DESIGN_JA.md` |
| STD-B25 | 7.0 | 6.7-E1 英語版 | `差分未証明` | Part 1 2.2.2.4、2.2.2.10〜2.2.2.11、3.1.5〜3.1.7、3.2.3〜3.2.4、4.3.3.3、4.8 / MULTI2・ECM/EMM・Ks境界。§4.9はproduct-level非適合方針を別途維持 | 本書、`開発規則.md` |
| STD-B31 | 2.3 | 2.2-E1 英語版 | `差分未証明` | 2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7 / ISDB-T伝送parameter | 本書 |
| STD-B32 | 4.1 | 3.11-E1 英語版 Fascicle 1・2・3 | `差分未証明` | Fascicle 1 Chapter 5 5.1.1、Attachment 2 Chapter 5 5.1、Attachment 5 Chapter 5 5.1.1 / MPEG-2・AVC・HEVC video PESのPTS明示。Fascicle 2 Chapter 5.2.2 / MPEG-2 AAC LC ADTSのsampling frequencyと1 raw-data-block/frame、Attachment Chapter 2 2.1 / audio parameter切替後の先頭frameへのPTS。Fascicle 3 Chapter 3 3.1 / PES構文とH.222.0 optional header参照 | 本書 |

上表で`差分未証明`の規格について、本PRのARIB根拠は「今回精読した証拠本文が支持する範囲」に限定する。現行日本語版との条項差分が別途条項本文で確認されるまで、現行版まで検証済みという表現へ読み替えない。

### `CapabilitySnapshot` の依存閉包合成

`ProductProfile`は全能力を一個の候補vectorとして一括採否せず、次の`CapabilityClosure`ごとに優先順を持つ有限候補を宣言する。候補値は任意の非負整数とし、実資源を2の冪へ丸めない。

| 閉包 | 原子的に確定する内容 | 依存先 | 失敗時の縮退範囲 |
|---|---|---|---|
| frontend | backend、電源トポロジ、frontend object、tune/scan worker、callback、期限資源 | 機器probeと共有worker基盤 | 当該frontendだけを非公開 |
| demux base | demux object、入力境界、共通packet処理、基礎worker/cleanup枠 | 共有worker基盤 | demuxと配下能力だけを非公開 |
| filter main type / FMQ | main type別object数、FMQ byte、callback、assembler、配送worker。SECTIONでは公開数分の`TableInfoOneShotTracker`（target metadataと256-bit bitmap）を含む | demux base、共有worker基盤 | 当該main typeだけを非公開 |
| PES | PES filter数、assembler、`pesRuntimeBudgetBytes` | section以外の対象filter閉包、demux base | PES能力だけを非公開 |
| AV | AV filter数、1 event、filter別未解放総量、runtime総量、allocator/handle台帳 | 対象filter閉包、demux base | AV能力だけを非公開 |
| DVR playback / record | 用途別object数、FMQ、処理中buffer、worker、callback | demux base、共有worker基盤 | 当該DVR用途だけを非公開 |
| shared runtime | worker、callback、reaper、cleanup authority、診断台帳の共有上限 | なし | 依存する閉包だけを候補から除外 |

各閉包は、必要な共有runtime claimを含む全依存資源を同一transactionで仮予約し、全て成功した候補だけを選ぶ。ある閉包の失敗を理由に、依存関係のない閉包を落としてはならない。共有枠が複数閉包で競合する場合は`ProductProfile`の固定優先順で候補を評価し、先に確定したclaimを後続候補が越えないようにする。候補間の数値を無制約に組み合わせるのではなく、各閉包自身の内部不変条件と明示した依存辺を保ったまま合成する。

全閉包の選択後、次を一括検証して変更不能な`CapabilitySnapshot`を確定する。

- `getFrontendIds()`、`getFrontendInfo()`、open受付が同じfrontend集合を参照する。
- `getDemuxIds()`、`getDemuxInfo()`、`getDemuxCaps()`、open受付が同じdemux集合と個数を参照する。
- `numDemux`、main type別`filterCaps`、PES/AV/DVR個数が、依存先demuxと共有runtime claimを越えない。
- FMQ、PES、AV、playback処理中buffer、callback、worker、reaper、cleanupの各台帳上限が、選択済み閉包の合計claim以上である。
- capability query、open、configure、start、配送の受付判定が、同じsnapshotと台帳残量だけを入力にする。

合成後の横断検証に失敗した場合はsnapshotを公開せず、全仮予約を逆順に返却する。サービス寿命中にsnapshotの個数または能力集合を部分更新しない。open/配送時の実領域はsnapshotの閉包別台帳残量から割り当てる。

### サービスオブジェクトの上限

サービスオブジェクトの公開個数、FMQ・PES・AVの各byte上限とSECTION one-shot追跡上限、worker・callback・reaper・cleanup枠は、選択済み`CapabilityClosure`のclaimから導出する。ある閉包候補を予約できない場合は、その閉包と推移的に依存する能力だけを候補から除外し、依存しないfrontend、filter main type、DVR用途を0へ落とさない。`ProductProfile`の優先順は共有資源を競合する閉包候補の選択順にだけ使用し、全能力を含む単一vectorの採否またはquery-only一括縮退へ使用しない。

全閉包の合成後にquery/open、`numDemux`、`filterCaps`、用途別個数、全byte台帳の横断不変条件を検証する。整合したsnapshotを構成できない場合は全仮予約を戻してserviceを登録しないが、AV、PES、SECTION、DVR等の局所閉包不足だけを理由に、整合して残せる無関係な能力を全0にしてserviceを登録する状態は設けない。変更不能なsnapshotを個数、依存枠、byte予算、受付可否の正本とし、`CleanupPending`または`Quarantined`は解放完了まで使用中と数える。

| 資源 | 範囲 | `ProductProfile`上限 | 公開数 | 最小解放数 | 所有者別上限 | 保証しない事項 |
|---|---|---:|---|---:|---|---|
| LIVE_DEMUX | サービス全体 | 8 | `CapabilitySnapshot`の値 | 0 | なし | 呼び出し側指定のFMQ容量はsnapshotの`fmqRuntimeBudgetBytes`から別transactionで予約する。 |
| FILTER_TS | サービス全体 | 32 | `CapabilitySnapshot`の値 | 0 | なし | 呼び出し側指定のFMQ容量はsnapshotの`fmqRuntimeBudgetBytes`から別transactionで予約する。 |
| FILTER_SECTION | サービス全体 | 8 | `CapabilitySnapshot`の値 | 0 | なし | FMQ容量に加え、各公開filterについて1個のtarget metadataと256-bit（32 byte）の配送済みbitmapをSECTION閉包から予約する。section payloadは逐次配送し、table全体のpayload領域を別途予約しない。 |
| FILTER_AUDIO | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | なし | FMQの`bufferSize`とは別に、実payloadをsnapshotの`avPerFilterLiveBytes`と`avRuntimeBudgetBytes`から割り当てる。物理領域の起動時先取りはしない。 |
| FILTER_VIDEO | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | なし | FMQの`bufferSize`とは別に、実payloadをsnapshotの`avPerFilterLiveBytes`と`avRuntimeBudgetBytes`から割り当てる。物理領域の起動時先取りはしない。 |
| FILTER_PES | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | 有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じPES capabilityで扱う。宣言長ありPESは宣言長+6 byteをPES実行時台帳からclaimし、映像`0xE0..0xEF`の長さ0 PESは`MAX_PES_BUFFER_BYTES`と同台帳の上限内で組み立てる。stream ID別の非公開capabilityを設けない。 |
| FILTER_PCR | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | なし | PCRは通常payload FMQを持たず、`openFilter()`の`bufferSize`を`fmqRuntimeBudgetBytes`から予約しない。固定資源にはstatus callbackとA/V sync / PCR clockに必要なgeneration-local state（`PcrClockAnchor`等）を含む。 |
| DVR_PLAYBACK | サービス全体 | 8 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | configure時にFMQと同容量の処理中バッファーをsnapshotの2台帳から同時予約する。`VtsEnvironmentProfile`が`UNBOUND`ならXML、モジュール、試験シナリオを選択しない。 |
| DVR_RECORD | サービス全体 | 8 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | `VtsEnvironmentProfile`が`UNBOUND`ならXML、モジュール、試験シナリオを選択しない。`BOUND`なら宣言済み静的設定のキュー容量だけを原子的に予約する。 |

### AV割り当て

| 項目 | 値 | 範囲 | 設計根拠 | 動作 |
|---|---|---|---|---|
| transport_profile | DUAL_SHARED_PLUS_EVENT_LOCAL | AV filterの世代ごと | AOSPの`MediaEvent`とJNIの二重表現 | 共有領域とイベント専用領域は同じ実行時バイト台帳を使用する。 |
| fmq_byte_budget | `openFilter(type, bufferSize, cb)`の`bufferSize` | filterの世代ごと | AOSP open要求 | FMQ容量としてだけ予約する。AV payload領域の上限または裏付けに流用しない。 |
| filter_live_byte_budget | `CapabilitySnapshot.avPerFilterLiveBytes` | AV filterの世代ごと | 起動前に検証済みの製品メモリー予算 | 当該filterの未解放payload合計上限とする。FMQ領域とは別に数える。0ならAV能力を公開しない。 |
| service_live_byte_budget | `CapabilitySnapshot.avRuntimeBudgetBytes` | サービスインスタンス | 起動前に検証済みの製品メモリー予算 | 起動時に物理領域を先取りせず、全AV filterの未解放実サイズ合計を上限以下に保つ。 |
| allocation_size | イベントの実payloadバイト数 | 割り当てごと | MediaEvent payload | filter別残量とサービス全体残量の両方に収まる場合だけ正確なサイズを確保する。 |
| implementation_pool | 非規範 | allocator内部 | 性能最適化 | 固定slot数・slot sizeは実装詳細であり、公開能力、AU上限、設計変更判定へ使わない。 |
| allocation_failure | ASYNC_OVERFLOW_WITHOUT_MEDIA_EVENT | イベントごと | 割り当てトランザクション | 当該イベントを破棄し、`OVERFLOW` statusと型付き診断を通知する。`dataId`を公開せず、filter状態を維持し、次payloadで再試行する。使用中の割り当てを追い出さない。 |
| data_id | CHECKED_POSITIVE_SIGNED_63_BIT_NEVER_REUSED | サービスの存続期間 | AV台帳 | IDを発行できない場合は割り当てを拒否する。 |
| delivered_lifetime | ACTIVE_OR_RELEASE_ONLY_UNTIL_RELEASE | 割り当てごと | 解放規則 | 配送済み領域はflush、再設定、論理closeでは回収せず、解放要求まで保持する。 |

### 失敗影響範囲

| 種別 | 検出境界 | 例 | raw TS | record TS | section・PES・AVの意味処理 | workerまたは経路の状態 | 公開APIの結果 | 診断 | 隔離規則 |
|---|---|---|---|---|---|---|---|---|---|
| InfrastructureCorrupt | FMQ・ネイティブトランザクション・制御面 | descriptor grantorの範囲外、成立しないトランザクション長、キュー制御ブロックの不変条件違反、`EventFlag`オブジェクト破損 | 対象外または影響経路を停止 | 対象外または影響経路を停止 | 影響経路を停止 | 影響するキューまたは経路を遮断して隔離 | `UNKNOWN_ERROR`または操作固有の基盤障害 | 識別子・世代・方向を含む`InfrastructureCorrupt` | 影響する基盤経路を必ず隔離する。サービス全体の隔離は`InfrastructureCorrupt`または`FatalUnfencedGlobalMutation`の場合だけ許可する。`FatalOwnedIo`は自身の後片付けが未完了の場合だけ所有者または経路単位で隔離できる。 |
| PacketMalformed | 188バイトTSの入口検証 | 長さが188以外、syncが0x47以外、予約済み`adaptation_control`、adaptation長超過 | 不正packetを破棄 | 不正packetを破棄し、`byteNumber`は実際の書き込みバイト数だけを数える | 破棄し、必要な場合だけ影響する組み立て途中の状態を戻す | 継続 | packetごとのAIDL失敗は返さない | 上限付き`malformed_ts`計数と理由 | 隔離しない |
| TransportErrorIndicator | 検証済み188バイトTS header | `TEI=1` | 到着順のまま保持 | 保持し、`byteNumber`は実際のバイト数を数える | 破棄して再同期し、解析イベントを出さない | 継続 | なし | `tei_packets_observed`と意味処理別のTEI破棄記録 | 隔離しない |
| ContinuityDiscontinuity | PIDの連続性とadaptation discontinuity | CC欠落、同じCCで188バイトTS packetが不一致、`discontinuity_indicator` | 保持 | 保持 | 境界前後のsection / PES等を連結しない。steady-state continuityは`PacketPipeline`、parser/assembler stateは各per-filter parser owner、Filter parser fenceは`FilterProducerDrainGate.parser_state_generation`を正本とし、`StreamBoundaryTxn`はtyped reset / invalidate dispatchだけを行う | 継続 | なし | PIDと世代を含む不連続診断 | 隔離しない |
| `SemanticParseFailure` | section、PES、録画索引の解析器 | section長、設定時のCRC検査、予約bit、PESヘッダーまたはPTSマーカーの異常 | 検証済みTSを保持 | 検証済みTSを保持 | non-rawの影響する意味単位を破棄する。raw sectionは上記行列で配送可能な完全バイト列を保持し、型付きeventだけを抑止する | 継続 | なし | 解析器の理由とPID、raw配送有無 | 隔離しない |
| NoUsableDescramblerKey | descrambler方針 | 対応する有効な鍵がないscrambled packet | scrambled packetを保持 | scrambled packetを保持 | 復号済みの意味イベントを出さない | 継続 | なし | 上限付き`scrambled_without_key`計数 | 隔離しない |
| FatalOwnedIo | 所有する入力元・driver・`EventFlag`の実行時処理 | 永続的なreadまたはioctl失敗、必須deviceのclose、所有する`EventFlag`の回復不能障害 | 影響経路を停止 | 影響経路を停止 | 影響経路を停止 | 所有者単位の実行処理を失敗終了 | 操作境界に応じて`UNKNOWN_ERROR`または`UNAVAILABLE` | 型付きの主障害 | 所有者の後片付けが未完了の場合だけ、所有者または経路単位で隔離する。`FatalUnfencedGlobalMutation`の証拠なしに基盤またはサービス全体を隔離しない。 |
| FatalUnfencedGlobalMutation | cleanupとreaperの監視 | 世代を無効化した後も残存workerが全体状態を変更できる | 対象外 | 対象外 | 対象外 | サービス継続に重大な証拠として、全体変更が証明されない限り影響する権限だけを止める | サービス継続に重大な失敗として公開 | 遮断できていない全体変更の証拠 | 証明された権限を必ず隔離する。サービス全体への拡大には、全体変更の明示的な証拠を必要とする。 |

### フロントエンド設定の反映表

本表をfrontend設定とselectorに関する入力分類の正本とする。規格上は有効だが、対象のbackendとdeviceで反映できない値は`UNAVAILABLE`、不正なtag、予約値、規格値域外は`INVALID_ARGUMENT`とし、どちらの場合もbackendと直前の要求を変更しない。

| backendと対応能力 | frontend | 設定項目 | 受理する入力 | 成功時の動作 | 規格上は有効だが未対応 | 不正または値域外 |
|---|---|---|---|---|---|---|
| 条件に完全一致するpx4の対応項目 | ISDB-T | 周波数 | backendで検証済みの値域 | 検証済みの周波数設定経路へ反映 | 別のprofileでは有効な値：`UNAVAILABLE` | `INVALID_ARGUMENT` |
| 条件に完全一致するpx4またはearth_pt1の対応項目 | ISDB-T | 帯域幅 | `AUTO`または`BANDWIDTH_6MHZ` | 検証済みの6 MHz設定経路を使用 | その他の既知の帯域幅：`UNAVAILABLE` | `INVALID_ARGUMENT` |
| 対象のpx4またはearth_pt1 | ISDB-T | モード、変調、符号化率、ガードインターバル、時間インターリーブ | `AUTO` | バックエンドの自動検出を使用 | 既知の具体値：`UNAVAILABLE` | `INVALID_ARGUMENT` |
| 対象のpx4またはearth_pt1 | ISDB-T | `inversion` | 未指定・自動を表すAIDL値 | 明示inversion制約を付けずbackendへ要求 | 設定・固定値検証できない既知の明示値：`UNAVAILABLE` | 予約値・未知値：`INVALID_ARGUMENT` |
| 対象のpx4またはearth_pt1 | ISDB-T | `serviceAreaId` | `0` | 未指定として扱い、追加制約を付けない | 正の値でbackendへ反映・検証経路なし：`UNAVAILABLE` | 負値：`INVALID_ARGUMENT` |
| 対象のpx4またはearth_pt1 | ISDB-T | `partialReceptionFlag` | 未指定を表すAIDL値。px4は`TRUE` / `FALSE`も対象 | 未指定は追加制約なし。px4の明示値は同期`tune()`受付後、同一generationのfreshなTMCC readback一致時だけ非同期`LOCKED`成立。不一致は`NO_SIGNAL`、scan candidateではlock成立として通知しない | earth_pt1のreadback blocker未解決中の明示`TRUE` / `FALSE`：`UNAVAILABLE` | 予約値・未知値：`INVALID_ARGUMENT` |
| 対象のpx4またはearth_pt1 | ISDB-T | layer `numOfSegment` | `0`、`0xFF`（`isSegmentAuto=true`時） | `0`は未指定として追加制約を付けない。`0xFF`はAndroid 14 CTS互換AUTOとしてbackend/demodulatorのsegment自動判定を使用 | `0xFF`かつ`isSegmentAuto=false`、または`1..13`で明示segment数を反映・検証できない値：`UNAVAILABLE` | `14..254`、負値、255超：`INVALID_ARGUMENT` |
| 対象のpx4またはLinux DVB / earth_pt1 | ISDB-S | selector未指定 | AOSP SDK defaultの`STREAM_ID + INVALID_STREAM_ID(0xFFFF)`を`Unspecified`へ正規化 | px4 BSは互換fallbackとしてrelative slot `0`、px4 CS110はfixed slot `0`、Linux DVB / earth_pt1は`DTV_STREAM_ID=NO_STREAM_ID_FILTER`を明示設定 | なし。通常の日本向けBSサービス選択はTISの明示absolute TSID経路を使用 | `0xFFFF`を明示TSIDとして再解釈しない |
| px4 legacy selector ABIの完全一致項目 | ISDB-S | `RELATIVE_STREAM_NUMBER` | `0..7` | 値を変更せずlegacy `slot`へ渡す | なし | `0..7`以外：`INVALID_ARGUMENT` |
| px4 legacy selector ABIの完全一致項目 | ISDB-S | `STREAM_ID` | `12..65534` | 値を変更せずlegacy `slot`へ渡す | `0..11`はAOSP上有効だがABI衝突で表現不能：`UNAVAILABLE` | `65535`または値域外：`INVALID_ARGUMENT` |
| absolute selectorに対応するLinux DVBの完全一致項目 | ISDB-S | `STREAM_ID` | `0..65534` | 値を変更せず`DTV_STREAM_ID`へ渡す | relative selectorに対応しない場合、`RELATIVE_STREAM_NUMBER 0..7`は`UNAVAILABLE` | `STREAM_ID=65535`または`0..7`以外の相対値：`INVALID_ARGUMENT` |
| 対象のpx4またはearth_pt1 | ISDB-S | modulation・code rate | `AUTO` | backendの自動検出を使用 | 既知の具体値：`UNAVAILABLE` | `INVALID_ARGUMENT` |
| 対象のpx4またはearth_pt1 | ISDB-S | `rolloff` | 未指定を表すAIDL値 | backend既定値を使用 | 設定または固定値検証できない既知の明示値：`UNAVAILABLE` | 予約値・未知値：`INVALID_ARGUMENT` |

選択子の対応能力は、機器識別情報と改訂適用範囲、versioned backend manifestのABI/API契約版、要求を実際に設定して結果を読み戻すfunctional probeが一致し、かつ`selector_capability_release_eligible=true`である台帳項目だけから作る。repository、commit SHA、build IDは台帳項目の作成証跡として保存してよいが、実行時の一致条件にしない。px4 legacy ABI契約の台帳項目では、相対`0..7`とabsolute `12..65534`を別typed selectorとして有効にする。absolute `0..11`は有効なAOSP値だがABIで表現不能なので`UNAVAILABLE`とし、相対値へ読み替えない。項目が空、不一致、または使用不可の場合は該当frontendを公開しない。`ProductProfile`は使用可能な部分集合を抑止できるだけで、対応能力を新設または拡張できない。AOSP SDK defaultの`STREAM_ID=INVALID_STREAM_ID(65535)`はBS/CS110を問わずselectorなしを表す入力として明示selector値の検証より先に`Unspecified`へ正規化し、本表で明示selector値`65535`を拒否する規則と混同しない。px4 BSのslot `0` fallbackはAOSP未指定入力をlegacy ABI契約で成立させるためだけの互換経路であり、通常の日本向けBS scan、channel保存、ライブ再選局のサービス選択へ使用してはならない。Linux DVB / earth_pt1では`Unspecified`を`NO_STREAM_ID_FILTER`として毎回明示設定する。

### LNB機器の資源規則

| backend | 検証証跡metadata | AOSPの公開API | driverの事実 | 設計規則 | 資源規則 | 根拠箇所 |
|---|---|---|---|---|---|---|
| px4_drv feat/android-ddk | c2a031db8771ddd6e3e0b3b4a712b64ec384139b | 証跡済み`setVoltage()`能力を持つ公開`ILnb` endpoint | 0 Vまたは15 Vのみ。tone、position、DiSEqCの実処理証跡なし | `aidl_baseline_eligible=false`はCTS分類として保持するがpublication gateにしない。probe/endpoint条件成立時は`getLnbIds()`へ列挙し、0 V / 15 Vを実機へ適用する。tone、position、DiSEqCは有効要求を`UNAVAILABLE`とする。機器項目が`InternalFixed15V`ならHAL内固定15 V、`ExternalOrShared`なら電圧非操作でISDB-S frontendを公開可能。`UnknownOrDisabled`ならsatellite frontend非公開 | 公開endpointにはLNB leaseを生成する。固定15 V経路はcaller制御可能な`ILnb.setVoltage()`と分離し、既存の機器rail lease・rollback・safe-state規則を使う | `driver/px4_device.c`のblob cfed72f...、`driver/ptx_chrdev.c`のblob 18f074... |
| earth_pt1 Linux v6.6 | ffc253263a1375a65fa6c9f62a893e9767fbebfa | 証跡済み`setVoltage()`能力を持つ公開`ILnb` endpoint | `pt1.c`では`SEC_VOLTAGE_13`を11 V、`SEC_VOLTAGE_18`を15 Vに対応付ける。tone、position、DiSEqCの実処理証跡なし | `aidl_baseline_eligible=false`はCTS分類として保持するがpublication gateにしない。probe/endpoint条件成立時は`getLnbIds()`へ列挙し、11 V / 15 Vを実機へ適用する。tone、position、DiSEqCは有効要求を`UNAVAILABLE`とする。機器項目が`InternalFixed15V`ならHAL内固定15 V、`ExternalOrShared`なら電圧非操作でISDB-S frontendを公開可能。`UnknownOrDisabled`ならsatellite frontend非公開 | 公開endpointにはLNB leaseを生成する。固定15 V経路はcaller制御可能な`ILnb.setVoltage()`と分離し、既存の機器rail lease・rollback・safe-state規則を使う | Linux v6.6 commitの`drivers/media/pci/pt1/pt1.c` |

### VTS環境に関する設計保留

| 入力ID | 必要な入力 | 判断規則 | 状態 |
|---|---|---|---|
| VTS-ENV-01 | AOSP branch、Tuner VTS設定schema/版、使用VTS artifact/tag/commit | 選択するVTS binary/source contractを一意に固定する | UNDECLARED_DESIGN_HOLD |
| VTS-ENV-02 | フロントエンドの入力元と選局・走査パラメーター | XML選択前に機器またはソフトウェアの入力元、周波数、ストリームID・種類、信号の有無を宣言する | UNDECLARED_DESIGN_HOLD |
| VTS-ENV-03 | audio・video・recordのPIDと有効なdata flow | 使用可能な入力元と対応済みHAL経路の両方があるflowだけを含める | UNDECLARED_DESIGN_HOLD |
| VTS-ENV-04 | filterとDVRのbuffer size | 宣言済みの静的設定値を起動時の資源一式としてそのまま使い、オブジェクト数から推定しない | UNDECLARED_DESIGN_HOLD |
| VTS-ENV-05 | productのprocess memoryとFMQ割り当て予算 | serviceまたはVTSの起動前に宣言済みキュー一式を原子的に予約し、失敗した設定は拒否する | UNDECLARED_DESIGN_HOLD |
| VTS-ENV-06 | VTS設定XMLのfilename解決 | 選択済みVTS artifact/tag/commitと試験実行機の`ro.vendor.vts_tuner_configuration_variant`から、そのVTS実装が実際に読むfilenameを解決する。artifactまたはproperty未確定時はfilenameを推測しない。実行時rendererは使用しない | UNDECLARED_DESIGN_HOLD |
| VTS-STATE-UNBOUND | 完全な`VtsEnvironmentProfile`がない | VTS artifact/tag/commitまたはvariant propertyを含む環境入力が未確定で、XML filenameを解決せずXMLをinstallしない| DESIGN_HOLD |
| VTS-STATE-BOUND | 6入力が宣言済みで、object要求が確定済みsnapshot以内に収まり、queue一式の予約に成功 | 起動前に選択したVTS実装で解決済みのpathへ宣言済み静的XMLを正確に1つinstallし、別設定へ自動fallbackしない | BOUND_STATIC_XML |
| VTS-STATE-REJECTED | object要求がsnapshotを超える、またはqueue予約に失敗 | 設定を拒否し、runtime snapshotを維持する。解決済みpathまたは既定値を推測したXMLまたは既定XMLをinstallしない | PROFILE_REJECTED |

### キューと生産側の内部プロトコル

#### 適用範囲

安定版 Tuner AIDL は変更しない。Filter / SharedFilter の drain state、permit、flush / close semantics は 0-S-3B の `FilterProducerDrainGate`、DVR の queue epoch、read / write token、flush / close semantics は `QueueEpochProtocol`、両者の公開 `flush()` cleanup orchestration は `QueueCleanupUseCase` を唯一の正本とする。この節では同じ state machine を再定義しない。

Playback DVR の queue incarnation identity は `PlaybackQueueBacking` が所有し、`QueueEpochProtocol` はその identity を参照して同一 identity 内の `queue_epoch` だけを所有する。入力 origin の正本キーは `TsInputOrigin::PlaybackDvr(dvr_id, queue_identity, queue_epoch)` とし、`queue_identity`、`queue_epoch`、stream boundary の変更はそれぞれ `PlaybackQueueBacking`、`QueueEpochProtocol`、`StreamBoundaryTxn` の契約に従う。

#### 独立した世代軸

`queue_epoch`、`filter_delivery_generation`、`parser_state_generation` を同じ値の別名にしたり、1個の世代としてまとめて進めたりしてはならない。

### ワーカー終了契約

Generic worker lifecycle の owner generation、stop / wake / join、reaper handoff、terminal budget、lease return、`Quarantined` / `ServiceCritical` 分岐は 0-S-3B の `WorkerRuntime` を唯一の正本とし、failure category の分類は `WorkerFailureClassifier` を正とする。本節では第二の generic lifecycle state machine を定義しない。

#### フィルターの排出処理との接続

Filter `flush()` が待つ producer drain と permit の意味は `FilterProducerDrainGate` を正とし、worker の cancel / wake / join / reaper は `WorkerRuntime` に従う。公開 `flush()` は Binder callback 完了や上限のない join を待たない。

#### LNBとの接続

LNB の logical close と cleanup authority は `ObjectCloseTxn` および LNB 資源契約を正とし、worker の停止・回収は `WorkerRuntime` に従う。終端 cleanup が完了するまで endpoint lease を新しい `openLnbById()` / `openLnbByName()` 受付へ戻さない。


## clear non-passthrough MediaEvent presentation timestamp 契約

本製品が成功対応として表明するlive AVのclear / non-passthrough media-filter profileでは、Tuner HAL / media-filter producerは、配送するすべてのnon-empty `DemuxFilterMediaEvent`について、当該eventのESデータへ適用可能な有効な33-bit 90 kHz presentation timestampを`pts`へ設定してから配送する。AOSP契約どおり`isPtsPresent`は元PES headerに明示PTSが存在したかというprovenanceだけを表し、timestamp validity flagとして使用しない。明示PTSを持つPESでは`isPtsPresent=true`かつ`pts`をその明示PTSとする。明示PTSを持たない合法なPESでは`isPtsPresent=false`を維持し、hardware demux / driver / producer-side media extractor等が当該media outputに対応するpresentation timestampをauthoritative timing metadataとして確定できる場合に限り、その対応値を`pts`へ設定する。定数0、単純な直前PTS carry-forward、PCR、wallclock、arrival time、nominal frame rateまたはsample rateだけからpresentation timestampを推測生成しない。provenanceを満たすために`isPtsPresent`を`true`へ偽装してはならない。

presentation timestampと当該media outputのassociation責務は`MediaEvent`を公開するproducer側境界で完了させ、TISへPES再解析、codec別AU parser、AU再構成、独自clockを要求しない。backendがauthoritative timing metadataを直接出す場合はその値を透過する。producer-side media extractorを使う場合は、対応codecの連続ES上でframe header構文、frame length、時刻算出parameterを検証する。PES境界はframe境界と一致する前提にせず、開始済みframeは規格上限内のbytesを同一`FilterRuntime`へ最大1件だけ保持して完成させる。audioの`MediaEvent`は完成した1 AUを正確な`dataLength`で配送し、`pts`、`streamId`、PTS/DTS presence/valueは同じAUの開始PESまたは構造検証済みtimelineから構成する。明示PTSはH.222.0どおり当該PES内で開始する最初のaudio access unitをanchorとし、PES先頭が前AUのcontinuationならそのAUを完成後に従前の時刻で配送し、当該PES内で開始するfirst AUは別eventとして明示PTSへreanchorする。PTS-sparse AUの時刻は同一timing parameterのexact sample countとbitstream headerのactual sample rateから確定する。filter開始またはstream boundary直後の未anchor状態で`data_alignment_indicator=false`なら、先行AUの最大残り長未満だけを探索し、対応codecの完全headerと宣言frame lengthを構造検証する。次境界未確認の候補をauthoritative anchorへ即時commitせず、先行AU最大1 frame、当該PESで開始するfirst AU最大1 frame、次header最大7 byteの合計16389 byte以内だけ同一ownerへ保留する。後続PES上で同一signatureの次境界まで確認できた一意候補だけをcommitし、候補の複数成立または上限超過はfail-closedとする。syncword一致だけの探索、上限なし走査、第二queueは追加しない。`data_alignment_indicator=true`ならpayload先頭以外へ探索しない。未anchor、未対応／不正header、未通知parameter変更、overflow、TEI、continuity gap、scramble/drop、flush、source/generation変更では対応付けを停止してanchor、未完了frame、cold-start保留bytesを破棄し、PCR / wallclock / nominal値へfallbackしない。合法なPES間partial frameだけを破棄理由にしない。この有限codec extractorを、入力構文を検証しないgeneric timestamp interpolationへ拡張しない。producer側境界でもauthoritative associationを成立させられないbackend/profileまたはeventは、live media-filter成功配送として扱わない。公開Tuner AIDL/VINTFの`isPtsPresent` / `pts`の意味は変更せず、VTS profileも既存capability整合規則に従う。

製品既定profileのTS videoで対象とするMPEG-2 Video、AVC、HEVCについて、今回精読したARIB STD-B32 3.11-E1 Fascicle 1は各video PES headerへのPTS明示を要求する。この入力profileではPES parserが当該media outputのpresentation timestampをauthoritativeに確定できるため、`numVideoFilter=1`と有限AV予算を広告する。PTSのないvideo PESを別時刻源で補間して成功入力へ昇格させず、AOSPのpresence/valueを偽装しない。現行日本語版4.1との条項差分は未証明であり、上記主張を現行版までの完全適合確認とは扱わない。

一方、同3.11-E1 Fascicle 2 Attachment Chapter 2 2.1のaudio規定はparameter切替等の境界における先頭frameへのPTSを要求するが、全audio PESへの明示を保証しない。Fascicle 3 Chapter 3 3.1が参照するH.222.0 2.4.3.7ではaudio PTSは当該PES内で開始する最初のaudio access unitへ対応し、Fascicle 2 Chapter 5.2.2の製品対象MPEG-2 AAC LC ADTSはheaderにactual sampling frequencyを持ち、`number_of_raw_data_blocks_in_frame=0`により1 raw-data-block/frameへ固定される。さらにARIB TR-B15 4.6-E1 Fascicle 3 4.2.2はBS／広帯域CSのMPEG-2 AACについてPES packetとaudio frameのnon-synchronizationを許容する。製品既定producerはMPEG-2 AAC LC ADTSとMPEG audioについて、PESを跨いで連続するframe header／lengthを上限付きで追跡し、この規格上のanchor、actual sample rate、exact sample countを結合してPTS-sparse eventのauthoritativeな33-bit 90 kHz時刻を確定する。parameter変更後の明示PTSで最初に開始するAUへ再anchorし、未通知変更、unsupported／malformed frame、gapまたは各lifecycle境界では配送を抑止して型付き診断を残す。この成立範囲に対応する`numAudioFilter=1`とaudio/video各1件を閉じる有限AV予算を製品既定profileで広告する。`getAvSyncTime()`用のPCR/wallclock補間を個別eventのPTSとして流用しない。TR-B15の証拠は公式英訳4.6-E1の精読範囲であり、現行日本語版8.9との差分は未証明とする。

最低試験は、(1) explicit PTS PESでは`isPtsPresent=true`かつ`pts`がその明示PTSと一致すること、(2) `isPtsPresent=false`の合法なPTS-sparse inputでもbackend metadataまたはframe境界・timing parameterを構造検証済みのframe列からauthoritative associationを確定できる場合は当該media outputに対応する値を`pts`へ出すこと、(3) authoritative sourceがない場合は定数0、直前PTS、PCR、wallclock、nominal frame rate / sample rate等のgeneric interpolationを行わず、そのeventを成功配送しないこと、(4) exact sample countの有理数累積、33-bit wrapとA/V timeline差を維持すること、(5) `isPtsPresent=false`だけを理由にpayload破棄/fatalせず、associationが成立するeventはprovenanceをfalseのまま配送すること、(6) flush、transport discontinuity、source/generation変更後は明示PTSまで再開しないこと、(7) ADTS／MPEG audioのframe bodyおよびheader途中にPES境界を置いても有限残余で明示anchorからPTS-sparse eventへ継続し、最初に開始するAUの時刻とpresence=falseを維持すること、(8)未anchorかつ`data_alignment_indicator=false`の最初のPESが先行AUのcontinuationから始まる場合に、sync-likeな偽候補を次frame境界の構造不一致で拒否し、最初に開始する検証済みAUへだけ明示PTSをanchorして後続PTS-sparse eventへ継続すること、(9)先行する偽`HeaderOnly`候補と真のfirst AUがともに次PESへ跨ぐ場合は後続境界確認までevent/anchorを公開せず、確認後に真のAUだけを採用すること、(10)continuation tailとnew AUが同じPESにある場合も、最終`MediaEvent.offset/dataLength`が各完成AUの正確なrangeとなり、付与PTS/provenanceが同じAUへ対応すること、(11)製品snapshotのTS AUDIOが公開demux open-filter use-caseを通り、audio-only serviceがvideo filterなしで既存TIS start gateを通ること、を含める。
