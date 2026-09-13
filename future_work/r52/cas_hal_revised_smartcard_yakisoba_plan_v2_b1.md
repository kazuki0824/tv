# CAS HAL 実装計画 改訂版 v3
## 単一 `ICas` 実装 + スマートカード直結経路 / libyakisoba 統合経路
## B1 実装可否調査結果反映版

## 0. 目的

本計画は、日本向けデジタル放送の B25 / B1 系 CAS 処理を Android TV 14 系の AOSP Media CAS / Tuner framework に統合するための改訂案である。

対象は次の2系統である。

1. スマートカードに対する読み書きにより、B25では ECM / EMM、B1では ECM のみを処理する経路
2. Android 向けに一部フォークした `libyakisoba` を B25 の ECM / EMM backend として利用する経路。CAS service 内 adapter または別 vendor daemon のいずれかを product の権限分離・ABI・保守・配布条件に応じて選択できる。B1には使用しない

ただし、B1 については本改訂で次を固定する。

```text
B1 実装の参照元:
  - 公開ソースとして CAS HAL へ移植・検証・保守できる実装は、実質的に libaribb1 系に集約されている。

libyakisoba:
  - B1 実装として扱わない。
  - B1 backend として列挙しない。
  - B1 の yakisoba fallback 対象にしない。

B1 の実装範囲:
  - B1SmartCardPath / libaribb1 系参照の ECM-only 経路を正式対応とする。
  - B1 EMM 処理、EMMに依存する通電制御情報取得、契約更新、権利更新は恒久的に対応しない。
```

改訂後の基本方針は次のとおりである。

```text
AOSP から見える CAS plugin:
  - ClearKey を AOSP/VTS compatibility path として B25/B1 state から分離して保持する。
  - B25 / B1 の CA system ID 単位で列挙する。
  - B1 は B1SmartCardPath の ECM 処理が実装・検証できるまで列挙しない。

AOSP から見える ICas:
  - 単一の MaleicacidCasPlugin 実装に固定する。

内部実装:
  - B25:
      SmartCardCasPath
      YakisobaCasPath
      のいずれかを B25 plugin instance ごとに一度だけ選択し、release まで全 session / EMM で共有する。
  - B1:
      B1SmartCardPath のみを使用する。
      YakisobaCasPath には切り替えない。
```

同一 `caSystemId` に対して「スマートカード版 plugin」と「libyakisoba版 plugin」を別々に列挙する構成は採用しない。理由は、AOSP の `IMediaCasService.createPlugin()` が `caSystemId` を主キーとして plugin を生成する契約であり、同一 `caSystemId` の複数 backend を標準 API 上で選択する入力がないためである。

本 product の TIS `CasController` は `caSystemId` ごとに1個の live MediaCas/CAS plugin を共有し、B25 の ECM session と EMM を同じ plugin instance へ配送する。この product-level ownership を利用し、HAL 内に別 client の plugin instance まで横断する service-global backend selector は導入しない。

B25 の規範は採用 product で固定する ARIB STD-B25 日本語原本とする。Version 7.0 が現行版であることと、7.0 日本語原本の具体的な同時処理可能スクランブル鍵数/PID数等を確認済みであることは分けて扱う。B25 capability を広告する前に、採用する Version 7.0 日本語原本の該当受信機能力条項を確認し、product の effective capacity が確認済み要求を満たすことを検証する。旧版英訳の具体値を Version 7.0 の要求値として代用しない。

---

## 1. 全体構成

```text
IMediaCasService/default
  ├─ ClearKey compatibility path
  ├─ enumeratePlugins()
  ├─ isSystemIdSupported(caSystemId)
  ├─ isDescramblerSupported(caSystemId)
  └─ createPlugin(caSystemId, listener)
       └─ MaleicacidCasPlugin : ICas
            ├─ SessionTable
            ├─ CasPathSelector
            ├─ SmartCardCasPath        # B25/B-CAS
            ├─ B1SmartCardPath         # B1/libaribb1 系参照、ECM-only から開始
            ├─ YakisobaCasPath         # B25 only
            ├─ KeySlotRegistry bridge
            └─ ICasListenerBridge

YakisobaCasPath
  ├─ in-process libyakisoba adapter
  └─ または vendor.maleicacid.yakisoba-casd
       ├─ libyakisoba wrapper
       ├─ B25 ECM request handler
       ├─ B25 EMM request handler
       └─ health check / diagnostics
```

