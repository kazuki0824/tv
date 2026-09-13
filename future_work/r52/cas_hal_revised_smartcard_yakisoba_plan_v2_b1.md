# CAS HAL 実装計画 改訂版 v6
## AOSP Media CAS 境界 + B25 SmartCard / Yakisoba / B1 SmartCard

## 0. 設計原則

AOSP 公開面には標準 `IMediaCasService` / `ICas` だけを公開し、SmartCard、Yakisoba、credential 取得、vendor IPC、KeySlotRegistry は vendor 内部へ閉じる。AOSP が規定しない内部 transport、slot table、retry 方式、generation/cookie の表現を公開 HAL 契約へ昇格させない。

B25 正式構成は `smartcard_only` / `yakisoba_only` / `prefer_smartcard_then_yakisoba`。B1 は `smartcard_only` ECM-only。`yakisoba_only` は有効な B25 構成であり、SmartCard 実装を advertise 条件にしない。

ClearKey は B25/B1 state から分離した AOSP reference-compatible path として同じ `IMediaCasService/default` に合成する。Maleicacid 固有 profile/backend/key state を ClearKey へ混在させない。

B25 の規範は採用 product で固定する ARIB STD-B25 日本語原本とする。ARIB の公開一覧では Version 7.0 が現行版であることを確認できるが、本書は 7.0 日本語原本の同時処理可能スクランブル鍵数/PID数等の具体値を確認済みとは扱わない。ARIB の改定履歴上、これらの規定は過去版で変更・明確化されているため、旧版英訳の数値を 7.0 の要求値として代用しない。B25 capability を広告する前に、採用する 7.0 日本語原本の該当受信機能力条項を確認し、product の effective capacity がその要求を満たすことを検証する。この原本照合が未完了なら ARIB capability gate は未成立とする。

## 1. AOSP 公開契約

`android.hardware.cas.IMediaCasService/default` を1個だけ公開し、同一 CA system ID を backend 別に重複列挙しない。

`enumeratePlugins()`、`isSystemIdSupported()`、`createPlugin()` は同じ immutable capability snapshot を使用し、同一 service lifetime 中に互いに矛盾する能力を返さない。

列挙されない system ID は AIDL transport 成功のまま次を返す。

```text
isSystemIdSupported      -> false
isDescramblerSupported   -> false
createPlugin             -> null
createDescrambler        -> null
```

B25/B1 は Media CAS 側 descrambler を公開せず、packet descramble は Tuner HAL `IDescrambler` だけが所有する。ClearKey は AOSP/VTS 互換 plugin/session/descrambler semantics に従う。

採用 AIDL に default-session method が存在する場合、その B25/B1 既定 session は `LIVE + MULTI2` とする。明示 `openSession(intent, mode)` は本製品が成功対応する `LIVE + MULTI2` だけを受理し、それ以外は状態不変のまま `ERROR_CAS_CANNOT_HANDLE` とする。AIDL revision に存在しない method を vendor 独自 AIDL として追加しない。

## 2. capability profile / advertise gate

capability profile は vendor image で固定し、service 起動時に1回だけ snapshot 化する。runtime property、TIS 入力、card 挿抜、Yakisoba backend の一時 health で変更しない。入力の path/serialization は内部実装だが、欠落・不正・重複・未知値を検出し、該当 B25/B1 capability だけを非広告にする。ClearKey には影響させない。

B25 共通 gate:

```text
- ICas lifecycle / AOSP status contract
- ECM / EMM complete-section input contract
- MediaCas session ID -> Tuner token bridge
- complete MULTI2 material の atomic publish / stable-slot rotation
- revoke / stale-token rejection
- 採用 ARIB STD-B25 Version 7.0 日本語原本の該当受信機能力条項を確認済み
- product の effective capacity が上記確認済み条項を満たすことを検証済み
- TIS -> MediaCas -> Tuner 結合確認
```

内部 slot/PID table 上限は実装詳細でよい。本書は旧版英訳等の具体数値を Version 7.0 の要求値として固定しない。7.0 日本語原本の該当条項を確認していない状態では、この gate を成立済みにしない。

profile 別 gate:

- `smartcard_only`: SmartCard path、credential 初期化、ECM/EMM、bounded I/O、card 抜去、close。
- `yakisoba_only`: Yakisoba adapter、B25 ECM/EMM、credential source、bounded operation、secret lifetime、採用統合形態に必要な access control、配布条件。別 daemon を使う場合はその IPC/failure 契約も検証する。
- `prefer_smartcard_then_yakisoba`: 上記両 gate + service-wide backend bind 判定 + timeout/unknown-state 非 fallback。

