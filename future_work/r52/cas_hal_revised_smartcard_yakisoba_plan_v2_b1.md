# CAS HAL 実装計画 改訂版 v3
## AOSP Media CAS境界 + B25 SmartCard / Yakisoba / B1 SmartCard

## 0. 目的と設計原則

本計画は、日本向けデジタル放送の B25 / B1 系 CAS 処理を Android TV の標準 Media CAS / Tuner framework に統合するための設計である。

AOSP公開面には標準 `IMediaCasService` / `ICas` 契約だけを公開し、SmartCard、Yakisoba、credential取得、vendor-local IPC、KeySlotRegistry は vendor 内部へ閉じる。AOSPがvendor固有key ladder / secure service interfaceを標準化していないことを前提に、内部実装方式そのものをframework契約へ昇格させない。

固定する構成は次のとおりである。

```text
B25:
  smartcard_only
  yakisoba_only
  prefer_smartcard_then_yakisoba

B1:
  smartcard_only
```

`yakisoba_only` は B25 の正式な有効構成である。SmartCard実装の存在を `yakisoba_only` のadvertise条件にしない。

AOSP/VTS互換のClearKey pathはB25/B1 product capabilityから分離して同じ `IMediaCasService/default` 配下に保持し、B25/B1 profileの状態を理由にClearKeyを無効化しない。

---

## 1. AOSP公開面

### 1.1 service

```text
android.hardware.cas.IMediaCasService/default
```

service instance は1個だけ公開する。

```text
IMediaCasService/default
  ├─ ClearKey compatibility path
  ├─ B25 MaleicacidCasPlugin
  └─ B1  MaleicacidCasPlugin
```

SmartCard版/Yakisoba版を別serviceや同一CA system IDの別descriptorとして公開しない。backend差は `MaleicacidCasPlugin` 内部へ閉じる。

### 1.2 descriptor

```text
enumeratePlugins():
  - AOSP ClearKey descriptor
  - B25 "Maleicacid B25 CAS"   # B25 profile gate成立時のみ
  - B1  "Maleicacid B1 CAS"    # B1 gate成立時のみ
```

B25/B1のdescriptor集合は起動中にcard挿抜やdaemon healthで増減させない。descriptorはproduct/imageの能力、card/daemonの一時状態は利用時の結果として分離する。

### 1.3 unknown caSystemId

列挙されないCA system IDについてはAIDL transport自体を成功させ、次を返す。

```text
isSystemIdSupported(unknown)      -> false
isDescramblerSupported(unknown)   -> false
createPlugin(unknown)             -> null
createDescrambler(unknown)        -> null
```

unknown IDをservice-specific errorへ変換しない。

### 1.4 B25/B1 descrambler owner

B25/B1のCAS HAL自身はTS packetを復号しない。

```text
isDescramblerSupported(B25/B1) -> false
createDescrambler(B25/B1)      -> null
```

B25/B1 packet descramble ownerはTuner HALの `IDescrambler` だけとする。ClearKey compatibility pathはこの制約の対象外である。

---

## 2. immutable capability profile と advertise gate

### 2.1 profile ownership

profileの実ファイル配置、Soong/product組込み、SELinux、daemon同梱条件は `cas_hal/INTEGRATION.md` を正本とする。本書はruntimeが受け取る論理profileと意味だけを規定し、同じbuild統合仕様を複製しない。

runtimeはproduct imageで固定された capability snapshot をservice起動時に1回だけ読み、そのservice lifetime中は変更しない。runtime property、TIS入力、card挿抜、daemonの起動状態でprofileを書き換えない。

B25 profileは次のexactly-oneである。

```text
smartcard_only
yakisoba_only
prefer_smartcard_then_yakisoba
```

B1 profileは `smartcard_only` だけを許す。

profile欠落、不正、重複、build統合不整合では該当B25/B1 capabilityを広告しない。ClearKey capabilityは影響を受けない。

### 2.2 B25 common gate

どのB25 profileでも、次を満たしたproductだけがB25 descriptorを広告する。

```text
- ICas open/close/release lifecycle
- B25 ECM / EMM framework contract
- MediaCas session ID -> Tuner key token bridge
- complete MULTI2 context のatomic publish
- revoke / stale token拒否
- TIS -> MediaCas -> Tuner結合試験
```

