# Tuner HAL2 コーディング規則

この文書は `tuner_hal2` 固有の実装規約だけを書く。公開契約、状態遷移、戻り値、資源寿命、WorkerExit / WorkerFailureClassifier / ScanSessionTxn 論理契約は既存 `tuner_hal/DESIGN_JA.md` と `tuner_hal2/DESIGN_JA.md` の構造差分を正とする。

- 状態遷移、終了分類、失敗分類に自由文字列を使わない。
- 公開API相当の成功条件は、`validate -> reserve -> prepare -> apply -> commit` の各段階へ分ける。
- commit前失敗は必ずrollbackまたはquarantineへ接続する。
- cleanup、stop、join、callback、rollback の失敗を `let _ =` で捨ててはならない。
- Dropは公開closeの代替にしない。
- テストは公開関数、戻り値、状態、診断を直接検査し、同じソースファイルの文字列検索で完了判定しない。

## service_runtime transaction boundary

- top-level `service_runtime/src/*_ops.rs` は public API wrapper だけを置く。`TunerServiceRuntime` の private field を直接参照しない。
- domain transaction の意味単位は `boot/*_txn.rs` の `*Txn<'a>` context method として表現する。
- flat `transact_*` helper は boot child module 内の実装詳細であり、top-level `*_ops.rs` から直接呼ばない。
- AIDL 層から `service_runtime::frontend_worker_txn` または `boot/*_txn.rs` を直接 import しない。frontend worker 操作は service_runtime の public use-case façade を通す。
- AIDL helper から `RuntimeObjectTable` を直接参照しない。AIDL handle / generation から public runtime id や owner relation を解決する場合は service_runtime の query façade を通す。
- supported public API planning には `PublicApi` を使い、unsupported-by-design の戻り値生成には `UnsupportedPublicApi` を使う。query / open / 状態取得系を unsupported planning に流用しない。
- `RuntimeQuery<'a>` は read-only query 専用とし、mutable reference や mutating transaction context を持たせない。
- one-line wrapper を無制限に増やさない。公開 use-case 境界、domain naming 境界、型境界のいずれかを作る意味がある場合だけ追加する。
- `TunerServiceRuntime::registry_mut()` は `service_runtime/src/boot/*_txn.rs` の domain transaction implementation だけが production code で呼んでよい。top-level `service_runtime/src/*_ops.rs`、AIDL 層、domain crate、`query_api.rs` からは呼ばない。registry 変更は domain transaction context または narrow method に閉じる。
- test fixture が registry を直接組み立てる場合だけ、`registry_mut_for_test()` を使う。
- LNB runtime の値更新は `clone -> mutate -> store_lnb_runtime()` に揃える。registry slot を直接 mutable borrow してその場更新しない。
- service_runtime の LNB profile adapter は `ServiceRuntimeLnbProfileAdapter` と呼び、実 backend ではなく `LnbBackendOps` への adapter として扱う。

### wrapper creation criteria

Wrapper を置いてよい条件:

- public API 境界になる。
- domain naming を隠蔽する。
- AIDL/service_runtime から見える型境界を固定する。

Wrapper を置くべきでない条件:

- 名前も責務も同じ単純委譲である。
- context method と1対1で、公開境界・domain naming・型境界の意味が増えていない。

### static check position

- 静的チェックは規約違反候補を検出する補助確認であり、build / unit test / atest / VTS / 実機確認の代替にしない。
- 静的チェックを追加する場合は、何を検出するかを明示し、完了判定の主根拠にしない。
- 既存 wrapper は、呼び出し元の棚卸しで削減対象を判定する。crate 内だけで使う read-only query は `runtime.query().*` へ寄せ、外部公開境界として必要な wrapper だけ残す。