したがって `yakisoba_only` build は SmartCard 未搭載でも B25 を広告できる。Yakisoba 側 gate 未成立 image は B25 を広告しない。

B1 gate は B1 SmartCard ECM、B1 EMM 明示拒否、Yakisoba 非選択、generic MULTI2 key publish/rotation/revoke、close の確認とする。

## 3. B25 backend binding / service-wide ordering

`IMediaCasService.createPlugin(caSystemId, ...)` は同一 CA system ID に対して複数回呼ばれ得る一方、`ICas.processEmm()` は session ID を持たない。このため B25 backend 種別を session 単位または plugin instance 単位に独立選択しない。

B25 backend 種別の binding は同一 `IMediaCasService/default` 内の B25 CA system 全体で1個だけ共有する。

```text
B25BackendBinding:
  Unbound
  SmartCard
  Yakisoba
```

同時に存在する全 B25 plugin instance は同じ binding を観測する。各 plugin の private data、session table、listener、session-private data は独立してよいが、SmartCard/Yakisoba の backend 種別を plugin ごとに分裂させない。`processEmm()` は呼出し元 plugin の AOSP 状態を保持したまま、service-wide に選択済みの backend 種別へ配送する。

binding 選択は service の B25 owner が atomic に一意化する。同時に異なる B25 plugin instance から最初の backend-dependent operation が到達しても、backend 選択結果は1個だけ確定し、全 plugin が同じ結果を観測する。特定 lock/thread/counter 方式は規定しない。

- `smartcard_only`: B25 backend 種別は SmartCard。card 不在等は操作失敗とし Yakisoba へ切り替えない。
- `yakisoba_only`: B25 backend 種別は Yakisoba。SmartCard probe を行わず Yakisoba backend 障害時も SmartCard へ切り替えない。
- `prefer_smartcard_then_yakisoba`: service-wide binding が `Unbound` のとき、最初の B25 backend-dependent operation で1回だけ判定する。

```text
CARD_VALID                                      -> SmartCard
CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED
/ CARD_IO_UNAVAILABLE                           -> Yakisoba
CARD_UNKNOWN_TIMEOUT                            -> operation失敗、Unbound維持
```

binding 前に SmartCard 状態を確定できない場合は fallback しない。binding 後 backend failureでも、live B25 plugin/sessionが存在する状態で cross-backend fallbackしない。

別 backend への再選択を実装する場合は、B25 domain が quiescent であること、すなわち live B25 plugin/session がなく、旧 backend に対する mutation、cleanup、stale completion が新しい plugin/session stateへ影響し得ないことを確認した境界でのみ `Unbound` へ戻してよい。実装が service restart まで binding を維持してもよい。特定の rebind 機構を必須化しない。

`setPrivateData()` は plugin-local state とする。service-wide backend 種別が確定済みかどうかにかかわらず各 plugin が自身の committed private data を保持し、backend-dependent operationでその plugin contextへ必要な値を適用する。service-wide binding 自体へ plugin private data を混在させない。

backend が物理的に共有状態を持つ場合、EMM による entitlement/work-key 更新と各 plugin の ECM 処理の間に一貫した ordering を提供し、ECM が半端な EMM 更新 state を観測しないようにする。SmartCard では card I/O serialization、Yakisoba では採用 adapter の内部同期等で実現できるが、特定 thread/queue/version 方式を必須にしない。

stale plugin/session operation が現行 state を上書きしてはならない。実装は object identity、opaque cookie、counter 等を利用してよいが、特定 generation field や no-wrap counter を公開・内部 resource の必須形式にしない。

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

`Closing` / `Releasing` は caller から見て通常利用不能であり、key revoke 確定待ちを表す。backend 物理 cleanup だけが残る場合は `Closed` / `Released` へ進め、service-owned cleanup state として保持できる。cleanup worker/timer の具体方式は規定しない。

`Opening` では collision-safe session ID、registry reservation、backend open、private data 適用を prepare し、全て成功した場合だけ Active session ID を公開する。

各 session は mutating backend I/O を1件だけ in-flight にするか、同等の stale-completion 排除を行う。外部 I/O 開始前の session identity/lifecycle と、応答後の current state が一致し Active のままであることを確認してから private data/key material を commit する。

