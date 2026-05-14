# Maleicacid TV 入力サービス

このディレクトリは Android TV の `TvInputService` 実装を含む。Tuner HAL には Tuner SDK API 経由でアクセスする。

r51 では ライブ視聴、scan/setup、TvProvider 反映、CAS 制御 の入口を対象にする。予約録画と録画 UI は `rec` モジュールの r53 対象とする。


## r50ce 境界

TIS は Rust 由来の旧 `canonicalGenres` event フィールドや indexed JNI getter を使わない。TvProvider 標準列への Android canonical genre 投影は、TIS 側の明示写像表だけで決定する。
