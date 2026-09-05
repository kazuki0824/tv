# tuner_hal2 product integration

この文書は、`tuner_hal2` を Android TV 14 系 product image の既定 Tuner HAL service として組み込むためのSSOTである。

## 0. 固定方針

```text
- product default Tuner HAL service は tuner_hal2 だけとする。
- 旧 tuner_hal は参照用ソースとして repository に残すだけで、product image へ入れない。
- ITuner/default を登録する実体は android.hardware.tv.tuner-service.maleicacid2 だけとする。
- 旧 tuner_hal の product package、VINTF fragment、init rc、PRODUCT_PACKAGES、product integration を同一productで有効化しない。
- 旧 `tuner_hal/INTEGRATION.md` は legacy/reference 用であり、既定 product 統合手順のSSOTにはしない。
```

## 1. product makefile

製品の product makefile で次を継承する。

```make
$(call inherit-product, vendor/maleicacid/tv/tuner_hal2/config/product_integration.mk)
```

`config/product_integration.mk` は次だけを `PRODUCT_PACKAGES` に追加する。

```make
PRODUCT_PACKAGES += \
    android.hardware.tv.tuner-service.maleicacid2 \
    maleicacid_tuner_hal2_ueventd_rc
```

旧 `tuner_hal` の `maleicacid.tv.tuner_hal-service` は追加しない。

## 2. BoardConfig / sepolicy

BoardConfig 側で次を取り込む。

```make
include vendor/maleicacid/tv/tuner_hal2/config/BoardConfigVendorSePolicy.mk
```

`BoardConfigVendorSePolicy.mk` は `vendor/maleicacid/tv/tuner_hal2/sepolicy` だけを既定Tuner HAL用のvendor sepolicyとして追加する。

## 3. ueventd import

製品側の vendor ueventd rc から次を import する。

```rc
import /vendor/etc/ueventd.tuner_hal2.rc
```

`ueventd.tuner_hal2.rc` はDVB / px4 / dma_heapのdevice node permissionを設定する。

## 3.1 px4_drv readback ABI のproduct前提

px4 backendをproductへ組み込む場合、採用kernel driverは `../開発規則.md` のpx4_drv product-level invariantを満たす版へ固定する。公開AIDLの意味、status capability、generation/readiness、scan callbackの規範値は `../tuner_hal/DESIGN_JA.md` を正とし、本節では再定義しない。

product build / 実機VTSの前に、少なくとも次のABI接続を確認する。

```text
- include/ptx_ioctl.h に PTX_GET_LOCK_STATUS が存在すること
- include/ptx_ioctl.h に pointer-free fixed-size PTX_GET_TMCC_TSID_LIST が存在すること
- tuner_hal2/device の ABI mirror と ioctl number / payload size が一致すること
- TMCC TSID readbackがactive frontend sessionの既存control fdを使い、同一px4 chardevを再openしないこと
- VTS/profile toolingがdriver ioctlを直接呼ばず public Tuner AIDLだけを試験すること
```

これらを満たさないdriverでは、対応するpx4 frontend capabilityをproductで有効化したままVTS成功を宣言しない。

## 4. service登録

`tuner_hal2/Android.bp` の `rust_binary` は次を持つ。

```text
name: android.hardware.tv.tuner-service.maleicacid2
init_rc: tuner_hal2/init/android.hardware.tv.tuner-service.maleicacid2.rc
vintf_fragments: tuner_hal2/manifest/android.hardware.tv.tuner-service.maleicacid2.xml
```

init rc は `android.hardware.tv.tuner.ITuner/default` を登録する。VINTF fragmentも `ITuner/default` だけを宣言する。

## 5. 旧tuner_halの扱い

旧 `tuner_hal` は参照用ソースである。次をproductへ入れてはならない。

```text
- maleicacid.tv.tuner_hal-service
- tuner_hal/tuner-hal-service.rc
- tuner_hal/tuner-hal-service.xml
```

旧実装を手動でビルド・参照することは妨げないが、同一productで `ITuner/default` を二重登録してはならない。

## 6. VTS / product config policy

