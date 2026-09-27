# arib_si_engine_rs 実装規約

共通の実装規約は `../GLOBAL_CODE_CONVENTION.md` を正とする。JNIの戻り値と失敗理由は `DESIGN_JA.md` の「実行失敗と正常な空値の区別」に従う。

## JNIのスナップショット取得

- `snapshot_bulk_typed` は登録表から解析器参照を取得し、登録表のロックを解放してから解析器をロックする。
- 解析器ロック内では `build_bulk_snapshot` によりRust所有の `BulkSnapshot` を確定し、JVM object生成を行わない。snapshot確定後に解析器ロックを解放する。
- JNI入口はロック解放後に `jvm_snapshot::snapshot_to_java` を呼び、`BulkSnapshot` から `NativeSiSnapshot` と型付きSI domain objectを直接構築する。production SI snapshot境界でJSON文字列化、`serde_json::to_value` によるdynamic object graph化、`JSONObject` / `JSONArray` の生成を行わない。
- field集合、意味、値域、nullable条件、enum選択、cross-field不変条件はRust側のSI意味型・snapshot構築処理をSSOTとする。JVM constructionは検証済みfactの機械的投影だけに限定する。
- PMT section-filter bootstrapのcontrol snapshotは `nativeSnapshotPmtPidsForSectionFilters()` から `IntArray` を返し、JSON文字列境界を設けない。

## JNIの失敗伝達

- `throw_si_failure` を共通例外入口として使用する。
- `throw_si_failure` は失敗理由の識別子と表示用メッセージを別の引数として `NativeSiException` のコンストラクターへ渡す。
- typed DTO構築、JVM collection構築、primitive boxing、array生成に失敗した場合は空object/空listへ丸めず `JNI_OUTPUT` として失敗させる。
