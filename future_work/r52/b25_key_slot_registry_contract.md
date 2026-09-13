# r52 B25 KeySlotRegistry 内部鍵資源契約

本書は r52 の B25 CAS HAL と Tuner HAL descrambler の間で共有する内部鍵資源契約の正本である。AOSP の MediaCas session ID / Tuner `IDescrambler.setKeyToken()` に流す byte sequence は opaque な参照値のままとし、B25 鍵素材を AOSP Binder、TIS、通常 log へ公開しない。

## 1. AOSP 公開境界

- MediaCas が公開した session ID bytes を、そのまま Tuner key token とする。TIS 向けの別 token を生成しない。
- TIS は session ID bytes を `IDescrambler.setKeyToken()` へ渡すだけで、内部を解析・再構成しない。
- Tuner HAL は token を vendor 内部 registry で解決し、解決済み MULTI2 resource だけを packet descramble path へ渡す。
- token へ system key、CBC 初期値、odd/even Ks、CA system ID、owner identity、更新番号等を埋め込むことを公開契約にしない。
- AOSP AIDL / VINTF へ vendor 独自 field を追加しない。
- session ID は ECM 成功前でも公開され得る。その時点では identity 予約だけが成立していてよく、完全な鍵 context が publish されるまで resolve 不可とする。

## 2. Tuner 側の最小鍵 resource

Tuner 側が token 解決後に必要とする最小の論理 resource は次で足りる。

```text
Multi2KeyMaterial
  system_key
  cbc_initial_value
  even_ks
  odd_ks
```

CA system ID、MediaCas session identity、SmartCard/Yakisoba 種別、CAS owner の世代、鍵更新番号は CAS-domain / registry-domain の管理情報であり、`Multi2KeyMaterial` の必須 field にしない。CAS adapter は CAS 固有状態を検証したうえで generic MULTI2 material へ変換し、Tuner HAL は B25/B1 や provider identity の意味を解釈して分岐しない。

registry は、token から stable slot を引き、その slot が現在有効な complete material を指すか、revoke 済みかを判定できればよい。stale owner や stale update を識別するための generation、cookie、connection identity、version counter 等を内部実装として使用してよいが、それらを Tuner 側鍵 resource の必須 layout や AOSP token の形式にしない。

## 3. stable link と key rotation

`IDescrambler.setKeyToken(token)` は token をその時点の key bytes の snapshot へ固定する操作ではなく、stable な registry slot identity へ link する操作とする。

- 初回 ECM 成功後に token を descrambler へ設定した後、同じ MediaCas session の後続 ECM で鍵が更新されても、TIS が同じ token を設定し直す必要はない。
- registry は同じ stable slot identity の current material を atomic に差し替える。既 link descrambler の新規 packet 処理は更新後 material を参照する。
- 鍵切替と競合して既に material 参照を取得済みの packet は、その取得済み material で完了してよい。1 packet 処理中に旧/new material の field を混在させない。
- revoke / CAS owner loss 後は stable link が残っていても新規 material 取得を拒否する。既取得参照だけを drain する。
- token を別 slot identity へ付け替えない。slot/resource 本体を回収しても、旧 token を別 session に再割当てして stale link が別鍵へ接続される状態を作らない。

この indirection は継続的 ECM key rotation を既存 AOSP `setKeyToken()` linkage だけで成立させるための内部契約であり、AOSP へ更新 API を追加するものではない。数値の `key_epoch` を持つこと自体は必須条件ではない。

## 4. 所有権と供給経路

- system key / CBC 初期値は service lifetime 中に B25 用として一意に選択された backend 種別の下で、各 plugin/session の CAS context が所有する。B25 実カードでは検証済み card 初期化応答から取得し、このためだけの外部 secure store / factory provisioning を必須にしない。
- Yakisoba 等で外部 credential が必要な場合だけ、その供給元と access control を product 側で固定する。一般 property、公開 API、TIS、Tuner HAL、world-readable file を credential source にしない。
- odd/even Ks は CAS session が ECM/backend processing の結果として所有し、ECM 成功時に current material を更新する。
- raw key material を CAS から Tuner 側 registry へ運ぶ必要がある実装では、許可された CAS 側 owner だけが mutation できる vendor 内部境界を使用する。TIS や一般 app が publish/revoke mutation endpoint へ到達できてはならない。
- access-control の実装方式は product の process/IPC 構成に応じて決める。SELinux domain、socket ownership、peer credential、Binder caller identity 等のうち必要な仕組みを使用し、脅威モデル上不要な仕組みまで重複必須化しない。必要な性質は、未許可主体が接続・publish・revoke できないことである。
- registry は current CAS owner からの mutation だけを受理し、owner handover 後の旧接続・旧 request・revoke 済み token への publish を拒否する。owner identity の具体表現を resource layout に固定しない。
- raw-key の一時表現は必要期間を越えて保持・永続化しない。実装が mutable raw buffer を所有する場合は、その表現に適した消去処理を適用してから再利用/解放し、secure handle 等を使う場合は対応する destroy/release を行う。特定の zeroize API や memory primitive を必須化しない。
- 同一 service lifetime の B25 plugin instance 間で SmartCard/Yakisoba backend 種別を混在させない。plugin/session ごとの private data、session state、Ks は独立して保持してよいが、異なる backend 種別の credential context を同じ B25 service lifetime に併存させない。