VTS / product config の公開契約、capability、`VtsEnvironmentProfile`の入力、状態、`VTS-STATE-BOUND` / `VTS-STATE-REJECTED`の意味は `../tuner_hal/DESIGN_JA.md` の`製品スコープ / AOSP capability / VTS profile 境界`、`CapabilitySnapshot`、`ProductProfile`、`VTS環境に関する設計保留`を正とする。本節は、それらをproduct buildと実機VTSへ接続する配置・生成・検証経路だけを所有し、profile入力の規範値、HAL capability、公開API戻り値、VTS状態を再定義しない。

本製品は monitor event feature を製品能力として採用せず、静的VTS/product configでも同featureを要求・広告する構成にしない。monitor event の公開API戻り値とcapability契約は `../tuner_hal/DESIGN_JA.md` を正とし、本書では重複定義しない。本書のproduct integration設定を、未定義の将来profileでmonitor eventを有効化するための切替点として扱ってはならない。

### 6.1 単一VtsEnvironmentProfileファイルと依存方向

`VtsEnvironmentProfile` は論理概念だけではなく、対象productごとに実在する単一設定ファイルとする。VTS環境に関して人間またはCLIが保存する設定はこの1ファイルだけを正本とし、1回のVTS構成生成で複数のprofileファイル、product makefileの個別変数、生成済みXMLの手編集値を合成して1個の論理profileを作ってはならない。serialization形式と物理pathは実装詳細として一意に選んでよいが、同じproduct/profileについて複数の人間編集設定ファイルを設けてはならない。

対話CLIはこの同一ファイルを生成・読み込み・更新する。地域から生成した受信候補、実機接続後に解決したfrequency / service / PAT / PMT / PIDに由来する具体値も、別のderived-resolution設定ファイルへ分離せず、同じ`VtsEnvironmentProfile`ファイルへ保存する。生成AOSP Tuner VTS XMLはderived artifactであり、この設定ファイルと同格の正本ではない。

Tuner HALのcapability、公開個数、FMQ/PES/AV/DVR/worker等の製品資源上限、frontend probe結果を`VtsEnvironmentProfile`の独立した規範値として複製してはならない。これらは`../tuner_hal/DESIGN_JA.md`の`ProductProfile` / `CapabilitySnapshot`と`tuner_hal2`の実機probeを正本とする。profile compilerが静的照合に必要とするHAL側情報は、同じ正本から機械的に取得したread-only contractとして入力してよいが、人間編集設定として保存せず、HALのruntime能力を変更する入力にも使用しない。

依存方向は次に固定する。

```text
interactive VTS profile CLI
        |
        v
single VtsEnvironmentProfile file
        |
        +--> regional candidate resolver --+
        |                                   |
        +--> device resolver through AIDL --+
        |                                   |
        +<----------------------------------+
        |
        v
VTS profile compiler / validator
        |
        +--> selected AOSP VTS schema / loader contract
        |
        +--> read-only tuner_hal2 capability contract
        |
        v
static AOSP Tuner VTS XML
        |
        v
AOSP Tuner VTS
        |
        v
public Tuner AIDL
        |
        v
tuner_hal2
```

逆方向に、VTS XML、variant property、VTS profileまたはVTS test resultから`tuner_hal2`の`CapabilitySnapshot`、frontend registry、backend probe結果、公開API成功範囲を変更してはならない。VTS設定は`tuner_hal2`を試験するための環境記述であり、被試験HALの能力を選択・拡張・縮小する設定面ではない。

### 6.2 profileフィールドの消費契約

`VtsEnvironmentProfile` はAOSP Tuner VTS XMLより高水準の単一入力SSOTである。このためprofileの全フィールドがXMLへそのままシリアライズされる必要はない。ただし、profileに永続化する各フィールドは必ず次のいずれかに分類できなければならない。

1. AOSP Tuner VTS XMLの1個以上の具体値へ直接変換される。
2. XMLへ出力する具体値を決定するresolver入力として消費される。
3. XMLの値、対象VTS契約、生成先またはinstall先を検証・選択するためにcompiler / validatorが消費する。

上記のどれにも該当せず、XML生成・検証・配置のいずれにも影響しないprofileフィールドを追加してはならない。「後で使う可能性がある」という理由だけで未消費metadataを保存しない。compiler / validatorは、認識しているprofile fieldが上記の消費経路を持たない場合にfail-closedで拒否する。

