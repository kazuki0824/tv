# 予約録画 設計判断

予約録画、EIT p/f を使った追従録画、TvRecordingClient 制御は 後続作業対象である。本リリース範囲では TIS manifest から予約 サービスと receiver を外す。

本リリース範囲では `rec/` 配下の Kotlin 実装を product package に入れない。`ReservationManagerService`、`ReservationBootReceiver`、`ReservationAlarmReceiver`、`MaleicacidRecordingSession`、`RecordingPipeline` は 本リリース範囲 の起動対象ではない。

`MaleicacidRecScopeTests` は 後続作業用の明示実行対象であり、本リリース範囲の release確認条件へ含めない。
