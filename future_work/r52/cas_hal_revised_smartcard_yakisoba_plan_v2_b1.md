# CAS HAL 実装計画 改訂版 v3
## AOSP Media CAS境界 + B25 SmartCard / Yakisoba / B1 SmartCard

## 0. 設計原則

AOSP公開面には標準 `IMediaCasService` / `ICas` だけを公開し、SmartCard、Yakisoba、credential取得、vendor IPC、KeySlotRegistry はvendor内部へ閉じる。AOSPが規定しない内部transport、slot table、retry方式を公開HAL契約へ昇格させない。

B25正式構成は `smartcard_only` / `yakisoba_only` / `prefer_smartcard_then_yakisoba`。B1は `smartcard_only` ECM-only。`yakisoba_only` は有効なB25構成でありSmartCard実装をadvertise条件にしない。

ClearKeyはB25/B1 stateから分離したAOSP reference-compatible pathとして同じ `IMediaCasService/default` に合成する。Maleicacid固有profile/backend/key stateをClearKeyへ混在させない。

B25の規範は採用productで固定するARIB STD-B25日本語原本とする。本設計ではVersion 7.0を対象revisionとし、同時処理可能なスクランブル鍵/PID等の数値規定を本書へ複製せず、同revisionの該当条項をcapability gateのSSOTとして参照する。

## 1. AOSP公開契約

`android.hardware.cas.IMediaCasService/default` を1個だけ公開し、同一CA system IDをbackend別に重複列挙しない。

`enumeratePlugins()`、`isSystemIdSupported()`、`createPlugin()` は同じimmutable capability snapshotを使用し、同一service lifetime中に互いに矛盾する能力を返さない。

列挙されないsystem IDはAIDL transport成功のまま `isSystemIdSupported=false`、`isDescramblerSupported=false`、`createPlugin=null`、`createDescrambler=null` とする。

B25/B1はMedia CAS側descramblerを公開せず、packet descrambleはTuner HAL `IDescrambler`だけが所有する。ClearKeyはAOSP/VTS互換plugin/session/descrambler semanticsに従う。

採用AIDLにdefault-session methodが存在する場合、そのB25/B1既定sessionは `LIVE + MULTI2` とする。明示 `openSession(intent, mode)` は本製品が成功対応する `LIVE + MULTI2` だけを受理し、それ以外は状態不変のまま `ERROR_CAS_CANNOT_HANDLE` とする。AIDL revisionに存在しないmethodをvendor独自AIDLとして追加しない。

## 2. capability profile / advertise gate

capability profileはvendor imageで固定し、service起動時に1回だけsnapshot化する。runtime property、TIS入力、card挿抜、daemon healthで変更しない。入力のpath/serializationは内部実装だが、欠落・不正・重複・未知値をstrictに検出し、該当B25/B1 capabilityだけを非広告にする。ClearKeyには影響させない。

B25共通gate:

```text
- ICas lifecycle / AOSP status contract
- ECM / EMM complete-section input contract
- MediaCas session ID -> Tuner token bridge
- complete MULTI2 context atomic publish / stable slot rotation
- revoke / stale token拒否
- ARIB STD-B25 Version 7.0 の同時key/PID処理能力条項を満たす
- TIS -> MediaCas -> Tuner結合確認
```

内部slot/PID table上限は実装詳細でよいが、B25を広告するproductのeffective capacityはARIB条項を下回ってはならない。

profile別gate:

- `smartcard_only`: SmartCard path、credential初期化、ECM/EMM、timeout、card抜去、close。
- `yakisoba_only`: Yakisoba daemon/path、ECM/EMM、credential/access control、bounded authenticated IPC、timeout/切断/close、配布条件。
- `prefer_smartcard_then_yakisoba`: 上記両gate + backend bind判定 + timeout非fallback。

したがって `yakisoba_only` build はSmartCard未搭載でもB25を広告できる。Yakisoba側gate未成立imageはB25を広告しない。

B1 gateはB1 SmartCard ECM、B1 EMM明示拒否、Yakisoba非選択、generic MULTI2 key publish/rotation/revoke、closeの確認とする。

## 3. B25 backend binding / plugin-wide ordering

