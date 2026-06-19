# Tuner HAL2 コーディング規則

この文書は `tuner_hal2` 固有の実装規約を書く。公開契約、状態遷移、戻り値、capability/profile、VTS/product profile 方針、資源寿命、WorkerExit / WorkerFailureClassifier / ScanSessionTxn 論理契約、composed failure の意味は `tuner_hal2/DESIGN_JA.md` を正とする。

## 1. failure / rollback / cleanup の実装規約

- cleanup、rollback、stop、join、callback、unregister、close の失敗を `let _ =`、空分岐、ログだけ、`drop(result)` で捨てない。
- primary failure 発生後の cleanup / rollback を `?` だけで呼び、cleanup failure で primary failure を無診断で上書きしない。
- primary + cleanup failure を `format!(...)` や文字列 detail だけの generic internal error に潰さない。戻り値として片方の status を選ぶ場合でも、もう片方を composed failure または必須診断から消さない。
- `FirstErrorCollector` を primary + cleanup failure composition の代替にしない。collector は同一 cleanup phase 内の cleanup step 間 first cleanup error を集める部品としてだけ使う。
- post-allocation / post-registration failure path では、object table rollback、runtime cleanup、callback rollback、diagnostic / cleanup-failed marking を可能な限りすべて試行する。途中の `?` で後続 cleanup を飛ばさない。
- rollback / public close / owner-loss cleanup では `Option::None` / missing target を無言成功扱いしない。missing を許容する処理は read-only query、idempotent stop、best-effort telemetry、defensive unavailable path など、DESIGN_JA.md が許容した範囲に限る。
- rollback / public close / owner-loss cleanup の正本に void / best-effort-only cleanup を使わない。失敗を表面化できる戻り値を持つ operation に接続する。
- cleanup-failed marking、callback unhealthy marking、Drop leak quarantine、owner-loss cleanup failed state、scan terminal failure record は必須診断として扱い、best-effort log 扱いにしない。
- packet count、malformed count、throughput counter、補助ログなどの best-effort telemetry は primary failure を上書きしない。
- Drop は public close の代替にしない。`Drop` 実装は drop-leak 入口だけを呼び、object 種別固有 cleanup を書かない。

## 2. AIDL / service_runtime 境界の実装規約

- AIDL method body で `ensure_open()`、method planning、runtime lock、service_runtime use-case 呼び出しを手書きで組み合わせない。object_runtime façade または service_runtime use-case を通す。
- AIDL method body で fallible な request 変換、callback retain、source relation validation、unsupported / unavailable status mapping を object lifetime / generation / kind 確認より先に実行しない。
- AIDL 層から `RuntimeObjectTable`、`TunerServiceRuntime::object_table()`、`TunerServiceRuntime::object_table_mut()`、runtime registry、`service_runtime::boot/*_txn.rs`、`service_runtime::frontend_worker_txn` を直接参照しない。
- AIDL method implementation files から低レベル executor helper や `plan_object_method_dispatch` を直接呼ばない。
- service_runtime domain use-case 以外から `public_runtime_id_for_object_method` + `plan_object_method_dispatch` の組を直接扱わない。
- `best_effort` 名の callback cleanup helper を追加しない。
- production code の file split module では `use super::*;` を使わない。親 module から必要な型・関数を使う場合は `use super::{...};` で明示する。
- `#[path]` / `include!` / `include_str!` を使わない。
- `Status::new_service_specific_error()` を `aidl_service::error_bridge` 以外で直接呼ばない。
- `status_from_hal_error` / `status_from_tuner_status` / `service_error` 相当の helper を `tuner_service.rs`、object wrapper、child open helper、runtime helper へ再定義しない。
- `android.hardware.tv.tuner::Result` の整数値を、Binder status 生成目的で `error_bridge` 以外へ拡散しない。
- AIDL helper は Binder status 変換と method identity adapter に留め、object lifetime / request-builder critical section / domain state commit を所有しない。
- supported public API planning には `PublicApi` を使い、unsupported-by-design の戻り値生成には `UnsupportedPublicApi` を使う。query / open / 状態取得系を unsupported planning に流用しない。
- unavailable / unsupported / plan-only 経路で、AIDL method body に plan-only public helper や public thin wrapper を残さない。
- close method は `ObjectCloseTxn` pattern を使う。close preflight、`Closing` 遷移、domain cleanup hook、cleanup-failed marking を AIDL method body へ戻さない。
- close cleanup 系 helper は `Closing` を許すため、close preflight に使わない。通常 method、close preflight、close cleanup で lifecycle helper を混用しない。