`IMediaCasService` は1つだけ配置する。B25/B1 の `ICas` 実装は `MaleicacidCasPlugin` に一本化する。ClearKey は Maleicacid 固有 profile/backend/key state から分離した AOSP reference-compatible path とする。

B25では、スマートカード直結か libyakisoba 経路かを `CasPathSelector` が B25 plugin instance ごとに一度だけ選択する。B1では、`B1SmartCardPath` のみを選択対象とし、`YakisobaCasPath` は選択不可とする。

---

## 2. plugin 列挙仕様

### 2.1 採用する列挙

```text
enumeratePlugins():
  - caSystemId = ClearKey system ID
    name       = AOSP/VTS compatibility 用 descriptor

  - caSystemId = B25/B-CAS 用 system ID
    name       = "Maleicacid B25 CAS"
    ※B25 advertise gate を満たす場合のみ

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

列挙されない system ID は AIDL transport 成功のまま次を返す。

```text
isSystemIdSupported(caSystemId)    -> false
isDescramblerSupported(caSystemId) -> false
createPlugin(caSystemId, ...)      -> null
createDescrambler(caSystemId)      -> null
```

B25/B1 は Media CAS 側 descrambler を公開せず、packet descramble は Tuner HAL `IDescrambler` が所有する。ClearKey は AOSP/VTS compatibility の plugin/session/descrambler semantics に従う。

### 2.3 B1 plugin advertise gate

B1 plugin は、次の条件を満たすまで `enumeratePlugins()` に出さない。

```text
- B1SmartCardPath の ECM 処理が実装済みである。
- processEmm() は unsupported として明示実装されている。
- B1 EMMに依存する通電制御情報取得、契約更新、権利更新は unsupported として明示実装されている。
- YakisobaCasPath が B1 に対して選択されないことがテストで固定されている。
- generic MULTI2 key publish / rotation / revoke と close が確認済みである。
```

### 2.4 B25 plugin advertise gate

B25 plugin は、採用 profile に必要な backend と共通契約を満たす場合だけ列挙する。

```text
共通:
  - ICas lifecycle / AOSP status contract
  - complete ECM / EMM section input contract
  - MediaCas session ID -> Tuner token bridge
  - complete MULTI2 material の atomic publish / stable-slot rotation
  - revoke / stale-token rejection
  - 採用 ARIB STD-B25 Version 7.0 日本語原本の該当受信機能力条項を確認済み
  - product effective capacity が上記確認済み条項を満たすことを検証済み
  - TIS -> MediaCas -> Tuner 結合確認

smartcard_only:
  - SmartCard path / credential 初期化 / ECM / EMM / bounded I/O / card 抜去 / close

yakisoba_only:
  - Yakisoba adapter / B25 ECM / EMM / credential source / bounded operation / secret lifetime
  - 採用統合形態に必要な access control / 配布条件
  - 別 daemon 採用時は IPC/failure contract

prefer_smartcard_then_yakisoba:
  - 上記両 backend gate
  - plugin 単位 backend bind 判定
  - SmartCard 状態未確定時の non-fallback
```

`yakisoba_only` は SmartCard 未搭載でも成立する正式構成であり、SmartCard 実装を B25 advertise 条件にしない。Version 7.0 日本語原本の能力条項照合が未完了なら、ARIB capability gate は成立済みと扱わない。

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
- 採用 AIDL に存在する openSessionDefault() / openSession() の処理
- closeSession() の処理
- processEcm() の処理
- processEmm() の処理
- release() の処理
- ICasListener への event / status 通知
- MediaCas session ID と KeySlotRegistry entry の対応管理
- B25 plugin instance 単位 backend binding
```

