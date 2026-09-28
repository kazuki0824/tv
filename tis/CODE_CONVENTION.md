# TIS 実装規約

共通の実装規約は `../GLOBAL_CODE_CONVENTION.md` を正とする。SIの実行失敗に対するTISの振る舞いは `DESIGN_JA.md` の「SI収集の期限と失敗境界」に従う。

## SIのJNI呼出し

- `NativeAribSiParser` の文字列を返す provider-data 補助 `external` 宣言は `String?` とし、戻り値は `requireNativeString` を通して受け取る。呼出し先ごとにnull検査を複製しない。
- AAC codec probe は Rust の `AacConfigurationProbeDto` を SSOT とし、SI snapshot と同じ host-only codegen で生成した Kotlin DTO を JNI から直接返す。`JSONObject` / JSON文字列 / field-name lookup を transport contract にしない。
- SI runtime snapshotは `nativeSnapshotBulkTyped(): NativeSiSnapshot?` を使用し、`JSONObject` / `JSONArray` / JSON文字列として受け取らない。nullは `JNI_OUTPUT` として失敗させる。
- `NativeSiSnapshot` と `NativeSiJvmFactory` はRust snapshotのJNI bindingであり、Kotlin側でfield集合、値域、nullable条件、enum解釈、cross-field不変条件を再検証して第二contractを作らない。
- TIS固有のpolicy投影、たとえばservice stream factとevent component factのmergeはKotlin責務としてtyped object受領後に行い、Rust SI意味解析の再実装と混同しない。
- PMT section-filter bootstrapは `nativeSnapshotPmtPidsForSectionFilters(): IntArray?` を使用し、JSON parseを行わない。
- `NativeSiException` はコンストラクターで受け取った失敗理由の識別子を `NativeSiFailureReason` へ厳密に変換する。
- `ProviderDataBridge` でJSON解析を `runCatching` により囲む場合、JNI呼出しはその外で実行し、取得した文字列だけを解析処理へ渡す。これはTvProvider永続provider-data境界の規則であり、SI runtime snapshotをJSONへ戻す根拠にしない。