## 5. token identity と寿命

- MediaCas session ID は 1..16 bytes の opaque value とし、Tuner の VOID key token と同一値を発行しない。
- session ID 公開前に、現在 live な identity およびまだ stale linkage/retired reference が残る identity と衝突しないことを保証する。
- token は、その slot の revoke、必要な descrambler からの VOID unlink、内部参照 drain が完了するまで別 session へ再割当てしない。
- process lifetime 全体で token を永久に再利用しない実装を選んでもよいが、それ自体を必須契約にはしない。必要なのは stale token が別 session の鍵へ解決されないことである。
- current CAS owner の death/disconnect を検出した場合、その owner に属する entry の新規 resolve を遮断する。新 owner を受け入れる場合は、旧 owner からの後着 mutation を新 owner の更新として受理しない境界を確立する。
- owner handover や stale update 排除の実装には connection lifetime、opaque cookie、generation counter 等を使用してよいが、特定方式や no-wrap counter を必須設計にしない。
- revoke は最初に新規 resolve を遮断する。既に Tuner packet path が取得済みの内部 material 参照はその処理終了まで保持してよいが、新規 packet 処理へ再取得させない。
- 最後の取得済み参照解放後は material を再利用可能な状態へ戻す前に、その表現に応じた秘密情報の破棄を行う。具体的な memory wipe/handle destruction の方式は実装詳細とする。

## 6. commit / resolve / revoke 不変条件

- ECM 前の session ID は unresolved でよい。不完全 context、必要 parity 欠落、stale owner、stale update、revoke 済み slot を成功へ丸めない。
- ECM 更新では new material を prepare し、system key/CBC 初期値/odd/even Ks を含む complete context を検証してから stable slot の current material を一括 commit する。
- `processEcm()` 成功の linearization point は、新 material が registry へ commit 済みで、同じ session ID bytes の stable link から直ちに取得可能になった時点とする。commit 前失敗では旧 material を維持する。
- 同一 session の mutating CAS operation を直列化するか、同等の stale-completion 排除を行い、古い ECM 結果が後から current material を上書きしないようにする。方式は実装詳細とする。
- session close、CAS release、CAS owner loss、credential revoke、backend fatal failure、registry corruption では新規 resolve/resource 取得を revoke する。
- stale token を別 owner/session の resource へ再利用しない。
- resolve failure、incomplete context、stale owner/update、revoke 済み token を復号成功に丸めず、Tuner HAL の bad-token / unavailable-key / registry-failure 診断へ接続する。

## 7. TIS / Tuner teardown

MediaCas 由来 token を Tuner descrambler で使用した場合は、MediaCas session を close する前にその token を descrambler から解除する。

```text
1. 当該 key context の通常配送を停止し、可能な PID link を解除する。
2. session ID bytes を保持する全 descrambler へ setKeyToken(VOID) を行う。
3. 各 VOID 設定成功を、その descrambler が以後 token を新規 packet 処理へ使用しない linearization point とする。
4. 必要な全 descrambler で step 3 成立後に MediaCas session を close する。
```

PID unlink 失敗は診断へ残すが、token 解除成功後に session close を不必要に保持しない。逆に VOID 設定失敗の descrambler が残る間は、その token を使う MediaCas session を close 済み成功として扱わない。

CAS 側 revoke 後は新規 resolve/resource 取得を拒否し、競合して既に resource を取得済みの packet 処理だけを内部参照寿命で完了または破棄させる。最後の参照解放後の秘密 material 破棄方式は resource 表現に応じた実装詳細とする。追加の公開同期 API は設けない。

## 8. Tuner HAL の責務

Tuner HAL は stable slot から取得した MULTI2 material を使い、TS packet payload の MULTI2 復号と scrambling-control に基づく odd/even Ks 選択だけを行う。ECM/EMM、card I/O、Yakisoba IPC、権利判定、credential 取得を Tuner HAL へ移さない。

AOSP/VTS は key token を opaque linkage として扱い、B25 内部 material layout を規定しない。本契約は ARIB STD-B25 の MULTI2/Ks の意味を満たしながら、AOSP 公開境界へ raw material や CAS 方式固有 identity を露出しないための vendor 内部契約である。
