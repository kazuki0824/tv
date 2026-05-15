# CAS HAL 実装計画 改訂版 v2
## 単一 `ICas` 実装 + スマートカード直結経路 / libyakisoba 常駐プロセス経路
## B1 実装可否調査結果反映版

## 0. 目的

本計画は、日本向けデジタル放送の B25 / B1 系 CAS 処理を Android TV 14 系の AOSP Media CAS / Tuner framework に統合するための改訂案である。

対象は次の2系統である。

1. スマートカードに対する読み書きにより ECM / EMM を処理する経路
2. Android 向けに一部フォークした `libyakisoba` を常駐プロセスとして起動し、CAS HAL から ECM / EMM を送受信する経路

ただし、B1 については本改訂で次を固定する。

```text
B1 実装の参照元:
  - 公開ソースとして CAS HAL へ移植・検証・保守できる実装は、実質的に libaribb1 系に集約されている。

libyakisoba:
  - B1 実装として扱わない。
  - B1 backend として列挙しない。
  - B1 の yakisoba fallback 対象にしない。

B1 の初期実装範囲:
  - B1SmartCardPath / libaribb1 系参照の ECM-only 実験対応から開始する。
  - B1 EMM 処理と通電制御情報取得は、実装根拠と検証条件が揃うまで unsupported とする。
```

改訂後の基本方針は次のとおりである。

```text
AOSP から見える CAS plugin:
  - B25 / B1 の CA system ID 単位で列挙する。
  - B1 は B1SmartCardPath の ECM 処理が実装・検証できるまで列挙しない。

AOSP から見える ICas:
  - 単一の MaleicacidCasPlugin 実装に固定する。

内部実装:
  - B25:
      SmartCardCasPath
      YakisobaCasPath
      のいずれかを session 開始時に選択する。
  - B1:
      B1SmartCardPath のみを使用する。
      YakisobaCasPath には切り替えない。
```

同一 `caSystemId` に対して「スマートカード版 plugin」と「libyakisoba版 plugin」を別々に列挙する構成は採用しない。理由は、AOSP の `IMediaCasService.createPlugin()` が `caSystemId` を主キーとして plugin を生成する契約であり、同一 `caSystemId` の複数 plugin を標準 API 上で一意に選択する入力がないためである。

---

## 1. 全体構成

```text
IMediaCasService/default
  ├─ enumeratePlugins()
  ├─ isSystemIdSupported(caSystemId)
  ├─ isDescramblerSupported(caSystemId)
  └─ createPlugin(caSystemId, listener)
       └─ MaleicacidCasPlugin : ICas
            ├─ SessionTable
            ├─ CasPathSelector
            ├─ SmartCardCasPath        # B25/B-CAS
            ├─ B1SmartCardPath         # B1/libaribb1 系参照、ECM-only から開始
            ├─ YakisobaCasPath         # B25 実験用のみ
            ├─ KeySlotRegistry
            └─ ICasListenerBridge

vendor.maleicacid.yakisoba-casd
  ├─ libyakisoba wrapper
  ├─ B25 ECM request handler
  ├─ B25 EMM request handler
  └─ health check / diagnostics
```

`IMediaCasService` は1つだけ配置する。`ICas` 実装も `MaleicacidCasPlugin` に一本化する。

B25では、スマートカード直結か libyakisoba 常駐プロセスかを `CasPathSelector` が選択する。B1では、`B1SmartCardPath` のみを選択対象とし、`YakisobaCasPath` は選択不可とする。

---

## 2. plugin 列挙仕様

### 2.1 採用する列挙

```text
enumeratePlugins():
  - caSystemId = B25/B-CAS 用 system ID
    name       = "Maleicacid B25 CAS"

  - caSystemId = B1 用 system ID
    name       = "Maleicacid B1 CAS"
    ※B1SmartCardPath の ECM 処理が実装・検証できた場合のみ
```

### 2.2 採用しない列挙

次の形式は採用しない。

```text
enumeratePlugins():
  - caSystemId = B25
    name       = "Maleicacid B25 SmartCard"
  - caSystemId = B25
    name       = "Maleicacid B25 Yakisoba"

  - caSystemId = B1
    name       = "Maleicacid B1 SmartCard"
  - caSystemId = B1
    name       = "Maleicacid B1 Yakisoba"
```