close/release が I/O 中に到達した場合は先に Closing/Releasing へ遷移して新規 I/O を遮断する。遅れて返った ECM/key 結果を Closed/Releasing state や別 session の registry slot へ publish しない。

`processEmm()` は、同じ service-wide backend 種別を使う他 plugin/session の ECM が半端な backend-global state を観測しないように ordering を持つ。1つの plugin の release 後に遅れて返った EMM 結果を、その plugin の live state へ反映しない。共有 backend 自体の正規な状態更新と、release 済み plugin state への stale completion 反映は区別する。

### 4.1 closeSession

最初の `closeSession()` は session を Closing へ遷移させ、以後の通常 session operation を拒否する。token の新規 resolve revoke を確定できたら caller-visible session を Closed へ進める。

- revoke 未確定: close 成功にしない。Closing を保持し、後続 `closeSession()` または service-owned cleanup が revoke を再試行する。
- revoke 確定後の backend close 失敗: caller-visible session は Closed のまま。backend cleanup だけを継続し、session を Active へ戻さない。
- Closed 到達後の通常 session operation は `ERROR_CAS_SESSION_NOT_OPENED`。

close 成功確定点は logical close + token 新規 resolve revoke であり、backend 物理 close 完了を Binder 成功の必須条件にしない。

### 4.2 release

最初の `release()` は plugin を Releasing へ遷移させ、新規 method/callback delivery を遮断し、その plugin が所有する全 session token の新規 resolve revoke を試行する。

- token revoke 未確定 entry が残る: release 成功にせず Releasing を保持し、後続 `release()` / service-owned cleanup で revoke を再試行する。
- 全 token revoke 確定: Released へ進み、backend close 失敗が残っていても caller-visible object を再 live 化しない。物理 cleanup は service-owned state で継続する。
- Released 後の `release()` は idempotent に成功してよい。その他通常 method は `ERROR_CAS_INVALID_STATE`。

1つの plugin の release は、他の live B25 plugin/session が使用する service-wide backend binding や backend state を無効化しない。service-wide rebindは第3節の quiescent 条件を満たす場合だけ行える。

AOSP reference の release と同様、backend recovery 完了まで Binder object を live に保つ設計にはしない。一方、外部 key registry を持つ本構成では token の新規 resolve 遮断だけは release 成功前に確定させる。

live plugin/session と未完了 cleanup ownership の総量は有限に bound する。具体的数値は product capacity と確認済み ARIB gate を満たす実装詳細とし、新規受理で安全に管理できる範囲を超える場合は backend mutation 前に `ERROR_CAS_RESOURCE_BUSY` として拒否する。

## 5. ICas method / input / error contract

method 成功確定点:

- `setPrivateData()`: plugin-local committed state が更新され、backend へ即時反映が必要な実装ではその反映も成功した時点。失敗時は旧値維持。
- `setSessionPrivateData()`: backend 成功後、対象 session がまだ current Active session であることを確認して state を commit。失敗時は旧値維持。
- `openSession*()`: service-wide backend binding、backend open、registry reservationが成立した後に Active session ID 公開。
- `processEcm()`: backend 応答後の current-session/lifecycle 再確認を通り、complete new material を stable slot へ atomic publish し、既 link descrambler を含め同 session ID から取得可能になった時点。
- `processEmm()`: service-wide に bind 済みの B25 backend が成功し、呼出し元 plugin の release 競合で stale result になっていないことを確認した時点。別 backend へ fallback しない。
- `closeSession()`: Closing→Closed に必要な token revoke 確定。
- `release()`: Releasing→Released に必要な、その plugin 所有 token の全 revoke 確定。

ECM/EMM は complete section byte sequence として CAS へ渡す。TS packet、PID、demux buffer を CAS HAL へ渡さない。empty、section framing/declared length 不整合、対象 CA system として処理不能な外形は backend I/O 前に拒否する。

B1 `processEmm()`、B25/B1 `provision()`、`refreshEntitlements()`、未定義 vendor event は `ERROR_CAS_CANNOT_HANDLE` とする。

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

backend が AOSP 専用 status を判定できる場合は `ERROR_CAS_DEVICE_REVOKED`、`ERROR_CAS_NEED_ACTIVATION`、`ERROR_CAS_NEED_PAIRING` 等へ対応付け UNKNOWN へ潰さない。未実装を成功へ丸めない。