## 3. service_runtime transaction boundary

- top-level `service_runtime/src/*_ops.rs` は public façade だけを置き、`TunerServiceRuntime` の private field を直接参照しない。
- 状態変更は `service_runtime/src/boot/*_txn.rs` の domain transaction context へ閉じる。
- flat `transact_*` helper は boot child module 内の実装詳細であり、top-level `*_ops.rs` から直接呼ばない。
- `TunerServiceRuntime::registry_mut()` を呼んでよい production code は `service_runtime/src/boot/*_txn.rs` の domain transaction implementation に限る。top-level `*_ops.rs`、AIDL 層、domain crate、`query_api.rs` から呼ばない。
- `RuntimeQuery<'a>` は read-only query 専用とし、mutable reference や mutating transaction context を持たせない。
- read-only object query は `execute_object_query_use_case()` または service_runtime query façade を通す。AIDL method body で `ensure_open()` と query 側 lifecycle check を二重化しない。
- `transaction_registry.rs` は runtime transaction -> dispatch target の正本表に限定する。production dispatch と別に第2の runtime handler / status 判定層を作らない。

## 4. wrapper 作成基準

Wrapper を置いてよい条件:

- public API 境界になる。
- domain naming を隠蔽する。
- AIDL/service_runtime から見える型境界を固定する。
- object handle based use-case 境界、typed callback retain glue、phase order を所有する use-case 境界になる。

Wrapper を置くべきでない条件:

- 名前も責務も同じ単純委譲である。
- context method と1対1で、公開境界・domain naming・型境界の意味が増えていない。
- callback rollback だけ、profile validation だけ、close helper だけを包む public thin wrapper である。
- production 未接続の bridge / slot / mapper 型を public re-export するためだけの wrapper である。

## 5. capability token / guard 実装規約

- 検証済み状態、dispatch 済み状態、transaction plan、ledger guard、rollback guard などを表す型は capability token として扱う。
- capability token は、状態検証または予約を所有する共通部品だけが発行する。外部 caller が public constructor / public enum variant / public field struct literal で偽造できる形にしない。
- 一回性 token に `Clone` / `Copy` を付けない。consume-by-value の API で消費する。
- 複数回利用可能な値が必要な場合は token とは別の read-only descriptor 型に分離する。

## 6. worker / callback / source boundary 実装規約

- frontend worker の blocking join を `TunerServiceRuntime` lock 保持中に実行しない。runtime lock 内では cancel 設定済み join ticket 取得までに限定し、join は lock 外で行う。
- worker start 成功後に fallible commit を置く場合、commit failure path で起動済み worker を stop/join し、runtime snapshot / demux snapshot rollback を試行する。
- `CallbackRegistryUpdate::Missing` は rollback / public close / owner-loss cleanup で空分岐にしない。callback store の削除対象と runtime registry clear 結果を照合する。
- callback 未登録は callback delivery failure ではない。callback store が `None` を返しただけで runtime registry unhealthy marking へ流さない。
- `IFilter.setDataSource(source)` の non-null source relation validation は、sink/source object の lifetime / generation / kind 確認後に行う。same-demux 検証と self-source 検証を lifetime check より前に置かない。
- `DemuxRuntime::set_filter_source_non_null()` は `SourceBoundaryTxn` を通す。source boundary を迂回して sink source だけを更新しない。

## 7. 静的チェックの位置づけ

- 静的チェックは規約違反候補を検出する補助確認であり、build / unit test / atest / VTS / 実機確認の代替にしない。
- 静的チェックを追加する場合は、何を検出するかを明示し、完了判定の主根拠にしない。
- テストは公開関数、戻り値、状態、診断を直接検査し、同じソースファイルの文字列検索で完了判定しない。
