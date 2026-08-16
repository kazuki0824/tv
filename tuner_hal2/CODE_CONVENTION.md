# Tuner HAL2 コーディング規則

この文書は `tuner_hal2` 固有の**実装規約**だけを定める。公開契約、状態遷移、戻り値、capability / profile、資源寿命、transaction の phase / commit / rollback、cleanup / quarantine の意味、worker / callback / source boundary / descrambler の論理契約は `../tuner_hal/DESIGN_JA.md` を唯一の正本とする。実装 owner、module anchor、許可 entry point は `DESIGN_JA.md` の「共通transaction / use-caseの規範実装アンカー」を唯一の正本とする。

本書は論理状態名、commit point、rollback policy、quarantine 条件を第二の正本として定義しない。以下で論理契約名を記す場合も、その意味を再定義するのではなく、**実装が正本 owner / typed entry を迂回しないための禁止規約**を示すだけである。

## 1. failure / rollback / cleanup の実装規約

- cleanup、rollback、stop、join、callback、unregister、close の失敗を `let _ =`、空分岐、ログだけ、`drop(result)` で捨てない。
- primary failure 発生後の cleanup / rollback を `?` だけで呼び、cleanup failure で primary failure を無診断で上書きしない。
- primary + cleanup failure を文字列 detail だけの generic internal error に潰さない。戻り値として片方の status を選ぶ場合でも、もう片方を typed composed failure または必須診断から消さない。
- `FirstErrorCollector` は同一 cleanup phase 内の first cleanup error 集約だけに使用し、primary + cleanup failure composition や per-step outcome 保存の代替にしない。
- cleanup 系 top-level use-case は、全対象を試行した per-step outcome を bounded diagnostic store へ保存してから public failure を射影する。途中の `?` で後続 cleanup を飛ばさない。
- object cleanup と frontend worker cleanup の domain-specific context を `Option` field bag や `String` detail へ丸めず、variant-specific typed context を保持する。
- rollback / public close / owner-loss cleanup は、失敗を戻り値または必須診断へ接続できる typed operation を使う。void / best-effort-only helper を必須 cleanup の正本入口にしない。
- best-effort telemetry は primary failure を上書きしない。必須診断 store は bounded とし、records と dropped / record-failure counter を同じ snapshot で観測可能にする。
- bounded diagnostic store の reset は records と dropped counter を同時に初期化する。reset failure を silent success にしない。
- `Drop` に object 種別固有の cleanup state machine を書かない。`ObjectCloseTxn` owner が公開する owner-loss / Drop 用 typed entry だけへ接続する。

## 2. AIDL / service_runtime 境界

- AIDL method body で lifecycle check、request planning、runtime lock、domain mutation を手組みしない。object_runtime façade または service_runtime の typed use-case を通す。
- AIDL method body で fallible request 変換、callback retain、source relation validation、unsupported / unavailable mapping を、呼出対象 object の live / generation / kind 確認より先に実行しない。
- child object open では、service_runtime が typed runtime id と `RuntimeObjectEntry` を同一 result で返す。AIDL helper が ledger id を filter / DVR id へ再変換したり、rollback command / unhealthy marking / failure composition を再実装したりしない。
- AIDL 層から `RuntimeObjectTable`、runtime registry、transaction owner の private module / mutable registry を直接参照しない。
- `Status::new_service_specific_error()` は `aidl_service::error_bridge` 以外で直接呼ばない。Binder status mapping helper を object wrapper や runtime helper へ再定義しない。
- AIDL helper は Binder status 変換と method identity adapter に留め、object lifetime、request-builder critical section、domain state commit を所有しない。
- supported public API planning には `PublicApi`、unsupported-by-design の戻り値生成には `UnsupportedPublicApi` を使う。query / open / 状態取得系を unsupported planning に流用しない。
- public close は `ObjectCloseTxn` の typed entry にだけ接続する。AIDL method body が lifecycle phase、domain cleanup command、cleanup-failed marking、descendant 判定を直接変更しない。
- close finalization で複数 public runtime entry を unregister する場合は、destructive unregister 前に対象 entry を全件 preflight する。
- callback artifact cleanup helper へ raw `SharedTunerRuntime` を渡さない。artifact store mutation は runtime owner が発行した typed command を受ける bridge に限定する。
- production file-split module で `use super::*;` を使わず、必要項目を明示 import する。
- production source で `#[path]` / `include!` / `include_str!` を使わない。

## 3. transaction owner / typed entry を迂回しない

