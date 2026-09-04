# CAS HAL 設計判断

## 設計正本と責務

本書は Maleicacid CAS HAL の公開契約、状態、処理経路、鍵資源、失敗意味論の正本である。product 統合、partition、init、VINTF、SELinux、外部ライブラリとライセンスの条件は `INTEGRATION.md` を正とする。TIS の section filter と MediaCas orchestration は `../tis/DESIGN_JA.md`、TS packet の復号は `../tuner_hal/DESIGN_JA.md` を正とし、本書へ複製しない。

現行コードはAIDL CAS service、B25/B1 session、SmartCard/Yakisoba transport、ECM/EMM、CAS固有identityをgeneric Tuner key provisioningへ変換するadapter、close/revokeを実装している。製品固有のB25/B1能力広告は、各advertise gateを通過したproductが `/vendor/etc/maleicacid/cas_capabilities` へ固定したimmutable capability profileだけから起動時に確定する。profileが欠落、不正、または空の場合はfail-closedでB25/B1を非対応とし、実装済みであることだけを根拠に能力を広告しない。AOSP/VTS互換のClearKey `0xF6D8` はこのMaleicacid product capability profileの対象外とし、profileの状態を理由に無効化しない。

## AOSP公開面

service instance は AIDL VINTF version 1 の `android.hardware.cas.IMediaCasService/default` を1個だけ公開する。

| CA system | ID | plugin descriptor | 対応範囲 |
|---|---:|---|---|
| AOSP ClearKey | `0xF6D8` | `Clear Key CAS` | AOSP/VTS compatibility path。plugin/session/provision/ECM/EMM/event/descramblerはAOSP reference contractとVTS期待値に従う |
| ARIB STD-B25 / B-CAS | `0x0005` | `Maleicacid B25 CAS` | SmartCardによるECM/EMM。debug profileではYakisobaによるECM/EMMも選択可能 |
| ARIB STD-B1 | `0x0001` | `Maleicacid B1 CAS` | SmartCardによるECMだけ。EMM、通電制御情報取得、契約更新、権利更新は非対応 |

service capability snapshotはClearKey compatibility capabilityとMaleicacid product capabilityを分離して合成する。`/vendor/etc/maleicacid/cas_capabilities` が制御するのはB25/B1だけであり、ClearKey descriptorを増減させない。同一CA system IDを処理経路別に重複列挙しない。`enumeratePlugins()`、`isSystemIdSupported()`、`createPlugin()` は同じ合成済みsnapshotを使う。

列挙されないCA system IDについてはAIDL transport自体を成功させ、`isSystemIdSupported()` と `isDescramblerSupported()` は `false`、`createPlugin()` と `createDescrambler()` は `null` を返す。未知IDをservice-specific errorへ変換しない。この戻り値契約はAIDL VTSのinvalid-system-ID期待値を正とする。

B25 descriptorを広告できるのは、B25 SmartCard production path、session、ECM/EMM、鍵registry、Tuner token bridge、close/revokeの試験が成立した後だけとする。B1 descriptorを広告できるのは、B1 SmartCard ECM、B1 `processEmm()`の明示的非対応、Yakisobaを選択しないこと、close/revokeの試験が成立した後だけとする。probe時の一時的なcard不在を理由にB25/B1 descriptor集合を変動させず、利用時の失敗として返す。

B25/B1のCAS HAL自身はTS packetを復号しないため、B25/B1とも `isDescramblerSupported()` は `false`、`createDescrambler()` はAIDL成功かつ `null` を返す。B25/B1のpacket descramble ownerはTuner HALの`IDescrambler`だけである。ClearKey compatibility pathはこの制約の対象外であり、AOSP reference/VTSが要求するClearKey descramblerを同一service配下で提供する。

## plugin とsession

B25/B1の各 `createPlugin(caSystemId)` は1個の `MaleicacidCasPlugin` を生成し、CA system ID、listener artifact、plugin generation、session tableを所有する。SmartCard/Yakisobaの差はplugin内部の `CasProcessingPath` に閉じ、AOSP descriptorや別serviceへ露出しない。ClearKey plugin/session lifecycleはMaleicacidのSmartCard/Yakisoba/key-provisioning stateへ混在させず、AOSP reference contractとVTS互換pathとして独立して扱う。

