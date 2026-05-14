# 予約録画 設計判断

予約録画、EIT p/f を使った追従録画、TvRecordingClient 制御は r53 対象である。r51 では TIS manifest から予約 サービスと receiver を外す。

r51 では `rec/` 配下の Kotlin 実装を product package に入れない。`ReservationManagerService`、`ReservationBootReceiver`、`ReservationAlarmReceiver`、`MaleicacidRecordingSession`、`RecordingPipeline` は r51 の起動対象ではない。

`MaleicacidRecScopeTests` は r53 作業用の明示実行対象であり、r51 の release確認条件へ含めない。