代表的な対応は次とする。

| profile情報 | XMLへの反映 | 消費先 |
|---|---|---|
| frontend type / frequency / stream selector | 直接反映 | frontend設定 |
| filter / DVR種別、PID、flow、queue要求 | 直接反映 | filter / DVR / data flow設定 |
| 地域指定 | 直接は反映しない | region resolverがfrontend/frequency候補を決定 |
| 地域から得た受信候補集合 | 最終採用値だけ反映 | device resolverが実機でfrequency候補を評価 |
| service / PAT / PMTで得た識別値 | XMLが必要とする最終PID等へ反映またはその決定に使用 | device resolver / compiler |
| AOSP/VTS契約識別 | 直接は反映しない | compilerが使用schema / loader contractを照合 |
| target product / backend | 直接は反映しない | compilerが`tuner_hal2`用capabilityと生成先を選択・検証 |
| variant指定 | loader契約に応じてfilename/propertyへ反映 | filename解決 / product配置 |

### 6.3 対話CLIによるprofile生成とtuner_hal2限定target

VTS環境profileには、実機が接続されていない開発環境でも作成・保存できる対話CLIを設ける。CLIの論理操作は少なくとも次を持つ。

```text
init            対話入力から単一VtsEnvironmentProfileファイルを新規作成して保存する
resolve-region  同じファイルの地域入力から受信候補集合を生成して同じファイルへ保存する
resolve-device  public Tuner AIDLと受信TSを使って実機依存値を解決し同じファイルへ保存する
compile         同じファイルだけを入力に検証しAOSP Tuner VTS XMLを生成する
```

`init` は実機接続を前提にしない。AOSP/VTS契約識別、対象backend/product、受信方式、明示入力または地域入力、要求するVTS flow、queue要求等、入力時点で確定できる値を対話的に取得し、未確定項目を架空値で埋めずにprofileを保存する。必要入力が揃っていないprofileは `../tuner_hal/DESIGN_JA.md` の `VTS-STATE-UNBOUND` 判定に従い、保存可能であっても静的VTS XMLをinstall可能とは扱わない。

CLIと生成profileのtargetはproduct defaultである`tuner_hal2`に固定する。profile compiler、生成XML module、variant設定、vendor imageへの配置は`tuner_hal2`のproduct integrationだけへ接続し、旧`tuner_hal`の`profiles/`、`tools/render_vts_config.py`、`config/tuner_vts_config_*`、旧service packageを更新・参照・fallback先にしてはならない。旧`tuner_hal`に存在するprofile rendererは設計参考として読めても、このCLIの実行対象または生成先にはしない。

ここでいう`tuner_hal2`への反映は、`tuner_hal2`を被試験HALとするVTS構成を生成・配置することだけを意味する。`VtsEnvironmentProfile`をHAL serviceがruntime設定として読み込み、`CapabilitySnapshot`、frontend registry、backend probe結果、資源上限、公開API成功範囲を変更する経路は設けない。

### 6.4 地域入力からの受信候補解決と実機解決

`init`では具体的な受信チャンネルを必須入力にせず、地上波では住所、郵便番号、緯度経度から、実機確認を開始できる可能性が高い順に少数の受信候補を導出する。都道府県名だけでは地点ごとの受信可能性を順位付けできないため、県内全送信所の物理ch和集合へ拡大せずfail-closedで拒否する。

地上波候補の放送情報正本はINA4Nの公開地上デジタル中継局ページに固定する。checked-in `vts_channel_plan.japan.json` は県内channel unionを保持する静的snapshotではなく、`mode=live-ina4n` のsource descriptorとする。region resolverはGSIで入力地点の都道府県・市区町村を確定した後、その都道府県のINA4N周波数ページと送信所詳細ページから、その実行時点の送信所名、詳細URL、放送局別物理ch、偏波、出力、「主なカバーエリア」、所在地を送信所単位で取得する。`prefecture_channels`のような県内全送信所の物理ch和集合を通常候補生成の正本にしてはならない。

INA4Nで偏波または出力が空欄の場合はunknownのまま保持し、既定値を捏造しない。物理chが有効なのに出力・偏波だけがunknownという理由で送信所を候補datasetから削除してはならない。現行ISDB-T物理chを1件も持たない旧局等はVTS受信候補ではないため候補datasetから除外してよい。

