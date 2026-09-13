# CAS HAL 実装計画 改訂版 v3
## AOSP Media CAS境界 + B25 SmartCard / Yakisoba / B1 SmartCard

## 0. 設計原則
AOSP公開面には標準 `IMediaCasService` / `ICas` だけを公開し、SmartCard、Yakisoba、credential取得、vendor IPC、KeySlotRegistry はvendor内部へ閉じる。

B25正式構成は `smartcard_only` / `yakisoba_only` / `prefer_smartcard_then_yakisoba`。B1は `smartcard_only` ECM-only。`yakisoba_only` は有効構成でありSmartCard実装をadvertise条件にしない。ClearKeyはB25/B1能力から独立させる。

## 1. AOSP公開契約
`android.hardware.cas.IMediaCasService/default` を1個だけ公開し、同一CA system IDをbackend別に重複列挙しない。

`enumeratePlugins()`、`isSystemIdSupported()`、`createPlugin()` は同じimmutable capability snapshotを使用し、同一service lifetime中に互いに矛盾する能力を返さない。

列挙されないsystem IDはAIDL transport成功のまま `isSystemIdSupported=false`、`isDescramblerSupported=false`、`createPlugin=null`、`createDescrambler=null` とする。

B25/B1はMedia CAS側descramblerを公開せず、packet descrambleはTuner HAL `IDescrambler`だけが所有する。ClearKeyはAOSP/VTS互換pathとして別扱いする。

## 2. capability profile / advertise gate
capability profileはvendor imageで固定し、service起動時に1回だけsnapshot化する。runtime property、TIS入力、card挿抜、daemon healthで変更しない。入力のpath/serializationは内部実装だが、欠落・不正・重複・未知値をstrictに検出し、該当B25/B1 capabilityだけを非広告にする。ClearKeyには影響させない。

B25共通gateは ICas lifecycle、ECM/EMM contract、MediaCas session ID→Tuner token bridge、complete MULTI2 context atomic publish、revoke/stale token拒否、TIS→MediaCas→Tuner結合確認。

profile別gate:
- `smartcard_only`: SmartCard path、credential初期化、ECM/EMM、timeout、card抜去、close。
- `yakisoba_only`: Yakisoba daemon/path、ECM/EMM、credential/access control、bounded authenticated IPC、timeout/切断/close、配布条件。
- `prefer_smartcard_then_yakisoba`: 上記両gate + backend bind判定 + timeout非fallback。

したがって `yakisoba_only` build はSmartCard未搭載でもB25を広告できる。

B1 gateはB1 SmartCard ECM、B1 EMM明示拒否、Yakisoba非選択、token publish/revoke/close。

## 3. B25 backend binding
`ICas.processEmm()` はsession IDを持たないため、backendをsession単位に選択しない。各B25 plugin generationは `Unbound | SmartCard | Yakisoba` のbindingを1個だけ所有し、commit後はreleaseまで変更しない。全session、EMM、credential contextは同じbackendを使う。

binding選択はplugin ownerがatomicに直列化する。同時に複数の最初のbackend-dependent operationが到達してもprobe/bind transactionは1個だけ実行し、他operationはそのcommit結果または同じ失敗結果を観測する。半端なbindingを公開しない。

- `smartcard_only`: 生成時にSmartCardへbind。card不在等は操作失敗。Yakisobaへ切替えない。
- `yakisoba_only`: 生成時にYakisobaへbind。SmartCard probeを行わない。daemon障害時もSmartCardへ切替えない。
- `prefer_smartcard_then_yakisoba`: 最初の `openSession*()` または `processEmm()` で1回だけ判定する。`CARD_VALID -> SmartCard`、`CARD_ABSENT/INVALID/UNSUPPORTED/IO_UNAVAILABLE -> Yakisoba`、`CARD_UNKNOWN_TIMEOUT -> 失敗してUnbound維持`。

binding前timeoutではfallbackしない。binding後backend failureでも同plugin内でcross-backend fallbackしない。再判定にはpluginをreleaseして新plugin generationを作る。

`setPrivateData()` はbinding前ならplugin-localに保持する。binding transactionは同じplugin ownerの下でprivate-data snapshotを取り、選択backendへ適用してからbindingをcommitする。bindingと同時にprivate data updateが競合しても、成功時にbackendとplugin-local値が同じversionになるようにする。

## 4. lifecycle / commit semantics
pluginは `Live -> Releasing -> Released`。sessionは `Opening -> Active -> Closing -> Closed`、fatal/outcome-unknown時は `Active -> Failed -> Closing -> Closed`。

`openSessionDefault()` は本製品の既定 `LIVE + MULTI2` sessionを開く。`openSession(intent, mode)` は `LIVE + MULTI2` だけを受理し、それ以外は状態不変のまま `ERROR_CAS_CANNOT_HANDLE` とする。

`Opening` ではno-reuse session ID、registry reservation、backend open、private data適用をprepareし、全て成功した時だけActive session IDを公開する。

