# CAS HAL 実装計画 改訂版 v3
## AOSP Media CAS境界 + B25 SmartCard / Yakisoba / B1 SmartCard

## 0. 設計原則

AOSP公開面には標準 `IMediaCasService` / `ICas` だけを公開し、SmartCard、Yakisoba、credential取得、vendor IPC、KeySlotRegistry はvendor内部へ閉じる。AOSPが規定しない内部transport、slot table、retry方式を公開HAL契約へ昇格させない。

B25正式構成は次の3つとする。

```text
smartcard_only
yakisoba_only
prefer_smartcard_then_yakisoba
```

B1は `smartcard_only` のECM-onlyとする。`yakisoba_only` は有効なB25構成であり、SmartCard実装をadvertise条件にしない。ClearKey compatibility pathはB25/B1 product capabilityから独立させる。

B25の規範は採用productで固定する現行ARIB STD-B25日本語原本とする。本設計ではVersion 7.0を対象revisionとし、同時処理可能なスクランブル鍵/PID等の数値規定を文書内へ複製せず、同revisionの該当条項をcapability gateのSSOTとして参照する。

## 1. AOSP公開契約

`android.hardware.cas.IMediaCasService/default` を1個だけ公開し、同一CA system IDをbackend別に重複列挙しない。

`enumeratePlugins()`、`isSystemIdSupported()`、`createPlugin()` は同じimmutable capability snapshotを使用し、同一service lifetime中に互いに矛盾する能力を返さない。

列挙されないsystem IDはAIDL transport成功のまま次を返す。

```text
isSystemIdSupported      -> false
isDescramblerSupported   -> false
createPlugin             -> null
createDescrambler        -> null
```

B25/B1はMedia CAS側descramblerを公開せず、packet descrambleはTuner HAL `IDescrambler`だけが所有する。ClearKeyはAOSP/VTS互換pathとして別扱いする。

採用AIDLにdefault-session methodが存在する場合、そのB25/B1既定sessionは `LIVE + MULTI2` とする。明示 `openSession(intent, mode)` は本製品が成功対応する `LIVE + MULTI2` だけを受理し、その他は状態不変のまま `ERROR_CAS_CANNOT_HANDLE` とする。AIDL revisionに存在しないmethodをvendor独自AIDLとして追加しない。

## 2. capability profile / advertise gate

capability profileはvendor imageで固定し、service起動時に1回だけsnapshot化する。runtime property、TIS入力、card挿抜、daemon healthで変更しない。入力のpath/serializationは内部実装だが、欠落・不正・重複・未知値をstrictに検出し、該当B25/B1 capabilityだけを非広告にする。ClearKeyには影響させない。

B25共通gate:

```text
- ICas lifecycle / status contract
- ECM / EMM section input contract
- MediaCas session ID -> Tuner token bridge
- complete MULTI2 context atomic publish
- revoke / stale token拒否
- ARIB STD-B25 Version 7.0 の同時key/PID処理能力条項を満たす
- TIS -> MediaCas -> Tuner結合確認
```

内部slot上限やPID table上限は実装詳細でよいが、B25を広告するproductのeffective capacityは上記ARIB条項を下回ってはならない。

profile別gate:

- `smartcard_only`: SmartCard path、credential初期化、ECM/EMM、timeout、card抜去、close。
- `yakisoba_only`: Yakisoba daemon/path、ECM/EMM、credential/access control、bounded authenticated IPC、timeout/切断/close、配布条件。
- `prefer_smartcard_then_yakisoba`: 上記両gate + backend bind判定 + timeout非fallback。

したがって `yakisoba_only` build はSmartCard未搭載でもB25を広告できる。Yakisoba側gateが成立していないimageはB25を広告しない。

B1 gateはB1 SmartCard ECM、B1 EMM明示拒否、Yakisoba非選択、token publish/revoke/closeの確認とする。

## 3. B25 backend binding

`ICas.processEmm()` はsession IDを持たないため、backendをsession単位に選択しない。各B25 plugin generationは次のbindingを1個だけ所有する。

```text
Unbound | SmartCard | Yakisoba
```

binding commit後はplugin releaseまで変更しない。全session、EMM、credential contextは同じbackendを使う。

binding選択はplugin ownerがatomicに直列化する。同時に複数の最初のbackend-dependent operationが到達してもprobe/bind transactionは1個だけ実行し、他operationはそのcommit結果または同じ失敗結果を観測する。半端なbindingを公開しない。

- `smartcard_only`: plugin生成時にSmartCardへbind。card不在等は操作失敗とし、Yakisobaへ切り替えない。
- `yakisoba_only`: plugin生成時にYakisobaへbind。SmartCard probeを行わず、daemon障害時もSmartCardへ切り替えない。
- `prefer_smartcard_then_yakisoba`: 最初の `openSession*()` または `processEmm()` で1回だけ判定する。