listener notification は state commit 後に行う。外部 callback の blocking/reentrancy により half-committed state を観測・変更させないことを必須とするが、特定の mutex/unlock 構成を必須化しない。state を保護する lock を使う実装では、callback が再入可能な同じ state lock を保持したまま外部 Binder callback を呼ばない。listener failure で commit 済み state を rollback しない。現設計では vendor scheme event 番号を定義しないため、診断目的だけで `onEvent()` / `onSessionEvent()` を合成しない。AOSP listener method を使用する場合は AIDL 引数契約をそのまま守り、ECM/EMM 本文、private data、raw/prepared key material を listener data へ含めない。

## 6. SmartCard path

同一 physical card への I/O は単一 owner が直列化するか、同等に card state mutation の順序を一意化する。card I/O に同期 lock を使う実装では、その lock を保持したまま外部 Binder callback を呼ばない。open/reset/APDU 等の同期 I/O は無期限に caller を占有せず、deadline/cancellation/非同期境界等で bounded completion を保証する。

binding 前 probe で card 状態を期限内に確定できない場合は `CARD_UNKNOWN_TIMEOUT` 相当の一時失敗として fallback しない。binding 後に request を送信して結果不明となった session は Failed として新規処理を遮断し、token revoke/close へ進む。同じ service-wide binding のまま Yakisoba へ切り替えない。

bind 済み SmartCard の抜去または恒久的 card invalidation を検出した場合、その card state に依存する全 B25 Active session を Failed へ遷移させ、新規 resolve を revoke する。live B25 plugin/session が残る間は Yakisoba へ切り替えない。後続の新 session を同じ SmartCard binding で受理する場合は、card probe/reset/credential 初期化を再実行して新 session として成立させる。

B25 system key/CBC 初期値は検証済み card 初期化応答から取得する。B1 は採用 B1 protocol の検証済み応答から供給元を確定し、B25 配置を推測して流用しない。

SmartCard I/O component は product の権限分離要件に応じて CAS service 内または別 vendor privilege domain に置ける。別 domain に分離する場合は CAS service へ不要な card-reader device access を付与せず、adapter 側へ必要最小限の権限を与える。別 process/socket 自体を AOSP 要件として必須化しない。

## 7. Yakisoba path

Yakisoba backend は B25 ECM/EMM を提供する内部 adapter として扱い、AOSP 公開面へ統合形態を露出しない。libyakisoba を CAS service 内で直接利用する in-process adapter と、別 vendor daemon に閉じて vendor-local IPC で利用する adapter の双方を許す。どちらを採用するかは product の権限分離、ABI、保守、配布条件に応じて決める。

libyakisoba を直接 link する場合は、その link 形態を含む採用 revision のライセンス条件を満たす。別 daemon 化を GPL 義務消滅の根拠にしない。B1 request はどちらの統合形態でも拒否する。

全 Yakisoba adapter に共通して次を要求する。

```text
- B25 ECM/EMM inputを他CA systemやstale session stateと混同しない
- malformed input/resultを成功へ丸めない
- backend operationが無期限にcallerを占有しない
- raw key / ECM / EMM / credentialを通常logへ出さない
- backend owner loss後のstale resultをcurrent stateとして受理しない
```

### 7.1 daemon/IPC を採用する場合

別 daemon を採用した場合だけ、IPC は次の意味契約を満たす。

```text
- request/response schemaの互換性を保証し、不互換またはmalformed frameを拒否する
- request と response を一意に対応付け、別requestのresponseを受理しない
- stale session/plugin request を current state として受理しない
- request/response sizeをboundedにする
- 未許可主体がdaemonへ接続・mutationできないようにする
- connect/write/readを含むI/Oが無期限にcallerを占有しない
- raw key / ECM / EMM / credentialを通常logへ出さない
```

明示 protocol-version field は実装選択であり必須ではない。固定 schema を同時更新する構成でも、version field を持つ構成でも、実際に使用する両 endpoint が互換でない入力を成功扱いしないことを要求する。

IPC access-control は product の threat model と Android process 構成に応じ、SELinux domain、socket ownership、peer credential 等の必要な仕組みで実現する。SELinux と peer credential の両方を無条件の必須条件にはしない。必要なのは許可された CAS service 以外が接続・mutationできないことである。

送信後に response を受け取れない場合は、backend 側 mutation の成否を確定できない限り outcome unknown とする。同一 request identity に対する replay/idempotency を backend contract が保証し、二重 mutation が起きないことを検証できる実装では安全な再送を許してよい。保証がない場合は自動再送しない。outcome unknown のまま別 backend へ fallback して state を分岐させない。