同一 `caSystemId` を重複列挙すると、標準の `createPlugin(caSystemId)` 呼び出しだけではどちらを生成するか一意に定まらない。そのため、plugin descriptor は CA system 単位に固定し、処理経路の差は `ICas` 内部に閉じ込める。

### 2.3 B1 plugin advertise gate

B1 plugin は、次の条件を満たすまで `enumeratePlugins()` に出さない。

```text
- B1SmartCardPath の ECM 処理が実装済みである。
- processEmm() は unsupported として明示実装されている。
- 通電制御情報取得は unsupported として明示実装されている。
- YakisobaCasPath が B1 に対して選択されないことがテストで固定されている。
```

---

## 3. 単一 `ICas` 実装

### 3.1 実装クラス

```text
MaleicacidCasPlugin : ICas
```

責務は次のとおりである。

```text
- session table 管理
- setPrivateData() の受領
- setSessionPrivateData() の受領
- openSessionDefault() / openSession() の処理
- closeSession() の処理
- processEcm() の処理
- processEmm() の処理
- release() の処理
- ICasListener への event / status 通知
- KeySlotRegistry への key 登録
- Tuner descrambler へ渡す opaque key token の生成
```

### 3.2 内部 trait

`SmartCardCasPath`、`B1SmartCardPath`、`YakisobaCasPath` は、同一 trait を実装する。

```rust
trait CasProcessingPath {
    fn probe(&mut self) -> ProbeResult;

    fn open_session(
        &mut self,
        ca_system_id: i32,
        intent: SessionIntent,
        mode: ScramblingMode,
    ) -> CasResult<InternalSession>;

    fn close_session(&mut self, session: &InternalSession) -> CasResult<()>;

    fn set_private_data(&mut self, data: &[u8]) -> CasResult<()>;

    fn set_session_private_data(
        &mut self,
        session: &InternalSession,
        data: &[u8],
    ) -> CasResult<()>;

    fn process_ecm(
        &mut self,
        session: &InternalSession,
        ecm: &[u8],
    ) -> CasResult<KeyUpdate>;

    fn process_emm(&mut self, emm: &[u8]) -> CasResult<EmmUpdate>;
}
```

`MaleicacidCasPlugin` は、この trait を通じて下位処理を呼ぶ。上位の `ICas` 契約は、スマートカード直結経路でも libyakisoba 経路でも変化しない。ただし、B1の `process_emm()` は、実装根拠が固定されるまでは明示的に unsupported を返す。

---

## 4. SmartCardCasPath / B1SmartCardPath 仕様

### 4.1 目的

`SmartCardCasPath` は、ARIB 資料に準拠したスマートカード I/O を担当する。CAS HAL から見た主処理は ECM / EMM の処理であり、TS demux や AV / DVR 出力は担当しない。

B1については、`B1SmartCardPath` を別経路として実装する。B1の公開・移植可能な参照実装は実質的に `libaribb1` 系に集約されているため、B1は `libaribb1` 系の挙動を参照して ECM-only 実験対応から開始する。

### 4.2 実装対象

B25 / B-CAS 系:

```text
- カードリーダー検出
- カード挿入状態確認
- reset / ATR 取得
- カード種別確認
- ARIB 準拠 APDU の生成
- APDU transmit
- APDU response status decode
- ECM 処理
- EMM 処理
- 未契約 / カード未挿入 / カード不正 / I/O エラー分類
- ECM 結果から odd/even CW 更新
- KeySlotRegistry への key 登録
- opaque key token 発行
```

B1 系:

```text
- B1 caSystemId の扱い
- B1カード probe
- openSession / closeSession
- PMT/CAT 由来 CA private data の保持
- ECM section payload のカード投入
- ECM 結果から key slot / opaque token 生成
- Tuner HAL descrambler への token 連携

当面 unsupported:
  - B1 EMM 処理
  - B1 通電制御情報取得
  - B1 契約更新・権利更新を受信機側で完結させる処理
```

### 4.3 完了条件

B25 / B-CAS 系:

```text
- カード未挿入時、processEcm() が成功扱いにならない
- カード不正時、CARD_INVALID として分類できる
- 対象 caSystemId に非対応のカードは CARD_UNSUPPORTED として分類できる
- ECM 成功時、raw CW を Binder / logcat / IPC に出さない
- ECM 成功時、KeySlotRegistry に key を登録し、opaque token のみを返す
- EMM 成功/失敗が diagnostics または ICasListener event として確認できる
- session close 後、その session に対する processEcm() が失敗する
```