```text
CARD_VALID                                      -> SmartCard
CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED
/ CARD_IO_UNAVAILABLE                           -> Yakisoba
CARD_UNKNOWN_TIMEOUT                            -> operation失敗、Unbound維持
```

binding前timeoutではfallbackしない。binding後backend failureでも同plugin内でcross-backend fallbackしない。再判定にはpluginをreleaseして新plugin generationを作る。

`setPrivateData()` はbinding前ならplugin-localに保持する。binding transactionは同じplugin ownerの下でprivate-data versionをsnapshotし、選択backendへ適用してからbindingをcommitする。bindingとprivate-data updateが競合しても、成功時にbackendとplugin-local値が同じversionになるよう直列化する。

plugin/session/provider generationはwrap/reuseしない。次generationを安全に発行できない場合は新plugin/sessionを公開せずfail-closedとする。

## 4. lifecycle / concurrency / cleanup

caller-visible plugin state:

```text
Live -> Released
```

内部にはrelease後も `CleanupPending` を保持してよいが、callerへ再度liveとして見せない。`release()` はidempotentに成功できる。

session state:

```text
Opening -> Active -> LogicalClosed
                 \-> Failed -> LogicalClosed
```

内部backend resourceが残る場合だけ `CleanupPending` を保持する。

`Opening` ではno-reuse session ID、registry reservation、backend open、private data適用をprepareし、全て成功した時だけActive session IDを公開する。

各sessionはmutating backend I/Oを1件だけin-flightにする。外部I/O開始前にsession generation/lifecycleをsnapshotし、lockを外してI/Oし、応答後に同じgenerationがActiveであることを再検証してからprivate data/key epochをcommitする。

`closeSession()` / `release()` がI/O中に到達した場合は先にlogical close/releaseをcommitして新規I/Oを遮断する。進行中I/Oのownerが結果を回収するまで内部recordを保持し、遅れて返ったECM/key結果をregistryへpublishしない。

`processEmm()` はplugin-wide operationなので同一pluginで1件だけin-flightにする。release後に遅れて返った結果を新しいlive stateへ反映しない。

### 4.1 closeSession

`closeSession()` のcaller-visible成功確定点は、sessionをLogicalClosedへcommitし、当該tokenの新規resolveをrevokeした時点とする。その後backend closeを全件試行する。

backend closeが失敗しても、logical closeとrevokeが成功済みならsessionを再びActiveへ戻さず、失敗をservice-owned cleanup/diagnosticsへ引き渡す。物理cleanup失敗だけを理由にcallerへsessionをliveとして残さない。revoke自体を安全に確定できない場合だけclose成功にしない。

unknownまたは既にLogicalClosedのsessionに対する通常のsession operationは `ERROR_CAS_SESSION_NOT_OPENED` とする。

### 4.2 release

`release()` はpluginをReleasedへ不可逆にcommitし、新規method/callback deliveryを遮断し、全session tokenの新規resolveをrevokeする。各backend closeも全件試行するが、物理cleanup失敗はservice-owned cleanupへ引き継ぎ、AOSP objectのreleaseをbackend recovery loopへ変えない。

caller-visible release成功後、`release()` 自体はidempotentに成功してよい。その他の通常methodは `ERROR_CAS_INVALID_STATE` とする。Binder artifact消滅後も未完了cleanup stateはservice寿命のownerが保持するが、worker/timer構成は実装詳細とする。

## 5. ICas method / input / error contract

method成功確定点:

- `setPrivateData()`: plugin-local値と、binding済みならbackend値のcommit完了。失敗時は旧値維持。
- `setSessionPrivateData()`: backend成功後、generation/lifecycle再検証を通ってsession stateをcommit。失敗時は旧値維持。
- `openSession*()`: backend open + registry reservation後にActive session ID公開。
- `processEcm()`: backend応答後のgeneration/lifecycle再検証を通り、完全な新epochをatomic publishして同session IDから即resolve可能。
- `processEmm()`: bind済みB25 backendが成功し、release競合がないことを再検証。別backendへfallbackしない。
- `closeSession()`: LogicalClosed + token新規resolve revokeを確定し、backend cleanupを全件起動。
- `release()`: Released + 全session新規resolve revokeを確定し、backend cleanupを全件起動。

ECM/EMMはCASへ完全なsection byte sequenceとして渡す。TS packet、PID、demux bufferをCAS HALへ渡さない。empty、section framing/declared length不整合、対象CA systemとして処理不能な外形はbackend I/O前に拒否する。

B1 `processEmm()`、B25/B1 `provision()`、`refreshEntitlements()`、未定義vendor eventは `ERROR_CAS_CANNOT_HANDLE` とする。

error mapping:

