# r52 B25 KeySlotRegistry 内部鍵資源契約

本書は r52 の B25 CAS HAL と Tuner HAL descrambler の間で共有する **内部鍵資源** の単一正本である。AOSP の `MediaCas.Session` / Tuner `IDescrambler.setKeyToken()` に流す byte sequence は不透明な参照値のままとし、B25 鍵素材を Binder、TIS、logcat へ公開しない。

## 1. AOSP 公開境界

- CAS HAL が `openSession()` / `openSessionDefault()` で公開する MediaCas session ID bytes を、そのまま Tuner key token とする。TIS 向けに別形式の token を生成しない。
- TIS は MediaCas session ID bytes を `IDescrambler.setKeyToken()` へ渡すだけであり、token 内部を解析・再構成しない。
- Tuner HAL は token を vendor shared registry で解決し、解決済み内部 resource だけを packet descramble path に渡す。
- token 自体へ system key、CBC 初期値、odd/even Ks を埋め込まない。caSystemId、session generation、key epoch 等を token の公開形式から推定して registry 検証を省略しない。
- AOSP AIDL / VINTF に vendor 独自 field を追加しない。
- MediaCas session ID は ECM 成功前でも公開され得る。その時点では registry 上の identity 予約だけが成立していてよいが、完全な鍵 context が publish されるまで復号可能 token として resolve してはならない。

## 2. B25 internal key resource

B25 用 `KeySlotRegistry` entry が Tuner HAL に解決する resource は、次の論理内容を一体として持つ。

```text
B25DescrambleContext
  provider_incarnation
  ca_system_id
  cas_session_generation
  key_epoch
  system_key
  cbc_initial_value
  even_ks
  odd_ks
  validity / revoke state
```

`provider_incarnation` はCAS provider/service instanceの内部incarnation fenceであり、AOSP tokenへ公開しない。`system_key`、`cbc_initial_value`、`even_ks`、`odd_ks` は raw byte materialであり、上記resourceの外へ公開しない。secure-memory object、handle、key-ladder slot等へ置換してもよいが、Tuner HALがtoken解決後に同じprovider/session/epochの完全なMULTI2 contextを一意に取得できなければならない。

このobject layout自体をAOSPやARIBが要求しているとは主張しない。必要なのは、opaque tokenから完全なMULTI2 contextを一意に解決でき、旧incarnation・旧世代・失効済み・不完全なcontextを成功扱いしないことである。

## 3. 所有権と供給経路

- **system key / CBC 初期値**: plugin generation にbindされた CAS backend が所有する。B25実カード経路では、検証済みカード初期化応答から取得し、このためだけの外部secure storeやfactory provisioningを必須にしない。応答の形式・長さ・statusを検証した後だけ同じbackend/sessionのcredential contextとして採用する。
- Yakisoba等、外部credentialが必要なbackendを採用する場合だけ、その供給元とアクセス制御をproduct統合側で固定する。system key / CBC初期値をBinder、TIS、Tuner HAL、一般property、公開API、任意設定ファイルから取得する経路は設けない。
- **odd/even Ks**: CAS sessionがECM/backend processingの結果として所有し、ECM成功時に該当sessionのcurrent key epochとして更新する。
- `SmartCardCasPath` はcard responseから得たodd/even Ksを、同じsessionのsystem key/CBC初期値を持つregistry entryへ更新する。
- `YakisobaCasPath` のECM responseに含まれるkey materialはCAS/vendor内部registryへsession-relative materialを渡すためだけに使用し、Binder/TIS/Tunerへraw keyとしてtransportしない。
- 1つの `ICas` plugin generationでB25 backendをbindした後はplugin releaseまで切り替えず、異なるcredential sourceのsystem key/CBC初期値や別sessionのKsを混成しない。

## 4. token identity と寿命

