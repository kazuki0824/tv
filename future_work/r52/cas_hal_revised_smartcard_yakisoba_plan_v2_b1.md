# CAS HAL 実装計画 改訂版 v4
## AOSP Media CAS 境界 + B25 SmartCard / Yakisoba / B1 SmartCard

## 0. 設計原則

AOSP 公開面には標準 `IMediaCasService` / `ICas` だけを公開し、SmartCard、Yakisoba、credential 取得、vendor IPC、KeySlotRegistry は vendor 内部へ閉じる。AOSP が規定しない内部 transport、slot table、retry 方式、generation/cookie の表現を公開 HAL 契約へ昇格させない。

B25 正式構成は `smartcard_only` / `yakisoba_only` / `prefer_smartcard_then_yakisoba`。B1 は `smartcard_only` ECM-only。`yakisoba_only` は有効な B25 構成であり、SmartCard 実装を advertise 条件にしない。

ClearKey は B25/B1 state から分離した AOSP reference-compatible path として同じ `IMediaCasService/default` に合成する。Maleicacid 固有 profile/backend/key state を ClearKey へ混在させない。

B25 の規範は採用 product で固定する ARIB STD-B25 日本語原本とする。本設計では Version 7.0 を対象 revision とし、同時処理可能なスクランブル鍵/PID 等の数値規定を本書へ複製せず、同 revision の該当条項を capability gate の SSOT として参照する。

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

capability profile は vendor image で固定し、service 起動時に1回だけ snapshot 化する。runtime property、TIS 入力、card 挿抜、daemon health で変更しない。入力の path/serialization は内部実装だが、欠落・不正・重複・未知値を検出し、該当 B25/B1 capability だけを非広告にする。ClearKey には影響させない。

B25 共通 gate:

```text
- ICas lifecycle / AOSP status contract
- ECM / EMM complete-section input contract
- MediaCas session ID -> Tuner token bridge
- complete MULTI2 material の atomic publish / stable-slot rotation
- revoke / stale-token rejection
- ARIB STD-B25 Version 7.0 の同時 key/PID 処理能力条項を満たす
- TIS -> MediaCas -> Tuner 結合確認
```

内部 slot/PID table 上限は実装詳細でよいが、B25 を広告する product の effective capacity は ARIB 条項を下回ってはならない。

profile 別 gate:

- `smartcard_only`: SmartCard path、credential 初期化、ECM/EMM、timeout、card 抜去、close。
- `yakisoba_only`: Yakisoba daemon/path、ECM/EMM、credential/access control、bounded IPC、timeout/切断/close、配布条件。
- `prefer_smartcard_then_yakisoba`: 上記両 gate + backend bind 判定 + timeout 非 fallback。

したがって `yakisoba_only` build は SmartCard 未搭載でも B25 を広告できる。Yakisoba 側 gate 未成立 image は B25 を広告しない。

B1 gate は B1 SmartCard ECM、B1 EMM 明示拒否、Yakisoba 非選択、generic MULTI2 key publish/rotation/revoke、close の確認とする。

## 3. B25 backend binding / plugin-wide ordering

`ICas.processEmm()` は session ID を持たないため、backend を session 単位に選択しない。各 B25 plugin instance は `Unbound | SmartCard | Yakisoba` の binding を1個だけ所有し、commit 後は release まで変更しない。全 session、EMM、credential context は同じ backend を使う。

binding 選択は plugin owner が atomic に直列化する。同時に複数の最初の backend-dependent operation が到達しても probe/bind transaction は1個だけ実行し、他 operation はその確定結果を観測する。半端な binding を公開しない。

- `smartcard_only`: plugin 生成時に SmartCard へ bind。card 不在等は操作失敗とし Yakisoba へ切り替えない。
- `yakisoba_only`: plugin 生成時に Yakisoba へ bind。SmartCard probe を行わず daemon 障害時も SmartCard へ切り替えない。
- `prefer_smartcard_then_yakisoba`: 最初の `openSession*()` または `processEmm()` で1回だけ判定する。

```text
CARD_VALID                                      -> SmartCard
CARD_ABSENT / CARD_INVALID / CARD_UNSUPPORTED
/ CARD_IO_UNAVAILABLE                           -> Yakisoba
CARD_UNKNOWN_TIMEOUT                            -> operation失敗、Unbound維持
```