### 2.3 profile別 B25 gate

`smartcard_only`:

```text
- B25 SmartCard adapter/path が実装済み
- card初期化応答からcredential contextを検証取得できる
- ECM / EMM、timeout、card抜去、closeを確認済み
```

`yakisoba_only`:

```text
- Yakisoba daemon/path が実装済み
- B25 ECM / EMMを処理できる
- 必要credential sourceとaccess controlがproductで固定済み
- bounded IPC、peer認証、timeout、daemon切断、closeを確認済み
- 採用libyakisoba revisionの配布条件を満たす
```

`prefer_smartcard_then_yakisoba`:

```text
- smartcard_only gateを満たす
- yakisoba_only gateを満たす
- backend bind判定とtimeout非fallbackを確認済み
```

したがって `yakisoba_only` build はSmartCard pathが未搭載でもB25を広告できる。ただしYakisoba側gateが成立していないimageはB25を広告しない。

### 2.4 B1 gate

```text
- B1SmartCardPath ECMが実装・検証済み
- B1 processEmm()が明示的unsupported
- YakisobaをB1に選択しない
- token publish / revoke / closeが成立
```

B1 EMM、EMM依存の通電制御情報取得、契約更新、権利更新は対応しない。

---

## 3. B25 backend binding

### 3.1 plugin-generation単位のowner

AOSP `ICas.processEmm()` はsession IDを引数に持たない。このためB25 backendをsessionごとに独立選択し、EMM送信先をactive session集合から推測する構成は採用しない。

各B25 `MaleicacidCasPlugin` generationは次のstateを1個だけ持つ。

```text
B25BackendBinding:
  Unbound
  SmartCard
  Yakisoba
```

一度 `SmartCard` または `Yakisoba` にcommitしたbindingはplugin releaseまで変更しない。全session、`processEmm()`、credential contextは同じbindingを使用する。

### 3.2 smartcard_only

plugin生成時の論理bindingは `SmartCard` とする。card不在やI/O障害はplugin capabilityの消滅ではなく、その操作の失敗として返す。同一pluginをYakisobaへ切り替えない。

### 3.3 yakisoba_only

plugin生成時の論理bindingは `Yakisoba` とする。SmartCard probeを行わず、SmartCardの存在を要求しない。daemon障害時に同一pluginをSmartCardへ切り替えない。

### 3.4 prefer_smartcard_then_yakisoba

pluginは `Unbound` で生成し、最初のbackend-dependent operationで1回だけbindingを決定する。backend-dependent operationは `openSession*()` と `processEmm()` である。

```text
SmartCard probe == CARD_VALID
  -> SmartCardへcommit

CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE
  -> Yakisobaへcommit

CARD_UNKNOWN_TIMEOUT
  -> operation失敗
  -> Unboundのまま
  -> Yakisobaへfallbackしない
```

`CARD_UNKNOWN_TIMEOUT` はbinding commit前の失敗なので、後続operationで再度probeできる。一度bindingがcommitされた後にbackendが故障しても、同じplugin generationでは別backendへfallbackしない。再選択が必要な場合は現在のpluginをreleaseし、新しいplugin generationを生成する。

`setPrivateData()` はbinding前でもplugin-local opaque stateとして保持できる。bindingをcommitするときは、保持済みprivate dataを選択backendへ適用してから最初のbackend-dependent operationを成功させる。

このplugin単位bindingにより `processEmm()` ownerは常に一意である。

---

## 4. plugin / session lifecycle

### 4.1 plugin state

```text
Live -> Releasing -> Released
```

`Releasing` 以降は新規session、ECM、EMM、private-data更新を受理しない。

### 4.2 session state

```text
Opening -> Active -> Closing -> Closed
                 \-> Failed -> Closing -> Closed
```

- `Opening`: no-reuse session ID、registry reservation、backend open、必要なprivate data適用をprepareする。全て成功した場合だけ `Active` とsession IDを公開する。
- `Active`: ECMとsession private dataを受理できる。
- `Failed`: backend outcome不明、fatal I/O、credential不整合等により通常処理を継続できない。新規ECMを拒否し、cleanupへ進む。
- `Closing`: 新規処理を遮断済み。registry revokeとbackend closeの未完了stepだけを再試行する。
- `Closed`: key resourceとbackend sessionのcleanup完了。

