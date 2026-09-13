# CAS HAL 実装計画 改訂版 v2
## 単一 `ICas` 実装 + スマートカード直結経路 / libyakisoba 常駐プロセス経路
## B1 実装可否調査結果反映版

## 0. 目的

本計画は、日本向けデジタル放送の B25 / B1 系 CAS 処理を Android TV 14 系の AOSP Media CAS / Tuner framework に統合するための改訂案である。

対象は次の2系統である。

1. スマートカードに対する読み書きにより、B25では ECM / EMM、B1では ECM のみを処理する経路
2. Android 向けに一部フォークした `libyakisoba` を常駐プロセスとして起動し、B25 の ECM / EMM をCAS HALと送受信する経路。B1には使用しない

B1については、`B1SmartCardPath` / libaribb1 系参照の ECM-only 経路を正式対応とし、B1 EMM、EMMに依存する通電制御情報取得、契約更新、権利更新は対応しない。libyakisobaはB1 backendとして扱わない。

AOSP/VTS互換のClearKey pathはB25/B1 product capabilityから分離して同じ `IMediaCasService/default` 配下に保持する。B25/B1実装の完成度やproduct capability profileを理由にClearKeyを無効化しない。

---

## 1. 全体構成

```text
IMediaCasService/default
  ├─ ClearKey compatibility path
  ├─ enumeratePlugins()
  ├─ isSystemIdSupported(caSystemId)
  ├─ isDescramblerSupported(caSystemId)
  ├─ createPlugin(caSystemId, listener)
  └─ createDescrambler(caSystemId)

MaleicacidCasPlugin : ICas
  ├─ SessionTable
  ├─ CasPathSelector
  ├─ SmartCardCasPath        # B25/B-CAS
  ├─ B1SmartCardPath         # B1/libaribb1 系参照、ECM-only
  ├─ YakisobaCasPath         # B25 実験用のみ
  ├─ KeySlotRegistry adapter
  └─ ICasListenerBridge

vendor.maleicacid.yakisoba-casd
  ├─ libyakisoba wrapper
  ├─ B25 ECM request handler
  ├─ B25 EMM request handler
  └─ health check / diagnostics
```

`IMediaCasService` は1つだけ配置する。B25/B1向け `ICas` 実装は `MaleicacidCasPlugin` に一本化する。SmartCard/Yakisobaの差はplugin内部に閉じ、同一`caSystemId`をbackend別の複数descriptorとして列挙しない。

B25/B1のCAS HAL自身はTS packetを復号しない。B25/B1について `isDescramblerSupported()` は `false`、`createDescrambler()` はAIDL呼出し成功かつ `null` とする。packet descramble ownerはTuner HALの`IDescrambler`だけである。ClearKey compatibility pathはこの制約の対象外とし、AOSP/VTS互換descramblerを提供する。

---

## 2. plugin列挙・AOSP公開契約

### 2.1 descriptor

```text
enumeratePlugins():
  - caSystemId = AOSP ClearKey 用 system ID
    name       = ClearKey compatibility descriptor

  - caSystemId = B25/B-CAS 用 system ID
    name       = "Maleicacid B25 CAS"
    ※B25 advertise gate成立時のみ

  - caSystemId = B1 用 system ID
    name       = "Maleicacid B1 CAS"
    ※B1 advertise gate成立時のみ
```

同一`caSystemId`をSmartCard/Yakisoba別に重複列挙しない。標準APIは`caSystemId`を主キーとしてpluginを生成するため、backend差は`ICas`内部へ閉じる。

### 2.2 unknown caSystemId

列挙されないCA system IDについてはAIDL transport自体を成功させ、次を返す。

```text
isSystemIdSupported(unknown)      -> false
isDescramblerSupported(unknown)   -> false
createPlugin(unknown)             -> null
createDescrambler(unknown)        -> null
```

unknown IDをservice-specific errorへ変換しない。

### 2.3 advertise gate

B25 descriptorは、次が検証済みの場合だけ広告する。