B1 系:

```text
- B1カードまたは妥当なテストベクタで ECM 処理を検証できる
- ECM 成功時、raw key を Binder / logcat / IPC に出さない
- ECM 成功時、KeySlotRegistry に key を登録し、opaque token のみを返す
- processEmm() は明示的に unsupported を返す
- 通電制御情報取得は明示的に unsupported として扱う
- B1 で YakisobaCasPath が選択されない
- B1 plugin advertise は上記確認後にのみ有効化される
```

---

## 5. YakisobaCasPath 仕様

### 5.1 目的

`YakisobaCasPath` は、CAS HAL から libyakisoba 常駐プロセスへ ECM / EMM 処理を依頼する経路である。CAS HAL は libyakisoba に直接リンクしない。CAS HAL と libyakisoba 常駐プロセスは、固定したローカル IPC で接続する。

`YakisobaCasPath` は B25 / B-CAS 系の実験用 backend として扱う。B1 backend としては扱わない。

### 5.2 B1 非対応の固定

```text
YakisobaCasPath:
  B25:
    実験用 backend として実装可能。

  B1:
    未対応。
    yakisoba_only の対象にしない。
    prefer_smartcard_then_yakisoba の切替対象にしない。
    B1SmartCardPath の代替 backend として扱わない。
```

理由:

```text
- libyakisoba は B1 実装として扱える公開根拠がない。
- libyakisoba の公開 API は B-CAS / B25 系 ECM / EMM 処理を前提にしている。
- 公開・移植可能な B1 実装は実質的に libaribb1 系に集約されている。
- B1 の EMM 処理と通電制御情報取得は、libaribb1 系でも未対応制約がある。
```

### 5.3 Android 統合時に必要な libyakisoba 側改変

| 項目 | 改変内容 |
|---|---|
| ビルド | AOSP/Soong で `cc_binary` または `cc_library_shared` としてビルドできるよう `Android.bp` を追加する。 |
| インストール先 | Linux desktop 前提の `/usr/local` 依存を除去し、`/vendor/bin`、`/vendor/lib64`、`/vendor/etc` 等に固定する。 |
| 設定探索 | home directory、任意パス、環境変数依存の鍵・設定探索をAndroid製品向けには無効化または明示設定化する。 |
| daemon化 | `vendor.maleicacid.yakisoba-casd` として init rc から起動する。 |
| IPC | CAS HAL からのみ接続できる Unix domain socket または Binder service を実装する。 |
| SELinux | CAS HAL domain から daemon への接続のみ許可する。一般 app / TIS / Tuner HAL からの直接接続は禁止する。 |
| 複数 session | daemon 側で session table または request queue を持ち、複数 session の ECM 処理が混線しないようにする。 |
| timeout | ECM / EMM 処理に固定 timeout を設ける。CAS HAL binder thread を長時間塞がない。 |
| ログ | ECM / EMM 本文、鍵値、内部鍵素材、token を logcat に出さない。 |
| 結果形式 | libyakisoba の結果をそのまま外へ出さず、CAS HAL 側の KeySlotRegistry へ登録する入力に正規化する。 |

### 5.4 daemon プロセス仕様

```text
プロセス名:
  vendor.maleicacid.yakisoba-casd

起動:
  init rc で起動

通信:
  /dev/socket/maleicacid_yakisoba_casd
  または vendor Binder service

接続元:
  CAS HAL domain のみ

提供コマンド:
  - HealthCheck
  - ResetSession
  - DecodeEcm
  - ProcessEmm

禁止:
  - ECM / EMM 本文のログ出力
  - 鍵値のログ出力
  - 外部 storage 参照
  - 任意パスからの設定ファイル探索
  - shell property から鍵素材を読むこと
  - 一般 app からの直接アクセス
```

### 5.5 IPC request / response

```text
DecodeEcmRequest:
  - session_id
  - ca_system_id
  - ca_private_data_hash
  - service_id
  - ecm_pid
  - scrambling_mode
  - ecm_section_payload

DecodeEcmResponse:
  - status
  - key_epoch
  - odd_even_validity
  - key_material_for_local_registry
  - diagnostic_code

ProcessEmmRequest:
  - ca_system_id
  - emm_section_payload

ProcessEmmResponse:
  - status
  - entitlement_update_hint
  - diagnostic_code
```