各sessionはmutating backend I/Oを1件だけin-flightにする。外部I/O開始前にsession generation/lifecycleをsnapshotし、lockを外してI/Oし、応答後に同じgenerationがActiveであることを再検証してからprivate data/key epochをcommitする。

`closeSession()` / `release()` がI/O中に到達した場合は先にClosing/Releasingをcommitして新規I/Oを遮断する。進行中I/Oのownerが結果を回収するまでsession stateを破棄しない。I/O応答後の再検証でClosing/Releasing/別generationを検出した場合、遅延結果をstateやregistryへpublishせずcleanupへ引き渡す。これによりclose後に遅れて返ったECM keyを復活させない。

`processEmm()` はplugin-wide operationなので同一pluginで1件だけin-flightにし、releaseはその結果回収後にReleasedへ進む。

`closeSession()` はregistry revokeとbackend closeを行う。部分失敗時は成功済みstepを繰り返さず、未完了stepだけを次回close/release/service-owned cleanupで再試行する。成功はClosed到達時だけ返す。

`release()` は全session cleanupを全件試行し、全完了時だけReleasedとして成功する。Binder artifact消滅後も未完了cleanup stateはservice ownerが保持するが、worker/timer構成は実装詳細とする。

method成功確定点:
- `setPrivateData()`: plugin-local値と、binding済みならbackend値のcommit完了。
- `setSessionPrivateData()`: backend成功後、generation/lifecycle再検証を通ってsession stateをcommit。
- `openSession*()`: backend open + registry reservation後にActive session ID公開。
- `processEcm()`: backend応答後のgeneration/lifecycle再検証を通り、完全な新epochをatomic publishして同session IDから即resolve可能。
- `processEmm()`: bind済みB25 backendが成功し、release競合がないことを再検証。別backendへfallbackしない。
- `closeSession()`: revoke + backend close完了。
- `release()`: 全in-flight結果回収 + 全session cleanup完了。

B1 `processEmm()`、B25/B1 `provision()`、`refreshEntitlements()`、未定義vendor eventは `ERROR_CAS_CANNOT_HANDLE`。

listenerはstate commit後かつ内部lock外で呼ぶ。listener failureでcommit済みstateをrollbackしない。listener/callbackにはECM/EMM本文、private data、session token、raw/prepared key materialを含めない。

## 5. error mapping
`malformed input -> AIDL BAD_VALUE`、`unsupported mode/B1 EMM -> ERROR_CAS_CANNOT_HANDLE`、`unknown/closed session -> ERROR_CAS_SESSION_NOT_OPENED`、`entitlement/keyなし -> ERROR_CAS_NO_LICENSE`、`期限切れentitlement/key -> ERROR_CAS_LICENSE_EXPIRED`、`必要credential/provisioning未成立 -> ERROR_CAS_NOT_PROVISIONED`、`card absent -> ERROR_CAS_NO_CARD`、`card invalid/unsupported -> ERROR_CAS_CARD_INVALID`、`card timeout/mute -> ERROR_CAS_CARD_MUTE`、`resource/concurrency exhaustion -> ERROR_CAS_RESOURCE_BUSY`、`送信後結果不明/state corruption -> ERROR_CAS_INVALID_STATE`、その他未知内部失敗のみ `ERROR_CAS_UNKNOWN`。未実装を成功へ丸めない。

backendがAOSPで専用statusを持つ状態を判定できる場合は `ERROR_CAS_DEVICE_REVOKED`、`ERROR_CAS_NEED_ACTIVATION`、`ERROR_CAS_NEED_PAIRING` 等へ対応付け、`ERROR_CAS_UNKNOWN` へ潰さない。

## 6. SmartCard path
同一physical cardへのI/Oは単一ownerが直列化し、card I/O lock中にBinder callbackを呼ばない。open/reset/APDUは有限deadlineを持つ。

binding前probe timeoutは `CARD_UNKNOWN_TIMEOUT` としてfallbackしない。binding後にrequestを送信して結果不明となったsessionはFailedへ遷移しtoken revoke/closeへ進む。同pluginでYakisobaへ切替えない。

B25 system key/CBC初期値は検証済みcard初期化応答から取得する。B1は採用B1 protocolの検証済み応答から供給元を確定し、B25配置を推測して流用しない。

## 7. Yakisoba path
CAS HALはlibyakisobaへ直接linkせず、vendor partition内の別daemonへB25 ECM/EMMをvendor-local IPCで要求する。B1 requestは拒否する。daemon endpoint/credentialを一般app、TIS、Tuner HAL、shellへ公開しない。

IPCはversion/operation/B25 identity検証、request ID照合、session/plugin generationによるstale判定、bounded frame、peer credential/SELinux domain検証、connect/write/readを含むdeadline、malformed/mismatched response拒否、秘密情報の通常log禁止を満たす。wire byte layout/magic値は内部実装とし、CAS serviceとdaemonで共通定義を使う。

