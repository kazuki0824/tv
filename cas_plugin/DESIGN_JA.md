# Maleicacid CAS plugin 設計正本

本書は Maleicacid の CAS plugin に関する規範正本である。AOSP Media CAS service と vendor CasPlugin の境界、B25/B1 capability、plugin/session lifecycle、backend ownership、ECM/EMM処理、Tuner key bridge、token寿命、teardownを所有する。

製品全体のrelease到達点とmodule間責務は `../開発規則.md`、Tuner HAL公開契約は `../tuner_hal/DESIGN_JA.md`、TIS runtimeは `../tis/DESIGN_JA.md` を正とする。本書はそれらを再定義しない。

## 1. AOSP Media CAS service と Maleicacid plugin の境界

`android.hardware.cas.IMediaCasService/default` はAOSP標準 `MediaCasService` を使用する。Maleicacidは独自 `IMediaCasService/default` serviceを実装しない。

MaleicacidはAOSP Media CAS plugin ABIに従うvendor shared libraryを提供する。

```text
TIS / android.media.MediaCas
          |
          | AIDL ICas
          v
AOSP android.hardware.cas.IMediaCasService/default
          |
          | FactoryLoader / createCasFactory()
          v
Maleicacid B25 CasPlugin library
  |- extern "C" createCasFactory()
  |- MaleicacidB25CasFactory : android::CasFactory
  `- MaleicacidB25CasPlugin  : android::CasPlugin
       |- SessionTable
       |- backend binding
       |- YakisobaBackend
       |- SmartCardBackend
       `- Tuner key bridge
```

AOSP `MediaCasService` はplugin libraryのdiscovery/load、service-level plugin列挙・support query、AIDL `ICas` wrapper生成、listener bridgeを所有する。Maleicacid pluginはこれらを重複実装しない。

plugin entry point `createCasFactory()` はC linkageとする。返却する `android::CasFactory` と生成する `android::CasPlugin` はAOSP C++ virtual interfaceであるため、MaleicacidのAOSP plugin ABI境界はC++で実装する。

B25/B1のpacket descrambleはMedia CAS descramblerへ移さず、Tuner HAL `IDescrambler` が所有する。Maleicacid B25 pluginはMedia CAS `DescramblerFactory`を提供しない。

ClearKeyはAOSP標準compatibility pathのままとし、Maleicacid B25/B1 backend stateから分離する。

## 2. factory と plugin capability

`MaleicacidB25CasFactory` はB25 CA system IDのsupport判定、plugin descriptor query、B25 CasPlugin instance生成を所有する。

同一CA system IDについてSmartCard版とYakisoba版を別descriptorとして列挙しない。backend差は1個のB25 plugin内部へ閉じる。

最初にadvertiseするMaleicacid capabilityはB25 `yakisoba_only` とする。SmartCard未実装を `yakisoba_only` の成立条件にしない。

B25をadvertiseするには、採用profileについて次を満たす。

```text
共通:
  - AOSP CasPlugin lifecycle / status contract
  - complete ECM / EMM input contract
  - MediaCas session ID -> Tuner token bridge
  - complete MULTI2 material の atomic publish / stable-slot rotation
  - revoke / stale-token rejection
  - TIS -> MediaCas -> Tuner 結合確認
  - 採用するARIB STD-B25日本語原本の受信機能力条項の確認
  - product effective capacity が確認済み要求を満たすことの検証

yakisoba_only:
  - Yakisoba backend
  - B25 ECM / EMM
  - credential供給
  - bounded operation
  - secret lifetime
  - 採用統合形態に必要なaccess controlと配布条件

smartcard_only:
  - SmartCard backend
  - card初期化 / ECM / EMM
  - bounded I/O
  - card removal / close

prefer_smartcard_then_yakisoba:
  - 両backendの成立条件
  - plugin単位backend binding
  - SmartCard状態未確定時のnon-fallback
```

ARIB STD-B25の現行版判定と、その版の具体的な受信機能力値を本文で確認済みであることは分けて扱う。旧版英訳の具体値を未確認の現行日本語原本要求値として代用しない。

B1は `B1SmartCardBackend` のECM-only経路が実装・検証され、B1 advertise gateを満たした場合だけ同じAOSP plugin modelへ追加する。B1は `yakisoba_only` の成立条件ではない。