`ICas.processEmm()` はsession IDを持たないため、backendをsession単位に選択しない。各B25 plugin generationは `Unbound | SmartCard | Yakisoba` のbindingを1個だけ所有し、commit後はreleaseまで変更しない。全session、EMM、credential contextは同じbackendを使う。

binding選択はplugin ownerがatomicに直列化する。同時に複数の最初のbackend-dependent operationが到達してもprobe/bind transactionは1個だけ実行し、他operationはそのcommit結果または同じ失敗結果を観測する。半端なbindingを公開しない。

- `smartcard_only`: plugin生成時にSmartCardへbind。card不在等は操作失敗としYakisobaへ切り替えない。
- `yakisoba_only`: plugin生成時にYakisobaへbind。SmartCard probeを行わずdaemon障害時もSmartCardへ切り替えない。
- `prefer_smartcard_then_yakisoba`: 最初の `openSession*()` または `processEmm()` で1回だけ判定する。

```text
CARD_VALID                                      -> SmartCard
CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED
/ CARD_IO_UNAVAILABLE                           -> Yakisoba
CARD_UNKNOWN_TIMEOUT                            -> operation失敗、Unbound維持
```

binding前timeoutではfallbackしない。binding後backend failureでも同plugin内でcross-backend fallbackしない。再判定にはpluginをreleaseして新plugin generationを作る。

`setPrivateData()`、backend binding、`processEmm()` はplugin-global stateとのorderingを持つ。binding transactionはprivate-data versionをsnapshotし選択backendへ適用してからcommitする。binding/private-data update競合後もbackendとplugin-local値を同じcommitted versionにする。

backendはEMMによるentitlement/work-key更新とECM処理の間にlinearizableなorderingを提供する。SmartCardではphysical card I/O serializationで、Yakisobaではdaemon内部state synchronizationで、ECMが半端なEMM更新stateを観測しないようにする。特定のthread/queue方式は規定しない。

plugin/session/provider generationはwrap/reuseしない。次generationを安全に発行できない場合は新plugin/sessionを公開せずfail-closedとする。

## 4. lifecycle / concurrency / cleanup

plugin state:

```text
Live -> Releasing -> Released
```

session state:

```text
Opening -> Active -> Closing -> Closed
                 \-> Failed -> Closing -> Closed
```

`Closing` / `Releasing` はcallerから見て既に通常利用不能であり、key revoke確定待ちを表す。backend物理cleanupだけが残る場合は `Closed` / `Released` へ進め、別のservice-owned `CleanupPending` として保持できる。

`Opening` ではno-reuse session ID、registry reservation、backend open、private data適用をprepareし、全て成功した時だけActive session IDを公開する。

各sessionはmutating backend I/Oを1件だけin-flightにする。外部I/O開始前にsession generation/lifecycleをsnapshotし、lockを外してI/Oし、応答後に同じgenerationがActiveであることを再検証してからprivate data/key epochをcommitする。

close/releaseがI/O中に到達した場合は先にClosing/Releasingへ遷移して新規I/Oを遮断する。進行中I/Oのownerが結果を回収するまで内部recordを保持し、Closing/Releasing後に遅れて返ったECM/key結果をregistryへpublishしない。

`processEmm()` はplugin-wide operationなので同一pluginで1件だけin-flightにする。release後に遅れて返った結果を新しいlive stateへ反映しない。

### 4.1 closeSession

最初の `closeSession()` はsessionをClosingへ遷移させ、以後の通常session operationを拒否する。tokenの新規resolve revokeを確定できたらcaller-visible sessionをClosedへ進める。

- revoke未確定: close成功にしない。Closingを保持し、同じsessionへの後続 `closeSession()` またはservice-owned cleanupがrevokeを再試行する。
- revoke確定後のbackend close失敗: caller-visible sessionはClosedのまま。backend cleanupだけを `CleanupPending` として再試行し、sessionをActiveへ戻さない。
- Closed到達後の通常session operationは `ERROR_CAS_SESSION_NOT_OPENED`。

close成功確定点はLogical close + token新規resolve revokeであり、backend物理close完了をBinder成功の必須条件にしない。ただしbackend closeは毎回全件試行し、失敗を診断/cleanup ownerへ引き渡す。

### 4.2 release

最初の `release()` はpluginをReleasingへ遷移させ、新規method/callback deliveryを遮断し、全session tokenの新規resolve revokeを試行する。

