# r52 B25 KeySlotRegistry 内部鍵資源契約

本書は r52 の B25 CAS HAL と Tuner HAL descrambler の間で共有する内部鍵資源契約の正本である。AOSP の MediaCas session ID / Tuner `IDescrambler.setKeyToken()` に流す byte sequence はopaqueな参照値のままとし、B25鍵素材をAOSP Binder、TIS、通常logへ公開しない。

## 1. AOSP 公開境界

- MediaCas が公開した session ID bytes を、そのまま Tuner key token とする。TIS向けの別tokenを生成しない。
- TISはsession ID bytesを `IDescrambler.setKeyToken()` へ渡すだけで、内部を解析・再構成しない。
- Tuner HALはtokenをvendor内部registryで解決し、解決済みMULTI2 resourceだけをpacket descramble pathへ渡す。
- tokenへsystem key、CBC初期値、odd/even Ks、CA system ID、generation、epochを埋め込むことを公開契約にしない。
- AOSP AIDL / VINTFへvendor独自fieldを追加しない。
- session IDはECM成功前でも公開され得る。その時点ではidentity予約だけが成立していてよく、完全な鍵contextがpublishされるまでresolve不可とする。

## 2. Tuner側の最小鍵resource

Tuner側がtoken解決後に必要とする論理resourceは次で足りる。

```text
Multi2KeyResource
  provider_incarnation   # opaque stale-owner fence
  key_epoch              # opaque monotonic version
  system_key
  cbc_initial_value
  even_ks
  odd_ks
  validity / revoke state
```

CA system ID、MediaCas session generation、SmartCard/Yakisoba種別はCAS-domain情報であり、Tuner側resourceの必須fieldにしない。CAS adapterはそれらをCAS側で検証したうえでgeneric MULTI2 resourceへ変換する。Tuner HALはprovider identityの意味やB25/B1を解釈して分岐しない。

`provider_incarnation` と `key_epoch` は比較・stale判定のための内部metadataであり、AOSP tokenへ公開しない。実装はsecure-memory object、key-ladder slot、opaque handle等へ置換できるが、token resolve後に同じprovider incarnation / epochの完全なMULTI2 contextを一意に取得できなければならない。

## 3. stable link と key rotation

`IDescrambler.setKeyToken(token)` はtokenをその時点のkey bytesのsnapshotへ固定する操作ではなく、stableなregistry slot identityへlinkする操作とする。

- 初回ECM成功後にtokenをdescramblerへ設定した後、同じMediaCas sessionの後続ECMでkey epochが更新されても、TISが同じtokenを設定し直す必要はない。
- registryは同じstable slot identityのcurrent resourceをatomicに差し替える。既link descramblerの新規packet処理は更新後resourceを参照する。
- epoch切替と競合して既にresource参照を取得済みのpacketは、その取得済みepochで完了してよい。1 packet処理中に旧/new epochのfieldを混在させない。
- revoke/provider death後はstable linkが残っていても新規resource取得を拒否する。既取得参照だけをdrainする。
- tokenを別slot identityへ付け替えない。slot/resource本体を回収しても同一tokenを別sessionへ再利用しない。

このindirectionは継続的ECM key rotationを既存AOSP `setKeyToken()` linkageだけで成立させるための内部契約であり、AOSPへ更新APIを追加するものではない。

## 4. 所有権と供給経路

- system key / CBC初期値はplugin generationにbindされたCAS backendが所有する。B25実カードでは検証済みcard初期化応答から取得し、このためだけの外部secure store / factory provisioningを必須にしない。
- Yakisoba等で外部credentialが必要な場合だけ、その供給元とaccess controlをproduct側で固定する。一般property、公開API、TIS、Tuner HAL、world-readable fileをcredential sourceにしない。
- odd/even KsはCAS sessionがECM/backend processingの結果として所有し、ECM成功時に新key epochとして更新する。
- raw key materialをCASからTuner側registryへ運ぶ必要がある実装では、認証済みvendor内部境界だけを使用する。TISや一般appがpublish/revoke mutation endpointへ到達できてはならない。
- registry mutationは認証済みCAS provider incarnationからだけ受理し、peer/owner不一致、旧incarnation、revoke済みtokenへのpublishを拒否する。
- transport/encode/decode用のraw-key一時bufferは必要最短寿命にし、commitまたは失敗後にzeroizeする。
- 1つのB25 plugin generationでbackendをbindした後はreleaseまで切り替えず、異なるcredential sourceや別sessionのKsを混成しない。

