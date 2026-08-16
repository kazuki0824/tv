# Tuner HAL2 コーディング規則

この文書は `tuner_hal2` 固有の**実装規約**だけを定める。公開契約、状態遷移、戻り値、capability / profile、資源寿命、transaction の phase / commit / rollback、cleanup / quarantine の意味、worker / callback / source boundary / descrambler の論理契約は `../tuner_hal/DESIGN_JA.md` を唯一の正本とする。実装 owner、module anchor、許可 entry point は `DESIGN_JA.md` の「共通transaction / use-caseの規範実装アンカー」を唯一の正本とする。プロジェクト全体の Rust 規約、no-`panic`、mutex汚染、一般worker、FFI、parser、ロック / callback、テスト自己参照禁止は `../GLOBAL_CODE_CONVENTION.md` を正とし、本書へ重複定義しない。旧 `../tuner_hal/CODE_CONVENTION.md` は旧参照実装だけの規約であり、`tuner_hal2` の現行実装規約の正本として参照しない。

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

## 12. runtime failure / capability inventory の実装境界

- service publication前のstartup validationでは、AIDL service registration不能、VINTF instance不整合、必須profile / 静的設定の解析不能、stable AIDL / service名 / init設定の自己矛盾をtyped startup failureとして確定し、明示診断を残して未公開のまま終了させる。service publication後のruntime failureをこのfail-fast経路へ流用しない。公開可否・capability意味論は`../tuner_hal/DESIGN_JA.md`、product統合条件は`INTEGRATION.md`を正とする。
- device node 不在、open不可、permission不足、probe不成立と、device存在下の runtime ioctl / read / pump failure を別の typed domain error として保持する。公開結果と状態遷移は `../tuner_hal/DESIGN_JA.md` を正とする。
- product runtime の frontend / backend inventory は、正本 capability owner が probe 成功と必要情報の確定を確認した entry だけから構成する。実体のない degraded frontend entry、診断専用 phantom entry、成功扱いの代替entryを生成しない。
- capability / resource query は正本 snapshot / ledger だけを参照し、未対応機能、存在しないresource、確保不能resourceを helper 層の成功 no-op で補償しない。
- client入力の不正、未対応、lifecycle不整合、resource unavailable、backend/internal failureを文字列または一個のgeneric errorへ早期に丸めず、`aidl_service::error_bridge`まで typed classification を保持する。

## 13. FMQ / callback / worker の失敗伝播

- FMQのcurrent implementation boundaryは、write success、short write、overflow、native write failure、EventFlag wake failureを区別する typed result を返す。write failureを0 byte成功、空queue、overflow、normal wakeへ丸めない。
- framework callback / Binder callback の戻り値を `let _ = ...`、`drop(result)`、ログだけで破棄しない。typed delivery failureを `WorkerFailureClassifier` / `PostCommitCallbackFailureTxn` の正本entryへ接続する。
- worker bodyは通常停止、停止要求、runtime failure、panic/join failureを区別できる typed terminal resultをownerへ返し、無言停止しない。terminal meaning自体は `../tuner_hal/DESIGN_JA.md` を正とする。
- worker runtime failureとpanic / join failureは別のtyped diagnostic categoryとして記録し、単一の「worker stopped」診断へ潰さない。counterを持つ場合もerror系とpanic/join系を別集計とし、診断名・counter値から公開状態を逆算しない。
- workerの待機はstop/wakeで解除可能なprimitiveを使い、client指定intervalをそのまま `thread::sleep()` してclose / Drop / shutdownを妨げない。
- generic worker生成・停止・wake・joinは `DESIGN_JA.md` の `WorkerRuntime` / `WorkerHandle` 規範アンカーへ接続する。規範owner外から `std::thread::spawn`、独自`JoinHandle` lifecycle、silent joinを追加しない。

## 14. transaction / cleanup / 非破壊最適化の実装境界

- public AIDL method、façade、worker、backend adapterが、正本transactionのvalidation / prepare / commit / rollback / cleanupを分解して別ownerとして再実装しない。正本ownerのtyped entryへ接続する。
- destructive mutationより前に検証できる条件、容量、ID / generation候補、rollback準備を完了させる。失敗し得るpreflightを旧resource破棄後へ送らない。
- critical cleanup、unregister、backend stop、token release、worker join、queue cleanupをbest-effort helperへ流して沈黙させない。失敗を直接返せない場所でもtyped diagnosticに対象と段階を残す。
- 同一条件の非破壊最適化は `../tuner_hal/DESIGN_JA.md` が許可する公開意味を変えず、正本transaction owner内で破壊的処理より前にだけ適用する。façadeやhelperが独自のsame-condition state machineを持たない。

## 15. lifetime ID / generation / token の実装規約

- lifetime ID、generation、worker signal generation、token、`startId`等の再利用禁止識別子の発行に `saturating_add()` またはwrapを許す `fetch_add()` を使用しない。
- 正本ownerは `checked_add()` 等で発行可能性を確定し、発行不能時の公開結果・局所failure / quarantineは `../tuner_hal/DESIGN_JA.md` の各契約へ接続する。wrapまたは予約値への回帰で処理継続しない。
- 0、負値、予約値を通常発行IDとして使用しない。失効・revoke後のtoken保持要否は正本key/token ownerの契約に従い、診断目的だけで復号可能なactive entryとして残さない。

## 16. backend 診断名前空間

- DVB backend の失敗を px4 専用診断record / counterへ記録せず、px4 backend の失敗を DVB 専用診断record / counterへ記録しない。
- frontend共通処理からbackend failureを記録する場合は、検証済みbackend kindをtyped contextとして渡し、対応する診断variant / namespaceだけを更新する。文字列からbackend種別を再推測しない。

## 17. source filter / packet pipeline 実装規約

- source filter relationの対応可否と公開結果は `../tuner_hal/DESIGN_JA.md` を正とし、実装は `SourceBoundaryTxn` のtyped entryを迂回しない。未対応組合せを成功no-opにしない。
- raw TS source由来packetも通常demux inputと同じ `PacketTxn` / `PacketPipeline` の検証済みpacket経路へ接続し、TEI、continuity、discontinuity、duplicate、stream / parser generation処理を別実装にしない。
- section / PES / AV / record payloadをraw TS source packetへ再解釈して別filterへ直接redispatchする経路を追加しない。公開source契約の変更が必要な場合は先に `../tuner_hal/DESIGN_JA.md` を変更する。