状態・寿命・failure transition の論理契約は `../tuner_hal/DESIGN_JA.md`、実装 owner / anchor / allowed entry は `DESIGN_JA.md` の「共通transaction / use-caseの規範実装アンカー」を正とする。本書はこれらの owner 表、phase order、commit / rollback / quarantine 条件を複製しない。

- 規範実装アンカーに owner として列挙されていない module は、当該 state / registry / generation / cleanup authority を直接変更しない。
- service-level orchestration は、正本 owner から受け取った typed request / token / command / result を接続するだけとし、domain-private state を直接書き戻さない。
- domain transaction は、自身が owner として指定された state だけを変更する。public object table、Binder artifact、別 domain の state を直接変更しない。
- façade / adapter / query は状態変更、commit、rollback、cleanup authority を持たない。
- 同じ state field、registry entry、lease、generation、cleanup authority を複数 module が mutation owner として扱わない。
- 上位 transaction から下位 transaction へ raw mutable registry、snapshot 本体、任意 restore closure を渡さない。
- object close / owner-loss cleanup の runtime unregister や domain cleanup を `FnOnce` closure として AIDL 側から注入しない。runtime owner が typed command を発行し、AIDL executor は command execution bridge に限定する。
- `TunerServiceRuntime::registry_mut()` のような raw mutation entry は owning domain transaction implementation だけから呼ぶ。service-level orchestration は専用 API / typed command を使う。
- `RuntimeQuery<'a>` は read-only query 専用とし、mutable reference、transaction context、mutation closure を持たせない。
- `transaction_registry.rs` は dispatch target の実装表に限定し、ownership、coverage、接続済み判定、公開 status semantics を第二の表として持たせない。
- 静的検査は directory 名や `*_ops.rs` / `*_txn.rs` suffix だけで owner を判定せず、規範アンカーにない module が owner state を直接 mutation していないことと typed entry を迂回していないことを検査する。

## 4. wrapper 作成基準

Wrapper を置いてよいのは、public API 境界、domain naming 隠蔽、AIDL/service_runtime 型境界、object-handle based use-case 境界、callback artifact bridge 境界など、明確な境界を追加する場合に限る。

次の wrapper は置かない。

- 名前も責務も同じ単純委譲。
- context method と1対1で、公開境界・domain naming・型境界の意味が増えないもの。
- callback rollback、profile validation、close helperだけを包む public thin wrapper。
- production 未接続の bridge / slot / mapper / transaction skeleton を public re-export するだけのもの。
- test だけで使う型を production 共通部品として公開するもの。

論理 contract 名を変更した場合は `tuner_hal2/DESIGN_JA.md` の規範実装アンカーだけを更新し、本書に旧名→新名の独自対応表を残さない。

## 5. capability token / guard

- production mutation method は typed request / capability token / transaction proof / transaction-owned rollback token のいずれかで entry を固定し、standalone public token factory、public field / constructor、token なしの薄い `&mut self` mutation method、arbitrary closure executor を追加しない。
- Rust visibility は実装規約として本書で扱う。crate 間 DTO / typed request / read-only snapshot / AIDL DTO 変換 accessor は `pub` を許容するが、state mutation、rollback restore、queue export、registry/session mutation、transaction plan を外部 caller が組み立てられる public surface にしない。
- capability token は owner だけが発行する。外部 caller が public constructor / enum variant / field struct literal で偽造できる形にしない。
- crate 間 typed request は operation DTO として使用してよいが、request 単体で snapshot、one-shot token、queue export handle、registry entry、任意 restore authority を得られないことを条件とする。
- one-shot token に `Clone` / `Copy` を付けず、consume-by-value で消費する。rollback snapshot 本体を token 外へ出さない。
- 再利用可能な値は token と分離した read-only descriptor にする。
- single-variant enum や未使用 variant で状態機械を装わない。

## 6. worker / callback / source boundary

- frontend worker の blocking join を `TunerServiceRuntime` lock 保持中に実行しない。lock 内は cancel / join ticket 取得までとし、join は lock 外で行う。
- worker replacement は、旧 worker を停止する前に検証可能な request precondition、generation candidate、rollback-token preparation を済ませる。旧 worker 停止後に初めて fallible preflight を行わない。
- replacement complete / start rollback の失敗は typed diagnostic に stopped old generation と candidate generation を残す。
- callback registry の missing を rollback / public close / owner-loss cleanup で無言成功にしない。artifact store と runtime registry の結果を照合する。
- callback delivery façade は artifact lookup、event conversion、Binder delivery を区別し、typed failure を `WorkerFailureClassifier` / `PostCommitCallbackFailureTxn` の正本 entry へ渡す。delivery module が health / rollback semantics を再定義しない。
- callback artifact store、DVR notifier store、filter dispatcher、drop-leak diagnostic store を process-global `OnceLock` / `static Mutex` に置かず、service instance lifetime に閉じる。
- `IFilter.setDataSource()` の relation validation と mutation は `SourceBoundaryTxn` の typed entry に接続する。AIDL / runtime helper が cycle、rollback、quarantine semantics を別定義しない。
- stream boundary は `StreamBoundaryTxn` の typed entry に接続する。packet / parser / queue owner の steady-state を boundary helper が直接所有しない。