`key_material_for_local_registry` は CAS HAL 内部の `KeySlotRegistry` に登録するためだけに使う。Binder、logcat、TIS、Tuner HAL へ raw key として出してはならない。

---

## 6. 処理経路選択仕様

### 6.1 mode

```text
cas.path.mode:
  - smartcard_only
  - yakisoba_only
  - prefer_smartcard_then_yakisoba
```

### 6.2 既定値

```text
userdebug / eng:
  prefer_smartcard_then_yakisoba

user / release:
  smartcard_only
```

本プロダクトは実験用であるため、`userdebug` / `eng` では、B25に限り、スマートカードが有効でないと確定した場合に libyakisoba 経路へ切り替える。製品版または配布版の既定値は `smartcard_only` とする。

### 6.3 カード状態分類

```text
CARD_VALID:
  - カードデバイスを開ける
  - reset / 初期化に成功する
  - カード種別確認に成功する
  - 対象 caSystemId に利用可能
  - fatal 状態ではない

CARD_ABSENT:
  - カードリーダーまたはカードが存在しない

CARD_INVALID:
  - カード応答はあるが、対象 CAS 処理に使えない

CARD_UNSUPPORTED:
  - カード種別が対象 caSystemId に対応しない

CARD_IO_UNAVAILABLE:
  - PC/SC または Android 側カード I/O が利用不能

CARD_UNKNOWN_TIMEOUT:
  - 応答待ちが固定 timeout を超え、有効/無効を確定できない
```

### 6.4 `prefer_smartcard_then_yakisoba` の固定動作

B25:

```text
1. createPlugin(caSystemId) 時に MaleicacidCasPlugin を生成する。
2. openSession() 前に SmartCardCasPath.probe() を実行する。
3. probe 結果が CARD_VALID なら SmartCardCasPath を選択する。
4. probe 結果が CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE なら YakisobaCasPath を選択する。
5. probe 結果が CARD_UNKNOWN_TIMEOUT の場合、YakisobaCasPath へ切り替えない。
6. CARD_UNKNOWN_TIMEOUT は CAS 一時失敗として上位へ返す。
7. 一度 session で選択した処理経路は、その session を close するまで変更しない。
8. session 中にカードが抜けた場合、その session は失敗扱いとする。
9. session 中に SmartCardCasPath から YakisobaCasPath へ切り替えてはならない。
10. 次回 openSession() で再度 probe し、条件を満たす場合だけ YakisobaCasPath を選択する。
```

B1:

```text
1. B1 では prefer_smartcard_then_yakisoba でも YakisobaCasPath を選択しない。
2. B1SmartCardPath.probe() が CARD_VALID の場合のみ B1SmartCardPath を選択する。
3. CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE / CARD_UNKNOWN_TIMEOUT のいずれでも YakisobaCasPath へ切り替えない。
4. B1で有効カードがない場合は CAS 失敗として上位へ返す。
```

`CARD_UNKNOWN_TIMEOUT` で切り替えない理由は、カードが存在する可能性が残る状態で別経路へ切り替えると、カード権利状態と視聴成否が一致しない誤動作になるためである。

---

## 7. KeySlotRegistry / token 仕様

### 7.1 基本方針

```text
- raw CW は Binder に出さない
- raw CW は logcat に出さない
- raw CW は TIS に返さない
- raw CW は Tuner HAL に直接渡さない
- CAS HAL 内部の KeySlotRegistry に登録する
- Tuner descrambler へ渡す値は opaque token のみとする
```

### 7.2 token

```text
KeyToken:
  - 16 bytes 以下
  - key slot 参照
  - caSystemId
  - session generation
  - odd/even key epoch
  - integrity check 用 tag
```

token は鍵値ではない。Tuner HAL の `IDescrambler.setKeyToken()` は、token を vendor shared registry で解決し、PID 単位の descramble stage に紐付ける。

---

## 8. Tuner HAL との境界

CAS HAL は TS packet path を持たない。TS packet payload の復号は Tuner HAL 側で行う。

### 8.1 CAS HAL の責務

```text
- CA system ID advertise
- ICas session 管理
- CA private data 受領
- ECM 処理
- EMM 処理
- smartcard / yakisoba 経路選択
- key slot 登録
- opaque key token 発行
- CAS event / status 通知
```

