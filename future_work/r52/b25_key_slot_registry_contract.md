# r52 B25 KeySlotRegistry 内部鍵資源契約

本書は r52 の B25 CAS HAL と Tuner HAL descrambler の間で共有する **内部鍵資源** の単一正本である。AOSP の `MediaCas.Session` / Tuner `IDescrambler.setKeyToken()` に流す byte sequence は不透明な参照値のままとし、B25 鍵素材を Binder、TIS、logcat へ公開しない。

## 1. AOSP 公開境界

- CAS HAL は標準 MediaCas session ID bytes と内部 `KeySlotRegistry` entry の対応を vendor bridge 内で成立させる。
- TIS は MediaCas session ID bytes を Tuner key token として `IDescrambler.setKeyToken()` へ渡すだけであり、鍵素材を解析・再構成しない。
- Tuner HAL は token を vendor shared registry で解決し、解決済み内部 resource だけを packet descramble path に渡す。
- token 自体へ system key、CBC 初期値、odd/even Ks を埋め込まない。
- AOSP AIDL / VINTF に vendor 独自 field を追加しない。

## 2. B25 internal key resource

B25 用 `KeySlotRegistry` entry が Tuner HAL に解決する resource は、少なくとも次の論理内容を一体として持つ。

```text
B25DescrambleContext
  ca_system_id
  cas_session_generation
  key_epoch
  system_key
  cbc_initial_value
  even_ks
  odd_ks
  validity / revoke state
```

`system_key`、`cbc_initial_value`、`even_ks`、`odd_ks` は raw byte material であり、上記 resource の外へ公開しない。実装上の secure-memory object、handle、key-ladder slot 等へ置換してもよいが、Tuner HAL が token 解決後に同じ session / generation / epoch に属する完全な MULTI2 descramble context を一意に取得できなければならない。

## 3. 所有権と供給経路

- **system key / CBC 初期値**: CAS/vendor secure bridge が所有し、製品で採用する B25 credential provisioning 経路から取得する。ECM の都度生成される session key として扱わない。TIS または Tuner HAL が設定ファイル、property、公開 API から独自に読み込まない。
- **odd/even Ks**: CAS session が ECM / card processing の結果として所有し、ECM 成功時に該当 session の current key epoch として更新する。
- `SmartCardCasPath` は card response から得た odd/even Ks を、同じ session の system key / CBC 初期値を持つ registry entry へ更新する。
- `YakisobaCasPath` の `DecodeEcmResponse.key_material_for_local_registry` は CAS HAL 内部 registry へ odd/even Ks 相当の session-relative material を渡すためだけに使用する。system key / CBC 初期値を Binder/TIS/Tuner へ transport する経路にはしない。
- CAS path が切り替わるのは session open 時だけであり、1 session の途中で異なる provisioning source の system key / CBC 初期値や別 session の Ks を混成しない。

## 4. commit / revoke 不変条件

- token を公開可能にするのは、当該 session / generation の `B25DescrambleContext` が完全に解決可能になった後だけとする。system key、CBC 初期値、必要な parity の Ks のいずれかが欠ける entry を「復号可能 token」として公開しない。
- ECM により odd/even Ks を更新する場合、new epoch の material を準備してから registry entry を一括更新し、packet path が旧 epoch と新 epoch の field を混在観測しないようにする。
- session close、CAS release、credential revoke、registry corruption では該当 entry を revoke し、以後の新規 resolve を拒否する。stale token を別 session / generation の resource へ再利用しない。
- registry resolve failure、incomplete context、generation / epoch mismatch は復号成功に丸めず、Tuner HAL の既存 bad-token / unavailable-key / registry-failure 診断へ接続する。

## 5. Tuner HAL 側の使用範囲

Tuner HAL は解決済み `B25DescrambleContext` を使って、TS packet の payload 部分に対する MULTI2 復号と scrambling-control に基づく odd/even Ks 選択だけを行う。ECM / EMM、カード I/O、権利判定、credential provisioning、system key / CBC 初期値の取得を Tuner HAL 側へ移さない。

この object layout 自体を AOSP や ARIB が要求しているとは主張しない。AOSP/VTS は key token を opaque な key-slot linkage として扱い、B25 内部 material layout を規定しない。ARIB STD-B25 が要求する MULTI2 / ECM / EMM / Ks 等の意味を満たしつつ、AOSP 公開境界へ raw material を露出しないための本製品内部契約として本構成を固定する。