- token revoke未確定entryが残る: release成功にせずReleasingを保持し、後続 `release()` / service-owned cleanupでrevokeを再試行する。
- 全token revoke確定: Releasedへ進み、backend close失敗が残っていてもcaller-visible objectを再live化しない。物理cleanupはservice-owned `CleanupPending` で継続する。
- Released後の `release()` はidempotentに成功してよい。その他通常methodは `ERROR_CAS_INVALID_STATE`。

AOSP referenceのrelease同様、backend recovery完了までBinder objectをliveに保つ設計にはしない。一方、外部key registryを持つ本構成ではsecurity boundaryであるtoken revokeだけはrelease成功前に確定させる。

Binder artifact消滅後もReleasing/CleanupPending stateはservice寿命ownerが保持するが、worker/timer構成は実装詳細とする。

live plugin/sessionとReleasing/CleanupPending ownershipの総量は有限にboundする。具体的な数値上限はproduct capacityとARIB gateを満たす実装詳細とするが、新しいplugin/sessionを受理すると上限を超える場合はbackend mutation前に `ERROR_CAS_RESOURCE_BUSY` として拒否し、cleanup失敗による無制限なstate増加を許さない。

## 5. ICas method / input / error contract

method成功確定点:

- `setPrivateData()`: plugin-local値と、binding済みならbackend値のcommit完了。失敗時は旧値維持。
- `setSessionPrivateData()`: backend成功後、generation/lifecycle再検証を通ってsession stateをcommit。失敗時は旧値維持。
- `openSession*()`: backend open + registry reservation後にActive session ID公開。
- `processEcm()`: backend応答後のgeneration/lifecycle再検証を通り、完全な新epochをstable slotへatomic publishし、既link descramblerを含め同session IDから新epoch取得可能。
- `processEmm()`: bind済みB25 backendが成功し、release競合がないことを再検証。別backendへfallbackしない。
- `closeSession()`: Closing→Closedに必要なtoken revoke確定。backend cleanupを全件起動済み。
- `release()`: Releasing→Releasedに必要な全token revoke確定。backend cleanupを全件起動済み。

ECM/EMMは完全なsection byte sequenceとしてCASへ渡す。TS packet、PID、demux bufferをCAS HALへ渡さない。empty、section framing/declared length不整合、対象CA systemとして処理不能な外形はbackend I/O前に拒否する。

B1 `processEmm()`、B25/B1 `provision()`、`refreshEntitlements()`、未定義vendor eventは `ERROR_CAS_CANNOT_HANDLE` とする。

error mapping:

```text
malformed input                           -> AIDL BAD_VALUE
unsupported mode / B1 EMM                -> ERROR_CAS_CANNOT_HANDLE
unknown / Closed session                 -> ERROR_CAS_SESSION_NOT_OPENED
entitlement/keyなし                      -> ERROR_CAS_NO_LICENSE
期限切れentitlement/key                  -> ERROR_CAS_LICENSE_EXPIRED
必要credential/provisioning未成立        -> ERROR_CAS_NOT_PROVISIONED
card absent                              -> ERROR_CAS_NO_CARD
card invalid/unsupported                 -> ERROR_CAS_CARD_INVALID
card timeout/mute                        -> ERROR_CAS_CARD_MUTE
resource/concurrency exhaustion          -> ERROR_CAS_RESOURCE_BUSY
送信後結果不明 / state corruption        -> ERROR_CAS_INVALID_STATE
その他の未知内部失敗                     -> ERROR_CAS_UNKNOWN
```

backendがAOSP専用statusを判定できる場合は `ERROR_CAS_DEVICE_REVOKED`、`ERROR_CAS_NEED_ACTIVATION`、`ERROR_CAS_NEED_PAIRING` 等へ対応付けUNKNOWNへ潰さない。未実装を成功へ丸めない。

listenerはstate commit後かつ内部lock外で呼ぶ。listener failureでcommit済みstateをrollbackしない。現設計ではvendor scheme event番号を定義しないため、診断目的だけで `onEvent()` / `onSessionEvent()` を合成しない。将来AOSP listener methodを使用する場合はそのAIDL引数契約をそのまま守り、`onSessionEvent()` では正規のsession IDを渡す。ECM/EMM本文、private data、raw/prepared key materialをlistener dataへ含めない。

## 6. SmartCard path