## 7. query / packet / diagnostic 境界

- query façade は registry entry、runtime state、signal state、mutable handle を AIDL 側へ返さず、snapshot DTO だけを返す。
- `ObjectMethodDispatchProof` 等の dispatch capability は owner module 内で即時消費し、AIDL closure や top-level façadeへ渡さない。
- validated typed id の raw 値 accessor を routing / validation / mutation に使わない。raw 変換は AIDL DTO 変換や low-level parser 直前など、必要な境界だけに限定する。
- packet-derived PID と設定由来 PID を同じ raw integer helper 引数で混用しない。
- packet-bearing ingress は `ValidatedTsPacket` 等の検証済み型を正本とし、raw byte wrapper は validation 境界だけに置く。
- packet descramble path が raw key-slot id、raw key table、registry entry を直接取得しない。registry owner が解決した snapshot / predicate を渡す。
- diagnostic record を kind + 多数の optional field から意味復元する field bag にしない。variant-specific typed context を使う。
- public `HalError` detail と typed diagnostic record を併用する場合、typed record を正本として保存し、文字列だけを唯一の診断情報にしない。
- 診断専用 counter は `../tuner_hal/DESIGN_JA.md` の診断 counter 飽和契約に従い、business API の成功/失敗判定や lifetime / generation 発行に使わない。

## 8. public nullable / close / frontend count の実装入口

- public nullable API、public close、frontend count の公開意味は `../tuner_hal/DESIGN_JA.md` の該当API契約を正とする。本書では状態名、戻り値、phase を再定義しない。
- nullable Binder 引数を helper 内で非 nullable に潰さず、`None` を正本 use-case へ到達させる。
- AIDL façade は close / callback unregister / demux-input PID claim / frontend count の意味論を再定義せず、service_runtime use-case と Binder status bridge に限定する。
- input conversion helper は未対応値の丸め込みや独自戻り値 policy を持たず、validated request / command DTO へ接続する。

## 9. 共通部品境界の禁止事項

- callback unregister / cleanup policy を AIDL façade に書かない。callback artifact retain は object-method preflight 後にだけ行う。
- raw descrambler session-key mutator や key table owner を descrambler crate 外へ公開しない。key clear / replace / session cleanup は該当 typed transaction entry だけに接続する。
- close cascade の対象列挙、ordering、failure composition、descendant 判定を AIDL façadeへ戻さない。`ObjectCloseTxn` owner が生成する typed cleanup command だけを実行する。
- `SourceBoundaryTxn` / `StreamBoundaryTxn` の内部 step recording method を共通部品外へ公開しない。
- packet diagnostic に raw `i32` PID を保持しない。
- descrambler close / owner-loss cleanup で raw key token を service_runtime 側へ読み出してから release しない。
- record index / section / PES / descramble path で、検証済み packet を再び raw byte 入口へ戻さない。
- cross-crate 制限を visibility だけで表現せず、crate / module graph と typed entry の両方で閉じる。

## 10. callback delivery failure boundary

- callback artifact lookup failure と Binder delivery failure の公開意味は `../tuner_hal/DESIGN_JA.md` を正とする。delivery module は phase を保持した typed primary error を service_runtime completion use-case へ渡すだけにする。
- runtime lock poison 等で finish use-case に到達できない場合は、service-context owned typed fallback diagnostic store へ記録し、記録不能も counter へ表面化する。
- production code から callback artifact store を raw owner handle または all-artifact helper で直接 clear しない。runtime owner が発行する command bridge に限定する。
- artifact bridge は runtime callback registry を mutation しない。registry mutation と primary + cleanup failure composition は service_runtime owner に置く。

## 11. 静的チェックの位置づけ

- 静的チェックは規約違反候補を検出する補助確認であり、build / unit test / atest / VTS / 実機確認の代替にしない。
- 静的チェックを追加する場合は検出対象を明示し、完了判定の主根拠にしない。
- テストは公開関数、戻り値、状態、診断を直接検査し、同じソースファイルの文字列検索で完了判定しない。
- owner / entry の静的検査は `tuner_hal2/DESIGN_JA.md` の規範実装アンカーを入力とし、本書独自の ownership 表を入力にしない。