```text
- B25 SmartCard production path
- ICas session lifecycle
- B25 ECM / EMM
- KeySlotRegistry連携
- Tuner key token bridge
- close / revoke
```

B1 descriptorは、次が検証済みの場合だけ広告する。

```text
- B1SmartCardPath の ECM
- B1 processEmm() の明示的unsupported
- YakisobaCasPathがB1に選択されないこと
- key token bridge
- close / revoke
```

一時的なcard不在やprobe失敗を理由にdescriptor集合を動的増減させない。advertiseは製品能力、利用時card状態はsession利用結果として分離する。

---

## 3. 単一 `ICas` 実装

`MaleicacidCasPlugin` は次を所有する。

```text
- session table
- setPrivateData() / setSessionPrivateData()
- openSessionDefault() / openSession()
- closeSession()
- processEcm()
- processEmm()
- release()
- listener通知
- registry adapter
```

内部pathは共通境界を実装する。

```rust
trait CasProcessingPath {
    fn probe(&mut self) -> ProbeResult;
    fn open_session(&mut self, ca_system_id: i32, intent: SessionIntent, mode: ScramblingMode)
        -> CasResult<InternalSession>;
    fn close_session(&mut self, session: &InternalSession) -> CasResult<()>;
    fn set_private_data(&mut self, data: &[u8]) -> CasResult<()>;
    fn set_session_private_data(&mut self, session: &InternalSession, data: &[u8]) -> CasResult<()>;
    fn process_ecm(&mut self, session: &InternalSession, ecm: &[u8]) -> CasResult<KeyUpdate>;
    fn process_emm(&mut self, emm: &[u8]) -> CasResult<EmmUpdate>;
}
```

一度sessionで選択したpathはcloseまで不変とし、card抜去・daemon障害時に同sessionを別backendへ切り替えない。

---

## 4. SmartCardCasPath / B1SmartCardPath

### 4.1 B25

B25 SmartCard pathは次を担当する。

```text
- card reader検出 / card存在確認
- reset / ATR / card種別確認
- ARIB準拠APDU生成・送受信・status検証
- ECM / EMM
- card不在・不正・unsupported・I/O失敗分類
- B25 credential context取得
- ECM結果からodd/even Ks更新
```

B25のsystem key / CBC初期値は、検証済みのcard初期化応答から取得する。このためだけの外部secure storeやfactory provisioningを必須にしない。初期化応答の形式・長さ・statusを検証する前にcredentialとして採用しない。

ECM成功時はraw keyをBinder/TIS/logcatへ公開せず、完全なMULTI2 contextをregistryへcommitする。

### 4.2 B1

B1はECM-onlyとする。

```text
- B1 caSystemId
- B1 card probe
- openSession / closeSession
- CA private data保持
- ECM投入
- ECM結果からkey context更新
- Tuner token連携

非対応:
- B1 EMM
- EMM依存の通電制御情報取得
- EMM依存の契約更新・権利更新
```

B1のcredential sourceは採用するB1 protocol/参照実装の検証済み応答に基づいて確定し、B25応答配置を推測して流用しない。

---

## 5. YakisobaCasPath

`YakisobaCasPath` はB25実験用backendであり、B1には使用しない。CAS HALはlibyakisobaへ直接リンクせず、別vendor daemonとの固定ローカルIPCでB25 ECM/EMMを要求する。

Android統合では、Soong build、vendor配置、init、SELinux、bounded IPC、session混線防止、timeout、秘密情報ログ禁止を満たす。daemonから受け取るkey materialはCAS/vendor内部境界に限定し、registry commit用bufferへ移した後に一時bufferを破棄する。

B1 requestはdaemon側でも明示的にunsupportedとする。

### 5.1 B25 EMM owner

AIDL `processEmm()` にはsession IDがないため、EMM routingを「呼出し時に任意sessionの選択backendへ送る」設計にはしない。

B25でEMMを処理するbackend ownerは、plugin生成時に確定するB25 capability/path profileから一意に決める。1 plugin内でSmartCard系sessionとYakisoba系sessionを混在させ、そのsession集合からEMM送信先を推測してはならない。