### 4.3 close / release

`closeSession()` は最初にlogical stateを `Closing` へcommitして新規処理を遮断する。その後、registryの新規resolveをrevokeし、backend closeを実行する。

revokeまたはbackend closeの一方だけ成功した場合、成功済みstepを繰り返さず、sessionを `Closing` に保持して失敗stepだけを次回 `closeSession()` / `release()` / service-owned cleanupで再試行する。

`closeSession()` が成功を返すのは必要cleanupが完了して `Closed` になった時点だけとする。

`release()` はpluginを `Releasing` へcommitし、全sessionについてcleanupを全件試行する。1sessionの失敗で残りを中断しない。全cleanup完了時だけ `Released` として成功する。途中失敗時は `Releasing` に留まり、後続 `release()` は未完了stepだけを再試行する。

Binder artifactが先に消滅しても、未完了cleanup stateはservice寿命のownerが保持する。cleanup triggerのthread/timer構成は実装詳細とし、本書では新規APIを要求しない。

---

## 5. ICas method契約

| method | B25 | B1 | 成功確定点 |
|---|---|---|---|
| `setPrivateData()` | 対応 | 対応 | plugin-local値と、binding済みならbackend値が同じ新値へcommitした時点。失敗時は旧値維持 |
| `setSessionPrivateData()` | 対応 | 対応 | Active sessionのbackend適用成功後にsession stateをcommitした時点。失敗時は旧値維持 |
| `openSessionDefault()` / `openSession()` | 対応 | 対応 | backend openとregistry reservationが成立し、Active session IDを公開した時点 |
| `processEcm()` | 対応 | 対応 | 完全な新key epochがregistryへatomic commitされ、同じsession IDで即resolve可能になった時点 |
| `processEmm()` | 対応 | 非対応 | pluginにbindされたB25 backendがEMM処理成功を返した時点。別backendへfallbackしない |
| `closeSession()` | 対応 | 対応 | token revokeとbackend session closeが完了した時点 |
| `release()` | 対応 | 対応 | plugin logical closeと全session cleanupが完了した時点 |
| `provision()` | 非対応 | 非対応 | `ERROR_CAS_CANNOT_HANDLE` |
| `refreshEntitlements()` | 非対応 | 非対応 | `ERROR_CAS_CANNOT_HANDLE` |
| vendor `sendEvent*` | 非対応 | 非対応 | vendor event番号を定義しない限り `ERROR_CAS_CANNOT_HANDLE` |

listener通知は上表のstate commit後、CAS/session/registry lockを保持せず実行する。listener失敗はcommit済みstateをrollbackせず、listener health/diagnosticsだけを更新する。callbackへECM/EMM本文、private data、key material、tokenを含めない。

---

## 6. error semantics

公開AIDL statusを内部errnoやtransport errorのまま露出しない。

```text
malformed input / section framing error
  -> AIDL BAD_VALUE

unsupported session intent / scrambling mode / B1 EMM
  -> ERROR_CAS_CANNOT_HANDLE

unknown / closed session
  -> ERROR_CAS_SESSION_NOT_OPENED

card absent
  -> ERROR_CAS_NO_CARD

card invalid / unsupported for selected system
  -> ERROR_CAS_CARD_INVALID

card response timeout / mute
  -> ERROR_CAS_CARD_MUTE

resource exhaustion / concurrent operation conflict
  -> ERROR_CAS_RESOURCE_BUSY

request送信後の結果不明、state corruption、commit outcome不明
  -> ERROR_CAS_INVALID_STATE

既知分類へ写像できない内部失敗
  -> ERROR_CAS_UNKNOWN
```

未実装を成功へ丸めない。

---

## 7. SmartCard path

SmartCard pathはcard deviceのprobe/reset、対象card識別、APDU生成・送受信、応答検証、B25 ECM/EMMまたはB1 ECMを所有する。

### 7.1 concurrency

同一physical cardへのI/Oは単一ownerが直列化する。card I/O lockを保持したままBinder listener callbackを呼ばない。

### 7.2 deadline / outcome

open/reset/APDUには有限deadlineを持たせる。