AIDL revision に存在しない method を vendor 独自 AIDL として追加しない。明示 `openSession(intent, mode)` では、本 r52 が成功対応する `LIVE + MULTI2` を受理し、非対応 intent/mode は状態不変で `ERROR_CAS_CANNOT_HANDLE` とする。

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

`MaleicacidCasPlugin` は、この trait を通じて下位処理を呼ぶ。上位の `ICas` 契約は、スマートカード直結経路でも libyakisoba 経路でも変化しない。ただし、B1はECM-only能力として固定し、`process_emm()` は恒久的に unsupported を返す。TISはB1 sessionで `MediaCas.processEmm()` を呼ばない。

B25 plugin state は `Unbound / SmartCard / Yakisoba` の backend binding を1個だけ持つ。同じ plugin instance に最初の backend-dependent operation が並行到達しても binding 結果を一意にし、release まで全 session / EMM で同じ backend を使う。特定 lock/thread/counter 方式は規定しない。

plugin/session lifecycle は少なくとも次の意味を持つ。

```text
plugin:  Live -> Releasing -> Released
session: Opening -> Active -> Closing -> Closed
                         \-> Failed -> Closing -> Closed
```

close/release と外部 I/O が競合した場合、遅れて返った ECM/key/EMM 結果を Closed/Releasing state や別 session へ commit しない。listener notification は state commit 後に行い、listener failure で commit 済み state を rollback しない。callback 実装方式は固定せず、half-committed state を外部へ露出しないことを契約とする。

---

## 4. SmartCardCasPath / B1SmartCardPath 仕様

### 4.1 目的

`SmartCardCasPath` は、ARIB 資料に準拠したスマートカード I/O を担当する。CAS HAL から見た主処理は ECM / EMM の処理であり、TS demux や AV / DVR 出力は担当しない。

B1については、`B1SmartCardPath` を別経路として実装する。B1の公開・移植可能な参照実装は実質的に `libaribb1` 系に集約されているため、B1は `libaribb1` 系の挙動を参照するECM-only経路だけを正式対応とする。

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
- card 初期化応答から system key / CBC 初期値を検証して取得
- ECM 結果から odd/even CW 更新
- KeySlotRegistry への complete MULTI2 material 登録
- MediaCas session ID bytes を Tuner key token として利用
```

B1 系:

```text
- B1 caSystemId の扱い
- B1カード probe
- openSession / closeSession
- PMT/CAT 由来 CA private data の保持
- ECM section payload のカード投入
- ECM 結果から key slot / MediaCas session ID token 連携
- Tuner HAL descrambler への token 連携

恒久的に非対応:
  - B1 EMM 処理
  - B1 EMMに依存する通電制御情報取得
  - B1 EMMに依存する契約更新・権利更新を受信機側で完結させる処理