送信所座標はINA4N詳細ページの地図リンクに埋め込まれた座標を第一選択とする。INA4Nに座標リンクがない場合は、同一局であることを人間が確認したA-PAB公開UIの局位置を`coordinate_overrides`へ明示して補完してよい。A-PABからcoverage polygon、物理ch、出力、偏波を取り込んではならない。A-PAB overrideがない場合はINA4N所在地文字列をGSIでgeocodeしてよい。これらでも座標が得られない場合、座標unknownのまま送信所を保持し、INA4Nの「主なカバーエリア」と市区町村が一致する場合はcoverage根拠だけで低優先候補に残してよい。

住所と郵便番号はまずGSIで緯度経度へ解決し、緯度経度入力はその座標を直接使用する。GSI reverse geocoderから得た都道府県・市区町村は、INA4Nから読み込む都道府県ページの選択と「主なカバーエリア」一致判定に使用する。住所文字列を直接INA4Nのarea文字列へsubstring照合してはならない。

候補順位は次の優先クラスに固定する。

1. INA4Nの「主なカバーエリア」が入力地点の市区町村と一致し、送信出力と座標の両方が既知なら、同群を`P / max(d, 0.1)^2`の降順にする。
2. coverage一致で座標は既知だが出力がunknownなら、既知出力群より後ろで距離昇順にする。
3. coverage一致だが座標がunknownなら、そのcoverage一致群のさらに後ろに残す。
4. coverage一致がない場合、出力と座標が既知の送信所を`P / max(d, 0.1)^2`の降順にする。
5. coverage一致がなく出力unknown・座標既知なら距離昇順とする。座標もunknownなら最後尾とする。

ここで`P[W]`はINA4N記載の送信出力、`d[km]`は入力地点から送信所座標までの大円距離である。`P/d^2`および距離fallbackは実機確認の探索順を決めるheuristicに限定し、受信電界強度、ERP/EIRP、terrain、建物、アンテナ高を含む受信可能性の証明またはcoverage polygonとして扱わない。

各送信所からは既知出力が最大のサービスをprobe frequencyとして1つ選ぶ。全サービスの出力がunknownならremote-control key、物理ch、サービス名で決定的に1サービスを選ぶ。同一frequencyは重複probeせず、根拠なく候補数を固定上限で切り捨てない。候補labelには送信所名、サービス名、および`coverage+inverse-square`、`coverage+distance-no-output`、`coverage-no-coordinate`、`inverse-square`、`distance-no-output`、`no-coordinate`のいずれかの探索根拠を残す。

`resolve-region`が生成するのはVTS実機確認を開始するための順位付き受信候補であり、service ID、PMT PID、audio/video/record PID等のTS内識別値を地域情報から推定して確定してはならない。実機接続後、`resolve-device`はpublic Tuner AIDLで候補frequencyをtuneし、LOCKEDを確認したTSからPATを取得してserviceとPMT PIDを解決し、PMTから要求flowに必要なES PIDを解決する。解決したfrequency、service、PAT/PMTに基づくPID等は同じ`VtsEnvironmentProfile`へ保存する。

要求flowを満たすservice候補が1件ならそのserviceを採用してよい。複数存在する場合は対話CLIで選択させるか、profileに明示されたselectorで決定する。非対話実行で複数候補が残りselectorがない場合はfail-closedとし、偶然の列挙順からserviceを選ばない。

具体的なfrontend/frequency/PIDからXMLを生成した後、VTS preflight失敗を理由に別候補へ自動fallbackして同じ生成物の意味を変えてはならない。別候補を採用する場合は`resolve-device`で同じprofileファイルの解決値を更新し、その更新後profileからXMLを再生成する。

### 6.5 build-time compiler / validator契約

VTS用静的XMLは手編集正本にせず、単一`VtsEnvironmentProfile`ファイルだけをprofile入力としてbuild-timeのcompiler / validatorで生成する。compiler / validatorは少なくとも次の順序でfail-closedに検証する。