## 5. token identity と寿命

- MediaCas session IDは1..16 bytesのopaque valueとし、TunerのVOID key tokenと同一値を発行しない。
- 同一CAS service process lifetime内でsession ID bytesを再利用しない。生成方式は公開契約にしない。
- session ID公開前にservice-global token namespaceでlive/retired identityと衝突しないことを保証する。reservation APIやtable layoutは実装詳細とする。
- provider incarnationをwrap/reuseしない。次incarnationを安全に発行できない場合は新providerを有効化しない。
- key epochを同一live token内でwrap/reuseしない。次epochを発行できない場合はECM成功にせず、そのsessionを新規key publish不能として閉じる。
- CAS provider/serviceのdeath/disconnectを検出した場合、そのincarnationに属するentryの新規resolveを一括遮断する。
- revokeは最初に新規resolveを遮断する。既にTuner packet pathが取得済みの内部resource参照はその処理終了まで保持してよいが、新規packet処理へ再取得させない。
- 最後の取得済み参照解放後にraw key materialをzeroizeしresource本体を回収する。旧entryが残る間は同じtoken candidateの新規reservationを拒否する。

## 6. commit / resolve / revoke 不変条件

- ECM前のsession IDはunresolvedでよい。不完全context、必要parity欠落、provider incarnation mismatch、epoch mismatchを成功へ丸めない。
- ECM更新ではnew epoch materialをprepareし、system key/CBC初期値/odd/even Ksを含む完全contextを検証してからstable slotのcurrent resourceを一括commitする。
- `processEcm()` 成功のlinearization pointは、新epochがregistryへcommit済みで、同じsession ID bytesのstable linkから直ちに新epochを取得可能になった時点とする。commit前失敗では旧epochを維持する。
- session close、CAS release、provider death、credential revoke、backend fatal failure、registry corruptionでは新規resolve/resource取得をrevokeする。
- stale tokenを別provider/sessionのresourceへ再利用しない。
- resolve failure、incomplete context、incarnation/epoch mismatch、revoke済みtokenを復号成功に丸めず、Tuner HALのbad-token / unavailable-key / registry-failure診断へ接続する。

## 7. TIS / Tuner teardown

MediaCas由来tokenをTuner descramblerで使用した場合は、MediaCas sessionをcloseする前にそのtokenをdescramblerから解除する。

```text
1. 当該key contextの通常配送を停止し、可能なPID linkを解除する。
2. session ID bytesを保持する全descramblerへ setKeyToken(VOID) を行う。
3. 各VOID設定成功を、そのdescramblerが以後tokenを新規packet処理へ使用しないlinearization pointとする。
4. 必要な全descramblerでstep 3成立後にMediaCas sessionをcloseする。
```

PID unlink失敗は診断へ残すが、token解除成功後にsession closeを不必要に保持しない。逆にVOID設定失敗のdescramblerが残る間は、そのtokenを使うMediaCas sessionをclose済み成功として扱わない。

CAS側revoke後は新規resolve/resource取得を拒否し、競合して既にresourceを取得済みのpacket処理だけを内部参照寿命で完了または破棄させ、最後の参照解放後にzeroizeする。追加の公開同期APIは設けない。

## 8. Tuner HAL の責務

Tuner HALはstable slotから取得したMULTI2 resourceを使い、TS packet payloadのMULTI2復号とscrambling-controlに基づくodd/even Ks選択だけを行う。ECM/EMM、card I/O、Yakisoba IPC、権利判定、credential取得をTuner HALへ移さない。

AOSP/VTSはkey tokenをopaque linkageとして扱い、B25内部material layoutを規定しない。本契約はARIB STD-B25のMULTI2/Ksの意味を満たしながら、AOSP公開境界へraw materialやCAS方式固有identityを露出しないためのvendor内部契約である。