```

B25 system key / CBC 初期値は検証済み card 初期化応答から取得する。このためだけに外部 secure store / factory provisioning を必須にしない。B1 は採用 B1 protocol の検証済み応答から供給元を確定し、B25 の配置を推測して流用しない。

SmartCard I/O は同一 physical card の state mutation 順序を一意にし、open/reset/APDU 等が caller を無期限に占有しないようにする。deadline/cancellation/非同期化等の具体方式は固定しない。request 送信後に結果不明となった operation を成功扱いしない。

### 4.3 完了条件

B25 / B-CAS 系:

```text
- カード未挿入時、processEcm() が成功扱いにならない
- カード不正時、CARD_INVALID として分類できる
- 対象 caSystemId に非対応のカードは CARD_UNSUPPORTED として分類できる
- ECM 成功時、raw CW を Binder / logcat / TIS に出さない
- ECM 成功時、complete current material を同じ MediaCas session ID の stable link から取得可能にする
- EMM 成功/失敗が AOSP status または必要な diagnostics として確認できる
- session close 後、その session に対する processEcm() が失敗する
- card 抜去または恒久 invalidation 後、影響 session の新規 key resolve を遮断する
```

B1 系:

```text
- B1カードまたは妥当なテストベクタで ECM 処理を検証できる
- ECM 成功時、raw key を Binder / logcat / TIS に出さない
- ECM 成功時、generic MULTI2 stable-slot へ material を登録する
- processEmm() は明示的に unsupported を返す
- B1 EMMに依存する通電制御情報取得、契約更新、権利更新は明示的に unsupported として扱う
- B1 で YakisobaCasPath が選択されない
- B1 plugin advertise は上記確認後にのみ有効化される
```

---

## 5. YakisobaCasPath 仕様

### 5.1 目的

`YakisobaCasPath` は、CAS HAL から libyakisoba を利用して B25 ECM / EMM を処理する経路である。CAS service 内で直接利用する in-process adapter と、別 vendor daemon に閉じて vendor-local IPC で利用する adapter の双方を許す。どちらを採用するかは product の権限分離、ABI、保守、配布条件に応じて決める。

`YakisobaCasPath` は B25 / B-CAS 系 backend として扱う。B1 backend としては扱わない。

### 5.2 B1 非対応の固定

```text
YakisobaCasPath:
  B25:
    backend として実装可能。

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
| ビルド | AOSP/Soong で採用形態に応じた `cc_library_*` または `cc_binary` としてビルドできるよう `Android.bp` を追加する。 |
| インストール先 | Linux desktop 前提の `/usr/local` 依存を除去し、`/vendor/bin`、`/vendor/lib64`、`/vendor/etc` 等の採用構成に固定する。 |
| 設定探索 | home directory、任意パス、環境変数依存の鍵・設定探索をAndroid製品向けには無効化または明示設定化する。 |
| 統合形態 | in-process adapter または別 vendor daemon を選択できる。別 daemon 自体を AOSP 要件として必須化しない。 |
| IPC | 別 daemon 採用時だけ、CAS HAL から利用する Unix domain socket または Binder 等の vendor-local IPC を実装する。 |
| access control | 未許可主体が credential/key mutation 境界へ到達できないことを保証する。SELinux、socket ownership、peer credential、Binder identity 等の具体機構は採用構成に応じて選ぶ。 |
| 複数 session | 採用 adapter/backend 側で複数 session の ECM 処理が混線しないようにする。 |
| bounded operation | ECM / EMM 処理が caller を無期限に占有しない。具体 timeout値や同期方式は実装詳細とする。 |
| ログ | ECM / EMM 本文、鍵値、内部鍵素材、token を通常 log に出さない。 |
| 結果形式 | libyakisoba の結果をそのまま外へ出さず、CAS HAL 側の KeySlotRegistry へ登録する入力に正規化する。 |

### 5.4 daemon プロセス仕様

別 vendor daemon を採用する場合の実装候補を次に示す。process名、socket path、wire field値は上位CAS契約ではなく実装詳細である。

```text
プロセス名の例:
  vendor.maleicacid.yakisoba-casd

起動:
  init rc で起動

通信例:
  /dev/socket/maleicacid_yakisoba_casd
  または vendor Binder service

接続元:
  許可された CAS service

提供操作の例:
  - HealthCheck
  - ResetSession
  - DecodeEcm
  - ProcessEmm

禁止:
  - ECM / EMM 本文の通常ログ出力
  - 鍵値の通常ログ出力
  - 外部 storage からの credential 探索
  - 任意パスからの設定ファイル探索
  - shell property から鍵素材を読むこと
  - 一般 app からの直接 mutation
```

### 5.5 IPC request / response

別 daemon を採用する場合の reference schema を次に示す。field名・明示version field・wire layout自体をAOSP公開契約または唯一の必須実装にしない。採用実装は request/response の互換性、対応付け、size bound、access control、bounded I/O、stale request rejection を満たす。

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
  - key_update_identity   # 数値key_epoch自体は必須ではない
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

送信後に response を受け取れず backend 側 mutation の成否を確定できない場合は outcome unknown とする。replay-safe/idempotency を証明できる同一 backend 実装なら安全な再送を許してよいが、保証がない場合は自動再送しない。outcome unknown のまま別 backend へ fallback しない。

---

## 6. 処理経路選択仕様

### 6.1 mode

```text
cas.path.mode:
  - smartcard_only
  - yakisoba_only
  - prefer_smartcard_then_yakisoba
```

