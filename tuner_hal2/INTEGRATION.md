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

### 6.1 VTS環境profileの配置正本と依存方向

対象productでTuner VTSを有効にする場合、人間が編集するVTS環境依存値は、`../tuner_hal/DESIGN_JA.md` が要求する入力集合を表現するproductごとの単一`VtsEnvironmentProfile`ファイルに集約する。このファイルをproduct integration上の唯一の人間編集入口とし、1回のVTS構成生成で複数のprofileファイル、product makefileの個別変数、生成済みXMLの手編集値を合成して1個の論理profileを作ってはならない。profileの具体的なファイル形式と物理pathは実装時に一意に固定し、生成XMLと同格の第二正本を追加しない。

Tuner HALのcapability、公開個数、FMQ/PES/AV/DVR/worker等の製品資源上限、frontend probe結果を`VtsEnvironmentProfile`の独立した規範値として複製してはならない。これらは`../tuner_hal/DESIGN_JA.md`の`ProductProfile` / `CapabilitySnapshot`と`tuner_hal2`の実機probeを正本とする。profile compilerが静的照合に必要とするHAL側情報は、同じ正本から機械生成したread-only capability contractとして入力してよい。生成contractは人間編集対象にせず、HALのruntime能力を変更する入力にも使用しない。

依存方向は次に固定する。