- request送信前に確定したcard不在・invalid・unsupported・I/O unavailableはtyped probe resultへ写像する。
- `prefer_smartcard_then_yakisoba` のbinding前probeでtimeoutした場合は `CARD_UNKNOWN_TIMEOUT` とし、Yakisobaへfallbackしない。
- backend binding後、ECM等のrequestをcardへ送信した後にtimeout/切断し、結果を確定できない場合、そのsessionを `Failed` としてtokenをrevokeする。同一pluginでYakisobaへ切り替えない。

### 7.3 credential

B25のsystem key / CBC初期値は、検証済みcard初期化応答から取得する。このためだけの外部secure store/factory provisioningを必須にしない。応答長、status、card種別を検証してから同じbackend generationのcredential contextとして採用する。

B1は採用するB1 protocol/参照実装の検証済み応答から供給元を確定し、B25応答配置を推測して流用しない。

---

## 8. Yakisoba path

### 8.1 boundary

CAS HALはlibyakisobaへ直接linkせず、別vendor daemonへvendor-local IPCでB25 ECM/EMMを要求する。B1 requestは明示的に拒否する。

AOSP公開AIDLにはYakisoba固有field、socket、status、credentialを追加しない。

### 8.2 IPC invariant

wire layoutの具体値はproduct統合正本へ置き、本書では次の不変条件だけを要求する。

```text
- protocol versionを検証する
- request IDを応答と照合する
- operationとB25 system identityを検証する
- session operationはsession/plugin generationでstale requestを区別する
- request/response sizeをboundedにする
- peer credential / SELinux domainを検証する
- connect / write / readを含む有限deadlineを持つ
- malformed / unknown / mismatched responseを成功へ丸めない
- raw key / ECM / EMM / session IDを通常logへ出さない
```

### 8.3 transport failure

request byteを1 byteも送っていないことが確定した接続失敗だけはoperation未開始として扱える。

1 byte以上送信した後のtimeout、切断、response破損、request ID不一致ではbackend側commit有無を確定できないため、自動再送や別backend fallbackを行わず outcome unknown とする。

- session openのoutcome unknownでは、同じsession identityに対するidempotent closeをcleanupとして試行し、session IDを公開しない。
- ECMのoutcome unknownではsessionを `Failed` とし、registry publishを行わずrevoke/closeへ進む。
- EMMのoutcome unknownでは `ERROR_CAS_INVALID_STATE` を返し、自動再送しない。plugin backend binding自体は変更しない。

Yakisoba daemonのcloseは、open response喪失後のcleanupを可能にするため、同じsession identityに対して未作成/既終了でもidempotentに扱える契約とする。

### 8.4 yakisoba_only

`yakisoba_only` ではSmartCard probeを一切要求しない。daemon/credentialが一時利用不能なら操作を失敗させるが、descriptor集合をruntimeで変更せず、SmartCardへfallbackしない。

---

## 9. KeySlotRegistry / token

B25の詳細契約は `future_work/r52/b25_key_slot_registry_contract.md` を正本とし、本書では重複定義しない。

B25/B1ともTunerへ渡す公開tokenは `MediaCas.Session.getSessionId()` bytesそのものとする。

```text
- 1..16 bytes
- opaque
- service process lifetime中no-reuse
- raw key materialを含まない
```

ECM成功前はunresolvedでよい。ECM成功時には完全なkey contextを同じsession IDから即resolve可能にする。close/release/fatal failureでは新規resolveをrevokeする。

具体的な `Reserve/Publish/Revoke` API名、TTL、slot上限、wire magic、retry回数はAOSP公開契約ではなく実装選択なので、本計画の必須設計にはしない。

---

## 10. TIS / Tunerとの境界

### 10.1 TIS

B25ではCA descriptor、ECM、EMMをTuner frameworkから取得し、B1ではECMだけをCASへ渡す。TISはCAS backend種別を解釈しない。

ECM成功後、同じMediaCas session ID bytesをTuner `IDescrambler.setKeyToken()`へ渡す。

### 10.2 Tuner HAL

Tuner HALはtokenを内部registryで解決し、PID linkageとTS payload-only MULTI2復号を担当する。ECM/EMM、card I/O、Yakisoba IPC、entitlement判断をTuner HALへ移さない。

### 10.3 teardown

MediaCas由来key tokenを使用した全descramblerについて `setKeyToken(VOID)` が成功した後にMediaCas sessionをcloseする。