profile は vendor image で固定し、service 起動時に capability snapshot として読む。runtime property、TIS入力、card挿抜、Yakisoba backendの一時healthで descriptor 集合を変更しない。欠落・不正・重複・未知値は該当 B25/B1 capability の advertise gate 不成立として扱う。

### 6.2 構成別動作

```text
smartcard_only:
  - B25 plugin instance を SmartCard backend に bind する。
  - card 不在等で Yakisoba へ暗黙切替しない。

yakisoba_only:
  - B25 plugin instance を Yakisoba backend に bind する。
  - SmartCard probe を行わない。
  - Yakisoba障害でSmartCardへ暗黙切替しない。

prefer_smartcard_then_yakisoba:
  - plugin binding が Unbound のとき、最初の backend-dependent operation で選択する。
```

build typeだけから backend を暗黙決定しない。

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
  - 規定したbounded operation内に有効/無効を確定できない
```

### 6.4 `prefer_smartcard_then_yakisoba` の固定動作

B25:

```text
1. createPlugin(caSystemId) 時に MaleicacidCasPlugin を生成する。
2. 当該 plugin の最初の backend-dependent operation 前に SmartCardCasPath.probe() を実行する。
3. probe 結果が CARD_VALID なら SmartCardCasPath を選択する。
4. probe 結果が CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE なら YakisobaCasPath を選択する。
5. probe 結果が CARD_UNKNOWN_TIMEOUT の場合、YakisobaCasPath へ切り替えない。
6. CARD_UNKNOWN_TIMEOUT は CAS 一時失敗として上位へ返し、plugin binding は Unbound のままにする。
7. 一度 plugin で選択した処理経路は、その plugin を release するまで変更しない。
8. session 中にカードが抜けた場合、その card state に依存する session は失敗扱いとする。
9. 同じ plugin instance で SmartCardCasPath から YakisobaCasPath へ切り替えてはならない。
10. backend 再選択が必要な場合は plugin を release し、新しい plugin instance で再評価する。
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
- raw CW は通常 log に出さない
- raw CW は TIS に返さない
- raw CW は Tuner HAL に直接渡さない
- CAS HAL 内部の KeySlotRegistry に登録する
- Tuner descrambler へ渡す値は MediaCas session ID bytes とする
```

### 7.2 token

```text
Tuner key token:
  - MediaCas.Session.getSessionId() bytes そのもの
  - 1..16 bytes
  - opaque
  - raw key / caSystemId / backend種別 / owner世代 / key epoch を公開形式へ埋め込まない
```

Tuner HAL の `IDescrambler.setKeyToken()` は、token を vendor internal registry の stable slot identity へ link する。TIS向けの別tokenを生成しない。

初回 ECM 成功前は同 session ID が unresolved でよい。`processEcm()` success の確定点では complete current MULTI2 material が同じ session ID の stable link から直ちに取得可能でなければならない。後続 ECM は同じ stable slot の current material を atomic に切り替え、1 packet 処理中に旧/new material のfieldを混在させない。数値 `key_epoch` や特定lock-free構造は必須ではない。

token は live/retired linkage が残る間は別 session へ再割当てしない。process lifetime全体の永久no-reuseは選択可能だが必須ではない。CAS owner loss / close / release / fatal failure / revoke 後は新規 resolve を拒否し、旧ownerの後着mutationをcurrent stateとして受理しない。

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
- B25 plugin instance 単位 backend 選択
- key slot 登録 / rotation / revoke
- MediaCas session ID と Tuner key token の対応維持
- CAS event / status 通知
```

B1はECM-only能力として固定し、EMM処理を恒久的に対応しない。

### 8.2 Tuner HAL の責務

```text
- ITuner.openDescrambler()
- Tuner IDescrambler.setKeyToken()
- Tuner IDescrambler.addPid()
- Tuner IDescrambler.removePid()
- token -> stable key slot linkage
- PID -> key token mapping
- TS header / adaptation field を壊さない payload-only MULTI2 復号
- 復号後 TS を既存 soft demux / DVR / AV path へ渡す
- 復号失敗 / key 未設定 / PID 未登録の diagnostics
```