B1については、EMM処理を当面 unsupported とする。

### 8.2 Tuner HAL の責務

```text
- ITuner.openDescrambler()
- Tuner IDescrambler.setKeyToken()
- Tuner IDescrambler.addPid()
- Tuner IDescrambler.removePid()
- PID -> key token mapping
- TS header / adaptation field を壊さない payload-only MULTI2 復号
- 復号後 TS を既存 soft demux / DVR / AV path へ渡す
- 復号失敗 / key 未設定 / PID 未登録の diagnostics
```

### 8.3 TIS の責務

```text
- PMT / CAT / ECM / EMM filter を Tuner API 経由で開く
- CA descriptor を解析し caSystemId を決定する
- MediaCas(caSystemId) を生成する
- setPrivateData() / setSessionPrivateData() を呼ぶ
- processEcm() / processEmm() を呼ぶ
- MediaCas session 由来の key token を Tuner descrambler へ渡す
- addPid() で video/audio PID を復号対象にする
```

B1では、`processEmm()` が unsupported を返すことをTIS側でも許容し、空成功として扱わない。

---

## 9. GPL / ライセンス方針

### 9.1 前提

`libyakisoba` は GPL-3.0 として配布されている。そのため、Android 製品イメージや配布物に `libyakisoba` またはその改変版を同梱する場合、GPL の配布義務を前提に扱う。

B1参照元として扱う `libaribb1` 系についても、実際にソースを取り込む、リンクする、daemonへ組み込む、またはコードを移植する場合は、そのライセンス条件を別途確認し、配布義務を固定してから実装に入る。

### 9.2 判例から言えること

オープンソースライセンス条件は、単なる任意のお願いではなく、条件違反により著作権侵害または契約違反として問題になり得る。

代表例:

- `Jacobsen v. Katzer`: オープンソースライセンス条件が著作権上の条件として執行可能になり得ることを示した事案。
- `Artifex v. Hancom`: GPL違反について契約違反および著作権侵害が争点となり、GPLに基づく金銭的救済の可能性が否定されなかった事案。

ただし、別プロセス IPC で接続した場合に、呼び出し側プログラムへ GPL が絶対に波及しないと明確に判示した支配的判例があるわけではない。したがって、法務上は「リスク低減策」と「配布義務の明確化」を分けて扱う。

### 9.3 形態別の扱い

| 形態 | 扱い |
|---|---|
| libyakisoba を改変せず単体 daemon として同梱 | daemon / libyakisoba について GPL 本文、著作権表示、対応するソース提供が必要。 |
| libyakisoba を Android 向けに改変して同梱 | 改変済み libyakisoba と daemon 部分の対応ソースを GPLv3 条件で提供する必要がある。 |
| CAS HAL が libyakisoba.so に直接リンク | CAS HAL まで結合著作物と評価されるリスクが高い。採用しない。 |
| CAS HAL と libyakisoba daemon が固定 IPC で通信 | CAS HAL 本体を別著作物として扱える余地が最も大きい。ただし daemon 側の GPL 配布義務は残る。 |
| B1実装として libaribb1 系を参照・移植・リンクする | libaribb1 系のライセンス条件を確認し、その条件に従う。B1はyakisobaでは代替しない。 |

### 9.4 固定する法務仕様

```text
- CAS HAL 本体は libyakisoba に直接リンクしない。
- libyakisoba は別プロセス daemon に閉じ込める。
- IPC は ECM / EMM 処理要求と結果 status の単純な要求応答に限定する。
- libyakisoba 改変版を配布する場合、改変済みソースを提供する。
- libyakisoba を改変しない場合でも、同梱配布する GPL プログラムとして必要なライセンス文、著作権表示、対応ソース提供を行う。
- GPL 義務を消すために daemon 化した、という説明はしない。
- daemon 化は CAS HAL 本体との結合リスクを下げるための構造分離策として扱う。
- B1については libyakisoba を実装根拠にしない。
- B1で libaribb1 系コードを参照・移植・リンクする場合、そのライセンス条件を実装前に固定する。
```

---

## 10. 実装フェーズ

### Phase 1: CAS HAL 骨格

```text
- IMediaCasService/default を実装する
- VINTF manifest を追加する
- init rc を追加する
- SELinux domain を追加する
- enumeratePlugins() を実装する
- unsupported caSystemId の挙動を固定する
- createPlugin(caSystemId) が MaleicacidCasPlugin を返す
```

