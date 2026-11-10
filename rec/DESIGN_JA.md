# 予約録画 設計判断

予約録画、追従録画、TvRecordingClient制御を製品機能として扱うrelease境界は `../開発規則.md` のr53到達点を正とする。本書はrelease scopeを再定義しない。

`rec/` のproduct package組込み、receiver/service/test moduleの有効化条件は `../tis/INTEGRATION.md` を正とする。予約録画runtimeの設計をr53で具体化する場合は、本書をそのmodule固有契約の正本として更新してから実装・統合条件を有効化する。