```text
interactive VTS profile CLI
        |
        v
VtsEnvironmentProfile
        |
        +--> regional candidate resolver
        |
        +--> device resolver through public Tuner AIDL
        |
        v
VTS profile compiler / validator
        |
        +--> selected AOSP VTS schema / loader contract
        |
        +--> generated read-only tuner_hal2 capability contract
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

逆方向に、VTS XML、variant property、VTS profileまたはVTS test resultから`tuner_hal2`の`CapabilitySnapshot`、frontend registry、backend probe結果、公開API成功範囲を変更してはならない。VTS設定は被試験対象の能力を選択・拡張・縮小する設定面ではなく、既に公開可能と判定された能力を試験するための環境記述である。

### 6.2 対話CLIによるprofile生成とtuner_hal2限定target

VTS環境profileには、実機が接続されていない開発環境でも作成・保存できる対話CLIを設ける。CLIの論理操作は少なくとも次を持つ。

```text
init            対話入力から単一VtsEnvironmentProfileを新規作成して保存する
resolve-region  profileの地域入力から受信候補集合を決定論的に更新する
resolve-device  public Tuner AIDLと受信TSを使って実機依存値を検証・解決する
compile         解決済みprofileを検証してAOSP Tuner VTS XMLを生成する
```

`init` は実機接続を前提にしない。AOSP/VTS契約識別、対象backend/product、受信方式、明示入力または地域入力、要求するVTS flow、queue要求等、入力時点で確定できる値を対話的に取得し、未確定項目を架空値で埋めずにprofileを保存する。必要入力が揃っていないprofileは `../tuner_hal/DESIGN_JA.md` の `VTS-STATE-UNBOUND` 判定に従い、保存可能であっても静的VTS XMLをinstall可能とは扱わない。

CLIと生成profileのtargetはproduct defaultである`tuner_hal2`に固定する。profile compiler、生成XML module、variant設定、vendor imageへの配置は`tuner_hal2`のproduct integrationだけへ接続し、旧`tuner_hal`の`profiles/`、`tools/render_vts_config.py`、`config/tuner_vts_config_*`、旧service packageを更新・参照・fallback先にしてはならない。旧`tuner_hal`に存在するprofile rendererは設計参考として読めても、このCLIの実行対象または生成先にはしない。

ここでいう`tuner_hal2`への反映は、`tuner_hal2`を被試験HALとするVTS構成を生成・配置することだけを意味する。`VtsEnvironmentProfile`をHAL serviceがruntime設定として読み込み、`CapabilitySnapshot`、frontend registry、backend probe結果、資源上限、公開API成功範囲を変更する経路は設けない。

### 6.3 地域入力からの受信候補解決

`init`では具体的な受信チャンネルを必須入力にせず、地上波では地域指定から候補を導出できる。地域入力は少なくとも郵便番号、住所、緯度経度のいずれかを表現可能とし、市区町村等の粗い入力で複数候補が残る場合は候補集合を維持して一意の周波数を捏造しない。

地域resolverは、版・取得元・内容hashを識別できるversionedな放送エリア/チャンネル計画datasetを入力とする。同じprofile入力と同じdataset版からは同じ順序の候補集合を生成し、profileには地域入力、dataset識別情報、候補生成結果のprovenanceを保存する。ネットワーク上の最新値を識別子なしで毎回直接参照してprofileの意味を変えてはならない。

地上波の地域resolverが生成してよいのは、送信所または受信エリアに対応するdelivery system、物理チャンネル、frequency等の**受信候補**である。候補に含まれることを、その地点・アンテナ・配線・tunerで実際に受信可能である証明として扱わない。BS/110度CS等、地域による送信周波数候補の選択を必要としない方式では、地域情報を周波数選択の擬似根拠にせず、versionedな対象transport候補表から候補を構成する。

service ID、PMT PID、audio/video/record PID等のTS内識別値を地域情報から推定して確定してはならない。これらがVTS flowに必要な場合、`resolve-device`は候補frontend/frequencyへpublic Tuner AIDLでtuneし、LOCKEDを確認したTSからPATを取得してserviceとPMT PIDを解決し、PMTから要求flowに必要なES PIDを解決する。serviceを自動選択する場合は、要求flowを満たすservice集合に対する決定論的な選択規則をprofileに保持し、CLI実行ごとの偶然の列挙順で別serviceへ変えない。

地域由来の候補集合がまだ具体的なVTS入力へ解決されていない段階では、`resolve-device`がその候補集合を決定論的な順序で評価してよい。候補評価はprofileを具体化するための解決処理であり、受信成立したcandidateとPAT/PMT由来値をprofileのderived resolutionとして原子的に更新する。一度具体的なfrontend/frequency/PIDが解決され、compilerがその値からXMLを生成した後は、VTS preflight失敗を理由に別候補へ自動fallbackして同じ生成物の意味を変えてはならない。別候補を採用する場合は`resolve-device`を再実行してderived resolutionを更新し、その新しい解決結果からXMLを再生成する。

profile内のderived resolutionは人間が独立編集する第二SSOTにしない。地域入力、dataset版、device-resolution入力または受信TSが変わった場合はCLIが再生成し、compilerはderived resolutionのsource fingerprint/provenanceがprofileの現在入力と一致しない場合に拒否する。

### 6.4 build-time compiler / validator契約

VTS用静的XMLは手編集正本にせず、選択済み`VtsEnvironmentProfile`からbuild-timeのcompiler / validatorで生成する。compiler / validatorは少なくとも次の順序でfail-closedに検証する。

1. profile自体のschema、必須項目、型、ID参照、derived resolutionのprovenanceを検証する。profileが保存可能でも、`../tuner_hal/DESIGN_JA.md`が静的XMLに要求する具体入力が未解決ならXML生成へ進めない。
2. `../tuner_hal/DESIGN_JA.md` が要求するVTS契約識別入力と、実際にbuild/testへ使用するAOSP Tuner VTS契約を照合する。一意に一致しない場合はXMLを生成・installしない。
3. profileのfrontend設定、flow、filter種別、DVR種別、PID、queue容量が`../tuner_hal/DESIGN_JA.md`で成功対応として認めた公開契約と矛盾しないことを検証する。
4. tuner_hal2の同一product contractから機械生成したread-only capability contractと照合し、VTSが要求するfilter / DVR個数、FMQ / processing bufferその他の静的資源claimが製品上限を超えないことを依存閉包単位で検証する。VTS合格のためにHAL側の能力値を上書きまたは縮退させてはならない。
5. 選択したAOSP Tuner VTS schemaで生成XMLを検証する。
6. `../tuner_hal/DESIGN_JA.md` のfilename解決契約に従い、選択したVTS loaderとvariant入力からinstall先を一意に解決する。

いずれかが失敗した場合は、推測値、既定PID、既定周波数、sample XML値、別profileへのfallbackで補完せず、VTS config artifactを成立させない。生成済みXMLを直接修正してvalidatorを迂回してはならない。

### 6.5 build-timeとdevice preflightの境界

build-time compiler / validatorは、静的なHAL product contractとAOSP VTS契約の整合を検証する。起動時probeで初めて確定するfrontendの実在性、公開frontend ID、hardware info、実信号のLOCKED到達、PID上の実データ到来はbuild-timeに捏造しない。

実機がある場合、`resolve-device`はprofileの未解決な受信候補とTS内識別値を具体化するためにpublic Tuner AIDLを使用する。具体値の解決後、VTS実行前には同じくpublic Tuner AIDLだけを使用するdevice preflightを行い、生成済みVTS構成が要求するfrontend種別、公開`FrontendInfo` / `DemuxCapabilities`、解決済み信号のLOCKED到達、対象PIDのdata pathが実機上で成立することを確認する。具体的な合否項目と実行手順は`タスク完了判定の実施方法.md`を正とし、本書では試験手順を二重定義しない。

`resolve-device`とpreflightはHAL内部registry、private diagnostic、driver-private stateをVTS成功条件の正本にしない。解決済みprofileに対するpreflight不成立時は別frontend、別周波数、別PIDへ自動fallbackして同じprofileの解決結果を変更せず、そのprofileによるVTS実行を開始しない。preflight結果またはVTS実行結果をHAL runtime capabilityへフィードバックして次回起動時の公開能力を変更してはならない。

### 6.6 生成物とproduct配置

生成されるAOSP Tuner VTS XMLはderived artifactであり、`VtsEnvironmentProfile`と同格の正本ではない。product integrationは、`../tuner_hal/DESIGN_JA.md`のfilename解決契約で得た解決済みfilenameを使用して、生成XMLをvendor imageへ正確に1個installする。

variantを使用する場合、variant propertyの値と生成XML filenameは同一`VtsEnvironmentProfile`の入力から導出し、product makefile側で別値を独立定義しない。variantを使用しない場合も、その判断は同じprofileから導出し、別のproduct設定面を設けない。

`config/product_integration.mk`は、`../tuner_hal/DESIGN_JA.md`のVTS状態契約に従って静的configをinstall可能と判定されたproductだけで生成VTS config moduleを`PRODUCT_PACKAGES`へ追加できる構造にする。VTS config moduleを含めないproductでは、推測した既定XMLまたは旧`tuner_hal`のVTS XMLを代用しない。

### 6.7 統合完了条件への接続

本節は`VTS-STATE-BOUND`等の状態意味を追加定義しない。`../tuner_hal/DESIGN_JA.md`で静的VTS configをinstall可能と判定されたprofileについて、product integrationとしては次が成立していることを要求する。

- 実機がなくても対話CLIでprofileを保存でき、未解決値を架空値で埋めない。
- 地域入力を使用するprofileでは、versioned datasetから候補集合が決定論的に生成され、実受信可能性とTS内PIDを地域情報だけで確定しない。
- 実機接続後はpublic Tuner AIDLとPAT/PMTによって必要な具体値を解決でき、derived resolutionのprovenanceがprofile入力へ結び付いている。
- 環境依存値の人間編集入口が単一profileに集約され、生成XMLまたはproduct makefileに同じ値の独立正本がない。
- AOSP VTS契約とのbuild-time照合、HAL capability/resource contractとの静的照合、AOSP schema検証が自動化されている。
- 解決済みfilenameへのvendor image installがbuild graphに接続され、生成物が`tuner_hal2`のproduct integrationだけへ属する。
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