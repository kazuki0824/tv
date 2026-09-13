# Maleicacid CAS plugin

CAS pluginの設計正本は `DESIGN_JA.md` とする。

## 現行repository状態

`cas_plugin/` にはMaleicacid独自 `android.hardware.cas.IMediaCasService/default` service binary、CAS service用init rc、CAS service用VINTF fragmentを置かない。AOSP標準 `MediaCasService` と競合するdefault instanceをこのmoduleから登録しない。

現時点ではB25/B1 vendor CasPlugin shared libraryも未実装であり、このmodule自身がB25/B1 capabilityを広告するproduct artifactは存在しない。TIS/Tuner側はCAS pluginが利用可能であることを仮定しない。

## 実装構成

実装は `DESIGN_JA.md` に従い、AOSP標準 `MediaCasService` が `/vendor/lib64/mediacas` または `/vendor/lib/mediacas` からロードするC++ vendor CasPlugin shared libraryとして追加する。entry pointは `createCasFactory()` とし、最初にB25 `yakisoba_only` backendを成立させ、その後同じB25 pluginへSmartCard backendを追加する。