binding 前 timeout では fallback しない。binding 後 backend failure でも同 plugin 内で cross-backend fallback しない。再判定には現在の plugin を release して新しい plugin instance を生成する。

`setPrivateData()`、backend binding、`processEmm()` は plugin-global state と一貫した ordering を持つ。binding は committed private-data snapshot を選択 backend へ適用してから確定し、同時更新と競合しても plugin-local state と backend state が異なる成功状態を公開しない。version counter の有無や形式は実装詳細とする。

backend は EMM による entitlement/work-key 更新と ECM 処理の間に linearizable な ordering を提供し、ECM が半端な EMM 更新 state を観測しないようにする。SmartCard では card I/O serialization、Yakisoba では daemon 内部同期等で実現できるが、特定 thread/queue/version 方式を必須にしない。

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

`processEmm()` は plugin-wide mutation として他の EMM/backend-global state mutation と整合した ordering を持つ。release 後に遅れて返った結果を live state へ反映しない。

### 4.1 closeSession

最初の `closeSession()` は session を Closing へ遷移させ、以後の通常 session operation を拒否する。token の新規 resolve revoke を確定できたら caller-visible session を Closed へ進める。

- revoke 未確定: close 成功にしない。Closing を保持し、後続 `closeSession()` または service-owned cleanup が revoke を再試行する。
- revoke 確定後の backend close 失敗: caller-visible session は Closed のまま。backend cleanup だけを継続し、session を Active へ戻さない。
- Closed 到達後の通常 session operation は `ERROR_CAS_SESSION_NOT_OPENED`。

close 成功確定点は logical close + token 新規 resolve revoke であり、backend 物理 close 完了を Binder 成功の必須条件にしない。

### 4.2 release

最初の `release()` は plugin を Releasing へ遷移させ、新規 method/callback delivery を遮断し、全 session token の新規 resolve revoke を試行する。

- token revoke 未確定 entry が残る: release 成功にせず Releasing を保持し、後続 `release()` / service-owned cleanup で revoke を再試行する。
- 全 token revoke 確定: Released へ進み、backend close 失敗が残っていても caller-visible object を再 live 化しない。物理 cleanup は service-owned state で継続する。
- Released 後の `release()` は idempotent に成功してよい。その他通常 method は `ERROR_CAS_INVALID_STATE`。

AOSP reference の release と同様、backend recovery 完了まで Binder object を live に保つ設計にはしない。一方、外部 key registry を持つ本構成では token の新規 resolve 遮断だけは release 成功前に確定させる。

live plugin/session と未完了 cleanup ownership の総量は有限に bound する。具体的数値は product capacity と ARIB gate を満たす実装詳細とし、新規受理で安全に管理できる範囲を超える場合は backend mutation 前に `ERROR_CAS_RESOURCE_BUSY` として拒否する。

## 5. ICas method / input / error contract

method 成功確定点:

- `setPrivateData()`: plugin-local 値と、binding 済みなら backend 値が同じ committed state になった時点。失敗時は旧値維持。
- `setSessionPrivateData()`: backend 成功後、対象 session がまだ current Active session であることを確認して state を commit。失敗時は旧値維持。
- `openSession*()`: backend open + registry reservation 後に Active session ID 公開。
- `processEcm()`: backend 応答後の current-session/lifecycle 再確認を通り、complete new material を stable slot へ atomic publish し、既 link descrambler を含め同 session ID から取得可能になった時点。
- `processEmm()`: bind 済み B25 backend が成功し、release 競合がないことを確認した時点。別 backend へ fallback しない。
- `closeSession()`: Closing→Closed に必要な token revoke 確定。
- `release()`: Releasing→Released に必要な全 token revoke 確定。

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

listener は state commit 後かつ内部 lock 外で呼ぶ。listener failure で commit 済み state を rollback しない。現設計では vendor scheme event 番号を定義しないため、診断目的だけで `onEvent()` / `onSessionEvent()` を合成しない。AOSP listener method を使用する場合は AIDL 引数契約をそのまま守り、ECM/EMM 本文、private data、raw/prepared key material を listener data へ含めない。

## 6. SmartCard path

同一 physical card への I/O は単一 owner が直列化するか、同等に card state mutation の順序を一意化する。card I/O lock 中に Binder callback を呼ばない。open/reset/APDU は有限 deadline を持つ。

