# Maleicacid CAS plugin

CAS pluginの設計正本は `DESIGN_JA.md` とする。

## 現行実装

現行コードは、CAS接続境界を保持するためのプレースホルダーであり、B25/B1を含む全CAS system IDについて本番plugin / descramblerを広告しない。本番CAS復号成功を表明しない。

プレースホルダーのビルド対象は `maleicacid.tv.cas_plugin-stub-service` と、その非広告動作を確認する単体テストである。

## 目標構成

次の実装段階ではMaleicacid独自 `IMediaCasService/default` を製品経路から除き、AOSP標準MediaCasServiceがロードするC++ B25 CasPlugin shared libraryへ置換する。最初に `yakisoba_only` backendを成立させ、その後同じB25 pluginへSmartCard backendを追加する。