- MediaCas session ID は1..16 bytesのopaque valueとし、TunerのVOID key tokenと同一値を発行しない。
- 同一CAS service process lifetime内でsession ID bytesを再利用しない。生成方法は公開契約にしないが、generation/nonce等を用いてno-reuseを保証し、token内容をTIS/Tunerに解釈させない。
- MediaCas session IDを公開する前に、service-global token namespaceでlive identityと衝突しないことを保証する。具体的なreservation API、table layout、wire protocolは固定しない。
- registryはCAS provider incarnationを内部的に識別し、新provider incarnationを旧incarnationと同一ownerとして扱わない。CAS provider/serviceのdeath/disconnectを検出した場合、そのincarnationに属するentryの新規resolveを一括遮断する。
- revoke時はまず新規resolveを遮断する。既にTuner packet pathが取得済みの内部resource参照はその処理終了まで保持してよいが、新規packet処理へ再取得させない。
- 最後の取得済み参照が解放された時点でraw key materialをzeroizeし、resource本体を回収できる。旧incarnationのentryが残る間は同じtoken candidateの新規reservationを衝突として拒否する。

## 5. commit / resolve / revoke 不変条件

- ECM前のsession IDはregistry上で未解決状態であってよい。不完全context、必要parityのKs欠落、provider/session generation mismatch、key epoch mismatchを復号成功へ丸めない。
- ECMによりodd/even Ksを更新する場合、new epoch materialをprepareし、system key/CBC初期値を含む完全なcontextを検証してからregistry entryを一括更新する。packet pathが旧epochと新epochのfieldを混在観測してはならない。
- `processEcm()` が成功を返すlinearization pointは、新epochの完全なcontextがregistryにcommit済みで、同じMediaCas session ID bytesから直ちにresolve可能になった時点とする。commit前の失敗では旧epochを維持する。
- session close、CAS release、provider death、credential revoke、backend fatal failure、registry corruptionでは該当entryをrevokeし、以後の新規resolveを拒否する。stale tokenを別provider/session/generationのresourceへ再利用しない。
- registry resolve failure、incomplete context、provider/session generation mismatch、epoch mismatch、revoke済みtokenは復号成功に丸めず、Tuner HALのbad-token / unavailable-key / registry-failure診断へ接続する。

## 6. TIS / Tuner teardown 契約

AOSP Tuner APIが要求する順序に従い、MediaCas session ID bytesをTuner descramblerのkey tokenとして使用した場合は、MediaCas sessionをcloseする前にそのtokenをdescramblerから解除する。

```text
1. 当該 key context の通常配送を停止し、可能な PID link を解除する。
2. その MediaCas session ID bytes を保持する全 descrambler に対し
   IDescrambler.setKeyToken(VOID key token) を呼ぶ。
3. 各 setKeyToken(VOID) の成功を「その descrambler が以後その token を
   新規packet処理へ使用しない」linearization pointとする。
4. 必要な全 descrambler で step 3 が成立した後に MediaCas session を close する。
```

PID unlink失敗は診断へ残すが、token解除に成功していればMediaCas session closeを不必要に保持しない。逆に `setKeyToken(VOID)` が失敗したdescramblerが残る間は、そのtokenを使用したMediaCas sessionをclose済み成功として扱わない。

CAS側close/revoke後は新規resolveを拒否する。closeと競合して既にresourceを取得済みのpacket処理は内部参照寿命管理で完了または破棄させ、最後の参照解放後にzeroizeする。追加の公開同期APIは設けない。

## 7. Tuner HAL 側の使用範囲

Tuner HALは解決済み `B25DescrambleContext` を使って、TS packetのpayload部分に対するMULTI2復号とscrambling-controlに基づくodd/even Ks選択だけを行う。ECM/EMM、カードI/O、権利判定、credential provisioning、system key/CBC初期値の取得をTuner HAL側へ移さない。

AOSP/VTSはkey tokenをopaqueなkey-slot linkageとして扱い、B25内部material layoutを規定しない。ARIB STD-B25が要求するMULTI2/ECM/EMM/Ks等の意味を満たしつつ、AOSP公開境界へraw materialを露出しないための本製品内部契約として本構成を固定する。