binding 前 probe timeout は `CARD_UNKNOWN_TIMEOUT` として fallback しない。binding 後に request を送信して結果不明となった session は Failed として新規処理を遮断し、token revoke/close へ進む。同 plugin で Yakisoba へ切り替えない。

bind 済み SmartCard の抜去または恒久的 card invalidation を検出した場合、その card state に依存する Active session を Failed へ遷移させ、新規 resolve を revoke する。同 plugin 内で Yakisoba へ切り替えない。後続の新 session を同じ SmartCard binding で受理する場合は、card probe/reset/credential 初期化を再実行して新 session として成立させる。

B25 system key/CBC 初期値は検証済み card 初期化応答から取得する。B1 は採用 B1 protocol の検証済み応答から供給元を確定し、B25 配置を推測して流用しない。

SmartCard I/O component は product の権限分離要件に応じて CAS service 内または別 vendor privilege domain に置ける。別 domain に分離する場合は CAS service へ不要な card-reader device access を付与せず、adapter 側へ必要最小限の権限を与える。別 process/socket 自体を AOSP 要件として必須化しない。

## 7. Yakisoba path

CAS HAL は libyakisoba へ直接 link せず、vendor partition 内の別 daemon へ B25 ECM/EMM を vendor-local IPC で要求する。B1 request は拒否する。daemon endpoint/credential を一般 app、TIS、Tuner HAL、shell へ公開しない。

IPC は次の意味契約を満たす。

```text
- protocol version / operation / B25 identity を検証する
- request と response を一意に対応付け、別requestのresponseを受理しない
- stale session/plugin request を current state として受理しない
- request/response sizeをboundedにする
- 未許可主体がdaemonへ接続・mutationできないようにする
- connect/write/readを含む有限deadlineを持つ
- malformed/mismatched responseを成功へ丸めない
- raw key / ECM / EMM / credentialを通常logへ出さない
```

IPC access-control は product の threat model と Android process 構成に応じ、SELinux domain、socket ownership、peer credential 等の必要な仕組みで実現する。SELinux と peer credential の両方を無条件の必須条件にはしない。必要なのは許可された CAS service 以外が接続・mutationできないことである。

送信 0 byte が確定した失敗だけを operation 未開始として扱う。1 byte 以上送信後の timeout、切断、response 不整合は outcome unknown として自動再送・別 backend fallback をしない。

- open outcome unknown: 同じ session identity を idempotent close し session ID を公開しない。
- ECM outcome unknown: session を Failed、registry publish なし、revoke/close。
- EMM outcome unknown: `ERROR_CAS_INVALID_STATE`、自動再送なし、binding 維持。

Yakisoba close は同じ session identity について未作成/終了済みでも idempotent に扱う。`yakisoba_only` は SmartCard probe を行わず、daemon/credential 一時利用不能時も descriptor 集合を変えず操作失敗とする。

Yakisoba daemon の death/restart を検出した場合、旧 daemon state に依存する Active session を Failed へ遷移させ、新規 resolve を revoke する。同 plugin 内で SmartCard へ切り替えない。後続の新 session を同じ Yakisoba binding で受理する場合は、新 daemon との接続確立と committed plugin private data の再適用を完了してから open する。旧 session を新 daemon へ引き継がない。restart/stale-owner の識別方式は connection lifetime、opaque cookie、service manager state 等の実装詳細とする。

Yakisoba から受領した key material は registry commit に必要な最短寿命だけ保持し、一時 response/encode buffer は commit または失敗後に zeroize する。

## 8. KeySlotRegistry / Tuner boundary

B25 鍵詳細は `future_work/r52/b25_key_slot_registry_contract.md` を正本とする。B1 も Tuner 側では同じ generic MULTI2 stable-slot semantics と token lifetime/revoke 規則を再利用し、B1 protocol/credential 意味は CAS 側 adapter に閉じる。Tuner HAL へ B25/B1 識別を要求しない。

B25/B1 公開 Tuner token は `MediaCas.Session.getSessionId()` bytes そのものとし、1..16 bytes、opaque、raw key 非包含とする。token は live/retired linkage が残る間は別 session へ再割当てしない。process lifetime 全体で no-reuse にする実装を選んでもよいが、必須契約にはしない。

ECM 成功前は unresolved でよく、ECM 成功時は complete current material を同 session ID の stable link から取得可能にする。close/release/backend owner loss/fatal failure では新規 resource 取得を revoke する。

