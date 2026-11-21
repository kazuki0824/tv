# arib_si_engine_rs 実装規約

共通の実装規約は `../GLOBAL_CODE_CONVENTION.md` を正とする。JNIの戻り値と失敗理由は `DESIGN_JA.md` の「実行失敗と正常な空値の区別」に従う。

## JNIのスナップショット取得

- full `snapshot_bulk_typed` と bounded `snapshot_si_collection_typed` / `try_snapshot_si_collection_typed` は共通helperで登録表から解析器参照を取得し、登録表のロックを解放してから解析器をロックする。
- 解析器ロック内ではfull経路は `build_bulk_snapshot`、bounded collection経路は `build_si_collection_snapshot` を使い、共有意味factは同じRust projectionを正本とする。JVM object生成は行わず、snapshot確定後に解析器ロックを解放する。
- JNI入口はロック解放後に full 経路では `jvm_snapshot_generated::snapshot_to_java`、bounded collection 経路では `jvm_snapshot_generated::si_collection_snapshot_to_java` を呼び、Rust DTOからsame-build codegen生成Kotlin DTOを直接構築する。Kotlin側はfull `BulkSnapshotDto`を`GeneratedSiSnapshotMapper.toDomainSnapshot()`で`NativeSiSnapshot`へ、bounded `SiCollectionSnapshotDto`を`GeneratedSiSnapshotMapper.toDomainServiceRegistrationSnapshot()`で`ServiceRegistrationSnapshot`へ機械的に投影する。production SI snapshot境界でJSON文字列化、`serde_json::to_value` によるdynamic object graph化、`JSONObject` / `JSONArray` の生成を行わない。
- field集合、意味、値域、nullable条件、enum選択、cross-field不変条件はRust側のSI意味型・snapshot構築処理をSSOTとする。JVM constructionは検証済みfactの機械的投影だけに限定する。
- PMT section-filter bootstrapのcontrol snapshotは `nativeSnapshotPmtPidsForSectionFilters()` から `IntArray` を返し、JSON文字列境界を設けない。
- AAC codec probe は `codec_probe_dto.rs` の `AacConfigurationProbeDto` / `AacAdtsConfigurationDto` / `AacProbeStatusDto` を transport SSOT とし、同じ serde-reflection / serde-generate 生成物を JVM へ直接構築する。codec probe の結果をJSON文字列化しない。

- non-blocking `try_snapshot_si_collection_typed` では `TryLockError::WouldBlock` だけを正常な未取得として返す。registry/parser poison、invalid handle、JNI変換失敗はtyped failureのまま保持し、busyへ丸めない。blocking/non-blockingの双方は同じRust意味factを投影する。

## JNIの失敗伝達

- `throw_si_failure` を共通例外入口として使用する。
- `throw_si_failure` は失敗理由の識別子と表示用メッセージを別の引数として `NativeSiException` のコンストラクターへ渡す。
- typed DTO構築、JVM collection構築、primitive boxing、array生成に失敗した場合は空object/空listへ丸めず `JNI_OUTPUT` として失敗させる。