Tuner HAL は B25/B1、SmartCard/Yakisoba、credential source を解釈して分岐しない。

### 8.3 TIS の責務

```text
- caSystemId ごとに1個の live MediaCas/CAS plugin を共有する
- B25では PMT / CAT / ECM / EMM filter を Tuner API 経由で開く
- B1では PMT / ECM filter を Tuner API 経由で開き、EMM filterを起動しない
- CA descriptor を解析し caSystemId を決定する
- MediaCas(caSystemId) を生成する
- setPrivateData() / setSessionPrivateData() を呼ぶ
- B25では processEcm() / processEmm()、B1では processEcm() だけを呼ぶ
- MediaCas session ID bytes を Tuner descrambler へ key token として渡す
- addPid() で video/audio PID を復号対象にする
```

B1では`MediaCas.processEmm()`を呼ばない。防御的にB1 pluginの`processEmm()`へ到達した場合も明示的unsupportedを返し、空成功にしない。

MediaCas session を close する前に、そのsession ID tokenを保持する全 descramblerで `setKeyToken(VOID)` を成功させる。VOID成功をそのdescramblerが以後tokenを新規packet処理へ使用しない確定点とする。既取得内部key参照は処理終了まで保持してよいが、新規取得はCAS側revoke後に拒否する。秘密materialの破棄方法はresource表現に応じた実装詳細とする。

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
| CAS HAL が libyakisoba.so に直接リンク | 統合形態として採用する場合、CAS HAL側を含む結合範囲についてGPLv3上の義務を法務・配布設計で確認し、その条件を満たす。daemon必須化の代わりに無条件採用可とは扱わない。 |
| CAS HAL と libyakisoba daemon が固定 IPC で通信 | CAS HAL 本体を別著作物として扱える余地が大きくなるが、daemon側のGPL配布義務は残る。daemon化だけでGPL義務が消滅するとは扱わない。 |
| B1実装として libaribb1 系を参照・移植・リンクする | libaribb1 系のライセンス条件を確認し、その条件に従う。B1はyakisobaでは代替しない。 |

### 9.4 固定する法務仕様

```text
- libyakisoba の in-process / daemon 統合は、採用 revision と統合形態に適用される GPL-3.0 条件を確認してから選択する。
- libyakisoba 改変版を配布する場合、改変済みソースを提供する。
- libyakisoba を改変しない場合でも、同梱配布する GPL プログラムとして必要なライセンス文、著作権表示、対応ソース提供を行う。
- GPL 義務を消すために daemon 化した、という説明はしない。
- daemon 化を採用する場合は CAS HAL 本体との構造分離策として扱う。
- direct linkを採用する場合は、その結合範囲へ適用されるGPL条件を満たす。
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
- 必要な SELinux policy を追加する
- ClearKey compatibility path を保持する
- enumeratePlugins() を実装する
- unsupported caSystemId の false/null 挙動を固定する
- createPlugin(caSystemId) が MaleicacidCasPlugin を返す
```

完了条件:

```text
- ClearKey descriptor / create semantics が AOSP/VTS と一致する
- 同一 caSystemId の重複 descriptor がない
- unknown caSystemId の support query が false
- createPlugin(unknown) / createDescrambler(unknown) が AIDL成功かつnull
- createPlugin(B25) が ICas を返す
- B25 は採用profileのadvertise gateを満たすまで enumeratePlugins() に出ない
- B1 は B1SmartCardPath が検証済みになるまで enumeratePlugins() に出ない
```

### Phase 2: 単一 ICas 実装

```text
- session table
- 採用 AIDL に存在する openSessionDefault()
- openSession()
- closeSession()
- setPrivateData()
- setSessionPrivateData()
- processEcm()
- processEmm()
- release()
- ICasListenerBridge
- KeySlotRegistry bridge
- plugin instance 単位 B25 backend binding
```

完了条件:

```text
- session ID がlive/retired参照と衝突しない
- closeSession 後の session は無効
- release 後にそのpluginを再利用できない
- close/release と競合した遅延結果をpublishしない
- processEcm() / processEmm() が未実装成功扱いにならない
- B1 processEmm() は明示的 unsupported として固定されている
- unsupported intent/mode は状態不変で ERROR_CAS_CANNOT_HANDLE
```