TIS は backend 種別を解釈しない。Tuner HAL は token→stable slot linkage、current material 取得、PID linkage、TS payload-only MULTI2 だけを担当する。

MediaCas 由来 token を保持する全 descrambler で `setKeyToken(VOID)` が成功した後に MediaCas session を close する。VOID 成功を新規 packet 利用停止の linearization point とし、既取得内部 key 参照は drain 後 zeroize する。追加 framework API は導入しない。

具体的な `Reserve/Publish/Revoke` API 名、owner/generation counter、key epoch、TTL、slot 上限、wire magic、retry 回数は内部実装選択であり必須設計にしない。ただし effective capacity は第2節 ARIB gate を満たす。

## 9. product integration / license

CAS service と Tuner key bridge は vendor 側へ閉じ、各 process/component へ必要な permission だけを与える。

- `yakisoba_only`: Yakisoba daemon + credential + access-control policy + license 成果物。SmartCard component 不要。
- `smartcard_only`: SmartCard path/component。Yakisoba daemon 不要。
- `prefer_smartcard_then_yakisoba`: 両 backend。

libyakisoba を同梱・改変する場合は採用 revision の GPL-3.0 配布条件を満たす。daemon 分離を GPL 義務消滅の根拠にしない。B1 参照実装を移植/link する場合は採用 revision の license 条件を固定する。

module 名、socket path、wire field 値、owner cookie/generation の形式等の内部名称・表現は AOSP 公開契約にせず採用実装内で一意に定義する。

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
- AOSP listener引数契約 / secret非露出
- SmartCard抜去時session revoke
- Yakisoba daemon restart時、旧session revoke + 新session再初期化
- Yakisoba IPC access-controlを選択した機構で実証
- listener failureでcommit済みstate非rollback
- Yakisoba outcome-unknown / temporary-key zeroize
- backend owner loss時のstale mutation拒否
- ARIB STD-B25 Version 7.0の同時key/PID能力gate
- stable link上の連続ECM key rotation
- stale ECM completionがcurrent materialを上書きしない
- token revoke/ref drain/zeroize
- VOID token -> MediaCas close
```

## 11. 最終固定事項

1. `IMediaCasService/default` は1個だけ公開し、capability query/create は同一 snapshot を使う。
2. ClearKey を B25/B1 能力から分離する。
3. B25 は3 profile を正式構成とし、`yakisoba_only` の advertise に SmartCard 完成を要求しない。
4. B1 は SmartCard ECM-only とする。
5. capability profile は image 固定・service lifetime 中不変とする。
6. backend 差を AOSP descriptor へ露出しない。
7. B25 backend は plugin instance 単位で atomic に一度だけ bind し全 session/EMM で共有する。
8. binding 前 SmartCard timeout では fallback しない。
9. binding 後 backend failure では同 plugin を別 backend へ切り替えない。
10. packet descramble は Tuner HAL だけが所有する。
11. stale operation/completion を current state として commit しない。識別方式は実装詳細とする。
12. close/release と競合した遅延 I/O 結果を publish しない。
13. key revoke 未確定と backend 物理 cleanup 失敗を区別し、revoke 済み object/session を cleanup 失敗で再 live 化しない。
14. live/cleanup ownership を bounded にし、安全に管理できる範囲を超える新規受理を RESOURCE_BUSY で拒否する。
15. listener failure で commit 済み state を rollback せず、AOSP listener の引数契約を狭めない。
16. backend I/O は bounded とし送信後結果不明を成功/fallback へ丸めない。
17. Tuner token は stale linkage が残る間、別 session へ再割当てしない。
18. backend owner loss では影響 session の新規 resource 取得を遮断し、旧 owner の後着 mutation を拒否する。
19. `processEcm()` 成功時に stable link から complete current material を取得可能にする。
20. MediaCas close 前に MediaCas 由来 token を全 descrambler から VOID で解除する。
21. raw key material を Binder、TIS、通常 log へ出さない。
22. B25 advertise 時の effective key/PID capacity は ARIB STD-B25 Version 7.0 の対象条項を満たす。
23. CAS HAL は TS demux / AV / DVR を担当しない。
24. provider incarnation、key epoch、peer credential+SELinux の二重適用等の具体方式を、必要な意味契約より強い必須条件として固定しない。