1. profile自体のschema、必須項目、型、ID参照、および全profile fieldに6.2の消費経路があることを検証する。profileが保存可能でも、`../tuner_hal/DESIGN_JA.md`が静的XMLに要求する具体入力が未解決ならXML生成へ進めない。
2. `../tuner_hal/DESIGN_JA.md` が要求するVTS契約識別入力と、実際にbuild/testへ使用するAOSP Tuner VTS契約を照合する。一意に一致しない場合はXMLを生成・installしない。
3. profileのfrontend設定、flow、filter種別、DVR種別、PID、queue容量が`../tuner_hal/DESIGN_JA.md`で成功対応として認めた公開契約と矛盾しないことを検証する。
4. `tuner_hal2`の同一product capability正本から機械的に取得したread-only contractと照合し、VTSが要求するfilter / DVR個数、FMQ / processing bufferその他の静的資源claimが製品上限を超えないことを依存閉包単位で検証する。VTS合格のためにHAL側の能力値を上書きまたは縮退させてはならない。
5. 選択したAOSP Tuner VTS schemaで生成XMLを検証する。
6. `../tuner_hal/DESIGN_JA.md` のfilename解決契約に従い、選択したVTS loaderとvariant入力からinstall先を一意に解決する。

いずれかが失敗した場合は、推測値、既定PID、既定周波数、sample XML値、別profileへのfallbackで補完せず、VTS config artifactを成立させない。生成済みXMLを直接修正してvalidatorを迂回してはならない。

### 6.6 build-timeとdevice preflightの境界

build-time compiler / validatorは、静的なHAL product contractとAOSP VTS契約の整合を検証する。起動時probeで初めて確定するfrontendの実在性、公開frontend ID、hardware info、実信号のLOCKED到達、PID上の実データ到来はbuild-timeに捏造しない。

実機がある場合、`resolve-device`はprofileの未解決な受信候補とTS内識別値を具体化するためにpublic Tuner AIDLを使用する。具体値の解決後、VTS実行前には同じくpublic Tuner AIDLだけを使用するdevice preflightを行い、生成済みVTS構成が要求するfrontend種別、公開`FrontendInfo` / `DemuxCapabilities`、解決済み信号のLOCKED到達、対象PIDのdata pathが実機上で成立することを確認する。具体的な合否項目と実行手順は`タスク完了判定の実施方法.md`を正とし、本書では試験手順を二重定義しない。

`resolve-device`とpreflightはHAL内部registry、private diagnostic、driver-private stateをVTS成功条件の正本にしない。解決済みprofileに対するpreflight不成立時は別frontend、別周波数、別PIDへ自動fallbackして同じprofileの解決結果を変更せず、そのprofileによるVTS実行を開始しない。preflight結果またはVTS実行結果をHAL runtime capabilityへフィードバックして次回起動時の公開能力を変更してはならない。

#### VTS device agentの配置と実行

`resolve-device`で使用する`maleicacid_tuner_hal2_vts_agent`は通常productの`PRODUCT_PACKAGES`へ含めない。host CLIは明示されたagent binaryを一時的に`/data/local/tmp`へadb-pushして起動し、解決処理終了後に除去する。この通常経路によってproduct imageへtest helperを恒久配置しない。

対象deviceのSELinuxまたはlinker policyにより、shell domainから起動した一時agentがpublic Tuner AIDLへ接続できない場合は、同じagentを明示的なVTS/test imageだけへ含める`config/vts_test_agent_integration.mk`を使用する。このtest image経路を通常product integrationへ継承してはならない。

agentの論理責務・禁止責務、C++をFMQ descriptor import/read境界へ限定する規則、およびSI意味解析をhost側のcanonical `arib_si_engine_rs`へ接続する依存方向は`DESIGN_JA.md`を正とする。本書では配置・起動・除去・test imageへの接続だけを所有する。

### 6.7 生成物とproduct配置

生成されるAOSP Tuner VTS XMLはderived artifactであり、`VtsEnvironmentProfile`と同格の正本ではない。product integrationは、`../tuner_hal/DESIGN_JA.md`のfilename解決契約で得た解決済みfilenameを使用して、生成・検証済みXMLをproduct build graph経由でvendor imageへ正確に1個installする。`PRODUCT_COPY_FILES`、生成済みconfig moduleその他の具体的なbuild mechanismは、この一意なinstall契約を満たす限り実装詳細とする。