```text
malformed input                           -> AIDL BAD_VALUE
unsupported mode / B1 EMM                -> ERROR_CAS_CANNOT_HANDLE
unknown / closed session                 -> ERROR_CAS_SESSION_NOT_OPENED
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

backendがAOSP専用statusを判定できる場合は `ERROR_CAS_DEVICE_REVOKED`、`ERROR_CAS_NEED_ACTIVATION`、`ERROR_CAS_NEED_PAIRING` 等へ対応付け、UNKNOWNへ潰さない。未実装を成功へ丸めない。

listenerはstate commit後かつ内部lock外で呼ぶ。listener failureでcommit済みstateをrollbackしない。listener/callbackにはECM/EMM本文、private data、session token、raw/prepared key materialを含めない。

## 6. SmartCard path

同一physical cardへのI/Oは単一ownerが直列化し、card I/O lock中にBinder callbackを呼ばない。open/reset/APDUは有限deadlineを持つ。

binding前probe timeoutは `CARD_UNKNOWN_TIMEOUT` としてfallbackしない。binding後にrequestを送信して結果不明となったsessionはFailedとして新規処理を遮断し、token revoke/closeへ進む。同pluginでYakisobaへ切り替えない。

B25 system key/CBC初期値は検証済みcard初期化応答から取得する。B1は採用B1 protocolの検証済み応答から供給元を確定し、B25配置を推測して流用しない。

## 7. Yakisoba path

CAS HALはlibyakisobaへ直接linkせず、vendor partition内の別daemonへB25 ECM/EMMをvendor-local IPCで要求する。B1 requestは拒否する。daemon endpoint/credentialを一般app、TIS、Tuner HAL、shellへ公開しない。

IPCは次を満たす。

```text
- version / operation / B25 identity検証
- request ID照合
- session/plugin generationによるstale判定
- bounded frame
- peer credential / SELinux domain検証
- connect/write/readを含むdeadline
- malformed/mismatched response拒否
- secret/session identityの通常log禁止
```

wire byte layout/magic値は内部実装とし、CAS serviceとdaemonで共通定義を使う。

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

具体的な `Reserve/Publish/Revoke` API名、TTL、slot上限、wire magic、retry回数は内部実装選択であり必須設計にしない。ただしeffective capacityは第2節のARIB gateを満たす。

## 9. product integration / license

CAS service、SmartCard adapter、Yakisoba daemon、Tuner key bridgeはvendor partition内に置き、各processへ必要なSELinux permissionだけを与える。

- `yakisoba_only`: Yakisoba daemon + credential + sepolicy + license成果物。SmartCard adapter不要。
- `smartcard_only`: SmartCard adapter。Yakisoba daemon不要。
- `prefer_smartcard_then_yakisoba`: 両backend。

libyakisobaを同梱・改変する場合は採用revisionのGPL-3.0配布条件を満たす。daemon分離をGPL義務消滅の根拠にしない。B1参照実装を移植/linkする場合は採用revisionのlicense条件を固定する。

module名、socket path、wire field値等の内部名称はAOSP公開契約にせず、採用実装内で一意に定義する。

## 10. validation

次を確認する。

```text
- ClearKey AOSP/VTS compatibility
- capability enumerate/support/createの同一snapshot整合
- unknown ID false/null
- B25/B1 explicit session: LIVE+MULTI2成功、非対応組合せ拒否
- smartcard_only B25
- yakisoba_only B25
- prefer: SmartCard valid / known unavailable / timeout
- concurrent first-bind / private-data race
- B1 ECM-only
- AOSP status mapping / complete section validation
- EMM owner == plugin binding
- binding後cross-backend fallbackなし
- close/releaseとin-flight ECM/EMM競合、late publishなし
- release idempotence / post-release invalid-state
- logical close/release後のbackend cleanup retry
- listener failureでcommit済みstate非rollback
- Yakisoba outcome-unknown / temporary-key zeroize
- provider death revoke
- ARIB STD-B25 Version 7.0の同時key/PID能力gate
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
13. caller-visible close/releaseとbackend物理cleanupを分離し、logical revoke後のcleanup失敗でobject/sessionを再live化しない。
14. listener failureでcommit済みstateをrollbackしない。
15. backend I/Oはboundedとし送信後結果不明を成功/fallbackへ丸めない。
16. Tuner tokenはMediaCas session ID bytesでservice process lifetime中再利用しない。
17. provider deathではそのincarnationの新規resolveを遮断する。
18. `processEcm()`成功時に完全contextを即resolve可能にする。
19. MediaCas close前にMediaCas由来tokenを全descramblerからVOIDで解除する。
20. raw key materialをBinder、TIS、通常logへ出さない。
21. B25 advertise時のeffective key/PID capacityはARIB STD-B25 Version 7.0の対象条項を満たす。
22. CAS HALはTS demux / AV / DVRを担当しない。
