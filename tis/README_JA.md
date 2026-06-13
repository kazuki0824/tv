# Maleicacid TV 入力サービス

このディレクトリは Android TV の `TvInputService` 実装を含む。Tuner HAL には Tuner SDK API 経由でアクセスする。

現行 product ではライブ視聴、scan/setup、TvProvider 反映、CAS 制御の入口を対象にする。予約録画と録画 UI は `rec` モジュールの現行 product 対象外とする。


## 公開境界の固定

TIS は Rust 由来の旧 `canonicalGenres` event フィールドや indexed JNI getter を使わない。TvProvider 標準列への Android canonical genre 投影は、TIS 側の明示写像表だけで決定する。