variantを使用する場合、variant propertyの値と生成XML filenameは同一`VtsEnvironmentProfile`の入力から導出し、product makefile側で別値を独立定義しない。variantを使用しない場合も、その判断は同じprofileから導出し、別のproduct設定面を設けない。

`config/product_integration.mk`は、`../tuner_hal/DESIGN_JA.md`のVTS状態契約に従って静的configをinstall可能と判定されたproductだけで、生成・検証済みVTS config artifactをvendor imageへのbuild graphへ接続できる構造にする。artifactを含めないproductでは、推測した既定XMLまたは旧`tuner_hal`のVTS XMLを代用しない。

### 6.8 統合完了条件への接続

本節は`VTS-STATE-BOUND`等の状態意味を追加定義しない。`../tuner_hal/DESIGN_JA.md`で静的VTS configをinstall可能と判定されたprofileについて、product integrationとしては次が成立していることを要求する。

- `VtsEnvironmentProfile`が実在する単一設定ファイルとして存在し、CLIがその同一ファイルを生成・更新する。
- 実機なしでも地域等から受信候補を同じprofileへ保存でき、未解決値を架空値で埋めない。
- 実機接続後はpublic Tuner AIDLとPAT/PMTによってfrequency / service / PID等を解決し、同じprofileへ保存できる。
- compilerはその単一profileファイルだけをprofile入力としてAOSP Tuner VTS XMLを生成する。
- profileの全永続フィールドがXML出力、XML値の決定、契約検証または生成物配置のいずれかに実際に消費され、未消費metadataがない。
- 環境依存値の人間編集入口が単一profileに集約され、生成XMLまたはproduct makefileに同じ値の独立正本がない。
- AOSP VTS契約とのbuild-time照合、HAL capability/resource contractとの静的照合、AOSP schema検証が自動化されている。
- 解決済みfilenameへのvendor image installがbuild graphに接続され、生成物が`tuner_hal2`のproduct integrationだけへ属する。
- VTS設定は`tuner_hal2`の試験設定にだけ使用され、HAL capability、frontend registry、backend probe結果または公開API成功範囲を書き換えない。
- device preflightとTuner VTS実行手順が`タスク完了判定の実施方法.md`から一意に実行できる。

これらが未接続の状態では、profileの値を手作業で複数箇所へ転記してVTS構成を成立させたことにしない。

## 7. section filter runtime契約の参照

`TableInfo repeat=false`を含むsection filterの公開意味、first-instance解決、停止条件、`repeat=true`との使い分け、未知の全instance集合の終端をHALが推測しない契約は`../tuner_hal/DESIGN_JA.md`を正とする。複数table instanceのinstance別完成・更新・寿命は`../arib_si_engine_rs/DESIGN_JA.md`の「複数table instanceの完成・更新・寿命」、操作ごとの必要instance集合と完成時の明示`stop()`は`../tis/DESIGN_JA.md`の「複数table instance収集と停止」を正とする。

本書が所有するのはproduct統合だけであり、VINTF/init/package/VTS設定の配置によって上記runtime契約を変更または再定義してはならない。

## 8. px4 device probe path契約

px4系device nodeのprobe prefixは本節をproduct integration上のSSOTとする。対象prefixは次のとおりである。

```text
/dev/px4video
/dev/pxmlt5video
/dev/pxmlt8video
/dev/isdb6014video
/dev/isdb2056video
/dev/pxm1urvideo
/dev/pxs1urvideo
/dev/isdbt2071video
```

このprefix集合を変更する場合は、次を同一変更で同期する。

- `tuner_hal2`のpx4 frontend probe adapterは本節のprefix集合だけを参照してdevice node候補を構成する。実装owner/anchorは`DESIGN_JA.md`のfrontend/backend実装ownerに従い、本書では別の実装ownerを設けない。
- `tuner_hal2/config/ueventd.tuner_hal2.rc`は同じdevice node集合のpermission entryを持つ。
- `tuner_hal2/sepolicy/file_contexts`その他のSELinux path設定で同device nodeを列挙する場合は、本節のprefix集合と一致させる。

probe adapter、ueventd、SELinux側のいずれかだけに別prefixを追加してはならない。具体device pathの正本を実装helper名やPR履歴へ置かず、本節から一方向に同期する。
