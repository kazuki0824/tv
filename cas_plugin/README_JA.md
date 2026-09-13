# Maleicacid CAS plugin

CAS pluginの設計正本は `DESIGN_JA.md` とする。

## 現行境界

Maleicacid独自 `IMediaCasService/default` は持たず、AOSP標準MediaCasServiceからロードされるvendor CasPluginを実装する。B25/B1のfactory/plugin ownership、capability、session lifecycle、backend、Tuner key bridgeの規範は `DESIGN_JA.md` を正とする。

## 目標構成

`MaleicacidCasFactory` がB25/B1のCA system IDを判定し、B25では `MaleicacidB25CasPlugin`、B1では `MaleicacidB1CasPlugin` を生成する。B25は最初に `yakisoba_only` backendを成立させ、その後同じB25 pluginへSmartCard backendを追加する。B1はECM-onlyのSmartCard経路として成立させる。