完了条件:

```text
- 同一 caSystemId の重複 descriptor がない
- unknown caSystemId は unsupported として扱われる
- createPlugin(B25) が ICas を返す
- createPlugin(unknown) が成功扱いにならない
- B1 は B1SmartCardPath が検証済みになるまで enumeratePlugins() に出ない
```

### Phase 2: 単一 ICas 実装

```text
- session table
- openSessionDefault()
- openSession()
- closeSession()
- setPrivateData()
- setSessionPrivateData()
- processEcm()
- processEmm()
- release()
- ICasListenerBridge
- KeySlotRegistry
```

完了条件:

```text
- session ID が一意
- closeSession 後の session は無効
- release 後に全 session / key slot が破棄される
- processEcm() / processEmm() が未実装成功扱いにならない
- B1 processEmm() は明示的 unsupported として固定されている
```

### Phase 3: SmartCardCasPath

```text
- カード probe
- CARD_VALID / CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE / CARD_UNKNOWN_TIMEOUT 分類
- ARIB 準拠 APDU 処理
- ECM 処理
- EMM 処理
- key 登録
- token 発行
```

完了条件:

```text
- 有効カードで ECM 処理が可能
- カードなしを CARD_ABSENT として分類できる
- 不正カードを CARD_INVALID として分類できる
- timeout を CARD_UNKNOWN_TIMEOUT として分類できる
- raw key が外部へ出ない
```

### Phase 4: B1SmartCardPath

```text
- libaribb1 系公開実装の移植可能範囲を確認する
- libaribb1 系のライセンス条件を確認する
- B1 caSystemId を固定する
- B1 card probe を実装する
- B1 ECM 処理を実装する
- B1 key token 発行を実装する
- B1 processEmm() を unsupported として明示実装する
- B1 通電制御情報取得を unsupported として明示実装する
```

完了条件:

```text
- B1 ECM 処理が実カードまたは妥当なテストベクタで確認できる
- B1 EMM が空成功にならない
- B1 通電制御情報取得が空成功にならない
- B1 で YakisobaCasPath が選択されない
- B1 advertise gate を満たすまで plugin descriptor が出ない
```

### Phase 5: YakisobaCasPath / daemon

```text
- libyakisoba Android.bp 追加
- daemon Android.bp 追加
- daemon init rc 追加
- daemon SELinux 追加
- IPC protocol 実装
- DecodeEcm 実装
- ProcessEmm 実装
- HealthCheck 実装
- timeout 実装
- ログ抑制
```

完了条件:

```text
- CAS HAL domain 以外から daemon に接続できない
- daemon が B25 ECM 要求に status を返す
- daemon が B25 EMM 要求に status を返す
- B1 要求は unsupported になる
- timeout 時に CAS HAL が binder thread を永久に塞がない
- 鍵値・ECM本文・EMM本文が logcat に出ない
```

### Phase 6: 処理経路選択

```text
- cas.path.mode を実装する
- smartcard_only を実装する
- yakisoba_only を実装する
- prefer_smartcard_then_yakisoba を実装する
- userdebug / eng 既定値を prefer_smartcard_then_yakisoba にする
- user / release 既定値を smartcard_only にする
```

完了条件:

```text
- B25 CARD_VALID では SmartCardCasPath が選ばれる
- B25 CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE では YakisobaCasPath が選ばれる
- B25 CARD_UNKNOWN_TIMEOUT では YakisobaCasPath が選ばれない
- B1 では CARD_VALID の場合のみ B1SmartCardPath が選ばれる
- B1 では YakisobaCasPath が選ばれない
- session 中に経路が切り替わらない
- 次回 openSession() で再判定される
```

### Phase 7: Tuner HAL 接続

```text
- CAS HAL が opaque key token を返す
- Tuner HAL が setKeyToken() で token を解決する
- addPid() / removePid() が PID 単位復号対象を管理する
- 復号 stage が TS payload のみを処理する
- 復号後 TS が既存 soft demux / DVR / AV へ流れる
```

完了条件:

```text
- token 未設定 PID は復号成功扱いにならない
- key parity に応じて odd/even key が選択される
- adaptation field / PCR / continuity counter が壊れない
- 復号不能時に diagnostics が増える
```

---