## 3. B25 CasPlugin責務

`MaleicacidB25CasPlugin` はAOSP `android::CasPlugin` 契約に従い、次を所有する。

```text
- setPrivateData()
- openSession()
- closeSession()
- setSessionPrivateData()
- processEcm()
- processEmm()
- sendEvent() / sendSessionEvent() の対応契約
- provision() / refreshEntitlements() の対応契約
- plugin/session lifecycle
- backend binding
- Tuner key bridge への publish / rotation / revoke
```

採用AOSP plugin ABIに存在しないvendor独自public methodを追加しない。公開面を拡張せず、対応できないoperationはAOSP既存statusへ写像する。

r52で成功対応するsession modeは `LIVE + MULTI2` とする。非対応intent/modeはstateを変更せず既存のcannot-handle相当statusを返す。

## 4. plugin / session lifecycle

論理stateは次を満たす。

```text
plugin:  Live -> Releasing -> Released
session: Opening -> Active -> Closing -> Closed
                         \-> Failed -> Closing -> Closed
```

具体的なclass名、mutex、thread、generation fieldは規定しない。必要な意味契約は次とする。

- Released pluginを再利用しない。
- Closed sessionに対するECM/private-data mutationを成功させない。
- close/releaseと外部backend処理が競合した場合、遅れて返った結果をClosed/Releasing stateへcommitしない。
- 古いECM completionが後から新しいcurrent key materialを上書きしない。
- listener notificationはstate commit後に行い、listener failureでcommit済みstateをrollbackしない。
- callbackからhalf-committed stateを観測可能にしない。
- method successは、そのmethodが公開上成立させるstateがcommit済みとなった時点だけ返す。

## 5. B25 backend ownership

YakisobaとSmartCardは別pluginではなく、B25 plugin instance内部のbackendとして扱う。

```text
B25Backend
  |- YakisobaBackend
  `- SmartCardBackend
```

backend種別は各B25 plugin instance内で一度だけ確定し、releaseまでそのpluginの全sessionとEMM処理で共有する。同じplugin instanceの途中でbackendを切り替えない。

product TISは同一B25 CA system IDについて1個のlive MediaCas/CAS pluginを共有し、そのpluginへB25 ECM sessionとEMMを配送する。AOSPが別clientによる独立plugin生成を許すことを理由に、HAL/service全体を横断するservice-global backend selectorを追加しない。

複数plugin instanceが同じphysical cardまたは同じYakisoba backend resourceを共有する実装では、その共有resource自身が必要なI/O/state orderingを提供する。backend種別の選択までplugin間で共有することを必須にしない。

### 5.1 profile

```text
smartcard_only:
  - pluginをSmartCard backendへbindする。
  - card不在等でYakisobaへ暗黙切替しない。

yakisoba_only:
  - pluginをYakisoba backendへbindする。
  - SmartCard probeを行わない。
  - Yakisoba障害でSmartCardへ暗黙切替しない。

prefer_smartcard_then_yakisoba:
  - pluginがUnboundのときだけSmartCardをprobeする。
  - CARD_VALIDならSmartCardへbindする。
  - CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLEならYakisobaへbindできる。
  - CARD_UNKNOWN_TIMEOUTではYakisobaへfallbackせず、bindingをUnboundのままにする。
```

build typeだけからbackendを暗黙決定しない。profileはproduct imageで固定したcapability設定から決定し、service lifetime中の一時healthやcard挿抜でdescriptor集合を変更しない。

## 6. 最初に成立させる `yakisoba_only`

最初に使用可能にするB25 profileは `yakisoba_only` とする。

最初の `YakisobaBackend` はB25 plugin library内のin-process backendとし、Soongで供給される `libyakisoba-cross` の `libyakisoba` C APIを使用する。

```text
MaleicacidB25CasPlugin (C++)
        |
        v
YakisobaBackend (C++)
        |
        | bcas_decodeECM() / bcas_decodeEMM()
        v