### Phase 3: SmartCardCasPath

```text
- カード probe
- CARD_VALID / CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLE / CARD_UNKNOWN_TIMEOUT 分類
- ARIB 準拠 APDU 処理
- card初期化応答からsystem key/CBC初期値を取得
- ECM 処理
- EMM 処理
- complete MULTI2 material 登録
- MediaCas session ID token linkage
```

完了条件:

```text
- 有効カードで ECM 処理が可能
- カードなしを CARD_ABSENT として分類できる
- 不正カードを CARD_INVALID として分類できる
- bounded operation内に状態確定できない場合を CARD_UNKNOWN_TIMEOUT として分類できる
- raw key が外部へ出ない
- processEcm() success時点で同session IDからcomplete current materialを取得できる
```

### Phase 4: B1SmartCardPath

```text
- libaribb1 系公開実装の移植可能範囲を確認する
- libaribb1 系のライセンス条件を確認する
- B1 caSystemId を固定する
- B1 card probe を実装する
- B1 ECM 処理を実装する
- B1 generic MULTI2 stable-slot連携を実装する
- B1 processEmm() を unsupported として明示実装する
- B1 EMMに依存する通電制御情報取得、契約更新、権利更新を unsupported として明示実装する
```

完了条件:

```text
- B1 ECM 処理が実カードまたは妥当なテストベクタで確認できる
- B1 EMM が空成功にならない
- B1 EMMに依存する通電制御情報取得、契約更新、権利更新が空成功にならない
- B1 で YakisobaCasPath が選択されない
- B1 advertise gate を満たすまで plugin descriptor が出ない
```

### Phase 5: YakisobaCasPath

```text
- libyakisoba Android.bp 追加
- in-process adapter または daemon adapter を選択可能にする
- B25 DecodeEcm 実装
- B25 ProcessEmm 実装
- backend owner loss / stale result拒否
- bounded operation 実装
- ログ抑制

別daemon採用時:
- daemon Android.bp / init rc / SELinux等の必要統合
- IPC schema compatibility / response対応付け / size bound / access control / bounded I/O
```

完了条件:

```text
- B25 ECM 要求に status / key materialを返す
- B25 EMM 要求に status を返す
- B1 要求は unsupported になる
- backend operation が caller を永久に塞がない
- 鍵値・ECM本文・EMM本文が通常logに出ない
- outcome unknownを成功扱いせず、二重mutationを起こさない
- daemon採用時、未許可主体からmutationできない
```

### Phase 6: 処理経路選択

```text
- immutable capability profile を実装する
- smartcard_only を実装する
- yakisoba_only を実装する
- prefer_smartcard_then_yakisoba を実装する
- build typeだけからbackendを暗黙決定しない
- product TISはcaSystemIdごとに1個のlive MediaCas/CAS pluginを共有する
```

完了条件:

```text
- smartcard_onlyではSmartCardだけを使用し、Yakisobaへ切り替わらない
- yakisoba_onlyではSmartCard probeを行わず、Yakisobaだけを使用する
- preferのB25 CARD_VALIDではSmartCardCasPathが選ばれる
- preferのB25 CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED / CARD_IO_UNAVAILABLEではYakisobaCasPathが選ばれる
- B25 CARD_UNKNOWN_TIMEOUTではYakisobaCasPathが選ばれない
- B1ではCARD_VALIDの場合のみB1SmartCardPathを選択する
- B1ではYakisobaCasPathが選択されない
- 同一B25 plugin instance中に経路が切り替わらない
- 再選択は新しいplugin instanceで行う
```

### Phase 7: Tuner HAL 接続

```text
- MediaCas session ID bytes をTuner key tokenとして使用する
- Tuner HAL が setKeyToken() で stable slotへlinkする
- addPid() / removePid() が PID 単位復号対象を管理する
- 後続ECMで同じslotのcurrent materialをatomic更新する
- 復号 stage が TS payload のみを処理する
- 復号後 TS が既存 soft demux / DVR / AV へ流れる
- MediaCas close前に該当descramblerへsetKeyToken(VOID)する
```