session IDは1 byte以上16 byte以下の再利用しないbyte sequenceである。B25/B1 session ID / Tuner key tokenのnamespaceは`android.hardware.cas.IMediaCasService/default`単位で1個とし、同serviceが生成したB25/B1の全`ICas` plugin instance・全CA systemを横断して、liveまたはTuner側にretired/stale参照が残り得るsession IDを再利用しない。各pluginのsession tableはsession lifecycleだけを所有し、session IDの一意性scopeをplugin-localへ狭めない。公開session IDは引き続きopaque bytesだけとし、CA system ID、plugin identity、generationを符号化しない。`openSessionDefault()` は対象CA systemの既定live/MULTI2 sessionを開く。`openSession(intent, mode)` は本製品が扱うlive/MULTI2組合せだけを受理し、その他は `ERROR_CAS_CANNOT_HANDLE` で状態不変とする。session IDを発行できない場合はsessionを公開しない。

B25/B1 sessionの状態は次に固定する。

```text
Opening -> Active -> Closing -> Closed
                 \-> Failed -> Closing -> Closed
```

- `Opening` 中にpath選択、下位session、registry予約、session IDをprepareし、すべて成功した時だけ `Active` をcommitする。
- 選択したpathはsession closeまで不変とし、card抜去やdaemon障害で別pathへ切り替えない。
- `closeSession()` は最初に新規ECMとprivate-data更新を遮断し、registry entryをrevokeしてから下位sessionを閉じる。既に閉じたsessionまたは未知sessionは `ERROR_CAS_SESSION_NOT_OPENED` とする。
- `release()` はpluginを論理closeし、全sessionのrevoke/close、listener解放を全件試行する。途中失敗を理由に残りのsessionを放置しない。release後の通常methodは `ERROR_CAS_INVALID_STATE` とする。
- generationをwrapまたは再利用しない。次generationを発行できない対象plugin/sessionはfail-closedとし、stale callback、stale token、別sessionへの資源再利用を許可しない。

## B25/B1 ICas method契約

| method | B25 | B1 | 成功確定点 / 非対応 |
|---|---|---|---|
| `setPrivateData()` | 対応 | 対応 | plugin pathへopaque dataをcommitした時点。失敗時は旧値維持 |
| `setSessionPrivateData()` | 対応 | 対応 | Active sessionへopaque dataをcommitした時点。失敗時は旧値維持 |
| `openSessionDefault()` / `openSession()` | 対応 | 対応 | pathとregistry予約を含むActive sessionを公開した時点 |
| `processEcm()` | 対応 | 対応 | 完全な新key epochをregistryへatomic commitした時点 |
| `processEmm()` | 対応 | 非対応 | B25下位pathがEMM更新をcommitした時点。B1は常に `ERROR_CAS_CANNOT_HANDLE` |
| `provision()` | 非対応 | 非対応 | `ERROR_CAS_CANNOT_HANDLE`。credentialはvendor secure provisioningが所有 |
| `refreshEntitlements()` | 非対応 | 非対応 | `ERROR_CAS_CANNOT_HANDLE`。B25更新はEMM、B1更新は非対応 |
| `sendEvent()` / `sendSessionEvent()` | 非対応 | 非対応 | vendor event番号を定義しないため `ERROR_CAS_CANNOT_HANDLE` |
| `closeSession()` | 対応 | 対応 | token revokeとlogical closeを確定し、下位cleanupを全件試行 |
| `release()` | 対応 | 対応 | plugin logical closeと全entry revokeを確定 |

ECM/EMMは完全なsection byte sequenceとして受け取り、TS packet、PID、demux、AV/DVR bufferをCAS HALへ渡さない。空、構文外形不正、対象CA systemと不整合な入力はAIDL `Status.BAD_VALUE`、既知だが処理不能なscrambling modeやB1 EMMは `ERROR_CAS_CANNOT_HANDLE`、session不在は `ERROR_CAS_SESSION_NOT_OPENED` とする。card不在は `ERROR_CAS_NO_CARD`、card無効は `ERROR_CAS_CARD_INVALID`、card応答不能は `ERROR_CAS_CARD_MUTE`、資源枯渇は `ERROR_CAS_RESOURCE_BUSY`、下位状態破損またはcommit結果不明は `ERROR_CAS_INVALID_STATE` に写像する。未知の内部失敗だけを `ERROR_CAS_UNKNOWN` とし、未実装を空成功へ丸めない。

listener通知は状態commit後にlock外で行う。listener失敗はcommit済みECM/EMM/session状態をrollbackせず、listener healthと診断だけを更新する。callbackへECM/EMM本文、session private data、鍵素材、tokenを含めない。

## 処理経路

### path capability

| path | B25 ECM | B25 EMM | B1 ECM | B1 EMM | profile |
|---|---:|---:|---:|---:|---|
| `SmartCardCasPath` | 対応 | 対応 | 非対象 | 非対象 | production |
| `B1SmartCardPath` | 非対象 | 非対象 | 対応 | 恒久非対応 | production |
| `YakisobaCasPath` | 対応 | 対応 | 非対応 | 非対応 | `userdebug` / `eng`の実験用 |

