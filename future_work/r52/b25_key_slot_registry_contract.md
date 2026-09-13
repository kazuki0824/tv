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

この object layout 自体を AOSP や ARIB が要求しているとは主張しない。必要なのは、opaque token から完全な MULTI2 context を一意に解決でき、旧世代・失効済み・不完全な context を成功扱いしないことである。

## 3. 所有権と供給経路

- **system key / CBC 初期値**: plugin generation にbindされた CAS backend が所有する。B25 実カード経路では、検証済みのカード初期化応答から取得し、このためだけの外部 secure store や factory provisioning を必須にしない。カード初期化応答の形式・長さ・status を検証した後だけ同じ backend / session の credential context として採用する。
- Yakisoba 等、外部 credential が必要な backend を採用する場合だけ、その供給元とアクセス制御を product 統合側で固定する。system key / CBC 初期値を Binder、TIS、Tuner HAL、一般 property、公開 API、任意設定ファイルから取得する経路は設けない。
- **odd/even Ks**: CAS session が ECM / backend processing の結果として所有し、ECM 成功時に該当 session の current key epoch として更新する。
- `SmartCardCasPath` は card response から得た odd/even Ks を、同じ session の system key / CBC 初期値を持つ registry entry へ更新する。
- `YakisobaCasPath` の ECM response に含まれる key material は CAS/vendor 内部 registry へ session-relative material を渡すためだけに使用し、Binder/TIS/Tuner へ raw key として transport しない。
- 1つの `ICas` plugin generation で B25 backend をbindした後は plugin release まで切り替えず、異なる credential source の system key / CBC 初期値や別 session の Ks を混成しない。

## 4. token identity と寿命

- MediaCas session ID は 1..16 bytes の opaque value とし、Tuner の VOID key token と同一値を発行しない。
- 同一 CAS service process lifetime 内で session ID bytes を再利用しない。生成方法は公開契約にしないが、generation/nonce等を用いて no-reuse を保証し、token内容をTIS/Tunerに解釈させない。
- MediaCas session ID を公開する前に、service-global token namespace で live identity と衝突しないことを保証する。具体的な reservation API、table layout、wire protocolは固定しない。
- revoke 時はまず新規 resolve を遮断する。既に Tuner packet path が取得済みの内部 resource 参照はその処理終了まで保持してよいが、新規packet処理へ再取得させない。
- 最後の取得済み参照が解放された時点で raw key material を zeroize し、resource 本体を回収できる。session ID bytes 自体は service process lifetime 中に再利用しないため、resource回収後に巨大な秘密material tombstoneを保持する必要はない。

## 5. commit / resolve / revoke 不変条件

- ECM 前の session ID は registry 上で未解決状態であってよい。不完全 context、必要 parity の Ks 欠落、generation / epoch mismatch を復号成功へ丸めない。
- ECM により odd/even Ks を更新する場合、new epoch の material を準備し、system key / CBC 初期値を含む完全な context を検証してから registry entry を一括更新する。packet path が旧 epoch と新 epoch の field を混在観測してはならない。
- `processEcm()` が成功を返す linearization point は、新 epoch の完全な context が registry に commit 済みで、同じ MediaCas session ID bytes から直ちに resolve 可能になった時点とする。commit 前の失敗では旧 epoch を維持する。
- session close、CAS release、credential revoke、backend fatal failure、registry corruption では該当 entry を revoke し、以後の新規 resolve を拒否する。stale token を別 session / generation の resource へ再利用しない。
- registry resolve failure、incomplete context、generation / epoch mismatch、revoke 済み token は復号成功に丸めず、Tuner HAL の bad-token / unavailable-key / registry-failure 診断へ接続する。

## 6. TIS / Tuner teardown 契約

AOSP Tuner API が要求する順序に従い、MediaCas session ID bytes を Tuner descrambler の key token として使用した場合は、MediaCas session を close する前にその token を descrambler から解除する。

```text
1. 当該 key context の通常配送を停止し、可能な PID link を解除する。
2. その MediaCas session ID bytes を保持する全 descrambler に対し
   IDescrambler.setKeyToken(VOID key token) を呼ぶ。
3. 各 setKeyToken(VOID) の成功を「その descrambler が以後その token を
   新規packet処理へ使用しない」linearization pointとする。
4. 必要な全 descrambler で step 3 が成立した後に MediaCas session を close する。
```

PID unlink の失敗は診断へ残すが、token解除に成功していれば MediaCas session close を不必要に保持しない。逆に `setKeyToken(VOID)` が失敗した descrambler が残る間は、その token を使用した MediaCas session をclose済み成功として扱わない。

CAS 側 close/revoke 後は新規 resolve を拒否する。close と競合して既に resource を取得済みの packet処理は、参照count等の内部寿命管理で完了または破棄させ、最後の参照解放後にzeroizeする。追加の公開同期APIは設けない。

## 7. Tuner HAL 側の使用範囲

Tuner HAL は解決済み `B25DescrambleContext` を使って、TS packet の payload 部分に対する MULTI2 復号と scrambling-control に基づく odd/even Ks 選択だけを行う。ECM / EMM、カード I/O、権利判定、credential provisioning、system key / CBC 初期値の取得を Tuner HAL 側へ移さない。

AOSP/VTS は key token を opaque な key-slot linkage として扱い、B25 内部 material layout を規定しない。ARIB STD-B25 が要求する MULTI2 / ECM / EMM / Ks 等の意味を満たしつつ、AOSP 公開境界へ raw material を露出しないための本製品内部契約として本構成を固定する。