`setKeyToken(VOID)` 成功を、そのdescramblerが当該tokenを新規packet処理に使用しないlinearization pointとする。追加のframework APIは導入しない。既取得内部key参照はregistry寿命管理でdrainし、最後の参照解放後にzeroizeする。

---

## 11. ライセンス / product integration

libyakisobaをimageへ同梱・改変する場合は採用revisionのGPL-3.0配布条件を満たす。daemon分離はprocess/権限境界を明確にするための設計であり、GPL義務を消す根拠にしない。

B1参照実装を移植・linkする場合は採用revisionのlicense条件を実装前に固定する。

partition、module名、socket path、wire field値、SELinux type、product makefile、third-party revision pinは `cas_hal/INTEGRATION.md` を正本とする。

---

## 12. 実装フェーズ

### Phase 1: public service contract

```text
- single IMediaCasService/default
- ClearKey compatibility
- unknown ID false/null contract
- B25/B1 descrambler owner分離
- immutable capability snapshot
- profile別advertise gate
```

### Phase 2: plugin lifecycle

```text
- plugin backend binding
- Opening/Active/Failed/Closing lifecycle
- method success point / error mapping
- close/release retry semantics
- listener post-commit semantics
```

### Phase 3: SmartCard

```text
- typed probe
- serialized bounded card I/O
- B25 credential initialization
- B25 ECM/EMM
- B1 ECM-only
```

### Phase 4: Yakisoba

```text
- yakisoba_only正式対応
- prefer profileでのplugin-level binding
- bounded/authenticated IPC
- outcome-unknown semantics
- idempotent cleanup close
```

### Phase 5: key bridge / Tuner

```text
- opaque session ID token
- ECM atomic publish
- revoke/ref drain/zeroize
- VOID token -> MediaCas close teardown
- payload-only MULTI2
```

### Phase 6: validation

```text
- ClearKey AOSP/VTS compatibility
- smartcard_only B25
- yakisoba_only B25
- prefer: SmartCard valid / known unavailable / timeout
- B1 ECM-only
- backend failure後に同pluginでcross-backend fallbackしない
- EMM ownerがplugin bindingと一致
- close/release partial failure retry
- listener failureでcommit済みstateをrollbackしない
```

---

## 13. 最終固定事項

```text
1. IMediaCasService/default は1個だけ公開する。
2. ClearKey compatibility pathをB25/B1 product capabilityから分離する。
3. B25は smartcard_only / yakisoba_only / prefer_smartcard_then_yakisoba を正式構成とする。
4. B1は smartcard_only ECM-only とする。
5. B25 advertise gateはprofile別に評価し、yakisoba_onlyへSmartCard完成を要求しない。
6. capability profileはproduct imageで固定し、service起動時snapshotからruntime中不変とする。
7. B25/B1 backend差をAOSP descriptorへ露出しない。
8. B25 backendはplugin generation単位で1回だけbindし、全sessionとEMMで共有する。
9. prefer profileのbinding前SmartCard timeoutではYakisobaへfallbackしない。
10. binding後backend failureでは同pluginを別backendへ切り替えない。
11. B25/B1 packet descramble ownerはTuner HALだけとする。
12. ICas methodごとのcommit pointとerror semanticsを固定する。
13. close/release partial failureでは成功済みcleanupを繰り返さず未完了stepだけを再試行する。
14. listener failureはcommit済みstateをrollbackしない。
15. SmartCard/Yakisoba I/Oはboundedとし、request送信後の結果不明を成功やfallbackへ丸めない。
16. Tuner key tokenはMediaCas session ID bytesそのものとし、service process lifetime中再利用しない。
17. processEcm()成功時点で完全なkey contextを同じsession IDからresolve可能にする。
18. MediaCas session close前にMediaCas由来tokenを全descramblerからVOID tokenで解除する。
19. raw key materialをBinder、TIS、通常logへ出さない。
20. CAS HALはTS demux / AV / DVRを担当しない。
```

---

## 14. 参考資料

- Android Media CAS / CAS framework
- Android Tuner framework
- AIDL `IMediaCasService` / `ICas`
- AOSP Tuner `Descrambler.setKeyToken()`
- `cas_hal/INTEGRATION.md`
- libyakisoba
- libaribb25 / B1参照実装