同一physical cardへのI/Oは単一ownerが直列化し、card I/O lock中にBinder callbackを呼ばない。open/reset/APDUは有限deadlineを持つ。

binding前probe timeoutは `CARD_UNKNOWN_TIMEOUT` としてfallbackしない。binding後にrequestを送信して結果不明となったsessionはFailedとして新規処理を遮断し、token revoke/closeへ進む。同pluginでYakisobaへ切り替えない。

bind済みSmartCardの抜去または恒久的card invalidationを検出した場合、そのcard/backend generationに依存するActive sessionをFailedへ遷移させ、新規resolveをrevokeする。同plugin内でYakisobaへ切り替えない。後続の新sessionを同じSmartCard bindingで受理する場合は、card probe/reset/credential初期化を再実行して新sessionとして成立させる。

B25 system key/CBC初期値は検証済みcard初期化応答から取得する。B1は採用B1 protocolの検証済み応答から供給元を確定し、B25配置を推測して流用しない。

## 7. Yakisoba path

CAS HALはlibyakisobaへ直接linkせず、vendor partition内の別daemonへB25 ECM/EMMをvendor-local IPCで要求する。B1 requestは拒否する。daemon endpoint/credentialを一般app、TIS、Tuner HAL、shellへ公開しない。

IPCはversion/operation/B25 identity検証、request ID照合、session/plugin generationによるstale判定、bounded frame、peer credential/SELinux domain検証、connect/write/readを含むdeadline、malformed/mismatched response拒否、秘密情報の通常log禁止を満たす。wire byte layout/magic値は内部実装とし、CAS serviceとdaemonで共通定義を使う。

送信0 byteが確定した失敗だけをoperation未開始として扱う。1 byte以上送信後のtimeout/切断/response不整合はoutcome unknownとして自動再送・別backend fallbackをしない。

- open outcome unknown: 同じsession identityをidempotent closeしsession IDを公開しない。
- ECM outcome unknown: sessionをFailed、registry publishなし、revoke/close。
- EMM outcome unknown: `ERROR_CAS_INVALID_STATE`、自動再送なし、binding維持。

Yakisoba closeは同じsession identityについて未作成/終了済みでもidempotentに扱う。`yakisoba_only` はSmartCard probeを行わず、daemon/credential一時利用不能時もdescriptor集合を変えず操作失敗とする。

Yakisoba daemonのdeath/restartを検出した場合、旧daemon incarnationに依存するActive sessionをFailedへ遷移させ、新規resolveをrevokeする。同plugin内でSmartCardへ切り替えない。後続の新sessionを同じYakisoba bindingで受理する場合は新daemon incarnationとのhandshakeとcommitted plugin private dataの再適用を完了してからopenする。旧sessionを新daemon incarnationへ引き継がない。

Yakisobaから受領したkey materialはregistry commitに必要な最短寿命だけ保持し、一時response/encode bufferはcommitまたは失敗後にzeroizeする。

## 8. KeySlotRegistry / Tuner boundary

B25鍵詳細は `future_work/r52/b25_key_slot_registry_contract.md` を正本とする。B1もTuner側では同じgeneric MULTI2 stable-slot resourceとtoken lifetime/revoke規則を再利用し、B1 protocol/credential意味はCAS側adapterに閉じる。Tuner HALへB25/B1識別を要求しない。

B25/B1公開Tuner tokenは `MediaCas.Session.getSessionId()` bytesそのものとし、1..16 bytes、opaque、raw key非包含、service process lifetime中no-reuseとする。ECM成功前はunresolvedでよく、ECM成功時はcomplete current epochを同session IDのstable linkから取得可能にする。close/release/provider death/fatal failureでは新規resource取得をrevokeする。

TISはbackend種別を解釈しない。Tuner HALはtoken→stable slot linkage、current resource取得、PID linkage、TS payload-only MULTI2だけを担当する。

MediaCas由来tokenを保持する全descramblerで `setKeyToken(VOID)` が成功した後にMediaCas sessionをcloseする。VOID成功を新規packet利用停止のlinearization pointとし、既取得内部key参照はdrain後zeroizeする。追加framework APIは導入しない。

具体的な `Reserve/Publish/Revoke` API名、TTL、slot上限、wire magic、retry回数は内部実装選択であり必須設計にしない。ただしeffective capacityは第2節ARIB gateを満たす。