libyakisoba-cross / libyakisoba
```

Yakisoba backendはlibyakisobaの戻り値とkey materialをplugin lifecycle、AOSP status、key-publish契約へ正規化する。

`processEcm()` はECM入力をbackendへ渡し、成功時にodd/even Ksを含むcomplete MULTI2 materialを同じMediaCas session IDのstable slotへatomic commitする。commit前にsuccessを返さない。

`processEmm()` はplugin-wide backend mutationとして扱い、関連ECM処理がhalf-updated entitlement/work-key stateを観測しないorderingを提供する。

backend operationはcallerを無期限に占有しない。deadline、cancellation、worker等の具体方式は固定しない。request送信後に結果不明となったmutationを成功扱いしない。

raw key、ECM、EMM、credentialを通常log、TIS、AOSP公開AIDLへ露出しない。

Yakisobaを別vendor daemonへ分離する実装も禁止しない。daemonを採用する場合だけ、IPC schema互換性、request/response対応付け、size bound、access control、bounded I/O、stale request rejectionを満たす。daemon自体、明示version field、特定socket pathをAOSP要件として必須化しない。

## 7. SmartCard backend

SmartCardは同じB25 pluginへ追加するbackendとする。SmartCard追加によってAOSP service、factory、plugin ABI、TIS、Tuner token contractを変更しない。

SmartCard backendは次を所有する。

```text
- card reader検出
- card挿入状態確認
- reset / ATR
- card種別確認
- ARIB準拠card command生成・送受信
- response status分類
- card初期化応答から必要なB25 MULTI2 base materialを取得
- ECM
- EMM
- card removal / fatal invalidation
```

SmartCard I/Oは同一physical cardのstate mutation orderingを一意にし、open/reset/card command等がcallerを無期限に占有しない。結果不明operationをsuccessへ丸めない。

`prefer_smartcard_then_yakisoba` でSmartCard状態を規定時間内に確定できない場合は `CARD_UNKNOWN_TIMEOUT` とし、そのpluginをYakisobaへbindしない。

pluginがSmartCardへbind済みの状態でcard removalまたはfatal invalidationが発生した場合、影響sessionを失敗させ、新規key resolveを遮断する。同じpluginをYakisobaへ切り替えない。

## 8. B1

B1の正式対応はECM-onlyとする。Yakisoba backendはB1で使用しない。

B1 plugin advertise gateは次を満たす。

```text
- B1 SmartCard ECM処理を実装・検証済み
- processEmm()をunsupportedとして明示
- EMM依存のactivation/control information取得をunsupportedとして明示
- EMM依存の契約更新・権利更新をunsupportedとして明示
- B1でYakisoba backendを選択しない
- generic MULTI2 publish / rotation / revoke / closeを検証済み
```

TISはB1 sessionでEMM filterを起動せず、`MediaCas.processEmm()`を呼ばない。CATにEMM PIDがあってもB1復号開始条件・成功条件にしない。

B1の公開・移植可能な参照実装としては `libaribb1` 系の挙動を一次候補にする。コードを移植・リンクする場合はライセンス条件を実装前に確認する。

## 9. MediaCas session ID / Tuner key token

TIS向けに第二のvendor-private tokenを発行しない。MediaCas session ID bytesをそのままTuner key tokenとして使用する。

```text
Tuner key token:
  - MediaCas.Session.getSessionId() bytes
  - 1..16 bytes
  - opaque
  - raw keyを含めない
  - caSystemId / backend種別 / owner世代 / key更新番号を公開形式へ埋め込まない
```

MediaCas session IDはECM成功前でも存在してよい。その時点ではidentityだけを予約し、complete key materialがpublishされるまでTuner側resolveを成功させない。

session ID公開前に、live identityおよびstale linkage/retired referenceが残るidentityと衝突しないことを保証する。

process lifetime全体でtokenを永久に再利用しない実装を選んでもよいが必須ではない。必要なのは、stale tokenが別sessionのkey materialへ接続されないことである。

## 10. Tuner key resource

Tuner側がtoken解決後に必要とする最小の論理materialは次とする。

```text
Multi2KeyMaterial
  system_key
  cbc_initial_value
  even_ks
  odd_ks