- open outcome unknown: session ID を公開せず、backend 側に残った可能性のある session を安全に破棄できる cleanup path を実行する。idempotent close はその実装方法の一つであり、唯一の必須方式にはしない。
- ECM outcome unknown: session を Failed、registry publish なし、revoke/cleanup。replay-safe が保証される場合だけ同一 backend で再送を選べる。
- EMM outcome unknown: state を成功扱いせず、replay-safe が保証されない限り自動再送しない。service-wide binding は維持する。

### 7.2 owner loss / restart

Yakisoba backend の owner loss/restart を検出した場合、旧 backend state に依存する全 B25 Active session を Failed へ遷移させ、新規 resolve を revoke する。live B25 plugin/session が残る間は SmartCard へ切り替えない。

後続の新 session を同じ Yakisoba binding で受理する場合は、新 owner の利用可能状態を確認し、呼出し元 plugin の committed private data を必要に応じて再適用してから open する。旧 session を新 owner へ暗黙継承しない。restart/stale-owner の識別方式は in-process object lifetime、connection lifetime、opaque cookie、service manager state 等の実装詳細とする。

Yakisoba から受領した key material は必要期間を越えて保持・永続化しない。mutable raw buffer を所有する場合の消去、secure handle の destroy/release 等は表現に応じて行い、特定の zeroize primitive を必須化しない。

## 8. KeySlotRegistry / Tuner boundary

B25 鍵詳細は `future_work/r52/b25_key_slot_registry_contract.md` を正本とする。B1 も Tuner 側では同じ generic MULTI2 stable-slot semantics と token lifetime/revoke 規則を再利用し、B1 protocol/credential 意味は CAS 側 adapter に閉じる。Tuner HAL へ B25/B1 識別を要求しない。

B25/B1 公開 Tuner token は `MediaCas.Session.getSessionId()` bytes そのものとし、1..16 bytes、opaque、raw key 非包含とする。token は live/retired linkage が残る間は別 session へ再割当てしない。process lifetime 全体で no-reuse にする実装を選んでもよいが、必須契約にはしない。

ECM 成功前は unresolved でよく、ECM 成功時は complete current material を同 session ID の stable link から取得可能にする。close/release/backend owner loss/fatal failure では新規 resource 取得を revoke する。

TIS は backend 種別を解釈しない。Tuner HAL は token→stable slot linkage、current material 取得、PID linkage、TS payload-only MULTI2 だけを担当する。

MediaCas 由来 token を保持する全 descrambler で `setKeyToken(VOID)` が成功した後に MediaCas session を close する。VOID 成功を新規 packet 利用停止の linearization point とし、既取得内部 key 参照はその処理終了まで保持し、最後の参照解放後に material 表現に応じた秘密情報破棄を行う。追加 framework API は導入しない。

具体的な `Reserve/Publish/Revoke` API 名、owner/generation counter、key epoch、TTL、slot 上限、wire magic、retry 回数は内部実装選択であり必須設計にしない。effective capacity の ARIB 適合は第2節の原本照合 gate で判断する。

## 9. product integration / license

CAS service と Tuner key bridge は vendor 側へ閉じ、各 process/component へ必要な permission だけを与える。

- `yakisoba_only`: Yakisoba adapter + 必要 credential + 採用統合形態に必要な access-control policy + license 成果物。SmartCard component 不要。別 daemon は必須ではない。
- `smartcard_only`: SmartCard path/component。Yakisoba component 不要。
- `prefer_smartcard_then_yakisoba`: 両 backend。

libyakisoba を同梱・改変・linkする場合は採用 revision と統合形態に適用される GPL-3.0 配布条件を満たす。daemon 分離を GPL 義務消滅の根拠にしない。B1 参照実装を移植/link する場合は採用 revision の license 条件を固定する。

module 名、socket path、wire field 値、owner cookie/generation の形式等の内部名称・表現は AOSP 公開契約にせず採用実装内で一意に定義する。

## 10. validation

