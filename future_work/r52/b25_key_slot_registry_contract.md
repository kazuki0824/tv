# r52 B25 KeySlotRegistry 内部鍵資源契約

本書は r52 の B25 CAS HAL と Tuner HAL descrambler の間で共有する **内部鍵資源** の単一正本である。AOSP の `MediaCas.Session` / Tuner `IDescrambler.setKeyToken()` に流す byte sequence は不透明な参照値のままとし、B25 鍵素材を Binder、TIS、通常 log へ公開しない。

## 1. AOSP 公開境界

- CAS HAL は標準 MediaCas session ID bytes と内部 `KeySlotRegistry` entry の対応を vendor bridge 内で成立させる。TIS 向けの別 token は生成しない。
- TIS は MediaCas session ID bytes を Tuner key token として `IDescrambler.setKeyToken()` へ渡すだけであり、鍵素材を解析・再構成しない。
- Tuner HAL は token を vendor shared registry で解決し、解決済み内部 resource だけを packet descramble path に渡す。
- token 自体へ system key、CBC 初期値、odd/even Ks、caSystemId、owner identity、鍵更新番号を埋め込まない。
- AOSP AIDL / VINTF に vendor 独自 field を追加しない。
- MediaCas session ID は ECM 成功前でも公開され得る。その時点では identity 予約だけが成立していてよく、完全な鍵 context が publish されるまで resolve 不可とする。

## 2. B25 internal key resource

B25 用 `KeySlotRegistry` entry が Tuner HAL に解決する最小の論理 resource は次で足りる。

```text
Multi2KeyMaterial
  system_key
  cbc_initial_value
  even_ks
  odd_ks
```

caSystemId、MediaCas session identity、SmartCard/Yakisoba 種別、CAS owner 世代、鍵更新番号は CAS-domain / registry-domain の管理情報であり、`Multi2KeyMaterial` の必須 field にしない。stale owner/update の識別に generation、cookie、connection identity、version counter 等を内部実装として使ってよいが、AOSP token または Tuner 側 resource の必須形式にはしない。

`IDescrambler.setKeyToken(token)` は token をその時点の key bytes snapshot へ固定する操作ではなく、stable な registry slot identity へ link する操作とする。同じ MediaCas session の後続 ECM では同じ slot の current material を atomic に更新し、既 link descrambler の新規 packet 処理から更新後 material を取得可能にする。既に material 参照を取得済みの packet はその参照で完了してよいが、1 packet 中に旧/new material の field を混在させない。数値 `key_epoch` や特定 lock-free 構造は必須にしない。

## 3. 所有権と供給経路

- **system key / CBC 初期値**: 当該 B25 plugin に bind された CAS backend が所有する。B25 実カードでは検証済み card 初期化応答から取得し、このためだけの外部 secure store / factory provisioning を必須にしない。Yakisoba 等で外部 credential が必要な場合だけ供給元と access control を product 側で固定する。
- **odd/even Ks**: CAS session が ECM / backend processing の結果として所有し、ECM 成功時に同じ stable slot の current material を更新する。
- product TIS は同一 `caSystemId` について1個の live MediaCas/CAS plugin を共有し、B25 ECM session と EMM を同じ plugin へ帰属させる。1つの B25 plugin instance では backend 種別を一度だけ選択し、release まで全 session / EMM で共有する。別 client の独立 plugin instance まで HAL service-global selector で横断調停しない。
- 複数 plugin instance が同じ physical card / Yakisoba resource を共有する場合は、その共有 resource 自身が必要な I/O/state ordering を提供する。
- raw key material を CAS から Tuner 側 registry へ運ぶ実装では、current authorized CAS owner だけが publish/revoke mutation できる vendor 内部境界を使用する。SELinux、socket ownership、peer credential、Binder identity 等の具体方式は採用構成に応じて選び、不要な重複機構を必須化しない。
- raw-key の一時表現は必要期間を越えて保持・永続化しない。mutable buffer の消去、secure handle の destroy/release 等は表現に応じて選び、特定 zeroize API を必須化しない。

## 4. commit / revoke 不変条件

- ECM 前の MediaCas session ID は unresolved でよい。`processEcm()` 成功の確定点は、system key、CBC 初期値、odd/even Ks を含む complete current material が registry へ commit 済みで、同じ session ID bytes の stable link から直ちに取得可能になった時点とする。commit 前失敗では旧 material を維持する。
- ECM による material 更新は一括 commit し、packet path が旧/new material の field を混在観測しないようにする。古い ECM completion が後から current material を上書きしないよう、同一 session の mutation を直列化するか同等の stale-completion 排除を行う。
- session close、CAS release、CAS owner loss、credential revoke、backend fatal failure、registry corruption では該当 entry の新規 resolve を拒否する。
- token は revoke、必要な descrambler の VOID unlink、既取得内部参照 drain が完了するまで別 session へ再割当てしない。process lifetime 全体で永久に再利用しない方式を選んでもよいが必須ではない。
- owner handover 後の旧接続・旧 request・revoke 済み token への publish を current state として受理しない。owner identity / stale update 排除の具体表現は実装詳細とする。
- registry resolve failure、incomplete context、stale owner/update、revoke 済み token は復号成功に丸めず、Tuner HAL の既存 bad-token / unavailable-key / registry-failure 診断へ接続する。

## 5. Tuner HAL 側の使用範囲

Tuner HAL は解決済み `Multi2KeyMaterial` を使って、TS packet の payload 部分に対する MULTI2 復号と scrambling-control に基づく odd/even Ks 選択だけを行う。ECM / EMM、カード I/O、Yakisoba integration、権利判定、credential provisioning、system key / CBC 初期値の取得を Tuner HAL 側へ移さない。

MediaCas 由来 token を Tuner descrambler で使用した場合は、MediaCas session を close する前に、その token を保持する全 descrambler で `setKeyToken(VOID)` を成功させる。VOID 成功後はその descrambler の新規 packet 処理で token を使用しない。CAS 側 revoke 後は新規 material 取得を拒否し、競合して既に取得済みの内部参照だけを処理終了まで保持する。最後の参照解放後の秘密 material 破棄方法は resource 表現に応じた実装詳細とする。

この object layout 自体を AOSP や ARIB が要求しているとは主張しない。AOSP/VTS は key token を opaque な key-slot linkage として扱い、B25 内部 material layout を規定しない。ARIB STD-B25 が要求する MULTI2 / ECM / EMM / Ks 等の意味を満たしつつ、AOSP 公開境界へ raw material や CAS 方式固有 identity を露出しないための本製品内部契約として本構成を固定する。