完了条件:

```text
- token 未設定 / unresolved PID は復号成功扱いにならない
- key parity に応じて odd/even key が選択される
- 1 packet処理中に旧/new materialを混在させない
- adaptation field / PCR / continuity counter が壊れない
- 復号不能時に diagnostics が増える
- VOID成功後にMediaCas sessionをcloseする
```

---

## 11. B1 実装対象判定

### 11.1 B1 を実装対象に含める可否

```text
判定:
  可能。

条件:
  - B1SmartCardPath の ECM-only 経路を正式対応とする。
  - B1 EMM は恒久的に非対応とする。
  - EMMに依存する通電制御情報取得は恒久的に非対応とする。
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
2. ClearKeyをB25/B1 capabilityから分離して保持する。
3. B25/B1 の plugin descriptor は caSystemId 単位で一意に列挙する。
4. smartcard と yakisoba を同一 caSystemId の別 descriptor として列挙しない。
5. B25/B1 ICas 実装は MaleicacidCasPlugin に一本化する。
6. product TISは同一caSystemIdについて1個のlive MediaCas/CAS pluginを共有し、B25 EMMとECM sessionを同じpluginへ帰属させる。
7. B25 backend種別は各plugin instance内で一度だけbindし、そのpluginの全session/EMMで共有する。
8. smartcard_only / yakisoba_only / prefer_smartcard_then_yakisoba を正式profileとし、yakisoba_onlyのadvertiseにSmartCard完成を要求しない。
9. build typeだけからbackendを暗黙決定しない。
10. preferのCARD_UNKNOWN_TIMEOUTではYakisobaCasPathへ切り替えない。
11. plugin binding後backend failureでは同じpluginを別backendへ切り替えない。再選択は新plugin instanceで行う。
12. B1ではYakisobaCasPathへ切り替えない。
13. B1 EMMとEMM依存の通電制御情報取得・契約更新・権利更新は恒久的に非対応とする。
14. libyakisobaのin-process/daemon統合はproduct選択とし、別daemon自体をAOSP要件として必須化しない。
15. daemon採用時だけIPCのschema互換性、response対応付け、size bound、access control、bounded I/Oを満たす。
16. raw CW/key materialはBinder、TIS、通常logへ出さず、必要期間を越えて保持・永続化しない。
17. MediaCas session ID bytesをTuner key tokenとして使用し、別の公開tokenを生成しない。
18. provider incarnation、key epoch、固定no-wrap counter、peer credential+SELinux二重適用、特定zeroize primitive等を必須形式にしない。
19. processEcm() success時点で同session IDのstable linkからcomplete current materialを取得可能にする。
20. stale tokenが別sessionの鍵へ接続されないよう、live/retired linkageが残る期間はtokenを再割当てしない。
21. close/release/backend owner lossでは新規key resolveを遮断し、stale mutationをcurrent stateとして受理しない。
22. backend物理cleanupのretry/reset/taint方式はbackend resource契約へ委ね、固定service-global CleanupPending worker/tableを必須化しない。
23. MediaCas close前にMediaCas由来tokenを全descramblerからVOIDで解除する。
24. CAS HALはTS demux / TS packet復号 / AV / DVRを担当しない。
25. TS payload復号はTuner HAL descramblerの責務とする。
26. B25 advertise前にARIB STD-B25 Version 7.0日本語原本の該当受信機能力条項を確認し、product effective capacityが確認済み要求を満たすことを検証する。旧版英訳の具体値を7.0要求として代用しない。
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

- ARIB STD-B25  
  https://www.arib.or.jp/kikaku/kikaku_hoso/desc/std-b25.html

- Jacobsen v. Katzer  
  https://jolt.law.harvard.edu/digest/jacobsen-v-katzer

- Artifex v. Hancom  
  https://docs.justia.com/cases/federal/district-courts/california/candce/3:2016cv06982/305835/54

- GNU GPL FAQ  
  https://www.gnu.org/licenses/gpl-faq.html