```text
- ClearKey AOSP/VTS compatibility
- capability enumerate/support/createの同一snapshot整合
- unknown ID false/null
- B25/B1 explicit session: LIVE+MULTI2成功、非対応組合せ拒否
- smartcard_only B25 / yakisoba_only B25 / prefer各probe結果
- 複数B25 plugin instanceの同時first-bindでservice-wide backend種別が一意
- 同一service binding中でSmartCard/Yakisoba plugin混在なし
- plugin-local private dataとservice-wide backend bindingの責務分離
- EMM updateと全plugin ECMの一貫したordering
- B1 ECM-only + generic stable-slot rotation
- AOSP status mapping / complete section validation
- binding後cross-backend fallbackなし
- quiescentでない状態のservice-wide rebind拒否
- close/releaseとin-flight ECM/EMM競合、late publishなし
- revoke失敗時Closing/Releasing維持
- revoke成功後backend close失敗時はClosed/Released維持 + cleanup retry
- 1 plugin releaseで他pluginのbackend binding/stateを破壊しない
- bounded admission / cleanup accumulation
- release idempotence / post-release invalid-state
- AOSP listener引数契約 / callback reentrancyでhalf-committed stateを露出しない
- SmartCard抜去時、影響する全B25 session revoke
- Yakisoba in-process/daemon各採用形態でB25 ECM/EMM成立
- daemon採用時のみIPC schema/response対応/bounds/access-control/I/O boundednessを実証
- outcome-unknownで二重mutationを起こさないこと
- replay-safeを実装する場合の同一request再送安全性
- backend owner loss時のstale mutation拒否
- ARIB STD-B25 Version 7.0日本語原本の該当能力条項を確認済み
- 確認済みARIB条項に対するproduct effective capacity検証
- stable link上の連続ECM key rotation
- stale ECM completionがcurrent materialを上書きしない
- token revoke/ref drain/secret destruction
- VOID token -> MediaCas close
```

## 11. 最終固定事項

1. `IMediaCasService/default` は1個だけ公開し、capability query/create は同一 snapshot を使う。
2. ClearKey を B25/B1 能力から分離する。
3. B25 は3 profile を正式構成とし、`yakisoba_only` の advertise に SmartCard 完成を要求しない。
4. B1 は SmartCard ECM-only とする。
5. capability profile は image 固定・service lifetime 中不変とする。
6. backend 差を AOSP descriptor へ露出しない。
7. B25 backend 種別は同一 service の B25 CA system 全体で一意に bindし、同時に存在する全B25 plugin instanceで共有する。
8. `prefer_smartcard_then_yakisoba` の binding 前 SmartCard状態未確定ではfallbackしない。
9. live B25 plugin/sessionが存在する binding 後 backend failureでは別backendへ切り替えない。rebindを実装する場合はB25 domainのquiescent境界だけで行う。
10. packet descramble は Tuner HAL だけが所有する。
11. stale operation/completion を current state として commit しない。識別方式は実装詳細とする。
12. close/release と競合した遅延 I/O 結果を publish しない。
13. key revoke 未確定と backend 物理 cleanup 失敗を区別し、revoke 済み object/session を cleanup 失敗で再 live 化しない。
14. live/cleanup ownership を bounded にし、安全に管理できる範囲を超える新規受理を RESOURCE_BUSY で拒否する。
15. listener failure で commit 済み state を rollbackせず、callback実装方式ではなくhalf-committed stateを外部へ露出しないことを契約とする。
16. backend operation は caller を無期限に占有せず、outcome unknown を成功扱いしない。再送は replay-safe が証明された場合だけ許す。
17. Tuner token は stale linkage が残る間、別 session へ再割当てしない。
18. backend owner loss では影響 session の新規 resource 取得を遮断し、旧 owner の後着 mutation を拒否する。
19. `processEcm()` 成功時に stable link から complete current material を取得可能にする。
20. MediaCas close 前に MediaCas 由来 token を全 descrambler から VOID で解除する。
21. raw key material を Binder、TIS、通常 log へ出さず、必要期間を越えて保持・永続化しない。
22. B25 advertise 前に ARIB STD-B25 Version 7.0 日本語原本の該当能力条項を確認し、product effective capacityが確認済み要求を満たすことを検証する。旧版英訳の数値を7.0要求として代用しない。
23. CAS HAL は TS demux / AV / DVR を担当しない。
24. Yakisoba の in-process/daemon 統合は product 選択とし、別 daemon/IPC 自体をAOSP要件として必須化しない。
25. provider incarnation、key epoch、固定no-wrap counter、明示protocol-version field、peer credential+SELinuxの二重適用、idempotent-close方式、特定zeroize primitive等の具体方式を、必要な意味契約より強い必須条件として固定しない。