```

CA system ID、MediaCas session identity、SmartCard/Yakisoba種別、CAS owner世代、鍵更新番号はCAS/plugin/key-bridge側の管理情報であり、`Multi2KeyMaterial` の必須fieldにしない。

stale owner/updateの排除にgeneration、cookie、connection identity、version counter等を内部実装として使用してよいが、それらをAOSP tokenまたはTuner key materialの必須形式にしない。

`IDescrambler.setKeyToken(token)` は、その時点のkey bytes snapshotへ固定する操作ではなくstable slot identityへlinkする操作とする。同じsessionの後続ECMでは同じstable slotのcurrent materialをatomicに差し替える。

既にpacket処理がmaterial参照を取得済みの場合は、その参照で処理を完了してよい。1 packet処理中に旧/new materialのfieldを混在させない。

## 11. key material ownership / mutation

- B25 base materialは当該pluginにbindされたbackendが所有する。
- odd/even KsはsessionのECM/backend処理結果として所有し、ECM成功時に同じstable slotのcurrent materialへ反映する。
- Tuner側registryへraw materialを運ぶ場合、current authorized CAS plugin ownerだけがpublish/rotate/revokeできるvendor内部境界を使用する。
- TISや一般appがkey mutation endpointへ到達できてはならない。
- owner handover後の旧接続、旧request、revoke済みtokenへのmutationをcurrent updateとして受理しない。
- access controlは採用process/IPC構成に応じてSELinux、socket ownership、peer credential、Binder identity等から必要な手段を選ぶ。不要な二重機構を必須化しない。
- raw keyの一時表現は必要期間を越えて保持・永続化しない。特定のzeroize APIやmemory primitiveを必須化しない。

## 12. processEcm() commit契約

`processEcm()` successのlinearization pointは、complete current `Multi2KeyMaterial` がregistryへcommit済みで、同じMediaCas session ID bytesのstable linkから直ちに取得可能になった時点とする。

commit前に失敗した場合は旧current materialを維持する。new materialをprepareしてから一括commitし、system key / CBC初期値 / odd/even Ksの新旧混在を観測させない。

同一sessionのmutating CAS operationを直列化するか、同等のstale-completion排除を行い、古いECM結果が後着してcurrent materialを巻き戻さないようにする。具体同期方式は実装詳細とする。

## 13. revoke / close / release

次の場合、新規key resolve/resource取得を遮断する。

```text
- session close
- plugin release
- current CAS owner loss
- credential revoke
- backend fatal failure
- registry corruption
```

revoke後に競合して既に取得済みの内部material参照は、そのpacket処理終了まで保持してよい。新規packet処理へ再取得させない。

tokenはrevoke、必要なdescramblerからのVOID unlink、既取得内部参照drainが完了するまで別sessionへ再割当てしない。

MediaCas由来tokenをTuner descramblerで使用した場合は、MediaCas sessionをcloseする前に、そのtokenを保持する全descramblerで `setKeyToken(VOID)` を成功させる。VOID成功を、そのdescramblerが以後の新規packet処理でtokenを使用しない確定点とする。

backend物理cleanupのretry/reset/taint方式はbackend resource ownerの実装詳細とし、service-global `CleanupPending` worker/tableを必須化しない。

## 14. CAS plugin / Tuner HAL / TIS責務

### CAS plugin

```text
- B25/B1 CA system supportをfactory経由で提供
- plugin/session lifecycle
- CA private data受領
- ECM
- B25 EMM
- backend binding
- complete MULTI2 material publish / rotation / revoke
- MediaCas session IDとstable key slotの対応維持
- CAS event/status
```

CAS pluginはTS demux、188-byte TS packet descramble、AV、DVRを担当しない。

### Tuner HAL

```text
- ITuner.openDescrambler()
- IDescrambler.setKeyToken()
- addPid() / removePid()
- token -> stable key slot linkage
- PID -> token mapping
- payload-only MULTI2 descramble
- scrambling-controlに基づくodd/even key選択
- descramble diagnostics
```

Tuner HALはB25/B1、SmartCard/Yakisoba、credential sourceを解釈して分岐しない。ECM/EMMやcard I/Oを担当しない。

### TIS

```text
- CA descriptorからcaSystemIdを決定
- caSystemIdごとに1個のlive MediaCas/CAS pluginを共有
- B25 PMT/CAT/ECM/EMM filter
- B1 PMT/ECM filter
- MediaCas / MediaCas.Session lifecycle
- B25 processEcm() / processEmm()
- B1 processEcm()
- MediaCas session ID bytesをTuner key tokenとしてsetKeyToken()へ渡す
- addPid()で対象PIDを接続
- MediaCas close前のVOID unlink
```

TISはraw key、card protocol、MULTI2 algorithmを解釈・保持しない。

## 15. error mapping

backend/internal errorは、判定できる場合AOSP CASの既存statusへ意味を保って写像する。専用意味が存在する状態をUNKNOWNへ潰さない。

unknown CA system IDのservice-level behaviorはAOSP標準MediaCasServiceに従う。Maleicacidは独自service semanticsを追加しない。

unsupported operationを空successにしない。

## 16. libyakisoba / GPL

`libyakisoba` はGPL-3.0として扱う。Android製品イメージや配布物へ `libyakisoba` または改変版を同梱する場合、適用される配布義務を満たす。

in-process direct linkを採用する場合は、その結合範囲へ適用されるGPL条件を確認し満たす。別daemonを採用する場合もdaemon側のGPL義務は残り、daemon化だけでGPL義務が消滅するとは扱わない。

libyakisoba改変版を配布する場合は、GPLv3条件に従って対応するソースを提供する。B1についてlibyakisobaを実装根拠にしない。

## 17. product integration方針

目標構成ではMaleicacid独自のCAS AIDL service binary、CAS service用VINTF fragment、CAS service用init rcを製品経路へ追加しない。

AOSP標準MediaCasServiceを製品で有効にし、Maleicacid B25 CasPlugin shared libraryをAOSP MediaCas plugin loaderが探索するvendor plugin配置へ組み込む。

plugin libraryは `createCasFactory()` をexportし、AOSP `media/cas/CasAPI.h` の `android::CasFactory` / `android::CasPlugin` ABIと整合させる。

最初の `yakisoba_only` 構成ではplugin libraryからSoong module `libyakisoba` を利用できるようdependencyを設定する。SmartCard componentを `yakisoba_only` のbuild/advertise条件にしない。

## 18. validation

B25 `yakisoba_only` の最低完了条件は次とする。

```text
- AOSP MediaCasServiceがdefault instanceとして起動する
- Maleicacid独自IMediaCasService serviceが製品経路に存在しない
- AOSP plugin loaderがMaleicacid createCasFactory()を発見する
- B25 descriptorが1個だけ列挙される
- B25 system ID support queryがtrue
- createPlugin(B25)がMaleicacid CasPluginをAIDL ICasとして返す
- ClearKey compatibility pathを破壊しない
- B25 Media CAS descramblerを追加しない
- packet descramble ownerがTuner HALのままである
- yakisoba_onlyではSmartCard probeが発生しない
- ECM/EMMがYakisoba backendへ到達する
- processEcm() success後に同じMediaCas session ID tokenからcomplete current materialを解決できる
- 後続ECMで同じstable slotのmaterialをatomic更新できる
- close/release後のlate resultがkey stateを復活させない
- MediaCas close前のVOID unlink / revoke契約を満たす
- raw key / ECM / EMM / credentialを通常logまたは公開AIDLへ露出しない
```

## 19. 実装順序

実装は次の順で成立させる。

```text
1. AOSP CasFactory / CasPlugin shared library skeleton
2. B25 plugin/session lifecycle
3. YakisobaBackend + libyakisoba-cross integration
4. yakisoba_only advertise / ECM / EMM
5. Tuner key bridgeとのend-to-end接続
6. SmartCardBackend
7. smartcard_only / prefer_smartcard_then_yakisoba
8. B1 plugin support
```

この順序により、SmartCard実装を待たずに `yakisoba_only` を最初のB25実動作経路として成立させる。

## 20. 参照

- AOSP `media/cas/CasAPI.h`
- AOSP CAS AIDL default `MediaCasService`
- AOSP CAS AIDL default `FactoryLoader`
- AOSP Tuner `Descrambler` API
- ARIB STD-B25
- `libyakisoba-cross`
- `libaribb25` / `libaribb1` はSmartCard挙動・B1挙動・MULTI2等の参照に使用し、製品B25の一体型TS→TS pipelineとしてリンクしない。