`CasPathSelector` はproduct image生成時に固定して起動時に一度だけ読むimmutable capability profileと、`openSession()`直前のcard probe snapshotだけからpathを決定する。profileのB25 entryは `b25-smartcard`、`b25-smartcard-yakisoba`、`b25-yakisoba` のexactly-one、B1 entryは `b1-smartcard` だけを許す。runtime property、起動後の設定変更、TISの入力でprofileを切り替えない。

probe結果は `CARD_VALID`、`CARD_ABSENT`、`CARD_INVALID`、`CARD_UNSUPPORTED`、`CARD_IO_UNAVAILABLE`、`CARD_UNKNOWN_TIMEOUT` を区別する。B25の `prefer_smartcard_then_yakisoba` では `CARD_VALID` のみSmartCardを選び、absence/invalid/unsupported/I/O unavailableが確定した場合だけYakisobaを選べる。timeoutはcard状態が確定していないためfallbackせず一時失敗とする。B1は `CARD_VALID` の場合だけB1 SmartCardを選び、すべての非valid結果で失敗し、Yakisobaを選ばない。

### SmartCard境界

SmartCard pathはcard deviceのprobe/reset、対象card識別、APDU直列化、ECM/EMM送受信、応答検証を所有する。同一card I/Oはsession間で直列化し、I/O lock保持中にBinder callbackを呼ばない。固定deadlineを持たないI/Oを開始せず、timeout後に成功/失敗を確定できないsessionは `Failed` としてtokenをrevokeする。B1 pathはB1 ECMだけを実装し、B25 APDUやYakisobaへfallbackしない。

### Yakisoba境界

CAS HALはlibyakisobaへ直接linkせず、別vendor daemonへ固定したローカルIPCでB25 ECM/EMMを要求する。IPCはversion、request ID、operation、B25 system ID、session generation、bounded payload、typed statusを持つ要求応答とする。B1 requestはdaemon側でも明示的に拒否する。peer credential、SELinux domain、message size、deadlineを検証し、timeout、切断、重複/未知request IDを成功へ丸めない。

daemonから受け取るkey materialはCAS/vendor内部境界に限定し、検証後ただちにregistry commit用bufferへ移し、一時bufferをzeroizeする。CAS Binder、TIS、logcat、dumpへ公開しない。daemon障害時に同じsessionをSmartCardへ切り替えない。

## KeySlotRegistry とtoken

### 公開token

標準MediaCas経路のTuner key tokenは `MediaCas.Session.getSessionId()` が返すsession ID bytesと完全に同一である。TIS向けvendor-private tokenを生成せず、CA system ID、session generation、key epoch、integrity tag、鍵素材をtoken bytesへ符号化しない。

tokenは1 byte以上16 byte以下であり、TunerのVOID token `[0x00]` と同一のsession IDを発行しない。TISはECM成功後に同じsessionのID bytesをそのまま `Descrambler.setKeyToken()` へ渡す。`processEcm()` 自体がtokenを戻す、またはTISがtokenを合成する契約にはしない。

### 内部entryとadapter

CAS HAL内部はCA system、MediaCas session generation、key epoch、credential contextをCAS-domain状態として保持してよい。ただしTuner registryへその意味を渡さない。`cas_hal/src/transport.rs` のintegration adapterがCAS内部identityを次のgeneric provisioning identityへ変換する。

```text
KeyProvisioningIdentity {
  provider_id,          // opaque; CA system IDではない
  provider_generation, // stale/ABA fence
  key_epoch,
}

Multi2KeyResource {
  system_key,
  cbc_initial_value,
  even_ks,
  odd_ks,
}
```

`provider_id` は本CAS serviceのkey provider instanceを識別するopaque値であり、B25 `0x0005` / B1 `0x0001`その他のCA system IDを符号化・別名化しない。同じCAS serviceから供給されるB25/B1 key resourceをTuner側でCA方式別に分岐させない。`provider_generation`はCAS session lifecycleからadapterが生成するstale/ABA fenceであり、Tunerは大小比較・CA意味解釈をせずidentity一致判定だけに使う。

`system_key`、`cbc_initial_value`、`even_ks`、`odd_ks` はraw key materialである。secure-memory object、key-ladder slot、opaque in-process handleへ置換してよいが、Tuner HALがopaque token resolve後に同じprovider identity/key epochの完全なMULTI2 contextを一意に得られなければならない。

system keyとCBC初期値はvendor secure provisioningが所有し、TIS/Tuner HALがproperty、公開API、一般設定ファイルから取得しない。ECM pathは得られたodd/even Ksを同一sessionのcredential contextへ結合する。Tuner HALはECM/EMM、card I/O、権利判定、credential provisioningを行わない。