送信0 byteが確定した失敗だけをoperation未開始として扱う。1 byte以上送信後のtimeout/切断/response不整合はoutcome unknownとして自動再送・別backend fallbackをしない。

- open outcome unknown: 同じsession identityをidempotent closeしsession IDを公開しない。
- ECM outcome unknown: sessionをFailed、registry publishなし、revoke/close。
- EMM outcome unknown: `ERROR_CAS_INVALID_STATE`、自動再送なし、binding維持。

Yakisoba closeは同じsession identityについて未作成/終了済みでもidempotentに扱う。`yakisoba_only` はSmartCard probeを行わず、daemon/credential一時利用不能時もdescriptor集合を変えず操作失敗とする。

Yakisobaから受領したkey materialはregistry commitに必要な最短寿命だけ保持し、一時response/encode bufferはcommitまたは失敗後にzeroizeする。

## 8. KeySlotRegistry / Tuner boundary
B25鍵詳細は `future_work/r52/b25_key_slot_registry_contract.md` を正本とする。

B25/B1公開Tuner tokenは `MediaCas.Session.getSessionId()` bytesそのものとし、1..16 bytes、opaque、raw key非包含、service process lifetime中no-reuseとする。ECM成功前はunresolvedでよく、ECM成功時は完全contextを同session IDから即resolve可能にする。close/release/provider death/fatal failureでは新規resolveをrevokeする。

TISはbackend種別を解釈しない。Tuner HALはtoken resolve、PID linkage、TS payload-only MULTI2だけを担当する。

MediaCas由来tokenを保持する全descramblerで `setKeyToken(VOID)` が成功した後にMediaCas sessionをcloseする。VOID成功を新規packet利用停止のlinearization pointとし、既取得内部key参照はdrain後zeroizeする。追加framework APIは導入しない。

具体的な `Reserve/Publish/Revoke` API名、TTL、slot上限、wire magic、retry回数は内部実装選択であり必須設計にしない。

## 9. product integration / license
CAS service、SmartCard adapter、Yakisoba daemon、Tuner key bridgeはvendor partition内に置き、各processへ必要なSELinux permissionだけを与える。

- `yakisoba_only`: Yakisoba daemon + credential + sepolicy + license成果物。SmartCard adapter不要。
- `smartcard_only`: SmartCard adapter。Yakisoba daemon不要。
- `prefer_smartcard_then_yakisoba`: 両backend。

libyakisobaを同梱・改変する場合は採用revisionのGPL-3.0配布条件を満たす。daemon分離をGPL義務消滅の根拠にしない。B1参照実装を移植/linkする場合は採用revisionのlicense条件を固定する。

module名、socket path、wire field値等の内部名称はAOSP公開契約にせず、採用実装内で一意に定義する。

## 10. validation
ClearKey AOSP/VTS、同一snapshotによるenumerate/support/create整合、unknown ID false/null、LIVE+MULTI2以外の拒否、smartcard_only B25、yakisoba_only B25、preferのSmartCard valid/known unavailable/timeout、concurrent first-bind、B1 ECM-only、AOSP status mapping、EMM owner==plugin binding、binding後cross-backend fallbackなし、close/releaseとin-flight ECM競合、close/release partial failure retry、listener failure非rollback、Yakisoba outcome-unknown/temporary-key zeroize、provider death revoke、token revoke/ref drain/zeroize、VOID token→MediaCas closeを確認する。

## 11. 最終固定事項
1. `IMediaCasService/default` は1個だけ公開し、capability query/createは同一snapshotを使う。
2. ClearKeyをB25/B1能力から分離する。
3. B25は3profileを正式構成とし、`yakisoba_only` のadvertiseにSmartCard完成を要求しない。
4. B1はSmartCard ECM-onlyとする。
5. capability profileはimage固定・service lifetime中不変とする。
6. backend差をAOSP descriptorへ露出しない。
7. B25 backendはplugin generation単位でatomicに一度だけbindし全session/EMMで共有する。
8. binding前SmartCard timeoutではfallbackしない。
9. binding後backend failureでは同pluginを別backendへ切替えない。
10. packet descrambleはTuner HALだけが所有する。
11. mutating session I/Oは1件だけin-flightにし、外部I/O後にgeneration/lifecycleを再検証してからcommitする。
12. close/releaseと競合した遅延I/O結果をpublishしない。
13. cleanup部分失敗は未完了stepだけ再試行する。
14. listener failureでcommit済みstateをrollbackしない。
15. backend I/Oはboundedとし送信後結果不明を成功/fallbackへ丸めない。
16. Tuner tokenはMediaCas session ID bytesでservice process lifetime中再利用しない。
17. provider deathではそのincarnationの新規resolveを遮断する。
18. `processEcm()`成功時に完全contextを即resolve可能にする。
19. MediaCas close前にMediaCas由来tokenを全descramblerからVOIDで解除する。
20. raw key materialをBinder、TIS、通常logへ出さない。
21. CAS HALはTS demux / AV / DVRを担当しない。