## 11. B1 実装対象判定

### 11.1 B1 を実装対象に含める可否

```text
判定:
  可能。

条件:
  - B1SmartCardPath として ECM-only 実験対応から開始する。
  - B1 EMM は未対応として固定する。
  - 通電制御情報取得は未対応として固定する。
  - B1 の公開参照実装は libaribb1 系を一次候補とする。
```

### 11.2 libyakisoba を B1 実装として扱えるか

```text
判定:
  扱わない。

理由:
  - libyakisoba は B1 実装として扱える公開根拠がない。
  - B1 の公開・移植可能な実装は実質的に libaribb1 系に集約されている。
  - B1 で yakisoba fallback を行うと、未検証 backend による成功扱いが発生し得る。
```

### 11.3 その他の B1 実装

```text
判定:
  CAS HAL へ移植・検証・保守できる独立した公開ソース実装は、現時点では確認できない。

扱い:
  - B1_p2c9 等の古いバイナリ・派生情報は、保守可能な公開ソース実装としては扱わない。
  - B1Decoder.dll 等は libaribb1 系互換物・派生物として扱い、独立した実装根拠にはしない。
```

---

## 12. 最終固定事項

```text
1. IMediaCasService は1つだけ実装する。
2. B25/B1 の plugin descriptor は caSystemId 単位で一意に列挙する。
3. smartcard と yakisoba を同一 caSystemId の別 plugin として列挙しない。
4. ICas 実装は MaleicacidCasPlugin に一本化する。
5. SmartCardCasPath、B1SmartCardPath、YakisobaCasPath は ICas 内部の処理経路として扱う。
6. userdebug / eng では、B25に限り、有効カードなしが確定した場合だけ YakisobaCasPath へ切り替える。
7. B1では YakisobaCasPath へ切り替えない。
8. B1では CARD_VALID の場合のみ B1SmartCardPath を選択する。
9. CARD_UNKNOWN_TIMEOUT では YakisobaCasPath へ切り替えない。
10. user / release の既定値は smartcard_only とする。
11. CAS HAL は libyakisoba に直接リンクしない。
12. libyakisoba は別プロセス daemon として統合する。
13. libyakisoba daemon を同梱する場合、GPL 配布義務の対象として扱う。
14. libyakisoba は B1 実装として扱わない。
15. B1実装の公開参照元は libaribb1 系を一次候補とする。
16. B1 EMM は実装根拠が揃うまで unsupported とする。
17. B1 通電制御情報取得は実装根拠が揃うまで unsupported とする。
18. raw CW は Binder、logcat、TIS、Tuner HAL へ出さない。
19. CAS HAL は TS demux / TS packet 復号 / AV / DVR を担当しない。
20. TS payload 復号は Tuner HAL descrambler の責務とする。
```

---

## 13. 参考資料

- Android Media CAS  
  https://source.android.com/docs/devices/tv/media-cas

- Android Tuner framework  
  https://source.android.com/docs/devices/tv/tuner-framework

- AIDL HAL / VINTF stability  
  https://source.android.com/docs/core/architecture/aidl/aidl-hals

- AOSP `IMediaCasService.aidl`  
  https://android.googlesource.com/platform/hardware/interfaces/+/master/cas/aidl/android/hardware/cas/IMediaCasService.aidl

- AOSP `ICas.aidl`  
  https://android.googlesource.com/platform/hardware/interfaces/+/master/cas/aidl/android/hardware/cas/ICas.aidl

- AOSP `AidlCasPluginDescriptor.aidl`  
  https://android.googlesource.com/platform/hardware/interfaces/+/master/cas/aidl/android/hardware/cas/AidlCasPluginDescriptor.aidl

- AOSP Tuner `Descrambler.java`  
  https://android.googlesource.com/platform/frameworks/base/+/master/media/java/android/media/tv/tuner/Descrambler.java

- libyakisoba  
  https://github.com/tsunoda14/libyakisoba

- libaribb25 / libaribb1  
  https://github.com/tsukumijima/libaribb25

- Jacobsen v. Katzer  
  https://jolt.law.harvard.edu/digest/jacobsen-v-katzer

- Artifex v. Hancom  
  https://docs.justia.com/cases/federal/district-courts/california/candce/3:2016cv06982/305835/54

- GNU GPL FAQ  
  https://www.gnu.org/licenses/gpl-faq.html