```text
smartcard_only:
  EMM owner = SmartCardCasPath

yakisoba_only:
  EMM owner = YakisobaCasPath

prefer_smartcard_then_yakisoba:
  pluginのEMM ownerはopenSession結果ではなく、製品で明示した単一owner規則に固定する。
  ownerが利用不能ならprocessEmm()を失敗させ、別backendへ暗黙fallbackしない。
```

---

## 6. 処理経路選択

```text
cas.path.mode:
  - smartcard_only
  - yakisoba_only
  - prefer_smartcard_then_yakisoba
```

B25の`prefer_smartcard_then_yakisoba`では、`CARD_VALID`ならSmartCardを選択する。`CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE`が確定した場合だけYakisobaを選択できる。`CARD_UNKNOWN_TIMEOUT`ではcard状態未確定なのでfallbackしない。

B1では`CARD_VALID`の場合だけB1SmartCardPathを選択し、全ての非valid結果で失敗する。Yakisobaへ切り替えない。

session中にbackendを切り替えず、次回`openSession()`でのみ再判定する。

---

## 7. KeySlotRegistry / token

### 7.1 公開token

Tuner key tokenは `MediaCas.Session.getSessionId()` が返すsession ID bytesそのものとする。別のTIS向けtokenを生成しない。

```text
KeyToken:
  - MediaCas session ID bytesと同一
  - 1..16 bytes
  - opaque
  - raw key materialを含まない
```

`caSystemId`、session generation、key epoch、integrity情報などを公開tokenの必須fieldとして規定しない。内部identity/generation/epochはregistry側で保持・検証し、token内容の解釈で代替しない。

### 7.2 commit / resolve

session IDはECM成功前でも公開され得る。その時点ではregistry上でidentity予約だけが成立していてよく、完全なkey contextがpublishされるまでresolve不可とする。

`processEcm()`は新epoch materialをprepareし、credential contextと必要parityを検証した後、完全なcontextをatomic commitする。

```text
processEcm() success の確定点:
  同じMediaCas session ID bytesから新epochを即時resolve可能になった時点
```

commit前の失敗では旧epochを維持する。incomplete、generation/epoch mismatch、revoke済みtokenを成功へ丸めない。

session close、release、credential revoke、path fatal failure、registry corruptionでは新規resolveを遮断し、stale tokenを別sessionへ再利用しない。

---

## 8. Tuner HAL / TISとの境界

### 8.1 CAS HAL

```text
- CA system ID advertise
- ICas session管理
- CA private data
- ECM / B25 EMM
- path選択
- key context登録
- opaque session ID tokenとの対応
- status / listener通知
```

CAS HALはTS packet pathを持たない。

### 8.2 Tuner HAL

```text
- ITuner.openDescrambler()
- IDescrambler.setKeyToken()
- addPid() / removePid()
- PID -> token mapping
- TS payload-only MULTI2復号
- 復号後TSをsoft demux / DVR / AVへ渡す
- bad token / unavailable key診断
```

### 8.3 TIS

B25ではPMT/CAT/ECM/EMM filterを開き、B1ではPMT/ECMのみを開く。CA descriptorから`caSystemId`を決定し、MediaCas sessionを生成してECMを処理する。ECM成功後、同じsession ID bytesをTuner descramblerへ渡す。

MediaCas sessionをcloseする前にTuner側参照を解除する。

```text
1. 対象PIDのlinkを解除
2. IDescrambler.setKeyToken(VOID key token)でcurrent tokenを解除
3. Tuner側で新規利用されない状態を確定
4. MediaCas sessionをclose
```

cleanup失敗を成功済みに丸めない。

---

## 9. GPL / ライセンス

`libyakisoba` を同梱・改変する場合はGPL-3.0の配布義務を前提に扱う。CAS HAL本体はlibyakisobaへ直接リンクせず、別process daemonとのIPCに分離する。ただしdaemon化自体がGPL義務を消すとは説明しない。

B1参照元としてlibaribb1系コードを実際に移植・リンク・組込みする場合は、そのライセンス条件を実装前に確認して固定する。

---

