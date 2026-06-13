# 予約録画 設計判断

予約録画、EIT p/f を使った追従録画、TvRecordingClient 制御は現行 product 対象外である。TIS manifest から予約サービスと receiver を外す。

現行 product では `rec/` 配下の Kotlin 実装を product package に入れない。`ReservationManagerService`、`ReservationBootReceiver`、`ReservationAlarmReceiver`、`MaleicacidRecordingSession`、`RecordingPipeline` は現行 product の起動対象ではない。

`MaleicacidRecScopeTests` は予約録画作業用の明示実行対象であり、現行 product の release確認条件へ含めない。
