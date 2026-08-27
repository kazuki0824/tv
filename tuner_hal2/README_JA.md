# Maleicacid Tuner HAL2

`tuner_hal2` は、LineageOS 22.1 / Android 15 の Tuner HAL service を再構成する実装である。

## 参照文書

- 設計差分: `tuner_hal2/DESIGN_JA.md`
- 実装規約: `tuner_hal2/CODE_CONVENTION.md`
- product統合手順: `tuner_hal2/INTEGRATION.md`
- 変更履歴: `tuner_hal2/CHANGELOG.md`

このREADMEは、第三者がこのディレクトリを使い始めるための入口に限定する。

## 主なディレクトリ

| ディレクトリ | 役割 |
| --- | --- |
| `common` | AIDLから独立した共通error / OS ABI |
| `device` | DVB / px4 backend adapter と frontend runtime |
| `demux` | TS parser、packet pipeline、filter/DVR runtime、AV shared memory部品 |
| `descrambler` | MULTI2 core、key token、session、PID claim |
| `lnb` | LNB runtime、apply/lifecycle/operation guard |
| `resource_ledger` | 汎用資源台帳 |
| `binder_adapter` | AIDL公開method相当入力をdomain commandへ変換する前段部品 |
| `service_runtime` | service状態、registry、object table、dispatch |
| `aidl_service` | Binder service実装 |
| `config` | product統合用makefile、BoardConfig、ueventd |
| `init` / `manifest` / `sepolicy` | 既定service登録に必要なrc、VINTF fragment、vendor policy断片 |
| `fmq` / `fmq_shim` | `system/libfmq` 接続用native shimとRust wrapper |