## 10. 実装フェーズ

### Phase 1: AOSP CAS service骨格

```text
- IMediaCasService/default
- VINTF / init / SELinux
- ClearKey compatibility path
- enumeratePlugins / support query
- unknown caSystemIdのVTS戻り値
- B25/B1 advertise gate
```

完了条件:

```text
- ClearKeyがB25/B1 profile非依存で利用可能
- unknown IDはfalse / nullをAIDL成功で返す
- 同一caSystemIdの重複descriptorなし
- B25/B1は各gate成立前に広告されない
- B25/B1 createDescrambler()はnull
```

### Phase 2: MaleicacidCasPlugin

```text
- session table
- open / close / release
- private data
- ECM / EMM contract
- listener
- session ID token予約・revoke
```

### Phase 3: B25 SmartCard

```text
- card probe / APDU
- card初期化応答からcredential context取得
- ECM / EMM
- key context atomic commit
```

### Phase 4: B1 SmartCard

```text
- B1 probe / ECM
- B1 credential source検証
- B1 EMM明示unsupported
- Yakisoba非選択
```

### Phase 5: Yakisoba

```text
- Android build / daemon / init / SELinux
- bounded local IPC
- B25 ECM / EMM
- B1拒否
- raw key / ECM / EMMログ禁止
```

### Phase 6: path selection / EMM owner

```text
- smartcard_only / yakisoba_only / prefer_smartcard_then_yakisoba
- timeout非fallback
- session中path不変
- plugin単位のB25 EMM owner一意化
```

### Phase 7: Tuner接続

```text
- MediaCas session ID bytesをopaque tokenとして使用
- ECM成功前はunresolved
- ECM success時に即resolve可能
- addPid / removePid
- payload-only MULTI2
- VOID token -> MediaCas close のteardown順序
```

---

## 11. B1 実装対象判定

B1は実装対象に含める。ただし正式対応はB1SmartCardPathのECM-only経路であり、B1 EMM、EMM依存の通電制御情報取得、契約更新、権利更新は対応しない。libyakisobaをB1 fallback/backendとして扱わない。

---

## 12. 最終固定事項

```text
1. IMediaCasService/default は1つだけ実装する。
2. ClearKey compatibility pathをB25/B1 product capabilityと分離して保持する。
3. B25/B1 descriptorは各advertise gate成立後だけ列挙する。
4. unknown caSystemIdはsupport=false、create*=nullをAIDL成功で返す。
5. B25/B1のMedia CAS descramblerは提供せず、packet descrambleはTuner HALだけが所有する。
6. B25/B1のICas実装はMaleicacidCasPluginに一本化する。
7. SmartCard/Yakisobaを同一caSystemIdの別descriptorとして列挙しない。
8. B25ではSmartCardまたはYakisobaを明示profileに従って選択し、session中は切り替えない。
9. B1はB1SmartCardPathのみを使用し、Yakisobaへ切り替えない。
10. CARD_UNKNOWN_TIMEOUTではfallbackしない。
11. B25 SmartCardのsystem key / CBC初期値は検証済みcard初期化応答から取得する。
12. B25 EMM ownerはplugin/profile単位で一意に固定し、session集合から推測しない。
13. Tuner key tokenはMediaCas session ID bytesそのものとし、独自構造tokenを公開契約にしない。
14. processEcm()成功時点で新epochが同じsession IDからresolve可能でなければならない。
15. MediaCas session close前にTuner key token参照を解除する。
16. raw key materialをBinder、TIS、logcatへ出さない。
17. CAS HALはTS demux / TS packet復号 / AV / DVRを担当しない。
18. libyakisobaは別process daemonとし、B1実装根拠にはしない。
```

---

## 13. 参考資料

- Android Media CAS
- Android Tuner framework
- AIDL HAL / VINTF stability
- AOSP `IMediaCasService.aidl`
- AOSP `ICas.aidl`
- AOSP `AidlCasPluginDescriptor.aidl`
- AOSP Tuner `Descrambler.java`
- libyakisoba
- libaribb25 / libaribb1
- GNU GPL FAQ