### commit、resolve、revoke

- session open時は、opaque session ID candidateをTuner key registryの`Reserve`へ渡し、このservice-global registry reservation成功をtoken namespaceの一意性linearization pointとする。既にliveまたはretired tokenとして占有されたcandidateはsessionとして公開せず、下位SmartCard/Yakisoba sessionを開く前にcandidateだけを破棄して別IDをbounded retryする。`Reserve`のcollision以外の失敗はsession open失敗とする。予約済みentryは未解決状態であり、復号可能tokenとして扱わない。
- `processEcm()` は候補epochをprepareし、credential contextと必要parityが完全かを検証し、1回のatomic commitでcurrent epochを置換する。途中失敗では旧epochを維持する。
- registry publishの確定点は、ECM処理成功が返る前にsession IDで新epochをresolve可能になった時点である。TISが成功を観測してtokenを渡したのにentryが未登録という窓を作らない。
- resolveはtoken、opaque provider ID、provider generation、current epoch、validityを同一snapshotで検証する。incomplete、generation/epoch mismatch、revoke済み、registry不整合を復号成功へ丸めない。
- session close、plugin release、credential revoke、path fatal failure、registry corruptionでentryをrevokeし、以後の新規resolveを拒否する。stale tokenを別session/generationへ再利用しない。
- Tuner側refが残るentryは新規resolveを遮断した後、ref解放まで隔離してzeroizeする。refcount不整合では通常再利用せず対象entryをquarantineする。

Tuner key registryのownerは論理的に1個とし、CAS HALやintegration adapterが独立したTuner-token shadow tableを持たない。具体的なprocess/共有memory/secure service構成は、上記atomicity、lifetime、access-controlを満たす実装から選ぶ。

## 直列化と秘密情報

service capability snapshot、plugin session table、path I/O、registry mutation、listener artifactは別のownerとlockを持つ。lock順序は capability → plugin/session → registry reservation とし、card/daemon I/OとBinder callbackの間はlockを解放する。I/O後はplugin/session generationを再検証してからcommitする。

ECM/EMM本文、APDU、private data、session ID、token、system key、CBC初期値、Ksを通常log、panic文字列、Binder status message、dumpへ出さない。診断はCA system、path種別、typed outcome、匿名化したgeneration、飽和counterだけを公開する。秘密bufferは最短寿命とし、close/revoke/失敗時にzeroizeする。

## 契約確認観点

- ClearKey `0xF6D8` がprofile状態に依存せず列挙され、AOSP/VTS互換plugin/descrambler contractを満たすことを確認する。
- unknown system IDについて `isSystemIdSupported=false`、`isDescramblerSupported=false`、`createPlugin=null`、`createDescrambler=null` をAIDL成功で返すことを確認する。
- descriptor一意性、B25/B1 advertise gate、B25/B1 CAS HAL descrambler非対応を確認する。
- B25/B1 `openSession*` のprepare/commit/rollback、session ID長、VOID衝突防止、一意性、generation exhaustion、close/release idempotenceを確認する。
- B25 SmartCard ECM/EMM、B1 SmartCard ECM、B1 EMM拒否、B1でYakisoba非選択を既知vectorで確認する。
- card probe全分類、timeout非fallback、session中path不変、card抜去/daemon切断時revokeを確認する。
- ECM commit前faultでは旧epoch維持、commit後はsession IDで即resolve可能、epoch置換のatomicity、stale/revoke token拒否、ref解放後zeroizeを確認する。
- raw key materialがBinder、TIS、callback、log、dumpへ出ないことを確認する。
- listener failureがcommit済みstateをrollbackしないこと、未知/非対応/一時失敗のstatus写像を確認する。
- TISのB25 ECM/EMM、B1 ECM-onlyと、Tuner HALのpayload-only MULTI2経路を結合確認する。

これらは設計契約の確認観点であり、build、unit test、VTS、実機card/放送波確認の実施結果は `CHANGELOG.md` とタスク完了判定で管理する。


## Tuner key provisioning 境界

CAS HAL内部ではB25/B1、CA system ID、MediaCas session、ECM/EMMを正規のCAS-domain情報として扱う。一方、Tuner HALへ渡す境界ではこれらの意味を公開しない。`cas_hal/src/transport.rs` のadapterは、本CAS serviceを表すopaqueな非CA-system値 `provider_id` と、sessionのstale/ABAを防ぐ `provider_generation`、`key_epoch`、opaque `key_token`、MULTI2 key resourceへ変換する。Tuner側はprovider IDの意味を解釈せず、B25/B1やCA system IDで分岐してはならない。