## 9. product integration / license

CAS service、SmartCard adapter、Yakisoba daemon、Tuner key bridgeはvendor partition内に置き、各processへ必要なSELinux permissionだけを与える。SmartCard adapterを別privilege domainに置く構成では、CAS serviceへcard reader device accessを直接付与せずadapterだけへ最小権限を与える。

- `yakisoba_only`: Yakisoba daemon + credential + sepolicy + license成果物。SmartCard adapter不要。
- `smartcard_only`: SmartCard adapter。Yakisoba daemon不要。
- `prefer_smartcard_then_yakisoba`: 両backend。

libyakisobaを同梱・改変する場合は採用revisionのGPL-3.0配布条件を満たす。daemon分離をGPL義務消滅の根拠にしない。B1参照実装を移植/linkする場合は採用revisionのlicense条件を固定する。

module名、socket path、wire field値等の内部名称はAOSP公開契約にせず採用実装内で一意に定義する。

## 10. validation

```text
- ClearKey AOSP/VTS compatibility
- capability enumerate/support/createの同一snapshot整合
- unknown ID false/null
- B25/B1 explicit session: LIVE+MULTI2成功、非対応組合せ拒否
- smartcard_only B25 / yakisoba_only B25 / prefer各probe結果
- concurrent first-bind / setPrivateData競合
- EMM updateとECMのlinearizable ordering
- B1 ECM-only + generic stable-slot rotation
- AOSP status mapping / complete section validation
- binding後cross-backend fallbackなし
- close/releaseとin-flight ECM/EMM競合、late publishなし
- revoke失敗時Closing/Releasing維持
- revoke成功後backend close失敗時はClosed/Released維持 + cleanup retry
- bounded admission / cleanup accumulation
- release idempotence / post-release invalid-state
- AOSP listener sessionId契約 / secret非露出
- SmartCard抜去時session revoke
- Yakisoba daemon restart時、旧session revoke + 新session再初期化
- listener failureでcommit済みstate非rollback
- Yakisoba outcome-unknown / temporary-key zeroize
- provider death revoke
- ARIB STD-B25 Version 7.0の同時key/PID能力gate
- stable link上の連続ECM key rotation
- token revoke/ref drain/zeroize
- VOID token -> MediaCas close
```

## 11. 最終固定事項

1. `IMediaCasService/default` は1個だけ公開し、capability query/createは同一snapshotを使う。
2. ClearKeyをB25/B1能力から分離する。
3. B25は3profileを正式構成とし、`yakisoba_only` のadvertiseにSmartCard完成を要求しない。
4. B1はSmartCard ECM-onlyとする。
5. capability profileはimage固定・service lifetime中不変とする。
6. backend差をAOSP descriptorへ露出しない。
7. B25 backendはplugin generation単位でatomicに一度だけbindし全session/EMMで共有する。
8. binding前SmartCard timeoutではfallbackしない。
9. binding後backend failureでは同pluginを別backendへ切り替えない。
10. packet descrambleはTuner HALだけが所有する。
11. generationをwrap/reuseせず、mutating I/O後にgeneration/lifecycleを再検証してからcommitする。
12. close/releaseと競合した遅延I/O結果をpublishしない。
13. key revoke未確定とbackend物理cleanup失敗を区別し、revoke済みobject/sessionをcleanup失敗で再live化しない。
14. live/cleanup ownershipをboundedにし、上限超過前にRESOURCE_BUSYでadmissionを拒否する。
15. listener failureでcommit済みstateをrollbackせず、AOSP listenerの必須引数契約は狭めない。
16. backend I/Oはboundedとし送信後結果不明を成功/fallbackへ丸めない。
17. Tuner tokenはMediaCas session ID bytesでservice process lifetime中再利用しない。
18. provider/backend deathでは影響sessionの新規resource取得を遮断する。
19. `processEcm()`成功時にstable linkからcomplete current epochを取得可能にする。
20. MediaCas close前にMediaCas由来tokenを全descramblerからVOIDで解除する。
21. raw key materialをBinder、TIS、通常logへ出さない。
22. B25 advertise時のeffective key/PID capacityはARIB STD-B25 Version 7.0の対象条項を満たす。
23. CAS HALはTS demux / AV / DVRを担当しない。
